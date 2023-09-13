//! Utilities to traverse the TIR.
use core::fmt;
use std::{cell::RefCell, collections::HashSet, ops::ControlFlow};

use hash_ast::ast::AstNodeId;
use hash_storage::store::{
    statics::{SequenceStoreValue, StoreId},
    SequenceStoreKey, TrivialSequenceStoreKey,
};
use hash_utils::derive_more::{From, TryInto};

use crate::{
    access::AccessTerm,
    args::{Arg, ArgsId, PatArg, PatArgsId, PatOrCapture},
    arrays::{ArrayPat, ArrayTerm, IndexTerm},
    casting::CastTerm,
    control::{IfPat, LoopTerm, MatchCase, MatchTerm, OrPat, ReturnTerm},
    data::{CtorDefId, CtorPat, CtorTerm, DataDefCtors, DataDefId, DataTy, PrimitiveCtorInfo},
    fns::{CallTerm, FnDef, FnDefId, FnTy},
    mods::{ModDefId, ModMemberId, ModMemberValue},
    node::{HasAstNodeId, Node, NodeId, NodeOrigin, NodesId},
    params::{Param, ParamsId},
    pats::{Pat, PatId, PatListId},
    refs::{DerefTerm, RefTerm, RefTy},
    scopes::{AssignTerm, BlockStatement, BlockStatementsId, BlockTerm, Decl},
    terms::{Term, TermId, TermListId, Ty, TyOfTerm, UnsafeTerm},
    tuples::{TuplePat, TupleTerm, TupleTy},
};

/// Contains methods to traverse the Hash TIR structure.
pub struct Visitor {
    visited: RefCell<HashSet<Atom>>,
    visit_fns_once: bool,
}

/// An atom in the TIR.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, From, TryInto)]
pub enum Atom {
    Term(TermId),
    FnDef(FnDefId),
    Pat(PatId),
}

impl Atom {
    pub fn origin(self) -> NodeOrigin {
        match self {
            Atom::Term(t) => t.origin(),
            Atom::FnDef(f) => f.origin(),
            Atom::Pat(p) => p.origin(),
        }
    }
}

impl HasAstNodeId for Atom {
    fn node_id(&self) -> Option<AstNodeId> {
        match self {
            Atom::Term(t) => t.node_id(),
            Atom::FnDef(f) => f.node_id(),
            Atom::Pat(p) => p.node_id(),
        }
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Atom::Term(term_id) => write!(f, "{}", term_id),
            Atom::FnDef(fn_def_id) => write!(f, "{}", fn_def_id),
            Atom::Pat(pat_id) => write!(f, "{}", pat_id),
        }
    }
}

/// Function to visit an atom.
///
/// This does not return a value, but instead returns a `ControlFlow` to
/// indicate whether to continue or break the traversal.
pub trait VisitFn<E> = FnMut(Atom) -> Result<ControlFlow<()>, E>;

/// Function to map an atom to another atom.
///
/// This returns a `ControlFlow` to indicate whether to continue by duplicating
/// the atom canonically or break the traversal with a custom atom.
pub trait MapFn<E> = Fn(Atom) -> Result<ControlFlow<Atom>, E> + Copy;

/// Contains the implementation of `fmap` and `visit` for each atom, as well as
/// secondary components such as arguments and parameters.
impl Visitor {
    /// Create a new `TraversingUtils`.
    pub fn new() -> Self {
        Self { visited: RefCell::new(HashSet::new()), visit_fns_once: true }
    }

    pub fn set_visit_fns_once(&mut self, visit_fns_once: bool) {
        self.visit_fns_once = visit_fns_once;
    }

    pub fn fmap_atom_non_preserving<E, F: MapFn<E>>(&self, atom: Atom, f: F) -> Result<Atom, E> {
        match f(atom)? {
            ControlFlow::Continue(()) => self.fmap_atom(atom, f),
            ControlFlow::Break(atom) => Ok(atom),
        }
    }

    pub fn fmap_atom<E, F: MapFn<E>>(&self, atom: Atom, f: F) -> Result<Atom, E> {
        match atom {
            Atom::Term(term_id) => Ok(Atom::Term(self.fmap_term(term_id, f)?)),
            Atom::FnDef(fn_def_id) => Ok(Atom::FnDef(self.fmap_fn_def(fn_def_id, f)?)),
            Atom::Pat(pat_id) => Ok(Atom::Pat(self.fmap_pat(pat_id, f)?)),
        }
    }

    pub fn fmap_term<E, F: MapFn<E>>(&self, term_id: TermId, f: F) -> Result<TermId, E> {
        let origin = term_id.origin();
        let result = match f(term_id.into())? {
            ControlFlow::Break(atom) => match atom {
                Atom::Term(t) => Ok(t),
                Atom::FnDef(fn_def_id) => Ok(Node::create_at(Term::Fn(fn_def_id), origin)),
                Atom::Pat(_) => unreachable!("cannot use a pattern as a term"),
            },
            ControlFlow::Continue(()) => match *term_id.value() {
                Term::Tuple(tuple_term) => {
                    let data = self.fmap_args(tuple_term.data, f)?;
                    Ok(Term::from(Term::Tuple(TupleTerm { data }), origin))
                }
                Term::Lit(lit) => Ok(Term::from(Term::Lit(lit), origin)),
                Term::Array(list_ctor) => {
                    let elements = self.fmap_term_list(list_ctor.elements, f)?;
                    Ok(Term::from(Term::Array(ArrayTerm { elements }), origin))
                }
                Term::Ctor(ctor_term) => {
                    let data_args = self.fmap_args(ctor_term.data_args, f)?;
                    let ctor_args = self.fmap_args(ctor_term.ctor_args, f)?;
                    Ok(Term::from(CtorTerm { ctor: ctor_term.ctor, data_args, ctor_args }, origin))
                }
                Term::Call(fn_call_term) => {
                    let subject = self.fmap_term(fn_call_term.subject, f)?;
                    let args = self.fmap_args(fn_call_term.args, f)?;
                    Ok(Term::from(
                        CallTerm { args, subject, implicit: fn_call_term.implicit },
                        origin,
                    ))
                }
                Term::Fn(fn_def_id) => {
                    let fn_def_id = self.fmap_fn_def(fn_def_id, f)?;
                    Ok(Term::from(Term::Fn(fn_def_id), origin))
                }
                Term::Block(block_term) => {
                    let statements = self.fmap_block_statements(block_term.statements, f)?;
                    let expr = self.fmap_term(block_term.expr, f)?;
                    Ok(Term::from(
                        BlockTerm { statements, stack_id: block_term.stack_id, expr },
                        origin,
                    ))
                }
                Term::Var(var_term) => Ok(Term::from(var_term, origin)),
                Term::Loop(loop_term) => {
                    let inner = self.fmap_term(loop_term.inner, f)?;
                    Ok(Term::from(LoopTerm { inner }, origin))
                }
                Term::LoopControl(loop_control_term) => Ok(Term::from(loop_control_term, origin)),
                Term::Match(match_term) => {
                    let subject = self.fmap_term(match_term.subject, f)?;

                    let cases = Node::<MatchCase>::seq(
                        match_term
                            .cases
                            .value()
                            .iter()
                            .map(|case| {
                                let case_value = case.value();
                                let bind_pat = self.fmap_pat(case_value.bind_pat, f)?;
                                let value = self.fmap_term(case_value.value, f)?;
                                Ok(Node::at(
                                    MatchCase { bind_pat, value, stack_id: case_value.stack_id },
                                    case_value.origin,
                                ))
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                    Ok(Term::from(
                        MatchTerm {
                            cases: Node::create_at(cases, match_term.cases.origin()),
                            subject,
                            origin: match_term.origin,
                        },
                        origin,
                    ))
                }
                Term::Return(return_term) => {
                    let expression = self.fmap_term(return_term.expression, f)?;
                    Ok(Term::from(ReturnTerm { expression }, origin))
                }
                Term::Assign(assign_term) => {
                    let subject = self.fmap_term(assign_term.subject, f)?;
                    let value = self.fmap_term(assign_term.value, f)?;
                    Ok(Term::from(AssignTerm { subject, value }, origin))
                }
                Term::Unsafe(unsafe_term) => {
                    let inner = self.fmap_term(unsafe_term.inner, f)?;
                    Ok(Term::from(UnsafeTerm { inner }, origin))
                }
                Term::Access(access_term) => {
                    let subject = self.fmap_term(access_term.subject, f)?;
                    Ok(Term::from(AccessTerm { subject, field: access_term.field }, origin))
                }
                Term::Index(index_term) => {
                    let subject = self.fmap_term(index_term.subject, f)?;
                    let index = self.fmap_term(index_term.index, f)?;
                    Ok(Term::from(IndexTerm { subject, index }, origin))
                }
                Term::Cast(cast_term) => {
                    let subject_term = self.fmap_term(cast_term.subject_term, f)?;
                    let target_ty = self.fmap_term(cast_term.target_ty, f)?;
                    Ok(Term::from(CastTerm { subject_term, target_ty }, origin))
                }
                Term::TypeOf(type_of_term) => {
                    let term = self.fmap_term(type_of_term.term, f)?;
                    Ok(Term::from(TyOfTerm { term }, origin))
                }
                Term::Ref(ref_term) => {
                    let subject = self.fmap_term(ref_term.subject, f)?;
                    Ok(Term::from(
                        RefTerm { subject, kind: ref_term.kind, mutable: ref_term.mutable },
                        origin,
                    ))
                }
                Term::Deref(deref_term) => {
                    let subject = self.fmap_term(deref_term.subject, f)?;
                    Ok(Term::from(DerefTerm { subject }, origin))
                }
                Term::Hole(hole_term) => Ok(Term::from(hole_term, origin)),
                Term::Intrinsic(intrinsic) => Ok(Term::from(intrinsic, origin)),
                Ty::TupleTy(tuple_ty) => {
                    let data = self.fmap_params(tuple_ty.data, f)?;
                    Ok(Ty::from(TupleTy { data }, origin))
                }
                Ty::FnTy(fn_ty) => {
                    let params = self.fmap_params(fn_ty.params, f)?;
                    let return_ty = self.fmap_term(fn_ty.return_ty, f)?;
                    Ok(Ty::from(
                        FnTy {
                            params,
                            return_ty,
                            implicit: fn_ty.implicit,
                            is_unsafe: fn_ty.is_unsafe,
                            pure: fn_ty.pure,
                        },
                        origin,
                    ))
                }
                Ty::RefTy(ref_ty) => {
                    let ty = self.fmap_term(ref_ty.ty, f)?;
                    Ok(Ty::from(RefTy { ty, kind: ref_ty.kind, mutable: ref_ty.mutable }, origin))
                }
                Ty::DataTy(data_ty) => {
                    let args = self.fmap_args(data_ty.args, f)?;
                    Ok(Ty::from(DataTy { args, data_def: data_ty.data_def }, origin))
                }
                Ty::Universe => Ok(Ty::from(Ty::Universe, origin)),
            },
        }?;

        Ok(result)
    }

    pub fn fmap_pat<E, F: MapFn<E>>(&self, pat_id: PatId, f: F) -> Result<PatId, E> {
        let origin = pat_id.origin();
        let result = match f(pat_id.into())? {
            ControlFlow::Break(pat) => Ok(PatId::try_from(pat).unwrap()),
            ControlFlow::Continue(()) => match *pat_id.value() {
                Pat::Binding(binding_pat) => Ok(Node::create_at(Pat::from(binding_pat), origin)),
                Pat::Range(range_pat) => Ok(Node::create_at(Pat::from(range_pat), origin)),
                Pat::Lit(lit_pat) => Ok(Node::create_at(Pat::from(lit_pat), origin)),
                Pat::Tuple(tuple_pat) => {
                    let data = self.fmap_pat_args(tuple_pat.data, f)?;
                    Ok(Node::create_at(
                        Pat::from(TuplePat { data_spread: tuple_pat.data_spread, data }),
                        origin,
                    ))
                }
                Pat::Array(list_pat) => {
                    let pats = self.fmap_pat_list(list_pat.pats, f)?;
                    Ok(Node::create_at(
                        Pat::from(ArrayPat { spread: list_pat.spread, pats }),
                        origin,
                    ))
                }
                Pat::Ctor(ctor_pat) => {
                    let data_args = self.fmap_args(ctor_pat.data_args, f)?;
                    let ctor_pat_args = self.fmap_pat_args(ctor_pat.ctor_pat_args, f)?;
                    Ok(Node::create_at(
                        Pat::from(CtorPat {
                            data_args,
                            ctor_pat_args,
                            ctor: ctor_pat.ctor,
                            ctor_pat_args_spread: ctor_pat.ctor_pat_args_spread,
                        }),
                        origin,
                    ))
                }
                Pat::Or(or_pat) => {
                    let alternatives = self.fmap_pat_list(or_pat.alternatives, f)?;
                    Ok(Node::create_at(Pat::from(OrPat { alternatives }), origin))
                }
                Pat::If(if_pat) => {
                    let pat = self.fmap_pat(if_pat.pat, f)?;
                    let condition = self.fmap_term(if_pat.condition, f)?;
                    Ok(Node::create_at(Pat::from(IfPat { pat, condition }), origin))
                }
            },
        }?;

        Ok(result)
    }

    pub fn fmap_block_statements<E, F: MapFn<E>>(
        &self,
        block_statements: BlockStatementsId,
        f: F,
    ) -> Result<BlockStatementsId, E> {
        let mut new_list = Vec::with_capacity(block_statements.len());
        for statement in block_statements.elements().value() {
            match *statement {
                BlockStatement::Decl(decl) => {
                    let bind_pat = self.fmap_pat(decl.bind_pat, f)?;
                    let ty = self.fmap_term(decl.ty, f)?;
                    let value = decl.value.map(|v| self.fmap_term(v, f)).transpose()?;
                    new_list.push(Node::at(
                        BlockStatement::Decl(Decl { ty, bind_pat, value }),
                        statement.origin,
                    ));
                }
                BlockStatement::Expr(expr) => {
                    new_list.push(Node::at(
                        BlockStatement::Expr(self.fmap_term(expr, f)?),
                        statement.origin,
                    ));
                }
            }
        }
        Ok(Node::create_at(Node::seq(new_list), block_statements.origin()))
    }

    pub fn fmap_term_list<E, F: MapFn<E>>(
        &self,
        term_list: TermListId,
        f: F,
    ) -> Result<TermListId, E> {
        let mut new_list = Vec::with_capacity(term_list.len());
        for term_id in term_list.elements().value() {
            new_list.push(self.fmap_term(term_id, f)?);
        }
        Ok(Node::create_at(TermId::seq(new_list), term_list.origin()))
    }

    pub fn fmap_pat_list<E, F: MapFn<E>>(&self, pat_list: PatListId, f: F) -> Result<PatListId, E> {
        let mut new_list = Vec::with_capacity(pat_list.len());
        for pat_id in pat_list.elements().value() {
            match pat_id {
                PatOrCapture::Pat(pat_id) => {
                    new_list.push(PatOrCapture::Pat(self.fmap_pat(pat_id, f)?));
                }
                PatOrCapture::Capture(node) => {
                    new_list.push(PatOrCapture::Capture(node));
                }
            }
        }
        Ok(Node::create_at(PatOrCapture::seq(new_list), pat_list.origin()))
    }

    pub fn fmap_params<E, F: MapFn<E>>(&self, params_id: ParamsId, f: F) -> Result<ParamsId, E> {
        let new_params = {
            let mut new_params = Vec::with_capacity(params_id.len());
            for param in params_id.elements().value() {
                new_params.push(Node::at(
                    Param {
                        name: param.name,
                        ty: self.fmap_term(param.ty, f)?,
                        default: param
                            .default
                            .map(|default| self.fmap_term(default, f))
                            .transpose()?,
                    },
                    param.origin,
                ));
            }
            Ok(Node::create_at(Node::<Param>::seq(new_params), params_id.origin()))
        }?;

        Ok(new_params)
    }

    pub fn fmap_args<E, F: MapFn<E>>(&self, args_id: ArgsId, f: F) -> Result<ArgsId, E> {
        let mut new_args = Vec::with_capacity(args_id.len());
        for arg in args_id.elements().value() {
            new_args.push(Node::at(
                Arg { target: arg.target, value: self.fmap_term(arg.value, f)? },
                arg.origin,
            ));
        }
        let new_args_id = Node::create_at(Node::<Arg>::seq(new_args), args_id.origin());
        Ok(new_args_id)
    }

    pub fn fmap_pat_args<E, F: MapFn<E>>(
        &self,
        pat_args_id: PatArgsId,
        f: F,
    ) -> Result<PatArgsId, E> {
        let new_pat_args = {
            let mut new_args = Vec::with_capacity(pat_args_id.len());
            for pat_arg in pat_args_id.elements().value() {
                new_args.push(Node::at(
                    PatArg {
                        target: pat_arg.target,
                        pat: match pat_arg.pat {
                            PatOrCapture::Pat(pat_id) => {
                                PatOrCapture::Pat(self.fmap_pat(pat_id, f)?)
                            }
                            PatOrCapture::Capture(node) => PatOrCapture::Capture(node),
                        },
                    },
                    pat_arg.origin,
                ));
            }
            Ok(Node::create_at(Node::<PatArg>::seq(new_args), pat_args_id.origin()))
        }?;

        Ok(new_pat_args)
    }

    pub fn fmap_fn_def<E, F: MapFn<E>>(&self, fn_def_id: FnDefId, f: F) -> Result<FnDefId, E> {
        if self.visit_fns_once {
            {
                if self.visited.borrow().contains(&fn_def_id.into()) {
                    return Ok(fn_def_id);
                }
            }
            self.visited.borrow_mut().insert(fn_def_id.into());
        }

        let new_fn_def = match f(fn_def_id.into())? {
            ControlFlow::Break(fn_def_id) => Ok(FnDefId::try_from(fn_def_id).unwrap()),
            ControlFlow::Continue(()) => {
                let fn_def = fn_def_id.value();
                let params = self.fmap_params(fn_def.ty.params, f)?;
                let return_ty = self.fmap_term(fn_def.ty.return_ty, f)?;
                let body = self.fmap_term(fn_def.body, f)?;

                let def = Node::create_at(
                    FnDef {
                        name: fn_def.name,
                        ty: FnTy {
                            params,
                            return_ty,
                            implicit: fn_def.ty.implicit,
                            is_unsafe: fn_def.ty.is_unsafe,
                            pure: fn_def.ty.pure,
                        },
                        body,
                    },
                    fn_def.origin,
                );

                Ok(def)
            }
        }?;

        Ok(new_fn_def)
    }

    pub fn visit_term<E, F: VisitFn<E>>(&self, term_id: TermId, f: &mut F) -> Result<(), E> {
        match f(term_id.into())? {
            ControlFlow::Break(_) => Ok(()),
            ControlFlow::Continue(()) => match *term_id.value() {
                Term::Tuple(tuple_term) => self.visit_args(tuple_term.data, f),
                Term::Lit(_) => Ok(()),
                Term::Array(list_ctor) => self.visit_term_list(list_ctor.elements, f),
                Term::Ctor(ctor_term) => {
                    self.visit_args(ctor_term.data_args, f)?;
                    self.visit_args(ctor_term.ctor_args, f)
                }
                Term::Call(fn_call_term) => {
                    self.visit_term(fn_call_term.subject, f)?;
                    self.visit_args(fn_call_term.args, f)
                }
                Term::Fn(fn_def_id) => self.visit_fn_def(fn_def_id, f),
                Term::Block(block_term) => {
                    self.visit_block_statements(block_term.statements, f)?;
                    self.visit_term(block_term.expr, f)
                }
                Term::Var(_) => Ok(()),
                Term::Loop(loop_term) => self.visit_term(loop_term.inner, f),
                Term::LoopControl(_) => Ok(()),
                Term::Match(match_term) => {
                    self.visit_term(match_term.subject, f)?;
                    for case in match_term.cases.elements().value() {
                        self.visit_pat(case.bind_pat, f)?;
                        self.visit_term(case.value, f)?;
                    }
                    Ok(())
                }
                Term::Return(return_term) => self.visit_term(return_term.expression, f),
                Term::Assign(assign_term) => {
                    self.visit_term(assign_term.subject, f)?;
                    self.visit_term(assign_term.value, f)
                }
                Term::Unsafe(unsafe_term) => self.visit_term(unsafe_term.inner, f),
                Term::Access(access_term) => self.visit_term(access_term.subject, f),
                Term::Index(index_term) => {
                    self.visit_term(index_term.subject, f)?;
                    self.visit_term(index_term.index, f)
                }
                Term::Cast(cast_term) => {
                    self.visit_term(cast_term.subject_term, f)?;
                    self.visit_term(cast_term.target_ty, f)
                }
                Term::TypeOf(type_of_term) => self.visit_term(type_of_term.term, f),
                Term::Ref(ref_term) => self.visit_term(ref_term.subject, f),
                Term::Deref(deref_term) => self.visit_term(deref_term.subject, f),
                Term::Hole(_) => Ok(()),
                Term::Intrinsic(_) => Ok(()),
                Ty::TupleTy(tuple_ty) => self.visit_params(tuple_ty.data, f),
                Ty::FnTy(fn_ty) => {
                    self.visit_params(fn_ty.params, f)?;
                    self.visit_term(fn_ty.return_ty, f)
                }
                Ty::RefTy(ref_ty) => self.visit_term(ref_ty.ty, f),
                Ty::DataTy(data_ty) => self.visit_args(data_ty.args, f),
                Ty::Universe => Ok(()),
            },
        }
    }

    pub fn visit_pat<E, F: VisitFn<E>>(&self, pat_id: PatId, f: &mut F) -> Result<(), E> {
        match f(pat_id.into())? {
            ControlFlow::Break(()) => Ok(()),
            ControlFlow::Continue(()) => match *pat_id.value() {
                Pat::Binding(_) | Pat::Range(_) | Pat::Lit(_) => Ok(()),
                Pat::Tuple(tuple_pat) => self.visit_pat_args(tuple_pat.data, f),
                Pat::Array(list_pat) => self.visit_pat_list(list_pat.pats, f),
                Pat::Ctor(ctor_pat) => {
                    self.visit_args(ctor_pat.data_args, f)?;
                    self.visit_pat_args(ctor_pat.ctor_pat_args, f)
                }
                Pat::Or(or_pat) => self.visit_pat_list(or_pat.alternatives, f),
                Pat::If(if_pat) => {
                    self.visit_pat(if_pat.pat, f)?;
                    self.visit_term(if_pat.condition, f)
                }
            },
        }
    }

    pub fn visit_fn_def<E, F: VisitFn<E>>(&self, fn_def_id: FnDefId, f: &mut F) -> Result<(), E> {
        if self.visit_fns_once {
            {
                if self.visited.borrow().contains(&fn_def_id.into()) {
                    return Ok(());
                }
            }
            self.visited.borrow_mut().insert(fn_def_id.into());
        }

        match f(fn_def_id.into())? {
            ControlFlow::Break(()) => Ok(()),
            ControlFlow::Continue(()) => {
                let fn_def = fn_def_id.value();
                let fn_ty = fn_def.ty;
                self.visit_params(fn_ty.params, f)?;
                self.visit_term(fn_ty.return_ty, f)?;
                self.visit_term(fn_def.body, f)
            }
        }
    }

    pub fn visit_atom<E, F: VisitFn<E>>(&self, atom: Atom, f: &mut F) -> Result<(), E> {
        match atom {
            Atom::Term(term_id) => self.visit_term(term_id, f),
            Atom::FnDef(fn_def_id) => self.visit_fn_def(fn_def_id, f),
            Atom::Pat(pat_id) => self.visit_pat(pat_id, f),
        }
    }

    pub fn visit_term_list<E, F: VisitFn<E>>(
        &self,
        term_list_id: TermListId,
        f: &mut F,
    ) -> Result<(), E> {
        for term in term_list_id.elements().value() {
            self.visit_term(term, f)?;
        }
        Ok(())
    }

    pub fn visit_block_statements<E, F: VisitFn<E>>(
        &self,
        block_statements: BlockStatementsId,
        f: &mut F,
    ) -> Result<(), E> {
        for statement in block_statements.elements().value() {
            match *statement {
                BlockStatement::Decl(decl) => {
                    self.visit_pat(decl.bind_pat, f)?;
                    self.visit_term(decl.ty, f)?;
                    decl.value.map(|v| self.visit_term(v, f)).transpose()?;
                }
                BlockStatement::Expr(expr) => {
                    self.visit_term(expr, f)?;
                }
            }
        }
        Ok(())
    }

    pub fn visit_pat_list<E, F: VisitFn<E>>(
        &self,
        pat_list_id: PatListId,
        f: &mut F,
    ) -> Result<(), E> {
        for pat in pat_list_id.elements().value() {
            if let PatOrCapture::Pat(pat) = pat {
                self.visit_pat(pat, f)?;
            }
        }
        Ok(())
    }

    pub fn visit_params<E, F: VisitFn<E>>(&self, params_id: ParamsId, f: &mut F) -> Result<(), E> {
        for param in params_id.elements().value() {
            self.visit_term(param.ty, f)?;
            if let Some(default) = param.default {
                self.visit_term(default, f)?;
            }
        }
        Ok(())
    }

    pub fn visit_pat_args<E, F: VisitFn<E>>(
        &self,
        pat_args_id: PatArgsId,
        f: &mut F,
    ) -> Result<(), E> {
        for arg in pat_args_id.elements().value() {
            if let PatOrCapture::Pat(pat) = arg.pat {
                self.visit_pat(pat, f)?;
            }
        }
        Ok(())
    }

    pub fn visit_args<E, F: VisitFn<E>>(&self, args_id: ArgsId, f: &mut F) -> Result<(), E> {
        for arg in args_id.elements().value() {
            self.visit_term(arg.value, f)?;
        }
        Ok(())
    }

    pub fn visit_ctor_def<E, F: VisitFn<E>>(
        &self,
        ctor_def_id: CtorDefId,
        f: &mut F,
    ) -> Result<(), E> {
        let ctor_def = ctor_def_id.value();

        // Visit the parameters
        self.visit_params(ctor_def.params, f)?;

        // Visit the arguments
        self.visit_args(ctor_def.result_args, f)?;

        Ok(())
    }

    pub fn visit_data_def<E, F: VisitFn<E>>(
        &self,
        data_def_id: DataDefId,
        f: &mut F,
    ) -> Result<(), E> {
        let (data_def_params, data_def_ctors) =
            data_def_id.map(|data_def| (data_def.params, data_def.ctors));

        // Params
        self.visit_params(data_def_params, f)?;

        match data_def_ctors {
            DataDefCtors::Defined(data_def_ctors_id) => {
                // Traverse the constructors
                for ctor_idx in data_def_ctors_id.value().to_index_range() {
                    self.visit_ctor_def(CtorDefId(data_def_ctors_id.elements(), ctor_idx), f)?;
                }
                Ok(())
            }
            DataDefCtors::Primitive(primitive) => match primitive {
                PrimitiveCtorInfo::Numeric(_)
                | PrimitiveCtorInfo::Str
                | PrimitiveCtorInfo::Char => {
                    // Nothing to do
                    Ok(())
                }
                PrimitiveCtorInfo::Array(list_ctor_info) => {
                    // Traverse the inner type
                    self.visit_term(list_ctor_info.element_ty, f)?;
                    Ok(())
                }
            },
        }
    }

    pub fn visit_mod_member<E, F: VisitFn<E>>(
        &self,
        mod_member_id: ModMemberId,
        f: &mut F,
    ) -> Result<(), E> {
        let value = mod_member_id.borrow().value;
        match value {
            ModMemberValue::Data(data_def_id) => {
                self.visit_data_def(data_def_id, f)?;
                Ok(())
            }
            ModMemberValue::Mod(mod_def_id) => {
                self.visit_mod_def(mod_def_id, f)?;
                Ok(())
            }
            ModMemberValue::Fn(fn_def_id) => {
                self.visit_fn_def(fn_def_id, f)?;
                Ok(())
            }
            ModMemberValue::Intrinsic(_) => {
                // Nothing to do
                Ok(())
            }
        }
    }

    pub fn visit_mod_def<E, F: VisitFn<E>>(
        &self,
        mod_def_id: ModDefId,
        f: &mut F,
    ) -> Result<(), E> {
        for member in mod_def_id.borrow().members.iter() {
            self.visit_mod_member(member, f)?;
        }
        Ok(())
    }
}

impl Default for Visitor {
    fn default() -> Self {
        Self::new()
    }
}