use regex::Regex;

/// Conservative pattern-based redaction. This is a risk reduction measure, not
/// guaranteed de-identification. Explicit names are replaced case-insensitively.
pub fn redact(input: &str, names: &[String]) -> String {
    let mut out = input.to_owned();
    for (i, name) in names.iter().filter(|n| !n.trim().is_empty()).enumerate() {
        let escaped = regex::escape(name.trim());
        if let Ok(re) = Regex::new(&format!("(?i){escaped}")) {
            out = re
                .replace_all(&out, format!("[PERSON_{}]", i + 1))
                .into_owned();
        }
    }
    let patterns = [
        (r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b", "[EMAIL]"),
        (
            r"\b(?:\+?1[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}\b",
            "[PHONE]",
        ),
        (r"\b[A-Z]\d[A-Z][ -]?\d[A-Z]\d\b", "[POSTAL_CODE]"),
        (r"\b\d{3}[- ]?\d{3}[- ]?\d{3}\b", "[ID_NUMBER]"),
    ];
    for (pattern, replacement) in patterns {
        if let Ok(re) = Regex::new(pattern) {
            out = re.replace_all(&out, replacement).into_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_supplied_name_and_common_identifiers() {
        let got = redact(
            "Jane Doe jane@example.com 902-555-1234",
            &["Jane Doe".into()],
        );
        assert!(!got.contains("Jane Doe"));
        assert!(!got.contains("jane@example.com"));
        assert!(!got.contains("902-555-1234"));
    }
}
