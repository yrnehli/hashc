//! Utilities for parameters and arguments.
use derive_more::Constructor;
use hash_utils::store::{SequenceStore, Store};
use itertools::Itertools;

use super::common::CommonUtils;
use crate::{
    impl_access_to_env,
    new::{
        args::{Arg, ArgData, ArgsId, PatArg, PatArgData, PatArgsId},
        data::DataDefId,
        environment::env::{AccessToEnv, Env},
        params::{Param, ParamData, ParamIndex, ParamsId},
        symbols::Symbol,
        terms::TermId,
    },
};

#[derive(Constructor)]
pub struct ParamUtils<'env> {
    env: &'env Env<'env>,
}

impl_access_to_env!(ParamUtils<'env>);

impl<'env> ParamUtils<'env> {
    /// Create a new parameter list with the given names, and holes for all
    /// types.
    pub fn create_hole_params(
        &self,
        param_names: impl Iterator<Item = Symbol> + ExactSizeIterator,
    ) -> ParamsId {
        self.stores().params().create_from_iter_with(
            param_names.map(|name| move |id| Param { id, name, ty: self.new_ty_hole() }),
        )
    }

    /// Create parameters from the given iterator of parameter data.
    pub fn create_params(
        &self,
        params: impl Iterator<Item = ParamData> + ExactSizeIterator,
    ) -> ParamsId {
        self.stores().params().create_from_iter_with(
            params.map(|data| move |id| Param { id, name: data.name, ty: data.ty }),
        )
    }

    /// Create arguments from the given iterator of argument data.
    pub fn create_args(&self, args: impl Iterator<Item = ArgData> + ExactSizeIterator) -> ArgsId {
        self.stores().args().create_from_iter_with(
            args.map(|data| move |id| Arg { id, target: data.target, value: data.value }),
        )
    }

    /// Create pattern arguments from the given iterator of argument data.
    pub fn create_pat_args(
        &self,
        args: impl Iterator<Item = PatArgData> + ExactSizeIterator,
    ) -> PatArgsId {
        self.stores().pat_args().create_from_iter_with(
            args.map(|data| move |id| PatArg { id, target: data.target, pat: data.pat }),
        )
    }

    /// Create definition arguments for the given data definition
    ///
    /// Each argument will be a positional argument. Note that the outer
    /// iterator is for the argument groups, and the inner iterator is for
    /// the arguments in each group.
    pub fn create_positional_args_for_data_def(
        &self,
        def: DataDefId,
        args: impl IntoIterator<Item = TermId>,
    ) -> ArgsId {
        let _params = self.stores().data_def().map_fast(def, |def| def.params);
        self.create_args(
            args.into_iter()
                .enumerate()
                .map(|(j, value)| ArgData { target: ParamIndex::Position(j), value })
                .collect_vec()
                .into_iter(),
        )
    }
}