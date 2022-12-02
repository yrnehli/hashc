//! Definitions related to arguments to data structures, functions,
//! etc.
use core::fmt;
use std::fmt::Debug;

use hash_utils::{
    new_sequence_store_key,
    store::{DefaultSequenceStore, SequenceStore},
};
use utility_types::omit;

use super::{
    environment::env::{AccessToEnv, WithEnv},
    params::ParamTarget,
    pats::PatId,
};
use crate::new::terms::TermId;

/// An argument to a parameter.
///
/// This might be used for arguments in constructor calls `C(...)`, function
/// calls `f(...)` or `f<...>`, or type arguments.
#[omit(ArgData, [id], [Debug, Clone, Copy])]
#[derive(Debug, Clone, Copy)]
pub struct Arg {
    /// The ID of the argument in the argument list.
    pub id: ArgId,
    /// Argument target (named or positional).
    pub target: ParamTarget,
    /// The term that is the value of the argument.
    pub value: TermId,
}

new_sequence_store_key!(pub ArgsId);
pub type ArgId = (ArgsId, usize);
pub type ArgsStore = DefaultSequenceStore<ArgsId, Arg>;

/// A pattern argument to a parameter
///
/// This might be used for constructor patterns like `C(true, x)`.
#[omit(PatArgData, [id], [Debug, Clone, Copy])]
#[derive(Debug, Clone, Copy)]
pub struct PatArg {
    /// The ID of the argument in the argument pattern list.
    pub id: PatArgId,
    /// Argument target (named or positional).
    pub target: ParamTarget,
    /// The pattern in place for this argument.
    pub pat: PatId,
}

new_sequence_store_key!(pub PatArgsId);
pub type PatArgId = (PatArgsId, usize);
pub type PatArgsStore = DefaultSequenceStore<PatArgsId, PatArg>;

impl fmt::Display for WithEnv<'_, &Arg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value.target {
            ParamTarget::Name(name) => {
                write!(f, "{} = {}", self.env().with(name), self.env().with(self.value.value))
            }
            ParamTarget::Position(_) => write!(f, "{}", self.env().with(self.value.value)),
        }
    }
}

impl fmt::Display for WithEnv<'_, ArgId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.env().with(&self.stores().args().get_element(self.value)))
    }
}

impl fmt::Display for WithEnv<'_, ArgsId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.stores().args().map_fast(self.value, |args| {
            for (i, arg) in args.iter().enumerate() {
                write!(f, "{}", self.env().with(arg))?;
                if i > 0 {
                    write!(f, ", ")?;
                }
            }
            Ok(())
        })
    }
}
