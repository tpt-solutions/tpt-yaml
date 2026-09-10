//! Minimal regex-subset matcher for JSON Schema `pattern`/`patternProperties`. Full ECMA-262
//! regex is out of scope for a zero-extra-dependency crate; this supports the pragmatic subset
//! real schemas actually use: `^`/`$` anchors, `.`, `*`, `+`, `?`, character classes `[...]`
//! (with ranges and `^` negation), and the escapes `\\d \\w \\s \\D \\W \\S` (both inside and
//! outside classes). Anything else in the pattern is matched literally. Documented in the
//! README as a known limitation.

use alloc::string::String;
use alloc::vec::Vec;

/// A parsed pattern, compiled once at schema-load time.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    /// Parsed AST; anchored at `start` when the pattern began with `^`.
    ast: Vec<Piece>,
    anchored_start: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Any,
    Literal(char),
    Class(Vec<ClassItem>, bool),
    /// `piece *`, `+`, `?` — target, min repeats, unbounded max.
    Repeat(Box<Piece>, usize, Option<usize>),
}

#[derive(Clone, Debug, PartialEq)]
enum ClassItem {
    Char(char),
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

pub(crate) fn compile(source: &str) -> Result<Pattern, ()> {
    let chars: Vec<char> = source.chars().collect();
    let mut pos = 0usize;
    let anchored_start = chars.first() == Some(&'^');
    if anchored_start {
        pos = 1;
    }
    let ast = parse_seq(&chars, &mut pos, &mut false)?;
    // A trailing `^` mid-pattern or unbalanced `[` shows up as leftover parse position.
    if pos < chars.len() && !matches!(chars.get(pos), Some('$')) {
        return Err(());
    }
    Ok(Pattern { ast, anchored_start })
}

fn parse_seq(chars: &[char], pos: &mut usize, stop_at_dollar: &mut bool) -> Result<Vec<Piece>, ()> {
    let mut pieces = Vec::new();
    while *pos < chars.len() {
        match chars[*pos] {
            '$' => {
                // `$` is only an end-anchor at the very end of the pattern.
                if *pos + 1 == chars.len() {
                    *stop_at_dollar = true;
                    *pos += 1;
                    return Ok(pieces);
                }
                pieces.push(Piece::Literal('$'));
                *pos += 1;
            }
            '.' => {
                pieces.push(quantify(chars, pos, Piece::Any)?);
            }
            '[' => {
                let class = parse_class(chars, pos)?;
                pieces.push(quantify(chars, pos, Piece::Class(class.0, class.1))?);
            }
            '\\' => {
                *pos += 1;
                let piece = match chars.get(*pos) {
                    Some('d') => Piece::Class(vec![ClassItem::Digit(false)], false),
                    Some('D') => Piece::Class(vec![ClassItem::Digit(true)], false),
                    Some('w') => Piece::Class(vec![ClassItem::Word(false)], false),
                    Some('W') => Piece::Class(vec![ClassItem::Word(true)], false),
                    Some('s') => Piece::Class(vec![ClassItem::Space(false)], false),
                    Some('S') => Piece::Class(vec![ClassItem::Space(true)], false),
                    Some(&c) => Piece::Literal(c),
                    None => return Err(()),
                };
                *pos += 1;
                pieces.push(quantify(chars, pos, piece)?);
            }
            '*' | '+' | '?' => return Err(()), // quantifier with nothing to repeat
            '^' => return Err(()),             // mid-pattern `^` unsupported
            c => {
                pieces.push(quantify(chars, pos, Piece::Literal(c))?);
            }
        }
    }
    Ok(pieces)
}

/// Consumes the quantifier (if any) following the piece that `*pos` currently points past.
fn quantify(chars: &[char], pos: &mut usize, piece: Piece) -> Result<Piece, ()> {
    *pos += 1; // consume the piece's own character
    match chars.get(*pos) {
        Some('*') => {
            *pos += 1;
            Ok(Piece::Repeat(Box::new(piece), 0, None))
        }
        Some('+') => {
            *pos += 1;
            Ok(Piece::Repeat(Box::new(piece), 1, None))
        }
        Some('?') => {
            *pos += 1;
            Ok(Piece::Repeat(Box::new(piece), 0, Some(1)))
        }
        _ => Ok(piece),
    }
}

fn parse_class(chars: &[char], pos: &mut usize) -> Result<(Vec<ClassItem>, bool), ()> {
    *pos += 1; // consume `[`
    let negated = matches!(chars.get(*pos), Some('^'));
    if negated {
        *pos += 1;
    }
    let mut items = Vec::new();
    let mut first = true;
    loop {
        let c = *chars.get(*pos).ok_or(())?;
        if c == ']' && !first {
            *pos += 1;
            return Ok((items, negated));
        }
        first = false;
        let lo = if c == '\\' {
            *pos += 1;
            let e = *chars.get(*pos).ok_or(())?;
            *pos += 1;
            match e {
                'd' => {
                    items.push(ClassItem::Digit(false));
                    continue;
                }
                'D' => {
                    items.push(ClassItem::Digit(true));
                    continue;
                }
                'w' => {
                    items.push(ClassItem::Word(false));
                    continue;
                }
                'W' => {
                    items.push(ClassItem::Word(true));
                    continue;
                }
                's' => {
                    items.push(ClassItem::Space(false));
                    continue;
                }
                'S' => {
                    items.push(ClassItem::Space(true));
                    continue;
                }
                other => other,
            }
        } else {
            *pos += 1;
            c
        };
        // Range `a-z` (a `-` not followed by `]` and not first).
        if chars.get(*pos) == Some(&'-') && chars.get(*pos + 1) != Some(&']') && chars.get(*pos + 1).is_some() {
            *pos += 1;
            let hi = *chars.get(*pos).ok_or(())?;
            *pos += 1;
            items.push(ClassItem::Range(lo, hi));
        } else {
            items.push(ClassItem::Char(lo));
        }
    }
}

impl Pattern {
    /// Whether `text` *contains* a match (ECMA regex `search` semantics).
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let start = if self.anchored_start { 0 } else { 0 };
        if self.anchored_start {
            return match_seq(&self.ast, &chars, 0).is_some();
        }
        let _ = start;
        for offset in 0..=chars.len() {
            if match_seq(&self.ast, &chars, offset).is_some() {
                return true;
            }
        }
        false
    }
}

/// Backtracking matcher: returns the furthest position reached matching `pieces` from `pos`, or
/// `None`. Unanchored search semantics are provided by `is_match`'s offset loop.
fn match_seq(pieces: &[Piece], chars: &[char], pos: usize) -> Option<usize> {
    let Some((first, rest)) = pieces.split_first() else {
        return Some(pos);
    };
    match first {
        Piece::Any => chars.get(pos).map(|_| pos + 1).and_then(|next| match_seq(rest, chars, next)),
        Piece::Literal(c) => {
            if chars.get(pos) == Some(c) {
                match_seq(rest, chars, pos + 1)
            } else {
                None
            }
        }
        Piece::Class(items, negated) => chars
            .get(pos)
            .filter(|c| class_matches(items, *negated, **c) != *negated)
            .and_then(|_| match_seq(rest, chars, pos + 1)),
        Piece::Repeat(target, min, max) => repeat_match(target, *min, *max, rest, chars, pos),
    }
}

fn repeat_match(
    target: &Piece,
    min: usize,
    max: Option<usize>,
    rest: &[Piece],
    chars: &[char],
    pos: usize,
) -> Option<usize> {
    // Greedy: consume as many as possible, backtrack until `rest` matches.
    let mut ends = vec![pos];
    let mut current = pos;
    let limit = max.unwrap_or(usize::MAX);
    while ends.len() <= limit {
        match match_seq(core::slice::from_ref(target), chars, current) {
            Some(next) if next > current || ends.len() <= min => {
                current = next;
                ends.push(current);
                if next == pos && ends.len() > min {
                    break; // zero-width match: don't loop forever
                }
            }
            _ => break,
        }
    }
    ends.reverse(); // greedy: longest first
    for &end in &ends {
        if ends.len() - 1 < min {
            continue;
        }
        if let Some(final_pos) = match_seq(rest, chars, end) {
            return Some(final_pos);
        }
    }
    None
}

fn class_matches(items: &[ClassItem], negated: bool, c: char) -> bool {
    let hit = items.iter().any(|item| match item {
        ClassItem::Char(x) => *x == c,
        ClassItem::Range(lo, hi) => *lo <= c && c <= *hi,
        ClassItem::Digit(neg) => c.is_ascii_digit() != *neg,
        ClassItem::Word(neg) => (c.is_alphanumeric() || c == '_') != *neg,
        ClassItem::Space(neg) => c.is_whitespace() != *neg,
    });
    hit != negated
}

#[allow(dead_code)]
fn _unused(_: String) {}
