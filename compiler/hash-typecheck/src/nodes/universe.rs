use hash_tir::tir::{TermId, TyId, UniverseTy};

use crate::{
    checker::Tc,
    env::TcEnv,
    errors::TcResult,
    operations::{
        normalisation::{already_normalised, NormalisationOptions},
        Operations,
    },
};

impl<E: TcEnv> Operations<UniverseTy> for Tc<'_, E> {
    type TyNode = TyId;
    type Node = TyId;

    fn check(
        &self,
        _: &mut hash_tir::context::Context,
        _: &mut UniverseTy,
        item_ty: Self::TyNode,
        _: Self::Node,
    ) -> TcResult<()> {
        // Type: Type
        self.check_is_universe(item_ty)
    }

    fn normalise(
        &self,
        _: &mut hash_tir::context::Context,
        _opts: &NormalisationOptions,
        _: UniverseTy,
        _: Self::Node,
    ) -> crate::operations::normalisation::NormaliseResult<TermId> {
        already_normalised()
    }

    fn unify(
        &self,
        _: &mut hash_tir::context::Context,
        _: &crate::operations::unification::UnificationOptions,
        _: &mut UniverseTy,
        _: &mut UniverseTy,
        _: Self::Node,
        _: Self::Node,
    ) -> TcResult<()> {
        Ok(())
    }

    fn substitute(&self, _: &hash_tir::sub::Sub, _: &mut UniverseTy) {}
}
