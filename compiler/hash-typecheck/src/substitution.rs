//! Operations to substitute variables in types and terms.

use std::{collections::HashSet, ops::ControlFlow};

use derive_more::{Constructor, Deref};
use hash_tir::{
    access::AccessTerm,
    args::ArgsId,
    holes::Hole,
    mods::ModDefId,
    params::{ParamData, ParamId, ParamIndex, ParamsId},
    pats::PatId,
    sub::Sub,
    symbols::Symbol,
    terms::{Term, TermId},
    tys::{Ty, TyId},
    utils::{common::CommonUtils, traversing::Atom, AccessToUtils},
};
use hash_utils::store::{SequenceStore, SequenceStoreKey, Store};

use crate::AccessToTypechecking;

#[derive(Constructor, Deref)]
pub struct SubstitutionOps<'a, T: AccessToTypechecking>(&'a T);

impl<T: AccessToTypechecking> SubstitutionOps<'_, T> {
    /// Apply the given substitution to the given atom, modifying it in place.
    ///
    /// Returns `ControlFlow::Break(())` if the atom was modified, and
    /// `ControlFlow::Continue(())` otherwise to recurse deeper.
    pub fn apply_sub_to_atom_in_place_once(&self, atom: Atom, sub: &Sub) -> ControlFlow<()> {
        if !self.can_apply_sub_to_atom(atom, sub) {
            return ControlFlow::Break(());
        }
        match atom {
            Atom::Ty(ty) => match self.get_ty(ty) {
                Ty::Hole(Hole(symbol)) | Ty::Var(symbol) => {
                    match sub.get_sub_for_var_or_hole(symbol) {
                        Some(term) => {
                            let subbed_ty = self.get_ty(self.use_term_as_ty(term));
                            self.stores().ty().modify_fast(ty, |ty| *ty = subbed_ty);
                            ControlFlow::Break(())
                        }
                        None => ControlFlow::Continue(()),
                    }
                }
                _ => ControlFlow::Continue(()),
            },
            Atom::Term(term) => match self.get_term(term) {
                Term::Hole(Hole(symbol)) | Term::Var(symbol) => match sub.get_sub_for(symbol) {
                    Some(term) => {
                        let subbed_term = self.get_term(term);
                        self.stores().term().modify_fast(term, |term| *term = subbed_term);
                        ControlFlow::Break(())
                    }
                    None => ControlFlow::Continue(()),
                },
                _ => ControlFlow::Continue(()),
            },
            Atom::FnDef(_) | Atom::Pat(_) => ControlFlow::Continue(()),
        }
    }

    /// Whether the given substitution can be appliedto the given atom,
    ///
    /// i.e. if the atom contains a variable or hole that is in the
    /// substitution.
    pub fn atom_contains_vars_once(
        &self,
        atom: Atom,
        var_matches: impl Fn(Symbol) -> bool,
        can_apply: &mut bool,
    ) -> ControlFlow<()> {
        if *can_apply {
            return ControlFlow::Break(());
        }
        match atom {
            Atom::Ty(ty) => match self.get_ty(ty) {
                Ty::Hole(Hole(symbol)) | Ty::Var(symbol) if var_matches(symbol) => {
                    *can_apply = true;
                    ControlFlow::Break(())
                }
                _ => ControlFlow::Continue(()),
            },
            Atom::Term(term) => match self.get_term(term) {
                Term::Hole(Hole(symbol)) | Term::Var(symbol) if var_matches(symbol) => {
                    *can_apply = true;
                    ControlFlow::Break(())
                }
                _ => ControlFlow::Continue(()),
            },
            Atom::FnDef(_) | Atom::Pat(_) => ControlFlow::Continue(()),
        }
    }

    /// Whether the given substitution can be appliedto the given atom,
    ///
    /// i.e. if the atom contains a variable or hole that is in the
    /// substitution.
    pub fn can_apply_sub_to_atom_once(
        &self,
        atom: Atom,
        sub: &Sub,
        can_apply: &mut bool,
    ) -> ControlFlow<()> {
        self.atom_contains_vars_once(atom, |symbol| sub.get_sub_for(symbol).is_some(), can_apply)
    }

    /// Apply the given substitution to the given parameters, while handling
    /// shadowing.
    ///
    /// Returns filtered sub, with shadowed variables removed.
    pub fn properly_apply_sub_to_params(&self, params: ParamsId, sub: &Sub) -> (ParamsId, Sub) {
        let mut filtered_sub = sub.clone();
        let mut param_data = vec![];

        for param_id in params.iter() {
            let param = self.get_param(param_id);
            param_data.push(ParamData {
                name: param.name,
                ty: self.apply_sub_to_ty(param.ty, &filtered_sub),
                default: param.default.map(|term| self.apply_sub_to_term(term, &filtered_sub)),
            });
            filtered_sub.remove(param.name);
        }

        let new_params = self.param_utils().create_params(param_data.into_iter());
        (new_params, filtered_sub)
    }

    /// Apply the given substitution to the given atom,
    ///
    /// Returns `ControlFlow::Break(a)` with a new atom, or
    /// `ControlFlow::Continue(())` otherwise to recurse deeper.
    pub fn apply_sub_to_atom_once(&self, atom: Atom, sub: &Sub) -> ControlFlow<Atom> {
        if !self.can_apply_sub_to_atom(atom, sub) {
            return ControlFlow::Break(atom);
        }

        match atom {
            Atom::Ty(ty) => match self.get_ty(ty) {
                Ty::Hole(Hole(symbol)) | Ty::Var(symbol) => match sub.get_sub_for(symbol) {
                    Some(term) => ControlFlow::Break(Atom::Ty(self.use_term_as_ty(term))),
                    None => ControlFlow::Continue(()),
                },
                _ => ControlFlow::Continue(()),
            },
            Atom::Term(term) => match self.get_term(term) {
                Term::Hole(Hole(symbol)) | Term::Var(symbol) => match sub.get_sub_for(symbol) {
                    Some(term) => ControlFlow::Break(Atom::Term(term)),
                    None => ControlFlow::Continue(()),
                },
                _ => ControlFlow::Continue(()),
            },
            Atom::FnDef(_) | Atom::Pat(_) => ControlFlow::Continue(()),
        }
    }

    /// Below are convenience methods for specific atoms:
    pub fn atom_contains_vars(&self, atom: Atom, filter: impl Fn(Symbol) -> bool + Copy) -> bool {
        let mut can_apply = false;
        self.traversing_utils()
            .visit_atom::<!, _>(atom, &mut |atom| {
                Ok(self.atom_contains_vars_once(atom, filter, &mut can_apply))
            })
            .into_ok();
        can_apply
    }

    /// Below are convenience methods for specific atoms:
    pub fn can_apply_sub_to_atom(&self, atom: Atom, sub: &Sub) -> bool {
        let mut can_apply = false;
        self.traversing_utils()
            .visit_atom::<!, _>(atom, &mut |atom| {
                Ok(self.can_apply_sub_to_atom_once(atom, sub, &mut can_apply))
            })
            .into_ok();
        can_apply
    }

    pub fn apply_sub_to_atom(&self, atom: Atom, sub: &Sub) -> Atom {
        self.traversing_utils()
            .fmap_atom::<!, _>(atom, |atom| Ok(self.apply_sub_to_atom_once(atom, sub)))
            .into_ok()
    }

    pub fn apply_sub_to_term(&self, term_id: TermId, sub: &Sub) -> TermId {
        self.traversing_utils()
            .fmap_term::<!, _>(term_id, |atom| Ok(self.apply_sub_to_atom_once(atom, sub)))
            .into_ok()
    }

    pub fn apply_sub_to_pat(&self, pat_id: PatId, sub: &Sub) -> PatId {
        self.traversing_utils()
            .fmap_pat::<!, _>(pat_id, |atom| Ok(self.apply_sub_to_atom_once(atom, sub)))
            .into_ok()
    }

    pub fn apply_sub_to_ty(&self, ty_id: TyId, sub: &Sub) -> TyId {
        self.traversing_utils()
            .fmap_ty::<!, _>(ty_id, |atom| Ok(self.apply_sub_to_atom_once(atom, sub)))
            .into_ok()
    }

    pub fn apply_sub_to_term_in_place(&self, term_id: TermId, sub: &Sub) {
        self.traversing_utils()
            .visit_term::<!, _>(term_id, &mut |atom| {
                Ok(self.apply_sub_to_atom_in_place_once(atom, sub))
            })
            .into_ok()
    }

    pub fn apply_sub_to_ty_in_place(&self, ty_id: TyId, sub: &Sub) {
        self.traversing_utils()
            .visit_ty::<!, _>(
                ty_id,
                &mut |atom| Ok(self.apply_sub_to_atom_in_place_once(atom, sub)),
            )
            .into_ok()
    }

    pub fn apply_sub_to_args(&self, args_id: ArgsId, sub: &Sub) -> ArgsId {
        self.traversing_utils()
            .fmap_args::<!, _>(args_id, |atom| Ok(self.apply_sub_to_atom_once(atom, sub)))
            .into_ok()
    }

    pub fn apply_sub_to_params_in_place(&self, params_id: ParamsId, sub: &Sub) {
        self.traversing_utils()
            .visit_params::<!, _>(params_id, &mut |atom| {
                Ok(self.apply_sub_to_atom_in_place_once(atom, sub))
            })
            .into_ok()
    }

    pub fn apply_sub_to_params(&self, params_id: ParamsId, sub: &Sub) -> ParamsId {
        self.traversing_utils()
            .fmap_params::<!, _>(params_id, |atom| Ok(self.apply_sub_to_atom_once(atom, sub)))
            .into_ok()
    }

    pub fn apply_sub_to_args_in_place(&self, args_id: ArgsId, sub: &Sub) {
        self.traversing_utils()
            .visit_args::<!, _>(args_id, &mut |atom| {
                Ok(self.apply_sub_to_atom_in_place_once(atom, sub))
            })
            .into_ok()
    }

    /// Determines whether the given atom contains a hole.
    ///
    /// If a hole is found, `ControlFlow::Break(())` is returned. Otherwise,
    /// `ControlFlow::Continue(())` is returned. `has_holes` is updated
    /// accordingly.
    pub fn has_holes_once(&self, atom: Atom, has_holes: &mut Option<Atom>) -> ControlFlow<()> {
        match atom {
            Atom::Ty(ty) => match self.get_ty(ty) {
                Ty::Hole(_) => {
                    *has_holes = Some(atom);
                    ControlFlow::Break(())
                }
                _ => ControlFlow::Continue(()),
            },
            Atom::Term(term) => match self.get_term(term) {
                Term::Hole(_) => {
                    *has_holes = Some(atom);
                    ControlFlow::Break(())
                }
                _ => ControlFlow::Continue(()),
            },
            Atom::FnDef(_) | Atom::Pat(_) => ControlFlow::Continue(()),
        }
    }

    /// Determines whether the given atom contains one or more holes.
    pub fn atom_has_holes(&self, atom: impl Into<Atom>) -> Option<Atom> {
        let mut has_holes = None;
        self.traversing_utils()
            .visit_atom::<!, _>(atom.into(), &mut |atom| {
                Ok(self.has_holes_once(atom, &mut has_holes))
            })
            .into_ok();
        has_holes
    }

    /// Determines whether the given module definition contains one or more
    /// holes.
    pub fn mod_def_has_holes(&self, mod_def_id: ModDefId) -> Option<Atom> {
        let mut has_holes = None;
        self.traversing_utils()
            .visit_mod_def::<!, _>(mod_def_id, &mut |atom| {
                Ok(self.has_holes_once(atom, &mut has_holes))
            })
            .into_ok();
        has_holes
    }

    /// Determines whether the given set of arguments contains one or more
    /// holes.
    pub fn args_have_holes(&self, args_id: ArgsId) -> Option<Atom> {
        let mut has_holes = None;
        self.traversing_utils()
            .visit_args::<!, _>(args_id, &mut |atom| Ok(self.has_holes_once(atom, &mut has_holes)))
            .into_ok();
        has_holes
    }

    /// Determines whether the given set of parameters contains one or more
    /// holes.
    pub fn params_have_holes(&self, params_id: ParamsId) -> Option<Atom> {
        let mut has_holes = None;
        self.traversing_utils()
            .visit_params::<!, _>(params_id, &mut |atom| {
                Ok(self.has_holes_once(atom, &mut has_holes))
            })
            .into_ok();
        has_holes
    }

    /// Create a substitution from the current scope members.
    pub fn create_sub_from_current_scope(&self) -> Sub {
        let mut sub = Sub::identity();

        let current_scope_index = self.context().get_current_scope_index();
        self.context().for_bindings_of_scope_rev(current_scope_index, |binding| {
            if let Some(value) = self.context_utils().try_get_binding_value(binding.name) {
                self.insert_to_sub_if_needed(&mut sub, binding.name, value);
            }
        });

        sub
    }

    /// Insert the given variable and value into the given substitution if
    /// the value is not a variable with the same name.
    pub fn insert_to_sub_if_needed(&self, sub: &mut Sub, name: Symbol, value: TermId) {
        let subbed_value = self.apply_sub_to_term(value, sub);
        if !matches!(self.get_term(subbed_value), Term::Var(v) if v == name) {
            sub.insert(name, subbed_value);
        }
    }

    /// Create a substitution from the given arguments.
    ///
    /// Invariant: the arguments are ordered to match the
    /// parameters.
    pub fn create_sub_from_args_of_params(&self, args_id: ArgsId, params_id: ParamsId) -> Sub {
        assert!(params_id.len() == args_id.len(), "called with mismatched args and params");

        let mut sub = Sub::identity();
        for (param_id, arg_id) in (params_id.iter()).zip(args_id.iter()) {
            let param = self.stores().params().get_element(param_id);
            let arg = self.stores().args().get_element(arg_id);
            self.insert_to_sub_if_needed(&mut sub, param.name, arg.value);
        }
        sub
    }

    /// Create a substitution from the given source parameter names to the
    /// same names but prefixed with the access subject.
    pub fn create_sub_from_param_access(&self, params: ParamsId, access_subject: TermId) -> Sub {
        let mut sub = Sub::identity();
        for src in params.iter() {
            let src = self.stores().params().get_element(src);
            if let Some(ident) = self.get_param_name_ident(src.id) {
                sub.insert(
                    src.name,
                    self.new_term(AccessTerm {
                        subject: access_subject,
                        field: ParamIndex::Name(ident),
                    }),
                );
            }
        }
        sub
    }

    /// Create a substitution from the given source parameter names to the
    /// target parameter names.
    ///
    /// Invariant: the parameters unify.
    pub fn create_sub_from_param_names(
        &self,
        src_params: ParamsId,
        target_params: ParamsId,
    ) -> Sub {
        let mut sub = Sub::identity();
        for (src, target) in (src_params.iter()).zip(target_params.iter()) {
            let src = self.stores().params().get_element(src);
            let target = self.stores().params().get_element(target);
            if src.name != target.name {
                sub.insert(src.name, self.new_term(target.name));
            }
        }
        sub
    }

    /// Hide the given set of parameters from the substitution.
    pub fn hide_param_binds(&self, params: impl IntoIterator<Item = ParamId>, sub: &Sub) -> Sub {
        let mut shadowed_sub = Sub::identity();
        let param_names =
            params.into_iter().map(|p| self.get_param(p).name).collect::<HashSet<_>>();

        for (name, value) in sub.iter() {
            // If the substitution is from that parameter, skip it.
            if param_names.contains(&name) {
                continue;
            }
            // If the substitution is to that parameter, skip it.
            if self.atom_contains_vars(value.into(), |v| param_names.contains(&v)) {
                continue;
            }

            shadowed_sub.insert(name, value);
        }

        shadowed_sub
    }

    /// Reverse the given substitution.
    ///
    /// Invariant: the substitution is injective.
    pub fn reverse_sub(&self, sub: &Sub) -> Sub {
        let mut reversed_sub = Sub::identity();
        for (name, value) in sub.iter() {
            match self.get_term(value) {
                Term::Var(v) => {
                    reversed_sub.insert(v, self.new_term(name));
                }
                Term::Hole(h) => {
                    reversed_sub.insert(h.0, self.new_term(name));
                }
                _ => {
                    panic!("cannot reverse non-injective substitution");
                }
            }
        }
        reversed_sub
    }

    /// Copies an atom, returning a new atom.
    pub fn copy_atom(&self, atom: Atom) -> Atom {
        self.traversing_utils()
            .fmap_atom::<!, _>(atom, |_a| Ok(ControlFlow::Continue(())))
            .into_ok()
    }

    /// Copies a type, returning a new type.
    pub fn copy_ty(&self, ty: TyId) -> TyId {
        self.traversing_utils().fmap_ty::<!, _>(ty, |_a| Ok(ControlFlow::Continue(()))).into_ok()
    }

    /// Copies parameters, returning new parameters.
    pub fn copy_params(&self, params: ParamsId) -> ParamsId {
        self.traversing_utils()
            .fmap_params::<!, _>(params, |_a| Ok(ControlFlow::Continue(())))
            .into_ok()
    }
}
