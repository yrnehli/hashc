//! Hash diagnostic report data structures.
use std::{cell::Cell, convert::Infallible, fmt, io};

use hash_error_codes::error_codes::HashErrorCode;
use hash_source::location::{RowColSpan, SourceLocation};

use crate::highlight::{highlight, Colour, Modifier};

/// A data type representing a comment/message on a specific span in a code
/// block.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ReportCodeBlockInfo {
    /// How many characters should be used for line numbers on the side.
    pub indent_width: usize,

    /// The span of the code block but using row and column indices.
    pub span: RowColSpan,
}

/// Enumeration describing the kind of [Report]; either being a warning, info or
/// an error.
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum ReportKind {
    /// The report is an error.
    Error,
    /// The report is an informational diagnostic (likely for internal
    /// purposes).
    Info,
    /// The report is a warning.
    Warning,
    // This is an internal compiler error.
    Internal,
}

impl ReportKind {
    /// Get the [Colour] of the label associated with the [ReportKind].
    pub(crate) fn as_colour(&self) -> Colour {
        match self {
            ReportKind::Error | ReportKind::Internal => Colour::Red,
            ReportKind::Info => Colour::Blue,
            ReportKind::Warning => Colour::Yellow,
        }
    }

    /// Get the string label associated with the [ReportKind].
    pub(crate) fn message(&self) -> &'static str {
        match self {
            ReportKind::Error => "error",
            ReportKind::Internal => "internal",
            ReportKind::Info => "info",
            ReportKind::Warning => "warn",
        }
    }
}

impl fmt::Display for ReportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", highlight(self.as_colour() | Modifier::Bold, self.message()))
    }
}

/// The kind of [ReportNote], this is primarily used for rendering the label of
/// the [ReportNote].
#[derive(Debug, Clone)]
pub enum ReportNoteKind {
    /// A help message or a suggestion.
    Help,

    /// Information note
    Info,

    /// Additional information about the diagnostic.
    Note,
}

impl ReportNoteKind {
    /// Get the string representation of the label.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReportNoteKind::Note => "note",
            ReportNoteKind::Info => "info",
            ReportNoteKind::Help => "help",
        }
    }
}

impl fmt::Display for ReportNoteKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportNoteKind::Note => write!(f, "note"),
            ReportNoteKind::Info => write!(f, "info"),
            ReportNoteKind::Help => write!(f, "{}", highlight(Colour::Cyan, "help")),
        }
    }
}

/// Data type representing a report note which consists of a label and the
/// message.
#[derive(Debug, Clone)]
pub struct ReportNote {
    pub label: ReportNoteKind,
    pub message: String,
}

impl ReportNote {
    pub fn new(label: ReportNoteKind, message: impl ToString) -> Self {
        Self { label, message: message.to_string() }
    }
}

/// Data structure representing an associated block of code with a report. The
/// type contains the span of the block, the message associated with a block and
/// optional [ReportCodeBlockInfo] which adds a message pointed to a code item.
#[derive(Debug, Clone)]
pub struct ReportCodeBlock {
    pub source_location: SourceLocation,
    pub code_message: String,
    pub(crate) info: Cell<Option<ReportCodeBlockInfo>>,
}

impl ReportCodeBlock {
    /// Create a new [ReportCodeBlock] from a [SourceLocation] and a message.
    pub fn new(source_location: SourceLocation, code_message: impl ToString) -> Self {
        Self { source_location, code_message: code_message.to_string(), info: Cell::new(None) }
    }
}

/// Enumeration representing types of components of a [Report]. A [Report] can
/// be made of either [ReportCodeBlock]s or [ReportNote]s.
#[derive(Debug, Clone)]
pub enum ReportElement {
    CodeBlock(ReportCodeBlock),
    Note(ReportNote),
}

/// The report data type represents the entire report which might contain many
/// [ReportElement]s. The report also contains a general [ReportKind] and a
/// general message.
#[derive(Debug, Clone)]
pub struct Report {
    /// The general kind of the report.
    pub kind: ReportKind,
    /// A title for the report.
    pub title: String,
    /// An optional associated general error code with the report.
    pub error_code: Option<HashErrorCode>,
    /// A vector of additional [ReportElement]s in order to add additional
    /// context to errors.
    pub contents: Vec<ReportElement>,
}

impl Report {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the report denotes an occurred error.
    pub fn is_error(&self) -> bool {
        self.kind == ReportKind::Error
    }

    /// Check if the report denotes an occurred warning.
    pub fn is_warning(&self) -> bool {
        self.kind == ReportKind::Warning
    }

    /// Add a title to the [Report].
    pub fn title(&mut self, title: impl ToString) -> &mut Self {
        self.title = title.to_string();
        self
    }

    /// Add a general kind to the [Report].
    pub fn kind(&mut self, kind: ReportKind) -> &mut Self {
        self.kind = kind;
        self
    }

    /// Add an associated [HashErrorCode] to the [Report].
    pub fn code(&mut self, error_code: HashErrorCode) -> &mut Self {
        self.error_code = Some(error_code);
        self
    }

    /// Add a [`ReportNoteKind::Help`] note with the given message to the
    /// [Report].
    pub fn add_help(&mut self, message: impl ToString) -> &mut Self {
        self.add_element(ReportElement::Note(ReportNote::new(
            ReportNoteKind::Help,
            message.to_string(),
        )))
    }

    /// Add a [`ReportNoteKind::Info`] note with the given message to the
    /// [Report].
    pub fn add_info(&mut self, message: impl ToString) -> &mut Self {
        self.add_element(ReportElement::Note(ReportNote::new(
            ReportNoteKind::Info,
            message.to_string(),
        )))
    }

    /// Add a [`ReportNoteKind::Note`] note with the given message to the
    /// [Report].
    pub fn add_note(&mut self, message: impl ToString) -> &mut Self {
        self.add_element(ReportElement::Note(ReportNote::new(
            ReportNoteKind::Note,
            message.to_string(),
        )))
    }

    /// Add a code block at the given location to the [Report].
    pub fn add_span(&mut self, location: SourceLocation) -> &mut Self {
        self.add_element(ReportElement::CodeBlock(ReportCodeBlock::new(location, "")))
    }

    /// Add a labelled code block at the given location to the [Report].
    pub fn add_labelled_span(
        &mut self,
        location: SourceLocation,
        message: impl ToString,
    ) -> &mut Self {
        self.add_element(ReportElement::CodeBlock(ReportCodeBlock::new(
            location,
            message.to_string(),
        )))
    }

    /// Add a [ReportElement] to the report.
    pub fn add_element(&mut self, element: ReportElement) -> &mut Self {
        self.contents.push(element);
        self
    }
}

impl Default for Report {
    fn default() -> Self {
        Self {
            kind: ReportKind::Error,
            title: "Bottom text".to_string(),
            error_code: None,
            contents: vec![],
        }
    }
}

/// Some basic conversions into reports
impl From<io::Error> for Report {
    fn from(err: io::Error) -> Self {
        let mut report = Report::new();

        // @@ErrorReporting: we might want to show a bit more info here.
        report.kind(ReportKind::Error).title(err.to_string());
        report
    }
}

impl From<Infallible> for Report {
    fn from(err: Infallible) -> Self {
        match err {}
    }
}
