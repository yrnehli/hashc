//! This module defines types and logic that deal with ABIs (Application Binary
//! Interfaces). This is needed in order to communicate with the outside world
//! and to be able to call functions from other languages, but to also provide
//! information to code generation backends about how values are represented.

use hash_layout::TyInfo;

/// Defines the available calling conventions that can be
/// used when invoking functions with the ABI.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallingConvention {
    /// The C calling convention.
    ///
    /// Equivalent to the `ccc` calling convention in LLVM.
    ///
    /// Ref: <https://llvm.org/docs/LangRef.html#calling-conventions> (ccc)
    C,

    /// Cold calling convention for functions that are unlikely to be called.
    ///
    /// Equivalent to the `coldcc` calling convention in LLVM.
    ///
    /// Ref: <https://llvm.org/docs/LangRef.html#calling-conventions> (coldcc)
    Cold,
}

/// Defines ABI specific information about a function.
///
/// @@TODO: Do we need to record information about variadics here (when we add
/// them)?
#[derive(Debug)]
pub struct FnAbi {
    /// All the types of the arguments in order, and how they should
    /// be passed to the function (as per convention).
    pub args: Box<[ArgAbi]>,

    /// The return type of the function, and how it should be returned
    /// (as per convention).
    pub ret_abi: ArgAbi,

    /// The calling convention that should be used when invoking the function.
    pub calling_convention: CallingConvention,
}

/// Defines ABI specific information about an argument. [ArgAbi] is also
/// used to denote the return type of the function it has similar conventions
/// to function arguments.
#[derive(Debug)]
pub struct ArgAbi {
    /// The type of the argument.
    pub info: TyInfo,

    /// The passing mode of the argument.
    pub mode: PassMode,
}

impl ArgAbi {
    /// Check if the [PassMode] of the [ArgAbi] is "indirect".
    pub fn is_indirect(&self) -> bool {
        matches!(self.mode, PassMode::Indirect { .. })
    }

    /// Check if the [PassMode] of the [ArgAbi] is "ignored".
    pub fn is_ignored(&self) -> bool {
        matches!(self.mode, PassMode::Ignore)
    }
}

bitflags::bitflags! {
    /// Defines the relevant attributes to ABI arguments.
    pub struct ArgAttributeFlags: u16 {
        /// This specifies that this pointer argument does not alias
        /// any other arguments or the return value.
        const NO_ALIAS = 1 << 1;

        /// This specifies that this pointer argument is not captured
        /// by the callee function.
        const NO_CAPTURE = 1 << 2;

        /// This specifies that the pointer argument does not contain
        /// any undefined values or is un-initialised.
        ///
        /// In the following "llvm" example, this denotes that `foo` takes
        /// a pointer argument `x` that does not contain any undefined values.
        /// ```llvm
        /// define i32 @foo(i32* noundef %x) {
        /// ...
        /// }
        /// ```
        const NO_UNDEF = 1 << 3;
        /// This denotes that the pointer argument is not-null.

        const NON_NULL = 1 << 4;

        /// The argument is a read-only value.
        const READ_ONLY = 1 << 5;

        /// Apply a hint to the code generation that this particular
        /// argument should be passed in a register.
        const IN_REG = 1 << 6;
    }
}

/// Defines how an argument should be extended to a certain size.
///
/// This is used when a particular ABI requires small integer sizes to
/// be extended to a full or a partial register size. Additionally, the
/// ABI defines whether the value should be sign-extended or zero-extended.
///
/// If this is not required, this should be set to [`ArgExtension::NoExtend`].
#[derive(Debug, Clone, Copy)]
pub enum ArgExtension {
    /// The argument should be zero-extended.
    ZeroExtend,

    /// The argument should be sign-extended.
    SignExtend,

    /// The argument does not need to be extended.
    NoExtend,
}

/// [ArgAttributes] provides all of the attributes that a
/// particular argument has, as in additional information that
/// can be used by the compiler to generate better code, or
/// if it is targetting a specific ABI which requires certain
/// operations to be performed on the argument in order to
/// properly pass it to the function.
#[derive(Debug, Clone, Copy)]
pub struct ArgAttributes {
    /// Additional information about the argument in the form
    /// of bit flags. The [ArgAttributeFlags] resemble a similar
    /// naming convention to LLVM's function parameter attributes
    /// but they are intended to be used regardless of which
    /// backend is being targeted.
    pub flags: ArgAttributeFlags,

    /// Depending on the ABI, the argument may need to be zero/sign-extended
    /// to a certain size.
    pub extension: ArgExtension,
}

/// Defines how an argument should be passed to a function.
#[derive(Debug)]
pub enum PassMode {
    /// Ignore the argument, this is used for arguments that are not
    /// inhabited (as in cannot be constructed) or ZSTs.
    Ignore,

    /// Pass the argument directly.
    ///
    /// The argument has a layout ABI of [`AbiRepresentation::Scalar`] or
    /// [`AbiRepresentation::Vector`], and potentially
    /// [`AbiRepresentation::Aggregate`] if the ABI allows for "small"
    /// structures to be passed as just integers.
    Direct(ArgAttributes),

    /// Pass the argument indirectly via a pointer. This corresponds to
    /// passing arguments "by value". The "by value" semantics implies that
    /// a copy of the argument is made between the caller and callee. This
    /// is used to pass structs and arrays as arguments.
    ///
    /// N.B. This is not a valid attribute on a return type.
    Indirect {
        /// Attributes about the argument.
        attributes: ArgAttributes,

        /// Whether or not this is being passed on the
        /// stack.
        on_stack: bool,
    },
}