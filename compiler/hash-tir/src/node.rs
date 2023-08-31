use derive_more::{Deref, DerefMut};
use hash_ast::ast::AstNodeId;
use hash_source::{location::Span, SourceId};
use hash_storage::store::statics::SingleStoreValue;

use crate::ast_info::HasNodeId;

/// Represents a node in the TIR.
///
/// Each node has an origin, and data associated with it.
#[derive(Debug, Deref, DerefMut, Copy, Clone, PartialEq, Eq)]
pub struct Node<Data> {
    pub origin: NodeOrigin,
    #[deref]
    #[deref_mut]
    pub data: Data,
}

impl<Data> Node<Data>
where
    Self: SingleStoreValue,
{
    pub fn create_at(data: Data, origin: NodeOrigin) -> <Self as SingleStoreValue>::Id {
        Self::create(Self::at(data, origin))
    }

    pub fn create_gen(data: Data) -> <Self as SingleStoreValue>::Id {
        Self::create(Self::gen(data))
    }
}

impl<Data> Node<Data> {
    pub fn at(data: Data, origin: NodeOrigin) -> Self {
        Self { data, origin }
    }

    pub fn gen(data: Data) -> Self {
        Self { data, origin: NodeOrigin::Generated }
    }

    pub fn node(&self) -> Option<AstNodeId> {
        self.origin.node()
    }

    pub fn span(&self) -> Option<Span> {
        self.node().map(|n| n.span())
    }

    pub fn source(&self) -> Option<SourceId> {
        self.node().map(|n| n.source())
    }

    pub fn with_data<E>(&self, new_data: E) -> Node<E> {
        Node { data: new_data, origin: self.origin }
    }
}

impl<D, Data: From<D>> From<(D, NodeOrigin)> for Node<Data> {
    fn from((d, o): (D, NodeOrigin)) -> Self {
        Node::at(d.into(), o)
    }
}

impl<T> HasNodeId for Node<T> {
    fn node_id(&self) -> Option<AstNodeId> {
        match self.origin {
            NodeOrigin::Given(id) | NodeOrigin::InferredFrom(id) => Some(id),
            NodeOrigin::Generated => None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NodeOrigin {
    /// The node was given by the user.
    Given(AstNodeId),
    /// The node was created through type inference of the given origin.
    InferredFrom(AstNodeId),
    /// The node was generated by the compiler, and has no origin.
    Generated,
}

impl NodeOrigin {
    pub fn node(&self) -> Option<AstNodeId> {
        match self {
            NodeOrigin::Given(node) | NodeOrigin::InferredFrom(node) => Some(*node),
            NodeOrigin::Generated => None,
        }
    }
}
