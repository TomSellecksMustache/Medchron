use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Visit,
    Diagnosis,
    Imaging,
    Medication,
    Treatment,
    Insurer,
    #[default]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub date: Option<String>,
    pub category: Category,
    pub summary: String,
    pub source_file: String,
    pub page: usize,
    pub quote: String,
    pub verified: bool,
    pub extractor: String,
    pub status: Status,
    pub edited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    // Backwards-compatible: cases created before profiles existed deserialize
    // with an empty profile rather than becoming unreadable.
    #[serde(default)]
    pub patient: PatientProfile,
    pub id: String,
    pub created: DateTime<Utc>,
    pub files: Vec<String>,
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct EntryPatch {
    pub status: Option<Status>,
    pub date: Option<Option<String>>,
    pub category: Option<Category>,
    pub summary: Option<String>,
}

// User-verified information only: never infer identity from extracted PDF text.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatientProfile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub issues: String,
}

#[derive(Debug, Serialize)]
pub struct CaseSummary {
    pub id: String,
    pub name: String,
    pub created: DateTime<Utc>,
    pub file_count: usize,
    pub entry_count: usize,
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn old_case_without_patient_is_readable() {
        let old = r#"{"id":"abc123","created":"2026-10-01T12:00:00Z","files":[],"entries":[],"warnings":[]}"#;
        let case: Case = serde_json::from_str(old).expect("legacy case should deserialize");
        assert!(case.patient.name.is_empty());
        assert!(case.patient.address.is_empty());
    }
}
