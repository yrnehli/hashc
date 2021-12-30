use std::{collections::{BTreeMap, HashMap}, slice::SliceIndex};

use hash_alloc::{brick::Brick, collections::row::Row, row, Wall};
use hash_ast::ast::TypeId;
use hash_utils::counter;

use crate::types::{TypeList, Types, FnType};

counter! {
    name: TraitId,
    counter_name: TRAIT_COUNTER,
    visibility: pub,
    method_visibility:,
}

#[derive(Debug)]
pub struct TraitBounds<'c> {
    pub bounds: Row<'c, TraitBound<'c>>,
}

impl<'c> TraitBounds<'c> {
    pub fn empty() -> Self {
        Self { bounds: row![] }
    }

    // pub fn map(&self, f: impl FnMut(&TraitBound<'c>) -> TraitBound<'c>) -> Self {
    //     TraitBounds {
    //         data: Row::from_iter(self.data.iter().map(f), &self.wall),
    //         wall: self.wall.clone(),
    //     }
    // }
}

#[derive(Debug)]
pub struct TraitBound<'c> {
    pub trt: TraitId,
    pub params: TypeList<'c>,
}

impl TraitBound<'_> {}

counter! {
    name: TraitImplId,
    counter_name: TRAIT_IMPL_COUNTER,
    visibility: pub,
    method_visibility:,
}

#[derive(Debug)]
pub struct TraitImpl<'c> {
    pub trait_id: TraitId,
    pub args: TypeList<'c>,
    pub bounds: TraitBounds<'c>,
}

impl<'c> TraitImpl<'c> {
    pub fn matches_fn_args(&self, traits: &Traits) -> bool {
        let trt = traits.get(self.trait_id);
        todo!()
    }

    pub fn instantiate(&self, given_args: &TypeList<'c>) -> Option<()> {
        if given_args.len() != self.args.len() {
            // @@TODO: error
            return None;
        }

        for (&_trait_arg, &_given_arg) in self.args.iter().zip(given_args.iter()) {}

        None
    }
}

#[derive(Debug)]
pub struct Trait<'c> {
    pub args: TypeList<'c>,
    pub bounds: TraitBounds<'c>,
    pub fn_type: &'c FnType<'c>,
}

#[derive(Debug)]
pub struct ImplsForTrait<'c> {
    trt: TraitId,
    impls: BTreeMap<TraitImplId, TraitImpl<'c>>,
}

impl<'c> ImplsForTrait<'c> {
    pub fn resolve_call(&self, fn_args: &[TypeId]) -> TraitImplId {
        todo!();
        // for (&impl_id, impl) in self.impls.iter() {
        // }
    }
}

#[derive(Debug)]
pub struct TraitImpls<'c, 'w> {
    data: HashMap<TraitId, ImplsForTrait<'c>>,
    wall: &'w Wall<'c>,
}

impl<'c, 'w> TraitImpls<'c, 'w> {
    pub fn new(wall: &'w Wall<'c>) -> Self {
        Self {
            data: HashMap::new(),
            wall,
        }
    }

    pub fn for_trait(&self, trait_id: TraitId) -> &ImplsForTrait<'c> {
        self.data.get(&trait_id).unwrap()
    }

    pub fn resolve_call(trait_id: TraitId, fn_args: &[TypeId]) -> TraitImplId {
        // Should substitute given TypeIds with their correct version from the trait impl!

        todo!()

    }
}

#[derive(Debug)]
pub struct Traits<'c, 'w> {
    data: HashMap<TraitId, Brick<'c, Trait<'c>>>,
    wall: &'w Wall<'c>,
}

impl<'c, 'w> Traits<'c, 'w> {
    pub fn new(wall: &'w Wall<'c>) -> Self {
        Self {
            data: HashMap::new(),
            wall,
        }
    }

    pub fn get(&self, trait_id: TraitId) -> &Trait<'c> {
        self.data.get(&trait_id).unwrap()
    }

    pub fn create(&mut self, trt: Trait<'c>) -> TraitId {
        let id = TraitId::new();
        self.data.insert(id, Brick::new(trt, self.wall));
        id
    }
}

#[derive(Debug)]
pub struct CoreTraits {
    pub hash: TraitId,
    pub eq: TraitId,
}

impl<'c, 'w> CoreTraits {
    pub fn create(types: &mut Types<'c, 'w>, wall: &'w Wall<'c>) -> Self {
        todo!()
    }
}
