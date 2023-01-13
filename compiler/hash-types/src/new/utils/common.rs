// @@Docs

use hash_source::{identifier::Identifier, location::SourceLocation};
use hash_utils::store::{CloneStore, SequenceStore, SequenceStoreKey, Store};

use crate::new::{
    args::{ArgsId, PatArgsId},
    data::{DataDef, DataDefId, DataTy},
    defs::{DefArgsId, DefParamGroup, DefParamsId, DefPatArgsId},
    environment::env::AccessToEnv,
    fns::{FnDef, FnDefId},
    holes::{Hole, HoleKind},
    locations::LocationTarget,
    params::{DefParamIndex, Param, ParamIndex, ParamsId},
    pats::{Pat, PatId, PatListId},
    symbols::{Symbol, SymbolData},
    terms::{Term, TermId, TermListId},
    tuples::{TupleTerm, TupleTy},
    tys::{Ty, TyId, UniverseTy},
};

/// Assert that the given term is of the given variant, and return it.
#[macro_export]
macro_rules! term_as_variant {
    ($self:expr, value $term:expr, $variant:ident) => {{
        let term = $term;
        if let $crate::new::terms::Term::$variant(term) = term {
            term
        } else {
            panic!("Expected term to be a {}", stringify!($variant))
        }
    }};
}

/// Assert that the given type is of the given variant, and return it.
#[macro_export]
macro_rules! ty_as_variant {
    ($self:expr, value $ty:expr, $variant:ident) => {{
        let ty = $ty;
        if let hash_types::new::tys::Ty::$variant(ty) = ty {
            ty
        } else {
            panic!("Expected type to be a {}", stringify!($variant))
        }
    }};
}

pub trait CommonUtils: AccessToEnv {
    /// Check whether the given term is a void term (i.e. empty tuple).
    fn term_is_void(&self, term_id: TermId) -> bool {
        matches! {
          self.stores().term().get(term_id),
          Term::Tuple(tuple_term) if tuple_term.data.is_empty()
        }
    }

    /// Get the parameter of the given parameters ID and index which is
    /// either symbolic or positional.
    ///
    /// This will panic if the index does not exist.
    fn get_param_by_index(&self, params_id: ParamsId, index: ParamIndex) -> Param {
        match index {
            ParamIndex::Name(name) => self.stores().params().map_fast(params_id, |params| {
                params
                    .iter()
                    .find_map(|x| {
                        if self.stores().symbol().get(x.name).name? == name {
                            Some(*x)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "Parameter with name `{}` does not exist in `{}`",
                            name,
                            self.env().with(params_id)
                        )
                    })
            }),
            ParamIndex::Position(i) => {
                self.stores().params().map_fast(params_id, |params| params[i])
            }
        }
    }

    /// Get the parameter group of the given definition parameters ID and
    /// positional index.
    ///
    /// This will panic if the index does not exist.
    fn get_param_group_by_index(&self, def_params_id: DefParamsId, index: usize) -> DefParamGroup {
        self.stores().def_params().map_fast(def_params_id, |def_params| def_params[index])
    }

    /// Get the parameter of the given definition parameters ID and
    /// definition parameter index.
    ///
    /// This will panic if the index does not exist.
    fn get_def_param_by_index(&self, def_params_id: DefParamsId, index: DefParamIndex) -> Param {
        let params = self.get_param_group_by_index(def_params_id, index.group_index).params;
        self.get_param_by_index(params, index.param_index)
    }

    /// Create a new symbol with the given name.
    fn new_symbol(&self, name: impl Into<Identifier>) -> Symbol {
        self.stores().symbol().create_with(|symbol| SymbolData { name: Some(name.into()), symbol })
    }

    /// Create a new empty parameter list.
    fn new_empty_params(&self) -> ParamsId {
        self.stores().params().create_from_slice(&[])
    }

    /// Get a term by its ID.
    fn get_term(&self, term_id: TermId) -> Term {
        self.stores().term().get(term_id)
    }

    /// Get a type by its ID.
    fn get_ty(&self, ty_id: TyId) -> Ty {
        self.stores().ty().get(ty_id)
    }

    /// Get a pattern by its ID.
    fn get_pat(&self, pat_id: PatId) -> Pat {
        self.stores().pat().get(pat_id)
    }

    /// Get a data definition by its ID.
    fn get_data_def(&self, data_def_id: DataDefId) -> DataDef {
        self.stores().data_def().get(data_def_id)
    }

    /// Get a function definition by its ID.
    fn get_fn_def(&self, fn_def_id: FnDefId) -> FnDef {
        self.stores().fn_def().get(fn_def_id)
    }

    /// Get the location of a location target.
    fn get_location(&self, target: impl Into<LocationTarget>) -> Option<SourceLocation> {
        self.stores().location().get_location(target)
    }

    /// Get symbol data.
    fn get_symbol(&self, symbol: Symbol) -> SymbolData {
        self.stores().symbol().get(symbol)
    }

    /// Duplicate a symbol by creating a new symbol with the same name.
    fn duplicate_symbol(&self, existing_symbol: Symbol) -> Symbol {
        let existing_symbol_name = self.stores().symbol().map_fast(existing_symbol, |s| s.name);
        self.stores()
            .symbol()
            .create_with(|symbol| SymbolData { name: existing_symbol_name, symbol })
    }

    /// Create a new type.
    fn new_ty(&self, ty: impl Into<Ty>) -> TyId {
        self.stores().ty().create(ty.into())
    }

    /// Create a new term.
    fn new_term(&self, term: impl Into<Term>) -> TermId {
        self.stores().term().create(term.into())
    }

    /// Create a new term list.
    fn new_term_list(&self, terms: impl IntoIterator<Item = TermId>) -> TermListId {
        let terms = terms.into_iter().collect::<Vec<_>>();
        self.stores().term_list().create_from_slice(&terms)
    }

    /// Create a new pattern.
    fn new_pat(&self, pat: Pat) -> PatId {
        self.stores().pat().create(pat)
    }

    /// Create a new pattern list.
    fn new_pat_list(&self, pats: impl IntoIterator<Item = PatId>) -> PatListId {
        let pats = pats.into_iter().collect::<Vec<_>>();
        self.stores().pat_list().create_from_slice(&pats)
    }

    /// Create a new internal symbol.
    fn new_fresh_symbol(&self) -> Symbol {
        self.stores().symbol().create_with(|symbol| SymbolData { name: None, symbol })
    }

    /// Create a new term hole.
    fn new_term_hole(&self) -> TermId {
        let hole_id = self.stores().hole().create_with(|id| Hole { id, kind: HoleKind::Term });
        self.stores().term().create_with(|_| Term::Hole(hole_id))
    }

    /// Create a new type hole.
    fn new_ty_hole(&self) -> TyId {
        let hole_id = self.stores().hole().create_with(|id| Hole { id, kind: HoleKind::Ty });
        self.stores().ty().create_with(|_| Ty::Hole(hole_id))
    }

    /// Create a new empty definition parameter list.
    fn new_empty_def_params(&self) -> DefParamsId {
        self.stores().def_params().create_from_slice(&[])
    }

    /// Create a new empty definition argument list.
    fn new_empty_def_args(&self) -> DefArgsId {
        self.stores().def_args().create_from_slice(&[])
    }

    /// Create a new empty argument list.
    fn new_empty_args(&self) -> ArgsId {
        self.stores().args().create_from_slice(&[])
    }

    /// Create a new positional parameter list with the given types.
    fn new_params(&self, types: &[TyId]) -> ParamsId {
        self.stores().params().create_from_iter_with(types.iter().copied().map(|ty| {
            move |id| Param { id, name: self.new_fresh_symbol(), ty, default_value: None }
        }))
    }

    /// Create a new data type with no arguments.
    fn new_data_ty(&self, data_def: DataDefId) -> TyId {
        self.stores().ty().create(Ty::Data(DataTy { data_def, args: self.new_empty_def_args() }))
    }

    /// Create a new empty pattern argument list.
    fn new_empty_pat_args(&self) -> PatArgsId {
        self.stores().pat_args().create_from_slice(&[])
    }

    /// Create a new empty pattern definition argument list.
    fn new_empty_def_pat_args(&self) -> DefPatArgsId {
        self.stores().def_pat_args().create_from_slice(&[])
    }

    /// Create a type of types, i.e. small `Type`.
    fn new_small_universe_ty(&self) -> TyId {
        self.stores().ty().create(Ty::Universe(UniverseTy { size: 0 }))
    }

    /// Create a large type of types, i.e. `Type(n)` for some natural number
    /// `n`.
    fn new_universe_ty(&self, n: usize) -> TyId {
        self.stores().ty().create(Ty::Universe(UniverseTy { size: n }))
    }

    /// Create a new empty tuple type.
    fn new_void_ty(&self) -> TyId {
        self.stores().ty().create(Ty::Tuple(TupleTy { data: self.new_empty_params() }))
    }

    /// Create a new empty tuple term.
    fn new_void_term(&self) -> TermId {
        self.stores().term().create(Term::Tuple(TupleTerm {
            data: self.new_empty_args(),
            original_ty: Some(TupleTy { data: self.new_empty_params() }),
        }))
    }

    /// Create a new variable type.
    fn new_var_ty(&self, symbol: Symbol) -> TyId {
        self.stores().ty().create(Ty::Var(symbol))
    }
}

impl<T: AccessToEnv> CommonUtils for T {}