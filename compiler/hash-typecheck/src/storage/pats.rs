//! Contains structures to keep track of patterns and information relating to
//! them.
use std::cell::RefCell;

use super::primitives::{Pat, PatArgs, PatArgsId, PatId};
use slotmap::SlotMap;

/// Stores patterns, indexed by [PatId]s.
#[derive(Debug, Default)]
pub struct PatStore {
    data: RefCell<SlotMap<PatId, Pat>>,
}

impl PatStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a pattern, returning its assigned [PatId].
    pub fn create(&self, pat: Pat) -> PatId {
        self.data.borrow_mut().insert(pat)
    }

    /// Get a pattern by [PatId].
    pub fn get(&self, id: PatId) -> Pat {
        self.data.borrow().get(id).unwrap().clone()
    }
}

/// Stores pattern parameters, indexed by [PatArgsId]s.
#[derive(Debug, Default)]
pub struct PatArgsStore {
    data: SlotMap<PatArgsId, PatArgs>,
}

impl PatArgsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create pattern parameters, returning its assigned [PatArgsId].
    pub fn create(&mut self, params: PatArgs) -> PatArgsId {
        self.data.insert(params)
    }

    /// Get pattern parameters by [PatArgsId].
    pub fn get(&self, id: PatArgsId) -> &PatArgs {
        self.data.get(id).unwrap()
    }

    /// Get pattern parameters by [PatArgsId], mutably.
    pub fn get_mut(&mut self, id: PatArgsId) -> &mut PatArgs {
        self.data.get_mut(id).unwrap()
    }
}
