//! Representing and modifying the typechecking context.
use core::fmt;
use std::{cell::RefCell, convert::Infallible};

use indexmap::IndexMap;

use super::env::{AccessToEnv, WithEnv};
use crate::new::{
    data::DataDefId, defs::DefParamGroupId, fns::FnDefId, mods::ModDefId, params::ParamId,
    scopes::StackId, symbols::Symbol, trts::TrtDefId,
};
/// The kind of a binding.
#[derive(Debug, Clone, Copy)]
pub enum BindingKind {
    /// A binding that is a trait member.
    ///
    /// For example, `trait { y := 3; z := y }`
    TrtMember(BindingOrigin<TrtDefId, usize>),
    /// A binding that is a module member.
    ///
    /// For example, `mod { Q := struct(); Q }`
    ModMember(BindingOrigin<ModDefId, usize>),
    /// A binding that is a stack member.
    ///
    /// For example, `{ a := 3; a }`
    StackMember(BindingOrigin<StackId, usize>),
    /// A binding that is a constructor definition.
    ///
    /// For example, `false`, `None`, `Some(_)`.
    Ctor(BindingOrigin<DataDefId, usize>),
    /// A binding that represents a parameter variable of a function.
    ///
    /// For example, `(x: i32) => x`
    BoundVar(BindingOrigin<ScopeKind, usize>),
}

/// The origin of a binding, which consists of a definition (whatever it may be)
/// ID, and an index into that definition's "members".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindingOrigin<Id, Index>
where
    Id: fmt::Debug + Copy + Eq,
    Index: fmt::Debug + Copy + Eq,
{
    pub id: Id,
    pub index: Index,
}

/// All the different places a bound variable can originate from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundVarOrigin {
    /// Module parameter.
    Mod(ModDefId, DefParamGroupId, ParamId),
    /// Trait parameter.
    Trt(TrtDefId, DefParamGroupId, ParamId),
    /// Function parameter.
    Fn(FnDefId, ParamId),
    /// Data definition parameter.
    Data(DataDefId, DefParamGroupId, ParamId),
}

/// A binding.
///
/// A binding is essentially something in the form `a := b` in the current
/// context.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    /// The name of the binding.
    pub name: Symbol,
    /// The kind of the binding.
    pub kind: BindingKind,
}

/// All the different kinds of scope there are, and their associated data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// A module scope.
    Mod(ModDefId),
    /// A trait scope.
    Trt(TrtDefId),
    /// A stack scope.
    Stack(StackId),
    /// A function scope.
    Fn(FnDefId),
    /// A data definition.
    Data(DataDefId),
}

/// Data structure managing the typechecking context.
///
/// The context is a stack of scopes, each scope being a stack in itself.
///
/// The context is used to resolve symbols to their corresponding bindings, and
/// thus interpret their meaning. It can read and add [`Binding`]s, and can
/// enter and exit scopes.
#[derive(Debug, Clone)]
pub struct Context {
    scope_levels: RefCell<Vec<usize>>,
    members: RefCell<IndexMap<Symbol, Binding>>,
    scope_kinds: RefCell<Vec<ScopeKind>>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            scope_levels: RefCell::new(vec![]),
            members: RefCell::new(IndexMap::new()),
            scope_kinds: RefCell::new(vec![]),
        }
    }

    /// Enter a new scope in the context.
    pub fn add_scope(&self, kind: ScopeKind) {
        self.scope_kinds.borrow_mut().push(kind);
        self.scope_levels.borrow_mut().push(self.members.borrow().len());
    }

    /// Exit the last entered scope in the context
    ///
    /// Returns the scope kind that was removed, or `None` if there are no
    /// scopes to remove.
    pub fn remove_scope(&self) -> Option<ScopeKind> {
        match (self.scope_levels.borrow_mut().pop(), self.scope_kinds.borrow_mut().pop()) {
            (Some(last_level), Some(last_kind)) => {
                self.members.borrow_mut().truncate(last_level);
                Some(last_kind)
            }
            (None, None) => None,
            _ => panic!("mismatch in lengths of `scope_levels` and `kinds`"),
        }
    }

    /// Enter a new scope in the context, and run the given function in that
    /// scope.
    ///
    /// The scope is exited after the function has been run.
    pub fn enter_scope<T>(&self, kind: ScopeKind, f: impl FnOnce() -> T) -> T {
        self.add_scope(kind);
        let res = f();
        if self.remove_scope().is_none() {
            panic!("tried to remove a scope that didn't exist");
        }
        res
    }

    /// Enter a new scope in the context, and run the given function in that
    /// scope, with a mutable `self` that implements [`AccessToEnv`].
    ///
    /// The scope is exited after the function has been run.
    pub fn enter_scope_mut<T, This: AccessToEnv>(
        this: &mut This,
        kind: ScopeKind,
        f: impl FnOnce(&mut This) -> T,
    ) -> T {
        this.context().add_scope(kind);
        let res = f(this);
        if this.context().remove_scope().is_none() {
            panic!("tried to remove a scope that didn't exist");
        }
        res
    }

    /// Add a new binding to the current scope context.
    pub fn add_binding(&self, binding: Binding) {
        self.members.borrow_mut().insert(binding.name, binding);
    }

    /// Get a binding from the context, reading all accessible scopes.
    pub fn get_binding(&self, name: Symbol) -> Option<Binding> {
        self.members.borrow().get(&name).copied()
    }

    /// Get the kind of the current scope.
    pub fn get_scope_kind(&self) -> ScopeKind {
        self.scope_kinds.borrow().last().copied().unwrap_or_else(|| {
            panic!("tried to get the scope kind of a context with no scopes");
        })
    }

    /// Get all the scope levels in the context.
    pub fn get_scope_levels(&self) -> impl Iterator<Item = usize> {
        0..self.scope_levels.borrow().len()
    }

    /// Get the scope kind of the given scope level.
    pub fn get_scope_kind_of_level(&self, level: usize) -> ScopeKind {
        self.scope_kinds.borrow()[level]
    }

    /// Iterate over all the bindings in the context for the given scope level
    /// (fallible).
    pub fn try_for_bindings_of_level<E>(
        &self,
        level: usize,
        mut f: impl FnMut(&Binding) -> Result<(), E>,
    ) -> Result<(), E> {
        let scope_levels = self.scope_levels.borrow();
        let current_level_member_index = scope_levels[level];
        let next_level_member_index =
            scope_levels.get(level + 1).copied().unwrap_or(scope_levels.len());
        for (_, binding) in self
            .members
            .borrow()
            .iter()
            .skip(current_level_member_index)
            .take(next_level_member_index - current_level_member_index)
        {
            f(binding)?
        }
        Ok(())
    }

    /// Iterate all the bindings in the context for the given scope level.
    pub fn for_bindings_of_level(&self, level: usize, mut f: impl FnMut(&Binding)) {
        let _ = self.try_for_bindings_of_level(level, |binding| -> Result<(), Infallible> {
            f(binding);
            Ok(())
        });
    }

    /// Get all the bindings in the context for the given scope level.
    pub fn get_bindings_of_level(&self, level: usize) -> Vec<Symbol> {
        let mut symbols = vec![];
        self.for_bindings_of_level(level, |binding| symbols.push(binding.name));
        symbols
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WithEnv<'_, Binding> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ...", self.env().with(self.value.name))
    }
}

impl fmt::Display for WithEnv<'_, &Context> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for scope_level in self.value.get_scope_levels() {
            let scope_kind = self.value.get_scope_kind_of_level(scope_level);
            writeln!(f, "scope level {scope_level}: {scope_kind:?}")?;
            self.value.try_for_bindings_of_level(scope_level, |binding| {
                writeln!(f, "  {}", self.env().with(*binding))
            })?;
        }
        Ok(())
    }
}
