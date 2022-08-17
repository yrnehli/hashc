//! Structures relating to storing type information, symbol information, and
//! other kinds of information managed by the typechecker.
//!
//! This information lives either in [LocalStorage] or [GlobalStorage],
//! depending on if it is accessible from within a given source only, or
//! accessible globally. For example, a stack variable will be in [LocalStorage]
//! because it is only accessible from one file, whereas a type definition will
//! be in [GlobalStorage] because it can be accessed from any file (with the
//! appropriate import).

pub mod arguments;
pub mod cache;
pub mod deconstructed;
pub mod location;
pub mod mods;
pub mod nominals;
pub mod param_list;
pub mod params;
pub mod pats;
pub mod primitives;
pub mod scope;
pub mod sources;
pub mod terms;
pub mod trts;

use std::cell::Cell;

use hash_source::{SourceId, SourceMap};
use hash_utils::store::Store;

use self::{
    arguments::ArgsStore,
    cache::Cache,
    deconstructed::{DeconstructedCtorStore, DeconstructedPatStore},
    location::LocationStore,
    mods::ModDefStore,
    nominals::NominalDefStore,
    params::ParamsStore,
    pats::{PatArgsStore, PatStore},
    primitives::{Scope, ScopeKind},
    scope::{ScopeId, ScopeStack, ScopeStore},
    sources::CheckedSources,
    terms::TermStore,
    trts::TrtDefStore,
};
use crate::{
    diagnostics::DiagnosticsStore,
    fmt::{ForFormatting, PrepareForFormatting},
    ops::bootstrap::create_core_defs_in,
};

/// Keeps track of typechecking information across all source files.
#[derive(Debug)]
pub struct GlobalStorage {
    pub scope_store: ScopeStore,
    pub term_store: TermStore,
    pub location_store: LocationStore,
    pub params_store: ParamsStore,
    pub args_store: ArgsStore,
    pub trt_def_store: TrtDefStore,
    pub mod_def_store: ModDefStore,
    pub nominal_def_store: NominalDefStore,
    pub pat_store: PatStore,
    pub pat_args_store: PatArgsStore,
    pub checked_sources: CheckedSources,

    /// Storage for tc diagnostics
    pub diagnostics_store: DiagnosticsStore,

    /// Pattern fields from
    /// [super::exhaustiveness::deconstruct::DeconstructedPat]
    pub deconstructed_pat_store: DeconstructedPatStore,

    /// The [super::exhaustiveness::construct::DeconstructedCtor] store.
    pub deconstructed_ctor_store: DeconstructedCtorStore,

    /// The typechecking cache, contains cached simplification, validation
    /// and unification results
    pub cache: Cache,

    /// Used to create the first scope when creating a LocalStorage.
    ///
    /// This includes all the core language definitions; it shouldn't be
    /// directly queried, but rather the [LocalStorage] scopes should be
    /// queried.
    pub root_scope: ScopeId,
}

impl GlobalStorage {
    /// Create a new, empty [GlobalStorage].
    pub fn new() -> Self {
        let scope_store = ScopeStore::new();
        let root_scope = scope_store.create(Scope::empty(ScopeKind::Constant));
        let gs = Self {
            location_store: LocationStore::new(),
            term_store: TermStore::new(),
            scope_store,
            diagnostics_store: DiagnosticsStore::default(),
            trt_def_store: TrtDefStore::new(),
            mod_def_store: ModDefStore::new(),
            nominal_def_store: NominalDefStore::new(),
            pat_store: PatStore::new(),
            pat_args_store: PatArgsStore::new(),
            deconstructed_pat_store: DeconstructedPatStore::new(),
            deconstructed_ctor_store: DeconstructedCtorStore::new(),
            checked_sources: CheckedSources::new(),
            root_scope,
            params_store: ParamsStore::new(),
            args_store: ArgsStore::new(),
            cache: Cache::new(),
        };
        create_core_defs_in(&gs);
        gs
    }
}

impl Default for GlobalStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Keeps track of typechecking information specific to a given source file.
#[derive(Debug)]
pub struct LocalStorage {
    /// All the scopes in a given source.
    pub scopes: ScopeStack,
    /// The current [SourceId]
    pub id: Cell<SourceId>,
}

impl LocalStorage {
    /// Create a new, empty [LocalStorage] for the given source.
    pub fn new(gs: &GlobalStorage, id: SourceId) -> Self {
        Self {
            scopes: ScopeStack::many([
                // First the root scope
                gs.root_scope,
                // Then the scope for the source
                gs.scope_store.create(Scope::empty(ScopeKind::Constant)),
            ]),
            id: Cell::new(id),
        }
    }

    /// Get the current [SourceId]
    pub fn current_source(&self) -> SourceId {
        self.id.get()
    }

    /// Set the current [SourceId], it does not matter whether
    /// this is a [SourceId::Module] or [SourceId::Interactive]
    pub fn set_current_source(&self, id: SourceId) {
        self.id.set(id);
    }
}

/// A reference to the storage, which includes both local and global storage, as
/// well as core definitions.
#[derive(Debug, Clone, Copy)]
pub struct StorageRef<'tc> {
    pub local_storage: &'tc LocalStorage,
    pub global_storage: &'tc GlobalStorage,
    pub source_map: &'tc SourceMap,
}

/// Trait that provides convenient accessor methods to various parts of the
/// storage given a path to a [StorageRef] object.
pub trait AccessToStorage {
    fn storages(&self) -> StorageRef;

    fn global_storage(&self) -> &GlobalStorage {
        self.storages().global_storage
    }

    fn local_storage(&self) -> &LocalStorage {
        self.storages().local_storage
    }

    fn diagnostic_store(&self) -> &DiagnosticsStore {
        &self.storages().global_storage.diagnostics_store
    }

    fn scope_store(&self) -> &ScopeStore {
        &self.global_storage().scope_store
    }

    fn term_store(&self) -> &TermStore {
        &self.global_storage().term_store
    }

    fn cache(&self) -> &Cache {
        &self.global_storage().cache
    }

    fn location_store(&self) -> &LocationStore {
        &self.global_storage().location_store
    }

    fn nominal_def_store(&self) -> &NominalDefStore {
        &self.global_storage().nominal_def_store
    }

    fn trt_def_store(&self) -> &TrtDefStore {
        &self.global_storage().trt_def_store
    }

    fn args_store(&self) -> &ArgsStore {
        &self.global_storage().args_store
    }

    fn params_store(&self) -> &ParamsStore {
        &self.global_storage().params_store
    }

    fn mod_def_store(&self) -> &ModDefStore {
        &self.global_storage().mod_def_store
    }

    fn pat_store(&self) -> &PatStore {
        &self.global_storage().pat_store
    }

    fn constructor_store(&self) -> &DeconstructedCtorStore {
        &self.global_storage().deconstructed_ctor_store
    }

    fn deconstructed_pat_store(&self) -> &DeconstructedPatStore {
        &self.global_storage().deconstructed_pat_store
    }

    fn pat_args_store(&self) -> &PatArgsStore {
        &self.global_storage().pat_args_store
    }

    fn checked_sources(&self) -> &CheckedSources {
        &self.global_storage().checked_sources
    }

    fn root_scope(&self) -> ScopeId {
        self.global_storage().root_scope
    }

    fn scopes(&self) -> &ScopeStack {
        &self.local_storage().scopes
    }

    /// Create a [ForFormatting] for the given `T` and [GlobalStorage] from
    /// self.
    fn for_fmt<T>(&self, t: T) -> ForFormatting<T>
    where
        T: PrepareForFormatting,
    {
        t.for_formatting(self.global_storage())
    }

    /// Get a reference to the [SourceMap]
    fn source_map(&self) -> &SourceMap {
        self.storages().source_map
    }
}

impl<'tc> AccessToStorage for StorageRef<'tc> {
    fn storages(&self) -> StorageRef {
        StorageRef { ..*self }
    }
}
