//! Error-related data structures for errors that occur during typechecking.
use hash_error_codes::error_codes::HashErrorCode;
use hash_reporting::{
    self,
    reporter::{Reporter, Reports},
};
use hash_source::location::SourceLocation;
use hash_tir::new::{
    environment::env::AccessToEnv, symbols::Symbol, terms::TermId, utils::common::CommonUtils,
};
use hash_typecheck::errors::{TcError, TcErrorReporter};

use crate::new::{
    environment::tc_env::{AccessToTcEnv, WithTcEnv},
    passes::resolution::scoping::ContextKind,
};

/// An error that occurs during semantic analysis.
#[derive(Clone, Debug)]
pub enum SemanticError {
    /// A series of errors.
    Compound { errors: Vec<SemanticError> },
    /// An error exists, this is just a signal to stop typechecking.
    Signal,
    /// More type annotations are needed to infer the type of the given term.
    NeedMoreTypeAnnotationsToInfer { term: TermId },
    /// Traits are not yet supported.
    TraitsNotSupported { trait_location: SourceLocation },
    /// Merge declarations are not yet supported.
    MergeDeclarationsNotSupported { merge_location: SourceLocation },
    /// Some specified symbol was not found.
    SymbolNotFound { symbol: Symbol, location: SourceLocation, looking_in: ContextKind },
    /// Cannot use a module in a value position.
    CannotUseModuleInValuePosition { location: SourceLocation },
    /// Cannot use a module in a type position.
    CannotUseModuleInTypePosition { location: SourceLocation },
    /// Cannot use a module in a pattern position.
    CannotUseModuleInPatternPosition { location: SourceLocation },
    /// Cannot use a data type in a value position.
    CannotUseDataTypeInValuePosition { location: SourceLocation },
    /// Cannot use a data type in a pattern position.
    CannotUseDataTypeInPatternPosition { location: SourceLocation },
    /// Cannot use a constructor in a type position.
    CannotUseConstructorInTypePosition { location: SourceLocation },
    /// Cannot use a function in type position.
    CannotUseFunctionInTypePosition { location: SourceLocation },
    /// Cannot use a function in a pattern position.
    CannotUseFunctionInPatternPosition { location: SourceLocation },
    /// Cannot use the subject as a namespace.
    InvalidNamespaceSubject { location: SourceLocation },
    /// Cannot use arguments here.
    UnexpectedArguments { location: SourceLocation },
    /// Type error, forwarded from the typechecker.
    TypeError { error: TcError },
}

impl From<TcError> for SemanticError {
    fn from(value: TcError) -> Self {
        Self::TypeError { error: value }
    }
}

pub type SemanticResult<T> = Result<T, SemanticError>;

impl<'tc> From<WithTcEnv<'tc, &SemanticError>> for Reports {
    fn from(ctx: WithTcEnv<'tc, &SemanticError>) -> Self {
        let mut builder = Reporter::new();
        ctx.add_to_reporter(&mut builder);
        builder.into_reports()
    }
}

impl<'tc> WithTcEnv<'tc, &SemanticError> {
    /// Format the error nicely and add it to the given reporter.
    fn add_to_reporter(&self, reporter: &mut Reporter) {
        let locations = self.tc_env().stores().location();
        match &self.value {
            SemanticError::Signal => {}
            SemanticError::NeedMoreTypeAnnotationsToInfer { term } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::UnresolvedType)
                    .title("cannot infer the type of this term".to_string());

                if let Some(location) = self.tc_env().get_location(term) {
                    error
                        .add_span(location)
                        .add_help("consider adding more type annotations to this expression");
                }
            }
            SemanticError::Compound { errors } => {
                for error in errors {
                    self.tc_env().with(error).add_to_reporter(reporter);
                }
            }
            SemanticError::TraitsNotSupported { trait_location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::UnsupportedTraits)
                    .title("traits are work-in-progress and currently not supported".to_string());

                error.add_span(*trait_location).add_help("cannot use traits yet");
            }
            SemanticError::MergeDeclarationsNotSupported { merge_location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::UnsupportedTraits)
                    .title("merge declarations are currently not supported".to_string());

                error.add_span(*merge_location).add_help("cannot use merge declarations yet");
            }
            SemanticError::SymbolNotFound { symbol, location, looking_in } => {
                let def_name = format!("{}", self.tc_env().with(looking_in));
                let search_name = self.tc_env().env().with(*symbol);
                let noun = match looking_in {
                    ContextKind::Access(_, _) => "member",
                    ContextKind::Environment => "name",
                };
                let error = reporter
                    .error()
                    .code(HashErrorCode::UnresolvedSymbol)
                    .title(format!("cannot find {noun} `{search_name}` in {def_name}"));
                error.add_labelled_span(
                    *location,
                    format!("tried to look for {noun} `{search_name}` in {def_name}",),
                );

                if let ContextKind::Access(_, def) = looking_in {
                    if let Some(location) = locations.get_location(def) {
                        error.add_span(location).add_info(format!(
                            "{def_name} is defined here, and has no member `{search_name}`",
                        ));
                    }
                }
            }
            SemanticError::CannotUseModuleInValuePosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::NonRuntimeInstantiable)
                    .title("cannot use a module in expression position");

                error
                    .add_span(*location)
                    .add_info("cannot use this in expression position as it is a module");
            }
            SemanticError::CannotUseModuleInTypePosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::ValueCannotBeUsedAsType)
                    .title("cannot use a module in type position");

                error
                    .add_span(*location)
                    .add_info("cannot use this in type position as it is a module");
            }
            SemanticError::CannotUseModuleInPatternPosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::ValueCannotBeUsedAsType)
                    .title("cannot use a module in pattern position");

                error
                    .add_span(*location)
                    .add_info("cannot use this in pattern position as it is a module");
            }
            SemanticError::CannotUseDataTypeInValuePosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::NonRuntimeInstantiable)
                    .title("cannot use a data type in expression position")
                    .add_help("consider using a constructor call instead");

                error
                    .add_span(*location)
                    .add_info("cannot use this in expression position as it is a data type");
            }
            SemanticError::CannotUseDataTypeInPatternPosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::NonRuntimeInstantiable)
                    .title("cannot use a data type in pattern position")
                    .add_help("consider using a constructor pattern instead");

                error
                    .add_span(*location)
                    .add_info("cannot use this in pattern position as it is a data type");
            }
            SemanticError::CannotUseConstructorInTypePosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::ValueCannotBeUsedAsType)
                    .title("cannot use a constructor in type position");

                error
                    .add_span(*location)
                    .add_info("cannot use this in type position as it is a constructor");
            }
            SemanticError::CannotUseFunctionInTypePosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::ValueCannotBeUsedAsType)
                    .title("cannot use a function in type position");

                error.add_span(*location).add_info(
                    "cannot use this in type position as it refers to a function definition",
                );
            }
            SemanticError::CannotUseFunctionInPatternPosition { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::ValueCannotBeUsedAsType)
                    .title("cannot use a function in pattern position");

                error.add_span(*location).add_info(
                    "cannot use this in pattern position as it refers to a function definition",
                );
            }
            SemanticError::InvalidNamespaceSubject { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::UnsupportedAccess)
                    .title("only data types and modules can be used as namespacing subjects");

                error
                    .add_span(*location)
                    .add_info("cannot use this as a subject of a namespace access");
            }
            SemanticError::TypeError { error } => {
                TcErrorReporter::new(self.env()).add_to_reporter(error, reporter)
            }
            SemanticError::UnexpectedArguments { location } => {
                let error = reporter
                    .error()
                    .code(HashErrorCode::ValueCannotBeUsedAsType)
                    .title("unexpected arguments given to subject");

                error
                    .add_span(*location)
                    .add_info("cannot use these arguments as the subject does not expect them");
            }
        }
    }
}
