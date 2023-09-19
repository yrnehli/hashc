//! Definitions related to symbols and names.

use std::fmt::Display;

use hash_source::identifier::{Identifier, IDENTS};
use hash_storage::{
    get,
    store::{statics::StoreId, StoreKey},
};

use crate::{
    stores::tir_stores,
    tir::{Node, NodeOrigin},
    tir_node_single_store,
};

/// The data carried by a symbol.
///
/// For each context, each distinct member in the context will be given a
/// different `SymbolId`. `SymbolId`s are never "shadowed" like names in a scope
/// stack might; new ones are always created for names that might shadow
/// previous names.
///
/// For example:
/// ```notrust
/// {
///     foo := 3 // -- SymbolId(72)
///     {
///         foo := 4 // -- SymbolId(73)
///     }
/// }
/// ```
///
///
/// This is used to avoid needing to perform alpha-conversion on terms when
/// performing substitutions.
#[derive(Debug, Clone, Copy)]
pub struct Symbol {
    /// A symbol might originate from an identifier name.
    ///
    /// If this is `None`, then the symbol is "internal"/generated by the
    /// compiler, and cannot be referenced by the user.
    pub name: Option<Identifier>,
}

tir_node_single_store!(Symbol);

impl SymbolId {
    /// Create a new symbol from a name.
    pub fn from_name(name: impl Into<Identifier>, origin: NodeOrigin) -> Self {
        Node::create_at(Symbol { name: Some(name.into()) }, origin)
    }

    /// Create a new symbol without a name.
    pub fn fresh(origin: NodeOrigin) -> Self {
        Node::create_at(Symbol { name: None }, origin)
    }

    /// Create a new symbol with the same name as this one.
    pub fn duplicate(&self, origin: NodeOrigin) -> SymbolId {
        let name = self.borrow().name;
        Node::create_at(Symbol { name }, origin)
    }

    /// Create a new symbol with an `_` as its name.
    pub fn fresh_underscore(origin: NodeOrigin) -> Self {
        Node::create_at(Symbol { name: Some(IDENTS.underscore) }, origin)
    }

    /// Get an [Identifier] name for this symbol. If the symbol does not have a
    /// name, then a `_` is returned.
    pub fn ident(&self) -> Identifier {
        match self.borrow().name {
            Some(name) => name,
            None => IDENTS.underscore,
        }
    }
}

impl Display for SymbolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match get!(*self, name) {
            Some(name) => write!(f, "{name}"),
            None => write!(f, "s{}", self.to_index()),
        }
    }
}