use crate::version::YamlVersion;
use crate::ScalarValue;

/// Resolve an implicit scalar according to the selected YAML version.
pub fn resolve_scalar(text: &str, version: YamlVersion) -> ScalarValue {
    let text = text.trim();
    if text.is_empty() {
        return ScalarValue::Null;
    }

    if version == YamlVersion::Version12 {
        if text == "true" || text == "True" || text == "TRUE" {
            return ScalarValue::Bool(true);
        }
        if text == "false" || text == "False" || text == "FALSE" {
            return ScalarValue::Bool(false);
        }
    } else {
        // YAML 1.1's core schema bool set is a superset of 1.2's: `y`/`yes`/`on` and friends
        // *in addition to* `true`/`false` (not instead of them) — see the Norway-problem table
        // in the YAML 1.1 spec. Omitting `true`/`false` here would make them resolve as
        // `String` under 1.1 but `Bool` under 1.2, flagging the single most common YAML boolean
        // spelling as version-ambiguous for no reason (see `ParserOptions::strict_version`).
        match text {
            "y" | "Y" | "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => {
                return ScalarValue::Bool(true)
            }
            "n" | "N" | "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off"
            | "OFF" => return ScalarValue::Bool(false),
            _ => {}
        }
    }

    if text == "~" || text == "null" || text == "Null" || text == "NULL" {
        return ScalarValue::Null;
    }

    if version == YamlVersion::Version11 {
        if let Some(value) = parse_sexagesimal_int(text) {
            return ScalarValue::Int(value);
        }
        if let Some(value) = parse_sexagesimal_float(text) {
            return ScalarValue::Float(value);
        }
    }

    if let Some(value) = parse_int(text, version) {
        return ScalarValue::Int(value);
    }
    if let Some(value) = parse_float(text) {
        return ScalarValue::Float(value);
    }
    if looks_like_timestamp(text) {
        return ScalarValue::Timestamp(text.to_string());
    }
    ScalarValue::String(text.to_string())
}

/// YAML 1.1 sexagesimal integer, e.g. `1:20:30` (only digit groups, no fraction).
fn parse_sexagesimal_int(text: &str) -> Option<i64> {
    if !text.contains(':') || text.contains(['.', 'e', 'E']) {
        return None;
    }
    let (neg, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let mut value: i64 = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let digit: i64 = part.parse().ok()?;
        if i > 0 && digit >= 60 {
            return None;
        }
        value = value.checked_mul(60)?.checked_add(digit)?;
    }
    Some(if neg { -value } else { value })
}

/// YAML 1.1 sexagesimal float, e.g. `1:20:30.5`.
fn parse_sexagesimal_float(text: &str) -> Option<f64> {
    if !text.contains(':') {
        return None;
    }
    let value = text.replace(',', "");
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() == 2 || parts.len() == 3 {
        let whole: f64 = parts[0].parse().ok()?;
        let frac: f64 = parts[1].parse().ok()?;
        let sec: f64 = if parts.len() == 3 { parts[2].parse().ok()? } else { 0.0 };
        if (0.0..60.0).contains(&frac) && (0.0..60.0).contains(&sec) {
            return Some(whole + frac / 60.0 + sec / 3600.0);
        }
    }
    None
}

/// Converts a non-negative magnitude to `i64`, honoring `negative` — including the one
/// magnitude (`2^63`) that only fits in `i64` as `i64::MIN`, which a plain `try_from` rejects.
fn to_signed(value: u64, negative: bool) -> Option<i64> {
    if negative {
        if value == 1u64 << 63 {
            Some(i64::MIN)
        } else {
            i64::try_from(value).ok().map(|v| -v)
        }
    } else {
        i64::try_from(value).ok()
    }
}

fn parse_int(text: &str, version: YamlVersion) -> Option<i64> {
    let negative = text.starts_with('-');
    let signed = negative || text.starts_with('+');
    let digits = if signed { &text[1..] } else { text };

    if let Some(rest) = digits.strip_prefix('0') {
        if !rest.is_empty() {
            if version == YamlVersion::Version12 && digits.starts_with("0o") {
                let value = u64::from_str_radix(&digits[2..], 8).ok()?;
                return to_signed(value, negative);
            }
            if version == YamlVersion::Version11 && rest.bytes().all(|b| (b'0'..=b'7').contains(&b))
            {
                let value = u64::from_str_radix(rest, 8).ok()?;
                return to_signed(value, negative);
            }
            return None;
        }
    }

    if digits.bytes().all(|b| b.is_ascii_digit()) {
        let value: u64 = digits.parse().ok()?;
        return to_signed(value, negative);
    }
    None
}

fn parse_float(text: &str) -> Option<f64> {
    let value = text.replace(',', "");
    if text.contains(['.', 'e', 'E']) || text.starts_with(['+', '-']) {
        return value.parse::<f64>().ok();
    }
    None
}

fn looks_like_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_prefixed_int_is_not_negated() {
        assert_eq!(resolve_scalar("+42", YamlVersion::Version12), ScalarValue::Int(42));
    }

    #[test]
    fn minus_prefixed_int_is_negated() {
        assert_eq!(resolve_scalar("-42", YamlVersion::Version12), ScalarValue::Int(-42));
    }

    #[test]
    fn i64_min_round_trips_as_int_not_float() {
        assert_eq!(
            resolve_scalar("-9223372036854775808", YamlVersion::Version12),
            ScalarValue::Int(i64::MIN)
        );
    }

    #[test]
    fn i64_max_round_trips_as_int() {
        assert_eq!(
            resolve_scalar("9223372036854775807", YamlVersion::Version12),
            ScalarValue::Int(i64::MAX)
        );
    }
}
