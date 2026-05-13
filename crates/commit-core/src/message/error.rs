use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    EmptyMessage,
    MissingType,
    InvalidType,
    InvalidScope,
    MissingColon,
    MissingDescription,
    InvalidBody,
    InvalidFooter,
    InvalidFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintError {
    pub kind: LintErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintErrorKind {
    SubjectTooLong { max: usize, actual: usize },
    SubjectEndsWithPunctuation,
    BannedPhrase { phrase: String },
    DisallowedType { ty: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageError {
    Parse(ParseError),
    Lint(LintError),
}

impl ParseError {
    pub(crate) fn new(kind: ParseErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl LintError {
    pub(crate) fn new(kind: LintErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl fmt::Display for LintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl fmt::Display for MessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Lint(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ParseError {}
impl std::error::Error for LintError {}
impl std::error::Error for MessageError {}
