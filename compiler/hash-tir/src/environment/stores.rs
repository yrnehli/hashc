use std::sync::OnceLock;

use hash_storage::stores;

use crate::{
    args::{ArgsSeqStore, ArgsStore, PatArgsSeqStore, PatArgsStore},
    ast_info::AstInfo,
    atom_info::AtomInfoStore,
    control::{MatchCasesSeqStore, MatchCasesStore},
    data::{CtorDefsSeqStore, CtorDefsStore, DataDefStore},
    fns::FnDefStore,
    locations::LocationStore,
    mods::{ModDefStore, ModMembersSeqStore, ModMembersStore},
    params::{ParamsSeqStore, ParamsStore},
    pats::{PatListSeqStore, PatListStore, PatStore},
    scopes::StackStore,
    symbols::SymbolStore,
    terms::{TermListSeqStore, TermListStore, TermStore},
    tys::TyStore,
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

#[macro_export]
macro_rules! tir_node_single_store {
    ($name:ident) => {
        paste::paste! {
            tir_node_single_store!(
                store = pub [<$name:camel Store>],
                id = pub [<$name:camel Id>],
                value = $name,
                store_name = [<$name:snake>]
            );
        }
    };
    (
        store = $store_vis:vis $store:ident,
        id = $id_vis:vis $id:ident,
        value = $value:ty,
        store_name = $store_name:ident
    ) => {
        hash_storage::static_single_store!(
            store = $store_vis $store,
            id = $id_vis $id,
            value = $crate::node::Node<$value>,
            store_name = $store_name,
            store_source = tir_stores()
        );
        $crate::tir_debug_value_of_single_store_id!($id);

        // @@Todo: enable once locations are properly set up
        // impl $crate::ast_info::HasNodeId for $id {
        //     fn node_id(&self) -> Option<hash_ast::ast::AstNodeId> {
        //         self.value().node_id()
        //     }
        // }
    };
}

#[macro_export]
macro_rules! tir_node_sequence_store_direct {
    ($name:ident) => {
        paste::paste! {
            tir_node_sequence_store_direct!(
                store = pub [<$name:camel sStore>] -> [<$name:camel sSeqStore>],
                id = pub [<$name:camel sId>] -> [<$name:camel sSeqId>][[<$name:camel Id>]],
                value = $name,
                store_name = ([<$name:snake s>], [<$name:snake s_seq>])
            );
        }
    };
    (
        store = $store_vis:vis $store:ident -> $seq_store:ident,
        id = $id_vis:vis $id:ident -> $id_seq:ident[$el_id:ident],
        value = $value:ty,
        store_name = ($store_name:ident, $seq_store_name:ident)
    ) => {
        $crate::tir_node_single_store!(
            store = $store_vis $store,
            id = $id_vis $id,
            value = $id_seq,
            store_name = $store_name
        );

        hash_storage::static_sequence_store_direct!(
            store = $store_vis $seq_store,
            id = $id_vis $id_seq[$el_id],
            value = $crate::node::Node<$value>,
            store_name = $seq_store_name,
            store_source = tir_stores()
        );

        $crate::tir_debug_value_of_sequence_store_element_id!($el_id);

        impl hash_storage::store::sequence::SequenceStoreKey for $id {
            type ElementKey = $el_id;

            fn to_index_and_len(self) -> (usize, usize) {
                use hash_storage::store::statics::StoreId;
                self.value().to_index_and_len()
            }

            fn from_index_and_len_unchecked(_: usize, _: usize) -> Self {
                panic!(
                    "{} cannot be used to create a new sequence, use {} instead",
                    stringify!($id),
                    stringify!($id_seq)
                )
            }
        }

        impl From<($id, usize)> for $el_id {
            fn from(value: ($id, usize)) -> Self {
                use hash_storage::store::statics::StoreId;
                $el_id(value.0.value().data, value.1)
            }
        }
    };
}

#[macro_export]
macro_rules! tir_node_sequence_store_indirect {
    ($name_s:ident[$element:ty]) => {
        paste::paste! {
            tir_node_sequence_store_indirect!(
                store = pub [<$name_s:camel Store>] -> [<$name_s:camel SeqStore>],
                id = pub [<$name_s:camel Id>] -> [<$name_s:camel SeqId>][[<$element>]],
                store_name = ([<$name_s:snake>], [<$name_s:snake _seq>])
            );
        }
    };
    (
        store = $store_vis:vis $store:ident -> $seq_store:ident,
        id = $id_vis:vis $id:ident -> $id_seq:ident[$el_id:ident],
        store_name = ($store_name:ident, $seq_store_name:ident)
    ) => {
        $crate::tir_node_single_store!(
            store = $store_vis $store,
            id = $id_vis $id,
            value = $id_seq,
            store_name = $store_name
        );

        hash_storage::static_sequence_store_indirect!(
            store = $store_vis $seq_store,
            id = $id_vis $id_seq[$el_id],
            store_name = $seq_store_name,
            store_source = tir_stores()
        );

        impl hash_storage::store::sequence::SequenceStoreKey for $id {
            type ElementKey = $el_id;

            fn to_index_and_len(self) -> (usize, usize) {
                use hash_storage::store::statics::StoreId;
                self.value().to_index_and_len()
            }

            fn from_index_and_len_unchecked(_: usize, _: usize) -> Self {
                panic!(
                    "{} cannot be used to create a new sequence, use {} instead",
                    stringify!($id),
                    stringify!($id_seq)
                )
            }
        }

        impl From<($id, usize)> for $el_id {
            fn from(value: ($id, usize)) -> Self {
                use hash_storage::store::statics::StoreId;
                value.0.borrow().at(value.1).unwrap()
            }
        }
    };
}
