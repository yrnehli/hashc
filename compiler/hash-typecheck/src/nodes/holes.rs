use hash_storage::store::statics::StoreId;
use hash_tir::tir::{Hole, TermId, TyId};

use crate::{
    checker::Tc,
    env::TcEnv,
    errors::TcResult,
    operations::{unification::UnificationOptions, Operations},
};

impl<E: TcEnv> Tc<'_, E> {
    /// Unify two holes.
    ///
    /// This modifies src to have the contents of dest, and adds a unification
    /// to the context.
    pub fn unify_hole_with(
        &self,
        opts: &UnificationOptions,
        hole: Hole,
        hole_src: TermId,
        sub_dest: TermId,
    ) -> TcResult<()> {
        if opts.modify_terms.get() {
            hole_src.set(sub_dest.value());
        }
        self.add_unification(hole.0, sub_dest);
        Ok(())
    }
}

impl<E: TcEnv> Operations<Hole> for Tc<'_, E> {
    type TyNode = TyId;
    type Node = TermId;

    fn check(
        &self,
        _item: &mut Hole,
        _item_ty: Self::TyNode,
        _item_node: Self::Node,
    ) -> crate::errors::TcResult<()> {
        // No-op
        Ok(())
    }

    fn normalise(
        &self,
        _opts: &crate::operations::normalisation::NormalisationOptions,
        _item: Hole,
        _item_node: Self::Node,
    ) -> crate::operations::normalisation::NormaliseResult<Self::Node> {
        todo!()
    }

    fn unify(
        &self,
        _opts: &crate::operations::unification::UnificationOptions,
        _src: &mut Hole,
        _target: &mut Hole,
        _src_node: Self::Node,
        _target_node: Self::Node,
    ) -> crate::errors::TcResult<()> {
        todo!()
    }

    fn substitute(&self, _sub: &hash_tir::sub::Sub, _target: &mut Hole) {
        todo!()
    }
}
