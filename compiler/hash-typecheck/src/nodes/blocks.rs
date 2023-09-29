use hash_tir::{
    scopes::BlockTerm,
    tir::{TermId, TyId},
};

use crate::{
    checker::Tc,
    env::TcEnv,
    errors::TcResult,
    operations::{
        normalisation::{NormalisationOptions, NormaliseResult},
        unification::UnificationOptions,
        Operations,
    },
};

impl<E: TcEnv> Operations<BlockTerm> for Tc<'_, E> {
    type TyNode = TyId;
    type Node = TermId;

    fn check(
        &self,

        _item: &mut BlockTerm,
        _item_ty: Self::TyNode,
        _item_node: Self::Node,
    ) -> TcResult<()> {
        todo!()
    }

    fn normalise(
        &self,

        _opts: &NormalisationOptions,
        _item: BlockTerm,
        _item_node: Self::Node,
    ) -> NormaliseResult<TermId> {
        todo!()
    }

    fn unify(
        &self,

        _opts: &UnificationOptions,
        _src: &mut BlockTerm,
        _target: &mut BlockTerm,
        _src_node: Self::Node,
        _target_node: Self::Node,
    ) -> TcResult<()> {
        todo!()
    }

    fn substitute(&self, _sub: &hash_tir::sub::Sub, _target: &mut BlockTerm) {
        todo!()
    }
}
