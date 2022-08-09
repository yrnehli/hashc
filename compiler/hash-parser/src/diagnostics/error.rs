//! Hash Compiler parser error utilities.
use derive_more::Constructor;
use hash_pipeline::fs::ImportError;
use hash_reporting::{
    builder::ReportBuilder,
    report::{Report, ReportCodeBlock, ReportElement, ReportKind, ReportNote, ReportNoteKind},
};
use hash_source::location::SourceLocation;
use hash_token::{TokenKind, TokenKindVector};
use hash_utils::printing::SequenceDisplay;

use super::TyArgumentKind;

/// Utility wrapper type for [ParseError] in [Result]
pub type ParseResult<T> = Result<T, ParseError>;

/// A [ParseError] represents possible errors that occur when transforming the
/// token stream into the AST.
#[derive(Debug, Clone, Constructor)]
pub struct ParseError {
    /// The kind of the error.
    kind: ParseErrorKind,
    /// Location of where the error references
    location: SourceLocation,
    /// An optional vector of tokens that are expected to circumvent the error.
    expected: Option<TokenKindVector>,
    /// An optional token in question that was received byt shouldn't of been
    received: Option<TokenKind>,
}

/// Enum representation of the AST generation error variants.
#[derive(Debug, Clone)]
pub enum ParseErrorKind {
    /// Expected keyword at current location
    Keyword,
    /// Generic error specifying an expected token atom.
    Expected,
    /// Expected the beginning of a body block.
    Block,
    /// Expecting a re-assignment operator at the specified location.
    /// Re-assignment operators are like normal operators, but they expect
    /// an 'equals' sign after the specified operator.
    ReAssignmentOp,
    /// Error representing expected type arguments. This error has two variants,
    /// it can either be 'struct' or 'enum' type arguments. The reason why
    /// there are two variants is to add additional information in the error
    /// message.
    TypeDefinition(TyArgumentKind),
    /// Expected a name here.
    ExpectedName,
    /// Expected a binary operator that ties two expressions together to create
    /// a binary expression.
    ExpectedOperator,
    /// Expected an expression.
    ExpectedExpr,
    /// Expected a '=>' at the current location. This error can occur in a
    /// number of places; including but not limited to: after type
    /// arguments, lambda definition, trait bound annotation, etc.
    ExpectedArrow,
    /// Specific error when expecting an arrow after the function definition
    ExpectedFnArrow,
    /// Expected a function body at the current location.
    ExpectedFnBody,
    /// Expected a type at the current location.
    ExpectedType,
    /// Expected an expression after a type annotation within named tuples
    ExpectedValueAfterTyAnnotation,
    /// After a dot operator, the parser expects either a property access or an
    /// infix-like method call which is an extended version of a property
    /// access.
    ExpectedPropertyAccess,
    /// Expected a pattern at this location
    ExpectedPat,
    /// When the `import()` directive is used, the only argument should be a
    /// string path. @@Future: @@CompTime: This could likely change in the
    /// future.
    ImportPath,
    /// Expected an identifier after a name qualifier '::'.
    Namespace,
    /// If an imported module has errors, it should be reported
    ErroneousImport(ImportError),
    /// Malformed spread pattern (if for any reason there is a problem with
    /// parsing the spread operator)
    MalformedSpreadPattern(u8),
    /// Expected a literal token, mainly originating from range pattern parsing
    ExpectedLiteral,
}

/// Conversion implementation from an AST Generator Error into a Parser Error.
impl From<ParseError> for Report {
    fn from(err: ParseError) -> Self {
        let expected = err.expected;

        let mut base_message = match &err.kind {
            ParseErrorKind::Keyword => {
                format!(
                    "encountered an unexpected keyword {}",
                    err.received.unwrap().as_error_string()
                )
            }
            ParseErrorKind::Expected => match &err.received {
                Some(kind) => format!("unexpectedly encountered {}", kind.as_error_string()),
                None => "unexpectedly reached the end of input".to_string(),
            },
            ParseErrorKind::Block => "expected block body, which begins with a `{`".to_string(),
            ParseErrorKind::ReAssignmentOp => "expected a re-assignment operator".to_string(),
            ParseErrorKind::TypeDefinition(ty) => {
                format!("expected {ty} definition entries here which begin with a `(`")
            }
            ParseErrorKind::ExpectedValueAfterTyAnnotation => {
                "expected value assignment after type annotation within named tuple".to_string()
            }
            ParseErrorKind::ExpectedOperator => "expected an operator".to_string(),
            ParseErrorKind::ExpectedExpr => "expected an expression".to_string(),
            ParseErrorKind::ExpectedName => "expected a name here".to_string(),
            ParseErrorKind::ExpectedArrow => "expected an arrow `=>` ".to_string(),
            ParseErrorKind::ExpectedFnArrow => {
                "expected an arrow `->` after type arguments denoting a function type".to_string()
            }
            ParseErrorKind::ExpectedFnBody => "expected a function body".to_string(),
            ParseErrorKind::ExpectedType => "expected a type annotation".to_string(),
            ParseErrorKind::ExpectedPropertyAccess => {
                "expected field name access or a method call".to_string()
            }
            ParseErrorKind::ExpectedPat => "expected pattern".to_string(),
            ParseErrorKind::ImportPath => {
                "expected an import path which should be a string".to_string()
            }
            ParseErrorKind::ErroneousImport(err) => err.to_string(),
            ParseErrorKind::Namespace => {
                "expected identifier after a name access qualifier `::`".to_string()
            }
            ParseErrorKind::MalformedSpreadPattern(dots) => {
                format!(
                    "malformed spread pattern, expected {dots} more `.` to complete the pattern"
                )
            }
            ParseErrorKind::ExpectedLiteral => "expected literal".to_string(),
        };

        // `AstGenErrorKind::Expected` format the error message in their own way,
        // whereas all the other error types follow a conformed order to
        // formatting expected tokens
        if !matches!(&err.kind, ParseErrorKind::Expected) {
            if let Some(kind) = err.received {
                let atom_msg = format!(", however received {}", kind.as_error_string());
                base_message.push_str(&atom_msg);
            }
        }

        // If the generated error has suggested tokens that aren't empty.
        let help = expected.map(|tokens| {
            format!("consider adding {}", SequenceDisplay::either(tokens.into_inner().as_slice()))
        });

        // Now actually build the report
        let mut builder = ReportBuilder::new();
        builder
            .with_kind(ReportKind::Error)
            .with_message(base_message)
            .add_element(ReportElement::CodeBlock(ReportCodeBlock::new(err.location, "here")));

        // Add the `help` message as a separate note
        if let Some(help_message) = help {
            builder.add_element(ReportElement::Note(ReportNote::new(
                ReportNoteKind::Help,
                help_message,
            )));
        }

        builder.build()
    }
}
