//! Contains type definitions that the rest of the storage and the general typechecker use.
use hash_source::{identifier::Identifier, SourceId};
use slotmap::new_key_type;
use std::collections::HashMap;

/// The visibility of a member of a const scope.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

/// The mutability of a variable in a scope.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Mutability {
    Mutable,
    Immutable,
}

/// A member of a scope, i.e. a variable or a type definition.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: Identifier,
    pub ty: TermId,
    pub value: Option<TermId>,
    pub visibility: Visibility,
    pub mutability: Mutability,
}

/// A scope is either a variable scope or a constant scope.
///
/// Examples of variable scopes are:
/// - Block expression scope
/// - Function parameter scope
///
/// Examples of const scopes are:
/// - The root scope
/// - Module block scope
/// - Trait block scope
/// - Impl block scope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Variable,
    Constant,
}

/// Stores a list of members, indexed by the members' names.
#[derive(Debug, Clone)]
pub struct Scope {
    pub kind: ScopeKind,
    pub members: HashMap<Identifier, Member>,
}

impl Scope {
    /// Create an empty [Scope].
    pub fn empty(kind: ScopeKind) -> Self {
        Self {
            kind,
            members: HashMap::new(),
        }
    }

    /// Create a new [Scope] from the given members.
    pub fn new(kind: ScopeKind, members: impl IntoIterator<Item = Member>) -> Self {
        Self {
            kind,
            members: members
                .into_iter()
                .map(|member| (member.name, member))
                .collect(),
        }
    }

    /// Add a member by name.
    pub fn add(&mut self, member: Member) {
        self.members.insert(member.name, member);
    }

    /// Get a member by name.
    pub fn get(&self, member_name: Identifier) -> Option<&Member> {
        self.members.get(&member_name)
    }

    /// Get a member by name, mutably.
    pub fn get_mut(&mut self, member_name: Identifier) -> Option<&mut Member> {
        self.members.get_mut(&member_name)
    }
}

/// Trait to be implemented by primitives which contain a `name` field that is an
/// optional identifier.
pub trait GetNameOpt {
    /// Get the name of [Self], which should be an [Option<Identifier>].
    fn get_name_opt(&self) -> Option<Identifier>;
}

/// A list of parameters, generic over the parameter type.
///
/// Provides ways to store and get a parameter by its name or index.
#[derive(Debug, Clone)]
pub struct ParamList<ParamType: Clone> {
    params: Vec<ParamType>,
    name_map: HashMap<Identifier, usize>,
}

impl<ParamType: GetNameOpt + Clone> ParamList<ParamType> {
    /// Create a new [ParamList] from the given list of parameters.
    pub fn new(params: Vec<ParamType>) -> Self {
        let name_map = params
            .iter()
            .enumerate()
            .filter_map(|(i, param)| Some((param.get_name_opt()?, i)))
            .collect();

        Self { params, name_map }
    }

    /// Get the parameters as a positional slice
    pub fn positional(&self) -> &[ParamType] {
        &self.params
    }

    /// Get a parameter by name.
    pub fn get_by_name(&self, name: Identifier) -> Option<(usize, &ParamType)> {
        let param_index = *self.name_map.get(&name)?;
        Some((param_index, self.positional().get(param_index)?))
    }
}

/// An argument to a parameter.
#[derive(Debug, Clone, Hash)]
pub struct Arg {
    pub name: Option<Identifier>,
    pub value: TermId,
}

impl GetNameOpt for Arg {
    fn get_name_opt(&self) -> Option<Identifier> {
        self.name
    }
}

/// A list of arguments.
pub type Args = ParamList<Arg>;

/// A parameter, declaring a potentially named variable with a given type and default value.
#[derive(Debug, Clone, Hash)]
pub struct Param {
    pub name: Option<Identifier>,
    pub ty: TermId,
    pub default_value: Option<TermId>,
}

impl GetNameOpt for Param {
    fn get_name_opt(&self) -> Option<Identifier> {
        self.name
    }
}

/// A list of parameters.
pub type Params = ParamList<Param>;

/// The origin of a module: was it defined in a `mod` block, an anonymous `impl` block, or an
/// `impl Trait` block?
#[derive(Debug, Clone, Hash)]
pub enum ModDefOrigin {
    /// Defined as a trait implementation (for the given term that should resolve to a trait
    /// value).
    TrtImpl(TermId),
    /// Defined as an anonymous implementation.
    AnonImpl,
    /// Defined as a module (`mod` block).
    Mod,
    /// Defined as a file module or interactive.
    Source(SourceId),
}

/// A module definition, which is of a given origin, has a binding name, and contains some constant
/// members.
#[derive(Debug, Clone)]
pub struct ModDef {
    pub name: Option<Identifier>,
    pub origin: ModDefOrigin,
    pub members: ScopeId,
}

/// The fields of a struct.
#[derive(Debug, Clone)]
pub enum StructFields {
    /// An explicit set of fields, as a set of parameters.
    Explicit(Params),
    /// The struct does not have any accessible parameters.
    ///
    /// This is used for core language definitions that will be filled in later in the compiler
    /// pipeline.
    Opaque,
}

/// A struct definition, containing a binding name and a set of fields.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: Option<Identifier>,
    pub fields: StructFields,
}

/// An enum variant, containing a variant name and a set of fields.
///
/// Structurally the same as a struct.
#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: Identifier,
    pub fields: Params,
}

/// An enum definition, containing a binding name and a set of variants.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: Option<Identifier>,
    pub variants: HashMap<Identifier, EnumVariant>,
}

/// A trait definition, containing a binding name and a set of constant members.
#[derive(Debug, Clone)]
pub struct TrtDef {
    pub name: Option<Identifier>,
    pub members: ScopeId,
}

/// A nominal definition, which for now is either a struct or an enum.
#[derive(Debug, Clone)]
pub enum NominalDef {
    Struct(StructDef),
    Enum(EnumDef),
}

/// A tuple type, containg parameters as members.
#[derive(Debug, Clone)]
pub struct TupleTy {
    pub members: Params,
}

/// A function type, with a set of input parameters and a return type.
///
/// All the parameter types and return type must be level 0
#[derive(Debug, Clone)]
pub struct FnTy {
    pub params: Params,
    pub return_ty: TermId,
}

/// A type function type.
///
/// A type function is a compile-time function that works on types. Type function return values
/// can be level 0, level 1 or level 2. It has a general set of "base" parameters and return
/// type.
///
/// These are refined in the `cases` field, which provides conditional values for the return
/// value of the function, based on what the arguments are.
///
/// For example, consider:
///
/// ```ignore
/// Dog := <T> => struct(foo: T);
///
/// Dog ~= <T: Hash> => impl Hash {
///     hash = (self) => self.foo.hash();
/// };
///
/// Dog ~= <T: Hash ~ Eq> => impl FindInHashMap {
///     ...
/// };
/// ```
///
/// Then, the definition of the type function `Dog` would look something like:
///
/// ```ignore
/// TyFnTy {
///     general_params = (T: Term::Level2(Ty)),
///     general_return_ty = Term::Level2(Ty),
///     cases = {
///         (T: Term::Level2(Ty)) -> Term::Level2(Ty) => Term::NominalDef(DogStructDef),
///         (T: Term::Level2(Ty)(HashTraitDef)) -> Term::Level2(Ty)(HashTraitDef) => Term::Merge([
///             Term::NominalDef(DogStructDef),
///             Term::Mod(
///                 origin=TraitImpl(Term::Trt(HashTraitDef)),
///                 members=..
///             ),
///         ]),
///         (T: Ty::Merge([Term::Level2(Ty)(HashTraitDef), Term::Level2(Ty)(EqTraitDef)]))
///             -> Term::Level2(Ty)(FindInHashMapTraitDef) =>
///             => Term::Merge([
///                 Term::NominalDef(DogStructDef),
///                 Term::Mod(
///                     origin=TraitImpl(Term::Trt(FindInHashMapTraitDef)),
///                     members=..
///                 ),
///             ])
///     }
/// }
/// ```
///
/// At any point, the resolved type of `Dog<T>` is the merged type of the return type of each case
/// which matches `T`. In other words, cases are not short-circuiting; they are all evaluated and
/// then combined.
///
/// The `general_return_ty` field is always a supertype of the return type of each case.
#[derive(Debug, Clone)]
pub struct TyFn {
    /// An optional name for the type function, if it is directly assigned to a binding.
    pub name: Option<Identifier>,
    pub general_params: Params,
    pub general_return_ty: TermId,
    pub cases: Vec<TyFnCase>,
}

/// Represents a case in a type function, for some subset of its `general_params`, to some specific
/// return type and refined return value.
///
/// The `value` property of each [Param] in the `params` field represents types which have been
/// set, for example:
///
/// ```ignore
/// Dog<str> ~= impl Conv<str> {
///     conv = (self) => "Dog with foo=" + conv(self.foo);
/// };
/// ```
///
/// This translates to a case:
/// ```ignore
/// (T: Term::Level2(Ty) = Term::Level1(NominalDef(strDefId)))
///     -> Term::AppTyFn(ConvValue (a type fn), [Term::Level1(NominalDef(strDefId))])
///     => Term::Merge([
///         Term::AppTyFn(DogValue (a type fn), [Term::Level1(NominalDef(strDefId))]),
///         Term::ModDef(
///             origin=TraitImpl(Term::AppTyFn(ConvValue (a type fn),
///             [Term::Level1(NominalDef(strDefId))])),
///             members=...
///         )
///     ])
/// ```
///
/// The case's `return_ty` must always be able to unify with the target `general_return_ty`,
/// and the type parameters should be able to each unify with the target `general_params`, of the
/// parent [TyFn].
#[derive(Debug, Clone)]
pub struct TyFnCase {
    pub params: Params,
    pub return_ty: TermId,
    pub return_value: TermId,
}

/// Not yet resolved.
///
/// The resolution ID is incremented for each new unresolved term.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct UnresolvedTerm {
    pub resolution_id: ResolutionId,
}

/// A variable, which is just a name.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Var {
    pub name: Identifier,
}

/// The action of applying a set of arguments to a type function.
///
/// This essentially creates a lambda calculus within the Hash type system, which allows it to
/// express arbitrary programs.
///
/// When this type is unified with another type, the function is applied by first instantiating
/// its return value over its type parameters, and then unifying the instantiated type parameters
/// with the given type arguments of the function (the `ty_args` field).
#[derive(Debug, Clone)]
pub struct AppTyFn {
    pub ty_fn_value: TermId,
    pub args: Args,
}

/// The type of a type function, for example:
///
/// ```ignore
/// T: <X: Type> -> Type
/// ```
///
/// would be
///
/// ```ignore
/// name: "T",
/// ty: Term::TyFnTy(params = [(name="X", ty=Term::Level2(Ty))], return_ty=Term::Level2(Ty))
/// value: Term::Unset,
/// ```
#[derive(Debug, Clone)]
pub struct TyFnTy {
    pub params: Params,
    pub return_ty: TermId,
}

/// An enum variant value, consisting of a [NominalDefId] pointing to an enum, as well as the
/// variant of the enum in the form of an [Identifier].
///
/// Has a level 0 type.
#[derive(Debug, Clone)]
pub struct EnumVariantValue {
    pub enum_def_id: NominalDefId,
    pub variant_name: Identifier,
}

/// An access term, which is of the form X::Y, where X is a term and Y is an identifier.
///
/// Has level N where N is the level of the Y property of X.
#[derive(Debug, Clone)]
pub struct AccessTerm {
    pub subject_id: TermId,
    pub name: Identifier,
}

/// A level 3 term.
///
/// Type of: traits, for example: `trait(..)`.
/// Value of: nothing.
#[derive(Debug, Clone)]
pub enum Level3Term {
    /// The type that traits have
    TrtKind,
}

/// A level 2 term.
///
/// Type of: types, for example: `struct(..)`, `enum(..)`, `mod {..}`, `impl {..}`.
/// Value of: traits, for example `trait(..)`.
#[derive(Debug, Clone)]
pub enum Level2Term {
    // ---- Level 2 ---- the term that is a return term of trait(..)
    /// A trait term.
    Trt(TrtDefId),
    /// Basically a trait term that all types implement, i.e. the trait that is a supertrait to
    /// all other traits.
    AnyTy,
}

/// A level 1 term.
///
/// Type of: values, for example: `3`, `"test"`, `[1, 2, 3]`, `Dog(name="Bob")`.
/// Value of: types, for example `struct(..)`, `enum(..)`, `mod {..}`, `(a: A) -> B` etc.
#[derive(Debug, Clone)]
pub enum Level1Term {
    /// Modules or impls.
    ///
    /// Modules and trait implementations, as well as anonymous implementations, are treated as
    /// types, but do not have instance values.
    ///
    /// Information about the origin of each module definition can be found in its corresponding
    /// [ModDef] struct.
    ModDef(ModDefId),

    /// A nominal type definition, either a struct or an enum.
    NominalDef(NominalDefId),

    /// Tuple type.
    Tuple(TupleTy),

    /// Function type.
    Fn(FnTy),
}

/// A level 0 term.
///
/// Type of: nothing.
/// Value of: values, for example `3`, `Result::Ok(3)`, etc.
#[derive(Debug, Clone)]
pub enum Level0Term {
    /// A runtime value, has some Level 1 term as type (the inner data).
    Rt(TermId),

    /// An enum variant, which is either a constant term or a function value.
    EnumVariant(EnumVariantValue),
}

/// The basic data structure of a compile-time term.
#[derive(Debug, Clone)]
pub enum Term {
    // ---- Derived ----
    /// Access a member of a term.
    ///
    /// Is level N, where N is the level of the resultant access.
    Access(AccessTerm),

    /// A type-level variable, with some type that is stored in the current scope.
    ///
    /// Is level N-1, where N is the level of the type of the variable in the context
    Var(Var),

    /// Merge of multiple terms.
    ///
    /// Inner types must have the same level.
    ///
    /// Is level N, where N is the level of the inner types.
    Merge(Vec<TermId>),

    /// A type function term.
    ///
    /// Is level N, where N is the level of the return term of the function.
    TyFn(TyFn),

    /// Type function type.
    ///
    /// Is level N, where N is the level of the return term of the function.
    TyFnTy(TyFnTy),

    /// A type function application.
    ///
    /// Is level N, where N is the level of the resultant application.
    AppTyFn(AppTyFn),

    /// Not yet resolved.
    ///
    /// Unknown level (but not 0), to be determined by unification.
    Unresolved(UnresolvedTerm),

    /// A level 3 term.
    Level3(Level3Term),

    /// A level 2 term.
    Level2(Level2Term),

    /// A level 1 term.
    Level1(Level1Term),

    /// A level 0 term.
    Level0(Level0Term),
}

// IDs for all the primitives to be stored on mapped storage.

new_key_type! {
    /// The ID of a [TrtDef] stored in [super::trts::TrtDefStore].
    pub struct TrtDefId;
}

new_key_type! {
    /// The ID of a [NominalDef] stored in [super::nominals::NominalDefStore].
    pub struct NominalDefId;
}

new_key_type! {
    /// The ID of a [ModDef] stored in [super::mods::ModDefStore].
    pub struct ModDefId;
}

new_key_type! {
    /// The ID of a [Term] stored in [super::values::TermStore].
    pub struct TermId;
}

new_key_type! {
    /// The ID of a [Scope] stored in [super::values::ScopeStore].
    pub struct ScopeId;
}

/// The ID of a [UnresolvedTerm], separate from its [TermId], stored in
/// [super::terms::TermStore].
///
/// This needs to be separate from [TermId] so that if a type is copied (and new IDs are
/// generated for its members) the identity of the unknown variables remains the same as in the
/// original type.
#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct ResolutionId(pub(super) usize);
