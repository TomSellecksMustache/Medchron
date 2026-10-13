use crate::{
    models::{Category, Entry, Status},
    redaction::redact,
};
use chrono::NaiveDate;
use regex::Regex;
use uuid::Uuid;

pub const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;

fn month_number(s: &str) -> Option<u32> {
    match s.trim_end_matches('.').to_ascii_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}
fn written_date(line: &str) -> Option<String> {
    let names = "January|February|March|April|May|June|July|August|September|October|November|December|Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sep|Sept|Oct|Nov|Dec";
    let mdy = Regex::new(&format!(
        r"(?i)\b({names})\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?,?\s+((?:19|20)\d{{2}})\b"
    ))
    .ok()?;
    if let Some(c) = mdy.captures(line) {
        if let Some(date) =
            NaiveDate::from_ymd_opt(c[3].parse().ok()?, month_number(&c[1])?, c[2].parse().ok()?)
        {
            return Some(date.to_string());
        }
    }
    let dmy = Regex::new(&format!(
        r"(?i)\b(\d{{1,2}})(?:st|nd|rd|th)?\s+({names})\.?,?\s+((?:19|20)\d{{2}})\b"
    ))
    .ok()?;
    if let Some(c) = dmy.captures(line) {
        if let Some(date) =
            NaiveDate::from_ymd_opt(c[3].parse().ok()?, month_number(&c[2])?, c[1].parse().ok()?)
        {
            return Some(date.to_string());
        }
    }
    None
}

fn find_date(text: &str) -> Option<String> {
    let iso = Regex::new(r"\b((?:19|20)\d{2}-\d{2}-\d{2})\b").ok()?;
    for cap in iso.captures_iter(text) {
        let s = cap.get(1)?.as_str();
        if NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
            return Some(s.to_string());
        }
    }
    let numeric = Regex::new(r"\b(\d{1,2})/(\d{1,2})/((?:19|20)\d{2})\b").ok()?;
    for cap in numeric.captures_iter(text) {
        let m: u32 = cap[1].parse().ok()?;
        let d: u32 = cap[2].parse().ok()?;
        let y: i32 = cap[3].parse().ok()?;
        if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
            return Some(date.to_string());
        }
    }
    written_date(text)
}

fn starts_dated_event(line: &str) -> bool {
    if find_date(line).is_none() {
        return false;
    }
    let prefix = Regex::new(r"(?i)^(date(?: of service| of study)?|visit date|service date|lumbar spine mri - date of study)\s*:\s*").unwrap();
    let starts_iso = Regex::new(r"^(?:19|20)\d{2}-\d{2}-\d{2}\b").unwrap();
    let start_written = Regex::new(
        r"(?i)^(?:\d{1,2}\s+)?(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\b",
    )
    .unwrap();
    prefix.is_match(line) || starts_iso.is_match(line) || start_written.is_match(line)
}

fn classify(text: &str) -> Category {
    let t = text.to_lowercase();
    if [
        "insurer",
        "insurance",
        "claim",
        "adjuster",
        "benefits",
        "denying",
    ]
    .iter()
    .any(|s| t.contains(s))
    {
        Category::Insurer
    } else if ["diagnosis", "diagnosed", "assessment:"]
        .iter()
        .any(|s| t.contains(s))
    {
        Category::Diagnosis
    } else if ["clinic", "appointment", "follow-up", "seen", "visit"]
        .iter()
        .any(|s| t.contains(s))
    {
        Category::Visit
    } else if [
        "mri",
        "ct scan",
        "x-ray",
        "radiology",
        "imaging",
        "impression:",
    ]
    .iter()
    .any(|s| t.contains(s))
    {
        Category::Imaging
    } else if ["prescribed", "medication", "mg daily", "tablet"]
        .iter()
        .any(|s| t.contains(s))
    {
        Category::Medication
    } else if [
        "physiotherapy",
        "physiotherapist",
        "treatment",
        "exercise program",
    ]
    .iter()
    .any(|s| t.contains(s))
    {
        Category::Treatment
    } else {
        Category::Other
    }
}

fn push_entry(
    entries: &mut Vec<Entry>,
    filename: &str,
    page_number: usize,
    date: Option<String>,
    quote: String,
    page_text: &str,
) {
    if quote.trim().is_empty() {
        return;
    }
    let summary = quote
        .lines()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("Review source page")
        .chars()
        .take(180)
        .collect();
    entries.push(Entry {
        id: Uuid::new_v4().simple().to_string()[..8].to_string(),
        date,
        category: classify(&quote),
        summary,
        source_file: filename.to_string(),
        page: page_number,
        verified: page_text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains(&quote.split_whitespace().collect::<Vec<_>>().join(" ")),
        quote,
        extractor: "rules".into(),
        status: Status::Pending,
        edited: false,
    });
}

pub fn entries_for_document(
    filename: &str,
    pages: &[String],
    names: &[String],
) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for (idx, raw) in pages.iter().enumerate() {
        if raw.trim().is_empty() {
            warnings.push(format!(
                "{filename}, page {}: no selectable text found; scanned PDFs need local OCR.",
                idx + 1
            ));
            continue;
        }
        let page = redact(raw, names);
        let lines: Vec<&str> = page.lines().map(str::trim).collect();
        let event_starts: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| starts_dated_event(line).then_some(i))
            .collect();
        if event_starts.is_empty() {
            // Undated page: preserve traceability rather than inventing a date.
            let quote = page.trim().chars().take(700).collect::<String>();
            push_entry(&mut entries, filename, idx + 1, None, quote, &page);
            continue;
        }
        for (n, start) in event_starts.iter().enumerate() {
            let end = event_starts.get(n + 1).copied().unwrap_or(lines.len());
            let block = lines[*start..end].join("\n");
            let quote = block.trim().chars().take(700).collect::<String>();
            push_entry(
                &mut entries,
                filename,
                idx + 1,
                find_date(lines[*start]),
                quote,
                &page,
            );
        }
    }
    (entries, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_dates_and_categories() {
        assert_eq!(
            find_date("Date of service: 2024-03-12"),
            Some("2024-03-12".into())
        );
        assert_eq!(classify("MRI lumbar spine report"), Category::Imaging);
        assert_eq!(find_date("March 18, 2024"), Some("2024-03-18".into()));
        assert_eq!(find_date("18 March 2024"), Some("2024-03-18".into()));
    }
    #[test]
    fn splits_multiple_dated_events_on_one_page() {
        let pages = vec!["Date of service: 2024-02-12\nFollow-up appointment\n\nDate of service: 2024-03-18\nAnother follow-up visit".to_string()];
        let (entries, warnings) = entries_for_document("test.pdf", &pages, &[]);
        assert!(warnings.is_empty());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].date.as_deref(), Some("2024-03-18"));
        assert!(entries
            .iter()
            .all(|e| e.verified && e.status == Status::Pending));
    }
}
