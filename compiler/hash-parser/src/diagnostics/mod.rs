//! Hash Parser diagnostic utilities, error and warning definitions.
//! This module contains all of the logic that provides diagnostic
//! capabilities within the parser.
pub(crate) mod error;
pub(crate) mod warning;

use hash_reporting::{diagnostic::Diagnostics, report::Report};
use smallvec::SmallVec;

use self::{
    error::ParseError,
    warning::{ParseWarning, ParseWarningWrapper},
};
use crate::parser::AstGen;

/// Enum representing the kind of statement where type arguments can be expected
/// to be present.
#[derive(Debug, Clone, Copy)]
pub enum TyArgumentKind {
    /// Type arguments at a struct definition.
    Struct,
    /// Type arguments at a enum definition.
    Enum,
}

impl std::fmt::Display for TyArgumentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TyArgumentKind::Struct => write!(f, "struct"),
            TyArgumentKind::Enum => write!(f, "enumeration"),
        }
    }
}

/// Represents the stored diagnostics within the parser.
#[derive(Default)]
pub struct ParserDiagnostics {
    /// Errors generated by the parser, this is set as a [SmallVec] since
    /// [AstGen] variants are unlikely to ever collect more than a few
    /// errors until the bubble up into the final [AstGen]
    pub(crate) errors: SmallVec<[ParseError; 2]>,
    /// Warnings generated by the parser
    warnings: Vec<ParseWarning>,
}

impl<'stream, 'resolver> Diagnostics<ParseError, ParseWarning> for AstGen<'stream, 'resolver> {
    type DiagnosticsStore = ParserDiagnostics;

    fn diagnostic_store(&self) -> &Self::DiagnosticsStore {
        &self.diagnostics
    }

    fn add_error(&mut self, error: ParseError) {
        self.diagnostics.errors.push(error);
    }

    fn add_warning(&mut self, warning: ParseWarning) {
        self.diagnostics.warnings.push(warning);
    }

    fn has_errors(&self) -> bool {
        !self.diagnostic_store().errors.is_empty()
    }

    fn has_warnings(&self) -> bool {
        !self.diagnostic_store().warnings.is_empty()
    }

    fn into_reports(self) -> Vec<Report> {
        self.diagnostics
            .errors
            .into_iter()
            .map(|err| err.into())
            .chain(
                self.diagnostics.warnings.into_iter().map(|warn| {
                    ParseWarningWrapper(warn, self.resolver.current_source_id()).into()
                }),
            )
            .collect()
    }

    fn into_diagnostics(self) -> (Vec<ParseError>, Vec<ParseWarning>) {
        (self.diagnostics.errors.to_vec(), self.diagnostics.warnings)
    }

    fn merge_diagnostics(&mut self, other: impl Diagnostics<ParseError, ParseWarning>) {
        let (errors, warnings) = other.into_diagnostics();

        self.diagnostics.errors.extend(errors);
        self.diagnostics.warnings.extend(warnings);
    }
}
