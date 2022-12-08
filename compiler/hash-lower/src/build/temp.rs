//! Lowering logic for compiling an [Expr] into a temporary [Local].

use hash_ast::ast::{AstNodeRef, Expr};
use hash_ir::{
    ir::{BasicBlock, Local, LocalDecl, Place},
    ty::{IrTyId, Mutability},
};

use super::{BlockAnd, Builder};
use crate::build::{unpack, BlockAndExtend};

impl<'tcx> Builder<'tcx> {
    /// Compile an [Expr] into a freshly created temporary [Place].
    pub(crate) fn expr_into_temp(
        &mut self,
        mut block: BasicBlock,
        expr: AstNodeRef<'tcx, Expr>,
        mutability: Mutability,
    ) -> BlockAnd<Local> {
        let temp = {
            // @@Hack: for now literal expressions don't get their type set on the node, it
            // is the underlying literal that has the type, so we read that in this case.
            let id = if let Expr::Lit(lit) = expr.body { lit.data.id() } else { expr.id() };
            let ty = self.ty_id_of_node(id);

            let local = LocalDecl::new_auxiliary(ty, mutability);
            let scope = self.current_scope();

            self.push_local(local, scope)
        };
        let temp_place = Place::from(temp);

        unpack!(block = self.expr_into_dest(temp_place, block, expr));
        block.and(temp)
    }

    /// Create a temporary place with a given type.
    pub(crate) fn temp_place(&mut self, ty: IrTyId) -> Place {
        let local = LocalDecl::new_auxiliary(ty, Mutability::Immutable);
        let scope = self.current_scope();

        let local = self.push_local(local, scope);
        Place::from(local)
    }
}
