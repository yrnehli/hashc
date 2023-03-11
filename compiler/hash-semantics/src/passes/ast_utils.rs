use hash_ast::{
    ast::{self, AstNodeRef, BodyBlock, Module, OwnsAstNode},
    node_map::SourceRef,
};
use hash_source::location::{SourceLocation, Span};
use hash_tir::{symbols::Symbol, utils::common::CommonUtils};

use crate::{diagnostics::error::SemanticResult, environment::sem_env::AccessToSemEnv};

pub trait AstPass: AccessToSemEnv {
    fn pass_interactive(&self, node: AstNodeRef<BodyBlock>) -> SemanticResult<()>;
    fn pass_module(&self, node: AstNodeRef<Module>) -> SemanticResult<()>;

    /// Called before the pass starts, returns `true` if the pass should go
    /// ahead
    fn pre_pass(&self) -> SemanticResult<bool> {
        Ok(true)
    }

    /// Called after the pass has finished
    fn post_pass(&self) -> SemanticResult<()> {
        Ok(())
    }

    fn pass_source(&self) -> SemanticResult<()> {
        if self.pre_pass()? {
            let source = self.node_map().get_source(self.current_source_info().source_id());
            match source {
                SourceRef::Interactive(interactive_source) => {
                    self.pass_interactive(interactive_source.node_ref())?
                }
                SourceRef::Module(module_source) => self.pass_module(module_source.node_ref())?,
            };
            self.post_pass()?;
            Ok(())
        } else {
            Ok(())
        }
    }
}

pub trait AstUtils: AccessToSemEnv {
    /// Create a [SourceLocation] from a [Span].
    fn source_location(&self, span: Span) -> SourceLocation {
        SourceLocation { span, id: self.current_source_info().source_id() }
    }

    /// Create a [SourceLocation] at the given [hash_ast::ast::AstNode].
    fn node_location<N>(&self, node: AstNodeRef<N>) -> SourceLocation {
        let node_span = node.span();
        self.source_location(node_span)
    }

    /// Create a [`Symbol`] for the given [`ast::Name`], or a fresh symbol if no
    /// name is provided.
    fn new_symbol_from_ast_name(&self, name: Option<&ast::AstNode<ast::Name>>) -> Symbol {
        match name {
            Some(name) => self.new_symbol(name.ident),
            None => self.new_fresh_symbol(),
        }
    }
}

impl<T: AccessToSemEnv> AstUtils for T {}