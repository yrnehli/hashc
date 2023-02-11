use std::collections::HashSet;

use derive_more::{Constructor, Deref};
use hash_tir::new::{
    args::{ArgData, ArgsId, PatArgData, PatArgsId, PatOrCapture, SomeArgId, SomeArgsId},
    params::{ParamId, ParamIndex, ParamsId},
    pats::Spread,
    utils::{common::CommonUtils, AccessToUtils},
};
use hash_utils::store::{SequenceStore, SequenceStoreKey};

use crate::{errors::TcResult, AccessToTypechecking};

#[derive(Constructor, Deref)]
pub struct ParamOps<'a, T: AccessToTypechecking>(&'a T);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamError {
    TooManyArgs { expected: ParamsId, got: SomeArgsId },
    DuplicateArg { first: SomeArgId, second: SomeArgId },
    DuplicateParam { first: ParamId, second: ParamId },
    PositionalArgAfterNamedArg { first_named: SomeArgId, next_positional: SomeArgId },
    RequiredParamAfterDefaultParam { first_default: ParamId, next_required: ParamId },
    ArgNameNotFoundInParams { arg: SomeArgId, params: ParamsId },
    ParamNameMismatch { param_a: ParamId, param_b: ParamId },
    RequiredParamNotFoundInArgs { param: ParamId, args: SomeArgsId },
    SpreadBeforePositionalArg { next_positional: SomeArgId },
}

impl<T: AccessToTypechecking> ParamOps<'_, T> {
    /// Validate the given parameters, returning an error if they are invalid.
    ///
    /// Conditions for valid parameters are:
    /// 1. No duplicate parameter names
    /// 2. All parameters with defaults are at the end
    pub fn validate_params(&self, params_id: ParamsId) -> TcResult<()> {
        let mut error_state = self.new_error_state();

        let mut seen = HashSet::new();
        let mut found_default = None;

        for param in params_id.iter() {
            // Check for duplicate parameters
            if let Some(param_name) = self.get_param_name_ident(param) {
                if seen.contains(&param_name) {
                    error_state
                        .add_error(ParamError::DuplicateParam { first: param, second: param });
                }
                seen.insert(param_name);
            }

            // Ensure that parameters with defaults follow parameters without
            // defaults
            if let Some(default_param) = found_default {
                if self.get_param_default(param).is_none() {
                    // Required parameter after named parameter,
                    // error
                    error_state.add_error(ParamError::RequiredParamAfterDefaultParam {
                        first_default: default_param,
                        next_required: param,
                    });
                }
            } else if self.get_param_default(param).is_some() {
                // Found the first default parameter
                found_default = Some(param);
            }
        }

        self.return_or_register_errors(|| Ok(()), error_state)
    }

    /// Validate the given arguments against the given parameters, returning an
    /// error if they are invalid.
    ///
    /// Conditions for valid arguments are:
    /// 1. No duplicate argument names
    /// 2. All named arguments follow positional arguments
    /// 3. No more arguments than parameters
    ///
    /// The specific unification of the argument and parameter types is not
    /// checked at this stage. The function
    /// `validate_and_reorder_args_against_params` performs additional
    /// validation of the argument names, reorders the arguments to match
    /// the parameters, and fills in default arguments.
    pub fn validate_args_against_params(
        &self,
        args_id: SomeArgsId,
        params_id: ParamsId,
    ) -> TcResult<()> {
        let mut error_state = self.new_error_state();

        // Check for too many arguments
        if args_id.len() > params_id.len() {
            error_state.add_error(ParamError::TooManyArgs { expected: params_id, got: args_id });
        }

        let mut seen = HashSet::new();
        let mut found_named = None;

        for arg in args_id.iter() {
            // Check for duplicate arguments
            match self.get_arg_index(arg) {
                ParamIndex::Name(arg_name) => {
                    if seen.contains(&arg_name) {
                        error_state.add_error(ParamError::DuplicateArg {
                            first: arg.into(),
                            second: arg.into(),
                        });
                    }
                    seen.insert(arg_name);
                }
                ParamIndex::Position(_) => {
                    // no-op, we assume that there are no duplicate positional
                    // arguments..
                }
            }

            // Ensure that named arguments follow positional arguments
            match found_named {
                Some(named_arg) => {
                    match self.get_arg_index(arg) {
                        ParamIndex::Name(_) => {
                            // Named arguments, ok
                        }
                        ParamIndex::Position(_) => {
                            // Positional arguments after named arguments, error
                            error_state.add_error(ParamError::PositionalArgAfterNamedArg {
                                first_named: named_arg,
                                next_positional: arg.into(),
                            });
                        }
                    }
                }
                None => match self.get_arg_index(arg) {
                    ParamIndex::Name(_) => {
                        // Found the first named argument
                        found_named = Some(arg.into());
                    }
                    ParamIndex::Position(_) => {
                        // Still positional arguments, ok
                    }
                },
            }
        }

        self.return_or_register_errors(|| Ok(()), error_state)
    }

    /// Validate the given arguments against the given parameters and reorder
    /// the arguments to match the parameters.
    ///
    /// This does not modify the arguments or parameters, but instead returns a
    /// new argument list.
    ///
    /// *Invariant*: The parameters have already been validated through
    /// `validate_params`.
    pub fn validate_and_reorder_args_against_params(
        &self,
        args_id: ArgsId,
        params_id: ParamsId,
    ) -> TcResult<ArgsId> {
        // First validate the arguments
        self.validate_args_against_params(args_id.into(), params_id)?;

        let mut error_state = self.new_error_state();
        let mut result: Vec<Option<ArgData>> = vec![None; params_id.len()];

        // Note: We have already validated that the number of arguments is less than
        // or equal to the number of parameters

        for (j, arg_id) in args_id.iter().enumerate() {
            let arg = self.stores().args().get_element(arg_id);

            match arg.target {
                // Invariant: all positional arguments are before named
                ParamIndex::Position(j_received) => {
                    assert!(j_received == j);

                    result[j] = Some(ArgData {
                        // Add the name if present
                        target: self.get_param_index((params_id, j)),
                        value: arg.value,
                    });
                }
                ParamIndex::Name(arg_name) => {
                    // Find the position in the parameter list of the parameter with the
                    // same name as the argument
                    let maybe_param_index = params_id.iter().position(|param_id| {
                        match self.get_param_name_ident(param_id) {
                            Some(name) => name == arg_name,
                            None => false,
                        }
                    });

                    match maybe_param_index {
                        Some(i) => {
                            if result[i].is_some() {
                                // Duplicate argument name, must be from positional
                                assert!(j != i);
                                error_state.add_error(ParamError::DuplicateArg {
                                    first: (args_id, i).into(),
                                    second: (args_id, j).into(),
                                });
                            } else {
                                // Found an uncrossed parameter, add it to the result
                                result[i] = Some(ArgData { target: arg.target, value: arg.value });
                            }
                        }
                        None => {
                            // No parameter with the same name as the argument
                            error_state.add_error(ParamError::ArgNameNotFoundInParams {
                                arg: arg_id.into(),
                                params: params_id,
                            });
                        }
                    }
                }
            }
        }

        // If there were any errors, return them
        if error_state.has_errors() {
            return self.return_or_register_errors(|| unreachable!(), error_state);
        }

        // Populate default values and catch missing arguments
        for i in params_id.to_index_range() {
            if result[i].is_none() {
                let param_id = (params_id, i);
                let default = self.get_param_default(param_id);

                if let Some(default) = default {
                    // If there is a default value, add it to the result
                    result[i] =
                        Some(ArgData { target: self.get_param_index(param_id), value: default });
                } else {
                    // No default value, and not present in the arguments, so
                    // this is an error
                    error_state.add_error(ParamError::RequiredParamNotFoundInArgs {
                        param: param_id,
                        args: args_id.into(),
                    });
                }
            }
        }

        // If there were any errors, return them
        if error_state.has_errors() {
            return self.return_or_register_errors(|| unreachable!(), error_state);
        }

        // Now, create the new argument list
        // There should be no `None` elements at this point
        let new_args_id =
            self.param_utils().create_args(result.into_iter().map(|arg| arg.unwrap()));

        Ok(new_args_id)
    }

    /// Validate the given pattern arguments against the given parameters and
    /// reorder the arguments to match the parameters. Additionally, add
    /// `Captured` members to the pattern arguments where appropriate if
    /// there is a spread.
    ///
    /// This does not modify the arguments or parameters, but instead returns a
    /// new argument list.
    ///
    /// *Invariant*: The parameters have already been validated through
    /// `validate_params`.
    ///
    /// *Invariant*: The input arguments are *not* already validated/reordered.
    /// Specifically, they do not contain any `Capture` members.
    pub fn validate_and_reorder_pat_args_against_params(
        &self,
        args_id: PatArgsId,
        spread: Option<Spread>,
        params_id: ParamsId,
    ) -> TcResult<PatArgsId> {
        // First validate the arguments
        self.validate_args_against_params(args_id.into(), params_id)?;

        let mut error_state = self.new_error_state();
        let mut result: Vec<Option<PatArgData>> = vec![None; params_id.len()];

        // Note: We have already validated that the number of arguments is less than
        // or equal to the number of parameters

        for (j, arg_id) in args_id.iter().enumerate() {
            let arg = self.stores().pat_args().get_element(arg_id);

            match arg.target {
                // Invariant: all positional arguments are before named
                ParamIndex::Position(j_received) => {
                    assert!(j_received == j);

                    // If the previous argument was a spread, this is an error
                    if let Some(spread) = spread && j != 0 && spread.index == j - 1 {
                        error_state.add_error(ParamError::SpreadBeforePositionalArg {
                            next_positional: arg_id.into(),
                        });
                    }

                    result[j] = Some(PatArgData {
                        // Add the name if present
                        target: self.get_param_index((params_id, j)),
                        pat: arg.pat,
                    });
                }
                ParamIndex::Name(arg_name) => {
                    // Find the position in the parameter list of the parameter with the
                    // same name as the argument
                    let maybe_param_index = params_id.iter().position(|param_id| {
                        match self.get_param_name_ident(param_id) {
                            Some(name) => name == arg_name,
                            None => false,
                        }
                    });

                    match maybe_param_index {
                        Some(i) => {
                            if result[i].is_some() {
                                // Duplicate argument name, must be from positional
                                assert!(j != i);
                                error_state.add_error(ParamError::DuplicateArg {
                                    first: (args_id, i).into(),
                                    second: (args_id, j).into(),
                                });
                            } else {
                                // Found an uncrossed parameter, add it to the result
                                result[i] = Some(PatArgData { target: arg.target, pat: arg.pat });
                            }
                        }
                        None => {
                            // No parameter with the same name as the argument
                            error_state.add_error(ParamError::ArgNameNotFoundInParams {
                                arg: arg_id.into(),
                                params: params_id,
                            });
                        }
                    }
                }
            }
        }

        // If there were any errors, return them
        if error_state.has_errors() {
            return self.return_or_register_errors(|| unreachable!(), error_state);
        }

        // Populate missing arguments with captures
        for i in params_id.to_index_range() {
            if result[i].is_none() {
                let param_id = (params_id, i);
                if spread.is_some() {
                    result[i] = Some(PatArgData {
                        target: self.get_param_index(param_id),
                        pat: PatOrCapture::Capture,
                    });
                } else {
                    // No spread, and not present in the arguments, so
                    // this is an error
                    error_state.add_error(ParamError::RequiredParamNotFoundInArgs {
                        param: param_id,
                        args: args_id.into(),
                    });
                }
            }
        }

        // If there were any errors, return them
        if error_state.has_errors() {
            return self.return_or_register_errors(|| unreachable!(), error_state);
        }

        // Now, create the new argument list
        // There should be no `None` elements at this point
        let new_args_id =
            self.param_utils().create_pat_args(result.into_iter().map(|arg| arg.unwrap()));

        Ok(new_args_id)
    }
}