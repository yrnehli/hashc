//! Operations to infer types of terms and patterns.
use derive_more::Constructor;
use hash_ast::ast::{FloatLitKind, IntLitKind};
use hash_intrinsics::primitives::AccessToPrimitives;
use hash_source::constant::{FloatTy, IntTy, SIntTy, UIntTy};
use hash_types::new::{
    args::{ArgsId, PatArgsId},
    data::{CtorTerm, DataTy},
    defs::{DefArgsId, DefParamsId, DefPatArgsId},
    environment::env::AccessToEnv,
    fns::FnCallTerm,
    lits::{Lit, PrimTerm},
    params::ParamsId,
    refs::DerefTerm,
    terms::{RuntimeTerm, Term, TermId},
    tuples::{TupleTerm, TupleTy},
    tys::{Ty, TyId},
    utils::{common::CommonUtils, AccessToUtils},
};
use hash_utils::store::{CloneStore, SequenceStore};

use super::{common::CommonOps, AccessToOps};
use crate::{
    impl_access_to_tc_env,
    new::{
        diagnostics::error::{TcError, TcResult},
        environment::tc_env::TcEnv,
    },
};

#[derive(Constructor)]
pub struct InferOps<'tc> {
    tc_env: &'tc TcEnv<'tc>,
}

impl_access_to_tc_env!(InferOps<'tc>);

impl<'tc> InferOps<'tc> {
    pub fn infer_def_params_of_def_pat_args(
        &self,
        _def_pat_args: DefPatArgsId,
    ) -> TcResult<DefParamsId> {
        todo!()
    }

    pub fn infer_def_params_of_def_args(&self, _def_args: DefArgsId) -> TcResult<DefParamsId> {
        todo!()
    }

    pub fn infer_params_of_pat_args(&self, _pat_args: PatArgsId) -> TcResult<ParamsId> {
        todo!()
    }

    pub fn infer_params_of_args(&self, _args: ArgsId) -> TcResult<ParamsId> {
        todo!()
    }

    /// Infer the type of a sequence of terms which should all match.
    pub fn infer_unified_ty_of_terms(&self, _terms: &[TermId]) -> TcResult<TyId> {
        todo!()
    }

    /// Infer the type of a term, or create a new a type hole.
    pub fn infer_ty_of_term_or_hole(&self, term: TermId) -> TcResult<TyId> {
        Ok(self.infer_ty_of_term(term)?.unwrap_or_else(|| self.new_ty_hole()))
    }

    /// Infer the type of a runtime term.
    pub fn infer_ty_of_runtime_term(&self, term: &RuntimeTerm) -> TyId {
        term.term_ty
    }

    /// Infer the type of a tuple term.
    pub fn infer_ty_of_tuple_term(&self, term: &TupleTerm) -> TcResult<TupleTy> {
        match term.original_ty {
            Some(ty) => Ok(ty),
            None => Ok(TupleTy { data: self.infer_params_of_args(term.data)? }),
        }
    }

    /// Infer the type of a primitive term.
    pub fn infer_ty_of_prim_term(&self, term: &PrimTerm) -> TcResult<TyId> {
        match term {
            PrimTerm::Lit(lit_term) => Ok(self.new_data_ty(match lit_term {
                Lit::Int(int_lit) => match int_lit.underlying.kind {
                    IntLitKind::Suffixed(suffix) => match suffix {
                        IntTy::Int(s_int_ty) => match s_int_ty {
                            SIntTy::I8 => self.primitives().i8(),
                            SIntTy::I16 => self.primitives().i16(),
                            SIntTy::I32 => self.primitives().i32(),
                            SIntTy::I64 => self.primitives().i64(),
                            SIntTy::I128 => self.primitives().i128(),
                            SIntTy::ISize => self.primitives().isize(),
                            SIntTy::IBig => self.primitives().ibig(),
                        },
                        IntTy::UInt(u_int_ty) => match u_int_ty {
                            UIntTy::U8 => self.primitives().u8(),
                            UIntTy::U16 => self.primitives().u16(),
                            UIntTy::U32 => self.primitives().u32(),
                            UIntTy::U64 => self.primitives().u64(),
                            UIntTy::U128 => self.primitives().u128(),
                            UIntTy::USize => self.primitives().usize(),
                            UIntTy::UBig => self.primitives().ubig(),
                        },
                    },
                    // By default, we assume that all integers are i32 unless annotated otherwise.
                    IntLitKind::Unsuffixed => self.primitives().i32(),
                },
                Lit::Str(_) => self.primitives().str(),
                Lit::Char(_) => self.primitives().char(),
                Lit::Float(float) => match float.underlying.kind {
                    FloatLitKind::Suffixed(float_suffix) => match float_suffix {
                        FloatTy::F32 => self.primitives().f32(),
                        FloatTy::F64 => self.primitives().f64(),
                    },
                    // By default, we assume that all floats are f32 unless annotated otherwise.
                    FloatLitKind::Unsuffixed => self.primitives().f32(),
                },
            })),
            PrimTerm::List(list_term) => {
                let list_inner_type =
                    self.stores().term_list().map_fast(list_term.elements, |elements| {
                        self.try_or_add_error(self.infer_unified_ty_of_terms(elements))
                            .unwrap_or_else(|| self.new_ty_hole())
                    });
                let list_ty = self.new_ty(DataTy {
                    data_def: self.primitives().list(),
                    args: self.param_utils().create_positional_args_for_data_def(
                        self.primitives().list(),
                        [[self.new_term(Term::Ty(list_inner_type))]],
                    ),
                });
                Ok(list_ty)
            }
        }
    }

    /// Infer the type of a constructor term.
    pub fn infer_ty_of_ctor_term(&self, term: &CtorTerm) -> DataTy {
        let data_def =
            self.stores().ctor_defs().map_fast(term.ctor.0, |terms| terms[term.ctor.1].data_def_id);
        DataTy { data_def, args: term.data_args }
    }

    /// Infer the type of a function call.
    pub fn infer_ty_of_fn_call_term(
        &self,
        term: &FnCallTerm,
        original_term_id: TermId,
    ) -> TcResult<Option<TyId>> {
        match self.infer_ty_of_term(term.subject)? {
            Some(subject_ty) => self.map_ty(subject_ty, |subject| match subject {
                Ty::Eval(_) => {
                    // @@Todo: Normalise
                    Ok(None)
                }
                Ty::Ref(_) => {
                    // Try the same thing, but with the dereferenced type.
                    let new_subject =
                        self.new_term(Term::Deref(DerefTerm { subject: term.subject }));
                    self.infer_ty_of_fn_call_term(
                        &FnCallTerm { subject: new_subject, ..*term },
                        original_term_id,
                    )
                    .map_err(|err| {
                        if matches!(err, TcError::NotAFunction { .. }) {
                            // Show it with the reference type:
                            TcError::NotAFunction {
                                fn_call: original_term_id,
                                actual_subject_ty: subject_ty,
                            }
                        } else {
                            err
                        }
                    })
                }
                Ty::Fn(fn_ty) => {
                    // First infer the parameters of the function call.
                    let inferred_fn_call_params = self.infer_params_of_args(term.args)?;

                    // Unify the parameters of the function call with the parameters of the
                    // function type.
                    let sub =
                        self.unify_ops().unify_params(inferred_fn_call_params, fn_ty.params)?;

                    // Apply the substitution to the arguments.
                    self.substitute_ops().apply_sub_to_args_in_place(term.args, &sub);

                    // Create a substitution from the parameters of the function type to the
                    // parameters of the function call.
                    let arg_sub = self
                        .substitute_ops()
                        .create_sub_from_applying_args_to_params(term.args, fn_ty.params)?;

                    // Apply the substitution to the return type of the function type.
                    let subbed_return_ty =
                        self.substitute_ops().apply_sub_to_ty(fn_ty.return_ty, &arg_sub);

                    Ok(Some(subbed_return_ty))
                }
                Ty::Universe(_) | Ty::Data(_) | Ty::Tuple(_) | Ty::Var(_) => {
                    // Type variable is not a function type.
                    Err(TcError::NotAFunction {
                        fn_call: original_term_id,
                        actual_subject_ty: subject_ty,
                    })
                }
                Ty::Hole(_) => Ok(None),
            }),
            None => {
                // We don't know the type of the subject, so we can't infer the type of the
                // call.
                Ok(None)
            }
        }
    }

    /// Infer a concrete type for a given term.
    ///
    /// If this is not possible, return `None`.
    /// To create a hole when this is not possible, use
    /// [`InferOps::infer_ty_of_term_or_hole`].
    pub fn infer_ty_of_term(&self, term_id: TermId) -> TcResult<Option<TyId>> {
        self.stores().term().map(term_id, |term| {
            match term {
                Term::Runtime(rt_term) => Ok(Some(self.infer_ty_of_runtime_term(rt_term))),
                Term::Tuple(tuple_term) => {
                    Ok(Some(self.new_ty(self.infer_ty_of_tuple_term(tuple_term)?)))
                }
                Term::Prim(prim_term) => Ok(Some(self.infer_ty_of_prim_term(prim_term)?)),
                Term::Ctor(ctor_term) => {
                    Ok(Some(self.new_ty(self.infer_ty_of_ctor_term(ctor_term))))
                }
                Term::FnCall(_) => todo!(),
                Term::FnRef(_fn_def_id) => todo!(),
                Term::Block(_) => todo!(),
                Term::Var(_) => todo!(),
                Term::Loop(_) => {
                    // @@Future: if loop is proven to not break, return never
                    todo!()
                }
                Term::LoopControl(_) => todo!(),
                Term::Match(_) => todo!(),
                Term::Return(_) => todo!(),
                Term::DeclStackMember(_) => todo!(),
                Term::Assign(_) => todo!(),
                Term::Unsafe(_) => todo!(),
                Term::Access(_) => todo!(),
                Term::Cast(_) => todo!(),
                Term::TypeOf(_) => todo!(),
                Term::Ty(_ty_id) => {
                    todo!()
                    // match self.get_ty(ty_id) {
                    //     Ty::Hole(_) =>
                    // Err(TcError::NeedMoreTypeAnnotationsToInfer { term }),
                    //     Ty::Tuple(_) | Ty::Fn(_) | Ty::Ref(_) | Ty::Data(_)
                    // => {         // @@Todo: bounds
                    //         Ok(self.new_small_universe_ty())
                    //     }
                    //     Ty::Universe(universe_ty) =>
                    // Ok(self.new_universe_ty(universe_ty.size + 1)),
                    //     Ty::Var(_) => todo!(),
                    //     Ty::Eval(_) => todo!(),
                    // }
                }
                Term::Ref(_ref_term) => {
                    todo!()
                    // let inner_ty =
                    // self.infer_ty_of_term_or_hole(ref_term.subject);
                    // Ok(Some(self.new_ty(Ty::Ref(RefTy {
                    //     ty: inner_ty,
                    //     mutable: ref_term.mutable,
                    //     kind: ref_term.kind,
                    // }))))
                }
                Term::Deref(_) => todo!(),
                Term::Hole(_) => {
                    todo!()
                }
            }
        })
    }
}
