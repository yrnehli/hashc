//! Contains functions to traverse the AST and add types to it, while checking
//! it for correctness.

use std::{cell::Cell, iter};

use hash_ast::{
    ast::{
        self, AccessKind, AstNodeRef, BinOp, Lit, MatchOrigin, OwnsAstNode, ParamOrigin, RefKind,
        UnOp,
    },
    node_map::{NodeMap, SourceRef},
    visitor::{walk, AstVisitor},
};
use hash_reporting::{diagnostic::Diagnostics, macros::panic_on_span};
use hash_source::{
    entry_point::EntryPointKind,
    identifier::{Identifier, IDENTS},
    location::{SourceLocation, Span},
    ModuleKind, SourceId,
};
use hash_tir::{
    args::Arg,
    location::{IndexedLocationTarget, LocationTarget},
    mods::ModDefOrigin,
    nodes::NodeInfoTarget,
    nominals::EnumVariant,
    params::{AccessOp, Field, Param},
    pats::{BindingPat, ConstPat, Pat, PatArg, PatId, RangePat, SpreadPat},
    scope::{Member, Mutability, ScopeKind, ScopeMember, Visibility},
    storage::LocalStorage,
    terms::{Sub, TermId},
};
use hash_utils::store::{CloneStore, Store};
use itertools::Itertools;

use super::{scopes::VisitConstantScope, AccessToTraverseOps};
use crate::{
    diagnostics::{
        error::{TcError, TcResult},
        macros::tc_panic,
        warning::TcWarning,
    },
    ops::AccessToOps,
    storage::{AccessToStorage, StorageRef},
};

bitflags::bitflags! {
    /// The currently applying directives at the current location within
    /// the AST.
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub(crate) struct AppliedDirectives: u8 {
        /// The traversal is currently within a "intrinsics" directive, specifying
        /// a module within the prelude which declares all of the intrinsic language
        /// items.
        const INTRINSICS = 1 << 1;

        /// The traversal is currently with a "entry_point" directive, expecting
        /// for their to be a function definition which is used as the entry
        /// point of the function.
        const ENTRY_POINT = 1 << 2;
    }
}

impl AppliedDirectives {
    /// Check whether `intrinsics` is set.
    pub fn is_intrinsics(&self) -> bool {
        self.contains(Self::INTRINSICS)
    }

    /// Check whether `entry_point` is set.
    pub fn is_entry_point(&self) -> bool {
        self.contains(Self::ENTRY_POINT)
    }
}

/// Internal state that the [TcVisitor] uses when traversing the
/// given sources.
#[derive(Default)]
pub struct TcVisitorState {
    /// Pattern hint from declaration
    pub declaration_name_hint: Cell<Option<Identifier>>,

    /// Return type for functions with return statements
    pub fn_def_return_ty: Cell<Option<TermId>>,

    /// The currently applied directives on the current traversed
    /// node.
    pub(crate) applied_directives: Cell<AppliedDirectives>,

    /// If traversing a declaration, what to set for the
    /// `assignments_until_closed` field.
    pub declaration_assignments_until_closed: Cell<usize>,
}

impl TcVisitorState {
    /// Create a new [TcVisitorState]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a reference to the [AppliedDirectives] currently applied
    /// to the current node.
    pub(crate) fn applied_directives(&self) -> AppliedDirectives {
        self.applied_directives.get()
    }
}

/// Traverses the AST and adds types to it, while checking it for correctness.
///
/// Contains typechecker state that is accessed while traversing.
pub struct TcVisitor<'tc> {
    /// A reference to the storage.
    pub storage: StorageRef<'tc>,

    /// A reference to all of the sources in the current workspace.
    pub node_map: &'tc NodeMap,

    /// The state of the visitor.
    pub state: TcVisitorState,
}

impl<'tc> AccessToStorage for TcVisitor<'tc> {
    fn storages(&self) -> StorageRef {
        self.storage.storages()
    }
}

impl<'tc> TcVisitor<'tc> {
    /// Create a new [TcVisitor] with the given state, traversing the given
    /// source from [SourceId].
    pub fn new_in_source(storage: StorageRef<'tc>, node_map: &'tc NodeMap) -> Self {
        TcVisitor { storage, node_map, state: TcVisitorState::new() }
    }

    /// Visits the source passed in as an argument to [Self::new_in_source], and
    /// returns the term of the module that corresponds to the source.
    pub fn visit_source(&self) -> TcResult<TermId> {
        let source_id = self.local_storage().current_source();
        let source = self.node_map.get_source(source_id);

        let result = match source {
            SourceRef::Interactive(interactive_source) => {
                self.visit_body_block(interactive_source.node_ref())
            }
            SourceRef::Module(module_source) => self.visit_module(module_source.node_ref()),
        }?;

        // Add the result to the checked sources.
        // @@Correctness: the visitor will loop infinitely if there are circular module
        // dependencies. Need to find a way to prevent this.
        self.checked_sources().mark_checked(source_id, result);

        // If the source is the entry point of the program, and it is specified that
        // an executable is to be produced from the sources, then we need to verify
        // the module contains a main function

        Ok(result)
    }

    /// Create a [SourceLocation] from a [Span].
    pub(crate) fn source_location(&self, span: Span) -> SourceLocation {
        SourceLocation { span, id: self.local_storage().current_source() }
    }

    /// Create a [SourceLocation] at the given [ast::AstNode].
    pub(crate) fn source_location_at_node<N>(&self, node: AstNodeRef<N>) -> SourceLocation {
        let node_span = node.span();
        self.source_location(node_span)
    }

    /// Copy the [SourceLocation] of the given [ast::AstNode] to the
    /// given [LocationTarget].
    pub(crate) fn copy_location_from_node_to_target<N>(
        &self,
        node: AstNodeRef<N>,
        target: impl Into<LocationTarget>,
    ) {
        let location = self.source_location_at_node(node);
        self.location_store().add_location_to_target(target, location);
    }

    /// Copy the [SourceLocation] of the given [ast::AstNode] list to
    /// the given [LocationTarget] list represented by a type `Target` where
    /// `(Target, usize)` implements [Into<LocationTarget>].
    pub(crate) fn copy_location_from_nodes_to_targets<'n, N: 'n>(
        &self,
        nodes: impl IntoIterator<Item = AstNodeRef<'n, N>>,
        targets: impl Into<IndexedLocationTarget> + Clone,
    ) {
        let targets = targets.into();
        for (index, param) in nodes.into_iter().enumerate() {
            self.copy_location_from_node_to_target(param, LocationTarget::from((targets, index)));
        }
    }

    /// Register the given [`TermId`] or [`PatId`] as describing the given
    /// [AstNodeRef].
    ///
    /// This copies the node's location to the target, and adds the node-target
    /// pair to [hash_tir::nodes::NodeInfoStore].
    pub(crate) fn register_node_info_and_location<T>(
        &self,
        node: AstNodeRef<T>,
        data: impl Into<NodeInfoTarget> + Into<LocationTarget> + Clone,
    ) {
        self.copy_location_from_node_to_target(node, data.clone());
        self.node_info_store().update_or_insert(node.id(), data.into());
    }

    /// Register the given [`NodeInfoTarget`] as describing the given
    /// [AstNodeRef].
    ///
    /// This adds the node-target pair to [hash_tir::nodes::NodeInfoStore].
    pub(crate) fn register_node_info<T>(
        &self,
        node: AstNodeRef<T>,
        data: impl Into<NodeInfoTarget>,
    ) {
        self.node_info_store().update_or_insert(node.id(), data.into());
    }

    /// Validate and register node info for the given term.
    fn validate_and_register_simplified_term<T>(
        &self,
        node: AstNodeRef<T>,
        term: TermId,
    ) -> TcResult<TermId> {
        // Copy location before so that validation errors point to the right location:
        self.copy_location_from_node_to_target(node, term);

        let simplified_term_id = self.validator().validate_term(term)?.simplified_term_id;

        // Check if there is already an entry for this node in the node info store.
        self.node_info_store().update_or_insert(node.id(), simplified_term_id.into());

        Ok(simplified_term_id)
    }
}

/// Implementation of [AstVisitor] for [TcVisitor], to traverse the AST
/// and type it.
///
/// Notes:
/// - Terms derived from expressions are always validated, in order to ensure
///   they are correct. The same goes for types.
impl<'tc> AstVisitor for TcVisitor<'tc> {
    type Error = TcError;

    type TupleLitEntryRet = Arg;

    fn visit_tuple_lit_entry(
        &self,
        node: AstNodeRef<ast::TupleLitEntry>,
    ) -> Result<Self::TupleLitEntryRet, Self::Error> {
        let walk::TupleLitEntry { name, value, ty } = walk::walk_tuple_lit_entry(self, node)?;

        let ty_or_unresolved = ty.unwrap_or_else(|| self.builder().create_unresolved_term());
        let value_ty = self.typer().infer_ty_of_term(value)?;
        self.register_node_info_and_location(node.value.ast_ref(), value_ty);

        // Check that the type of the value and the type annotation match and then apply
        // the substitution onto ty
        let ty_sub = self.unifier().unify_terms(value_ty, ty_or_unresolved)?;
        let value = self.substituter().apply_sub_to_term(&ty_sub, value);

        Ok(Arg { name, value })
    }

    type RefExprRet = TermId;

    fn visit_ref_expr(
        &self,
        node: AstNodeRef<ast::RefExpr>,
    ) -> Result<Self::RefExprRet, Self::Error> {
        // @@Todo: ref kind
        let walk::RefExpr { inner_expr, mutability, kind: _ } = walk::walk_ref_expr(self, node)?;

        // Depending on the `mutability` of the reference, create the relevant type
        // function application
        let ref_def = if mutability.is_some() {
            self.core_defs().reference_mut_ty_fn()
        } else {
            self.core_defs().reference_ty_fn()
        };

        let builder = self.builder();

        // Create either `RefMut<T>` or `Ref<T>`
        let ref_args =
            builder.create_args([builder.create_arg("T", inner_expr)], ParamOrigin::TyFn);
        let ref_ty = builder.create_app_ty_fn_term(ref_def, ref_args);

        let term = builder.create_rt_term(ref_ty);

        self.validate_and_register_simplified_term(node, term)
    }

    type BreakStatementRet = TermId;

    fn visit_break_statement(
        &self,
        node: AstNodeRef<ast::BreakStatement>,
    ) -> Result<Self::BreakStatementRet, Self::Error> {
        let builder = self.builder();
        let term = builder.create_void_term();
        self.validate_and_register_simplified_term(node, term)
    }

    type FloatLitRet = TermId;

    fn visit_float_lit(
        &self,
        node: AstNodeRef<ast::FloatLit>,
    ) -> Result<Self::FloatLitRet, Self::Error> {
        let f32_def = self.core_defs().f32_ty();
        let term = self.builder().create_rt_term(f32_def);
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type BodyBlockRet = TermId;

    fn visit_body_block(
        &self,
        node: AstNodeRef<ast::BodyBlock>,
    ) -> Result<Self::BodyBlockRet, Self::Error> {
        // First create a new scope and assign it to the block node
        let scope = self.builder().create_scope(ScopeKind::Variable, []);
        self.register_node_info(node, scope);

        // Enter the scope and visit the block
        self.scope_manager().enter_scope(scope, |_| {
            // Traverse each statement
            for statement in node.statements.iter() {
                let statement_id = self.visit_expr(statement.ast_ref())?;
                self.validator().validate_term(statement_id)?;
                // @@Design: do we check that the return type is void? Should we
                // warn if it isn't?
            }

            // Traverse the ending expression, if any, or return void.
            let term = match &node.expr {
                Some(expr) => self.visit_expr(expr.ast_ref())?,
                None => self.builder().create_void_term(),
            };
            self.validate_and_register_simplified_term(node, term)
        })
    }

    type CharLitRet = TermId;

    fn visit_char_lit(
        &self,
        node: AstNodeRef<ast::CharLit>,
    ) -> Result<Self::CharLitRet, Self::Error> {
        let term = self.builder().create_lit_term(node.data);
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type LitRet = TermId;

    fn visit_lit(&self, node: AstNodeRef<Lit>) -> Result<Self::LitRet, Self::Error> {
        walk::walk_lit_same_children(self, node)
    }

    type UnaryExprRet = TermId;

    fn visit_unary_expr(
        &self,
        node: AstNodeRef<ast::UnaryExpr>,
    ) -> Result<Self::UnaryExprRet, Self::Error> {
        let walk::UnaryExpr { expr, .. } = walk::walk_unary_expr(self, node)?;

        // @@Hack: currently, trait resolution logic is broken, so we introduce
        // a workaround for primitives to support all unary operators on primitive
        // types.
        let expr_ty = self.typer().infer_ty_of_term(expr)?;

        if self.oracle().term_is_primitive(expr_ty) {
            let term = self.term_store().get(expr);
            let term = self.builder().create_term(term);

            return self.validate_and_register_simplified_term(node, term);
        }

        let operator_fn = |trait_fn_name: &str| {
            let prop_access = self.builder().create_prop_access(expr, trait_fn_name);
            self.copy_location_from_node_to_target(node.operator.ast_ref(), prop_access);

            let builder = self.builder();
            builder.create_fn_call_term(prop_access, builder.create_args([], ParamOrigin::Fn))
        };

        // Convert the unary operator into a method call on the `expr`
        let term = match node.operator.body() {
            UnOp::BitNot => operator_fn("bit_not"),
            UnOp::Not => operator_fn("not"),
            UnOp::Neg => operator_fn("neg"),
            UnOp::TypeOf => self.builder().create_ty_of_term(expr),
        };

        self.validate_and_register_simplified_term(node, term)
    }

    type SetTyRet = TermId;

    fn visit_set_ty(&self, node: AstNodeRef<ast::SetTy>) -> Result<Self::SetTyRet, Self::Error> {
        let walk::SetTy { inner } = walk::walk_set_ty(self, node)?;

        let inner_ty = self.core_defs().set_ty_fn();
        let builder = self.builder();

        let term = builder.create_app_ty_fn_term(
            inner_ty,
            builder.create_args([builder.create_arg("T", inner)], ParamOrigin::TyFn),
        );

        self.validate_and_register_simplified_term(node, term)
    }

    type NamedTyRet = TermId;

    fn visit_named_ty(
        &self,
        node: AstNodeRef<ast::NamedTy>,
    ) -> Result<Self::NamedTyRet, Self::Error> {
        let walk::NamedTy { name } = walk::walk_named_ty(self, node)?;

        // Infer type if it is an underscore:
        let term = if name == IDENTS.underscore {
            self.builder().create_unresolved_term()
        } else {
            self.builder().create_var_term(name)
        };

        self.validate_and_register_simplified_term(node, term)
    }

    type ContinueStatementRet = TermId;

    fn visit_continue_statement(
        &self,
        node: AstNodeRef<ast::ContinueStatement>,
    ) -> Result<Self::ContinueStatementRet, Self::Error> {
        let builder = self.builder();
        let term = builder.create_void_term();
        self.validate_and_register_simplified_term(node, term)
    }

    type ListTyRet = TermId;

    fn visit_list_ty(&self, node: AstNodeRef<ast::ListTy>) -> Result<Self::ListTyRet, Self::Error> {
        let walk::ListTy { inner } = walk::walk_list_ty(self, node)?;

        let inner_ty = self.core_defs().list_ty_fn();
        let builder = self.builder();

        let term = builder.create_app_ty_fn_term(
            inner_ty,
            builder.create_args([builder.create_arg("T", inner)], ParamOrigin::TyFn),
        );

        self.validate_and_register_simplified_term(node, term)
    }

    type OrPatRet = PatId;

    fn visit_or_pat(&self, node: AstNodeRef<ast::OrPat>) -> Result<Self::OrPatRet, Self::Error> {
        let walk::OrPat { variants } = walk::walk_or_pat(self, node)?;
        let pat = self.builder().create_or_pat(variants);
        self.register_node_info_and_location(node, pat);
        Ok(pat)
    }

    type AssignOpExprRet = TermId;

    fn visit_assign_op_expr(
        &self,
        node: AstNodeRef<ast::AssignOpExpr>,
    ) -> Result<Self::AssignOpExprRet, Self::Error> {
        debug_assert!(node.operator.is_re_assignable());

        let rhs = self.visit_expr(node.rhs.ast_ref())?;

        let name = match node.lhs.body() {
            ast::Expr::Variable(name) => name.name.ident,
            // @@Incomplete: what about tuples, or array indices?
            _ => {
                return Err(TcError::InvalidAssignSubject {
                    location: self.source_location_at_node(node).into(),
                });
            }
        };

        let var_term = self.builder().create_var_term(name);
        self.copy_location_from_node_to_target(node, var_term);

        let ScopeMember { member, scope_id, index } =
            self.scope_manager().resolve_name_in_scopes(name, var_term)?;

        // We want to create the type that represents this operation, and then
        // ensure that it type checks...
        let ty = self.create_operator_fn(var_term, rhs, node.operator.ast_ref(), true);
        let _ = self.validate_and_register_simplified_term(node, ty)?;

        // Now check that the declared local item is declared as mutable

        let site: LocationTarget = self.source_location_at_node(node).into();

        if matches!(member.mutability(), Mutability::Immutable) {
            self.diagnostics().add_error(TcError::MemberIsImmutable {
                name: member.name(),
                site,
                decl: (scope_id, index),
            })
        }

        let term = self.builder().create_void_term();
        self.validate_and_register_simplified_term(node, term)
    }

    type TuplePatRet = PatId;

    fn visit_tuple_pat(
        &self,
        node: AstNodeRef<ast::TuplePat>,
    ) -> Result<Self::TuplePatRet, Self::Error> {
        let walk::TuplePat { mut fields, spread } = walk::walk_tuple_pat(self, node)?;

        // @@Hack: if we have a spread pattern present, then we will insert it
        // at the specified index.
        if let Some(spread_pat) = spread && let Some(spread_node) = &node.spread {
            fields.insert(spread_node.position, PatArg { pat: spread_pat, name: None })
        }

        let members = self.builder().create_pat_args(fields, ParamOrigin::Tuple);
        self.copy_location_from_nodes_to_targets(node.fields.ast_ref_iter(), members);
        let tuple_pat = self.builder().create_tuple_pat(members);

        self.copy_location_from_node_to_target(node, tuple_pat);
        self.validator().validate_tuple_pat(members)?;
        self.register_node_info_and_location(node, tuple_pat);
        Ok(tuple_pat)
    }

    type ListLitRet = TermId;

    fn visit_list_lit(
        &self,
        node: AstNodeRef<ast::ListLit>,
    ) -> Result<Self::ListLitRet, Self::Error> {
        let walk::ListLit { elements } = walk::walk_list_lit(self, node)?;

        let list_inner_ty = self.core_defs().list_ty_fn();
        let element_ty = self.unifier().unify_rt_term_sequence(elements)?;

        let builder = self.builder();
        let list_ty = builder.create_app_ty_fn_term(
            list_inner_ty,
            builder.create_args([builder.create_arg("T", element_ty)], ParamOrigin::TyFn),
        );

        let term = builder.create_rt_term(list_ty);
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type TyExprRet = TermId;

    fn visit_ty_expr(&self, node: AstNodeRef<ast::TyExpr>) -> Result<Self::TyExprRet, Self::Error> {
        Ok(walk::walk_ty_expr(self, node)?.ty)
    }

    type LitExprRet = TermId;

    fn visit_lit_expr(
        &self,
        node: AstNodeRef<ast::LitExpr>,
    ) -> Result<Self::LitExprRet, Self::Error> {
        let walk::LitExpr { data } = walk::walk_lit_expr(self, node)?;

        self.register_node_info_and_location(node, data);
        Ok(data)
    }

    type ModDefRet = TermId;

    fn visit_mod_def(&self, node: AstNodeRef<ast::ModDef>) -> Result<Self::ModDefRet, Self::Error> {
        // create a scope for the module definition
        let VisitConstantScope { scope_name, scope_id, .. } =
            self.visit_constant_scope(node.block.members(), node, None, ScopeKind::Mod)?;

        // @@Todo: bound variables
        let mod_def = self.builder().create_mod_def(scope_name, ModDefOrigin::Mod, scope_id);
        let term = self.builder().create_mod_def_term(mod_def);

        // Validate the definition
        let is_within_intrinsics = self.state.applied_directives().is_intrinsics();
        self.validator().validate_mod_def(mod_def, term, is_within_intrinsics)?;

        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type VariableExprRet = TermId;

    fn visit_variable_expr(
        &self,
        node: AstNodeRef<ast::VariableExpr>,
    ) -> Result<Self::VariableExprRet, Self::Error> {
        let walk::VariableExpr { name } = walk::walk_variable_expr(self, node)?;
        let term = self.builder().create_var_term(name);
        self.validate_and_register_simplified_term(node, term)
    }

    type LoopBlockRet = TermId;

    fn visit_loop_block(
        &self,
        node: AstNodeRef<ast::LoopBlock>,
    ) -> Result<Self::LoopBlockRet, Self::Error> {
        let walk::LoopBlock { contents: _ } = walk::walk_loop_block(self, node)?;
        let void_term = self.builder().create_void_term();
        self.validate_and_register_simplified_term(node, void_term)
    }

    type PropertyKindRet = Field;

    fn visit_property_kind(
        &self,
        node: AstNodeRef<ast::PropertyKind>,
    ) -> Result<Self::PropertyKindRet, Self::Error> {
        Ok(match node.body() {
            ast::PropertyKind::NamedField(name) => Field::Named(*name),
            ast::PropertyKind::NumericField(index) => Field::Numeric(*index),
        })
    }

    type ConstructorPatRet = PatId;

    fn visit_constructor_pat(
        &self,
        node: AstNodeRef<ast::ConstructorPat>,
    ) -> Result<Self::ConstructorPatRet, Self::Error> {
        let walk::ConstructorPat { mut fields, subject, spread } =
            walk::walk_constructor_pat(self, node)?;

        // @@Hack: if we have a spread pattern present, then we will insert it
        // at the specified index.
        if let Some(spread_pat) = spread && let Some(spread_node) = &node.spread {
            fields.insert(spread_node.position, PatArg { pat: spread_pat, name: None })
        }

        let constructor_params =
            self.builder().create_pat_args(fields, ParamOrigin::ConstructorPat);
        self.copy_location_from_nodes_to_targets(node.fields.ast_ref_iter(), constructor_params);

        let subject = self.typer().get_term_of_pat(subject)?;
        let simplified = self.simplifier().potentially_simplify_term(subject)?;

        let constructor_pat = self.builder().create_constructor_pat(simplified, constructor_params);

        self.copy_location_from_node_to_target(node, constructor_pat);
        self.validator().validate_constructor_pat(constructor_pat)?;
        self.register_node_info_and_location(node, constructor_pat);
        Ok(constructor_pat)
    }

    type TupleTyRet = TermId;

    fn visit_tuple_ty(
        &self,
        node: AstNodeRef<ast::TupleTy>,
    ) -> Result<Self::TupleTyRet, Self::Error> {
        let walk::TupleTy { entries } = walk::walk_tuple_ty(self, node)?;

        let members = self.builder().create_params(entries, ParamOrigin::Tuple);
        self.copy_location_from_nodes_to_targets(node.entries.ast_ref_iter(), members);
        let term = self.builder().create_tuple_ty_term(members);

        self.validate_and_register_simplified_term(node, term)
    }

    type ImplDefRet = TermId;

    fn visit_impl_def(
        &self,
        node: AstNodeRef<ast::ImplDef>,
    ) -> Result<Self::ImplDefRet, Self::Error> {
        // create a scope for the module definition
        let VisitConstantScope { scope_name, scope_id, .. } =
            self.visit_constant_scope(node.block.members(), node, None, ScopeKind::Impl)?;

        // @@Todo: bound variables
        let mod_def = self.builder().create_mod_def(scope_name, ModDefOrigin::AnonImpl, scope_id);
        let term = self.builder().create_mod_def_term(mod_def);

        // Validate the definition
        self.validator().validate_mod_def(mod_def, term, false)?;

        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type CastExprRet = TermId;

    fn visit_cast_expr(
        &self,
        node: AstNodeRef<ast::CastExpr>,
    ) -> Result<Self::CastExprRet, Self::Error> {
        let walk::CastExpr { expr, ty } = walk::walk_cast_expr(self, node)?;
        let expr_ty = self.typer().infer_ty_of_term(expr)?;

        // Ensure that the `expr` can be unified with the provided `ty`...
        let sub = self.unifier().unify_terms(expr_ty, ty)?;

        // Return Rt(..) of the ty:
        let term = self.substituter().apply_sub_to_term(&sub, self.builder().create_rt_term(ty));
        self.validate_and_register_simplified_term(node, term)
    }

    type AccessTyRet = TermId;

    fn visit_access_ty(
        &self,
        node: AstNodeRef<ast::AccessTy>,
    ) -> Result<Self::AccessTyRet, Self::Error> {
        let walk::AccessTy { subject, property } = walk::walk_access_ty(self, node)?;
        let term = self.builder().create_access(subject, property, AccessOp::Namespace);
        self.validate_and_register_simplified_term(node, term)
    }

    type WildPatRet = PatId;

    fn visit_wild_pat(
        &self,
        node: AstNodeRef<ast::WildPat>,
    ) -> Result<Self::WildPatRet, Self::Error> {
        let pat = self.builder().create_wildcard_pat();
        self.register_node_info_and_location(node, pat);
        Ok(pat)
    }

    type ModulePatEntryRet = PatArg;

    fn visit_module_pat_entry(
        &self,
        node: AstNodeRef<ast::ModulePatEntry>,
    ) -> Result<Self::ModulePatEntryRet, Self::Error> {
        let walk::ModulePatEntry { name, pat } = walk::walk_module_pat_entry(self, node)?;
        Ok(self.builder().create_pat_arg(name, pat))
    }

    type ListPatRet = PatId;

    fn visit_list_pat(
        &self,
        node: AstNodeRef<ast::ListPat>,
    ) -> Result<Self::ListPatRet, Self::Error> {
        let walk::ListPat { mut fields, spread } = walk::walk_list_pat(self, node)?;

        // We need to collect all of the terms within the inner pattern, but we need
        // have a special case for `spread patterns` because they will return `[term]`
        // rather than `term`...
        let inner_terms = fields
            .iter()
            .zip(node.fields.iter())
            .map(|(element, _)| -> TcResult<TermId> { self.typer().get_term_of_pat(*element) })
            .collect::<TcResult<Vec<_>>>()?;

        let list_term = self.unifier().unify_rt_term_sequence(inner_terms)?;

        // @@Hack: if we have a spread pattern present, then we will insert it
        // at the specified index.
        if let Some(spread_pat) = spread && let Some(spread_node) = &node.spread {
            fields.insert(spread_node.position, spread_pat)
        }

        let members = self.builder().create_pat_args(
            fields.into_iter().map(|pat| PatArg { name: None, pat }),
            ParamOrigin::ListPat,
        );

        let list_pat = self.builder().create_list_pat(list_term, members);

        self.copy_location_from_nodes_to_targets(node.fields.ast_ref_iter(), members);

        self.register_node_info_and_location(node, list_pat);
        Ok(list_pat)
    }

    type TyRet = TermId;

    fn visit_ty(&self, node: AstNodeRef<ast::Ty>) -> Result<Self::TyRet, Self::Error> {
        walk::walk_ty_same_children(self, node)
    }

    type DeclarationRet = TermId;

    fn visit_declaration(
        &self,
        node: AstNodeRef<ast::Declaration>,
    ) -> Result<Self::DeclarationRet, Self::Error> {
        let pat_id = self.visit_pat(node.pat.ast_ref())?;
        let pat = self.reader().get_pat(pat_id);

        // Set the declaration hit if it is just a binding pattern:
        if let Pat::Binding(BindingPat { name, .. }) = pat {
            self.state.declaration_name_hint.set(Some(name));
        };

        let ty = node.ty.as_ref().map(|t| self.visit_ty(t.ast_ref())).transpose()?;
        let value = node.value.as_ref().map(|t| self.visit_expr(t.ast_ref())).transpose()?;

        // Clear the declaration hint
        self.state.declaration_name_hint.take();

        let ty_or_unresolved = self.builder().or_unresolved_term(ty);

        // Unify the type of the declaration with the type of the value of the
        // declaration.
        let sub = if let Some(value) = value {
            let ty_of_value = self.typer().infer_ty_of_term(value)?;
            self.unifier().unify_terms(ty_of_value, ty_or_unresolved)?
        } else {
            Sub::empty()
        };

        // Apply the substitution on the type and value
        let mut value = value.map(|value| self.substituter().apply_sub_to_term(&sub, value));
        let ty = self.substituter().apply_sub_to_term(&sub, ty_or_unresolved);

        if value.is_none() && self.state.applied_directives().is_intrinsics() {
            // @@Todo: see #391
            value = Some(self.builder().create_rt_term(ty));
        }

        // Get the declaration member(s)
        let members = match value {
            Some(value) => {
                // If there is a value, match it with the pattern and acquire the members to add
                // to the scope.
                let members = match self.pat_matcher().match_pat_with_term(pat_id, value)? {
                    Some(members) => members,

                    None => {
                        self.diagnostics().add_warning(TcWarning::UselessMatchCase {
                            pat: pat_id,
                            subject: value,
                        });
                        vec![]
                    }
                };

                // Ensure that the given pattern is irrefutable given the type of the term
                self.exhaustiveness_checker().is_pat_irrefutable(&[pat_id], ty, None)?;
                members
            }
            None => {
                let kind = self.scope_manager().current_scope_kind();

                // Verify that we are in a trait definition if it has no value, as all other
                // cases are invalid.
                if !matches!(kind, ScopeKind::Trait) {
                    return Err(TcError::UninitialisedMemberNotAllowed {
                        member: self.source_location_at_node(node).into(),
                    });
                }

                // If the member is a binding, and within a trait definition we will allow
                // to become an `uninitialised constant`
                if let Pat::Binding(BindingPat { name, mutability, visibility }) = pat {
                    // Disallow constant members being declared as immutable...
                    if mutability == Mutability::Mutable {
                        self.diagnostics().add_error(TcError::MemberMustBeImmutable {
                            name,
                            site: self.source_location_at_node(node.pat.ast_ref()).into(),
                        });
                    }

                    vec![Member::uninitialised_constant(name, visibility, ty)]
                } else {
                    // If there is no value, one cannot use pattern matching!
                    return Err(TcError::CannotPatMatchWithoutAssignment { pat: pat_id });
                }
            }
        };

        // Add the members to scope:
        let current_scope_id = self.scopes().current_scope();
        let member_indexes = members
            .iter()
            .map(|member| {
                self.scope_store().modify_fast(current_scope_id, |scope| scope.add(*member))
            })
            .collect::<Vec<_>>();

        // Add the locations of all members:
        for member_idx in &member_indexes {
            let member_span = iter::once(node.pat.span())
                .chain(node.ty.as_ref().map(|ty| ty.span()))
                .chain(node.value.as_ref().map(|value| value.span()))
                .reduce(|acc, span| acc.join(span))
                .unwrap();

            self.location_store().add_location_to_target(
                (current_scope_id, *member_idx),
                self.source_location(member_span),
            );
        }

        // Declaration should return its value if any:
        let term = match value {
            Some(value) => value,
            // Void if no value:
            None => self.builder().create_void_term(),
        };

        self.validate_and_register_simplified_term(node, term)
    }

    type StructDefRet = TermId;

    fn visit_struct_def(
        &self,
        node: AstNodeRef<ast::StructDef>,
    ) -> Result<Self::StructDefRet, Self::Error> {
        let walk::StructDef { fields, .. } = walk::walk_struct_def(self, node)?;

        // create the params
        let fields = self.builder().create_params(fields, ParamOrigin::Struct);

        // add the location of each parameter
        self.copy_location_from_nodes_to_targets(node.fields.ast_ref_iter(), fields);

        // take the declaration hint here...
        let name = self.state.declaration_name_hint.take();

        let builder = self.builder();
        let nominal_id = builder.create_struct_def(name, fields);
        let term = builder.create_nominal_def_term(nominal_id);

        // validate the constructed nominal def
        self.validator().validate_nominal_def(nominal_id)?;

        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type IfPatRet = PatId;

    fn visit_if_pat(&self, node: AstNodeRef<ast::IfPat>) -> Result<Self::IfPatRet, Self::Error> {
        let walk::IfPat { condition, pat } = walk::walk_if_pat(self, node)?;
        let pat = self.builder().create_if_pat(pat, condition);
        self.register_node_info_and_location(node, pat);
        Ok(pat)
    }

    type StrLitRet = TermId;

    fn visit_str_lit(&self, node: AstNodeRef<ast::StrLit>) -> Result<Self::StrLitRet, Self::Error> {
        let term = self.builder().create_lit_term(node.data.to_string());
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type TraitDefRet = TermId;

    fn visit_trait_def(
        &self,
        node: AstNodeRef<ast::TraitDef>,
    ) -> Result<Self::TraitDefRet, Self::Error> {
        // create a scope for the module definition
        let VisitConstantScope { scope_name, scope_id, .. } =
            self.visit_constant_scope(node.members.ast_ref_iter(), node, None, ScopeKind::Trait)?;

        // @@Todo: bound variables
        let trt_def = self.builder().create_trt_def(scope_name, scope_id);
        let term = self.builder().create_trt_term(trt_def);

        // Validate the definition
        self.validator().validate_trt_def(trt_def)?;

        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type ParamRet = Param;

    fn visit_param(&self, node: AstNodeRef<ast::Param>) -> Result<Self::ParamRet, Self::Error> {
        match node.origin {
            ParamOrigin::Struct | ParamOrigin::Fn | ParamOrigin::EnumVariant => {
                self.visit_fn_or_struct_param(node)
            }
            ParamOrigin::TyFn => self.visit_ty_fn_param(node),
            // Any other `origin` does not occur within `Param`
            _ => unreachable!(),
        }
    }

    type ReturnStatementRet = TermId;

    fn visit_return_statement(
        &self,
        node: AstNodeRef<ast::ReturnStatement>,
    ) -> Result<Self::ReturnStatementRet, Self::Error> {
        let walk::ReturnStatement { expr } = walk::walk_return_statement(self, node)?;

        // Add the given return value's type to the return type hint.
        // Return term is either given or void.
        let return_term = expr.unwrap_or_else(|| {
            let builder = self.builder();
            let term = builder.create_void_term();
            self.copy_location_from_node_to_target(node, term);
            term
        });

        let return_ty = self.typer().infer_ty_of_term(return_term)?;
        let already_given_return_ty = self
            .state
            .fn_def_return_ty
            .get()
            .unwrap_or_else(|| self.builder().create_unresolved_term());

        let return_ty_sub = self.unifier().unify_terms(return_ty, already_given_return_ty)?;
        let unified_return_ty =
            self.substituter().apply_sub_to_term(&return_ty_sub, already_given_return_ty);
        self.state.fn_def_return_ty.set(Some(unified_return_ty));

        // Return never as the return expression shouldn't evaluate to anything.
        let never_term = self.builder().create_never_ty();
        let term = self.builder().create_rt_term(never_term);

        self.validate_and_register_simplified_term(node, term)
    }

    type FnDefRet = TermId;

    fn visit_fn_def(&self, node: AstNodeRef<ast::FnDef>) -> Result<Self::FnDefRet, Self::Error> {
        // @@Temporary: try and get the name of the function declaration
        // if possible, in the event of the lhs of the declaration being
        // more complicated than `foo := ...`, we will need to figure
        // out a way of getting the name of the declaration.
        let name = self.state.declaration_name_hint.get();

        let params: Vec<_> =
            node.params.iter().map(|a| self.visit_param(a.ast_ref())).collect::<TcResult<_>>()?;

        let return_ty = node.return_ty.as_ref().map(|t| self.visit_ty(t.ast_ref())).transpose()?;

        // Set return type to hint if given
        let old_return_ty = self.state.fn_def_return_ty.replace(return_ty);
        let params_potentially_unresolved = self.builder().create_params(params, ParamOrigin::Fn);
        let param_scope = self.scope_manager().make_rt_param_scope(params_potentially_unresolved);

        // Set the function scope to the parameter scope:
        self.register_node_info(node, param_scope);

        let (params, return_ty, return_value) =
            self.scope_manager().enter_scope(param_scope, |_| {
                let fn_body = self.visit_expr(node.fn_body.ast_ref())?;

                let hint_return_ty = self.state.fn_def_return_ty.get();
                let return_ty_or_unresolved = self.builder().or_unresolved_term(hint_return_ty);

                let body_sub = {
                    let ty_of_body = self.typer().infer_ty_of_term(fn_body)?;
                    match hint_return_ty {
                        Some(_) => {
                            // Try to unify ty_of_body with void, and if so, then ty of
                            // body should be unresolved:
                            let void = self.builder().create_void_ty_term();
                            match self.unifier().unify_terms(ty_of_body, void) {
                                Ok(_) => Sub::empty(),
                                Err(_) => {
                                    // Must be returning the same type:
                                    self.unifier()
                                        .unify_terms(ty_of_body, return_ty_or_unresolved)?
                                }
                            }
                        }
                        None => self.unifier().unify_terms(ty_of_body, return_ty_or_unresolved)?,
                    }
                };

                let return_value = self.substituter().apply_sub_to_term(&body_sub, fn_body);
                let return_ty =
                    self.substituter().apply_sub_to_term(&body_sub, return_ty_or_unresolved);
                let params = self
                    .substituter()
                    .apply_sub_to_params(&body_sub, params_potentially_unresolved);

                // Add all the locations to the parameters
                self.copy_location_from_nodes_to_targets(node.params.ast_ref_iter(), params);

                Ok((params, return_ty, return_value))
            })?;

        let builder = self.builder();

        let fn_ty_term = builder.create_fn_lit_term(
            name,
            builder.create_fn_ty_term(name, params, return_ty),
            return_value,
        );

        // Clear return type
        self.state.fn_def_return_ty.set(old_return_ty);
        let fn_ty_term = self.validate_and_register_simplified_term(node, fn_ty_term)?;

        // If the module is an entry point, and if the function
        // is either named "main", or the `#entry_point` attribute
        // is specified, then we specify that the entry point is
        // now this term...
        if !self.workspace().yields_executable(self.settings()) {
            return Ok(fn_ty_term);
        }

        let maybe_module = { self.eps().module() };

        if let Some(module) = maybe_module && let Some(name) = name {
            if self.local_storage().current_source() != SourceId::from(module) {
                return Ok(fn_ty_term);
            }

            if name == IDENTS.main || self.state.applied_directives().is_entry_point() {
                println!("Found entry point: {name}");
                if self.local_storage().current_source() == SourceId::from(module) {
                    let kind = if self.state.applied_directives().is_entry_point() {
                        EntryPointKind::Named(name)
                    } else {
                        EntryPointKind::Main
                    };

                    let result = {
                        // We specify that this function is the entry point
                        self.eps_mut().set(fn_ty_term, kind)
                    };

                    // If the entry point was declared twice, then we 
                    // return an error.
                    result.ok_or_else(|| {
                        TcError::MultipleEntryPoints {
                            site: self.eps().def.unwrap().into(),
                            duplicate_site: fn_ty_term.into()
                        }
                    })?;
                }
            }
        }

        Ok(fn_ty_term)
    }

    type BoolLitRet = TermId;

    fn visit_bool_lit(
        &self,
        node: AstNodeRef<ast::BoolLit>,
    ) -> Result<Self::BoolLitRet, Self::Error> {
        let term =
            self.builder().create_var_term(if node.data { IDENTS.r#true } else { IDENTS.r#false });
        self.validate_and_register_simplified_term(node, term)
    }

    type MergeTyRet = TermId;

    fn visit_merge_ty(
        &self,
        node: AstNodeRef<ast::MergeTy>,
    ) -> Result<Self::MergeTyRet, Self::Error> {
        let walk::MergeTy { lhs, rhs } = walk::walk_merge_ty(self, node)?;
        let merge_term = self.builder().create_merge_term(vec![lhs, rhs]);
        self.validate_and_register_simplified_term(node, merge_term)
    }

    type IfClauseRet = TermId;

    fn visit_if_clause(
        &self,
        node: AstNodeRef<ast::IfClause>,
    ) -> Result<Self::IfClauseRet, Self::Error> {
        panic_on_span!(
            self.source_location_at_node(node),
            self.source_map(),
            "hit non de-sugared if-clause whilst performing typechecking"
        )
    }

    type TyFnRet = TermId;

    fn visit_ty_fn(&self, node: AstNodeRef<ast::TyFn>) -> Result<Self::TyFnRet, Self::Error> {
        let params = node
            .params
            .iter()
            .map(|a| self.visit_param(a.ast_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        let params = self.builder().create_params(params, ParamOrigin::TyFn);

        let param_scope = self.scope_manager().make_bound_scope(params);
        let return_value = self
            .scope_manager()
            .enter_scope(param_scope, |_| self.visit_ty(node.return_ty.ast_ref()))?;

        // Add all the locations to the parameters:
        self.copy_location_from_nodes_to_targets(node.params.ast_ref_iter(), params);

        // Create the type function type term:
        let ty_fn_ty_term = self.builder().create_ty_fn_ty_term(params, return_value);

        self.validate_and_register_simplified_term(node, ty_fn_ty_term)
    }

    type RefKindRet = RefKind;

    fn visit_ref_kind(&self, node: AstNodeRef<RefKind>) -> Result<Self::RefKindRet, Self::Error> {
        Ok(*node.body())
    }

    type IntLitRet = TermId;

    fn visit_int_lit(&self, node: AstNodeRef<ast::IntLit>) -> Result<Self::IntLitRet, Self::Error> {
        let term = self.builder().create_lit_term(*node.body());
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type RangePatRet = PatId;
    fn visit_range_pat(
        &self,
        node: AstNodeRef<ast::RangePat>,
    ) -> Result<Self::RangePatRet, Self::Error> {
        let walk::RangePat { lo, hi } = walk::walk_range_pat(self, node)?;

        let range_pat = RangePat { lo, hi, end: node.body().end };
        let pat = self.builder().create_range_pat(range_pat);

        self.copy_location_from_node_to_target(node, pat);
        self.validator().validate_range_pat(&range_pat)?;
        self.register_node_info_and_location(node, pat);
        Ok(pat)
    }

    type MapLitRet = TermId;

    fn visit_map_lit(&self, node: AstNodeRef<ast::MapLit>) -> Result<Self::MapLitRet, Self::Error> {
        let walk::MapLit { elements } = walk::walk_map_lit(self, node)?;
        let map_inner_ty = self.core_defs().map_ty_fn();

        // Unify the key and value types...
        let key_ty = self.unifier().unify_rt_term_sequence(elements.iter().map(|(k, _)| *k))?;
        let val_ty = self.unifier().unify_rt_term_sequence(elements.iter().map(|(v, _)| *v))?;

        let builder = self.builder();
        let map_ty = builder.create_app_ty_fn_term(
            map_inner_ty,
            builder.create_args(
                [builder.create_arg("K", key_ty), builder.create_arg("V", val_ty)],
                ParamOrigin::TyFn,
            ),
        );

        let term = builder.create_rt_term(map_ty);
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type ConstructorCallExprRet = TermId;

    fn visit_constructor_call_expr(
        &self,
        node: AstNodeRef<ast::ConstructorCallExpr>,
    ) -> Result<Self::ConstructorCallExprRet, Self::Error> {
        let walk::ConstructorCallExpr { args, subject } =
            walk::walk_constructor_call_expr(self, node)?;

        // Create the args with origin as `FnCall`, although it might be altered
        // when the term is simplified into `Struct` or `Enum` variant.
        let args = self.builder().create_args(args, ParamOrigin::FnCall);
        self.copy_location_from_nodes_to_targets(node.args.ast_ref_iter(), args);

        // Create the function call term:
        let return_term = self.builder().create_fn_call_term(subject, args);

        self.validate_and_register_simplified_term(node, return_term)
    }

    type RefTyRet = TermId;
    fn visit_ref_ty(&self, node: AstNodeRef<ast::RefTy>) -> Result<Self::RefTyRet, Self::Error> {
        let walk::RefTy { inner, mutability, kind } = walk::walk_ref_ty(self, node)?;

        // Either mutable or immutable, raw or normal, depending on what was given:
        let ref_def = match (kind, mutability) {
            // Immutable, normal, by default:
            (Some(RefKind::Normal) | None, None | Some(Mutability::Immutable)) => {
                self.core_defs().reference_ty_fn()
            }
            (Some(RefKind::Raw), None | Some(Mutability::Immutable)) => {
                self.core_defs().raw_reference_ty_fn()
            }
            (Some(RefKind::Normal) | None, Some(Mutability::Mutable)) => {
                self.core_defs().reference_mut_ty_fn()
            }
            (Some(RefKind::Raw), Some(Mutability::Mutable)) => {
                self.core_defs().raw_reference_mut_ty_fn()
            }
        };
        let builder = self.builder();
        let ref_args = builder.create_args([builder.create_arg("T", inner)], ParamOrigin::TyFn);
        let ref_ty = builder.create_app_ty_fn_term(ref_def, ref_args);

        // Add locations:
        self.copy_location_from_node_to_target(node.inner.ast_ref(), (ref_args, 0));

        self.validate_and_register_simplified_term(node, ref_ty)
    }

    type AccessKindRet = AccessOp;

    fn visit_access_kind(
        &self,
        node: AstNodeRef<AccessKind>,
    ) -> Result<Self::AccessKindRet, Self::Error> {
        match node.body() {
            AccessKind::Namespace => Ok(AccessOp::Namespace),
            AccessKind::Property => Ok(AccessOp::Property),
        }
    }

    type SetLitRet = TermId;

    fn visit_set_lit(&self, node: AstNodeRef<ast::SetLit>) -> Result<Self::SetLitRet, Self::Error> {
        let walk::SetLit { elements } = walk::walk_set_lit(self, node)?;

        let set_inner_ty = self.core_defs().set_ty_fn();
        let element_ty = self.unifier().unify_rt_term_sequence(elements)?;

        let builder = self.builder();
        let set_ty = builder.create_app_ty_fn_term(
            set_inner_ty,
            builder.create_args([builder.create_arg("T", element_ty)], ParamOrigin::TyFn),
        );

        let term = builder.create_rt_term(set_ty);
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type ForLoopBlockRet = TermId;

    fn visit_for_loop_block(
        &self,
        node: AstNodeRef<ast::ForLoopBlock>,
    ) -> Result<Self::ForLoopBlockRet, Self::Error> {
        panic_on_span!(
            self.source_location_at_node(node),
            self.source_map(),
            "hit non de-sugared for-block whilst performing typechecking"
        )
    }

    type TyFnDefRet = TermId;

    fn visit_ty_fn_def(
        &self,
        node: AstNodeRef<ast::TyFnDef>,
    ) -> Result<Self::TyFnDefRet, Self::Error> {
        let declaration_hint = self.state.declaration_name_hint.take();

        // Traverse the parameters:
        let param_elements = node
            .params
            .iter()
            .map(|t| self.visit_param(t.ast_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        let params = self.builder().create_params(param_elements, ParamOrigin::TyFn);

        // Add all the locations to the parameters:
        self.copy_location_from_nodes_to_targets(node.params.ast_ref_iter(), params);

        // Enter parameter scope:
        let param_scope = self.scope_manager().make_bound_scope(params);
        self.scope_manager().enter_scope(param_scope, |_| {
            // Traverse return type and return value:
            let return_ty =
                node.return_ty.as_ref().map(|t| self.visit_ty(t.ast_ref())).transpose()?;
            let body = self.visit_expr(node.ty_fn_body.ast_ref())?;

            // Create the type function type term:
            let ty_fn_return_ty = self.builder().or_unresolved_term(return_ty);
            let ty_of_ty_fn_return_value = self.typer().infer_ty_of_term(body)?;
            let return_ty_sub =
                self.unifier().unify_terms(ty_of_ty_fn_return_value, ty_fn_return_ty)?;
            let ty_fn_return_ty =
                self.substituter().apply_sub_to_term(&return_ty_sub, ty_fn_return_ty);
            let ty_fn_return_value = self.substituter().apply_sub_to_term(&return_ty_sub, body);

            let ty_fn_term = self.builder().create_ty_fn_term(
                declaration_hint,
                params,
                ty_fn_return_ty,
                ty_fn_return_value,
            );

            self.validate_and_register_simplified_term(node, ty_fn_term)
        })
    }

    type AccessExprRet = TermId;

    fn visit_access_expr(
        &self,
        node: AstNodeRef<ast::AccessExpr>,
    ) -> Result<Self::AccessExprRet, Self::Error> {
        let walk::AccessExpr { subject, property, kind } = walk::walk_access_expr(self, node)?;
        let term = self.builder().create_access(subject, property, kind);
        self.validate_and_register_simplified_term(node, term)
    }

    type PatRet = PatId;

    fn visit_pat(&self, node: AstNodeRef<ast::Pat>) -> Result<Self::PatRet, Self::Error> {
        walk::walk_pat_same_children(self, node)
    }

    type ImportRet = TermId;

    fn visit_import(&self, node: AstNodeRef<ast::Import>) -> Result<Self::ImportRet, Self::Error> {
        // Try resolve the path of the import to a SourceId:
        let source_id = self.source_map().get_id_by_path(&node.resolved_path);
        match source_id {
            Some(id) => {
                // Resolve the ModDef corresponding to the SourceId if it exists:
                match self.checked_sources().source_mod_def(id) {
                    Some(already_checked_mod_term) => {
                        // Already exists, meaning this module has been checked before:
                        Ok(already_checked_mod_term)
                    }
                    None => {
                        // Module has not been checked before, time to check it:

                        // First, create a new storage reference for the child module:
                        let node_map = self.node_map;
                        let storage_ref = self.storages();
                        let child_local_storage = LocalStorage::new(storage_ref.global_storage, id);
                        let child_storage_ref =
                            StorageRef { local_storage: &child_local_storage, ..storage_ref };

                        // Visit the child module
                        let child_visitor = TcVisitor::new_in_source(child_storage_ref, node_map);
                        let module_term = child_visitor.visit_source()?;
                        Ok(module_term)
                    }
                }
            }
            None => {
                // This should never happen because all modules should have been parsed by now.
                let unresolved_import_term = self.builder().create_unresolved_term();
                self.register_node_info_and_location(node, unresolved_import_term);
                tc_panic!(
                    unresolved_import_term,
                    self,
                    "Found import path that hasn't been resolved to a module!"
                )
            }
        }
    }

    type EnumDefEntryRet = EnumVariant;

    fn visit_enum_def_entry(
        &self,
        node: AstNodeRef<ast::EnumDefEntry>,
    ) -> Result<Self::EnumDefEntryRet, Self::Error> {
        let walk::EnumDefEntry { name, fields } = walk::walk_enum_def_entry(self, node)?;

        // Create the enum variant parameters
        let fields = if fields.is_empty() {
            None
        } else {
            Some(self.builder().create_params(
                fields.iter().map(|field| Param {
                    name: field.name,
                    ty: field.ty,
                    default_value: field.default_value,
                }),
                ParamOrigin::EnumVariant,
            ))
        };

        Ok(EnumVariant { name, fields })
    }

    type BindingPatRet = PatId;

    fn visit_binding_pat(
        &self,
        node: AstNodeRef<ast::BindingPat>,
    ) -> Result<Self::BindingPatRet, Self::Error> {
        let name = node.name.ident;
        let var_term = self.builder().create_var_term(name);

        if let Ok(scope_member) = self.scope_manager().resolve_name_in_scopes(name, var_term) {
            let term = scope_member.member.value_or_ty();
            let ty = scope_member.member.ty();

            // So this should only become a constant if we are a `enum` or `struct`
            // definition. Otherwise, we shadow the variable
            if self.oracle().term_is_struct_def(term) || self.oracle().term_is_enum_def(term) {
                self.location_store().copy_location(ty, var_term);
                let pat = self.builder().create_pat(Pat::Const(ConstPat { term: var_term }));

                self.register_node_info_and_location(node, pat);
                return Ok(pat);
            }
        }

        let pat = self.builder().create_binding_pat(
            node.name.body().ident,
            match node.mutability.as_ref().map(|x| *x.body()) {
                Some(ast::Mutability::Mutable) => Mutability::Mutable,
                Some(ast::Mutability::Immutable) | None => Mutability::Immutable,
            },
            match node.visibility.as_ref().map(|x| *x.body()) {
                Some(ast::Visibility::Private) | None => Visibility::Private,
                Some(ast::Visibility::Public) => Visibility::Public,
            },
        );

        // Copy the location of this node to the pat_id
        self.register_node_info_and_location(node, pat);
        Ok(pat)
    }

    type BlockExprRet = TermId;

    fn visit_block_expr(
        &self,
        node: AstNodeRef<ast::BlockExpr>,
    ) -> Result<Self::BlockExprRet, Self::Error> {
        let walk::BlockExpr { data } = walk::walk_block_expr(self, node)?;

        // register the term with this block expression:
        self.validate_and_register_simplified_term(node, data)
    }

    type ImportExprRet = TermId;

    fn visit_import_expr(
        &self,
        node: AstNodeRef<ast::ImportExpr>,
    ) -> Result<Self::ImportExprRet, Self::Error> {
        Ok(walk::walk_import_expr(self, node)?.data)
    }

    type BinaryExprRet = TermId;

    fn visit_binary_expr(
        &self,
        node: AstNodeRef<ast::BinaryExpr>,
    ) -> Result<Self::BinaryExprRet, Self::Error> {
        let walk::BinaryExpr { lhs, rhs, .. } = walk::walk_binary_expr(self, node)?;

        // @@Hack: currently, trait resolution logic is broken, so we introduce
        // a workaround for primitives to support all operators so that we can
        // continue onto lowering...
        let lhs_term = self.typer().infer_ty_of_term(lhs)?;
        let rhs_term = self.typer().infer_ty_of_term(rhs)?;

        if self.oracle().term_is_primitive(lhs_term) && self.oracle().term_is_primitive(rhs_term) {
            if !self.unifier().terms_are_equal(lhs_term, rhs_term) {
                return Err(TcError::CannotUnify { src: rhs_term, target: lhs_term });
            }

            // If the operator is a logical operator, or comparator, then the result of the
            // expression will be a boolean term, so we return the type of the expression as
            // yielding a boolean term.
            let term = if node.operator.is_lazy() || node.operator.is_comparator() {
                self.builder().create_rt_term(self.core_defs().bool_ty())
            } else {
                let lhs_term = self.term_store().get(lhs);
                self.builder().create_term(lhs_term)
            };

            return self.validate_and_register_simplified_term(node, term);
        }

        let term = if matches!(node.operator.body(), BinOp::Merge) {
            self.builder().create_merge_term([lhs, rhs])
        } else if node.operator.is_lazy() {
            self.create_lazy_operator_fn(lhs, rhs, *node.operator.body())?
        } else {
            self.create_operator_fn(lhs, rhs, node.operator.ast_ref(), false)
        };

        self.validate_and_register_simplified_term(node, term)
    }

    type FnTyRet = TermId;

    fn visit_fn_ty(&self, node: AstNodeRef<ast::FnTy>) -> Result<Self::FnTyRet, Self::Error> {
        let walk::FnTy { params, return_ty } = walk::walk_fn_ty(self, node)?;
        let params = self.builder().create_params(params, ParamOrigin::Fn);

        // Add all the locations to the parameters:
        self.copy_location_from_nodes_to_targets(node.params.ast_ref_iter(), params);

        // Create the function type term:
        let fn_ty_term = self.builder().create_fn_ty_term(None, params, return_ty);

        self.validate_and_register_simplified_term(node, fn_ty_term)
    }

    type TyFnCallRet = TermId;
    fn visit_ty_fn_call(
        &self,
        node: AstNodeRef<ast::TyFnCall>,
    ) -> Result<Self::TyFnCallRet, Self::Error> {
        let walk::TyFnCall { args, subject } = walk::walk_ty_fn_call(self, node)?;

        // These should be converted to args
        let args = self.builder().create_args(
            args.iter().map(|param_arg| Arg { name: param_arg.name, value: param_arg.ty }),
            ParamOrigin::TyFn,
        );

        // Add all the locations to the args:
        self.copy_location_from_nodes_to_targets(node.args.ast_ref_iter(), args);

        // Create the type function call term:
        let app_ty_fn_term = self.builder().create_app_ty_fn_term(subject, args);

        self.validate_and_register_simplified_term(node, app_ty_fn_term)
    }

    type LitPatRet = PatId;

    fn visit_lit_pat(&self, node: AstNodeRef<ast::LitPat>) -> Result<Self::LitPatRet, Self::Error> {
        let walk::LitPat { data } = walk::walk_lit_pat(self, node)?;

        let pat = match node.body().data.body() {
            Lit::Bool(_) => self.builder().create_constant_pat(data),
            _ => self.builder().create_lit_pat(data),
        };

        self.register_node_info_and_location(node, pat);
        Ok(pat)
    }

    type AccessPatRet = PatId;

    fn visit_access_pat(
        &self,
        node: AstNodeRef<ast::AccessPat>,
    ) -> Result<Self::AccessPatRet, Self::Error> {
        let walk::AccessPat { subject, property } = walk::walk_access_pat(self, node)?;
        let access_pat = self.builder().create_access_pat(subject, property);
        self.register_node_info_and_location(node, access_pat);
        Ok(access_pat)
    }

    type AssignExprRet = TermId;

    fn visit_assign_expr(
        &self,
        node: AstNodeRef<ast::AssignExpr>,
    ) -> Result<Self::AssignExprRet, Self::Error> {
        let rhs = self.visit_expr(node.rhs.ast_ref())?;
        let site: LocationTarget = self.source_location_at_node(node).into();

        // Try to resolve the variable in scopes; if it is not found, it is an error. If
        // it is found, then set it to its new term.
        let name = match node.lhs.body() {
            ast::Expr::Variable(name) => name.name.ident,
            _ => {
                return Err(TcError::InvalidAssignSubject { location: site });
            }
        };

        let var_term = self.builder().create_var_term(name);
        self.copy_location_from_node_to_target(node, var_term);
        let scope_item = self.scope_manager().resolve_name_in_scopes(name, var_term)?;

        // Set the value to the member:
        self.scope_manager().assign_member(scope_item.scope_id, scope_item.index, rhs, site)?;

        let term = self.builder().create_void_term();
        self.validate_and_register_simplified_term(node, term)
    }

    type NameRet = Identifier;
    fn visit_name(&self, node: AstNodeRef<ast::Name>) -> Result<Self::NameRet, Self::Error> {
        Ok(node.ident)
    }

    type IfBlockRet = TermId;

    fn visit_if_block(
        &self,
        node: AstNodeRef<ast::IfBlock>,
    ) -> Result<Self::IfBlockRet, Self::Error> {
        panic_on_span!(
            self.source_location_at_node(node),
            self.source_map(),
            "hit non de-sugared if-block whilst performing typechecking"
        )
    }

    type WhileLoopBlockRet = TermId;

    fn visit_while_loop_block(
        &self,
        node: AstNodeRef<ast::WhileLoopBlock>,
    ) -> Result<Self::WhileLoopBlockRet, Self::Error> {
        panic_on_span!(
            self.source_location_at_node(node),
            self.source_map(),
            "hit non de-sugared while-block whilst performing typechecking"
        )
    }

    type ConstructorCallArgRet = Arg;

    fn visit_constructor_call_arg(
        &self,
        node: AstNodeRef<ast::ConstructorCallArg>,
    ) -> Result<Self::ConstructorCallArgRet, Self::Error> {
        let walk::ConstructorCallArg { name, value } = walk::walk_constructor_call_arg(self, node)?;
        Ok(Arg { name, value })
    }

    type TuplePatEntryRet = PatArg;

    fn visit_tuple_pat_entry(
        &self,
        node: AstNodeRef<ast::TuplePatEntry>,
    ) -> Result<Self::TuplePatEntryRet, Self::Error> {
        // Here we set the `in_pat_fields` as true since we're currently within
        // some kind of pattern fields...
        let walk::TuplePatEntry { name, pat } = walk::walk_tuple_pat_entry(self, node)?;

        Ok(PatArg { name, pat })
    }

    type TyArgRet = Param;

    fn visit_ty_arg(&self, node: AstNodeRef<ast::TyArg>) -> Result<Self::TyArgRet, Self::Error> {
        let walk::TyArg { ty, name } = walk::walk_ty_arg(self, node)?;
        self.register_node_info_and_location(node, ty);
        Ok(Param { ty, name, default_value: None })
    }

    type UnionTyRet = TermId;

    fn visit_union_ty(
        &self,
        node: AstNodeRef<ast::UnionTy>,
    ) -> Result<Self::UnionTyRet, Self::Error> {
        let walk::UnionTy { lhs, rhs } = walk::walk_union_ty(self, node)?;
        let union_term = self.builder().create_union_term(vec![lhs, rhs]);
        self.validate_and_register_simplified_term(node, union_term)
    }

    type ModuleRet = TermId;

    fn visit_module(&self, node: AstNodeRef<ast::Module>) -> Result<Self::ModuleRet, Self::Error> {
        let id = self.local_storage().current_source();

        // Decide on whether we are creating a new scope, or if we are going to use the
        // global scope. If we're within the `prelude` module, we need to append
        // all members into the global scope rather than creating a new scope
        let members = if let Some(ModuleKind::Prelude) = self.source_map().module_kind_by_id(id) {
            self.root_scope()
        } else {
            self.builder().create_scope(ScopeKind::Mod, vec![])
        };

        // Get the end of the filename for the module and use this as the name of the
        // module
        let name = self.source_map().source_name(id).to_owned();
        let VisitConstantScope { scope_id, .. } = self.visit_constant_scope(
            node.contents.ast_ref_iter(),
            node,
            Some(members),
            ScopeKind::Mod,
        )?;

        let mod_def = self.builder().create_named_mod_def(name, ModDefOrigin::Source(id), scope_id);

        let term = self.builder().create_mod_def_term(mod_def);
        self.copy_location_from_node_to_target(node, term);
        self.validator().validate_mod_def(mod_def, term, false)?;

        // Add location to the term
        self.register_node_info_and_location(node, term);
        Ok(term)
    }

    type ModulePatRet = PatId;

    fn visit_module_pat(
        &self,
        node: AstNodeRef<ast::ModulePat>,
    ) -> Result<Self::ModulePatRet, Self::Error> {
        let walk::ModulePat { fields } = walk::walk_module_pat(self, node)?;
        let members = self.builder().create_pat_args(fields, ParamOrigin::ModulePat);
        let module_pat = self.builder().create_mod_pat(members);
        self.copy_location_from_nodes_to_targets(node.fields.ast_ref_iter(), members);
        self.register_node_info_and_location(node, module_pat);
        Ok(module_pat)
    }

    type BlockRet = TermId;

    fn visit_block(&self, node: AstNodeRef<ast::Block>) -> Result<Self::BlockRet, Self::Error> {
        walk::walk_block_same_children(self, node)
    }

    type UnsafeExprRet = TermId;

    fn visit_unsafe_expr(
        &self,
        node: AstNodeRef<ast::UnsafeExpr>,
    ) -> Result<Self::UnsafeExprRet, Self::Error> {
        // @@UnsafeExpr: Decide on what to do with unsafe expressions, but for now walk
        // the inner types...
        let walk::UnsafeExpr { data } = walk::walk_unsafe_expr(self, node)?;
        Ok(data)
    }

    type MapTyRet = TermId;

    fn visit_map_ty(&self, node: AstNodeRef<ast::MapTy>) -> Result<Self::MapTyRet, Self::Error> {
        let walk::MapTy { key, value } = walk::walk_map_ty(self, node)?;

        let inner_ty = self.core_defs().map_ty_fn();
        let builder = self.builder();

        let term = builder.create_app_ty_fn_term(
            inner_ty,
            builder.create_args(
                [builder.create_arg("K", key), builder.create_arg("V", value)],
                ParamOrigin::TyFn,
            ),
        );

        self.validate_and_register_simplified_term(node, term)
    }

    type MatchBlockRet = TermId;

    fn visit_match_block(
        &self,
        node: AstNodeRef<ast::MatchBlock>,
    ) -> Result<Self::MatchBlockRet, Self::Error> {
        let walk::MatchBlock { subject, .. } = walk::walk_match_block(self, node)?;

        let mut branches = vec![];

        let match_return_values: Vec<_> = node
            .cases
            .ast_ref_iter()
            .map(|case| {
                // Try to match the pattern with the case
                let case_pat = self.visit_pat(case.pat.ast_ref())?;
                branches.push(case_pat);

                let case_match = self.pat_matcher().match_pat_with_term(case_pat, subject)?;
                match case_match {
                    Some(members_to_add) => {
                        // Enter a new scope and add the members
                        let match_case_scope =
                            self.builder().create_scope(ScopeKind::Variable, members_to_add);

                        // Assign the scope to the match case
                        self.register_node_info(case, match_case_scope);

                        self.scope_manager().enter_scope(match_case_scope, |_| {
                            // Traverse the body with the bound variables:
                            let case_body = self.visit_expr(case.expr.ast_ref())?;
                            Ok(Some(case_body))
                        })
                    }
                    None => {
                        // Emit warning for the useless match case in the event that the pattern
                        // will never match with the subject since we know the types will never
                        // correspond.
                        self.diagnostics()
                            .add_warning(TcWarning::UselessMatchCase { pat: case_pat, subject });

                        Ok(None)
                    }
                }
            })
            .flatten_ok()
            .collect::<TcResult<_>>()?;

        let ty_of_subject = self.typer().infer_ty_of_term(subject)?;

        // Skip origins of `while` and `if` since they are always irrefutable, if the
        // origin is a match, we want to check call `is_match_exhaustive` since it will
        // generate a more relevant error, otherwise we call `is_pat_irrefutable`
        match &node.body().origin {
            MatchOrigin::If | MatchOrigin::While => {}
            MatchOrigin::Match => {
                self.exhaustiveness_checker().is_match_exhaustive(&branches, subject)?
            }
            origin @ MatchOrigin::For => self.exhaustiveness_checker().is_pat_irrefutable(
                &branches,
                ty_of_subject,
                Some(*origin),
            )?,
        }

        let match_return_types: Vec<_> = match_return_values
            .iter()
            .copied()
            .map(|value| self.typer().infer_ty_of_term(value))
            .collect::<TcResult<_>>()?;
        let return_ty = self.builder().create_union_term(match_return_types);
        let return_term = self.builder().create_rt_term(return_ty);

        self.validate_and_register_simplified_term(node, return_term)
    }

    type VisibilityRet = Visibility;

    fn visit_visibility(
        &self,
        node: AstNodeRef<ast::Visibility>,
    ) -> Result<Self::VisibilityRet, Self::Error> {
        Ok(match node.body() {
            ast::Visibility::Public => Visibility::Public,
            ast::Visibility::Private => Visibility::Private,
        })
    }

    type TraitImplRet = TermId;

    fn visit_trait_impl(
        &self,
        node: AstNodeRef<ast::TraitImpl>,
    ) -> Result<Self::TraitImplRet, Self::Error> {
        let trt_term = self.visit_ty(node.ty.ast_ref())?;

        // create a scope for the module definition
        let VisitConstantScope { scope_name, scope_id, .. } =
            self.visit_constant_scope(node.trait_body.ast_ref_iter(), node, None, ScopeKind::Impl)?;

        // @@Todo: bound variables
        let mod_def =
            self.builder().create_mod_def(scope_name, ModDefOrigin::TrtImpl(trt_term), scope_id);
        let term = self.builder().create_mod_def_term(mod_def);

        // Validate the definition
        self.register_node_info_and_location(node.ty.ast_ref(), term);
        self.validator().validate_mod_def(mod_def, term, false)?;

        Ok(term)
    }

    type TupleLitRet = TermId;

    fn visit_tuple_lit(
        &self,
        node: AstNodeRef<ast::TupleLit>,
    ) -> Result<Self::TupleLitRet, Self::Error> {
        let walk::TupleLit { elements } = walk::walk_tuple_lit(self, node)?;
        let builder = self.builder();

        let params = builder.create_args(elements, ParamOrigin::Tuple);
        let term = builder.create_tuple_lit_term(params);

        // add the location of each parameter, and the term, to the location storage
        self.copy_location_from_nodes_to_targets(node.elements.ast_ref_iter(), params);
        self.register_node_info_and_location(node, term);

        Ok(term)
    }

    type IndexExprRet = TermId;

    fn visit_index_expr(
        &self,
        node: AstNodeRef<ast::IndexExpr>,
    ) -> Result<Self::IndexExprRet, Self::Error> {
        let walk::IndexExpr { .. } = walk::walk_index_expr(self, node)?;

        // We just translate this to a function call:
        // let builder = self.builder();
        // let index_fn_call_args =
        //     builder.create_args([builder.create_nameless_arg(index_expr)],
        // ParamOrigin::Fn); let index_fn_call_subject =
        // builder.create_prop_access(subject, "index"); let index_fn_call =
        // builder.create_fn_call_term(index_fn_call_subject, index_fn_call_args);

        // // Add locations:
        // self.copy_location_from_node_to_target(node, index_fn_call);
        // self.copy_location_from_node_to_target(node.subject.ast_ref(),
        // index_fn_call_subject); self.copy_location_from_node_to_target(node.
        // index_expr.ast_ref(), (index_fn_call_args, 0));

        // @@FixMe: we just get the inner element of the subject since we want to return
        //          the type of the inner element.
        let ty = self.builder().create_rt_term(self.core_defs().i32_ty());
        self.validate_and_register_simplified_term(node, ty)
    }

    type MatchCaseRet = ();

    fn visit_match_case(
        &self,
        _: AstNodeRef<ast::MatchCase>,
    ) -> Result<Self::MatchCaseRet, Self::Error> {
        // Handled in match
        Ok(())
    }

    type MutabilityRet = Mutability;

    fn visit_mutability(
        &self,
        node: AstNodeRef<ast::Mutability>,
    ) -> Result<Self::MutabilityRet, Self::Error> {
        Ok(match node.body() {
            ast::Mutability::Mutable => Mutability::Mutable,
            ast::Mutability::Immutable => Mutability::Immutable,
        })
    }

    type EnumDefRet = TermId;

    fn visit_enum_def(
        &self,
        node: AstNodeRef<ast::EnumDef>,
    ) -> Result<Self::EnumDefRet, Self::Error> {
        let walk::EnumDef { entries, .. } = walk::walk_enum_def(self, node)?;

        // take the declaration hint here...
        let name = self.state.declaration_name_hint.take();

        let builder = self.builder();
        let enum_def = builder.create_nominal_def_term(builder.create_enum_def(name, entries));

        self.validate_and_register_simplified_term(node, enum_def)
    }

    type BinOpRet = ();
    fn visit_bin_op(&self, _node: AstNodeRef<BinOp>) -> Result<Self::BinOpRet, Self::Error> {
        Ok(())
    }

    type MergeDeclarationRet = TermId;

    fn visit_merge_declaration(
        &self,
        node: AstNodeRef<ast::MergeDeclaration>,
    ) -> Result<Self::MergeDeclarationRet, Self::Error> {
        let walk::MergeDeclaration { decl, value } = walk::walk_merge_declaration(self, node)?;

        let merge_term = self.builder().create_merge_term(vec![decl, value]);

        // Add location
        self.copy_location_from_node_to_target(node, merge_term);
        Ok(self.validator().validate_term(merge_term)?.simplified_term_id)
    }

    type UnOpRet = ();

    fn visit_un_op(&self, _node: AstNodeRef<UnOp>) -> Result<Self::UnOpRet, Self::Error> {
        Ok(())
    }

    type DerefExprRet = TermId;

    fn visit_deref_expr(
        &self,
        node: AstNodeRef<ast::DerefExpr>,
    ) -> Result<Self::DerefExprRet, Self::Error> {
        let walk::DerefExpr { data } = walk::walk_deref_expr(self, node)?;
        let inner_ty = self.typer().infer_ty_of_term(data)?;

        // Create a `Ref<T>` dummy type for unification...
        let ap_ref_ty = {
            let ref_ty = self.core_defs().reference_ty_fn();
            let builder = self.builder();

            builder.create_app_ty_fn_term(
                ref_ty,
                builder.create_args(
                    [builder.create_arg("T", builder.create_unresolved_term())],
                    ParamOrigin::TyFn,
                ),
            )
        };

        // Create a `RefMut<T>` dummy type for unification...
        let ap_ref_mut_ty = {
            let ref_mut_ty = self.core_defs().reference_mut_ty_fn();
            let builder = self.builder();

            builder.create_app_ty_fn_term(
                ref_mut_ty,
                builder.create_args(
                    [builder.create_arg("T", builder.create_unresolved_term())],
                    ParamOrigin::TyFn,
                ),
            )
        };

        // Attempt to unify this with a `Ref<T>` to see if the `inner_ty` can
        // be de-referenced. If that fails, then try to unify with `RefMut<T>`
        //
        // @@Todo(feds01): Add a custom error type that says the desired type cannot be
        //                 de-referenced.
        let result = self
            .unifier()
            .unify_terms(inner_ty, ap_ref_ty)
            .or_else(|_| self.unifier().unify_terms(inner_ty, ap_ref_mut_ty))?;

        // get the substitution result and use that as the return from the `inner_ty`
        let inner_ty = result.pairs().next().unwrap().1;

        let term = self.builder().create_rt_term(inner_ty);

        self.validate_and_register_simplified_term(node, term)
    }

    type MapLitEntryRet = (TermId, TermId);

    fn visit_map_lit_entry(
        &self,
        node: AstNodeRef<ast::MapLitEntry>,
    ) -> Result<Self::MapLitEntryRet, Self::Error> {
        let walk::MapLitEntry { key, value } = walk::walk_map_lit_entry(self, node)?;

        Ok((key, value))
    }

    type SpreadPatRet = PatId;

    fn visit_spread_pat(
        &self,
        node: AstNodeRef<ast::SpreadPat>,
    ) -> Result<Self::SpreadPatRet, Self::Error> {
        let walk::SpreadPat { name } = walk::walk_spread_pat(self, node)?;

        let spread_pat = self.builder().create_pat(Pat::Spread(SpreadPat { name }));

        self.register_node_info_and_location(node, spread_pat);
        Ok(spread_pat)
    }

    type DirectiveExprRet = TermId;

    fn visit_directive_expr(
        &self,
        node: AstNodeRef<ast::DirectiveExpr>,
    ) -> Result<Self::DirectiveExprRet, Self::Error> {
        let old_applied_directives = self.state.applied_directives.get();

        // If the current specified directive is `intrinsics`, then we have to enable
        // the flag `within_intrinsics_directive` which changes the way that `mod`
        // blocks are validated and changes the parsing of the declarations inside the
        // mod block.
        for directive in &node.directives {
            if directive.is(IDENTS.intrinsics) {
                self.state.applied_directives.update(|a| a | AppliedDirectives::INTRINSICS);
            }

            if directive.is(IDENTS.entry_point) {
                self.state.applied_directives.update(|a| a | AppliedDirectives::ENTRY_POINT);
            }
        }

        // @@Directives: Decide on what to do with directives, but for now walk the
        // inner types...
        let walk::DirectiveExpr { subject, .. } = walk::walk_directive_expr(self, node)?;

        self.state.applied_directives.set(old_applied_directives);

        self.register_node_info_and_location(node, subject);
        Ok(subject)
    }

    type ExprRet = TermId;

    fn visit_expr(&self, node: AstNodeRef<ast::Expr>) -> Result<Self::ExprRet, Self::Error> {
        let term_id = walk::walk_expr_same_children(self, node)?;
        // Since this is an expression, we want to coerce the term to a value if
        // applicable:
        Ok(self.coercing().potentially_coerce_as_value(term_id, node))
    }
}
