//! Hand-rolled `format` keyword validators for a small, commonly used JSON Schema subset:
//! `email`, `date-time`, `date`, `uri`, `ipv4`, `ipv6`, `uuid`. No `regex`/`chrono`/`url`
//! dependency — deliberately hand-rolled, same "pragmatic subset, not the full spec" approach as
//! `pattern.rs`'s regex-lite matcher. See the README's "Known limitations" for exactly what each
//! validator does and doesn't check; none of these claim full RFC conformance.

use alloc::vec::Vec;

/// A supported `format` keyword value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// A loose but structurally-checked email address (`local@domain`, no full RFC 5322 quoting
    /// or comment support).
    Email,
    /// An RFC 3339 date-time, e.g. `2024-01-15T10:30:00Z` or `2024-01-15T10:30:00.123+02:00`.
    /// Requires an offset (`Z` or `±HH:MM`); leap seconds (`:60`) are accepted.
    DateTime,
    /// An RFC 3339 full-date, e.g. `2024-01-15` (calendar-valid: rejects `2024-02-30`).
    Date,
    /// A URI with a scheme (`scheme:` followed by a non-empty rest); not a full RFC 3986
    /// grammar check.
    Uri,
    /// A dotted-decimal IPv4 address, e.g. `192.168.1.1` (rejects octets over 255 and leading
    /// zeros).
    Ipv4,
    /// A colon-separated IPv6 address, including `::` compression. Does not support the
    /// IPv4-mapped tail form (`::ffff:192.0.2.1`).
    Ipv6,
    /// An 8-4-4-4-12 hyphenated hex UUID (any version/variant; doesn't check version/variant
    /// bits).
    Uuid,
}

impl Format {
    /// Maps a schema's `format` string to a supported validator, or `None` if it names a format
    /// this crate doesn't validate — matching this crate's existing "unknown keyword silently
    /// ignored" convention, now scoped to just the subset of `format` values with no validator.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "email" => Self::Email,
            "date-time" => Self::DateTime,
            "date" => Self::Date,
            "uri" => Self::Uri,
            "ipv4" => Self::Ipv4,
            "ipv6" => Self::Ipv6,
            "uuid" => Self::Uuid,
            _ => return None,
        })
    }

    /// The `format` keyword's string spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::DateTime => "date-time",
            Self::Date => "date",
            Self::Uri => "uri",
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
            Self::Uuid => "uuid",
        }
    }

    /// Whether `value` satisfies this format.
    pub fn is_match(self, value: &str) -> bool {
        match self {
            Self::Email => is_email(value),
            Self::DateTime => is_date_time(value),
            Self::Date => is_date(value),
            Self::Uri => is_uri(value),
            Self::Ipv4 => is_ipv4(value),
            Self::Ipv6 => is_ipv6(value),
            Self::Uuid => is_uuid(value),
        }
    }
}

fn is_email(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let Some((local, domain)) = s.split_once('@') else { return false };
    if local.is_empty() || local.len() > 64 {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    if !local.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c)) {
        return false;
    }
    is_hostname(domain)
}

fn is_hostname(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 || !s.contains('.') {
        return false;
    }
    if s.starts_with('.') || s.ends_with('.') {
        return false;
    }
    s.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

fn is_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let [y, m, d] = parts[..] else { return false };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>())
    else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }
    (1..=days_in_month(year, month)).contains(&day)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn is_date_time(s: &str) -> bool {
    let Some(t_pos) = s.find(['T', 't']) else { return false };
    let (date_part, rest) = s.split_at(t_pos);
    if !is_date(date_part) {
        return false;
    }
    is_time(&rest[1..])
}

fn is_time(s: &str) -> bool {
    // Split off the required timezone: a trailing `Z`/`z`, or a `+HH:MM`/`-HH:MM` offset that
    // must appear after the seconds field (so it isn't confused with anything else).
    let (body, has_tz) = if let Some(stripped) = s.strip_suffix(['Z', 'z']) {
        (stripped, true)
    } else if let Some(pos) = s.rfind(['+', '-']) {
        if pos < 8 {
            return false;
        }
        let (t, tz) = s.split_at(pos);
        if !is_valid_offset(tz) {
            return false;
        }
        (t, true)
    } else {
        (s, false)
    };
    if !has_tz {
        // RFC 3339 date-times require an offset (this validator doesn't accept a bare local time).
        return false;
    }
    let (hms, frac) = match body.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (body, None),
    };
    if let Some(f) = frac {
        if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    let parts: Vec<&str> = hms.split(':').collect();
    let [hh, mm, ss] = parts[..] else { return false };
    if hh.len() != 2 || mm.len() != 2 || ss.len() != 2 {
        return false;
    }
    let (Ok(h), Ok(m), Ok(sec)) = (hh.parse::<u32>(), mm.parse::<u32>(), ss.parse::<u32>()) else {
        return false;
    };
    // `sec <= 60` allows a leap second.
    h <= 23 && m <= 59 && sec <= 60
}

fn is_valid_offset(tz: &str) -> bool {
    let bytes = tz.as_bytes();
    if bytes.len() != 6 || (bytes[0] != b'+' && bytes[0] != b'-') || bytes[3] != b':' {
        return false;
    }
    let (Ok(h), Ok(m)) = (tz[1..3].parse::<u32>(), tz[4..6].parse::<u32>()) else { return false };
    h <= 23 && m <= 59
}

fn is_uri(s: &str) -> bool {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return false;
    }
    let Some(colon) = s.find(':') else { return false };
    let scheme = &s[..colon];
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else { return false };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return false;
    }
    // Require something after the scheme's colon (rules out a bare "http:").
    colon + 1 < s.len()
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    let [a, b, c, d] = parts[..] else { return false };
    [a, b, c, d].iter().all(|p| is_ipv4_octet(p))
}

fn is_ipv4_octet(p: &str) -> bool {
    if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if p.len() > 1 && p.starts_with('0') {
        return false;
    }
    p.parse::<u16>().map(|v| v <= 255).unwrap_or(false)
}

fn is_ipv6(s: &str) -> bool {
    if s.matches("::").count() > 1 {
        return false;
    }
    if let Some(pos) = s.find("::") {
        let (left, right) = (&s[..pos], &s[pos + 2..]);
        let left_groups: Vec<&str> =
            if left.is_empty() { Vec::new() } else { left.split(':').collect() };
        let right_groups: Vec<&str> =
            if right.is_empty() { Vec::new() } else { right.split(':').collect() };
        // `::` must compress at least one group of the 8.
        left_groups.len() + right_groups.len() < 8
            && left_groups.iter().chain(right_groups.iter()).all(|g| is_hex_group(g))
    } else {
        let groups: Vec<&str> = s.split(':').collect();
        groups.len() == 8 && groups.iter().all(|g| is_hex_group(g))
    }
}

fn is_hex_group(g: &str) -> bool {
    !g.is_empty() && g.len() <= 4 && g.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let [p1, p2, p3, p4, p5] = parts[..] else { return false };
    [(p1, 8), (p2, 4), (p3, 4), (p4, 4), (p5, 12)]
        .iter()
        .all(|(p, len)| p.len() == *len && p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email() {
        for ok in ["a@b.co", "alice.bob+tag@sub.example.com"] {
            assert!(Format::Email.is_match(ok), "{ok}");
        }
        for bad in ["", "no-at-sign", "@b.co", "a@", "a@nodot", "a..b@example.com", "a@b..com"] {
            assert!(!Format::Email.is_match(bad), "{bad}");
        }
    }

    #[test]
    fn date_time() {
        for ok in [
            "2024-01-15T10:30:00Z",
            "2024-01-15t10:30:00z",
            "2024-01-15T10:30:00.123456+02:00",
            "2024-02-29T00:00:00Z", // 2024 is a leap year
        ] {
            assert!(Format::DateTime.is_match(ok), "{ok}");
        }
        for bad in [
            "2024-01-15",
            "10:30:00Z",
            "2024-01-15T10:30:00",  // no offset
            "2024-13-01T00:00:00Z", // bad month
            "2024-01-32T00:00:00Z", // bad day
            "2024-01-15T25:00:00Z", // bad hour
            "2023-02-29T00:00:00Z", // 2023 not a leap year
            "not-a-date-time",
        ] {
            assert!(!Format::DateTime.is_match(bad), "{bad}");
        }
    }

    #[test]
    fn date() {
        assert!(Format::Date.is_match("2024-01-15"));
        assert!(Format::Date.is_match("2000-02-29"));
        assert!(!Format::Date.is_match("2024-02-30"));
        assert!(!Format::Date.is_match("1900-02-29")); // not a leap year
        assert!(!Format::Date.is_match("2024-1-15"));
    }

    #[test]
    fn uri() {
        for ok in [
            "https://example.com/path?q=1",
            "mailto:alice@example.com",
            "urn:isbn:0451450523",
            "file:///etc/hosts",
        ] {
            assert!(Format::Uri.is_match(ok), "{ok}");
        }
        for bad in ["not a uri", "://no-scheme", "http:", "1http://bad-scheme-start"] {
            assert!(!Format::Uri.is_match(bad), "{bad}");
        }
    }

    #[test]
    fn ipv4() {
        for ok in ["0.0.0.0", "192.168.1.1", "255.255.255.255"] {
            assert!(Format::Ipv4.is_match(ok), "{ok}");
        }
        for bad in ["256.1.1.1", "1.2.3", "1.2.3.4.5", "01.2.3.4", "a.b.c.d"] {
            assert!(!Format::Ipv4.is_match(bad), "{bad}");
        }
    }

    #[test]
    fn ipv6() {
        for ok in ["2001:db8::1", "::1", "::", "fe80::1234:5678:9abc:def0", "1:2:3:4:5:6:7:8"] {
            assert!(Format::Ipv6.is_match(ok), "{ok}");
        }
        for bad in ["not-ipv6", "1:2:3:4:5:6:7:8:9", "12345::1", "1::2::3"] {
            assert!(!Format::Ipv6.is_match(bad), "{bad}");
        }
    }

    #[test]
    fn uuid() {
        assert!(Format::Uuid.is_match("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!Format::Uuid.is_match("not-a-uuid"));
        assert!(!Format::Uuid.is_match("550e8400-e29b-41d4-a716-44665544000")); // short
    }

    #[test]
    fn from_name_round_trips_and_rejects_unknown() {
        for f in [
            Format::Email,
            Format::DateTime,
            Format::Date,
            Format::Uri,
            Format::Ipv4,
            Format::Ipv6,
            Format::Uuid,
        ] {
            assert_eq!(Format::from_name(f.name()), Some(f));
        }
        assert_eq!(Format::from_name("not-a-real-format"), None);
    }
}
