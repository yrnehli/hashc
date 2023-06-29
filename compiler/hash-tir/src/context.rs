//! Representing and modifying the typechecking context.
use core::fmt;
use std::{
    cell::{Ref, RefCell},
    convert::Infallible,
    ops::Range,
};

use derive_more::From;
use hash_utils::{itertools::Itertools, store::StoreKey};
use indexmap::IndexMap;

use crate::{
    data::{CtorDefId, DataDefId},
    environment::env::AccessToEnv,
    fns::{FnDefId, FnTy},
    mods::ModDefId,
    scopes::StackId,
    symbols::Symbol,
    terms::TermId,
    tir_get,
    tuples::TupleTy,
    tys::TyId,
};

/// A binding that contains a type and optional value.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Decl {
    pub name: Symbol,
    pub ty: Option<TyId>,
    pub value: Option<TermId>,
}

/// An established equality between terms, that is in scope.
#[derive(Debug, Clone, Copy)]
pub struct EqualityJudgement {
    pub lhs: TermId,
    pub rhs: TermId,
}

/// All the different kinds of scope there are, and their associated data.
#[derive(Debug, Clone, Copy, From)]
pub enum ScopeKind {
    /// A module scope.
    Mod(ModDefId),
    // A stack scope.
    Stack(StackId),
    /// A function scope.
    Fn(FnDefId),
    /// A data definition.
    Data(DataDefId),
    /// A constructor definition.
    Ctor(CtorDefId),
    /// A function type scope.
    FnTy(FnTy),
    /// A tuple type scope.
    TupleTy(TupleTy),
    /// A substitution scope.
    Sub,
}

/// Information about a scope in the context.
#[derive(Debug, Clone)]
pub struct Scope {
    /// The kind of the scope.
    pub kind: ScopeKind,
    /// The bindings of the scope
    pub decls: RefCell<IndexMap<Symbol, Decl>>,
}

impl Scope {
    /// Create a new scope with the given kind.
    pub fn with_empty_members(kind: ScopeKind) -> Self {
        Self { kind, decls: RefCell::new(IndexMap::new()) }
    }

    /// Add a binding to the scope.
    pub fn add_decl(&self, decl: Decl) {
        self.decls.borrow_mut().insert(decl.name, decl);
    }

    /// Get the decl corresponding to the given symbol.
    pub fn get_decl(&self, symbol: Symbol) -> Option<Decl> {
        self.decls.borrow().get(&symbol).copied()
    }

    /// Set an existing decl kind of the given symbol.
    ///
    /// Returns `true` if the decl was found and updated, `false` otherwise.
    pub fn set_existing_decl(&self, symbol: Symbol, f: &impl Fn(Decl) -> Decl) -> bool {
        if let Some(old) = self.get_decl(symbol) {
            self.decls.borrow_mut().insert(symbol, f(old));
            true
        } else {
            false
        }
    }
}

/// Data structure managing the typechecking context.
///
/// The context is a stack of scopes, each scope being a stack in itself.
///
/// The context is used to resolve symbols to their corresponding decls, and
/// thus interpret their meaning. It can read and add [`Decl`]s, and can
/// enter and exit scopes.
#[derive(Debug, Clone, Default)]
pub struct Context {
    scopes: RefCell<Vec<Scope>>,
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter a new scope in the context.
    pub fn add_scope(&self, kind: ScopeKind) {
        self.scopes.borrow_mut().push(Scope::with_empty_members(kind));
    }

    /// Exit the last entered scope in the context
    ///
    /// Returns the scope kind that was removed, or `None` if there are no
    /// scopes to remove.
    pub fn remove_scope(&self) -> Option<Scope> {
        self.scopes.borrow_mut().pop()
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

    /// Add a new decl to the current scope context.
    pub fn add_decl(&self, name: Symbol, ty: Option<TyId>, value: Option<TermId>) {
        self.get_current_scope_ref().add_decl(Decl { name, ty, value })
    }

    /// Add a new decl to the scope with the given index.
    pub fn add_decl_to_scope(&self, decl: Decl, _scope_index: usize) {
        self.get_closest_stack_scope_ref().add_decl(decl);
    }

    /// Get a decl from the context, reading all accessible scopes.
    pub fn try_get_decl(&self, name: Symbol) -> Option<Decl> {
        self.scopes.borrow().iter().rev().find_map(|scope| scope.get_decl(name))
    }

    /// Get a decl from the context, reading all accessible scopes.
    ///
    /// Panics if the decl doesn't exist.
    pub fn get_decl(&self, name: Symbol) -> Decl {
        self.try_get_decl(name).unwrap_or_else(|| panic!("tried to get a decl that doesn't exist"))
    }

    /// Modify a decl in the context, with a function that takes the current
    /// decl kind and returns the new decl kind.
    pub fn modify_decl_with(&self, name: Symbol, f: impl Fn(Decl) -> Decl) {
        let _ = self
            .scopes
            .borrow()
            .iter()
            .rev()
            .find(|scope| scope.set_existing_decl(name, &f))
            .unwrap_or_else(|| panic!("tried to modify a decl that doesn't exist"));
    }

    /// Modify a decl in the context.
    pub fn modify_decl(&self, decl: Decl) {
        self.modify_decl_with(decl.name, |_| decl);
    }

    /// Get a reference to the current scope.
    pub fn get_current_scope_ref(&self) -> Ref<Scope> {
        Ref::map(self.scopes.borrow(), |scopes| {
            scopes.last().unwrap_or_else(|| {
                panic!("tried to get the scope kind of a context with no scopes");
            })
        })
    }

    /// Get the current scope.
    pub fn get_current_scope_kind(&self) -> ScopeKind {
        self.get_current_scope_ref().kind
    }

    /// Get the closest stack scope and its index.
    pub fn get_closest_stack_scope_ref(&self) -> Ref<Scope> {
        Ref::map(self.scopes.borrow(), |scopes| {
            scopes
                .iter()
                .rev()
                .find(|scope| matches!(scope.kind, ScopeKind::Stack(_)))
                .unwrap_or_else(|| {
                    panic!("tried to get the scope kind of a context with no scopes");
                })
        })
    }

    pub fn get_scope_ref_at_index(&self, index: usize) -> Ref<Scope> {
        Ref::map(self.scopes.borrow(), |scopes| {
            scopes.get(index).unwrap_or_else(|| {
                panic!("tried to get the scope kind of a context with no scopes");
            })
        })
    }

    /// Get the index of the current scope.
    pub fn get_current_scope_index(&self) -> usize {
        self.scopes.borrow().len().checked_sub(1).unwrap_or_else(|| {
            panic!("tried to get the scope kind of a context with no scopes");
        })
    }

    /// Get information about the scope with the given index.
    pub fn get_scope(&self, index: usize) -> Ref<Scope> {
        Ref::map(self.scopes.borrow(), |scopes| &scopes[index])
    }

    /// Get all the scope indices in the context.
    pub fn get_scope_indices(&self) -> Range<usize> {
        0..self.scopes.borrow().len()
    }

    /// Iterate over all the decls in the context for the scope with the
    /// given index (fallible).
    pub fn try_for_decls_of_scope_rev<E>(
        &self,
        scope_index: usize,
        mut f: impl FnMut(&Decl) -> Result<(), E>,
    ) -> Result<(), E> {
        self.scopes.borrow()[scope_index]
            .decls
            .borrow()
            .iter()
            .rev()
            .try_for_each(|(_, decl)| f(decl))
    }

    /// Iterate over all the decls in the context for the scope with the
    /// given index (fallible).
    pub fn try_for_decls_of_scope<E>(
        &self,
        scope_index: usize,
        mut f: impl FnMut(&Decl) -> Result<(), E>,
    ) -> Result<(), E> {
        self.scopes.borrow()[scope_index].decls.borrow().iter().try_for_each(|(_, decl)| f(decl))
    }

    /// Get the number of decls in the context for the scope with the given
    /// index.
    pub fn count_decls_of_scope(&self, scope_index: usize) -> usize {
        self.scopes.borrow()[scope_index].decls.borrow().len()
    }

    /// Iterate over all the decls in the context for the scope with the
    /// given index (reversed).
    pub fn for_decls_of_scope_rev(&self, scope_index: usize, mut f: impl FnMut(&Decl)) {
        let _ = self.try_for_decls_of_scope_rev(scope_index, |decl| -> Result<(), Infallible> {
            f(decl);
            Ok(())
        });
    }

    /// Iterate over all the decls in the context for the scope with the
    /// given index.
    pub fn for_decls_of_scope(&self, scope_index: usize, mut f: impl FnMut(&Decl)) {
        let _ = self.try_for_decls_of_scope(scope_index, |decl| -> Result<(), Infallible> {
            f(decl);
            Ok(())
        });
    }

    /// Get all the decls in the context for the given scope.
    pub fn get_owned_decls_of_scope(&self, scope_index: usize) -> Vec<Symbol> {
        self.scopes.borrow()[scope_index].decls.borrow().keys().copied().collect_vec()
    }

    /// Clear all the scopes and decls in the context.
    pub fn clear_all(&self) {
        self.scopes.borrow_mut().clear();
    }
}

impl fmt::Display for EqualityJudgement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} === {}", self.lhs, self.rhs)
    }
}

impl fmt::Display for Decl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ty_or_unknown = {
            if let Some(ty) = self.ty {
                ty.to_string()
            } else {
                "unknown".to_string()
            }
        };
        match self.value {
            Some(value) => {
                write!(f, "{}: {} = {}", self.name, ty_or_unknown, value,)
            }
            None => {
                write!(f, "{}: {}", self.name, ty_or_unknown)
            }
        }
    }
}

impl fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScopeKind::Mod(mod_def_id) => write!(f, "mod {}", tir_get!(*mod_def_id, name)),
            ScopeKind::Fn(fn_def_id) => write!(f, "fn {}", tir_get!(*fn_def_id, name)),
            ScopeKind::Data(data_def_id) => write!(f, "data {}", tir_get!(*data_def_id, name)),
            ScopeKind::Ctor(ctor_def) => write!(f, "ctor {}", ctor_def),
            ScopeKind::Stack(stack_def_id) => write!(f, "stack {}", stack_def_id.to_index(),),
            ScopeKind::FnTy(fn_ty) => write!(f, "fn ty {}", fn_ty),
            ScopeKind::TupleTy(tuple_ty) => write!(f, "tuple ty {}", tuple_ty),
            ScopeKind::Sub => {
                write!(f, "sub")
            }
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.kind)?;
        for decl in self.decls.borrow().values() {
            let result = (*decl).to_string();
            for line in result.lines() {
                writeln!(f, "  {line}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for scope_index in self.get_scope_indices() {
            let kind = self.get_scope(scope_index).kind;
            writeln!(f, "({}) {}:", scope_index, kind)?;
            self.try_for_decls_of_scope(scope_index, |decl| {
                let result = (*decl).to_string();
                for line in result.lines() {
                    writeln!(f, "  {line}")?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }
}
