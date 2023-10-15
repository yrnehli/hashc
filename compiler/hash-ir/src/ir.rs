//! Hash Compiler Intermediate Representation (IR) crate. This module is still
//! under construction and is subject to change.
use core::slice;
use std::{
    fmt,
    iter::{self, once},
};

use hash_ast::ast::AstNodeId;
use hash_source::{identifier::Identifier, location::Span, SourceId};
use hash_storage::{
    new_sequence_store_key_indirect,
    store::{
        statics::{SingleStoreValue, StoreId},
        LocalSequenceStore, SequenceStoreKey,
    },
};
use hash_tir::intrinsics::definitions::{
    BinOp as TirBinOp, CondBinOp as TirCondBinOp,
    ShortCircuitingBoolOp as TirShortCircuitingBoolOp, UnOp as TirUnOp,
};
use hash_utils::{
    graph::dominators::Dominators,
    index_vec::{self, IndexVec},
    smallvec::{smallvec, SmallVec},
};

pub use crate::constant::{AllocId, Const, ConstKind, Scalar};
use crate::{
    basic_blocks::BasicBlocks,
    cast::CastKind,
    ty::{AdtId, IrTy, IrTyId, Mutability, PlaceTy, RefKind, VariantIdx, COMMON_IR_TYS},
};

impl From<Const> for Operand {
    fn from(constant: Const) -> Self {
        Self::Const(constant)
    }
}

impl From<Const> for RValue {
    fn from(constant: Const) -> Self {
        Self::Use(Operand::Const(constant))
    }
}

/// A collection of operations that are constant and must run during the
/// compilation stage.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ConstOp {
    /// Yields the size of the given type.
    SizeOf,
    /// Yields the word alignment of the type.
    AlignOf,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UnaryOp {
    // Bitwise logical inversion
    BitNot,
    /// Logical inversion.
    Not,
    /// The operator '-' for negation
    Neg,
}

impl From<TirUnOp> for UnaryOp {
    fn from(value: TirUnOp) -> Self {
        use TirUnOp::*;
        match value {
            BitNot => Self::BitNot,
            Not => Self::Not,
            Neg => Self::Neg,
        }
    }
}

/// Represents a binary operation that is short-circuiting. These
/// operations are only valid on boolean values.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LogicalBinOp {
    /// '||'
    Or,
    /// '&&'
    And,
}

impl From<TirShortCircuitingBoolOp> for LogicalBinOp {
    fn from(value: TirShortCircuitingBoolOp) -> Self {
        use TirShortCircuitingBoolOp::*;

        match value {
            And => Self::And,
            Or => Self::Or,
        }
    }
}

/// Binary operations on [RValue]s that are typed as primitive, or have
/// `intrinsic` implementations defined for them. Any time that does not
/// implement these binary operations by default will create a function
/// call to the implementation of the binary operation.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BinOp {
    /// '=='
    Eq,
    /// '!='
    Neq,
    /// '|'
    BitOr,
    /// '&'
    BitAnd,
    /// '^'
    BitXor,
    /// '^^'
    Exp,
    /// '>'
    Gt,
    /// '>='
    GtEq,
    /// '<'
    Lt,
    /// '<='
    LtEq,
    /// '>>'
    Shr,
    /// '<<'
    Shl,
    /// '+'
    Add,
    /// '-'
    Sub,
    /// '*'
    Mul,
    /// '/'
    Div,
    /// '%'
    Mod,
}

impl BinOp {
    /// Returns whether the [BinOp] can be "checked".
    pub fn is_checkable(&self) -> bool {
        matches!(self, Self::Add | Self::Sub | Self::Mul | Self::Shl | Self::Shr)
    }

    /// Check if the [BinOp] is a comparator.
    pub fn is_comparator(&self) -> bool {
        matches!(self, Self::Eq | Self::Neq | Self::Gt | Self::GtEq | Self::Lt | Self::LtEq)
    }

    /// Compute the type of [BinOp] operator when applied to
    /// a particular [IrTy].
    pub fn ty(&self, lhs: IrTyId, rhs: IrTyId) -> IrTyId {
        match self {
            BinOp::BitOr
            | BinOp::BitAnd
            | BinOp::BitXor
            | BinOp::Div
            | BinOp::Sub
            | BinOp::Mod
            | BinOp::Add
            | BinOp::Mul
            | BinOp::Exp => {
                // Both `lhs` and `rhs` should be of the same type...
                debug_assert_eq!(
                    lhs, rhs,
                    "binary op types for `{:?}` should be equal, but got: lhs: `{}`, rhs: `{}`",
                    self, lhs, rhs
                );
                lhs
            }

            // Always the `lhs`, but `lhs` and `rhs` can be different types.
            BinOp::Shr | BinOp::Shl => lhs,

            // Comparisons
            BinOp::Eq | BinOp::Neq | BinOp::Gt | BinOp::GtEq | BinOp::Lt | BinOp::LtEq => {
                COMMON_IR_TYS.bool
            }
        }
    }
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Eq => write!(f, "=="),
            BinOp::Neq => write!(f, "!="),
            BinOp::BitOr => write!(f, "|"),
            BinOp::BitAnd => write!(f, "&"),
            BinOp::BitXor => write!(f, "^"),
            BinOp::Exp => write!(f, "**"),
            BinOp::Gt => write!(f, ">"),
            BinOp::GtEq => write!(f, ">="),
            BinOp::Lt => write!(f, "<"),
            BinOp::LtEq => write!(f, "<="),
            BinOp::Shr => write!(f, ">>"),
            BinOp::Shl => write!(f, "<<"),
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Mod => write!(f, "%"),
        }
    }
}

impl From<TirBinOp> for BinOp {
    fn from(value: TirBinOp) -> Self {
        use TirBinOp::*;

        match value {
            BitOr => Self::BitOr,
            BitAnd => Self::BitAnd,
            BitXor => Self::BitXor,
            Exp => Self::Exp,
            Shr => Self::Shr,
            Shl => Self::Shl,
            Add => Self::Add,
            Sub => Self::Sub,
            Mul => Self::Mul,
            Div => Self::Div,
            Mod => Self::Mod,
        }
    }
}

impl From<TirCondBinOp> for BinOp {
    fn from(value: TirCondBinOp) -> Self {
        use TirCondBinOp::*;

        match value {
            EqEq => Self::Eq,
            NotEq => Self::Neq,
            Gt => Self::Gt,
            GtEq => Self::GtEq,
            Lt => Self::Lt,
            LtEq => Self::LtEq,
        }
    }
}

/// Describes what kind of [Local] something is, whether it
/// is generated by the compiler, or a user variable.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LocalKind {
    /// The return local place of a function.
    Return,

    /// An argument to the function body.
    Arg,

    /// A local variable that is defined within the function body
    /// by the user.
    Var,

    /// A local variable that is generated as a temporary variable
    /// during the lowering process.
    Temp,
}

/// Essentially a register for a value, the local declaration
/// is used to store some data within the function body, it contains
/// an associated [Mutability], and [IrTy], as well as a name if the
/// information is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalDecl {
    /// Mutability of the local.
    pub mutability: Mutability,

    /// The type of the local.
    pub ty: IrTyId,

    /// An optional name for the local, this is used for building the
    /// IR and for printing the IR (in order to label which local associates
    /// with which variable and scope).
    pub name: Option<Identifier>,

    /// Whether the local declaration is an auxiliary. An auxiliary local
    /// declaration is used to store a temporary result of an operation that
    /// is used to store the result of expressions that return **nothing**,
    /// or temporary variables that are needed during the lowering process to
    /// lower edge case expressions. Auxiliary local declarations will be
    /// eliminated during the lowering process, when the IR undergoes
    /// optimisations.
    auxiliary: bool,
}

impl LocalDecl {
    /// Create a new [LocalDecl].
    pub fn new(name: Identifier, mutability: Mutability, ty: IrTyId) -> Self {
        Self { mutability, ty, name: Some(name), auxiliary: false }
    }

    /// Create a new mutable [LocalDecl].
    pub fn new_mutable(name: Identifier, ty: IrTyId) -> Self {
        Self::new(name, Mutability::Mutable, ty)
    }

    /// Create a new immutable [LocalDecl].
    pub fn new_immutable(name: Identifier, ty: IrTyId) -> Self {
        Self::new(name, Mutability::Immutable, ty)
    }

    pub fn new_auxiliary(ty: IrTyId, mutability: Mutability) -> Self {
        Self { mutability, ty, name: None, auxiliary: true }
    }

    /// Returns the [IrTyId] of the local.
    pub fn ty(&self) -> IrTyId {
        self.ty
    }

    /// Returns the [Mutability] of the local.
    pub fn mutability(&self) -> Mutability {
        self.mutability
    }

    /// Returns the name of the local.
    pub fn name(&self) -> Option<Identifier> {
        self.name
    }

    /// Is the [Local] an auxiliary?
    pub fn auxiliary(&self) -> bool {
        self.auxiliary
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PlaceProjection {
    /// When we want to narrow down the union type to some specific
    /// variant.
    Downcast(VariantIdx),
    /// A reference to a specific field within the place, at this stage they
    /// are represented as indexes into the field store of the place type.
    Field(usize),
    /// Take the index of some specific place, the index does not need to be
    /// constant
    Index(Local),

    /// This kind of index is used when slice patterns are specified, we know
    /// the exact location of the offset that this is referencing. Here are
    /// some examples of where the element `A` is referenced:
    /// ```ignore
    /// [A, _, .., _, _] => { offset: 0, min_length: 4, from_end: false }
    /// [_, _, .., _, A] => { offset: 0, min_length: 4, from_end: true }
    /// [_, _, .., A, _] => { offset: 1, min_length: 4, from_end: true }
    /// [_, A, .., _, _] => { offset: 1, min_length: 4, from_end: false }
    /// ```
    ConstantIndex {
        /// The index of the constant index.
        offset: usize,

        /// If the index is referencing from the end of the slice.
        from_end: bool,

        /// The minimum length of the slice that this is referencing.
        min_length: usize,
    },

    /// A sub-slice projection references a sub-slice of the original slice.
    /// This is generated from slice patterns that associate a sub-slice with
    /// a variable, for example:
    /// ```ignore
    /// [_, _, ...x, _]
    /// [_, ...x, _, _]
    /// ```
    SubSlice {
        /// The initial offset of where the slice is referencing
        /// from.
        from: usize,

        /// To which index the slice is referencing to.
        to: usize,

        /// If this is referring to from the end of a slice. This determines
        /// from where `to` counts from, whether the start or the end of the
        /// slice/list.
        from_end: bool,
    },

    /// We want to dereference the place
    Deref,
}

/// A [Place] describes a memory location that is currently
/// within the function of the body backed by a [Local].
///
/// Additionally, [Place]s allow for projections to be applied
/// to a place in order to specify a location within the [Local],
/// i.e. an array index, a field access, etc.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Place {
    /// The original place of where this is referring to.
    pub local: Local,

    /// Any projections that are applied onto the `local` in
    /// order to specify an exact location within the local.
    pub projections: ProjectionId,
}

impl Place {
    /// Create a [Place] that points to the return `place` of a lowered  body.
    pub fn return_place() -> Self {
        Self { local: RETURN_PLACE, projections: ProjectionId::empty() }
    }

    /// Deduce the type of the [Place] from the [IrCtx] and the local
    /// declarations.
    pub fn ty(&self, info: &BodyInfo) -> IrTyId {
        PlaceTy::from_place(*self, info).ty
    }

    /// Create a new [Place] from a [Local] with no projections.
    pub fn from_local(local: Local) -> Self {
        Self { local, projections: ProjectionId::empty() }
    }

    /// Create a new [Place] from an existing [Place] whilst also
    /// applying a [`PlaceProjection::Deref`] on the old one.
    pub fn deref(&self, store: &mut Projections) -> Self {
        // @@Todo: how can we just amend the existing projections?
        let projections = store.get_vec(self.projections);

        Self {
            local: self.local,
            projections: store
                .create_from_iter(projections.iter().copied().chain(once(PlaceProjection::Deref))),
        }
    }

    /// Create a new [Place] from an existing place whilst also
    /// applying a a [PlaceProjection::Field] on the old one.
    pub fn field(&self, field: usize, store: &mut Projections) -> Self {
        let projections = store.get_vec(self.projections);

        Self {
            local: self.local,
            projections: store.create_from_iter(
                projections.iter().copied().chain(once(PlaceProjection::Field(field))),
            ),
        }
    }

    /// Examine a [Place] as a [Local] with the condition that the
    /// [Place] has no projections.
    pub fn as_local(&self) -> Option<Local> {
        if self.projections.is_empty() {
            Some(self.local)
        } else {
            None
        }
    }
}

impl From<Place> for Operand {
    fn from(value: Place) -> Self {
        Self::Place(value)
    }
}

impl From<Place> for RValue {
    fn from(value: Place) -> Self {
        Self::Use(Operand::Place(value))
    }
}

/// [AggregateKind] represent an initialisation process of a particular
/// structure be it a tuple, array, struct, etc.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AggregateKind {
    /// A tuple value initialisation.
    Tuple(AdtId),

    /// An array aggregate kind initialisation. The type of the array
    /// is stored here. Additionally, the length of the array is recorded
    /// in [`IrTy::Array`] data, and can be derived from the type.
    ///
    /// N.B. This type is the type of the array, not the type of the
    /// elements within the array.
    Array(IrTyId),

    /// Enum aggregate kind, this is used to represent an initialisation
    /// of an enum variant with the specified variant index.
    Enum(AdtId, VariantIdx),

    /// Struct aggregate kind, this is used to represent a struct
    /// initialisation.
    Struct(AdtId),
}

impl AggregateKind {
    /// Check if the [AggregateKind] represents an ADT.
    pub fn is_adt(&self) -> bool {
        !matches!(self, AggregateKind::Array(_))
    }

    /// Get the [AdtId] of the [AggregateKind] if it is an ADT.
    ///
    /// N.B. This will panic if the [AggregateKind] is not an ADT.
    pub fn adt_id(&self) -> AdtId {
        match self {
            AggregateKind::Tuple(id) | AggregateKind::Enum(id, _) | AggregateKind::Struct(id) => {
                *id
            }
            AggregateKind::Array(_) => panic!("cannot get adt_id of non-adt aggregate kind"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum Operand {
    /// A constant value.
    Const(Const),

    /// A place that is being used.
    Place(Place),
}

impl Operand {
    /// Compute the type of the [Operand] based on
    /// the IrCtx.
    pub fn ty(&self, info: &BodyInfo) -> IrTyId {
        match self {
            Operand::Const(kind) => kind.ty(),
            Operand::Place(place) => place.ty(info),
        }
    }
}

impl From<Operand> for RValue {
    fn from(value: Operand) -> Self {
        Self::Use(value)
    }
}

/// The representation of values that occur on the right-hand side of an
/// assignment.
#[derive(Debug, PartialEq, Clone)]
pub enum RValue {
    /// Some value that is being used. Could be a constant or a place.
    Use(Operand),

    /// Compiler intrinsic operation, this will be computed in place and
    /// replaced by a constant.
    ///
    /// @@Future: maybe in the future this should be replaced by a compile-time
    /// API variant which will just run some kind of operation and return the
    /// constant.
    ConstOp(ConstOp, IrTyId),

    /// A unary expression with a unary operator.
    UnaryOp(UnaryOp, Operand),

    /// A binary expression with a binary operator and two inner expressions.
    BinaryOp(BinOp, Box<(Operand, Operand)>),

    /// A binary expression that is checked. The only difference between this
    /// and a normal [RValue::BinaryOp] is that this will return a boolean and
    /// the result of the operation in the form of `(T, bool)`. The boolean
    /// flag denotes whether the operation violated the check...
    CheckedBinaryOp(BinOp, Box<(Operand, Operand)>),

    /// A cast operation, this will convert the value of the operand to the
    /// specified type.
    Cast(CastKind, Operand, IrTyId),

    /// Compute the `length` of a place, yielding a `usize`.
    ///
    /// Any `place` that is not an array or slice, is not a valid [RValue].
    Len(Place),

    /// An expression which is taking the address of another expression with an
    /// mutability modifier e.g. `&mut x`.
    Ref(Mutability, Place, RefKind),

    /// Used for initialising structs, tuples and other aggregate
    /// data structures
    Aggregate(AggregateKind, Vec<Operand>),

    /// An array aggregate which is used to initialise an array with a repeated
    /// operand, this originates from the initial repeat expression: `[x; 5]`.
    Repeat(Operand, usize),

    /// Compute the discriminant of a [Place], this is essentially checking
    /// which variant a union is. For types that don't have a discriminant
    /// (non-union types ) this will return the value as 0.
    Discriminant(Place),
}

impl RValue {
    /// Check if an [RValue] is a constant.
    pub fn is_const(&self) -> bool {
        matches!(self, RValue::Use(Operand::Const(_)))
    }

    /// Convert the RValue into a constant, having previously
    /// checked that it is a constant.
    pub fn as_const(&self) -> Const {
        match self {
            RValue::Use(Operand::Const(c)) => *c,
            rvalue => unreachable!("Expected a constant, got {:?}", rvalue),
        }
    }

    /// Get the [IrTy] of the [RValue].
    pub fn ty(&self, info: &BodyInfo) -> IrTyId {
        match self {
            RValue::Use(operand) => operand.ty(info),
            RValue::ConstOp(ConstOp::AlignOf | ConstOp::SizeOf, _) => COMMON_IR_TYS.usize,
            RValue::UnaryOp(_, operand) => operand.ty(info),
            RValue::BinaryOp(op, box (lhs, rhs)) => op.ty(lhs.ty(info), rhs.ty(info)),
            RValue::CheckedBinaryOp(op, box (lhs, rhs)) => {
                let ty = op.ty(lhs.ty(info), rhs.ty(info));
                IrTy::make_tuple(&[ty, COMMON_IR_TYS.bool])
            }
            RValue::Cast(_, _, ty) => *ty,
            RValue::Len(_) => COMMON_IR_TYS.usize,
            RValue::Ref(mutability, place, kind) => {
                let ty = place.ty(info);
                IrTy::create(IrTy::Ref(ty, *mutability, *kind))
            }
            RValue::Aggregate(kind, _) => match kind {
                AggregateKind::Enum(id, _)
                | AggregateKind::Struct(id)
                | AggregateKind::Tuple(id) => IrTy::create(IrTy::Adt(*id)),
                AggregateKind::Array(ty) => *ty,
            },
            RValue::Discriminant(place) => {
                let ty = place.ty(info);
                ty.borrow().discriminant_ty()
            }
            RValue::Repeat(op, length) => {
                IrTy::create(IrTy::Array { ty: op.ty(info), length: *length })
            }
        }
    }
}

/// A defined statement within the IR
#[derive(Debug, PartialEq, Clone)]
pub enum StatementKind {
    /// Filler kind when expressions are optimised out or removed for other
    /// reasons.
    Nop,

    /// An assignment expression, a right hand-side expression is assigned to a
    /// left hand-side pattern e.g. `x = 2`
    Assign(Place, RValue),

    /// Set the discriminant on a particular place, this is used to concretely
    /// specify what the discriminant of a particular enum/union type is.
    Discriminate(Place, VariantIdx),

    /// A statement which is used to denote that a [Local] is now "live"
    /// in terms of the live interval.
    Live(Local),

    /// A statement which is used to denote that a [Local] is now "dead"
    /// in terms of live interval.
    Dead(Local),
}

/// A [Statement] is a intermediate transformation step within a [BasicBlock].
#[derive(Debug, PartialEq, Clone)]
pub struct Statement {
    /// The kind of [Statement] that it is.
    pub kind: StatementKind,

    /// The location of the statement. This is mostly used for error reporting
    /// or generating debug information at later stages of lowering
    /// beyond the IR.
    pub origin: AstNodeId,
}

/// The kind of assert terminator that it is.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AssertKind {
    /// A Division by zero assertion.
    DivisionByZero { operand: Operand },

    /// Occurs when an attempt to take the remainder of some operand with zero.
    RemainderByZero { operand: Operand },

    /// Performing an arithmetic operation has caused the operation to overflow
    Overflow {
        /// The operation that is being performed.
        op: BinOp,

        /// The left hand-side operand in the operation.
        lhs: Operand,

        /// The right hand-side operand in the operation.
        rhs: Operand,
    },

    /// Performing an arithmetic operation has caused the operation to overflow
    /// whilst subtracting or terms that are signed
    NegativeOverflow { operand: Operand },

    /// Bounds check assertion.
    BoundsCheck {
        /// The length of the array that is being checked.
        len: Operand,

        /// The index that is being checked.
        index: Operand,
    },
}

impl AssertKind {
    /// Get a general message of what the [AssertKind] is
    /// checking. This is used to generate a readable message
    /// within the executable for when the assert is triggered.
    pub fn message(&self) -> &'static str {
        match self {
            AssertKind::Overflow { op: BinOp::Add, .. } => "attempt to add with overflow\n",
            AssertKind::Overflow { op: BinOp::Sub, .. } => "attempt to subtract with overflow\n",
            AssertKind::Overflow { op: BinOp::Mul, .. } => "attempt to multiply with overflow\n",
            AssertKind::Overflow { op: BinOp::Div, .. } => "attempt to divide with overflow\n",
            AssertKind::Overflow { op: BinOp::Mod, .. } => {
                "attempt to calculate the remainder with overflow"
            }
            AssertKind::Overflow { op: BinOp::Shl, .. } => "attempt to shift left with overflow\n",
            AssertKind::Overflow { op: BinOp::Shr, .. } => "attempt to shift right with overflow\n",
            AssertKind::Overflow { op, .. } => panic!("unexpected overflow operator `{op}`\n"),
            AssertKind::DivisionByZero { .. } => "attempt to divide by zero\n",
            AssertKind::RemainderByZero { .. } => {
                "attempt to take remainder with a divisor of zero\n"
            }
            AssertKind::NegativeOverflow { .. } => "attempt to negate with overflow\n",
            AssertKind::BoundsCheck { .. } => "attempt to index array out of bounds\n",
        }
    }
}

/// [Terminator] statements are those that affect control
/// flow. All [BasicBlock]s must be terminated with a
/// [Terminator] statement that instructs where the program
/// flow is to go next.
#[derive(Debug, PartialEq)]
pub struct Terminator {
    /// The kind of [Terminator] that it is.
    pub kind: TerminatorKind,

    /// The source location of the terminator. This is mostly used for error
    /// reporting or generating debug information at later stages of
    /// lowering beyond the IR.
    pub origin: AstNodeId,
}

pub type Successors<'a> = impl Iterator<Item = BasicBlock> + 'a;

pub type SuccessorsMut<'a> =
    iter::Chain<std::option::IntoIter<&'a mut BasicBlock>, slice::IterMut<'a, BasicBlock>>;

impl Terminator {
    /// Get all of the successors of a [Terminator].
    pub fn successors(&self) -> Successors<'_> {
        match self.kind {
            TerminatorKind::Goto(target)
            | TerminatorKind::Call { target: Some(target), .. }
            | TerminatorKind::Assert { target, .. } => {
                Some(target).into_iter().chain([].iter().copied())
            }
            TerminatorKind::Switch { ref targets, .. } => {
                targets.otherwise.into_iter().chain(targets.targets.iter().copied())
            }
            _ => None.into_iter().chain([].iter().copied()),
        }
    }

    /// Get all of the successors of a [Terminator] as mutable references.
    pub fn successors_mut(&mut self) -> SuccessorsMut<'_> {
        match self.kind {
            TerminatorKind::Goto(ref mut target)
            | TerminatorKind::Call { target: Some(ref mut target), .. }
            | TerminatorKind::Assert { ref mut target, .. } => {
                Some(target).into_iter().chain(&mut [])
            }
            TerminatorKind::Switch { ref mut targets, .. } => {
                targets.otherwise.as_mut().into_iter().chain(targets.targets.iter_mut())
            }
            _ => None.into_iter().chain(&mut []),
        }
    }

    /// Function that replaces a specified successor with another
    /// [BasicBlock].
    pub fn replace_edge(&mut self, successor: BasicBlock, replacement: BasicBlock) {
        match self.kind {
            TerminatorKind::Goto(target) if target == successor => {
                self.kind = TerminatorKind::Goto(replacement)
            }
            TerminatorKind::Switch { ref mut targets, .. } => {
                targets.replace_edge(successor, replacement)
            }
            TerminatorKind::Call { target: Some(ref mut target), .. } if *target == successor => {
                *target = replacement;
            }
            TerminatorKind::Assert { ref mut target, .. } => {
                *target = replacement;
            }
            // All other edges cannot be replaced
            _ => {}
        }
    }
}

/// Struct that represents all of the targets that a [TerminatorKind::Switch]
/// can jump to. This also defines some useful methods on the block to iterate
/// over all the targets, etc.
#[derive(Debug, PartialEq, Eq)]
pub struct SwitchTargets {
    /// The values are stored as an [u128] because we only deal with **small**
    /// integral types, for larger integer values, we default to using `Eq`
    /// check. Since the value is stored as an [u128], this is nonsensical
    /// when it comes using these values, which is why a **bias** needs to
    /// be applied before properly reading the value which can be derived
    /// from the integral type that is being matched on.
    ///
    /// N.B. All values within the table are unique, there cannot be multiple
    /// targets for the same value.
    ///
    /// @@Todo: It would be nice to have a unified `table`, but ~~fucking~~
    /// iterators!
    pub values: SmallVec<[u128; 1]>,

    /// The jump table, contains corresponding values to *jump* on and the
    /// location of where the jump goes to. The index within `values` is the
    /// relative jump location that is used when performing the jump.
    pub targets: SmallVec<[BasicBlock; 1]>,

    /// If none of the corresponding values match, then jump to this block. This
    /// is set to [None] if the switch is exhaustive.
    pub otherwise: Option<BasicBlock>,
}

impl SwitchTargets {
    /// Create a new [SwitchTargets] with the specified jump table and
    /// an optional otherwise block.
    pub fn new(
        targets: impl Iterator<Item = (u128, BasicBlock)>,
        otherwise: Option<BasicBlock>,
    ) -> Self {
        let (values, targets): (SmallVec<[_; 1]>, SmallVec<[_; 1]>) = targets.unzip();

        Self { values, targets, otherwise }
    }

    /// Check if there is an `otherwise` block.
    pub fn has_otherwise(&self) -> bool {
        self.otherwise.is_some()
    }

    /// Get the `otherwise` block to jump to unconditionally.
    pub fn otherwise(&self) -> BasicBlock {
        self.otherwise.unwrap()
    }

    /// Iterate all of the associated targets.
    pub fn iter_targets(&self) -> impl Iterator<Item = BasicBlock> + '_ {
        self.otherwise.into_iter().chain(self.targets.iter().copied())
    }

    /// Replace a successor with another [BasicBlock].
    pub fn replace_edge(&mut self, successor: BasicBlock, replacement: BasicBlock) {
        for target in self.targets.iter_mut() {
            if *target == successor {
                *target = replacement;
            }
        }

        if let Some(otherwise) = self.otherwise {
            if otherwise == successor {
                self.otherwise = Some(replacement);
            }
        }
    }

    pub fn iter(&self) -> SwitchTargetsIter<'_> {
        SwitchTargetsIter { inner: iter::zip(&self.values, &self.targets) }
    }

    /// Find the target for a specific value, if it exists.
    pub fn corresponding_target(&self, value: u128) -> BasicBlock {
        self.values
            .iter()
            .position(|v| *v == value)
            .map(|pos| self.targets[pos])
            .unwrap_or_else(|| self.otherwise())
    }
}

pub struct SwitchTargetsIter<'a> {
    inner: iter::Zip<slice::Iter<'a, u128>, slice::Iter<'a, BasicBlock>>,
}

impl<'a> Iterator for SwitchTargetsIter<'a> {
    type Item = (u128, BasicBlock);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(val, bb)| (*val, *bb))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for SwitchTargetsIter<'_> {}

/// The kind of [Terminator] that it is.
///
/// @@Future: does this need an `Intrinsic(...)` variant for substituting
/// expressions for intrinsic functions?
#[derive(Debug, PartialEq)]
pub enum TerminatorKind {
    /// A simple go to block directive.
    Goto(BasicBlock),

    /// Return from the parent function
    Return,

    /// Perform a function call
    Call {
        /// The function that is being called
        op: Operand,

        /// Arguments to the function, later we might need to distinguish
        /// whether these are move or copy arguments.
        args: Vec<Operand>,

        /// Destination of the result...
        destination: Place,

        /// Where to return after completing the call
        target: Option<BasicBlock>,
    },

    /// Denotes that this terminator should never be reached, doing so will
    /// break IR control flow invariants.
    Unreachable,

    /// Essentially a `jump if <0> to <1> else go to <2>`. The last argument is
    /// the `otherwise` condition.
    Switch {
        /// The value to use when comparing against the cases.
        value: Operand,

        /// All of the targets that are defined for the particular switch.
        targets: SwitchTargets,
    },

    /// This terminator is used to verify that the result of some operation has
    /// no violated a some condition. Usually, this is combined with operations
    /// that perform a `checked` operation and sets some flag in the form of a
    /// [Operand] and expects it to be equal to the `expected` boolean value.
    Assert {
        /// The condition that is to be checked against the `expected value
        condition: Operand,
        /// What the assert terminator expects the `condition` to be
        expected: bool,
        /// What condition is the assert verifying that it holds
        kind: Box<AssertKind>,
        /// If the `condition` was verified, this is where the program should
        /// continue to.
        target: BasicBlock,
    },
}

impl TerminatorKind {
    /// Utility to create a [TerminatorKind::Switch] which emulates the
    /// behaviour of an `if` branch where the `true` branch is the
    /// `true_block` and the `false` branch is the `false_block`.
    pub fn make_if(value: Operand, true_block: BasicBlock, false_block: BasicBlock) -> Self {
        let targets =
            SwitchTargets::new(std::iter::once((false.into(), false_block)), Some(true_block));

        TerminatorKind::Switch { value, targets }
    }
}

/// The contents of a [BasicBlock], the statements of the block, and a
/// terminator. Initially, the `terminator` begins as [None], and will
/// be set when the lowering process is completed.
///
/// N.B. It is an invariant for a [BasicBlock] to not have a terminator
/// once it has been built.
#[derive(Debug, PartialEq)]
pub struct BasicBlockData {
    /// The statements that the block has.
    pub statements: Vec<Statement>,
    /// An optional terminating statement, where the block goes
    /// after finishing execution of these statements. When a
    /// [BasicBlock] is finalised, it must always have a terminator.
    pub terminator: Option<Terminator>,
}

impl BasicBlockData {
    /// Create a new [BasicBlockData] with no statements and a provided
    /// `terminator`. It is assumed that the statements are to be added
    /// later to the block.
    pub fn new(terminator: Option<Terminator>) -> Self {
        Self { statements: vec![], terminator }
    }

    /// Get a reference to the terminator of this [BasicBlockData].
    pub fn terminator(&self) -> &Terminator {
        self.terminator.as_ref().expect("expected terminator on block")
    }

    /// Get a mutable reference to the terminator of this [BasicBlockData].
    pub fn terminator_mut(&mut self) -> &mut Terminator {
        self.terminator.as_mut().expect("expected terminator on block")
    }

    /// Return a list of all of the successors of this [BasicBlock].
    pub fn successors(&self) -> SmallVec<[BasicBlock; 4]> {
        match &self.terminator {
            Some(terminator) => terminator.successors().collect(),
            None => smallvec![],
        }
    }

    /// Check if the [BasicBlockData] is empty, i.e. has no statements and
    /// the terminator is of kind [TerminatorKind::Unreachable].
    pub fn is_empty_and_unreachable(&self) -> bool {
        self.statements.is_empty()
            && self.terminator.as_ref().map_or(false, |t| t.kind == TerminatorKind::Unreachable)
    }
}

index_vec::define_index_type! {
    /// Index for [BasicBlockData] stores within generated [Body]s.
    pub struct BasicBlock = u32;

    MAX_INDEX = u32::max_value() as usize;
    DISABLE_MAX_INDEX_CHECK = cfg!(not(debug_assertions));

    DEBUG_FORMAT = "bb{}";
}

/// `0` is used as the starting block of any lowered body.
pub const START_BLOCK: BasicBlock = BasicBlock { _raw: 0 };

impl BasicBlock {
    /// Create an [IrRef] to the start of this [BasicBlock].
    pub fn ref_to_start(self) -> IrRef {
        IrRef { block: self, index: 0 }
    }
}

index_vec::define_index_type! {
    /// Index for [LocalDecl] stores within generated [Body]s.
    pub struct Local = u32;

    MAX_INDEX = u32::max_value() as usize;
    DISABLE_MAX_INDEX_CHECK = cfg!(not(debug_assertions));

    DEBUG_FORMAT = "_{}";
}

/// `0` is used as the return place of any lowered body.
pub const RETURN_PLACE: Local = Local { _raw: 0 };

/// The origin of a lowered function body.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BodySource {
    /// Constant block
    Const,
    /// The item is a normal function.
    Item,
}

impl fmt::Display for BodySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BodySource::Const => write!(f, "constant block"),
            BodySource::Item => write!(f, "function"),
        }
    }
}

/// All of the [LocalDecl]s that are used within a [Body].
pub type LocalDecls = IndexVec<Local, LocalDecl>;

/// All of the [PlaceProjection]s that are used within a [Body].
pub type Projections = LocalSequenceStore<ProjectionId, PlaceProjection>;

/// Represents a lowered IR body, which stores the created declarations,
/// blocks and various other metadata about the lowered body.
pub struct Body {
    /// The blocks that the function is represented with
    pub basic_blocks: BasicBlocks,

    /// Declarations of local variables:
    ///
    /// - The first local is used a representation of the function return value
    ///   if any.
    ///
    /// - the next `arg_count` locals are used to represent the assigning of
    ///   function arguments.
    ///
    /// - the remaining are temporaries that are used within the function.
    pub locals: LocalDecls,

    /// The interned projections that are used within the body.
    pub projections: Projections,

    /// Information that is derived when the body in being
    /// lowered.
    pub meta: BodyMetadata,

    /// Number of arguments to the function
    pub arg_count: usize,

    /// The location of the function
    origin: AstNodeId,

    /// Whether the IR Body that is generated should be printed
    /// when the generation process is finalised.
    dump: bool,
}

impl Body {
    /// Create a new [Body] with the given `name`, `arg_count`, `source_id` and
    /// `span`.
    pub fn new(
        blocks: IndexVec<BasicBlock, BasicBlockData>,
        locals: LocalDecls,
        projections: Projections,
        info: BodyMetadata,
        arg_count: usize,
        origin: AstNodeId,
    ) -> Self {
        Self {
            basic_blocks: BasicBlocks::new(blocks),
            meta: info,
            projections,
            locals,
            arg_count,
            origin,
            dump: false,
        }
    }

    /// Get a reference to the stored basic blocks of this
    /// [Body].
    pub fn blocks(&self) -> &IndexVec<BasicBlock, BasicBlockData> {
        &self.basic_blocks.blocks
    }

    /// Get a reference to the stored [Projections] of this [Body].
    pub fn projections(&self) -> &Projections {
        &self.projections
    }

    /// Get a mutable reference to the stored [Projections] of this [Body].
    pub fn projections_mut(&mut self) -> &mut Projections {
        &mut self.projections
    }

    /// Compute the [LocalKind] of a [Local] in this [Body].
    pub fn local_kind(&self, local: Local) -> LocalKind {
        if local == RETURN_PLACE {
            LocalKind::Return
        } else if local.index() < self.arg_count + 1 {
            LocalKind::Arg
        } else if self.locals[local].auxiliary || self.locals[local].name.is_none() {
            LocalKind::Temp
        } else {
            LocalKind::Var
        }
    }

    /// Returns an iterator over all function arguments.
    #[inline]
    pub fn args_iter(&self) -> impl ExactSizeIterator<Item = Local> {
        (1..self.arg_count + 1).map(Local::new)
    }

    /// Returns an iterator over all variables and temporaries. This
    /// excludes the return place and function arguments.
    #[inline]
    pub fn vars_iter(&self) -> impl ExactSizeIterator<Item = Local> {
        (self.arg_count + 1..self.locals.len()).map(Local::new)
    }

    /// Set the `dump` flag to `true` so that the IR Body that is generated
    /// will be printed when the generation process is finalised.
    pub fn mark_to_dump(&mut self) {
        self.dump = true;
    }

    /// Check if the [Body] needs to be dumped.
    pub fn needs_dumping(&self) -> bool {
        self.dump
    }

    /// Get the [BodyMetadata] for the [Body].
    pub fn metadata(&self) -> &BodyMetadata {
        &self.meta
    }

    /// Get the auxiliary stores for the [Body].
    pub fn aux(&self) -> BodyInfo<'_> {
        BodyInfo { locals: &self.locals, projections: &self.projections }
    }

    /// Get a mutable reference to the auxiliary stores for the [Body].
    pub fn aux_mut(&mut self) -> BodyInfoMut<'_> {
        BodyInfoMut { locals: &mut self.locals, projections: &mut self.projections }
    }

    /// Get the [Span] of the [Body].
    pub fn span(&self) -> Span {
        self.origin.span()
    }

    /// Get the [SourceId] of the [Body].
    pub fn source(&self) -> SourceId {
        self.origin.source()
    }
}

/// This struct contains additional metadata about the body that was lowered,
/// namely the associated name with the body that is derived from the
/// declaration that it was lowered from, the type of the body that is computed
/// during lowering, etc.
///
/// This type exists since it is expected that this information is constructed
/// during lowering and might not be initially available, so most of the fields
/// are wrapped in a [Option], however any access method on the field
/// **expects** that the value was computed.
pub struct BodyMetadata {
    /// The name of the body that was lowered. This is determined from the
    /// beginning of the lowering process.
    pub name: Identifier,

    /// The source of the body that was lowered, either an item, or a constant.
    pub source: BodySource,

    /// The type of the body that was lowered
    ty: Option<IrTyId>,
}

impl BodyMetadata {
    /// Create a new [BodyMetadata] with the given `name`.
    pub fn new(name: Identifier, source: BodySource) -> Self {
        Self { name, ty: None, source }
    }

    /// Set the type of the body that was lowered.
    pub fn set_ty(&mut self, ty: IrTyId) {
        self.ty = Some(ty);
    }

    /// Get the type of the body that was lowered.
    pub fn ty(&self) -> IrTyId {
        self.ty.expect("body type was not computed")
    }

    /// Get the name of the body that was lowered.
    pub fn name(&self) -> Identifier {
        self.name
    }

    /// Get the [BodySource] for [Body] that was lowered.
    pub fn source(&self) -> BodySource {
        self.source
    }
}

/// All of the auxiliary stores that are used within a [Body]. This is useful
/// for other functions that might need access to this information when reading
/// items within the [Body].
#[derive(Clone, Copy)]
pub struct BodyInfo<'body> {
    /// A reference to the local storage of the [Body].
    pub locals: &'body LocalDecls,

    /// A reference to the projection storage of the [Body].
    pub projections: &'body Projections,
}

pub struct BodyInfoMut<'body> {
    /// A reference to the local storage of the [Body].
    pub locals: &'body mut LocalDecls,

    /// A reference to the projection storage of the [Body].
    pub projections: &'body mut Projections,
}

new_sequence_store_key_indirect!(pub ProjectionId, PlaceProjection, derives=Debug);

/// An [IrRef] is a reference to where a particular item occurs within
/// the [Body]. The [IrRef] stores an associated [BasicBlock] and an
/// index into the statements of that [BasicBlock].
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct IrRef {
    /// The block that the reference points to.
    pub block: BasicBlock,

    /// The nth statement that the reference points to.
    pub index: usize,
}

impl Default for IrRef {
    fn default() -> Self {
        Self { block: START_BLOCK, index: Default::default() }
    }
}

impl fmt::Debug for IrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}[{}]", self.block, self.index)
    }
}

impl IrRef {
    /// Create a new [IrRef] with the given `block` and `index`.
    pub fn new(block: BasicBlock, index: usize) -> Self {
        Self { block, index }
    }

    /// Check if this [IrRef] dominates the given `other` [IrRef]
    /// with the set of dominators. If the two [IrRef]s are in the
    /// same block, then the index of the [IrRef] is checked to see
    /// if it is less than or equal to the other [IrRef].
    pub fn dominates(&self, other: IrRef, dominators: &Dominators<BasicBlock>) -> bool {
        if self.block == other.block {
            self.index <= other.index
        } else {
            dominators.is_dominated_by(self.block, other.block)
        }
    }
}

#[cfg(all(target_arch = "x86_64", target_pointer_width = "64"))]
mod size_asserts {
    use hash_utils::assert::static_assert_size;

    use super::*;

    static_assert_size!(Statement, 72);
    static_assert_size!(Terminator, 104);
    static_assert_size!(RValue, 48);
}
