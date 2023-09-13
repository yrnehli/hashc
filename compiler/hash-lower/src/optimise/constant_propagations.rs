//! Module which contains implementation and utilities for constant
//! propagation and constant folding optimisations that can occur on
//! Hash IR.

use hash_ir::{
    ir::{self, Const, ConstKind, Scalar},
    ty::{IrTy, IrTyId},
};
use hash_layout::compute::LayoutComputer;
use hash_storage::store::statics::StoreId;
use hash_target::primitives::FloatTy;
use hash_utils::derive_more::Constructor;

/// A constant folder which is used to fold constants into a single
/// constant.
#[derive(Constructor)]
pub struct ConstFolder<'ctx> {
    /// For computing layouts
    lc: LayoutComputer<'ctx>,
}

type OverflowingOp = fn(i128, i128) -> (i128, bool);

impl<'ctx> ConstFolder<'ctx> {
    /// Attempt to evaluate two [ir::Const]s and a binary operator.
    pub fn try_fold_bin_op(&self, op: ir::BinOp, lhs: &Const, rhs: &Const) -> Option<Const> {
        // If the two constants are non-scalar, then we abort the folding...
        let (ConstKind::Scalar(left), ConstKind::Scalar(right)) = (lhs.kind(), rhs.kind()) else {
            return None;
        };

        let (l_ty, r_ty) = (lhs.ty(), rhs.ty());

        match l_ty.value() {
            IrTy::Int(_) | IrTy::UInt(_) => {
                let size = self.lc.size_of_ty(l_ty).ok()?;
                let l_bits = left.to_bits(size).ok()?;
                let r_bits = right.to_bits(size).ok()?;
                self.binary_int_op(op, l_ty, l_bits, r_ty, r_bits)
            }
            IrTy::Float(ty) => match ty {
                FloatTy::F32 => Self::binary_float_op(op, left.to_f32(), left.to_f32()),
                FloatTy::F64 => Self::binary_float_op(op, left.to_f64(), left.to_f64()),
            },
            IrTy::Bool => {
                let l: bool = left.try_into().ok()?;
                let r: bool = right.try_into().ok()?;
                Self::binary_bool_op(op, l, r)
            }
            IrTy::Char => {
                let l: char = left.try_into().ok()?;
                let r: char = right.try_into().ok()?;
                Self::binary_char_op(op, l, r)
            }
            _ => None,
        }
    }

    fn binary_bool_op(bin_op: ir::BinOp, lhs: bool, rhs: bool) -> Option<Const> {
        use ir::BinOp::*;

        Some(match bin_op {
            Gt => Const::bool(lhs & !rhs),
            GtEq => Const::bool(lhs >= rhs),
            Lt => Const::bool(!lhs & rhs),
            LtEq => Const::bool(lhs <= rhs),
            Eq => Const::bool(lhs == rhs),
            Neq => Const::bool(lhs != rhs),
            BitAnd => Const::bool(lhs & rhs),
            BitOr => Const::bool(lhs | rhs),
            BitXor => Const::bool(lhs ^ rhs),
            _ => panic!("invalid operator `{bin_op}` for boolean operands"),
        })
    }

    /// Perform an operation on two character constants.
    ///
    /// ##Note: This only supports comparison operators, thus the output is always
    /// a boolean constant.
    fn binary_char_op(bin_op: ir::BinOp, lhs: char, rhs: char) -> Option<Const> {
        use ir::BinOp::*;

        Some(match bin_op {
            Gt => Const::bool(lhs > rhs),
            GtEq => Const::bool(lhs >= rhs),
            Lt => Const::bool(lhs < rhs),
            LtEq => Const::bool(lhs <= rhs),
            Eq => Const::bool(lhs == rhs),
            Neq => Const::bool(lhs != rhs),
            _ => panic!("invalid operator `{bin_op}` for boolean operands"),
        })
    }

    /// Perform an operation on two integer constants. This accepts the raw bits
    /// of the integer, and the size of the integer.
    fn binary_int_op(
        &self,
        bin_op: ir::BinOp,
        lhs_ty: IrTyId,
        lhs: u128,
        rhs_ty: IrTyId,
        rhs: u128,
    ) -> Option<Const> {
        use ir::BinOp::*;

        // We have to handle `shl` and `shr` differently since they have different
        // operand types.
        if matches!(bin_op, Shl | Shr) {
            todo!()
        }

        debug_assert_eq!(lhs_ty, rhs_ty);
        let size = self.lc.size_of_ty(lhs_ty).ok()?;

        // If the type is signed, we have to handle comparisons and arithmetic
        // operations differently.
        if lhs_ty.is_signed() {
            // Handle binary comparison operators first...
            let op: Option<fn(&i128, &i128) -> bool> = match bin_op {
                Gt => Some(i128::gt),
                GtEq => Some(i128::ge),
                Lt => Some(i128::lt),
                LtEq => Some(i128::le),
                Eq => Some(i128::eq),
                Neq => Some(i128::ne),
                _ => None,
            };

            if let Some(op) = op {
                // Sign extend the two values to become i128s.
                let left = size.sign_extend(lhs) as i128;
                let right = size.sign_extend(rhs) as i128;
                return Some(Const::bool(op(&left, &right)));
            }

            // The we get the function to perform the operation on the
            // two signed integers.
            let op: Option<OverflowingOp> = match bin_op {
                Add => Some(i128::overflowing_add),
                Sub => Some(i128::overflowing_sub),
                Mul => Some(i128::overflowing_mul),
                // @@ErrorHandling @@UB: we should somehow emit an error saying that a
                // constant operation is divide by zero, and this is UB at runtime!!!
                Div if rhs == 0 => return None,
                Mod if rhs == 0 => return None,
                Div => Some(i128::overflowing_div),
                Mod => Some(i128::overflowing_rem),
                _ => None,
            };

            if let Some(op) = op {
                let left = size.sign_extend(lhs) as i128;
                let right = size.sign_extend(rhs) as i128;

                // @@ErrorHandling @@UB: we should somehow emit an error saying that a
                // constant operation overflowed, and this is UB at runtime!!!
                if matches!(bin_op, Div | Mod) && left == size.signed_int_min() && right == -1 {
                    return None;
                }

                // Compute the result, truncate and convert into a Scalar.
                let (result, overflow) = op(left, right);
                let result = result as u128;
                let truncated = size.truncate(result);
                let overflow = overflow || size.sign_extend(truncated) != result;

                // @@ErrorHandling @@UB: we should somehow emit an error saying that a
                // constant operation overflowed, and this is UB at runtime!!!
                if overflow {
                    return None;
                }

                return Some(Const::new(
                    lhs_ty,
                    ir::ConstKind::Scalar(Scalar::from_uint(truncated, size)),
                ));
            }
        }

        match bin_op {
            Eq => Some(Const::bool(lhs == rhs)),
            Neq => Some(Const::bool(lhs != rhs)),
            Gt => Some(Const::bool(lhs > rhs)),
            GtEq => Some(Const::bool(lhs >= rhs)),
            Lt => Some(Const::bool(lhs < rhs)),
            LtEq => Some(Const::bool(lhs <= rhs)),
            BitOr => Some(Const::from_scalar_like(lhs | rhs, lhs_ty, &self.lc)),
            BitAnd => Some(Const::from_scalar_like(lhs & rhs, lhs_ty, &self.lc)),
            BitXor => Some(Const::from_scalar_like(lhs ^ rhs, lhs_ty, &self.lc)),
            Add | Sub | Mul | Div | Mod => {
                let op: fn(u128, u128) -> (u128, bool) = match bin_op {
                    Add => u128::overflowing_add,
                    Sub => u128::overflowing_sub,
                    Mul => u128::overflowing_mul,
                    // @@ErrorHandling @@UB: we should somehow emit an error saying that a
                    // constant operation is divide by zero, and this is UB at runtime!!!
                    Div if rhs == 0 => return None,
                    Mod if rhs == 0 => return None,
                    Div => u128::overflowing_div,
                    Mod => u128::overflowing_rem,
                    _ => unreachable!(),
                };

                // Compute the result, truncate and convert into a Scalar.
                let (result, overflow) = op(lhs, rhs);
                let truncated = size.truncate(result);
                let overflow = overflow || truncated != result;

                // @@ErrorHandling @@UB: we should somehow emit an error saying that a
                // constant operation overflowed, and this is UB at runtime!!!
                if overflow {
                    return None;
                }

                Some(Const::new(lhs_ty, ir::ConstKind::Scalar(Scalar::from_uint(truncated, size))))
            }
            Exp => None,
            _ => panic!("invalid operator, `{bin_op}` should have been handled"),
        }
    }

    /// Perform an operation on two floating point constants.
    fn binary_float_op<T: num_traits::Float + Into<Const>>(
        op: ir::BinOp,
        lhs: T,
        rhs: T,
    ) -> Option<Const> {
        Some(match op {
            ir::BinOp::Gt => Const::bool(lhs > rhs),
            ir::BinOp::GtEq => Const::bool(lhs >= rhs),
            ir::BinOp::Lt => Const::bool(lhs < rhs),
            ir::BinOp::LtEq => Const::bool(lhs <= rhs),
            ir::BinOp::Eq => Const::bool(lhs == rhs),
            ir::BinOp::Neq => Const::bool(lhs != rhs),
            ir::BinOp::Add => (lhs + rhs).into(),
            ir::BinOp::Sub => (lhs - rhs).into(),
            ir::BinOp::Mul => (lhs * rhs).into(),
            ir::BinOp::Div => (lhs / rhs).into(),
            ir::BinOp::Mod => (lhs % rhs).into(),
            ir::BinOp::Exp => (lhs.powf(rhs)).into(),

            // No other operations can be performed on floats
            _ => return None,
        })
    }
}