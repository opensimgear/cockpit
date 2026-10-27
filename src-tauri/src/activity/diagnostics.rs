//! Redacts credential values while retaining useful explanatory text.

const REDACTED: &str = "[redacted]";
const LIMIT: usize = 1024;
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passphrase",
    "secret",
    "ssid",
    "api_key",
    "api-key",
    "api key",
    "apikey",
    "access_token",
    "refresh_token",
    "auth_token",
    "token",
    "cookie",
    "set-cookie",
    "credential",
    "private_key",
    "private-key",
    "client_secret",
    "client-secret",
    "authorization",
];

/// Returns a bounded diagnostic with recognizable credential values removed.
/// Safe domain explanations (for example, “resource token expired”) are kept.
pub(crate) fn reason(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t')
        .collect();
    let mut text = redact_assignments(&cleaned);
    text = redact_labelled_passwords(&text);
    text = redact_uri_userinfo(&text);
    text = redact_bearer(&text);
    text = redact_osg_tokens(&text);
    if looks_like_unstructured_secret(&text) {
        return "Sensitive diagnostic content was omitted.".into();
    }
    let bounded: String = text.chars().take(LIMIT).collect();
    if bounded.trim().is_empty() {
        "No additional diagnostic details are available.".into()
    } else {
        bounded
    }
}

fn redact_labelled_passwords(input: &str) -> String {
    let words: Vec<&str> = input.split_inclusive(char::is_whitespace).collect();
    let mut out=String::new();
    let mut password=false;
    for word in words {
        let plain=word.trim();
        let lower=plain.to_ascii_lowercase();
        if password && !["is","was","has","must","authentication","required","[redacted]"].contains(&lower.as_str()) {
            out.push_str(REDACTED);
            out.push_str(&word[plain.len()..]);
        } else {out.push_str(word);}
        password=matches!(lower.as_str(),"password"|"passphrase");
    }
    out
}

fn redact_assignments(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let found = SENSITIVE_KEYS
            .iter()
            .filter_map(|key| {
                lower[cursor..]
                    .find(key)
                    .map(|offset| (cursor + offset, *key))
            })
            .filter(|(at, key)| token_boundary(&lower, *at, key.len()))
            .min_by_key(|(at, _)| *at);
        let Some((at, key)) = found else {
            out.push_str(&input[cursor..]);
            break;
        };
        out.push_str(&input[cursor..at]);
        let key_end = at + key.len();
        let mut value_start = key_end;
        while value_start < input.len() && input.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start < input.len()
            && input.as_bytes()[value_start] == b'"'
        {
            // JSON key syntax: the quoted key is followed by a colon.
            value_start += 1;
            while value_start < input.len() && input.as_bytes()[value_start].is_ascii_whitespace() {
                value_start += 1;
            }
        }
        if value_start >= input.len() || !matches!(input.as_bytes()[value_start], b'=' | b':') {
            out.push_str(&input[at..key_end]);
            cursor = key_end;
            continue;
        }
        value_start += 1;
        while value_start < input.len() && input.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start >= input.len() {
            out.push_str(&input[at..]);
            break;
        }
        let quote = input.as_bytes()[value_start];
        let start_value = if quote == b'"' || quote == b'\'' {
            value_start + 1
        } else {
            value_start
        };
        let mut end = start_value;
        if quote == b'"' || quote == b'\'' {
            let mut escaped = false;
            while end < input.len() {
                let byte = input.as_bytes()[end];
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == quote {
                    break;
                }
                end += 1;
            }
        } else {
            while end < input.len()
                && !matches!(
                    input.as_bytes()[end],
                    b',' | b';' | b'&' | b'\n' | b'}'
                )
            {
                end += 1;
            }
        }
        if start_value == end {
            out.push_str(&input[at..end]);
            cursor = end;
            continue;
        }
        out.push_str(&input[at..start_value]);
        out.push_str(REDACTED);
        if quote == b'"' || quote == b'\'' {
            out.push(quote as char);
            end = (end + 1).min(input.len());
        }
        cursor = end;
    }
    out
}

fn token_boundary(text: &str, at: usize, len: usize) -> bool {
    let before = at
        .checked_sub(1)
        .and_then(|i| text.as_bytes().get(i))
        .copied();
    let after = text.as_bytes().get(at + len).copied();
    before.is_none_or(|b| !b.is_ascii_alphanumeric() && b != b'_')
        && after.is_none_or(|b| !b.is_ascii_alphanumeric() && b != b'_')
}

fn redact_bearer(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::new();
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find("bearer ") {
        let at = cursor + offset;
        out.push_str(&input[cursor..at]);
        out.push_str("Bearer ");
        let start = at + 7;
        let mut end = start;
        while end < input.len()
            && !input.as_bytes()[end].is_ascii_whitespace()
            && !matches!(input.as_bytes()[end], b',' | b';' | b'"' | b'\'')
        {
            end += 1;
        }
        if end == start {
            cursor = start;
        } else {
            out.push_str(REDACTED);
            cursor = end;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn redact_osg_tokens(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::new();
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find("osg_mcp_") {
        let at = cursor + offset;
        out.push_str(&input[cursor..at]);
        let mut end = at + 8;
        while end < input.len()
            && (input.as_bytes()[end].is_ascii_alphanumeric()
                || matches!(input.as_bytes()[end], b'_' | b'-' | b'.'))
        {
            end += 1;
        }
        out.push_str(REDACTED);
        cursor = end;
    }
    out.push_str(&input[cursor..]);
    out
}

fn redact_uri_userinfo(input: &str) -> String {
    let mut out = String::new();
    let mut cursor = 0;
    while let Some(offset) = input[cursor..].find("://") {
        let scheme_end = cursor + offset + 3;
        out.push_str(&input[cursor..scheme_end]);
        let authority_end = input[scheme_end..]
            .find(['/', '?', '#', ' ', '\n', '\t'])
            .map(|n| scheme_end + n)
            .unwrap_or(input.len());
        if let Some(at_rel) = input[scheme_end..authority_end].rfind('@') {
            let at = scheme_end + at_rel;
            out.push_str(REDACTED);
            out.push_str(&input[at..authority_end]);
        } else {
            out.push_str(&input[scheme_end..authority_end]);
        }
        cursor = authority_end;
    }
    out.push_str(&input[cursor..]);
    out
}

fn looks_like_unstructured_secret(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with("-----BEGIN ") && trimmed.contains("PRIVATE KEY"))
        || (trimmed.len() > 80
            && !trimmed.contains(char::is_whitespace)
            && trimmed
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '_' | '-')))
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
