use crate::{
    Document, ErrorKind, NodeData, NodeId, NodeKind, Scalar, ScalarStyle, ScalarValue, Span,
    Trivia, TriviaKind, YamlError, YamlVersion,
};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Options controlling how a YAML source is parsed.
#[derive(Clone, Debug)]
pub struct ParserOptions {
    /// Force resolution under a specific YAML version; `None` means "auto" (defaults to 1.2,
    /// overridden by a `%YAML` directive when present).
    pub yaml_version: Option<YamlVersion>,
    /// Whether `<<` merge keys are expanded in mappings. Defaults to `true` regardless of
    /// `yaml_version`, matching common tooling rather than strict 1.2 semantics.
    pub merge_keys: bool,
    /// When `true` and `yaml_version` is `None` (no explicit pin, and no `%YAML` directive in
    /// the source), refuses any plain scalar that resolves to a different value under YAML 1.1
    /// than under 1.2 with an `ErrorKind::AmbiguousVersion` error instead of silently picking
    /// one interpretation (`current_doc_version`, which defaults to 1.2). Has no effect when a
    /// version is pinned, since there's no ambiguity to detect. Defaults to `false`.
    pub strict_version: bool,
}

impl Default for ParserOptions {
    fn default() -> Self {
        Self { yaml_version: None, merge_keys: true, strict_version: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ParserToken {
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
    Scalar { raw: String, style: ScalarStyle, tag: Option<String> },
    BlockScalar { indicator: BlockIndicator, value: String },
    Comment(String),
    BlankLine,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockIndicator {
    Literal,
    Folded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TokenWithSpan {
    token: ParserToken,
    span: Span,
}

struct Parser<'a> {
    tokens: Vec<TokenWithSpan>,
    pos: usize,
    source: &'a str,
    document: Document,
    anchors: BTreeMap<String, NodeId>,
    current_doc_version: YamlVersion,
    version_explicit: bool,
    /// Whether `current_doc_version` was pinned, either by `ParserOptions.yaml_version` or by a
    /// `%YAML` directive — as opposed to just defaulting to 1.2. Used by `strict_version` to
    /// know when there's no ambiguity to detect.
    version_pinned: bool,
    merge_keys: bool,
    strict_version: bool,
    pending_trivia: Vec<Trivia>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: Vec<TokenWithSpan>, options: &ParserOptions) -> Self {
        let version = options.yaml_version.unwrap_or(YamlVersion::Version12);
        let mut document = Document::new(version);
        document.nodes.reserve(1024);
        Self {
            tokens,
            pos: 0,
            source,
            document,
            anchors: BTreeMap::new(),
            current_doc_version: version,
            version_explicit: options.yaml_version.is_some(),
            version_pinned: options.yaml_version.is_some(),
            merge_keys: options.merge_keys,
            strict_version: options.strict_version,
            pending_trivia: Vec::new(),
        }
    }

    fn parse(mut self) -> Result<Document, YamlError> {
        self.skip_trivia();
        self.consume_directives()?;
        while !self.at_eof() {
            let doc_start_span = self.maybe_consume_document_start();
            self.skip_trivia();
            let root = self.parse_document(doc_start_span)?;
            self.document.documents.push(root);
            self.skip_trivia();
            self.consume_document_end()?;
            self.skip_trivia();
            self.consume_directives()?;
            if !self.at_eof() && !self.at_document_start() {
                return Err(
                    self.error(ErrorKind::TrailingContent, "expected '---' before next document")
                );
            }
        }
        if self.document.documents.is_empty() {
            let root = self.add_null_node(Span::new(0, 0, 1, 1));
            self.document.documents.push(root);
        }
        Ok(self.document)
    }

    fn current(&self) -> Option<&TokenWithSpan> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<TokenWithSpan> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn at_eof(&self) -> bool {
        matches!(self.current(), Some(t) if matches!(t.token, ParserToken::Eof))
            || self.pos >= self.tokens.len()
    }

    fn at_document_start(&self) -> bool {
        matches!(self.current(), Some(t) if matches!(t.token, ParserToken::DocumentStart))
    }

    fn maybe_consume_document_start(&mut self) -> Option<Span> {
        if let Some(token) = self.current() {
            if let ParserToken::DocumentStart = &token.token {
                let span = token.span;
                self.advance();
                return Some(span);
            }
        }
        None
    }

    fn consume_document_end(&mut self) -> Result<Option<Span>, YamlError> {
        if let Some(token) = self.current() {
            if let ParserToken::DocumentEnd = &token.token {
                let span = token.span;
                self.advance();
                return Ok(Some(span));
            }
        }
        Ok(None)
    }

    fn consume_directives(&mut self) -> Result<(), YamlError> {
        loop {
            self.skip_trivia();
            let Some(token) = self.current().cloned() else {
                break;
            };
            let ParserToken::Directive(text) = &token.token else {
                break;
            };
            self.apply_directive(text, token.span)?;
            self.advance();
        }
        Ok(())
    }

    fn apply_directive(&mut self, text: &str, span: Span) -> Result<(), YamlError> {
        let mut parts = text.split_whitespace();
        if parts.next() == Some("YAML") {
            if let Some(ver_str) = parts.next() {
                match ver_str.parse::<YamlVersion>() {
                    Ok(version) => {
                        if !self.version_explicit {
                            self.current_doc_version = version;
                            self.document.version = version;
                            self.version_pinned = true;
                        }
                    }
                    Err(()) => {
                        return Err(self.error_at_offset(
                            span.start,
                            ErrorKind::InvalidDirective,
                            "unrecognized %YAML version",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_document(&mut self, doc_start_span: Option<Span>) -> Result<NodeId, YamlError> {
        self.skip_trivia();
        if self.at_eof()
            || matches!(self.current(), Some(t) if matches!(t.token, ParserToken::DocumentEnd))
        {
            let span = doc_start_span
                .or_else(|| self.current().map(|t| t.span))
                .unwrap_or_else(|| Span::new(0, 0, 1, 1));
            return Ok(self.add_null_node(span));
        }
        self.parse_value()
    }

    /// Whether a value follows a mapping key positioned at `key_col`/`key_line`. Sequences are
    /// allowed to align at the same column as the key (a common YAML block-sequence exception);
    /// everything else must be indented strictly further, unless it's inline on the key's line.
    fn value_follows_key(&self, key_col: usize, key_line: usize) -> bool {
        match self.current() {
            None => false,
            Some(t) => {
                if t.span.line == key_line {
                    return !matches!(
                        t.token,
                        ParserToken::DocumentEnd | ParserToken::DocumentStart
                    );
                }
                match &t.token {
                    ParserToken::DocumentStart | ParserToken::DocumentEnd => false,
                    ParserToken::SequenceEntry => t.span.column >= key_col,
                    _ => t.span.column > key_col,
                }
            }
        }
    }

    /// Whether a value follows a sequence dash positioned at `dash_col`/`dash_line`.
    fn value_follows_dash(&self, dash_col: usize, dash_line: usize) -> bool {
        match self.current() {
            None => false,
            Some(t) => {
                if t.span.line == dash_line {
                    return !matches!(
                        t.token,
                        ParserToken::Comma | ParserToken::RightBrace | ParserToken::RightBracket
                    );
                }
                match &t.token {
                    ParserToken::DocumentStart | ParserToken::DocumentEnd => false,
                    _ => t.span.column > dash_col,
                }
            }
        }
    }

    fn peek_is_colon(&self) -> bool {
        self.tokens
            .get(self.pos + 1)
            .map(|t| matches!(t.token, ParserToken::Colon))
            .unwrap_or(false)
    }

    fn parse_value(&mut self) -> Result<NodeId, YamlError> {
        self.skip_trivia();
        let token = self
            .current()
            .cloned()
            .ok_or_else(|| self.error(ErrorKind::UnexpectedEof, "expected value"))?;

        match &token.token {
            ParserToken::Scalar { raw, style, tag } if self.peek_is_colon() => {
                let _ = (raw, style, tag);
                self.parse_block_mapping()
            }
            ParserToken::Scalar { raw, style, tag } => {
                let value = self.resolve_scalar_value(raw, *style, tag.as_deref(), token.span)?;
                let node = self.document.add_node(NodeData::new(
                    NodeId(0),
                    NodeKind::Scalar(Scalar {
                        value,
                        style: *style,
                        raw: raw.clone(),
                        tag: tag.clone(),
                    }),
                    Some(token.span),
                ));
                self.attach_trivia(node);
                self.advance();
                Ok(node)
            }
            ParserToken::BlockScalar { indicator, value } => {
                let style = match indicator {
                    BlockIndicator::Literal => ScalarStyle::BlockLiteral,
                    BlockIndicator::Folded => ScalarStyle::BlockChomped,
                };
                let scalar = Scalar {
                    value: ScalarValue::String(value.clone()),
                    style,
                    raw: value.clone(),
                    tag: None,
                };
                let node = self.document.add_node(NodeData::new(
                    NodeId(0),
                    NodeKind::Scalar(scalar),
                    Some(token.span),
                ));
                self.attach_trivia(node);
                self.advance();
                Ok(node)
            }
            ParserToken::Anchor(name) => {
                let span = token.span;
                let name = name.clone();
                self.advance();
                self.skip_trivia();
                let value_node = self.parse_value()?;
                if self.anchors.insert(name.clone(), value_node).is_some() {
                    return Err(self.error_at_offset(
                        span.start,
                        ErrorKind::InvalidAnchor,
                        "duplicate anchor",
                    ));
                }
                if let Some(node) = self.document.node_mut(value_node) {
                    node.anchor = Some(name);
                }
                Ok(value_node)
            }
            ParserToken::Alias(name) => {
                let target = *self.anchors.get(name).ok_or_else(|| {
                    self.error_at_token(&token, ErrorKind::UnknownAnchor, "unknown anchor")
                })?;
                let node = self.document.add_node(NodeData::new(
                    NodeId(0),
                    NodeKind::Alias(target),
                    Some(token.span),
                ));
                self.attach_trivia(node);
                self.advance();
                Ok(node)
            }
            ParserToken::Tag(tag) => {
                let tag = tag.clone();
                self.advance();
                self.skip_trivia();
                let value_node = self.parse_value()?;
                if let Some(node) = self.document.node_mut(value_node) {
                    node.tag = Some(tag);
                }
                Ok(value_node)
            }
            ParserToken::LeftBrace => self.parse_flow_mapping(),
            ParserToken::LeftBracket => self.parse_flow_sequence(),
            ParserToken::SequenceEntry => self.parse_block_sequence(),
            ParserToken::MappingKey => self.parse_block_mapping(),
            _ => Err(self.error_at_token(
                &token,
                ErrorKind::UnexpectedCharacter,
                "unexpected token in value position",
            )),
        }
    }

    fn parse_block_sequence(&mut self) -> Result<NodeId, YamlError> {
        let mut items = Vec::new();
        let mut first_span = None;
        let mut last_end = 0usize;
        let mut item_col: Option<usize> = None;

        loop {
            self.skip_trivia();
            let Some(token) = self.current().cloned() else { break };
            if !matches!(token.token, ParserToken::SequenceEntry) {
                break;
            }
            match item_col {
                Some(ic) if token.span.column != ic => break,
                None => item_col = Some(token.span.column),
                _ => {}
            }

            let entry_token = self.advance().unwrap();
            if first_span.is_none() {
                first_span = Some(entry_token.span);
            }
            last_end = last_end.max(entry_token.span.end);
            let item = if self.value_follows_dash(entry_token.span.column, entry_token.span.line) {
                self.parse_value()?
            } else {
                self.add_null_node(entry_token.span)
            };
            if let Some(s) = self.document.span(item) {
                last_end = last_end.max(s.end);
            }
            items.push(item);
        }

        let span = first_span
            .map(|s| Span::new(s.start, last_end.max(s.end), s.line, s.column))
            .unwrap_or_else(|| Span::new(0, 0, 1, 1));
        let node =
            self.document.add_node(NodeData::new(NodeId(0), NodeKind::Sequence(items), Some(span)));
        self.attach_trivia(node);
        Ok(node)
    }

    fn parse_block_mapping(&mut self) -> Result<NodeId, YamlError> {
        let mut entries = Vec::new();
        let mut first_span = None;
        let mut last_end = 0usize;
        let mut entry_col: Option<usize> = None;

        loop {
            self.skip_trivia();
            let Some(token) = self.current().cloned() else { break };
            let is_key_start = matches!(token.token, ParserToken::MappingKey)
                || matches!(token.token, ParserToken::Scalar { .. });
            if !is_key_start {
                break;
            }
            match entry_col {
                Some(ec) if token.span.column != ec => break,
                None => entry_col = Some(token.span.column),
                _ => {}
            }
            if first_span.is_none() {
                first_span = Some(token.span);
            }

            let key = match &token.token {
                ParserToken::MappingKey => {
                    self.advance();
                    self.skip_trivia();
                    self.parse_value()?
                }
                ParserToken::Scalar { raw, style, tag } => {
                    let value =
                        self.resolve_scalar_value(raw, *style, tag.as_deref(), token.span)?;
                    let key = self.document.add_node(NodeData::new(
                        NodeId(0),
                        NodeKind::Scalar(Scalar {
                            value,
                            style: *style,
                            raw: raw.clone(),
                            tag: tag.clone(),
                        }),
                        Some(token.span),
                    ));
                    self.advance();
                    key
                }
                _ => unreachable!("guarded by is_key_start"),
            };

            self.skip_trivia();
            let colon_token = self.current().cloned().ok_or_else(|| {
                self.error(ErrorKind::UnexpectedEof, "expected colon after mapping key")
            })?;
            if !matches!(colon_token.token, ParserToken::Colon) {
                return Err(self.error_at_token(
                    &colon_token,
                    ErrorKind::UnexpectedCharacter,
                    "expected colon after mapping key",
                ));
            }
            self.advance();
            self.skip_trivia();

            let key_col = token.span.column;
            let key_line = token.span.line;
            let value = if self.value_follows_key(key_col, key_line) {
                self.parse_value()?
            } else {
                self.add_null_node(colon_token.span)
            };
            if let Some(s) = self.document.span(key) {
                last_end = last_end.max(s.end);
            }
            if let Some(s) = self.document.span(value) {
                last_end = last_end.max(s.end);
            }
            entries.push((key, value));
        }

        let entries = self.apply_merge_keys(entries)?;
        let span = first_span
            .map(|s| Span::new(s.start, last_end.max(s.end), s.line, s.column))
            .unwrap_or_else(|| Span::new(0, 0, 1, 1));
        let node = self.document.add_node(NodeData::new(
            NodeId(0),
            NodeKind::Mapping(entries),
            Some(span),
        ));
        self.attach_trivia(node);
        Ok(node)
    }

    fn parse_flow_sequence(&mut self) -> Result<NodeId, YamlError> {
        let start_token = self.advance().unwrap();
        let mut items = Vec::new();
        let mut end = start_token.span.end;

        self.skip_trivia();
        while let Some(token) = self.current().cloned() {
            if matches!(token.token, ParserToken::RightBracket) {
                end = token.span.end;
                self.advance();
                break;
            }
            if matches!(token.token, ParserToken::Comma) {
                self.advance();
                self.skip_trivia();
                continue;
            }
            let item = self.parse_value()?;
            if let Some(s) = self.document.span(item) {
                end = end.max(s.end);
            }
            items.push(item);
            self.skip_trivia();
            if let Some(t) = self.current() {
                if matches!(t.token, ParserToken::Comma | ParserToken::RightBracket) {
                    continue;
                }
                return Err(self.error_at_token(
                    t,
                    ErrorKind::UnexpectedCharacter,
                    "expected comma or ] in flow sequence",
                ));
            }
        }

        let span =
            Span::new(start_token.span.start, end, start_token.span.line, start_token.span.column);
        let node =
            self.document.add_node(NodeData::new(NodeId(0), NodeKind::Sequence(items), Some(span)));
        self.attach_trivia(node);
        Ok(node)
    }

    fn parse_flow_mapping(&mut self) -> Result<NodeId, YamlError> {
        let start_token = self.advance().unwrap();
        let mut entries = Vec::new();
        let mut end = start_token.span.end;

        self.skip_trivia();
        while let Some(token) = self.current().cloned() {
            if matches!(token.token, ParserToken::RightBrace) {
                end = token.span.end;
                self.advance();
                break;
            }
            if matches!(token.token, ParserToken::Comma) {
                self.advance();
                self.skip_trivia();
                continue;
            }

            let key = self.parse_flow_key()?;
            self.skip_trivia();
            let colon_token = self.current().cloned().ok_or_else(|| {
                self.error(ErrorKind::UnexpectedEof, "expected colon after mapping key")
            })?;
            if !matches!(colon_token.token, ParserToken::Colon) {
                return Err(self.error_at_token(
                    &colon_token,
                    ErrorKind::UnexpectedCharacter,
                    "expected colon after mapping key",
                ));
            }
            self.advance();
            self.skip_trivia();
            let value = match self.current() {
                Some(t) if matches!(t.token, ParserToken::Comma | ParserToken::RightBrace) => {
                    self.add_null_node(colon_token.span)
                }
                Some(_) => self.parse_value()?,
                None => self.add_null_node(colon_token.span),
            };
            if let Some(s) = self.document.span(key) {
                end = end.max(s.end);
            }
            if let Some(s) = self.document.span(value) {
                end = end.max(s.end);
            }
            entries.push((key, value));
            self.skip_trivia();
            if let Some(t) = self.current() {
                if matches!(t.token, ParserToken::Comma | ParserToken::RightBrace) {
                    continue;
                }
                return Err(self.error_at_token(
                    t,
                    ErrorKind::UnexpectedCharacter,
                    "expected comma or } in flow mapping",
                ));
            }
        }

        let entries = self.apply_merge_keys(entries)?;
        let span =
            Span::new(start_token.span.start, end, start_token.span.line, start_token.span.column);
        let node = self.document.add_node(NodeData::new(
            NodeId(0),
            NodeKind::Mapping(entries),
            Some(span),
        ));
        self.attach_trivia(node);
        Ok(node)
    }

    fn parse_flow_key(&mut self) -> Result<NodeId, YamlError> {
        self.skip_trivia();
        let token = self
            .current()
            .cloned()
            .ok_or_else(|| self.error(ErrorKind::UnexpectedEof, "expected key in flow mapping"))?;

        match &token.token {
            ParserToken::Scalar { raw, style, tag } => {
                let value = self.resolve_scalar_value(raw, *style, tag.as_deref(), token.span)?;
                let node = self.document.add_node(NodeData::new(
                    NodeId(0),
                    NodeKind::Scalar(Scalar {
                        value,
                        style: *style,
                        raw: raw.clone(),
                        tag: tag.clone(),
                    }),
                    Some(token.span),
                ));
                self.advance();
                Ok(node)
            }
            ParserToken::Anchor(name) => {
                let name = name.clone();
                let span = token.span;
                self.advance();
                self.skip_trivia();
                let key = self.parse_flow_key()?;
                if self.anchors.insert(name.clone(), key).is_some() {
                    return Err(self.error_at_offset(
                        span.start,
                        ErrorKind::InvalidAnchor,
                        "duplicate anchor",
                    ));
                }
                if let Some(node) = self.document.node_mut(key) {
                    node.anchor = Some(name);
                }
                Ok(key)
            }
            ParserToken::Tag(tag) => {
                let tag = tag.clone();
                self.advance();
                self.skip_trivia();
                let key = self.parse_flow_key()?;
                if let Some(node) = self.document.node_mut(key) {
                    node.tag = Some(tag);
                }
                Ok(key)
            }
            _ => Err(self.error_at_token(
                &token,
                ErrorKind::UnexpectedCharacter,
                "invalid key in flow mapping",
            )),
        }
    }

    /// Implicit tag resolution only applies to plain scalars; quoted and block scalars are
    /// always strings unless an explicit tag overrides them.
    fn resolve_scalar_value(
        &self,
        raw: &str,
        style: ScalarStyle,
        tag: Option<&str>,
        span: Span,
    ) -> Result<ScalarValue, YamlError> {
        if tag == Some("!str") {
            return Ok(ScalarValue::String(raw.to_string()));
        }
        if style != ScalarStyle::Plain {
            return Ok(ScalarValue::String(raw.to_string()));
        }
        if self.strict_version && !self.version_pinned {
            self.check_version_ambiguity(raw, span)?;
        }
        Ok(crate::resolve::resolve_scalar(raw, self.current_doc_version))
    }

    /// With `ParserOptions.strict_version` set and no explicit `yaml_version`/`%YAML` pin, a
    /// plain scalar that resolves to a different value under YAML 1.1 than under 1.2 (the
    /// "Norway problem" family: `yes`/`no`/`on`/`off` booleans, bare-octal `0755`, sexagesimal
    /// `1:20:30`, ...) is refused rather than silently picking one interpretation.
    fn check_version_ambiguity(&self, raw: &str, span: Span) -> Result<(), YamlError> {
        let under_1_1 = crate::resolve::resolve_scalar(raw, YamlVersion::Version11);
        let under_1_2 = crate::resolve::resolve_scalar(raw, YamlVersion::Version12);
        if under_1_1 != under_1_2 {
            return Err(self.error_at_offset(
                span.start,
                ErrorKind::AmbiguousVersion,
                "resolves differently under YAML 1.1 vs 1.2; pin a version with a '%YAML' \
                 directive or ParserOptions::yaml_version, or quote the scalar to force it to a \
                 string",
            ));
        }
        Ok(())
    }

    fn is_merge_key(&self, id: NodeId) -> bool {
        matches!(self.document.node(id).map(|n| &n.kind), Some(NodeKind::Scalar(s)) if s.raw == "<<")
    }

    fn key_repr(&self, id: NodeId) -> Option<String> {
        match self.document.node(id).map(|n| &n.kind) {
            Some(NodeKind::Scalar(scalar)) => Some(scalar.raw.clone()),
            _ => None,
        }
    }

    /// Follows an alias chain to its final non-alias target. Structurally cycle-free: an anchor
    /// is only registered once its value has been fully parsed, so an alias can never (even
    /// transitively) point back at the node currently being built.
    fn deref_alias(&self, mut id: NodeId) -> NodeId {
        let mut seen = Vec::new();
        while let Some(node) = self.document.node(id) {
            if let NodeKind::Alias(target) = node.kind {
                if seen.contains(&id) {
                    break;
                }
                seen.push(id);
                id = target;
            } else {
                break;
            }
        }
        id
    }

    fn merge_sources(&self, value: NodeId) -> Result<Vec<(NodeId, NodeId)>, YamlError> {
        let offset = self.document.span(value).map(|s| s.start).unwrap_or(0);
        let resolved = self.deref_alias(value);
        match self.document.node(resolved).map(|n| &n.kind) {
            Some(NodeKind::Mapping(entries)) => Ok(entries.clone()),
            Some(NodeKind::Sequence(items)) => {
                let mut out = Vec::new();
                for item in items {
                    let item_resolved = self.deref_alias(*item);
                    match self.document.node(item_resolved).map(|n| &n.kind) {
                        Some(NodeKind::Mapping(entries)) => out.extend(entries.clone()),
                        _ => {
                            return Err(self.error_at_offset(
                                offset,
                                ErrorKind::InvalidMergeKey,
                                "merge value sequence must contain only mappings",
                            ))
                        }
                    }
                }
                Ok(out)
            }
            _ => Err(self.error_at_offset(
                offset,
                ErrorKind::InvalidMergeKey,
                "merge key value must be a mapping or sequence of mappings",
            )),
        }
    }

    /// Expands `<<` merge-key entries: explicit keys always win, and among multiple merge
    /// sources an earlier one wins over a later one.
    fn apply_merge_keys(
        &self,
        entries: Vec<(NodeId, NodeId)>,
    ) -> Result<Vec<(NodeId, NodeId)>, YamlError> {
        if !self.merge_keys {
            return Ok(entries);
        }
        let mut explicit = Vec::new();
        let mut merge_values = Vec::new();
        for (k, v) in entries {
            if self.is_merge_key(k) {
                merge_values.push(v);
            } else {
                explicit.push((k, v));
            }
        }
        if merge_values.is_empty() {
            return Ok(explicit);
        }
        let mut seen_keys: Vec<String> =
            explicit.iter().filter_map(|(k, _)| self.key_repr(*k)).collect();
        let mut merged = Vec::new();
        for mv in merge_values {
            for (k, v) in self.merge_sources(mv)? {
                if let Some(repr) = self.key_repr(k) {
                    if seen_keys.contains(&repr) {
                        continue;
                    }
                    seen_keys.push(repr);
                }
                merged.push((k, v));
            }
        }
        explicit.extend(merged);
        Ok(explicit)
    }

    fn add_null_node(&mut self, span: Span) -> NodeId {
        self.document.add_node(NodeData::new(
            NodeId(0),
            NodeKind::Scalar(Scalar {
                value: ScalarValue::Null,
                style: ScalarStyle::Plain,
                raw: String::new(),
                tag: None,
            }),
            Some(span),
        ))
    }

    fn attach_trivia(&mut self, node_id: NodeId) {
        if let Some(node) = self.document.node_mut(node_id) {
            node.trivia.append(&mut self.pending_trivia);
        }
    }

    fn skip_trivia(&mut self) {
        while let Some(token) = self.current() {
            match &token.token {
                ParserToken::Comment(text) => {
                    self.pending_trivia
                        .push(Trivia { kind: TriviaKind::Comment(text.clone()), span: token.span });
                    self.advance();
                }
                ParserToken::BlankLine => {
                    self.pending_trivia
                        .push(Trivia { kind: TriviaKind::BlankLine, span: token.span });
                    self.advance();
                }
                _ => break,
            }
        }
    }

    fn error(&self, kind: ErrorKind, message: &str) -> YamlError {
        let offset = self.current().map(|t| t.span.start).unwrap_or(self.source.len());
        YamlError::new(offset, kind, message, self.source)
    }

    fn error_at_offset(&self, offset: usize, kind: ErrorKind, message: &str) -> YamlError {
        YamlError::new(offset, kind, message, self.source)
    }

    fn error_at_token(&self, token: &TokenWithSpan, kind: ErrorKind, message: &str) -> YamlError {
        YamlError::new(token.span.start, kind, message, self.source)
    }
}

fn convert_scalar_style(style: crate::lexer::ScalarStyle) -> ScalarStyle {
    match style {
        crate::lexer::ScalarStyle::Plain => ScalarStyle::Plain,
        crate::lexer::ScalarStyle::SingleQuoted => ScalarStyle::SingleQuoted,
        crate::lexer::ScalarStyle::DoubleQuoted => ScalarStyle::DoubleQuoted,
    }
}

fn lex_to_parser_tokens(source: &str) -> Result<Vec<TokenWithSpan>, YamlError> {
    let lexer_tokens = crate::lexer::lex(source)?;
    let mut parser_tokens = Vec::with_capacity(lexer_tokens.len());

    for lt in lexer_tokens {
        let token = match lt.kind {
            crate::lexer::TokenKind::DocumentStart => ParserToken::DocumentStart,
            crate::lexer::TokenKind::DocumentEnd => ParserToken::DocumentEnd,
            crate::lexer::TokenKind::MappingKey => ParserToken::MappingKey,
            crate::lexer::TokenKind::SequenceEntry => ParserToken::SequenceEntry,
            crate::lexer::TokenKind::Colon => ParserToken::Colon,
            crate::lexer::TokenKind::Comma => ParserToken::Comma,
            crate::lexer::TokenKind::LeftBrace => ParserToken::LeftBrace,
            crate::lexer::TokenKind::RightBrace => ParserToken::RightBrace,
            crate::lexer::TokenKind::LeftBracket => ParserToken::LeftBracket,
            crate::lexer::TokenKind::RightBracket => ParserToken::RightBracket,
            crate::lexer::TokenKind::Anchor(name) => ParserToken::Anchor(name),
            crate::lexer::TokenKind::Alias(name) => ParserToken::Alias(name),
            crate::lexer::TokenKind::Tag(name) => ParserToken::Tag(name),
            crate::lexer::TokenKind::Directive(name) => ParserToken::Directive(name),
            crate::lexer::TokenKind::Scalar { raw, style, tag } => {
                ParserToken::Scalar { raw, style: convert_scalar_style(style), tag }
            }
            crate::lexer::TokenKind::BlockScalar { indicator, value, .. } => {
                let indicator = match indicator {
                    crate::lexer::BlockIndicator::Literal => BlockIndicator::Literal,
                    crate::lexer::BlockIndicator::Folded => BlockIndicator::Folded,
                };
                ParserToken::BlockScalar { indicator, value }
            }
            crate::lexer::TokenKind::Comment(text) => ParserToken::Comment(text),
            crate::lexer::TokenKind::BlankLine => ParserToken::BlankLine,
            crate::lexer::TokenKind::Eof => ParserToken::Eof,
        };
        parser_tokens.push(TokenWithSpan { token, span: lt.span });
    }
    Ok(parser_tokens)
}

pub fn parse(source: &str, options: &ParserOptions) -> Result<Document, YamlError> {
    let tokens = lex_to_parser_tokens(source)?;
    let parser = Parser::new(source, tokens, options);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParserOptions, YamlVersion};

    fn parse_simple(source: &str) -> Document {
        parse(source, &ParserOptions::default()).unwrap()
    }

    fn root_kind(doc: &Document) -> &NodeKind {
        &doc.node(doc.root().unwrap()).unwrap().kind
    }

    #[test]
    fn parses_simple_mapping() {
        let doc = parse_simple("key: value\n");
        assert_eq!(doc.documents.len(), 1);
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => assert_eq!(entries.len(), 1),
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_mapping() {
        let doc = parse_simple("a:\n  b: 1\n  c: 2\n");
        assert_eq!(doc.documents.len(), 1);
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => {
                assert_eq!(entries.len(), 1);
                match &doc.node(entries[0].1).unwrap().kind {
                    NodeKind::Mapping(inner) => assert_eq!(inner.len(), 2),
                    other => panic!("expected inner mapping, got {other:?}"),
                }
            }
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_sequence() {
        let doc = parse_simple("- a\n- b\n- c\n");
        assert_eq!(doc.documents.len(), 1);
        match root_kind(&doc) {
            NodeKind::Sequence(items) => assert_eq!(items.len(), 3),
            other => panic!("expected sequence, got {other:?}"),
        }
    }

    #[test]
    fn parses_sequence_value_aligned_with_key() {
        let doc = parse_simple("list:\n- a\n- b\n");
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => {
                assert_eq!(entries.len(), 1);
                match &doc.node(entries[0].1).unwrap().kind {
                    NodeKind::Sequence(items) => assert_eq!(items.len(), 2),
                    other => panic!("expected sequence, got {other:?}"),
                }
            }
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_mapping_after_nested_sequence() {
        let doc = parse_simple("items:\n  - a\n  - b\nname: end\n");
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => assert_eq!(entries.len(), 2),
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_flow_mapping() {
        let doc = parse_simple("{a: 1, b: 2}\n");
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => assert_eq!(entries.len(), 2),
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_flow_sequence() {
        let doc = parse_simple("[1, 2, 3]\n");
        match root_kind(&doc) {
            NodeKind::Sequence(items) => assert_eq!(items.len(), 3),
            other => panic!("expected sequence, got {other:?}"),
        }
    }

    #[test]
    fn parses_anchors_and_aliases() {
        let doc = parse_simple("a: &x 1\nb: *x\n");
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => {
                let b_value = entries[1].1;
                assert!(doc.is_alias(b_value));
            }
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_block_scalar() {
        let doc = parse_simple("text: |\n  line one\n  line two\n");
        assert_eq!(doc.documents.len(), 1);
    }

    #[test]
    fn parses_multiple_documents() {
        let doc = parse_simple("---\na: 1\n---\nb: 2\n");
        assert_eq!(doc.documents.len(), 2);
    }

    #[test]
    fn parses_bare_document_then_explicit() {
        let doc = parse_simple("a: 1\n---\nb: 2\n");
        assert_eq!(doc.documents.len(), 2);
    }

    #[test]
    fn rejects_trailing_content_without_separator() {
        let result = parse("a: 1\n]\n", &ParserOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn parses_quoted_strings() {
        let doc = parse_simple("a: 'single'\nb: \"double\"\n");
        assert_eq!(doc.documents.len(), 1);
    }

    #[test]
    fn quoted_scalars_are_not_type_resolved() {
        let doc = parse_simple("a: \"true\"\nb: '42'\n");
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => {
                for (_, value_id) in entries {
                    match &doc.node(*value_id).unwrap().kind {
                        NodeKind::Scalar(scalar) => {
                            assert!(matches!(scalar.value, ScalarValue::String(_)))
                        }
                        other => panic!("expected scalar, got {other:?}"),
                    }
                }
            }
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_boolean_1_2() {
        let opts =
            ParserOptions { yaml_version: Some(YamlVersion::Version12), ..Default::default() };
        let doc = parse("flag: true\n", &opts).unwrap();
        assert_eq!(doc.documents.len(), 1);
    }

    #[test]
    fn parses_sexagesimal_int_1_1() {
        let opts =
            ParserOptions { yaml_version: Some(YamlVersion::Version11), ..Default::default() };
        let doc = parse("value: 1:20:30\n", &opts).unwrap();
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => match &doc.node(entries[0].1).unwrap().kind {
                NodeKind::Scalar(scalar) => assert_eq!(scalar.value, ScalarValue::Int(4830)),
                other => panic!("expected scalar, got {other:?}"),
            },
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn rejects_duplicate_anchor() {
        let result = parse("a: &x 1\nb: &x 2\n", &ParserOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unknown_alias() {
        let result = parse("a: *x\n", &ParserOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn merge_key_expands_mapping() {
        let doc = parse_simple("base: &b\n  a: 1\n  b: 2\nchild:\n  <<: *b\n  b: 3\n  c: 4\n");
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => {
                let child = &entries[1];
                match &doc.node(child.1).unwrap().kind {
                    NodeKind::Mapping(child_entries) => {
                        let mut by_key = Vec::new();
                        for (k, v) in child_entries {
                            let key = match &doc.node(*k).unwrap().kind {
                                NodeKind::Scalar(s) => s.raw.clone(),
                                _ => panic!("expected scalar key"),
                            };
                            let value = match &doc.node(*v).unwrap().kind {
                                NodeKind::Scalar(s) => s.value.clone(),
                                _ => panic!("expected scalar value"),
                            };
                            by_key.push((key, value));
                        }
                        assert!(by_key.contains(&("b".to_string(), ScalarValue::Int(3))));
                        assert!(by_key.contains(&("a".to_string(), ScalarValue::Int(1))));
                        assert!(by_key.contains(&("c".to_string(), ScalarValue::Int(4))));
                        assert!(!by_key.iter().any(|(k, _)| k == "<<"));
                    }
                    other => panic!("expected mapping, got {other:?}"),
                }
            }
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn strict_version_rejects_a_1_1_vs_1_2_ambiguous_scalar() {
        let options = ParserOptions { strict_version: true, ..ParserOptions::default() };
        // "yes" is Bool(true) under 1.1's Norway-problem table but String("yes") under 1.2.
        let err = parse("a: yes\n", &options).unwrap_err();
        assert_eq!(err.kind, ErrorKind::AmbiguousVersion);
    }

    #[test]
    fn strict_version_accepts_an_unambiguous_scalar() {
        let options = ParserOptions { strict_version: true, ..ParserOptions::default() };
        // "true"/"false" resolve identically under both versions.
        let doc = parse("a: true\nb: 42\nc: hello\n", &options).unwrap();
        assert_eq!(doc.documents.len(), 1);
    }

    #[test]
    fn strict_version_is_inert_without_the_flag() {
        // Same ambiguous scalar as above, but the default (non-strict) parser just picks 1.2.
        let doc = parse_simple("a: yes\n");
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => {
                let value = &doc.node(entries[0].1).unwrap().kind;
                match value {
                    NodeKind::Scalar(s) => assert_eq!(s.value, ScalarValue::String("yes".into())),
                    other => panic!("expected scalar, got {other:?}"),
                }
            }
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn strict_version_is_inert_when_a_version_is_pinned() {
        let options = ParserOptions {
            strict_version: true,
            yaml_version: Some(YamlVersion::Version11),
            ..ParserOptions::default()
        };
        // Ambiguous between 1.1 and 1.2, but an explicit pin removes the ambiguity outright.
        let doc = parse("a: yes\n", &options).unwrap();
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => match &doc.node(entries[0].1).unwrap().kind {
                NodeKind::Scalar(s) => assert_eq!(s.value, ScalarValue::Bool(true)),
                other => panic!("expected scalar, got {other:?}"),
            },
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn strict_version_is_inert_when_a_yaml_directive_pins_the_version() {
        let options = ParserOptions { strict_version: true, ..ParserOptions::default() };
        // The `%YAML 1.1` directive pins the version for this document, so `yes` is unambiguous.
        let doc = parse("%YAML 1.1\n---\na: yes\n", &options).unwrap();
        match root_kind(&doc) {
            NodeKind::Mapping(entries) => match &doc.node(entries[0].1).unwrap().kind {
                NodeKind::Scalar(s) => assert_eq!(s.value, ScalarValue::Bool(true)),
                other => panic!("expected scalar, got {other:?}"),
            },
            other => panic!("expected mapping, got {other:?}"),
        }
    }
}
