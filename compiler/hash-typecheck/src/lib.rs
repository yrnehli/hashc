//! The Hash typechecker.
//!
//! This brings light to the world by ensuring the correctness of the crude and
//! dangerous Hash program that is given as input to the compiler.
//!
//! @@Docs(kontheocharis): write docs about the stages of the typechecker.

#![feature(generic_associated_types, decl_macro, slice_pattern, option_result_contains, let_else)]

use diagnostics::reporting::TcErrorWithStorage;
use hash_pipeline::{traits::Tc, CompilerResult};
use hash_source::SourceId;
use storage::{AccessToStorage, AccessToStorageMut, GlobalStorage, LocalStorage, StorageRefMut};
use traverse::TcVisitor;

use crate::fmt::PrepareForFormatting;

pub mod diagnostics;
pub mod exhaustiveness;
pub mod fmt;
pub mod ops;
pub mod storage;
pub mod traverse;

/// The entry point of the typechecker.
pub struct TcImpl;

/// Contains global typechecker state, used for the [Tc] implementation below.
#[derive(Debug)]
pub struct TcState {
    pub global_storage: GlobalStorage,
    pub prev_local_storage: LocalStorage,
}

impl TcState {
    /// Create a new [TcState].
    pub fn new() -> Self {
        let source_id = SourceId::default();

        let mut global_storage = GlobalStorage::new();
        let local_storage = LocalStorage::new(&mut global_storage, source_id);
        Self { global_storage, prev_local_storage: local_storage }
    }
}

impl Default for TcState {
    fn default() -> Self {
        Self::new()
    }
}

impl Tc<'_> for TcImpl {
    type State = TcState;

    /// Make a [State] for [TcImpl]. Internally, this creates
    /// a new [GlobalStorage] and [LocalStorage] with a default
    /// [SourceId]. This is safe because both methods that are used
    /// to visit any source kind, will overwrite the stored [SourceId]
    /// to the `entry_point`.
    fn make_state(&mut self) -> CompilerResult<Self::State> {
        Ok(TcState::new())
    }

    fn check_interactive(
        &mut self,
        id: hash_source::InteractiveId,
        workspace: &hash_pipeline::sources::Workspace,
        state: &mut Self::State,
        _job_params: &hash_pipeline::settings::CompilerJobParams,
    ) -> CompilerResult<()> {
        // We need to set the interactive-id to update the current local-storage `id`
        // value
        state.prev_local_storage.set_current_source(SourceId::Interactive(id));

        // Instantiate a visitor with the source and visit the source, using the
        // previous local storage.
        let mut storage = StorageRefMut {
            global_storage: &mut state.global_storage,
            local_storage: &mut state.prev_local_storage,
            source_map: &workspace.source_map,
        };
        let mut tc_visitor = TcVisitor::new_in_source(storage.storages_mut(), workspace.node_map());
        match tc_visitor.visit_source() {
            Ok(source_term) => {
                println!("{}", source_term.for_formatting(storage.global_storage()));

                Ok(())
            }
            Err(error) => {
                // Turn the error into a report:
                let err_with_storage = TcErrorWithStorage { error, storage: storage.storages() };
                Err(vec![err_with_storage.into()])
            }
        }
    }

    fn check_module(
        &mut self,
        id: hash_source::ModuleId,
        sources: &hash_pipeline::sources::Workspace,
        state: &mut Self::State,
        _job_params: &hash_pipeline::settings::CompilerJobParams,
    ) -> CompilerResult<()> {
        // Instantiate a visitor with the source and visit the source, using a new local
        // storage.
        let mut local_storage = LocalStorage::new(&mut state.global_storage, SourceId::Module(id));

        let mut storage = StorageRefMut {
            global_storage: &mut state.global_storage,
            local_storage: &mut local_storage,
            source_map: &sources.source_map,
        };

        let mut tc_visitor = TcVisitor::new_in_source(storage.storages_mut(), sources.node_map());

        match tc_visitor.visit_source() {
            Ok(_) => Ok(()),
            Err(error) => {
                // Turn the error into a report:
                let err_with_storage = TcErrorWithStorage { error, storage: storage.storages() };
                Err(vec![err_with_storage.into()])
            }
        }
    }
}
