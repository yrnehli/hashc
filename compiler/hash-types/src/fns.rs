use hash_utils::{new_store_key, store::DefaultStore};

use crate::{arguments::ArgsIdOld, params::ParamsId, symbols::Symbol, terms::TermId, types::TyId};

/// A function type.
#[derive(Debug, Clone, Copy)]
pub struct FnTy {
    // @@MemoryUsage: use bitflags here
    pub is_implicit: bool,
    pub is_pure: bool,
    pub is_unsafe: bool,

    pub params: ParamsId,
    pub conditions: ParamsId,
    /// This is parametrised by the parameters of the function
    pub return_type: TyId,
}

/// A function definition.
#[derive(Debug, Clone, Copy)]
pub struct FnDef {
    pub name: Symbol,
    pub ty: FnTy,
    /// This is parametrised by the parameters of the function
    pub return_term: TermId,
}
new_store_key!(pub FnDefId);
pub type FnDefStore = DefaultStore<FnDefId, FnDef>;

/// A function call.
#[derive(Debug, Clone, Copy)]
pub struct FnCallTerm {
    pub subject: TermId,
    pub args: ArgsIdOld,
    /// The parameters of the function subject, if known.
    pub params: Option<ParamsId>,
}
