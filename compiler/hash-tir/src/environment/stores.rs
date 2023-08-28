// @@Docs
use std::sync::OnceLock;

use hash_storage::stores;

use super::super::{
    args::{ArgsStore, PatArgsStore},
    data::{CtorDefsStore, DataDefStore},
    fns::FnDefStore,
    locations::LocationStore,
    mods::{ModDefStore, ModMembersStore},
    params::ParamsStore,
    pats::{PatListStore, PatStore},
    scopes::StackStore,
    symbols::SymbolStore,
    terms::{TermListStore, TermStore},
    tys::TyStore,
};
use crate::{
    args::{ArgsSeqStore, PatArgsSeqStore},
    ast_info::AstInfo,
    atom_info::AtomInfoStore,
    control::{MatchCasesSeqStore, MatchCasesStore},
    data::CtorDefsSeqStore,
    directives::AppliedDirectivesStore,
    mods::ModMembersSeqStore,
    params::ParamsSeqStore,
    pats::PatListSeqStore,
    terms::TermListSeqStore,
};

// All the stores that contain definitions for the typechecker.
stores! {
    Stores;
    args: ArgsStore,
    args_seq: ArgsSeqStore,
    ctor_defs: CtorDefsStore,
    ctor_defs_seq: CtorDefsSeqStore,
    data_def: DataDefStore,
    fn_def: FnDefStore,
    location: LocationStore,
    mod_def: ModDefStore,
    mod_members: ModMembersStore,
    mod_members_seq: ModMembersSeqStore,
    params: ParamsStore,
    params_seq: ParamsSeqStore,
    pat: PatStore,
    pat_args: PatArgsStore,
    pat_args_seq: PatArgsSeqStore,
    pat_list: PatListStore,
    pat_list_seq: PatListSeqStore,
    stack: StackStore,
    symbol: SymbolStore,
    term: TermStore,
    term_list: TermListStore,
    term_list_seq: TermListSeqStore,
    ty: TyStore,
    match_cases: MatchCasesStore,
    match_cases_seq: MatchCasesSeqStore,
    atom_info: AtomInfoStore,
    ast_info: AstInfo,
    directives: AppliedDirectivesStore,
}

/// The global [`Stores`] instance.
static STORES: OnceLock<Stores> = OnceLock::new();

/// Access the global [`Stores`] instance.
pub fn tir_stores() -> &'static Stores {
    STORES.get_or_init(Stores::new)
}

#[macro_export]
macro_rules! tir_debug_value_of_sequence_store_element_id {
    ($id:ident) => {
        impl std::fmt::Debug for $id {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use hash_storage::store::statics::StoreId;
                f.debug_tuple(stringify!($id))
                    .field(&(&self.0.index, &self.0.len))
                    .field(&self.1)
                    .field(&self.value())
                    .finish()
            }
        }
    };
}

#[macro_export]
macro_rules! tir_debug_value_of_single_store_id {
    ($id:ident) => {
        impl std::fmt::Debug for $id {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use hash_storage::store::statics::StoreId;
                f.debug_tuple(stringify!($id)).field(&self.index).field(&self.value()).finish()
            }
        }
    };
}

#[macro_export]
macro_rules! tir_debug_name_of_store_id {
    ($id:ident) => {
        impl std::fmt::Debug for $id {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use hash_storage::store::statics::StoreId;
                f.debug_tuple(stringify!($id)).field(&self.value().name).finish()
            }
        }
    };
}

#[macro_export]
macro_rules! tir_get {
    ($id:expr, $member:ident) => {{
        hash_storage::store::statics::StoreId::map($id, |x| x.$member)
    }};
}
