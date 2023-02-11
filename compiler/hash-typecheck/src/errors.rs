use std::{fmt, mem::take};

use derive_more::{Constructor, From};
use hash_error_codes::error_codes::HashErrorCode;
use hash_reporting::{
    reporter::{Reporter, Reports},
    writer::ReportWriter,
};
use hash_source::location::SourceLocation;
use hash_tir::{
    impl_access_to_env,
    new::{
        environment::env::{AccessToEnv, Env},
        locations::LocationTarget,
        params::{ParamIndex, ParamsId, SomeParamsOrArgsId},
        terms::TermId,
        tys::TyId,
        utils::common::CommonUtils,
    },
};
use hash_utils::store::SequenceStoreKey;

use crate::params::ParamError;

/// Accumulates errors that occur during typechecking in a local scope.
///
/// This is used for error recovery, so that multiple errors can be reported
/// at once.
#[derive(Debug)]
pub struct TcErrorState {
    pub errors: Vec<TcError>,
    pub has_blocked: bool,
}

impl TcErrorState {
    pub fn new() -> Self {
        Self { errors: vec![], has_blocked: false }
    }

    /// Add an error to the error state.
    pub fn add_error(&mut self, error: impl Into<TcError>) -> &TcError {
        let error = error.into();
        if let TcError::Blocked = error {
            self.has_blocked = true;
        }
        self.errors.push(error);
        self.errors.last().unwrap()
    }

    /// Add an error to the error state if the given result is an error.
    pub fn try_or_add_error<F>(&mut self, f: TcResult<F>) -> Option<F> {
        match f {
            Ok(v) => Some(v),
            Err(e) => {
                self.add_error(e);
                None
            }
        }
    }

    /// Add a set of errors to the error state.
    pub fn add_errors(&mut self, errors: impl IntoIterator<Item = impl Into<TcError>>) {
        self.errors.extend(errors.into_iter().map(|err| err.into()));
    }

    /// Whether the error state has any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Take the errors from the error state.
    pub fn take_errors(&mut self) -> Vec<TcError> {
        take(&mut self.errors)
    }
}

impl Default for TcErrorState {
    fn default() -> Self {
        Self::new()
    }
}

/// A kind of wrong term usage.
#[derive(Clone, Debug)]
pub enum WrongTermKind {
    /// Cannot call a term because it is not a function.
    NotAFunction,
    /// Cannot access members because the term is not a record.
    ///
    /// Records are tuples, single constructor data types.
    NotARecord,
    /// Cannot use the given term because it is not of the correct type.
    NotOfType { correct_ty: TyId },
}

/// An error that occurs during typechecking.
#[derive(Clone, Debug, From)]

pub enum TcError {
    /// Blocked, cannot continue. This is used as a signal in the typechecker.
    Blocked,

    /// Signal to assert that there are other errors in the diagnostics store.
    Signal,

    /// A series of errors.
    Compound { errors: Vec<TcError> },

    /// More type annotations are needed to infer the type of the given term.
    NeedMoreTypeAnnotationsToInfer { term: LocationTarget },

    /// The given arguments do not match the length of the target parameters.
    WrongArgLength { params_id: ParamsId, args_id: SomeParamsOrArgsId },

    /// The given parameters do not match the length of their annotations.
    WrongParamLength { given_params_id: ParamsId, annotation_params_id: ParamsId },

    /// Cannot deref the subject.
    CannotDeref { subject: TermId, actual_subject_ty: TyId },

    /// Types don't match
    MismatchingTypes { expected: TyId, actual: TyId, inferred_from: Option<LocationTarget> },

    /// Wrong type used somewhere
    WrongTy { term: TermId, inferred_term_ty: TyId, kind: WrongTermKind },

    /// The given property does not exist on the given term.
    PropertyNotFound { term: TermId, term_ty: TyId, property: ParamIndex },

    /// Undecidable equality between terms
    UndecidableEquality { a: TermId, b: TermId },

    /// Invalid range pattern literal
    InvalidRangePatternLiteral { location: SourceLocation },

    /// An error related to argument/parameter matching.
    #[from]
    ParamMatch(ParamError),

    /// An error that occurred in an intrinsic.
    Intrinsic(String),
}

pub type TcResult<T> = Result<T, TcError>;

#[derive(Constructor)]
pub struct TcErrorReporter<'env> {
    env: &'env Env<'env>,
}

impl_access_to_env!(TcErrorReporter<'env>);

impl fmt::Display for TcErrorReporter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reports = self.format_error(&TcError::Signal);
        write!(f, "{}", ReportWriter::new(reports, self.source_map()))
    }
}

impl<'tc> TcErrorReporter<'tc> {
    /// Format the error nicely and return it as a set of reports.
    pub fn format_error(&self, error: &TcError) -> Reports {
        let mut builder = Reporter::new();
        self.add_to_reporter(error, &mut builder);
        builder.into_reports()
    }

    /// Format the error nicely and add it to the given reporter.
    pub fn add_to_reporter(&self, error: &TcError, reporter: &mut Reporter) {
        let locations = self.stores().location();
        match error {
            TcError::Signal => {}
            TcError::Blocked => {
                let _error = reporter
                    .error()
                    .code(HashErrorCode::UnresolvedType)
                    .title("blocked while typechecking".to_string());
            }
            TcError::NeedMoreTypeAnnotationsToInfer { term } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::UnresolvedType)
                    .title("cannot infer the type of this term".to_string());

                if let Some(location) = self.get_location(term) {
                    error
                        .add_span(location)
                        .add_help("consider adding more type annotations to this expression");
                }
            }
            TcError::Compound { errors } => {
                for error in errors {
                    self.add_to_reporter(error, reporter);
                }
            }
            TcError::WrongArgLength { params_id, args_id } => {
                let param_length = params_id.len();
                let arg_length = args_id.len();

                let error =
                    reporter.error().code(HashErrorCode::ParameterLengthMismatch).title(format!(
                    "mismatch in parameter length: expected {param_length} but got {arg_length}"
                ));

                if let Some(location) = locations.get_overall_location(*params_id) {
                    error
                        .add_span(location)
                        .add_info(format!("expected {param_length} parameters here"));
                }

                if let Some(location) = locations.get_overall_location(*args_id) {
                    error
                        .add_span(location)
                        .add_info(format!("got {arg_length} {} here", args_id.as_str()));
                }
            }
            TcError::CannotDeref { subject, actual_subject_ty } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::InvalidCallSubject)
                    .title("the subject of this dereference is not a reference");
                if let Some(location) = locations.get_location(subject) {
                    error.add_labelled_span(
                        location,
                        format!(
                            "cannot use this as a subject of a dereference operation. It is of type `{}` which is not a reference type.",
                            self.env().with(*actual_subject_ty)
                        )
                    );
                }
            }
            TcError::MismatchingTypes { expected, actual, inferred_from } => {
                let error = reporter.error().code(HashErrorCode::TypeMismatch).title(format!(
                    "expected type `{}` but got `{}`",
                    self.env().with(*expected),
                    self.env().with(*actual),
                ));
                if let Some(location) = inferred_from.and_then(|term| locations.get_location(term))
                {
                    error.add_labelled_span(
                        location,
                        format!("type `{}` inferred from here", self.env().with(*actual)),
                    );
                }
                if let Some(location) = locations.get_location(expected) {
                    error.add_labelled_span(
                        location,
                        format!("this expects type `{}`", self.env().with(*expected)),
                    );
                }
                if let Some(location) = locations.get_location(actual) {
                    error.add_labelled_span(
                        location,
                        format!("this is of type `{}`", self.env().with(*actual)),
                    );
                }
            }
            TcError::UndecidableEquality { a, b } => {
                let error = reporter.error().code(HashErrorCode::TypeMismatch).title(format!(
                    "cannot determine if expressions `{}` and `{}` are equal",
                    self.env().with(*a),
                    self.env().with(*b),
                ));
                if let Some(location) = locations.get_location(a) {
                    error.add_labelled_span(
                        location,
                        format!(
                            "`{}` from here", //@@Todo: flag for if inferred or declared
                            self.env().with(*a)
                        ),
                    );
                }
                if let Some(location) = locations.get_location(b) {
                    error.add_labelled_span(
                        location,
                        format!("`{}` from here", self.env().with(*b)),
                    );
                }
            }
            TcError::InvalidRangePatternLiteral { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::TypeMismatch)
                    .title("range patterns should contain valid literals");
                if let Some(location) = locations.get_location(location) {
                    error.add_labelled_span(location, "not a valid range literal");
                }
            }
            TcError::ParamMatch(err) => match err {
                ParamError::TooManyArgs { expected, got } => {
                    let error = reporter
                        .error()
                        .code(HashErrorCode::ParameterLengthMismatch)
                        .title(format!(
                            "received {} arguments, but expected at most {} arguments",
                            got.len(),
                            expected.len()
                        ));
                    if let Some(location) = locations.get_overall_location(*expected) {
                        error.add_labelled_span(
                            location,
                            format!(
                                "expected at most {} arguments by this definition",
                                expected.len()
                            ),
                        );
                    }
                    if let Some(location) = locations.get_overall_location(*got) {
                        error.add_labelled_span(
                            location,
                            format!("received {} arguments here", got.len()),
                        );
                    }
                }
                ParamError::DuplicateArg { first, second } => {
                    let error = reporter
                        .error()
                        .code(HashErrorCode::ParameterInUse)
                        .title("received a duplicate argument");
                    if let Some(location) = locations.get_location(first) {
                        error.add_labelled_span(location, "first occurrence of this argument");
                    }
                    if let Some(location) = locations.get_location(second) {
                        error.add_labelled_span(location, "second occurrence of this argument");
                    }
                }
                ParamError::DuplicateParam { first, second } => {
                    let error = reporter
                        .error()
                        .code(HashErrorCode::ParameterInUse)
                        .title("received a duplicate parameter");
                    if let Some(location) = locations.get_location(first) {
                        error.add_labelled_span(location, "first occurrence of this parameter");
                    }
                    if let Some(location) = locations.get_location(second) {
                        error.add_labelled_span(location, "second occurrence of this parameter");
                    }
                }
                ParamError::PositionalArgAfterNamedArg { first_named, next_positional } => {
                    let error = reporter
                        .error()
                        .code(HashErrorCode::ParameterInUse)
                        .title("received a positional argument after a named argument");
                    if let Some(location) = locations.get_location(first_named) {
                        error.add_labelled_span(location, "first named argument");
                    }
                    if let Some(location) = locations.get_location(next_positional) {
                        error.add_labelled_span(location, "next positional argument");
                    }
                    error.add_info("positional arguments must come before named arguments");
                }
                ParamError::RequiredParamAfterDefaultParam {
                    first_default,
                    next_required: next_non_default,
                } => {
                    let error = reporter
                        .error()
                        .code(HashErrorCode::ParameterInUse)
                        .title("found a required parameter after a default parameter");
                    if let Some(location) = locations.get_location(first_default) {
                        error.add_labelled_span(location, "first default parameter");
                    }
                    if let Some(location) = locations.get_location(next_non_default) {
                        error.add_labelled_span(location, "next required parameter");
                    }
                    error.add_info("parameters with defaults must come after required parameters");
                }
                ParamError::ArgNameNotFoundInParams { arg, params } => {
                    let error =
                        reporter.error().code(HashErrorCode::ParameterInUse).title(format!(
                        "received an argument named `{}` but no parameter with that name exists",
                        self.get_arg_index(*arg)
                    ));
                    if let Some(location) = locations.get_location(arg) {
                        error.add_labelled_span(location, "argument with this name");
                    }
                    if let Some(location) = locations.get_overall_location(*params) {
                        error.add_labelled_span(
                            location,
                            format!(
                                "expected one of these parameters: {}",
                                params
                                    .iter()
                                    .map(|param| format!("`{}`", self.get_param_index(param)))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                }
                ParamError::RequiredParamNotFoundInArgs { param, args } => {
                    let error =
                        reporter.error().code(HashErrorCode::ParameterInUse).title(format!(
                            "expected an argument named `{}` but none was found",
                            self.get_param_index(*param)
                        ));
                    if let Some(location) = locations.get_location(param) {
                        error.add_labelled_span(location, "parameter with this name");
                    }
                    if let Some(location) = locations.get_overall_location(*args) {
                        error.add_labelled_span(
                            location,
                            format!(
                                "received these arguments: {}",
                                args.iter()
                                    .map(|arg| format!("`{}`", self.get_arg_index(arg)))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                }
                ParamError::SpreadBeforePositionalArg { next_positional } => {
                    let error = reporter
                        .error()
                        .code(HashErrorCode::ParameterInUse)
                        .title("received a positional argument after a spread argument");
                    if let Some(location) = locations.get_location(next_positional) {
                        error.add_labelled_span(location, "next positional argument");
                    }
                    error.add_info("positional arguments must come before spread arguments");
                }
                ParamError::ParamNameMismatch { param_a, param_b } => {
                    let error = reporter
                        .error()
                        .code(HashErrorCode::ParameterInUse)
                        .title("received two parameters with different names");
                    if let Some(location) = locations.get_location(param_a) {
                        error.add_labelled_span(location, "first parameter with this name");
                    }
                    if let Some(location) = locations.get_location(param_b) {
                        error.add_labelled_span(location, "second parameter with this name");
                    }
                }
            },
            TcError::WrongTy { term, inferred_term_ty, kind } => {
                let kind_name = match kind {
                    WrongTermKind::NotAFunction => "function".to_string(),
                    WrongTermKind::NotARecord => "record".to_string(),
                    WrongTermKind::NotOfType { correct_ty } => {
                        format!("value of type `{}`", self.env().with(*correct_ty))
                    }
                };

                let error =
                    reporter.error().code(HashErrorCode::InvalidCallSubject).title(format!(
                        "expected a {}, but got type `{}` instead",
                        kind_name,
                        self.env().with(*inferred_term_ty)
                    ));

                if let Some(location) = locations.get_location(term) {
                    error.add_labelled_span(
                        location,
                        format!("expected a {kind_name}, but got this value instead"),
                    );
                }

                if let Some(location) = locations.get_location(inferred_term_ty) {
                    error.add_labelled_span(
                        location,
                        format!("this value has type `{}`", self.env().with(*inferred_term_ty)),
                    );
                }
            }
            TcError::PropertyNotFound { term, term_ty, property } => {
                let error =
                    reporter.error().code(HashErrorCode::InvalidPropertyAccess).title(format!(
                        "property `{}` not found on type `{}`",
                        *property,
                        self.env().with(*term_ty)
                    ));
                if let Some(location) = locations.get_location(term) {
                    error.add_labelled_span(
                        location,
                        format!(
                            "term has type `{}`. Property `{}` is not present on this type",
                            self.env().with(*term_ty),
                            *property,
                        ),
                    );
                }
            }
            TcError::WrongParamLength { given_params_id, annotation_params_id } => {
                let error =
                    reporter.error().code(HashErrorCode::ParameterLengthMismatch).title(format!(
                        "wrong number of parameters. Expected {} but got {}",
                        annotation_params_id.len(),
                        given_params_id.len()
                    ));
                if let Some(location) = locations.get_overall_location(*given_params_id) {
                    error.add_labelled_span(
                        location,
                        format!("got {} parameters here", given_params_id.len(),),
                    );
                }
                if let Some(location) = locations.get_overall_location(*annotation_params_id) {
                    error.add_labelled_span(
                        location,
                        format!("expected {} parameters from here", annotation_params_id.len(),),
                    );
                }
            }
            TcError::Intrinsic(msg) => {
                let _error = reporter.error().code(HashErrorCode::TypeMismatch).title(msg);
            }
        }
    }
}
