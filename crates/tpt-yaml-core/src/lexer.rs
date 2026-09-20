use crate::{ErrorKind, Span, YamlError};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    DocumentStart,
    DocumentEnd,
    MappingKey,
    SequenceEntry,
    Colon,
    Comma,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Anchor(String),
    Alias(String),
    Tag(String),
    Directive(String),
    Scalar {
        raw: String,
        style: ScalarStyle,
        tag: Option<String>,
    },
    BlockScalar {
        indicator: BlockIndicator,
        chomping: Chomping,
        indentation: Option<usize>,
        value: String,
    },
    Comment(String),
    BlankLine,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockIndicator {
    Literal,
    Folded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chomping {
    Clip,
    Keep,
    Strip,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub indent: usize,
}

/// A pull-based tokenizer.
///
/// [`Lexer::next_token`] hands out one token at a time and only ever buffers a single physical
/// line's worth of tokens, so a caller that pulls lazily never materializes the token stream for
/// the whole source. [`Lexer::lex`] is the eager convenience wrapper that drains it into a `Vec`,
/// preserving the eager behaviour this crate has always had.
pub struct Lexer<'a> {
    source: &'a str,
    /// Byte offset of the next physical line to scan.
    line_start: usize,
    /// Tokens produced for one physical line but not yet handed out.
    queue: Vec<Token>,
    queue_pos: usize,
    /// Set once the final [`TokenKind::Eof`] token has been handed out.
    eof_done: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, line_start: 0, queue: Vec::new(), queue_pos: 0, eof_done: false }
    }

    /// Pulls the next token, or `None` once the final [`TokenKind::Eof`] token has been returned.
    ///
    /// This is what makes `tpt-yaml-core`'s event parser ([`crate::stream`]) constant-memory with
    /// respect to the number of tokens in the source: at most one line of tokens is ever live.
    pub fn next_token(&mut self) -> Result<Option<Token>, YamlError> {
        loop {
            if self.queue_pos < self.queue.len() {
                let token = self.queue[self.queue_pos].clone();
                self.queue_pos += 1;
                if self.queue_pos == self.queue.len() {
                    self.queue.clear();
                    self.queue_pos = 0;
                }
                return Ok(Some(token));
            }
            if self.eof_done {
                return Ok(None);
            }
            if self.line_start >= self.source.len() {
                self.eof_done = true;
                let span = self.span(self.source.len(), self.source.len());
                return Ok(Some(Token { kind: TokenKind::Eof, span, indent: 0 }));
            }
            self.scan_line()?;
        }
    }

    /// Tokenizes the entire source at once — equivalent to draining [`Self::next_token`].
    pub fn lex(mut self) -> Result<Vec<Token>, YamlError> {
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token()? {
            let is_eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    /// Scans the physical line at `self.line_start` into `self.queue`, then advances
    /// `self.line_start` past it (or past the entire block scalar it introduced).
    fn scan_line(&mut self) -> Result<(), YamlError> {
        let line_start = self.line_start;
        let (content_end, next_start) = self.line_bounds(line_start);
        let line = &self.source[line_start..content_end];
        let indent = self.leading_indent(line, line_start)?;

        let mut tokens = Vec::new();
        let mut jump_to = None;

        if indent == line.len() {
            tokens.push(self.token(TokenKind::BlankLine, line_start, content_end, indent));
        } else {
            let mut col = indent;

            while col < line.len() {
                let ch = line[col..].chars().next().unwrap();

                if ch == ' ' {
                    col += 1;
                    continue;
                }
                if ch == '\t' {
                    return Err(self.error_at(
                        line_start + col,
                        ErrorKind::TabCharacter,
                        "tabs are not allowed",
                    ));
                }

                if ch == '#' {
                    let start = line_start + col;
                    tokens.push(self.token(
                        TokenKind::Comment(line[col + 1..].trim_start().to_string()),
                        start,
                        content_end,
                        indent,
                    ));
                    break;
                }

                if col == 0 && (line.starts_with("---") || line.starts_with("...")) {
                    let rest = &line[3..];
                    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                        let kind = if line.starts_with("---") {
                            TokenKind::DocumentStart
                        } else {
                            TokenKind::DocumentEnd
                        };
                        tokens.push(self.token(kind, line_start, line_start + 3, indent));
                        col = 3;
                        continue;
                    }
                }

                if ch == '-' && line[col + 1..].chars().next().map_or(true, char::is_whitespace) {
                    tokens.push(self.token(
                        TokenKind::SequenceEntry,
                        line_start + col,
                        line_start + col + 1,
                        indent,
                    ));
                    col += 1;
                    continue;
                }

                if ch == '?' && line[col + 1..].chars().next().map_or(true, char::is_whitespace) {
                    tokens.push(self.token(
                        TokenKind::MappingKey,
                        line_start + col,
                        line_start + col + 1,
                        indent,
                    ));
                    col += 1;
                    continue;
                }

                if ch == '&' || ch == '*' || ch == '!' {
                    let (name, end) = self.read_name(line, col + 1, line_start)?;
                    let start = line_start + col;
                    let kind = match ch {
                        '&' => TokenKind::Anchor(name),
                        '*' => TokenKind::Alias(name),
                        _ => TokenKind::Tag(name),
                    };
                    tokens.push(self.token(kind, start, line_start + end, indent));
                    col = end;
                    continue;
                }

                if ch == '%' && col == 0 {
                    tokens.push(self.token(
                        TokenKind::Directive(line[1..].trim_end().to_string()),
                        line_start,
                        content_end,
                        indent,
                    ));
                    break;
                }

                if let Some(kind) = punctuation(ch) {
                    tokens.push(self.token(kind, line_start + col, line_start + col + 1, indent));
                    col += 1;
                    continue;
                }

                if ch == ':' && line[col + 1..].chars().next().map_or(true, char::is_whitespace) {
                    tokens.push(self.token(
                        TokenKind::Colon,
                        line_start + col,
                        line_start + col + 1,
                        indent,
                    ));
                    col += 1;
                    continue;
                }

                if ch == '|' || ch == '>' {
                    let start = line_start + col;
                    let (indicator, chomping, indentation, value, next) =
                        self.scan_block_scalar(ch, line, col, line_start, indent)?;
                    tokens.push(Token {
                        kind: TokenKind::BlockScalar { indicator, chomping, indentation, value },
                        span: self.span(start, next),
                        indent,
                    });
                    jump_to = Some(next);
                    break;
                }

                if ch == '\'' || ch == '"' {
                    let start = line_start + col;
                    let (raw, end) = self.read_quoted(line, col, ch, line_start)?;
                    let style = if ch == '\'' {
                        ScalarStyle::SingleQuoted
                    } else {
                        ScalarStyle::DoubleQuoted
                    };
                    tokens.push(self.token(
                        TokenKind::Scalar { raw, style, tag: None },
                        start,
                        line_start + end,
                        indent,
                    ));
                    col = end;
                    continue;
                }

                let start = line_start + col;
                let (raw, end) = self.read_plain(line, col);
                tokens.push(self.token(
                    TokenKind::Scalar { raw, style: ScalarStyle::Plain, tag: None },
                    start,
                    line_start + end,
                    indent,
                ));
                col = end;
            }
        }

        self.queue = tokens;
        self.queue_pos = 0;
        self.line_start = jump_to.unwrap_or(next_start);
        Ok(())
    }

    /// Returns `(content_end, next_line_start)` for the physical line beginning at `start`:
    /// `content_end` excludes a trailing `\r`, `next_line_start` is the byte offset right
    /// after the line's terminator (or `source.len()` at EOF).
    fn line_bounds(&self, start: usize) -> (usize, usize) {
        let line_end = self.source[start..].find('\n').map_or(self.source.len(), |p| start + p);
        let content_end = if line_end > start && self.source.as_bytes()[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };
        let next_start =
            if line_end >= self.source.len() { self.source.len() } else { line_end + 1 };
        (content_end, next_start)
    }

    fn leading_indent(&self, line: &str, line_start: usize) -> Result<usize, YamlError> {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b' ' => i += 1,
                b'\t' => {
                    return Err(self.error_at(
                        line_start + i,
                        ErrorKind::TabCharacter,
                        "tabs are not allowed for indentation",
                    ));
                }
                _ => break,
            }
        }
        Ok(i)
    }

    fn read_name(
        &self,
        line: &str,
        start: usize,
        line_start: usize,
    ) -> Result<(String, usize), YamlError> {
        let mut p = start;
        while p < line.len() {
            let ch = line[p..].chars().next().unwrap();
            if ch.is_alphanumeric() || matches!(ch, '-' | '_' | ':' | '!') {
                p += ch.len_utf8();
            } else {
                break;
            }
        }
        if p == start {
            return Err(self.error_at(
                line_start + start,
                ErrorKind::InvalidAnchor,
                "expected anchor or alias name",
            ));
        }
        Ok((line[start..p].to_string(), p))
    }

    fn read_quoted(
        &self,
        line: &str,
        col: usize,
        quote: char,
        line_start: usize,
    ) -> Result<(String, usize), YamlError> {
        let mut value = String::new();
        let mut p = col + 1;
        loop {
            if p >= line.len() {
                return Err(self.error_at(
                    line_start + p,
                    ErrorKind::InvalidScalar,
                    "unterminated quoted scalar",
                ));
            }
            let ch = line[p..].chars().next().unwrap();
            if ch == quote {
                if quote == '\'' && line[p + 1..].starts_with('\'') {
                    value.push('\'');
                    p += 2;
                    continue;
                }
                return Ok((value, p + 1));
            }
            if quote == '"' && ch == '\\' {
                let esc = line[p + 1..].chars().next().ok_or_else(|| {
                    self.error_at(
                        line_start + p,
                        ErrorKind::InvalidScalar,
                        "unterminated escape sequence",
                    )
                })?;
                match esc {
                    'n' => {
                        value.push('\n');
                        p += 2;
                    }
                    'r' => {
                        value.push('\r');
                        p += 2;
                    }
                    't' => {
                        value.push('\t');
                        p += 2;
                    }
                    '0' => {
                        value.push('\0');
                        p += 2;
                    }
                    '"' => {
                        value.push('"');
                        p += 2;
                    }
                    '\\' => {
                        value.push('\\');
                        p += 2;
                    }
                    'x' | 'X' => {
                        let hex = line.get(p + 2..p + 4).ok_or_else(|| {
                            self.error_at(
                                line_start + p,
                                ErrorKind::InvalidScalar,
                                "invalid hexadecimal escape",
                            )
                        })?;
                        let code = u8::from_str_radix(hex, 16).map_err(|_| {
                            self.error_at(
                                line_start + p,
                                ErrorKind::InvalidScalar,
                                "invalid hexadecimal escape",
                            )
                        })?;
                        value.push(code as char);
                        p += 4;
                    }
                    'u' | 'U' => {
                        let width = if esc == 'u' { 4 } else { 8 };
                        let hex = line.get(p + 2..p + 2 + width).ok_or_else(|| {
                            self.error_at(
                                line_start + p,
                                ErrorKind::InvalidScalar,
                                "invalid unicode escape",
                            )
                        })?;
                        let code = u32::from_str_radix(hex, 16).map_err(|_| {
                            self.error_at(
                                line_start + p,
                                ErrorKind::InvalidScalar,
                                "invalid unicode escape",
                            )
                        })?;
                        let decoded = char::from_u32(code).ok_or_else(|| {
                            self.error_at(
                                line_start + p,
                                ErrorKind::InvalidScalar,
                                "invalid unicode escape",
                            )
                        })?;
                        value.push(decoded);
                        p += 2 + width;
                    }
                    other => {
                        value.push(other);
                        p += 1 + other.len_utf8();
                    }
                }
                continue;
            }
            value.push(ch);
            p += ch.len_utf8();
        }
    }

    /// Reads a plain (unquoted) scalar starting at `col`. The caller guarantees the character
    /// at `col` is not whitespace, `#`, a flow indicator, or a `: `-style mapping-value colon,
    /// so the result is always non-empty.
    /// Reads a plain (unquoted) scalar starting at `col`, up to (not including) whichever of
    /// end-of-line, a ` #` comment, a flow indicator, or a `: `-style value colon comes first.
    /// Internal whitespace is preserved (YAML only folds *across* lines, which plain scalars
    /// here don't span); trailing whitespace before the stop point is trimmed from the result.
    fn read_plain(&self, line: &str, col: usize) -> (String, usize) {
        let mut content_end = col;
        let mut i = col;
        while i < line.len() {
            let ch = line[i..].chars().next().unwrap();
            if ch == '#' && line[..i].chars().next_back().map_or(true, char::is_whitespace) {
                break;
            }
            if matches!(ch, '{' | '}' | '[' | ']' | ',') {
                break;
            }
            if ch == ':' {
                let after = i + ch.len_utf8();
                if line[after..].chars().next().map_or(true, char::is_whitespace) {
                    break;
                }
            }
            if !ch.is_whitespace() {
                content_end = i + ch.len_utf8();
            }
            i += ch.len_utf8();
        }
        (line[col..content_end].to_string(), i)
    }

    fn scan_block_scalar(
        &self,
        ch: char,
        line: &str,
        col: usize,
        line_start: usize,
        indent: usize,
    ) -> Result<(BlockIndicator, Chomping, Option<usize>, String, usize), YamlError> {
        let bytes = line.as_bytes();
        let mut q = col + 1;
        let mut chomping = Chomping::Clip;
        let mut explicit_indent = None;
        while q < bytes.len() {
            match bytes[q] {
                b'+' => {
                    chomping = Chomping::Keep;
                    q += 1;
                }
                b'-' => {
                    chomping = Chomping::Strip;
                    q += 1;
                }
                b'1'..=b'9' => {
                    explicit_indent = Some((bytes[q] - b'0') as usize);
                    q += 1;
                }
                b' ' | b'\t' | b'#' => break,
                _ => {
                    return Err(self.error_at(
                        line_start + q,
                        ErrorKind::InvalidScalar,
                        "invalid block scalar header",
                    ))
                }
            }
        }

        let (_, mut current) = self.line_bounds(line_start);
        let mut base_indent = explicit_indent.map(|n| indent + n);
        let mut raw_lines: Vec<&str> = Vec::new();

        while current < self.source.len() {
            let (content_end, next_start) = self.line_bounds(current);
            let content = &self.source[current..content_end];
            let leading = content.bytes().take_while(|b| *b == b' ').count();
            let is_blank = leading == content.len();

            if !is_blank {
                match base_indent {
                    Some(bi) if leading < bi => break,
                    None if leading <= indent => break,
                    None => base_indent = Some(leading),
                    _ => {}
                }
            }

            raw_lines.push(content);
            current = next_start;
        }

        let bi = base_indent.unwrap_or(indent + 1);
        let mut value = String::new();
        for content in &raw_lines {
            if content.len() <= bi {
                value.push('\n');
            } else {
                value.push_str(&content[bi..]);
                value.push('\n');
            }
        }

        match chomping {
            Chomping::Strip => {
                while value.ends_with('\n') {
                    value.pop();
                }
            }
            Chomping::Clip => {
                while value.ends_with("\n\n") {
                    value.pop();
                }
            }
            Chomping::Keep => {}
        }

        let indicator = if ch == '|' { BlockIndicator::Literal } else { BlockIndicator::Folded };
        Ok((indicator, chomping, explicit_indent, value, current))
    }

    fn token(&self, kind: TokenKind, start: usize, end: usize, indent: usize) -> Token {
        Token { kind, span: self.span(start, end), indent }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let start = start.min(self.source.len());
        let end = end.min(self.source.len());
        let line = self.source[..start].bytes().filter(|b| *b == b'\n').count() + 1;
        let line_start = self.source[..start].rfind('\n').map_or(0, |p| p + 1);
        Span::new(start, end, line, start.saturating_sub(line_start) + 1)
    }

    fn error_at(&self, offset: usize, kind: ErrorKind, message: &str) -> YamlError {
        YamlError::new(offset.min(self.source.len()), kind, message, self.source)
    }
}

fn punctuation(ch: char) -> Option<TokenKind> {
    match ch {
        '{' => Some(TokenKind::LeftBrace),
        '}' => Some(TokenKind::RightBrace),
        '[' => Some(TokenKind::LeftBracket),
        ']' => Some(TokenKind::RightBracket),
        ',' => Some(TokenKind::Comma),
        _ => None,
    }
}

pub fn lex(source: &str) -> Result<Vec<Token>, YamlError> {
    Lexer::new(source).lex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_simple_mapping() {
        let tokens = lex("key: value\n").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Scalar { .. }));
        assert!(matches!(tokens[1].kind, TokenKind::Colon));
        assert!(matches!(tokens[2].kind, TokenKind::Scalar { .. }));
        assert!(matches!(tokens.last().unwrap().kind, TokenKind::Eof));
    }

    #[test]
    fn rejects_tab_indentation() {
        let err = lex("key:\n\tvalue\n").unwrap_err();
        assert_eq!(err.kind, ErrorKind::TabCharacter);
    }

    #[test]
    fn lexes_block_literal_scalar() {
        let tokens = lex("text: |\n  line one\n  line two\nafter: 1\n").unwrap();
        let block = tokens.iter().find_map(|t| match &t.kind {
            TokenKind::BlockScalar { value, .. } => Some(value.clone()),
            _ => None,
        });
        assert_eq!(block.as_deref(), Some("line one\nline two\n"));
    }

    #[test]
    fn lexes_quoted_scalars_with_escapes() {
        let tokens = lex("k: \"a\\nb\"\n").unwrap();
        let raw = tokens.iter().find_map(|t| match &t.kind {
            TokenKind::Scalar { raw, style: ScalarStyle::DoubleQuoted, .. } => Some(raw.clone()),
            _ => None,
        });
        assert_eq!(raw.as_deref(), Some("a\nb"));
    }

    #[test]
    fn lexes_anchor_and_alias() {
        let tokens = lex("a: &x 1\nb: *x\n").unwrap();
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Anchor(name) if name == "x")));
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Alias(name) if name == "x")));
    }
}
