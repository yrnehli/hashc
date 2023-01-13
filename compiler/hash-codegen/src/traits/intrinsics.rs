//! This defines all of the traits to generate **intrinsic** specific
//! code for a target backend. The intrinsics for Hash are defined in
//! the `prelude` module within the standard library, and are used to
//! provide a common interface for the standard library to interact with
//! compiler.

use hash_ir::ty::IrTyId;

use super::BackendTypes;

/// Trait that is used to generate intrinsic calls for a target backend.
///
/// @@Todo: this is probably where `va_*` intrinsic functions would be
/// defined.
pub trait BuildIntrinsicCallMethods<'b>: BackendTypes {
    /// Generate a call to an intrinsic function.
    fn codegen_intrinsic_call(
        &self,
        ty: IrTyId,
        args: &[Self::Value],
        result: Self::Value,
    ) -> Self::Value;

    /// Generate a call to the `abort` intrinsic function. This
    /// will terminate the program unconditionally.
    fn codegen_abort_intrinsic(&mut self);

    /// Generare a call to the `expect` intrinsic function. This
    /// will generate a panic if the `expected` value is `false`.
    ///
    /// Ref: <https://llvm.org/docs/LangRef.html#llvm-expect-intrinsic>
    fn codegen_expect_intrinsic(&mut self, value: Self::Value, expected: bool) -> Self::Value;
}