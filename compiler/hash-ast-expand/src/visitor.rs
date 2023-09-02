//! This defines the Hash AST expansion pass. This pass is responsible for
//! dealing with all macro invocations, and performing various transformations
//! on the AST based on the kind of macro that was invoked. Specifically, this
//! pass will:
//!
//! - Deal with all `#[attr(...)]` invocations which set attributes on their
//!   subjects. The attributes are registered in the `ATTRIBUTE_MAP` which can
//!   then be later accessed by various other passes which need to know about
//!   the attributes.  Dealing with attributes also means that the pass will
//!   check that all applied attributes exist (if not a warning is emitted) and
//!   that they are applied to the correct kind of AST item. However, this pass
//!   is not responsible for checking that a specific kind of attribute has a
//!   sane or valid value, this is up to the pass that is responsible or cares
//!   about the attribute to check.
//!
//! - @@Future: Perform expansions on macro invocations. Depending on whether
//!   this is an AST level macro or a token level macro, the expansion will be
//!   different.

use std::convert::Infallible;

use hash_ast::{
    ast, ast_visitor_mut_self_default_impl,
    visitor::{walk_mut_self, AstVisitorMutSelf},
};
use hash_attrs::{checks::AttrChecker, target::AttrNode};
use hash_pipeline::settings::CompilerSettings;
use hash_source::{SourceId, SourceMap};
use hash_target::data_layout::TargetDataLayout;
use hash_utils::crossbeam_channel::Sender;

use crate::diagnostics::{ExpansionDiagnostic, ExpansionDiagnostics};

pub struct AstExpander<'ctx> {
    /// An attribute checker, used to check that attributes are being applied
    /// correctly. This is for checks that are more specific to the context
    /// of the attribute, and not just the attribute itself.
    pub checker: AttrChecker<'ctx>,

    /// Session settings.
    pub settings: &'ctx CompilerSettings,

    /// The sources.
    pub sources: &'ctx SourceMap,

    /// Any diagnostics that have been emitted during the expansion stage.
    pub diagnostics: ExpansionDiagnostics,
}

impl<'ctx> AstExpander<'ctx> {
    /// Create a new [AstExpander]. Contains the [SourceMap] and the
    /// current id of the source in reference.
    pub fn new(
        id: SourceId,
        sources: &'ctx SourceMap,
        settings: &'ctx CompilerSettings,
        data_layout: &'ctx TargetDataLayout,
    ) -> Self {
        Self {
            diagnostics: ExpansionDiagnostics::new(),
            checker: AttrChecker::new(id, data_layout),
            settings,
            sources,
        }
    }

    /// Emit all diagnostics that have been collected during the expansion
    /// stage.
    pub(crate) fn emit_diagnostics_to(self, sender: &Sender<ExpansionDiagnostic>) {
        self.diagnostics.warnings.into_iter().for_each(|d| sender.send(d.into()).unwrap());
        self.diagnostics.errors.into_iter().for_each(|d| sender.send(d.into()).unwrap());
    }
}

impl AstVisitorMutSelf for AstExpander<'_> {
    type Error = Infallible;

    ast_visitor_mut_self_default_impl!(hiding:
        ExprMacroInvocation, TyMacroInvocation, PatMacroInvocation,
        ExprArg, TyArg, PatArg, EnumDefEntry, Param, MatchCase, Module
    );

    type ExprMacroInvocationRet = ();

    fn visit_expr_macro_invocation(
        &mut self,
        node: ast::AstNodeRef<ast::ExprMacroInvocation>,
    ) -> Result<Self::ExprMacroInvocationRet, Self::Error> {
        let target = AttrNode::from_expr(node.subject.ast_ref());
        self.check_macro_invocations(node.macros.ast_ref(), target);

        walk_mut_self::walk_expr_macro_invocation(self, node)?;
        Ok(())
    }

    type TyMacroInvocationRet = ();

    fn visit_ty_macro_invocation(
        &mut self,
        node: ast::AstNodeRef<ast::TyMacroInvocation>,
    ) -> Result<Self::TyMacroInvocationRet, Self::Error> {
        let target = AttrNode::Ty(node.subject.ast_ref());
        self.check_macro_invocations(node.macros.ast_ref(), target);

        walk_mut_self::walk_ty_macro_invocation(self, node)?;
        Ok(())
    }

    type PatMacroInvocationRet = ();

    fn visit_pat_macro_invocation(
        &mut self,
        node: ast::AstNodeRef<ast::PatMacroInvocation>,
    ) -> Result<Self::PatMacroInvocationRet, Self::Error> {
        let target = AttrNode::Pat(node.subject.ast_ref());
        self.check_macro_invocations(node.macros.ast_ref(), target);

        walk_mut_self::walk_pat_macro_invocation(self, node)?;
        Ok(())
    }

    type PatArgRet = ();

    fn visit_pat_arg(
        &mut self,
        node: ast::AstNodeRef<ast::PatArg>,
    ) -> Result<Self::PatArgRet, Self::Error> {
        if let Some(macros) = node.body.macros.as_ref() {
            let target = AttrNode::PatArg(node);
            self.check_macro_invocations(macros.ast_ref(), target);
        }

        walk_mut_self::walk_pat_arg(self, node)?;
        Ok(())
    }

    type EnumDefEntryRet = ();

    fn visit_enum_def_entry(
        &mut self,
        node: ast::AstNodeRef<ast::EnumDefEntry>,
    ) -> Result<Self::EnumDefEntryRet, Self::Error> {
        if let Some(macros) = node.body.macros.as_ref() {
            let target = AttrNode::EnumVariant(node);
            self.check_macro_invocations(macros.ast_ref(), target);
        }

        walk_mut_self::walk_enum_def_entry(self, node)?;
        Ok(())
    }

    type ParamRet = ();

    fn visit_param(
        &mut self,
        node: ast::AstNodeRef<ast::Param>,
    ) -> Result<Self::ParamRet, Self::Error> {
        if let Some(macros) = node.body.macros.as_ref() {
            let target = AttrNode::Param(node);
            self.check_macro_invocations(macros.ast_ref(), target);
        }

        walk_mut_self::walk_param(self, node)?;
        Ok(())
    }

    type MatchCaseRet = ();

    fn visit_match_case(
        &mut self,
        node: ast::AstNodeRef<ast::MatchCase>,
    ) -> Result<Self::MatchCaseRet, Self::Error> {
        if let Some(macros) = node.body.macros.as_ref() {
            let target = AttrNode::MatchCase(node);
            self.check_macro_invocations(macros.ast_ref(), target);
        }

        walk_mut_self::walk_match_case(self, node)?;
        Ok(())
    }

    type TyArgRet = ();

    fn visit_ty_arg(
        &mut self,
        node: ast::AstNodeRef<ast::TyArg>,
    ) -> Result<Self::TyArgRet, Self::Error> {
        if let Some(macros) = node.body.macros.as_ref() {
            let target = AttrNode::TyArg(node);
            self.check_macro_invocations(macros.ast_ref(), target);
        }

        walk_mut_self::walk_ty_arg(self, node)?;
        Ok(())
    }

    type ExprArgRet = ();

    fn visit_expr_arg(
        &mut self,
        node: ast::AstNodeRef<ast::ExprArg>,
    ) -> Result<Self::ExprArgRet, Self::Error> {
        if let Some(macros) = node.body.macros.as_ref() {
            let target = AttrNode::ExprArg(node);
            self.check_macro_invocations(macros.ast_ref(), target);
        }
        walk_mut_self::walk_expr_arg(self, node)?;
        Ok(())
    }

    type ModuleRet = ();

    fn visit_module(
        &mut self,
        node: ast::AstNodeRef<ast::Module>,
    ) -> Result<Self::ModuleRet, Self::Error> {
        let target = AttrNode::Module(node);
        self.check_macro_invocations(node.macros.ast_ref(), target);

        // We don't walk the module because this is handled by the
        // expander walking each expression in the module.
        Ok(())
    }

    // type DirectiveExprRet = ();
    // fn visit_directive_expr(
    //     &mut self,
    //     node: hash_ast::ast::AstNodeRef<hash_ast::ast::DirectiveExpr>,
    // ) -> Result<Self::DirectiveExprRet, Self::Error> { let _ =
    //   walk_mut_self::walk_directive_expr(self, node);

    //     let mut write_tree = |index| {
    //         let ast_settings = self.settings.ast_settings();

    //         // We want to get the total span of the subject, so we must
    //         // include the span of the directives that come after the `dump_ast`
    // directive.         let directive_span = if let Some(directive) =
    // node.directives.get(index + 1) {             let directive:
    // &AstNode<Name> = directive; // @@RustBug: for some reason it can't infer the
    // type here, maybe `smallvec`
    // // related?             directive.span().join(node.subject.span())
    //         } else {
    //             node.subject.span()
    //         };

    //         stream_writeln!(
    //             self.stdout,
    //             "AST dump for {}",
    //             self.source_map.fmt_location(directive_span)
    //         );

    //         match ast_settings.dump_mode {
    //             AstDumpMode::Pretty => {
    //                 let mut printer = AstPrinter::new(&mut self.stdout);
    //                 printer.visit_expr(node.subject.ast_ref()).unwrap();

    //                 // @@Hack: terminate the line with a newline.
    //                 stream_writeln!(self.stdout, "");
    //             }
    //             AstDumpMode::Tree => {
    //                 let mut tree =
    // AstTreeGenerator.visit_expr(node.subject.ast_ref()).unwrap();

    //                 // Since this might be a non-singular directive, we also
    // might                 // need to wrap the tree in a any of the directives
    // that were specified                 // after the `dump_ast` directive.
    //                 for directive in node.directives.iter().skip(index + 1).rev()
    // {                     tree = TreeNode::branch(
    //                         format!("directive \"{}\"", directive.ident),
    //                         vec![tree],
    //                     );
    //                 }

    //                 stream_writeln!(
    //                     self.stdout,
    //                     "{}",
    //                     TreeWriter::new_with_config(
    //                         &tree,
    //
    // TreeWriterConfig::from_character_set(self.settings.character_set)
    //                     )
    //                 );
    //             }
    //         }
    //     };

    //     // for the `dump_ast` directive, we essentially "dump" the generated tree
    //     // that the parser created. We emit this tree regardless of whether or
    // not     // there will be errors later on in the compilation stage.
    //     for (index, directive) in node.directives.iter().enumerate() {
    //         if directive.is(IDENTS.dump_ast) {
    //             write_tree(index)
    //         }
    //     }

    //     Ok(())
    // }
}
