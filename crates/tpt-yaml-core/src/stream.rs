//! Event-driven parsing straight from source, for constant-memory decode of large documents.
//!
//! [`crate::event`] replays an *already fully parsed* [`Document`](crate::Document) as a
//! [`crate::event::Event`] list indexed by [`NodeId`](crate::NodeId). That is a convenient
//! inventory of a parsed arena, but it cannot be produced without first materializing the whole
//! arena, so it is not a streaming API.
//!
//! [`EventParser`] is the real thing: a pull-based, resumable parser that reads tokens lazily
//! from [`crate::lexer::Lexer::next_token`] and emits self-contained [`Event`]s without ever
//! building a [`Document`](crate::Document). Memory use is O(nesting depth) for the parser plus
//! O(number of distinct anchors) for alias validation — not O(number of nodes). A caller can
//! therefore fold an arbitrarily large stream of documents through a fixed-size buffer:
//!
//! ```no_run
//! use tpt_yaml_core::stream::{Event, EventParser};
//! use tpt_yaml_core::ParserOptions;
//!
//! let source = "key: value\n";
//! let mut parser = EventParser::new(source, &ParserOptions::default());
//! while let Some(event) = parser.next_event()? {
//!     if let Event::Scalar(scalar) = event {
//!         // Consume one scalar, then drop it.
//!         let _ = scalar.value;
//!     }
//! }
//! # Ok::<(), tpt_yaml_core::YamlError>(())
//! ```
//!
//! # Differences from [`crate::parse`]
//!
//! - Merge keys (`<<`) are **rejected** with [`ErrorKind::InvalidMergeKey`] rather than expanded.
//!   Expanding a merge key requires replaying the referenced mapping's entries, which is only
//!   possible with the anchored mapping held in memory. Set [`ParserOptions::merge_keys`] to
//!   `false` to treat `<<` as an ordinary key instead of failing.
//! - A collection's event span is the span of its opening token, rather than the full byte extent
//!   the arena parser records on a container node.
//! - Aliases are emitted as [`Event::Alias`] carrying the anchor *name*; a consumer that needs the
//!   anchored value must remember it itself (see `tpt-yaml-serde`'s `streaming` feature for one
//!   implementation of that).

use crate::lexer::{BlockIndicator, Lexer, ScalarStyle as LexScalarStyle, Token, TokenKind};
use crate::node::{ScalarStyle, ScalarValue};
use crate::parser::ParserOptions;
use crate::{ErrorKind, Span, YamlError, YamlVersion};
use alloc::collections::{BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

/// Metadata carried by a collection's start event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeMeta {
    /// The anchor declared on the node, if any (`&name`).
    pub anchor: Option<String>,
    /// The explicit tag declared on the node, if any (`!tag`).
    pub tag: Option<String>,
    /// The span of the collection's opening token.
    pub span: Span,
}

/// A scalar node, with its value already implicitly resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct ScalarEvent {
    /// The resolved value (`Plain` style only type-resolves; quoted/block scalars stay strings).
    pub value: ScalarValue,
    /// The quoting/block style the scalar was written in.
    pub style: ScalarStyle,
    /// The source spelling of the scalar, without surrounding quotes.
    pub raw: String,
    /// The anchor declared on the scalar, if any.
    pub anchor: Option<String>,
    /// The explicit tag declared on the scalar, if any.
    pub tag: Option<String>,
    pub span: Span,
}

/// An alias node (`*name`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasEvent {
    /// The name being aliased. The anchor's own event was emitted earlier in the stream.
    pub name: String,
    /// An anchor declared on the alias itself, if any (`&x *y`).
    pub anchor: Option<String>,
    /// The explicit tag declared on the alias, if any.
    pub tag: Option<String>,
    pub span: Span,
}

/// One step of the event stream produced by [`EventParser`].
///
/// The stream is well-formed by construction: every `*Start` is matched by exactly one
/// corresponding `*End`, and every `DocumentStart` is matched by one `DocumentEnd`.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    MappingStart(NodeMeta),
    MappingEnd,
    SequenceStart(NodeMeta),
    SequenceEnd,
    Scalar(ScalarEvent),
    Alias(AliasEvent),
}

/// A cheap, copyable discriminant for a [`TokenKind`], used for lookahead decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
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
    Anchor,
    Alias,
    Tag,
    Directive,
    Scalar,
    BlockScalar,
    Comment,
    BlankLine,
    Eof,
}

fn class_of(kind: &TokenKind) -> Class {
    match kind {
        TokenKind::DocumentStart => Class::DocumentStart,
        TokenKind::DocumentEnd => Class::DocumentEnd,
        TokenKind::MappingKey => Class::MappingKey,
        TokenKind::SequenceEntry => Class::SequenceEntry,
        TokenKind::Colon => Class::Colon,
        TokenKind::Comma => Class::Comma,
        TokenKind::LeftBrace => Class::LeftBrace,
        TokenKind::RightBrace => Class::RightBrace,
        TokenKind::LeftBracket => Class::LeftBracket,
        TokenKind::RightBracket => Class::RightBracket,
        TokenKind::Anchor(_) => Class::Anchor,
        TokenKind::Alias(_) => Class::Alias,
        TokenKind::Tag(_) => Class::Tag,
        TokenKind::Directive(_) => Class::Directive,
        TokenKind::Scalar { .. } => Class::Scalar,
        TokenKind::BlockScalar { .. } => Class::BlockScalar,
        TokenKind::Comment(_) => Class::Comment,
        TokenKind::BlankLine => Class::BlankLine,
        TokenKind::Eof => Class::Eof,
    }
}

fn convert_style(style: LexScalarStyle) -> ScalarStyle {
    match style {
        LexScalarStyle::Plain => ScalarStyle::Plain,
        LexScalarStyle::SingleQuoted => ScalarStyle::SingleQuoted,
        LexScalarStyle::DoubleQuoted => ScalarStyle::DoubleQuoted,
    }
}

fn null_scalar_event(span: Span) -> Event {
    Event::Scalar(ScalarEvent {
        value: ScalarValue::Null,
        style: ScalarStyle::Plain,
        raw: String::new(),
        anchor: None,
        tag: None,
        span,
    })
}

/// Which container a stack frame is parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameKind {
    Document,
    BlockMapping,
    BlockSequence,
    FlowMapping,
    FlowSequence,
}

/// What a frame is waiting for next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameState {
    /// Waiting for the next entry (mapping key, sequence item, flow element).
    BeginEntry,
    /// A mapping key was parsed; waiting for `:` and then the value.
    ExpectColon,
    /// A value was parsed; the frame is ready to move on.
    AfterValue,
}

/// Where the top-level stream is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamState {
    Start,
    BetweenDocuments,
    Finished,
}

/// One entry of the explicit parse stack. Replaces the host stack that the recursive-descent
/// arena parser (`crate::parser`) relies on, which is what lets this parser suspend and resume
/// between events.
struct Frame {
    kind: FrameKind,
    state: FrameState,
    /// Column of the first entry/item, for block collections. `None` until the first one is seen.
    entry_col: Option<usize>,
    /// Column/line of the mapping key currently awaiting its `:`.
    key_col: usize,
    key_line: usize,
    /// Span used for a document root that turns out to be empty.
    null_span: Option<Span>,
    /// Anchors declared on this node, registered once the node is complete.
    pending_anchors: Vec<(String, Span)>,
}

impl Frame {
    fn new(kind: FrameKind) -> Self {
        Self {
            kind,
            state: FrameState::BeginEntry,
            entry_col: None,
            key_col: 0,
            key_line: 0,
            null_span: None,
            pending_anchors: Vec::new(),
        }
    }
}

/// A pull-based, source-driven event parser.
///
/// See the [module documentation](self) for why this exists alongside [`crate::event`].
pub struct EventParser<'a> {
    source: &'a str,
    lexer: Lexer<'a>,
    /// Token lookahead, never more than two tokens deep.
    lookahead: VecDeque<Token>,
    /// Events produced by the most recent step but not yet handed out.
    outbox: VecDeque<Event>,
    frames: Vec<Frame>,
    stream: StreamState,
    documents: usize,
    anchors: BTreeSet<String>,
    version: YamlVersion,
    version_explicit: bool,
    /// Whether `version` was pinned, either by `ParserOptions.yaml_version` or by a `%YAML`
    /// directive — as opposed to just defaulting to 1.2. Mirrors `crate::parser::Parser`'s field
    /// of the same name; used by `strict_version` to know when there's no ambiguity to detect.
    version_pinned: bool,
    merge_keys: bool,
    strict_version: bool,
    finished: bool,
}

impl<'a> EventParser<'a> {
    /// Creates a parser over `source`. No parsing happens until the first [`Self::next_event`].
    pub fn new(source: &'a str, options: &ParserOptions) -> Self {
        Self {
            source,
            lexer: Lexer::new(source),
            lookahead: VecDeque::new(),
            outbox: VecDeque::new(),
            frames: Vec::new(),
            stream: StreamState::Start,
            documents: 0,
            anchors: BTreeSet::new(),
            version: options.yaml_version.unwrap_or(YamlVersion::Version12),
            version_explicit: options.yaml_version.is_some(),
            version_pinned: options.yaml_version.is_some(),
            merge_keys: options.merge_keys,
            strict_version: options.strict_version,
            finished: false,
        }
    }

    /// Pulls the next event, or `None` once the stream is exhausted.
    ///
    /// After an error is returned the parser is exhausted; further calls return `None`.
    pub fn next_event(&mut self) -> Result<Option<Event>, YamlError> {
        if let Some(event) = self.outbox.pop_front() {
            return Ok(Some(event));
        }
        if self.finished {
            return Ok(None);
        }
        self.pump()?;
        Ok(self.outbox.pop_front())
    }

    /// Drops the remainder of the stream and marks the parser exhausted.
    pub fn finish(&mut self) {
        self.finished = true;
        self.outbox.clear();
    }

    /// Parses the whole source into an event list. Convenience for tests and small inputs; do not
    /// use this on a large document, since it materializes every event.
    pub fn collect_events(&mut self) -> Result<Vec<Event>, YamlError> {
        let mut events = Vec::new();
        while let Some(event) = self.next_event()? {
            events.push(event);
        }
        Ok(events)
    }

    // -- token lookahead -------------------------------------------------------------------

    fn ensure(&mut self, depth: usize) -> Result<(), YamlError> {
        while self.lookahead.len() < depth {
            match self.lexer.next_token()? {
                Some(token) => self.lookahead.push_back(token),
                None => break,
            }
        }
        Ok(())
    }

    fn class_at(&mut self, offset: usize) -> Result<Option<Class>, YamlError> {
        self.ensure(offset + 1)?;
        Ok(self.lookahead.get(offset).map(|token| class_of(&token.kind)))
    }

    fn span_at(&mut self, offset: usize) -> Result<Option<Span>, YamlError> {
        self.ensure(offset + 1)?;
        Ok(self.lookahead.get(offset).map(|token| token.span))
    }

    fn token_at(&mut self, offset: usize) -> Result<Option<Token>, YamlError> {
        self.ensure(offset + 1)?;
        Ok(self.lookahead.get(offset).cloned())
    }

    fn pop(&mut self) -> Result<Option<Token>, YamlError> {
        self.ensure(1)?;
        Ok(self.lookahead.pop_front())
    }

    fn at_eof(&mut self) -> Result<bool, YamlError> {
        Ok(matches!(self.class_at(0)?, None | Some(Class::Eof)))
    }

    fn peek_is_colon(&mut self) -> Result<bool, YamlError> {
        Ok(matches!(self.class_at(1)?, Some(Class::Colon)))
    }

    fn skip_trivia(&mut self) -> Result<(), YamlError> {
        while matches!(self.class_at(0)?, Some(Class::Comment) | Some(Class::BlankLine)) {
            self.pop()?;
        }
        Ok(())
    }
}

/// The driver: advances the explicit parse stack until at least one event is queued.
impl EventParser<'_> {
    fn pump(&mut self) -> Result<(), YamlError> {
        loop {
            if !self.outbox.is_empty() || self.finished {
                return Ok(());
            }
            match self.frames.last().map(|frame| frame.kind) {
                None => self.step_stream()?,
                Some(FrameKind::Document) => self.step_document()?,
                Some(FrameKind::BlockMapping) => self.step_block_mapping()?,
                Some(FrameKind::BlockSequence) => self.step_block_sequence()?,
                Some(FrameKind::FlowMapping) => self.step_flow_mapping()?,
                Some(FrameKind::FlowSequence) => self.step_flow_sequence()?,
            }
        }
    }

    fn step_stream(&mut self) -> Result<(), YamlError> {
        match self.stream {
            StreamState::Start => {
                self.stream = StreamState::BetweenDocuments;
                self.outbox.push_back(Event::StreamStart);
            }
            StreamState::BetweenDocuments => {
                self.skip_trivia()?;
                self.consume_directives()?;
                if self.at_eof()? {
                    if self.documents == 0 {
                        // Mirrors `tpt_yaml_core::parse`, which always yields at least one
                        // (null-rooted) document even for empty or trivia-only input.
                        self.documents += 1;
                        self.outbox.push_back(Event::DocumentStart);
                        self.outbox.push_back(null_scalar_event(Span::new(0, 0, 1, 1)));
                        self.outbox.push_back(Event::DocumentEnd);
                    }
                    self.stream = StreamState::Finished;
                    self.finished = true;
                    self.outbox.push_back(Event::StreamEnd);
                    return Ok(());
                }
                let doc_start_span = self.maybe_consume_document_start()?;
                self.skip_trivia()?;
                let mut frame = Frame::new(FrameKind::Document);
                frame.null_span = doc_start_span.or(self.span_at(0)?);
                self.frames.push(frame);
                self.documents += 1;
                self.outbox.push_back(Event::DocumentStart);
            }
            StreamState::Finished => {
                self.finished = true;
            }
        }
        Ok(())
    }

    fn step_document(&mut self) -> Result<(), YamlError> {
        match self.state() {
            Some(FrameState::BeginEntry) => {
                self.skip_trivia()?;
                let empty = self.at_eof()? || matches!(self.class_at(0)?, Some(Class::DocumentEnd));
                self.set_state(FrameState::AfterValue);
                if empty {
                    let span = self
                        .frames
                        .last()
                        .and_then(|frame| frame.null_span)
                        .unwrap_or(Span::new(0, 0, 1, 1));
                    self.emit_null(span);
                } else {
                    self.begin_value()?;
                }
            }
            Some(FrameState::AfterValue) => {
                self.skip_trivia()?;
                self.consume_document_end()?;
                self.skip_trivia()?;
                self.consume_directives()?;
                if !self.at_eof()? && !matches!(self.class_at(0)?, Some(Class::DocumentStart)) {
                    return Err(self
                        .error(ErrorKind::TrailingContent, "expected '---' before next document"));
                }
                self.outbox.push_back(Event::DocumentEnd);
                self.frames.pop();
            }
            Some(FrameState::ExpectColon) => {
                unreachable!("a document frame never waits for a colon")
            }
            None => unreachable!("step_document requires a document frame"),
        }
        Ok(())
    }

    fn state(&self) -> Option<FrameState> {
        self.frames.last().map(|frame| frame.state)
    }

    fn set_state(&mut self, state: FrameState) {
        if let Some(frame) = self.frames.last_mut() {
            frame.state = state;
        }
    }

    fn emit_null(&mut self, span: Span) {
        self.outbox.push_back(null_scalar_event(span));
    }

    fn close_mapping(&mut self) -> Result<(), YamlError> {
        if let Some(frame) = self.frames.pop() {
            self.register_anchors(frame.pending_anchors)?;
        }
        self.outbox.push_back(Event::MappingEnd);
        Ok(())
    }

    fn close_sequence(&mut self) -> Result<(), YamlError> {
        if let Some(frame) = self.frames.pop() {
            self.register_anchors(frame.pending_anchors)?;
        }
        self.outbox.push_back(Event::SequenceEnd);
        Ok(())
    }

    fn push_frame(&mut self, kind: FrameKind, pending_anchors: Vec<(String, Span)>) {
        let mut frame = Frame::new(kind);
        frame.pending_anchors = pending_anchors;
        self.frames.push(frame);
    }

    fn register_anchors(&mut self, anchors: Vec<(String, Span)>) -> Result<(), YamlError> {
        for (name, span) in anchors {
            if !self.anchors.insert(name) {
                return Err(YamlError::new(
                    span.start,
                    ErrorKind::InvalidAnchor,
                    "duplicate anchor",
                    self.source,
                ));
            }
        }
        Ok(())
    }
}

/// Document markers, directives, and contextual "does a value follow?" decisions.
impl EventParser<'_> {
    fn consume_directives(&mut self) -> Result<(), YamlError> {
        loop {
            self.skip_trivia()?;
            if !matches!(self.class_at(0)?, Some(Class::Directive)) {
                return Ok(());
            }
            let token = self.pop()?.ok_or_else(|| self.eof_error())?;
            if let TokenKind::Directive(text) = &token.kind {
                self.apply_directive(text, token.span)?;
            }
        }
    }

    fn apply_directive(&mut self, text: &str, span: Span) -> Result<(), YamlError> {
        let mut parts = text.split_whitespace();
        if parts.next() == Some("YAML") {
            if let Some(version_str) = parts.next() {
                match version_str.parse::<YamlVersion>() {
                    Ok(version) => {
                        if !self.version_explicit {
                            self.version = version;
                            self.version_pinned = true;
                        }
                    }
                    Err(()) => {
                        return Err(YamlError::new(
                            span.start,
                            ErrorKind::InvalidDirective,
                            "unrecognized %YAML version",
                            self.source,
                        ))
                    }
                }
            }
        }
        Ok(())
    }

    fn maybe_consume_document_start(&mut self) -> Result<Option<Span>, YamlError> {
        if matches!(self.class_at(0)?, Some(Class::DocumentStart)) {
            let span = self.span_at(0)?;
            self.pop()?;
            Ok(span)
        } else {
            Ok(None)
        }
    }

    fn consume_document_end(&mut self) -> Result<Option<Span>, YamlError> {
        if matches!(self.class_at(0)?, Some(Class::DocumentEnd)) {
            let span = self.span_at(0)?;
            self.pop()?;
            Ok(span)
        } else {
            Ok(None)
        }
    }

    /// Whether a value follows a mapping key at `key_col`/`key_line`. Mirrors the arena parser:
    /// block sequences may align with their key (a common YAML exception); everything else must
    /// be indented strictly further, unless it is inline on the key's own line.
    fn value_follows_key(&mut self, key_col: usize, key_line: usize) -> Result<bool, YamlError> {
        let (Some(class), Some(span)) = (self.class_at(0)?, self.span_at(0)?) else {
            return Ok(false);
        };
        if span.line == key_line {
            return Ok(!matches!(class, Class::DocumentEnd | Class::DocumentStart));
        }
        match class {
            Class::DocumentStart | Class::DocumentEnd => Ok(false),
            Class::SequenceEntry => Ok(span.column >= key_col),
            _ => Ok(span.column > key_col),
        }
    }

    /// Whether a value follows a sequence dash at `dash_col`/`dash_line`.
    fn value_follows_dash(&mut self, dash_col: usize, dash_line: usize) -> Result<bool, YamlError> {
        let (Some(class), Some(span)) = (self.class_at(0)?, self.span_at(0)?) else {
            return Ok(false);
        };
        if span.line == dash_line {
            return Ok(!matches!(class, Class::Comma | Class::RightBrace | Class::RightBracket));
        }
        match class {
            Class::DocumentStart | Class::DocumentEnd => Ok(false),
            _ => Ok(span.column > dash_col),
        }
    }
}

/// Value dispatch: anchor/tag prefixes plus every value shape the arena parser accepts.
impl EventParser<'_> {
    /// Parses one value, either emitting a leaf event immediately or pushing a container frame
    /// whose closing `*End` event is emitted once its contents are exhausted.
    fn begin_value(&mut self) -> Result<(), YamlError> {
        self.skip_trivia()?;

        let mut anchor: Option<String> = None;
        let mut tag: Option<String> = None;
        let mut pending_anchors: Vec<(String, Span)> = Vec::new();

        loop {
            match self.class_at(0)? {
                Some(Class::Anchor) => {
                    let token = self.pop()?.ok_or_else(|| self.eof_error())?;
                    if let TokenKind::Anchor(name) = token.kind {
                        if anchor.is_none() {
                            anchor = Some(name.clone());
                        }
                        pending_anchors.push((name, token.span));
                    }
                    self.skip_trivia()?;
                }
                Some(Class::Tag) => {
                    let token = self.pop()?.ok_or_else(|| self.eof_error())?;
                    if let TokenKind::Tag(name) = token.kind {
                        if tag.is_none() {
                            tag = Some(name);
                        }
                    }
                    self.skip_trivia()?;
                }
                _ => break,
            }
        }

        let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
        let span = token.span;
        let meta = NodeMeta { anchor: anchor.clone(), tag: tag.clone(), span };
        let starts_mapping =
            matches!(token.kind, TokenKind::Scalar { .. }) && self.peek_is_colon()?;

        match &token.kind {
            TokenKind::Scalar { .. } if starts_mapping => {
                self.push_frame(FrameKind::BlockMapping, pending_anchors);
                self.outbox.push_back(Event::MappingStart(meta));
            }
            TokenKind::Scalar { raw, style, .. } => {
                self.reject_merge_key(raw, span)?;
                let value = self.resolve(raw, *style, span)?;
                self.register_anchors(pending_anchors)?;
                self.pop()?;
                self.outbox.push_back(Event::Scalar(ScalarEvent {
                    value,
                    style: convert_style(*style),
                    raw: raw.clone(),
                    anchor,
                    tag,
                    span,
                }));
            }
            TokenKind::BlockScalar { indicator, value, .. } => {
                let style = match indicator {
                    BlockIndicator::Literal => ScalarStyle::BlockLiteral,
                    BlockIndicator::Folded => ScalarStyle::BlockChomped,
                };
                self.register_anchors(pending_anchors)?;
                self.pop()?;
                self.outbox.push_back(Event::Scalar(ScalarEvent {
                    value: ScalarValue::String(value.clone()),
                    style,
                    raw: value.clone(),
                    anchor,
                    tag,
                    span,
                }));
            }
            TokenKind::Alias(name) => {
                let name = name.clone();
                if !self.anchors.contains(&name) {
                    return Err(self.error_at_token(
                        &token,
                        ErrorKind::UnknownAnchor,
                        "unknown anchor",
                    ));
                }
                self.register_anchors(pending_anchors)?;
                self.pop()?;
                self.outbox.push_back(Event::Alias(AliasEvent { name, anchor, tag, span }));
            }
            TokenKind::LeftBrace => {
                self.pop()?;
                self.push_frame(FrameKind::FlowMapping, pending_anchors);
                self.outbox.push_back(Event::MappingStart(meta));
            }
            TokenKind::LeftBracket => {
                self.pop()?;
                self.push_frame(FrameKind::FlowSequence, pending_anchors);
                self.outbox.push_back(Event::SequenceStart(meta));
            }
            TokenKind::SequenceEntry => {
                self.push_frame(FrameKind::BlockSequence, pending_anchors);
                self.outbox.push_back(Event::SequenceStart(meta));
            }
            TokenKind::MappingKey => {
                self.push_frame(FrameKind::BlockMapping, pending_anchors);
                self.outbox.push_back(Event::MappingStart(meta));
            }
            _ => {
                return Err(self.error_at_token(
                    &token,
                    ErrorKind::UnexpectedCharacter,
                    "unexpected token in value position",
                ))
            }
        }
        Ok(())
    }
}

/// Block collection steps.
impl EventParser<'_> {
    fn step_block_mapping(&mut self) -> Result<(), YamlError> {
        match self.state() {
            Some(FrameState::BeginEntry) => {
                self.skip_trivia()?;
                let Some(class) = self.class_at(0)? else {
                    return self.close_mapping();
                };
                if !matches!(class, Class::MappingKey | Class::Scalar) {
                    return self.close_mapping();
                }
                let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
                if let Some(entry_col) = self.frames.last().and_then(|frame| frame.entry_col) {
                    if token.span.column != entry_col {
                        return self.close_mapping();
                    }
                } else if let Some(frame) = self.frames.last_mut() {
                    frame.entry_col = Some(token.span.column);
                }
                if matches!(token.kind, TokenKind::MappingKey) {
                    self.pop()?;
                    self.skip_trivia()?;
                    self.set_key_position(&token);
                    self.set_state(FrameState::ExpectColon);
                    self.begin_value()?;
                } else {
                    self.emit_key_scalar(&token)?;
                    self.pop()?;
                    self.set_key_position(&token);
                    self.set_state(FrameState::ExpectColon);
                }
            }
            Some(FrameState::ExpectColon) => {
                self.skip_trivia()?;
                let Some(class) = self.class_at(0)? else {
                    return Err(
                        self.error(ErrorKind::UnexpectedEof, "expected colon after mapping key")
                    );
                };
                if class != Class::Colon {
                    let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
                    return Err(self.error_at_token(
                        &token,
                        ErrorKind::UnexpectedCharacter,
                        "expected colon after mapping key",
                    ));
                }
                let colon_span = self.span_at(0)?.unwrap_or(Span::new(0, 0, 1, 1));
                self.pop()?;
                self.skip_trivia()?;
                let (key_col, key_line) = self
                    .frames
                    .last()
                    .map(|frame| (frame.key_col, frame.key_line))
                    .unwrap_or((0, 0));
                self.set_state(FrameState::AfterValue);
                if self.value_follows_key(key_col, key_line)? {
                    self.begin_value()?;
                } else {
                    self.emit_null(colon_span);
                }
            }
            Some(FrameState::AfterValue) => self.set_state(FrameState::BeginEntry),
            None => unreachable!("step_block_mapping requires a mapping frame"),
        }
        Ok(())
    }

    fn step_block_sequence(&mut self) -> Result<(), YamlError> {
        self.skip_trivia()?;
        let Some(class) = self.class_at(0)? else {
            return self.close_sequence();
        };
        if class != Class::SequenceEntry {
            return self.close_sequence();
        }
        let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
        if let Some(entry_col) = self.frames.last().and_then(|frame| frame.entry_col) {
            if token.span.column != entry_col {
                return self.close_sequence();
            }
        } else if let Some(frame) = self.frames.last_mut() {
            frame.entry_col = Some(token.span.column);
        }
        self.pop()?;
        if self.value_follows_dash(token.span.column, token.span.line)? {
            self.begin_value()?;
        } else {
            self.emit_null(token.span);
        }
        Ok(())
    }

    fn set_key_position(&mut self, token: &Token) {
        if let Some(frame) = self.frames.last_mut() {
            frame.key_col = token.span.column;
            frame.key_line = token.span.line;
        }
    }
}

/// Flow collection steps.
impl EventParser<'_> {
    fn step_flow_sequence(&mut self) -> Result<(), YamlError> {
        match self.state() {
            Some(FrameState::BeginEntry) => {
                self.skip_trivia()?;
                match self.class_at(0)? {
                    None => self.close_sequence()?,
                    Some(Class::RightBracket) => {
                        self.pop()?;
                        self.close_sequence()?;
                    }
                    Some(Class::Comma) => {
                        self.pop()?;
                    }
                    Some(_) => {
                        self.set_state(FrameState::AfterValue);
                        self.begin_value()?;
                    }
                }
            }
            Some(FrameState::AfterValue) => {
                self.skip_trivia()?;
                match self.class_at(0)? {
                    None => self.close_sequence()?,
                    Some(Class::Comma) | Some(Class::RightBracket) => {
                        self.set_state(FrameState::BeginEntry)
                    }
                    Some(_) => {
                        let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
                        return Err(self.error_at_token(
                            &token,
                            ErrorKind::UnexpectedCharacter,
                            "expected comma or ] in flow sequence",
                        ));
                    }
                }
            }
            Some(FrameState::ExpectColon) => {
                unreachable!("a flow sequence never waits for a colon")
            }
            None => unreachable!("step_flow_sequence requires a sequence frame"),
        }
        Ok(())
    }

    fn step_flow_mapping(&mut self) -> Result<(), YamlError> {
        match self.state() {
            Some(FrameState::BeginEntry) => {
                self.skip_trivia()?;
                match self.class_at(0)? {
                    None => self.close_mapping()?,
                    Some(Class::RightBrace) => {
                        self.pop()?;
                        self.close_mapping()?;
                    }
                    Some(Class::Comma) => {
                        self.pop()?;
                    }
                    Some(_) => {
                        self.parse_flow_key()?;
                        self.set_state(FrameState::ExpectColon);
                    }
                }
            }
            Some(FrameState::ExpectColon) => {
                self.skip_trivia()?;
                let Some(class) = self.class_at(0)? else {
                    return Err(
                        self.error(ErrorKind::UnexpectedEof, "expected colon after mapping key")
                    );
                };
                if class != Class::Colon {
                    let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
                    return Err(self.error_at_token(
                        &token,
                        ErrorKind::UnexpectedCharacter,
                        "expected colon after mapping key",
                    ));
                }
                let colon_span = self.span_at(0)?.unwrap_or(Span::new(0, 0, 1, 1));
                self.pop()?;
                self.skip_trivia()?;
                self.set_state(FrameState::AfterValue);
                match self.class_at(0)? {
                    None | Some(Class::Comma) | Some(Class::RightBrace) => {
                        self.emit_null(colon_span)
                    }
                    Some(_) => self.begin_value()?,
                }
            }
            Some(FrameState::AfterValue) => {
                self.skip_trivia()?;
                match self.class_at(0)? {
                    None => self.close_mapping()?,
                    Some(Class::Comma) | Some(Class::RightBrace) => {
                        self.set_state(FrameState::BeginEntry)
                    }
                    Some(_) => {
                        let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
                        return Err(self.error_at_token(
                            &token,
                            ErrorKind::UnexpectedCharacter,
                            "expected comma or } in flow mapping",
                        ));
                    }
                }
            }
            None => unreachable!("step_flow_mapping requires a mapping frame"),
        }
        Ok(())
    }
}

/// Scalar emission and flow-key parsing.
impl EventParser<'_> {
    /// Emits a flow-mapping key. The arena parser restricts flow keys to scalars with optional
    /// `&anchor`/`!tag` prefixes, so this does too.
    fn parse_flow_key(&mut self) -> Result<(), YamlError> {
        self.skip_trivia()?;
        let mut anchor: Option<String> = None;
        let mut anchor_span: Option<Span> = None;
        let mut tag: Option<String> = None;

        loop {
            match self.class_at(0)? {
                Some(Class::Anchor) => {
                    let token = self.pop()?.ok_or_else(|| self.eof_error())?;
                    if let TokenKind::Anchor(name) = token.kind {
                        if anchor.is_none() {
                            anchor = Some(name);
                            anchor_span = Some(token.span);
                        }
                    }
                    self.skip_trivia()?;
                }
                Some(Class::Tag) => {
                    let token = self.pop()?.ok_or_else(|| self.eof_error())?;
                    if let TokenKind::Tag(name) = token.kind {
                        if tag.is_none() {
                            tag = Some(name);
                        }
                    }
                    self.skip_trivia()?;
                }
                Some(Class::Scalar) => {
                    let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
                    let TokenKind::Scalar { raw, style, .. } = &token.kind else {
                        unreachable!("class checked above")
                    };
                    self.reject_merge_key(raw, token.span)?;
                    let value = self.resolve(raw, *style, token.span)?;
                    self.pop()?;
                    if let (Some(name), Some(span)) = (&anchor, anchor_span) {
                        self.register_anchors(alloc::vec![(name.clone(), span)])?;
                    }
                    self.outbox.push_back(Event::Scalar(ScalarEvent {
                        value,
                        style: convert_style(*style),
                        raw: raw.clone(),
                        anchor,
                        tag,
                        span: token.span,
                    }));
                    return Ok(());
                }
                _ => {
                    let token = self.token_at(0)?.ok_or_else(|| self.eof_error())?;
                    return Err(self.error_at_token(
                        &token,
                        ErrorKind::UnexpectedCharacter,
                        "invalid key in flow mapping",
                    ));
                }
            }
        }
    }

    /// Emits a bare (non-`?`-prefixed) block mapping key scalar.
    fn emit_key_scalar(&mut self, token: &Token) -> Result<(), YamlError> {
        let TokenKind::Scalar { raw, style, .. } = &token.kind else {
            return Ok(());
        };
        self.reject_merge_key(raw, token.span)?;
        let value = self.resolve(raw, *style, token.span)?;
        self.outbox.push_back(Event::Scalar(ScalarEvent {
            value,
            style: convert_style(*style),
            raw: raw.clone(),
            anchor: None,
            tag: None,
            span: token.span,
        }));
        Ok(())
    }
}

/// Implicit scalar resolution, merge-key policy, and diagnostics.
impl EventParser<'_> {
    /// Implicit tag resolution, matching the arena parser: only plain scalars type-resolve, and
    /// quoted/block scalars are always strings.
    fn resolve(
        &self,
        raw: &str,
        style: LexScalarStyle,
        span: Span,
    ) -> Result<ScalarValue, YamlError> {
        if style != LexScalarStyle::Plain {
            return Ok(ScalarValue::String(String::from(raw)));
        }
        if self.strict_version && !self.version_pinned {
            self.check_version_ambiguity(raw, span)?;
        }
        Ok(crate::resolve::resolve_scalar(raw, self.version))
    }

    /// See `crate::parser::Parser::check_version_ambiguity` — same check, same rationale,
    /// duplicated here because this parser tracks its own independent version/pinning state.
    fn check_version_ambiguity(&self, raw: &str, span: Span) -> Result<(), YamlError> {
        let under_1_1 = crate::resolve::resolve_scalar(raw, YamlVersion::Version11);
        let under_1_2 = crate::resolve::resolve_scalar(raw, YamlVersion::Version12);
        if under_1_1 != under_1_2 {
            return Err(YamlError::new(
                span.start,
                ErrorKind::AmbiguousVersion,
                "resolves differently under YAML 1.1 vs 1.2; pin a version with a '%YAML' \
                 directive or ParserOptions::yaml_version, or quote the scalar to force it to a \
                 string",
                self.source,
            ));
        }
        Ok(())
    }

    /// Merge keys need the anchored mapping's entries replayed, which a single-pass event stream
    /// cannot do, so they are refused rather than silently diverging from the arena parser.
    fn reject_merge_key(&self, raw: &str, span: Span) -> Result<(), YamlError> {
        if self.merge_keys && raw == "<<" {
            return Err(YamlError::new(
                span.start,
                ErrorKind::InvalidMergeKey,
                "merge keys ('<<') are not supported by the streaming parser; set \
                 ParserOptions::merge_keys to false to treat '<<' as a plain key",
                self.source,
            ));
        }
        Ok(())
    }

    fn eof_error(&self) -> YamlError {
        YamlError::new(
            self.source.len(),
            ErrorKind::UnexpectedEof,
            "unexpected end of input",
            self.source,
        )
    }

    fn error(&mut self, kind: ErrorKind, message: &str) -> YamlError {
        let offset = self.span_at(0).ok().flatten().map_or(self.source.len(), |span| span.start);
        YamlError::new(offset, kind, message, self.source)
    }

    fn error_at_token(&self, token: &Token, kind: ErrorKind, message: &str) -> YamlError {
        YamlError::new(token.span.start, kind, message, self.source)
    }
}

impl Iterator for EventParser<'_> {
    type Item = Result<Event, YamlError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_event() {
            Ok(Some(event)) => Some(Ok(event)),
            Ok(None) => None,
            Err(error) => {
                self.finish();
                Some(Err(error))
            }
        }
    }
}

/// Starts an event stream over `source` using [`ParserOptions::default`].
pub fn events(source: &str) -> EventParser<'_> {
    EventParser::new(source, &ParserOptions::default())
}

/// Starts an event stream over `source` with explicit parser options.
pub fn events_with_options<'a>(source: &'a str, options: &ParserOptions) -> EventParser<'a> {
    EventParser::new(source, options)
}

/// Parses `source` into a complete event list. Convenience for tests and small inputs.
pub fn collect_events(source: &str) -> Result<Vec<Event>, YamlError> {
    events(source).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParserOptions;

    fn scalar(events: &[Event], index: usize) -> &ScalarEvent {
        match &events[index] {
            Event::Scalar(scalar) => scalar,
            other => panic!("expected a scalar event at index {index}, got {other:?}"),
        }
    }

    #[test]
    fn emits_a_flat_mapping() {
        let events = collect_events("a: 1\nb: two\n").unwrap();
        assert_eq!(events.first(), Some(&Event::StreamStart));
        assert_eq!(events.get(1), Some(&Event::DocumentStart));
        assert!(matches!(events.get(2), Some(Event::MappingStart(_))));
        assert!(matches!(events.get(7), Some(Event::MappingEnd)));
        assert_eq!(events.get(8), Some(&Event::DocumentEnd));
        assert_eq!(events.get(9), Some(&Event::StreamEnd));
        assert_eq!(events.len(), 10);
        assert_eq!(scalar(&events, 3).value, ScalarValue::String(String::from("a")));
        assert_eq!(scalar(&events, 4).value, ScalarValue::Int(1));
        assert_eq!(scalar(&events, 5).value, ScalarValue::String(String::from("b")));
        assert_eq!(scalar(&events, 6).value, ScalarValue::String(String::from("two")));
    }

    #[test]
    fn emits_nested_sequences_and_flow_collections() {
        let events = collect_events("items: [1, {x: 2}]\n").unwrap();
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match e {
                Event::StreamStart => "StreamStart",
                Event::StreamEnd => "StreamEnd",
                Event::DocumentStart => "DocumentStart",
                Event::DocumentEnd => "DocumentEnd",
                Event::MappingStart(_) => "MappingStart",
                Event::MappingEnd => "MappingEnd",
                Event::SequenceStart(_) => "SequenceStart",
                Event::SequenceEnd => "SequenceEnd",
                Event::Scalar(_) => "Scalar",
                Event::Alias(_) => "Alias",
            })
            .collect();
        assert_eq!(
            kinds,
            alloc::vec![
                "StreamStart",
                "DocumentStart",
                "MappingStart", // outer mapping
                "Scalar",       // "items"
                "SequenceStart",
                "Scalar", // 1
                "MappingStart",
                "Scalar", // x
                "Scalar", // 2
                "MappingEnd",
                "SequenceEnd",
                "MappingEnd",
                "DocumentEnd",
                "StreamEnd",
            ]
        );
    }

    #[test]
    fn emits_anchors_and_aliases() {
        let events = collect_events("a: &x 1\nb: *x\n").unwrap();
        let anchor_event = events.iter().find_map(|e| match e {
            Event::Scalar(scalar) if scalar.anchor.is_some() => Some(scalar),
            _ => None,
        });
        assert_eq!(anchor_event.unwrap().anchor.as_deref(), Some("x"));
        let alias = events.iter().find_map(|e| match e {
            Event::Alias(alias) => Some(alias),
            _ => None,
        });
        assert_eq!(alias.unwrap().name, "x");
    }

    #[test]
    fn rejects_an_unknown_alias() {
        let err = collect_events("a: *missing\n").unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnknownAnchor);
    }

    #[test]
    fn rejects_a_duplicate_anchor() {
        let err = collect_events("a: &x 1\nb: &x 2\n").unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidAnchor);
    }

    #[test]
    fn rejects_merge_keys_by_default() {
        let err = collect_events("base: &b\n  a: 1\nchild:\n  <<: *b\n  b: 2\n").unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidMergeKey);
    }

    #[test]
    fn treats_merge_key_as_a_plain_key_when_disabled() {
        let options = ParserOptions { merge_keys: false, ..ParserOptions::default() };
        let result: Result<Vec<Event>, YamlError> =
            events_with_options("child:\n  <<: *b\n  b: 2\n", &options).collect();
        // `*b` is still an unknown alias (never anchored here), so this only exercises that
        // `<<` itself is no longer rejected outright before that alias lookup fails.
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnknownAnchor);
    }

    #[test]
    fn strict_version_rejects_a_1_1_vs_1_2_ambiguous_scalar() {
        let options = ParserOptions { strict_version: true, ..ParserOptions::default() };
        // Bare-octal `0755` is Int(493) under 1.1 but String("0755") under 1.2 (1.2 requires the
        // `0o` prefix for octal).
        let err = collect_events_with_options("a: 0755\n", &options).unwrap_err();
        assert_eq!(err.kind, ErrorKind::AmbiguousVersion);
    }

    #[test]
    fn strict_version_accepts_an_unambiguous_scalar() {
        let options = ParserOptions { strict_version: true, ..ParserOptions::default() };
        let events = collect_events_with_options("a: true\nb: 42\n", &options).unwrap();
        assert!(!events.is_empty());
    }

    #[test]
    fn strict_version_is_inert_when_a_version_is_pinned() {
        let options = ParserOptions {
            strict_version: true,
            yaml_version: Some(YamlVersion::Version11),
            ..ParserOptions::default()
        };
        let events = collect_events_with_options("a: 0755\n", &options).unwrap();
        assert!(!events.is_empty());
    }

    #[test]
    fn strict_version_is_inert_when_a_yaml_directive_pins_the_version() {
        let options = ParserOptions { strict_version: true, ..ParserOptions::default() };
        let events = collect_events_with_options("%YAML 1.1\n---\na: 0755\n", &options).unwrap();
        assert!(!events.is_empty());
    }

    fn collect_events_with_options(
        source: &str,
        options: &ParserOptions,
    ) -> Result<Vec<Event>, YamlError> {
        events_with_options(source, options).collect()
    }

    #[test]
    fn emits_every_document_in_a_multi_doc_stream() {
        let events = collect_events("a\n---\nb\n---\nc\n").unwrap();
        let doc_starts = events.iter().filter(|e| **e == Event::DocumentStart).count();
        let doc_ends = events.iter().filter(|e| **e == Event::DocumentEnd).count();
        assert_eq!(doc_starts, 3);
        assert_eq!(doc_ends, 3);
        assert_eq!(events.first(), Some(&Event::StreamStart));
        assert_eq!(events.last(), Some(&Event::StreamEnd));
    }

    #[test]
    fn empty_input_yields_a_single_null_document() {
        let events = collect_events("").unwrap();
        assert_eq!(events[..2], [Event::StreamStart, Event::DocumentStart][..]);
        assert_eq!(events[3..], [Event::DocumentEnd, Event::StreamEnd][..]);
        assert_eq!(events.len(), 5);
        assert_eq!(scalar(&events, 2).value, ScalarValue::Null);
    }

    #[test]
    fn next_event_is_resumable_across_calls() {
        let mut parser = EventParser::new("a: 1\n", &ParserOptions::default());
        let mut collected = Vec::new();
        while let Some(event) = parser.next_event().unwrap() {
            collected.push(event);
        }
        assert_eq!(collected.len(), collect_events("a: 1\n").unwrap().len());
        // The parser is exhausted; further calls keep returning `None`, not panicking or erroring.
        assert_eq!(parser.next_event().unwrap(), None);
        assert_eq!(parser.next_event().unwrap(), None);
    }

    #[test]
    fn finish_drops_the_remainder_of_the_stream() {
        let mut parser = EventParser::new("a: 1\nb: 2\n", &ParserOptions::default());
        assert_eq!(parser.next_event().unwrap(), Some(Event::StreamStart));
        parser.finish();
        assert_eq!(parser.next_event().unwrap(), None);
    }

    #[test]
    fn agrees_with_the_arena_parser_on_multi_doc_streams() {
        let source = "a: 1\n---\nb: 2\n";
        let arena = crate::parse(source).unwrap();
        let event_docs =
            collect_events(source).unwrap().iter().filter(|e| **e == Event::DocumentStart).count();
        assert_eq!(arena.documents.len(), event_docs);
    }
}
