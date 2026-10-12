use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Visit,
    Diagnosis,
    Imaging,
    Medication,
    Treatment,
    Insurer,
    Other,
}
impl Default for Category {
    fn default() -> Self {
        Self::Other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Accepted,
    Rejected,
}
impl Default for Status {
    fn default() -> Self {
        Self::Pending
    }
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
