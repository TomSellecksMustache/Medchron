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
    #[serde(default)]
    pub documents: Vec<DocumentMetadata>,
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

// Profile data is stored locally. Patient ID/name may be OCR-derived and
// must be verified against the original record before professional reliance.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatientProfile {
    // Only derived from an explicitly labeled field; retained locally for grouping.
    #[serde(default)]
    pub patient_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub issues: String,
}

// OCR-derived suggestions are untrusted until explicitly confirmed in the PDF viewer.
// Document metadata stays in the local case.json; old cases deserialize with an empty list.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldConfirmations {
    #[serde(default)]
    pub name: bool,
    #[serde(default)]
    pub date: bool,
    #[serde(default)]
    pub phone: bool,
    #[serde(default)]
    pub patient_id: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentMetadata {
    pub filename: String,
    #[serde(default)]
    pub detected_name: String,
    #[serde(default)]
    pub detected_date: String,
    #[serde(default)]
    pub detected_phone: String,
    #[serde(default)]
    pub detected_patient_id: String,
    #[serde(default)]
    pub calendar_dates: Vec<String>,
    #[serde(default)]
    pub fields: FieldConfirmations,
    #[serde(default)]
    pub confirmed: bool,
    // Phone-only reassignment has been explicitly approved by a user.
    #[serde(default)]
    pub phone_is_provider: bool,
    #[serde(default)]
    pub phone_association_verified: bool,
    // Preserve the difference between OCR auto-matching and human review.
    #[serde(default)]
    pub auto_confirmed: bool,
    #[serde(default)]
    pub confirmation_touched: bool,
    // Transient suggestion populated when retrieving an Unassigned document.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suggested_case_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suggested_person_name: String,
}

#[derive(Debug, Deserialize)]
pub struct DocumentConfirmation {
    pub fields: FieldConfirmations,
}

#[derive(Debug, Deserialize)]
pub struct DocumentMove {
    pub target_case_id: String,
    // Only true for an explicit confirmation of a proposed phone match.
    #[serde(default)]
    pub confirm_phone: bool,
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
