use alloc::string::String;
use core::fmt;

/// The category of a YAML error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    UnexpectedEof,
    UnexpectedCharacter,
    InvalidIndentation,
    TabCharacter,
    InvalidScalar,
    InvalidAnchor,
    UnknownAnchor,
    AliasCycle,
    InvalidDirective,
    InvalidTag,
    InvalidMergeKey,
    TrailingContent,
    Other,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::UnexpectedEof => "unexpected end of file",
            Self::UnexpectedCharacter => "unexpected character",
            Self::InvalidIndentation => "invalid indentation",
            Self::TabCharacter => "tabs are not allowed for indentation",
            Self::InvalidScalar => "invalid scalar",
            Self::InvalidAnchor => "invalid anchor",
            Self::UnknownAnchor => "unknown anchor",
            Self::AliasCycle => "alias cycle",
            Self::InvalidDirective => "invalid directive",
            Self::InvalidTag => "invalid tag",
            Self::InvalidMergeKey => "invalid merge key",
            Self::TrailingContent => "trailing content",
            Self::Other => "invalid YAML",
        };
        f.write_str(value)
    }
}

/// A small captured window around an error location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorContext {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
    pub text: String,
}

impl ErrorContext {
    pub fn new(source: &str, offset: usize) -> Self {
        let start = source[..offset.min(source.len())].rfind('\n').map_or(0, |p| p + 1);
        let end = source[offset.min(source.len())..]
            .find('\n')
            .map_or(source.len(), |p| offset.min(source.len()) + p);
        let line = source[..offset.min(source.len())].bytes().filter(|b| *b == b'\n').count() + 1;
        let column = offset.saturating_sub(start) + 1;
        Self { offset, line, column, text: source[start..end.min(source.len())].to_owned() }
    }
}

/// The primary error type returned by the parser and lexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlError {
    pub offset: usize,
    pub kind: ErrorKind,
    pub message: String,
    pub context: ErrorContext,
}

impl YamlError {
    pub fn new(offset: usize, kind: ErrorKind, message: impl Into<String>, source: &str) -> Self {
        Self { offset, kind, message: message.into(), context: ErrorContext::new(source, offset) }
    }

    pub fn at(offset: usize, kind: ErrorKind, message: impl Into<String>, source: &str) -> Self {
        Self::new(offset, kind, message, source)
    }
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}:{}: {}",
            self.kind, self.context.line, self.context.column, self.message
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for YamlError {}

/// Alias retained for callers that prefer the `ParseError` terminology.
pub type ParseError = YamlError;
