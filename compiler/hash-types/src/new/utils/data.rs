//! Data definition-related utilities.
use std::iter::{empty, once};

use derive_more::Constructor;
use hash_utils::store::{SequenceStore, SequenceStoreKey, Store};
use itertools::Itertools;

use super::{common::CommonUtils, AccessToUtils};
use crate::{
    impl_access_to_env,
    new::{
        args::ArgData,
        data::{
            CtorDef, CtorDefData, CtorDefsId, DataDef, DataDefCtors, DataDefId, PrimitiveCtorInfo,
        },
        defs::{DefArgGroupData, DefArgsId, DefParamGroupData, DefParamsId},
        environment::env::{AccessToEnv, Env},
        params::{ParamIndex, ParamsId},
        symbols::Symbol,
        terms::Term,
    },
};

/// Data definition-related operations.
#[derive(Constructor)]
pub struct DataUtils<'tc> {
    env: &'tc Env<'tc>,
}

impl_access_to_env!(DataUtils<'tc>);

impl<'tc> DataUtils<'tc> {
    /// Create an empty data definition.
    pub fn new_empty_data_def(&self, name: Symbol, params: DefParamsId) -> DataDefId {
        self.stores().data_def().create_with(|id| DataDef {
            id,
            name,
            params,
            ctors: DataDefCtors::Defined(self.stores().ctor_defs().create_from_slice(&[])),
        })
    }

    /// Set the constructors of the given data definition.
    pub fn set_data_def_ctors(&self, data_def: DataDefId, ctors: CtorDefsId) -> CtorDefsId {
        self.stores().data_def().modify_fast(data_def, |data_def| {
            data_def.ctors = DataDefCtors::Defined(ctors);
        });
        ctors
    }

    /// Create a primitive data definition.
    pub fn create_primitive_data_def(&self, name: Symbol, info: PrimitiveCtorInfo) -> DataDefId {
        self.stores().data_def().create_with(|id| DataDef {
            id,
            name,
            params: self.new_empty_def_params(),
            ctors: DataDefCtors::Primitive(info),
        })
    }

    /// Create a primitive data definition with parameters.
    ///
    /// These may be referenced in `info`.
    pub fn create_primitive_data_def_with_params(
        &self,
        name: Symbol,
        params: DefParamsId,
        info: impl FnOnce(DataDefId) -> PrimitiveCtorInfo,
    ) -> DataDefId {
        self.stores().data_def().create_with(|id| DataDef {
            id,
            name,
            params,
            ctors: DataDefCtors::Primitive(info(id)),
        })
    }

    /// Create data constructors from the given iterator, for the given data
    /// definition.
    pub fn create_data_ctors<I: IntoIterator<Item = CtorDefData>>(
        &self,
        data_def_id: DataDefId,
        data: I,
    ) -> CtorDefsId
    where
        I::IntoIter: ExactSizeIterator,
    {
        self.stores().ctor_defs().create_from_iter_with(data.into_iter().enumerate().map(
            |(index, data)| {
                move |id| CtorDef {
                    id,
                    name: data.name,
                    data_def_id,
                    data_def_ctor_index: index,
                    params: data.params,
                    result_args: data.result_args,
                }
            },
        ))
    }

    /// From the given definition parameters corresponding to the given data
    /// definition, create definition arguments that directly refer to the
    /// parameters from the data definition scope.
    ///
    /// Example:
    /// ```notrust
    /// X := datatype <A: Type, B: Type, C: Type> { foo: () -> X<A, B, C> }
    ///                                                         ^^^^^^^^^ this is what this function creates
    /// ```
    pub fn create_forwarded_data_args_from_params(
        &self,
        _data_def_id: DataDefId,
        def_params_id: DefParamsId,
    ) -> DefArgsId {
        self.stores().def_params().map(def_params_id, |def_params| {
            // For each parameter group, create an argument group
            self.param_utils().create_def_args(def_params.iter().enumerate().map(
                |(i, def_param_group)| {
                    // For the parameter list inside the group, create an argument
                    // list
                    let args = self.param_utils().create_args(self.stores().params().map(
                        def_param_group.params,
                        |params| {
                            // For each parameter, create an argument referring to it
                            params
                                .iter()
                                .enumerate()
                                .map(|(_j, param)| ArgData {
                                    target: ParamIndex::Position(i),
                                    value: self.new_term(Term::Var(param.name)),
                                })
                                .collect_vec()
                                .into_iter()
                        },
                    ));
                    DefArgGroupData { param_group: (def_params_id, i), args }
                },
            ))
        })
    }

    /// Create a struct data definition, with some parameters.
    ///
    /// The fields are given as an iterator of `ParamData`s, which are the names
    /// and types of the fields.
    ///
    /// This will create a data definition with a single constructor, which
    /// takes the fields as parameters and returns the data type.
    pub fn create_struct_def(
        &self,
        name: Symbol,
        params: DefParamsId,
        fields_params: ParamsId,
    ) -> DataDefId {
        // The field parameters correspond to a single parameter group
        let fields_def_params = self
            .param_utils()
            .create_def_params(once(DefParamGroupData { implicit: false, params: fields_params }));

        // Create the arguments for the constructor, which are the type
        // parameters given.
        let result_args =
            |data_def_id| self.create_forwarded_data_args_from_params(data_def_id, params);

        // Create the data definition
        self.stores().data_def().create_with(|id| DataDef {
            id,
            name,
            params,
            ctors: DataDefCtors::Defined(self.stores().ctor_defs().create_from_iter_with(once(
                |ctor_id| {
                    CtorDef {
                        id: ctor_id,
                        // Name of the constructor is the same as the data definition, though we
                        // need to create a new symbol for it
                        name: self.duplicate_symbol(name),
                        // The constructor is the only one
                        data_def_ctor_index: 0,
                        data_def_id: id,
                        params: fields_def_params,
                        result_args: result_args(id),
                    }
                },
            ))),
        })
    }

    /// Create an enum definition, with some parameters.
    ///
    /// The variants are given as an iterator of `(Symbol, Iter<ParamData>)`,
    /// which are the names and fields of the variants.
    ///
    /// This will create a data definition with a constructor for each variant,
    /// which takes the variant fields as parameters and returns the data
    /// type.
    pub fn create_enum_def(
        &self,
        name: Symbol,
        params: DefParamsId,
        variants: impl Fn(DataDefId) -> Vec<(Symbol, ParamsId)>,
    ) -> DataDefId {
        // Create the arguments for the constructor, which are the type
        // parameters given.
        let result_args =
            |data_def_id| self.create_forwarded_data_args_from_params(data_def_id, params);

        // Create the data definition for the enum
        self.stores().data_def().create_with(|id| DataDef {
            id,
            name,
            params,
            ctors: DataDefCtors::Defined(self.stores().ctor_defs().create_from_iter_with(
                variants(id).into_iter().enumerate().map(|(index, variant)| {
                    let variant_name = variant.0;
                    let fields_params = variant.1;

                    // The field parameters correspond to a single parameter group
                    let fields_def_params = if !fields_params.is_empty() {
                        self.param_utils().create_def_params(once(DefParamGroupData {
                            implicit: false,
                            params: fields_params,
                        }))
                    } else {
                        self.param_utils().create_def_params(empty())
                    };

                    // Create a constructor for each variant
                    move |ctor_id| CtorDef {
                        id: ctor_id,
                        name: variant_name,
                        data_def_ctor_index: index,
                        data_def_id: id,
                        params: fields_def_params,
                        result_args: result_args(id),
                    }
                }),
            )),
        })
    }
}