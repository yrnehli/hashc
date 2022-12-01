//! Hash IR writing utilities. This module contains functionality
//! for printing out the IR in a human readable format. The format
//! is similar to the format used by the Rust compiler. Each IR Body
//! is printed out as a function, the body of the function shows
//! all of the declarations made by the body, followed by all of
//! the basic blocks that are used within the function body definition.

pub mod graphviz;
pub mod pretty;

use std::fmt;

use hash_utils::store::Store;

use super::ir::*;
use crate::{
    ty::{AdtId, IrTyId, IrTyListId},
    IrStorage,
};

/// Struct that is used to write [IrTy]s.
pub struct ForFormatting<'ir, T> {
    /// The item that is being printed.
    pub item: T,

    /// Whether the formatting should be verbose or not.
    pub verbose: bool,

    /// Whether the formatting implementations should write
    /// edges for IR items, this mostly applies to [Terminator]s.
    pub with_edges: bool,

    /// The storage used to print various IR constructs.
    pub storage: &'ir IrStorage,
}

pub trait WriteIr: Sized {
    fn for_fmt(self, storage: &IrStorage) -> ForFormatting<Self> {
        ForFormatting { item: self, storage, verbose: false, with_edges: true }
    }

    fn fmt_with_opts(
        self,
        storage: &IrStorage,
        verbose: bool,
        with_edges: bool,
    ) -> ForFormatting<Self> {
        ForFormatting { item: self, storage, verbose, with_edges }
    }
}

impl WriteIr for IrTyId {}
impl WriteIr for IrTyListId {}
impl WriteIr for AdtId {}

impl WriteIr for RValueId {}

impl fmt::Display for ForFormatting<'_, RValueId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.storage.rvalue_store().map_fast(self.item, |rvalue| match rvalue {
            RValue::Use(place) => write!(f, "{place}"),
            RValue::Const(Const::Zero(ty)) => write!(f, "{}", ty.for_fmt(self.storage)),
            RValue::Const(const_value) => write!(f, "const {const_value}"),
            RValue::BinaryOp(op, lhs, rhs) => {
                write!(f, "{op:?}({}, {})", lhs.for_fmt(self.storage), rhs.for_fmt(self.storage))
            }
            RValue::CheckedBinaryOp(op, lhs, rhs) => {
                write!(
                    f,
                    "Checked{op:?}({}, {})",
                    lhs.for_fmt(self.storage),
                    rhs.for_fmt(self.storage)
                )
            }
            RValue::UnaryOp(op, operand) => {
                write!(f, "{op:?}({})", operand.for_fmt(self.storage))
            }
            RValue::ConstOp(op, operand) => write!(f, "{op:?}({operand:?})"),
            RValue::Discriminant(place) => write!(f, "discriminant({place:?})"),
            RValue::Ref(region, borrow_kind, place) => {
                write!(f, "&{region:?} {borrow_kind:?} {place:?}")
            }
            RValue::Aggregate(aggregate_kind, operands) => {
                write!(f, "{aggregate_kind:?}({operands:?})")
            }
        })
    }
}

impl WriteIr for &Statement {}

impl fmt::Display for ForFormatting<'_, &Statement> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.item.kind {
            StatementKind::Nop => write!(f, "nop"),
            StatementKind::Assign(place, value) => {
                write!(f, "{place} = {}", (*value).for_fmt(self.storage))
            }
            // @@Todo: figure out format for printing out the allocations that
            // are made.
            StatementKind::Alloc(_) => todo!(),
            StatementKind::AllocRaw(_) => todo!(),
        }
    }
}

impl WriteIr for &Terminator {}

impl fmt::Display for ForFormatting<'_, &Terminator> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.item.kind {
            TerminatorKind::Goto(place) if self.with_edges => write!(f, "goto -> {place:?}"),
            TerminatorKind::Goto(_) => write!(f, "goto"),
            TerminatorKind::Return => write!(f, "return"),
            TerminatorKind::Call { op, args, target, destination } => {
                write!(f, "{destination} = {}(", op.for_fmt(self.storage))?;

                // write all of the arguments
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }

                    write!(f, "{}", arg.for_fmt(self.storage))?;
                }

                // Only print the target if there is a target, and if the formatting
                // specifies that edges should be printed.
                if let Some(target) = target && self.with_edges {
                    write!(f, ") -> {target:?}")
                } else {
                    write!(f, ")")
                }
            }
            TerminatorKind::Unreachable => write!(f, "unreachable"),
            TerminatorKind::Switch { value, table, otherwise } => {
                write!(f, "switch({value:?})")?;

                if self.with_edges {
                    write!(f, " [")?;

                    // Iterate over each value in the table, and add a arrow denoting
                    // that the CF will go to the specified block given the specified
                    // `value`.
                    for (i, (value, target)) in table.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }

                        write!(f, "{value:?} -> {target:?}")?;
                    }

                    // Write the default case
                    write!(f, "otherwise -> {otherwise:?}]")?;
                }

                Ok(())
            }
            TerminatorKind::Assert { condition, expected, kind, target } => {
                write!(f, "assert({condition}, {expected:?}, {kind:?})")?;

                if self.with_edges {
                    write!(f, "-> {target:?}")?;
                }

                Ok(())
            }
        }
    }
}