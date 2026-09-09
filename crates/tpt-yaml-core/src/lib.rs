//! Lexer, parser, and arena-based node model for YAML 1.1/1.2, with zero external dependencies.
//!
//! [`parse`] builds a [`Document`]: an arena of [`NodeData`] indexed by [`NodeId`], with block
//! and flow mappings/sequences, anchors/aliases, `<<` merge keys, and multi-document streams.
//! [`YamlVersion`] controls implicit scalar resolution (the "Norway problem" boolean table,
//! octal sigils, sexagesimal ints/floats) via [`ParserOptions::yaml_version`], auto-detected
//! from a `%YAML` directive when not set explicitly.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod diagnostics;
pub mod event;
pub mod lexer;
pub mod node;
pub mod parser;
pub mod resolve;
pub mod span;
pub mod version;

pub use diagnostics::{ErrorContext, ErrorKind, ParseError, YamlError};
pub use node::{
    Document, Node, NodeData, NodeId, NodeKind, Scalar, ScalarStyle, ScalarValue, Trivia,
    TriviaKind,
};
pub use parser::{parse as parser_parse, ParserOptions};
pub use span::Span;
pub use version::YamlVersion;

use alloc::string::String;

/// Parse source as a YAML 1.2 stream using the default parser options.
pub fn parse(source: &str) -> Result<Document, YamlError> {
    parse_with_options(source, &ParserOptions::default())
}

/// Parse source with explicit version and merge-key options.
pub fn parse_with_options(source: &str, options: &ParserOptions) -> Result<Document, YamlError> {
    parser::parse(source, options)
}

/// Render a synthesized node using the shared YAML pretty printer.
pub fn pretty_print(node: NodeId, document: &Document) -> String {
    node::pretty_print(node, document)
}
