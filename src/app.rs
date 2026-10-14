use crate::{
    chronology::{entries_for_document, MAX_PDF_BYTES},
    models::{
        Case, CaseSummary, DocumentConfirmation, DocumentMetadata, DocumentMove, EntryPatch,
        PatientProfile, Status,
    },
    storage::Store,
};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tower_http::limit::RequestBodyLimitLayer;
use uuid::Uuid;

struct AppState {
    store: Store,
    move_lock: tokio::sync::Mutex<()>,
    data_dir: PathBuf,
}
#[derive(Deserialize)]
struct ExportQuery {
    include: Option<String>,
}

pub fn router(data_dir: PathBuf) -> Router {
    let state = Arc::new(AppState {
        store: Store::new(data_dir.clone()),
        move_lock: tokio::sync::Mutex::new(()),
        data_dir,
    });
    Router::new()
        .route("/", get(index))
        .route("/assets/medchron-mark.png", get(brand_mark))
        .route("/assets/medchron-wordmark.png", get(brand_wordmark))
        .route("/api/calendar", get(calendar_summary))
        .route("/api/cases", get(list_cases).post(create_case))
        .route("/api/intake/analyze", axum::routing::post(analyze_batch))
        .route("/api/intake/import", axum::routing::post(import_batch))
        .route("/api/intake/auto", axum::routing::post(auto_import))
        .route("/api/samples", get(list_samples))
        .route("/api/samples/:filename", get(get_sample_file))
        .route("/api/cases/:id", get(get_case))
        .route("/api/cases/:id/profile", patch(patch_profile))
        .route("/api/cases/:id/entries/:entry_id", patch(patch_entry))
        .route("/api/cases/:id/files/:filename", get(get_file))
        .route("/api/cases/:id/documents/:filename", get(get_document))
        .route(
            "/api/cases/:id/documents/:filename/confirm",
            patch(confirm_document),
        )
        .route(
            "/api/cases/:id/documents/:filename/move",
            post(move_document),
        )
        .route("/api/cases/:id/export.csv", get(export_csv))
        .route("/api/cases/:id/export.md", get(export_md))
        .with_state(state)
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(100 * 1024 * 1024))
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

fn png(bytes: &'static [u8]) -> Response {
    no_store(([(header::CONTENT_TYPE, "image/png")], bytes).into_response())
}
async fn brand_mark() -> Response {
    png(include_bytes!("../static/images/medchron-mark.png"))
}
async fn brand_wordmark() -> Response {
    png(include_bytes!("../static/images/medchron-wordmark.png"))
}

async fn create_case(State(state): State<Arc<AppState>>, mut multipart: Multipart) -> Response {
    let id = Uuid::new_v4().simple().to_string();
    let mut names = Vec::<String>::new();
    let mut profile = PatientProfile::default();
    let mut uploads: Vec<(String, Vec<u8>)> = Vec::new();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid multipart form"),
        };
        let field_name = field.name().unwrap_or("").to_string();
        if field_name == "patient_name"
            || field_name == "patient_address"
            || field_name == "patient_issues"
        {
            let value = match field.text().await {
                Ok(t) => t,
                Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid profile field"),
            };
            match field_name.as_str() {
                "patient_name" => profile.name = value,
                "patient_address" => profile.address = value,
                _ => profile.issues = value,
            }
        } else if field_name == "patient_names" {
            let text = field.text().await.unwrap_or_default();
            names.extend(
                text.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            );
        } else if field_name == "files" {
            let filename = field.file_name().unwrap_or("upload.pdf").to_string();
            let safe_name = Path::new(&filename)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("upload.pdf")
                .to_string();
            if !safe_name.to_lowercase().ends_with(".pdf") {
                return error(StatusCode::BAD_REQUEST, "Only PDF files are accepted");
            }
            let bytes = match field.bytes().await {
                Ok(b) => b.to_vec(),
                Err(_) => return error(StatusCode::BAD_REQUEST, "Could not read uploaded file"),
            };
            if bytes.len() > MAX_PDF_BYTES {
                return error(StatusCode::PAYLOAD_TOO_LARGE, "PDF exceeds 25 MB");
            }
            if !bytes.starts_with(b"%PDF") {
                return error(
                    StatusCode::BAD_REQUEST,
                    "File does not have a PDF signature",
                );
            }
            uploads.push((safe_name, bytes));
        }
    }
    if uploads.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Select at least one PDF");
    }
    let profile = match validate_profile(profile) {
        Ok(p) => p,
        Err(m) => return error(StatusCode::BAD_REQUEST, m),
    };
    let dir = match state.store.case_dir(&id) {
        Ok(p) => p,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Storage error"),
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not create case folder",
        );
    }
    if crate::storage::restrict_permissions(&dir).is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not restrict case directory permissions",
        );
    }
    let mut case = Case {
        id: id.clone(),
        created: Utc::now(),
        patient: profile,
        files: vec![],
        documents: vec![],
        entries: vec![],
        warnings: vec![],
    };
    for (filename, bytes) in uploads {
        let filename = unique_filename(&filename, &case.files);
        let path = dir.join(&filename);
        if crate::crypto::write(&path, &bytes).is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Could not store PDF");
        }
        if crate::storage::restrict_permissions(&path).is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not restrict PDF permissions",
            );
        }
        match crate::ocr::extract_pdf(&path, false) {
            Ok(extracted) => {
                let (mut entries, mut warnings) =
                    entries_for_document(&filename, &extracted.pages, &names);
                if extracted.ocr_pages > 0 {
                    for entry in &mut entries {
                        entry.verified = false;
                        entry.extractor = "rules+ocr".into();
                    }
                    case.warnings.push(format!("{filename}: OCR used on {} pages; check all quotations against source images", extracted.ocr_pages));
                }
                case.warnings.extend(
                    extracted
                        .warnings
                        .into_iter()
                        .map(|w| format!("{filename}: {w}")),
                );
                case.documents
                    .push(document_metadata(&filename, &extracted.pages, &entries));
                case.entries.append(&mut entries);
                case.warnings.append(&mut warnings);
            }
            Err(_) => {
                let _ = std::fs::remove_dir_all(&dir);
                return error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Could not extract PDF text",
                );
            }
        }
        case.files.push(filename);
    }
    case.entries.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then(a.source_file.cmp(&b.source_file))
            .then(a.page.cmp(&b.page))
    });
    if state.store.save(&case).is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save case metadata",
        );
    }
    no_store(Json(case).into_response())
}

async fn list_cases(State(s): State<Arc<AppState>>) -> Response {
    match s.store.list() {
        Ok(cases) => {
            let summaries: Vec<CaseSummary> = cases
                .into_iter()
                .map(|c| CaseSummary {
                    id: c.id,
                    name: c.patient.name,
                    created: c.created,
                    file_count: c.files.len(),
                    entry_count: c.entries.len(),
                })
                .collect();
            no_store(Json(summaries).into_response())
        }
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Could not list cases"),
    }
}

async fn patch_profile(
    State(s): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(profile): Json<PatientProfile>,
) -> Response {
    let profile = match validate_profile(profile) {
        Ok(p) => p,
        Err(m) => return error(StatusCode::BAD_REQUEST, m),
    };
    let mut case = match s.store.load(&id) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::NOT_FOUND, "Case not found"),
    };
    let mut profile = profile;
    // Preserve the document-derived key when a user edits the visible profile.
    profile.patient_id = case.patient.patient_id.clone();
    case.patient = profile;
    if s.store.save(&case).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "Could not save profile");
    }
    no_store(Json(case).into_response())
}

fn validate_profile(mut profile: PatientProfile) -> Result<PatientProfile, &'static str> {
    profile.name = profile.name.trim().to_string();
    profile.address = profile.address.trim().to_string();
    profile.issues = profile.issues.trim().to_string();
    profile.patient_id = profile.patient_id.trim().to_string();
    if profile.name.chars().count() > 120 || profile.name.chars().any(char::is_control) {
        return Err("Name must be at most 120 characters and contain no control characters");
    }
    if profile.patient_id.len() > 64
        || !profile.patient_id.is_ascii()
        || profile.patient_id.chars().any(char::is_control)
    {
        return Err("Patient identifier is invalid");
    }
    if profile.address.chars().count() > 400 || profile.issues.chars().count() > 2000 {
        return Err("Address or injury/issues field is too long");
    }
    Ok(profile)
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        header::HeaderValue::from_static("nosniff"),
    );
    response
}

async fn get_case(State(s): State<Arc<AppState>>, AxumPath(id): AxumPath<String>) -> Response {
    match s.store.load(&id) {
        Ok(c) => no_store(Json(c).into_response()),
        Err(_) => error(StatusCode::NOT_FOUND, "Case not found"),
    }
}

async fn patch_entry(
    State(s): State<Arc<AppState>>,
    AxumPath((id, entry_id)): AxumPath<(String, String)>,
    Json(patch): Json<EntryPatch>,
) -> Response {
    let mut case = match s.store.load(&id) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::NOT_FOUND, "Case not found"),
    };
    let Some(entry) = case.entries.iter_mut().find(|e| e.id == entry_id) else {
        return error(StatusCode::NOT_FOUND, "Entry not found");
    };
    if let Some(status) = patch.status {
        entry.status = status;
    }
    if let Some(date) = patch.date {
        if let Some(ref d) = date {
            if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
                return error(StatusCode::UNPROCESSABLE_ENTITY, "Date must be YYYY-MM-DD");
            }
        }
        entry.date = date;
        entry.edited = true;
    }
    if let Some(category) = patch.category {
        entry.category = category;
        entry.edited = true;
    }
    if let Some(summary) = patch.summary {
        if summary.trim().is_empty() {
            return error(StatusCode::UNPROCESSABLE_ENTITY, "Summary cannot be empty");
        }
        entry.summary = summary.trim().to_string();
        entry.edited = true;
    }
    let result = entry.clone();
    if s.store.save(&case).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "Could not save changes");
    }
    no_store(Json(result).into_response())
}

async fn get_file(
    State(s): State<Arc<AppState>>,
    AxumPath((id, filename)): AxumPath<(String, String)>,
) -> Response {
    let case = match s.store.load(&id) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::NOT_FOUND, "Case not found"),
    };
    if !case.files.contains(&filename) {
        return error(StatusCode::NOT_FOUND, "File not found");
    }
    let path = match s.store.case_dir(&id) {
        Ok(p) => p.join(filename),
        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid path"),
    };
    match crate::crypto::read(&path) {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "application/pdf")
            .header(header::CACHE_CONTROL, "no-store")
            .header("x-content-type-options", "nosniff")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Response error")),
        Err(_) => error(StatusCode::NOT_FOUND, "File not found"),
    }
}

// Normalize NANP phone numbers without retaining punctuation or adding network calls.
// Reject short, partial and international numbers rather than fuzzy-matching them.
fn normalize_patient_phone(input: &str) -> Option<String> {
    let digits: String = input.chars().filter(char::is_ascii_digit).collect();
    let value = if digits.len() == 11 && digits.starts_with('1') {
        &digits[1..]
    } else {
        digits.as_str()
    };
    (value.len() == 10).then(|| value.to_string())
}

// The optional provider-number whitelist is local configuration, not a source
// of patient identities. Each matching number is excluded from person matching.
// On Windows it lives at %LOCALAPPDATA%\\Medchron\\data\\provider-phones.txt.
// The caller can override it with MEDCHRON_PROVIDER_PHONES_FILE.
fn provider_phone_numbers(data_dir: &Path) -> Result<std::collections::BTreeSet<String>, String> {
    let path = std::env::var_os("MEDCHRON_PROVIDER_PHONES_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("provider-phones.txt"));
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(_) => return Err("Could not read the local provider phone whitelist".into()),
    };
    let mut numbers = std::collections::BTreeSet::new();
    for line in content.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(number) = normalize_patient_phone(line) else {
            return Err(
                "Invalid number in provider-phones.txt; use one 10-digit number per line".into(),
            );
        };
        numbers.insert(number);
    }
    Ok(numbers)
}

// Return a unique local person with the same detected phone. Only identified
// cases with a name/ID-confirmed document can nominate a person. The phone
// itself remains UNVERIFIED until a user explicitly confirms the match.
fn phone_match_candidate(
    cases: &[Case],
    detected_phone: &str,
    provider_numbers: &std::collections::BTreeSet<String>,
) -> Option<(String, String)> {
    let normalized = normalize_patient_phone(detected_phone)?;
    if provider_numbers.contains(&normalized) {
        return None;
    }
    let mut candidates = std::collections::BTreeMap::new();
    for case in cases {
        if case.patient.name.trim().is_empty() {
            continue;
        }
        for document in &case.documents {
            if !document.confirmed
                || document.phone_is_provider
                || !(document.fields.name || document.fields.patient_id)
            {
                continue;
            }
            if normalize_patient_phone(&document.detected_phone).as_deref()
                == Some(normalized.as_str())
            {
                candidates.insert(case.id.clone(), case.patient.name.clone());
            }
        }
    }
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

// Calendar is a read-only projection of local case metadata. No dates or PII
// are sent to any third-party calendar service.
async fn calendar_summary(State(s): State<Arc<AppState>>) -> Response {
    let cases = match s.store.list() {
        Ok(cases) => cases,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Could not load calendar"),
    };
    let mut dates = std::collections::BTreeSet::<String>::new();
    for c in cases {
        for e in c.entries {
            if let Some(d) = e.date {
                dates.insert(d);
            }
        }
        for doc in c.documents {
            dates.extend(doc.calendar_dates);
        }
    }
    no_store(Json(dates.into_iter().collect::<Vec<_>>()).into_response())
}

fn normalize_date(raw: &str) -> Option<String> {
    for format in ["%Y-%m-%d", "%Y/%m/%d", "%m/%d/%Y", "%d/%m/%Y"] {
        if let Ok(day) = chrono::NaiveDate::parse_from_str(raw, format) {
            if (1900..=2100).contains(&chrono::Datelike::year(&day)) {
                return Some(day.format("%Y-%m-%d").to_string());
            }
        }
    }
    None
}

fn document_metadata(
    filename: &str,
    pages: &[String],
    entries: &[crate::models::Entry],
) -> DocumentMetadata {
    document_metadata_with_provider_numbers(filename, pages, entries, &Default::default())
}

fn document_metadata_with_provider_numbers(
    filename: &str,
    pages: &[String],
    entries: &[crate::models::Entry],
    provider_numbers: &std::collections::BTreeSet<String>,
) -> DocumentMetadata {
    let (name, patient_id) = auto_identity(pages);
    let phone_pattern = regex::Regex::new(
        r"(?im)^[ \t]*(provider\s+phone|clinic\s+phone|office\s+phone|patient\s+phone|phone(?:\s+number)?|telephone|contact\s+number)[ \t]*[:\-][ \t]*(?:\r?\n[ \t]*)?(\+?\(?\d[\d() .\-]{7,22}\d)"
    ).unwrap();
    let date_field = regex::Regex::new(
        r"(?im)^[ \t]*(?:record\s+date|date\s+of\s+service|service\s+date|visit\s+date|document\s+date|date\s+of\s+study)[ \t]*[:\-][ \t]*(?:\r?\n[ \t]*)?((?:19|20)\d{2}[-/]\d{1,2}[-/]\d{1,2}|\d{1,2}/\d{1,2}/(?:19|20)\d{2})"
    ).unwrap();
    let all_dates = regex::Regex::new(
        r"\b((?:19|20)\d{2}[-/]\d{1,2}[-/]\d{1,2}|\d{1,2}/\d{1,2}/(?:19|20)\d{2})\b",
    )
    .unwrap();
    let mut patient_phones = std::collections::BTreeSet::new();
    let mut provider_phones = std::collections::BTreeSet::new();
    let mut provider_labeled_numbers = std::collections::BTreeSet::new();
    let mut labeled_dates = std::collections::BTreeSet::new();
    let mut dates = std::collections::BTreeSet::new();
    for page in pages {
        for match_ in phone_pattern.captures_iter(page) {
            if let Some(normalized) = normalize_patient_phone(match_[2].trim()) {
                let label = match_[1].to_ascii_lowercase();
                let value = match_[2].trim().to_string();
                if label.starts_with("provider")
                    || label.starts_with("clinic")
                    || label.starts_with("office")
                {
                    provider_labeled_numbers.insert(normalized);
                    provider_phones.insert(value);
                } else if !provider_numbers.contains(&normalized) {
                    patient_phones.insert(value);
                } else {
                    provider_phones.insert(value);
                }
            }
        }
        for match_ in date_field.captures_iter(page) {
            if let Some(d) = normalize_date(&match_[1]) {
                labeled_dates.insert(d);
            }
        }
        for match_ in all_dates.captures_iter(page) {
            if let Some(d) = normalize_date(&match_[1]) {
                dates.insert(d);
            }
        }
    }
    for entry in entries {
        if let Some(d) = entry.date.as_ref().and_then(|d| normalize_date(d)) {
            dates.insert(d);
        }
    }
    // A document may contain both a clinic phone and a patient phone.
    // Prefer one unambiguous patient-labeled number; never use a provider
    // number for identity matching.
    let (phone, phone_is_provider) = if patient_phones.len() == 1 {
        (patient_phones.into_iter().next().unwrap_or_default(), false)
    } else if patient_phones.is_empty() && provider_phones.len() == 1 {
        (provider_phones.into_iter().next().unwrap_or_default(), true)
    } else {
        (String::new(), false)
    };
    let phone_is_provider = phone_is_provider
        || normalize_patient_phone(&phone).is_some_and(|number| {
            provider_numbers.contains(&number) || provider_labeled_numbers.contains(&number)
        });
    DocumentMetadata {
        filename: filename.into(),
        detected_name: name.unwrap_or_default(),
        detected_date: labeled_dates
            .iter()
            .next()
            .or_else(|| dates.iter().next())
            .cloned()
            .unwrap_or_default(),
        detected_phone: phone,
        detected_patient_id: patient_id.unwrap_or_default(),
        calendar_dates: dates.into_iter().collect(),
        phone_is_provider,
        fields: crate::models::FieldConfirmations {
            phone: phone_is_provider,
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn get_document(
    State(s): State<Arc<AppState>>,
    AxumPath((id, filename)): AxumPath<(String, String)>,
) -> Response {
    let store = s.store.clone();
    let data_dir = s.data_dir.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<DocumentMetadata, String> {
        let mut case = store.load(&id).map_err(|_| "Person not found".to_owned())?;
        if !case.files.contains(&filename) {
            return Err("Document not found".into());
        }
        let provider_numbers = provider_phone_numbers(&data_dir)?;
        let mut metadata =
            if let Some(existing) = case.documents.iter().find(|d| d.filename == filename) {
                existing.clone()
            } else {
                // Backward-compatible: older cases have no document metadata.
                let source = store
                    .case_dir(&id)
                    .map_err(|_| "Invalid case".to_owned())?
                    .join(&filename);
                let extracted = crate::ocr::extract_pdf(&source, false)
                    .map_err(|_| "Could not extract document".to_owned())?;
                let candidates: Vec<_> = case
                    .entries
                    .iter()
                    .filter(|e| e.source_file == filename)
                    .cloned()
                    .collect();
                let generated = document_metadata_with_provider_numbers(
                    &filename,
                    &extracted.pages,
                    &candidates,
                    &provider_numbers,
                );
                case.documents.push(generated.clone());
                store
                    .save(&case)
                    .map_err(|_| "Could not save document metadata".to_owned())?;
                generated
            };
        // A phone alone may suggest an existing person, but never merges on
        // its own. Explicit PDF-viewer confirmation is required.
        if case.patient.name.is_empty()
            && metadata.detected_name.is_empty()
            && metadata.detected_patient_id.is_empty()
            && !metadata.detected_phone.is_empty()
        {
            if let Some((target_id, target_name)) = phone_match_candidate(
                &store
                    .list()
                    .map_err(|_| "Could not load people".to_owned())?,
                &metadata.detected_phone,
                &provider_numbers,
            ) {
                metadata.suggested_case_id = target_id;
                metadata.suggested_person_name = target_name;
            }
        }
        Ok(metadata)
    })
    .await;
    match result {
        Ok(Ok(document)) => no_store(Json(document).into_response()),
        Ok(Err(message)) => error(StatusCode::UNPROCESSABLE_ENTITY, &message),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Document metadata failed",
        ),
    }
}

async fn confirm_document(
    State(s): State<Arc<AppState>>,
    AxumPath((id, filename)): AxumPath<(String, String)>,
    Json(update): Json<DocumentConfirmation>,
) -> Response {
    let mut case = match s.store.load(&id) {
        Ok(case) => case,
        Err(_) => return error(StatusCode::NOT_FOUND, "Person not found"),
    };
    if !case.files.contains(&filename) {
        return error(StatusCode::NOT_FOUND, "Document not found");
    }
    let Some(document) = case.documents.iter_mut().find(|d| d.filename == filename) else {
        return error(
            StatusCode::CONFLICT,
            "Open PDF preview once to extract metadata first",
        );
    };
    let checks = &update.fields;
    if (checks.name && document.detected_name.is_empty())
        || (checks.date && document.detected_date.is_empty())
        || (checks.phone && document.detected_phone.is_empty())
        || (checks.patient_id && document.detected_patient_id.is_empty())
    {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Cannot confirm a missing extracted value",
        );
    }
    if checks.name
        && !case.patient.name.is_empty()
        && !case
            .patient
            .name
            .eq_ignore_ascii_case(&document.detected_name)
    {
        return error(
            StatusCode::CONFLICT,
            "Detected name does not match selected person; uncheck it or reassign document",
        );
    }
    if checks.patient_id
        && !case.patient.patient_id.is_empty()
        && !case
            .patient
            .patient_id
            .eq_ignore_ascii_case(&document.detected_patient_id)
    {
        return error(
            StatusCode::CONFLICT,
            "Detected ID does not match selected person",
        );
    }
    let identity_confirmed =
        checks.name || checks.patient_id || (checks.phone && document.phone_association_verified);
    document.fields = update.fields;
    document.auto_confirmed = false;
    document.confirmation_touched = true;
    // Confirming metadata is not the same as accepting an extracted chronology event.
    // A document is identity-confirmed only after confirming a name or patient ID.
    document.confirmed = identity_confirmed;
    let result = document.clone();
    if s.store.save(&case).is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save document verification",
        );
    }
    no_store(Json(result).into_response())
}

// Explicit manual reassignment is restricted to Unassigned cases. Moving a
// source PDF never accepts chronology entries or automatically verifies identity.
async fn move_document(
    State(s): State<Arc<AppState>>,
    AxumPath((source_id, filename)): AxumPath<(String, String)>,
    Json(request): Json<DocumentMove>,
) -> Response {
    let _guard = s.move_lock.lock().await;
    let store = &s.store;
    if source_id == request.target_case_id {
        return error(StatusCode::BAD_REQUEST, "Choose another person");
    }
    let mut source = match store.load(&source_id) {
        Ok(v) => v,
        Err(_) => return error(StatusCode::NOT_FOUND, "Unassigned case not found"),
    };
    if !source.patient.name.trim().is_empty() {
        return error(
            StatusCode::CONFLICT,
            "Only documents in Unassigned may be dragged to another person",
        );
    }
    if !source.files.contains(&filename) {
        return error(StatusCode::NOT_FOUND, "Source document not found");
    }
    let mut target = match store.load(&request.target_case_id) {
        Ok(v) => v,
        Err(_) => return error(StatusCode::NOT_FOUND, "Destination person not found"),
    };
    if target.patient.name.trim().is_empty() {
        return error(
            StatusCode::CONFLICT,
            "Destination must be an identified person",
        );
    }
    if request.confirm_phone {
        let Some(metadata) = source.documents.iter().find(|d| d.filename == filename) else {
            return error(StatusCode::CONFLICT, "Document metadata is missing");
        };
        if !metadata.detected_name.is_empty() || !metadata.detected_patient_id.is_empty() {
            return error(
                StatusCode::CONFLICT,
                "Phone-only confirmation requires a document without name or patient ID",
            );
        }
        if metadata.phone_is_provider {
            return error(
                StatusCode::CONFLICT,
                "Provider phones cannot identify a patient",
            );
        }
        let providers = match provider_phone_numbers(&s.data_dir) {
            Ok(p) => p,
            Err(msg) => return error(StatusCode::UNPROCESSABLE_ENTITY, &msg),
        };
        let cases = match store.list() {
            Ok(c) => c,
            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not validate phone match",
                )
            }
        };
        if phone_match_candidate(&cases, &metadata.detected_phone, &providers)
            .is_none_or(|(id, _)| id != target.id)
        {
            return error(
                StatusCode::CONFLICT,
                "No unique eligible phone match; move manually and verify identity",
            );
        }
    }
    let src_dir = match store.case_dir(&source_id) {
        Ok(p) => p,
        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid source"),
    };
    let dst_dir = match store.case_dir(&request.target_case_id) {
        Ok(p) => p,
        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid destination"),
    };
    let mut used = target.files.clone();
    let new_name = loop {
        let candidate = unique_filename(&filename, &used);
        if !dst_dir.join(&candidate).exists() {
            break candidate;
        }
        used.push(candidate);
    };
    let from = src_dir.join(&filename);
    let to = dst_dir.join(&new_name);
    let bytes = match crate::crypto::read(&from) {
        Ok(b) => b,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Cannot read source PDF"),
    };
    if std::fs::create_dir_all(&dst_dir).is_err()
        || crate::storage::restrict_permissions(&dst_dir).is_err()
    {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Cannot access destination folder",
        );
    }
    if crate::crypto::write(&to, &bytes).is_err()
        || crate::storage::restrict_permissions(&to).is_err()
    {
        let _ = std::fs::remove_file(&to);
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Cannot stage destination PDF",
        );
    }
    let mut metadata = source
        .documents
        .iter()
        .find(|d| d.filename == filename)
        .cloned()
        .unwrap_or_else(|| DocumentMetadata {
            filename: new_name.clone(),
            ..Default::default()
        });
    metadata.filename = new_name.clone();
    metadata.fields = Default::default();
    metadata.phone_association_verified = request.confirm_phone;
    metadata.auto_confirmed = false;
    metadata.confirmation_touched = true;
    metadata.fields.phone = request.confirm_phone;
    metadata.confirmed = request.confirm_phone;
    metadata.suggested_case_id.clear();
    metadata.suggested_person_name.clear();
    // Manual drag remains unverified; an explicitly approved, uniquely matched
    // patient phone is verified only after user action.
    target.documents.push(metadata);
    target.files.push(new_name.clone());
    for entry in source.entries.iter().filter(|e| e.source_file == filename) {
        let mut moved = entry.clone();
        moved.source_file = new_name.clone();
        moved.status = Status::Pending;
        moved.verified = false;
        target.entries.push(moved);
    }
    target
        .entries
        .sort_by(|a, b| a.date.cmp(&b.date).then(a.source_file.cmp(&b.source_file)));
    if store.save(&target).is_err() {
        let _ = std::fs::remove_file(&to);
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Cannot save destination case",
        );
    }
    source.files.retain(|f| f != &filename);
    source.documents.retain(|d| d.filename != filename);
    source.entries.retain(|e| e.source_file != filename);
    if store.save(&source).is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not complete move; source retained for recovery",
        );
    }
    let _ = std::fs::remove_file(&from);
    no_store(Json(serde_json::json!({"target_case_id":target.id, "filename":new_name, "verified":request.confirm_phone})).into_response())
}

async fn export_csv(
    State(s): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let case = match s.store.load(&id) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::NOT_FOUND, "Case not found"),
    };
    let mut wtr = csv::Writer::from_writer(vec![]);
    let _ = wtr.write_record([
        "date",
        "category",
        "summary",
        "source_file",
        "page",
        "quote",
        "verified",
        "status",
        "edited",
    ]);
    for e in case
        .entries
        .iter()
        .filter(|e| q.include.as_deref() == Some("all") || e.status == Status::Accepted)
    {
        let _ = wtr.write_record([
            e.date.as_deref().unwrap_or(""),
            category(&e.category),
            &safe_formula(&e.summary),
            &safe_formula(&e.source_file),
            &e.page.to_string(),
            &safe_formula(&e.quote),
            if e.verified { "true" } else { "false" },
            status(&e.status),
            if e.edited { "true" } else { "false" },
        ]);
    }
    let bytes = wtr.into_inner().unwrap_or_default();
    Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"chronology.csv\"",
        )
        .body(Body::from(bytes))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Export error"))
}
async fn export_md(
    State(s): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let case = match s.store.load(&id) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::NOT_FOUND, "Case not found"),
    };
    let mut md = String::from("# Medical Chronology\n\n> Draft for human review. Verify each entry against the original record.\n\n| Date | Category | Event | Source | Page | Status |\n|---|---|---|---|---:|---|\n");
    for e in case
        .entries
        .iter()
        .filter(|e| q.include.as_deref() == Some("all") || e.status == Status::Accepted)
    {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            e.date.as_deref().unwrap_or("Undated"),
            category(&e.category),
            e.summary.replace('|', "\\|").replace('\n', " "),
            e.source_file.replace('|', "\\|"),
            e.page,
            status(&e.status)
        ));
        md.push_str(&format!(
            "\n**Source quotation:** > {}\n\n",
            e.quote.replace('\n', " ").replace('>', "\\>")
        ));
    }
    Response::builder()
        .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"chronology.md\"",
        )
        .body(Body::from(md))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Export error"))
}
fn safe_formula(s: &str) -> String {
    if matches!(s.trim_start().chars().next(), Some('=' | '+' | '-' | '@'))
        || matches!(s.chars().next(), Some('\t' | '\r' | '\n'))
    {
        format!("'{}", s)
    } else {
        s.to_string()
    }
}
fn category(c: &crate::models::Category) -> &'static str {
    use crate::models::Category::*;
    match c {
        Visit => "visit",
        Diagnosis => "diagnosis",
        Imaging => "imaging",
        Medication => "medication",
        Treatment => "treatment",
        Insurer => "insurer",
        Other => "other",
    }
}
fn status(s: &Status) -> &'static str {
    match s {
        Status::Pending => "pending",
        Status::Accepted => "accepted",
        Status::Rejected => "rejected",
    }
}
fn error(code: StatusCode, message: &str) -> Response {
    (code, message.to_string()).into_response()
}

fn unique_filename(name: &str, existing: &[String]) -> String {
    if !existing.iter().any(|n| n == name) {
        return name.to_string();
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("upload");
    for n in 2..10000 {
        let candidate = format!("{stem}-{n}.pdf");
        if !existing.iter().any(|s| s == &candidate) {
            return candidate;
        }
    }
    format!("{}.pdf", Uuid::new_v4())
}
// This folder is local to the logged-in Windows user; no filesystem path is
// accepted from any incoming HTTP request. The Linux fallback helps local demos.
fn sample_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("MEDCHRON_SAMPLE_DIR") {
        return PathBuf::from(override_dir);
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("Medchron").join("sample");
    }
    PathBuf::from("samples")
}

async fn list_samples() -> Response {
    let directory = match std::fs::read_dir(sample_dir()) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Json(Vec::<String>::new()).into_response();
        }
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not list sample folder",
            )
        }
    };
    let mut names = Vec::new();
    for entry in directory.flatten() {
        // `is_file()` is false for symlinks, including links to outside this folder.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            if name.to_ascii_lowercase().ends_with(".pdf") {
                names.push(name.to_string());
            }
        }
    }
    names.sort_unstable();
    Json(names).into_response()
}

async fn get_sample_file(AxumPath(filename): AxumPath<String>) -> Response {
    // The route only serves direct PDF children of the fixed sample folder.
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename == "."
        || filename == ".."
        || !filename.to_ascii_lowercase().ends_with(".pdf")
    {
        return error(StatusCode::BAD_REQUEST, "Invalid sample filename");
    }
    let path = sample_dir().join(&filename);
    let info = match std::fs::symlink_metadata(&path) {
        Ok(info) if info.file_type().is_file() => info,
        _ => return error(StatusCode::NOT_FOUND, "Sample PDF not found"),
    };
    if info.len() > MAX_PDF_BYTES as u64 {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "Sample PDF exceeds 25 MB");
    }
    match tokio::fs::read(path).await {
        Ok(bytes) if bytes.starts_with(b"%PDF") => Response::builder()
            .header(header::CONTENT_TYPE, "application/pdf")
            .header(header::CACHE_CONTROL, "no-store")
            .header("x-content-type-options", "nosniff")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Response error")),
        _ => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Could not read sample PDF",
        ),
    }
}

// Batch intake: two-step process. Analysis NEVER creates people or cases.
// Confirmed import groups files by reviewed name; existing cases are not
// auto-merged based on an OCR guess (two people may share the same name).
#[derive(serde::Serialize)]
struct IntakePreview {
    file_index: usize,
    filename: String,
    suggested_name: Option<String>,
    evidence: &'static str,
    ocr_pages: usize,
    warning: Option<String>,
}

#[derive(serde::Deserialize)]
struct BatchDecisions {
    names: Vec<String>,
    #[serde(default)]
    case_ids: Vec<Option<String>>,
    confirmed: bool,
}

type Upload = (String, Vec<u8>);
type IntakeGroup = (String, Option<String>, Vec<Upload>);

async fn read_batch_uploads(
    mut multipart: Multipart,
) -> Result<(Vec<Upload>, Option<String>), (StatusCode, &'static str)> {
    let mut uploads = Vec::new();
    let mut decisions = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid multipart data"))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "files" => {
                if uploads.len() >= 30 {
                    return Err((StatusCode::PAYLOAD_TOO_LARGE, "Batch limit: 30 documents"));
                }
                let original = field.file_name().unwrap_or("upload.pdf");
                let filename = Path::new(original)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("upload.pdf")
                    .to_string();
                if !filename.to_ascii_lowercase().ends_with(".pdf") || filename == ".pdf" {
                    return Err((StatusCode::BAD_REQUEST, "Only PDF files are accepted"));
                }
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| (StatusCode::BAD_REQUEST, "Could not read PDF"))?;
                if data.len() > MAX_PDF_BYTES {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "One or more PDFs exceed 25 MB",
                    ));
                }
                if !data.starts_with(b"%PDF") {
                    return Err((StatusCode::BAD_REQUEST, "Invalid PDF signature"));
                }
                uploads.push((filename, data.to_vec()));
            }
            "decisions" => {
                if decisions.is_some() {
                    return Err((StatusCode::BAD_REQUEST, "Duplicate decisions field"));
                }
                decisions = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid decisions field"))?,
                );
            }
            _ => {}
        }
    }
    if uploads.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Select at least one PDF"));
    }
    Ok((uploads, decisions))
}

struct TempIntakePdf(PathBuf);
impl TempIntakePdf {
    fn save(bytes: &[u8]) -> std::io::Result<Self> {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("medchron-intake-{}.pdf", Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        crate::storage::restrict_permissions(&path)?;
        file.write_all(bytes)?;
        Ok(Self(path))
    }
}
impl Drop for TempIntakePdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn labeled_name(pages: &[String]) -> (Option<String>, &'static str) {
    let pattern = regex::Regex::new(
        r"(?im)^\s*(?:patient(?:\s+name)?|client(?:\s+name)?|name)\s*[:\-]\s*([^|\r\n]{3,120})",
    )
    .unwrap();
    let mut found = std::collections::BTreeSet::<String>::new();
    for page in pages {
        for captures in pattern.captures_iter(page) {
            let candidate = captures[1].split_whitespace().collect::<Vec<_>>().join(" ");
            if candidate.chars().count() <= 120
                && candidate.split_whitespace().count() >= 2
                && candidate
                    .chars()
                    .all(|ch| ch.is_alphabetic() || matches!(ch, ' ' | '-' | '\'' | '.'))
            {
                found.insert(candidate);
            }
        }
    }
    if found.len() == 1 {
        (
            found.into_iter().next(),
            "Labeled patient/client field — verify manually",
        )
    } else if found.len() > 1 {
        (None, "Conflicting names found — assign manually")
    } else {
        (
            None,
            "No explicit patient-name field found — assign manually",
        )
    }
}

async fn analyze_batch(multipart: Multipart) -> Response {
    let (uploads, _) = match read_batch_uploads(multipart).await {
        Ok(data) => data,
        Err(err) => return err.into_response(),
    };
    let result = tokio::task::spawn_blocking(move || {
        let mut previews = Vec::new();
        for (index, (filename, bytes)) in uploads.into_iter().enumerate() {
            let analysis = (|| -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
                let scratch = TempIntakePdf::save(&bytes)?;
                let text = crate::ocr::extract_pdf(&scratch.0, true)?;
                Ok(text)
            })();
            match analysis {
                Ok(text) => {
                    let (name, evidence) = labeled_name(&text.pages);
                    previews.push(IntakePreview {
                        file_index: index,
                        filename,
                        suggested_name: name,
                        evidence,
                        ocr_pages: text.ocr_pages,
                        warning: (!text.warnings.is_empty()).then(|| text.warnings.join("; ")),
                    });
                }
                Err(_) => previews.push(IntakePreview {
                    file_index: index,
                    filename,
                    suggested_name: None,
                    evidence: "Could not analyze PDF — verify before assigning",
                    ocr_pages: 0,
                    warning: Some("PDF could not be analyzed locally".into()),
                }),
            }
        }
        previews
    })
    .await;
    match result {
        Ok(previews) => no_store(Json(previews).into_response()),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Batch analysis failed"),
    }
}

async fn import_batch(State(state): State<Arc<AppState>>, multipart: Multipart) -> Response {
    let (uploads, raw) = match read_batch_uploads(multipart).await {
        Ok(data) => data,
        Err(err) => return err.into_response(),
    };
    let Some(raw) = raw else {
        return error(
            StatusCode::BAD_REQUEST,
            "Missing reviewed identity assignments",
        );
    };
    let decisions: BatchDecisions = match serde_json::from_str(&raw) {
        Ok(d) => d,
        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid assignment JSON"),
    };
    if !decisions.confirmed || decisions.names.len() != uploads.len() {
        return error(
            StatusCode::BAD_REQUEST,
            "Review and confirm an identity for every PDF",
        );
    }
    let mut targets = Vec::new();
    if !decisions.case_ids.is_empty() && decisions.case_ids.len() != uploads.len() {
        return error(
            StatusCode::BAD_REQUEST,
            "Invalid existing-person selections",
        );
    }
    let case_ids = if decisions.case_ids.is_empty() {
        vec![None; uploads.len()]
    } else {
        decisions.case_ids
    };
    for (name, case_id) in decisions.names.into_iter().zip(case_ids) {
        let profile = match validate_profile(PatientProfile {
            name,
            ..Default::default()
        }) {
            Ok(profile) if !profile.name.is_empty() => profile,
            _ => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "Every document requires a valid confirmed person's name",
                )
            }
        };
        targets.push((profile.name, case_id));
    }
    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<CaseSummary>, String> {
        let mut groups: std::collections::BTreeMap<String, IntakeGroup> =
            std::collections::BTreeMap::new();
        for ((filename, bytes), (name, existing)) in uploads.into_iter().zip(targets) {
            // Existing records only merge when explicitly selected by case ID.
            let key = existing.as_ref().map_or_else(
                || format!("new:{}", name.to_lowercase()),
                |id| format!("existing:{id}"),
            );
            let group = groups
                .entry(key)
                .or_insert_with(|| (name.clone(), existing.clone(), Vec::new()));
            if group.0 != name {
                return Err("Conflicting names for one selected person".into());
            }
            group.2.push((filename, bytes));
        }
        // Pre-process all PDFs before writing any new cases. A document with
        // broken structure cannot silently create an empty person's folder.
        let mut prepared = Vec::new();
        for (_, (name, existing, documents)) in groups {
            let mut files = Vec::new();
            for (filename, bytes) in documents {
                let scratch = TempIntakePdf::save(&bytes)
                    .map_err(|_| "Could not stage local PDF".to_string())?;
                let extracted = crate::ocr::extract_pdf(&scratch.0, false)
                    .map_err(|_| "Could not extract PDF pages".to_string())?;
                files.push((filename, bytes, extracted));
            }
            prepared.push((name, existing, files));
        }
        let mut summaries = Vec::new();
        for (name, existing, documents) in prepared {
            let (id, mut case) = if let Some(existing_id) = existing {
                let case = store
                    .load(&existing_id)
                    .map_err(|_| "Selected existing person not found".to_string())?;
                if case.patient.name.trim() != name {
                    return Err("Selected person name no longer matches their saved profile".into());
                }
                (existing_id, case)
            } else {
                let id = Uuid::new_v4().simple().to_string();
                let case = Case {
                    id: id.clone(),
                    created: Utc::now(),
                    patient: PatientProfile {
                        name: name.clone(),
                        ..Default::default()
                    },
                    files: vec![],
                    documents: vec![],
                    entries: vec![],
                    warnings: vec![],
                };
                (id, case)
            };
            let dir = store
                .case_dir(&id)
                .map_err(|_| "Invalid case folder".to_string())?;
            std::fs::create_dir_all(&dir)
                .map_err(|_| "Could not create local case folder".to_string())?;
            crate::storage::restrict_permissions(&dir)
                .map_err(|_| "Could not protect case folder".to_string())?;
            for (filename, bytes, extracted) in documents {
                let filename = unique_filename(&filename, &case.files);
                let destination = dir.join(&filename);
                crate::crypto::write(&destination, &bytes)
                    .map_err(|_| "Could not save local PDF".to_string())?;
                crate::storage::restrict_permissions(&destination)
                    .map_err(|_| "Could not restrict PDF permissions".to_string())?;
                let (mut entries, mut warnings) =
                    entries_for_document(&filename, &extracted.pages, std::slice::from_ref(&name));
                if extracted.ocr_pages > 0 {
                    for entry in &mut entries {
                        entry.verified = false;
                        entry.extractor = "rules+ocr".into();
                    }
                    case.warnings.push(format!(
                        "{filename}: OCR used on {} pages; verify the source images",
                        extracted.ocr_pages
                    ));
                }
                case.documents
                    .push(document_metadata(&filename, &extracted.pages, &entries));
                case.entries.append(&mut entries);
                case.warnings.append(&mut warnings);
                for w in extracted.warnings {
                    case.warnings.push(format!("{filename}: {w}"));
                }
                case.files.push(filename);
            }
            case.entries.sort_by(|a, b| {
                a.date
                    .cmp(&b.date)
                    .then(a.source_file.cmp(&b.source_file))
                    .then(a.page.cmp(&b.page))
            });
            store
                .save(&case)
                .map_err(|_| "Could not save local case metadata".to_string())?;
            summaries.push(CaseSummary {
                id,
                name,
                file_count: case.files.len(),
                entry_count: case.entries.len(),
                created: case.created,
            });
        }
        Ok(summaries)
    })
    .await;
    match result {
        Ok(Ok(summaries)) => no_store(Json(summaries).into_response()),
        Ok(Err(message)) => error(StatusCode::UNPROCESSABLE_ENTITY, &message),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Local batch processing failed",
        ),
    }
}

// Automatic local intake. Group on labeled identifiers, never on filenames.
// If identity evidence is missing or contradictory, create an isolated
// Unassigned case rather than attaching a record to the wrong person.
fn labeled_patient_id(pages: &[String]) -> Option<String> {
    let pattern = regex::Regex::new(
        r"(?im)^\s*(?:patient\s*(?:id|number)|medical\s*record\s*(?:number|no\.?|#)|mrn|health\s*(?:care\s*)?card\s*(?:number|no\.?|#)?)\s*[:\-]\s*([A-Za-z0-9][A-Za-z0-9_-]{2,63})\s*$",
    )
    .unwrap();
    let mut ids = std::collections::BTreeSet::new();
    for page in pages {
        for captures in pattern.captures_iter(page) {
            ids.insert(captures[1].to_ascii_uppercase());
        }
    }
    (ids.len() == 1).then(|| ids.into_iter().next()).flatten()
}

// Same named person without a machine-readable ID may be grouped within
// this upload only, never joined to an old case by name alone.
fn auto_identity(pages: &[String]) -> (Option<String>, Option<String>) {
    // Do not use a generic "Name:" label: it might name a treating clinician.
    let pattern = regex::Regex::new(
        r"(?im)^[ \t]*(?:patient(?:[ \t]+name)?|client[ \t]+name)[ \t]*[:\-][ \t]*(?:\r?\n[ \t]*)?([^|\r\n]{3,120})",
    )
    .unwrap();
    let mut names = std::collections::BTreeSet::new();
    for page in pages {
        for captures in pattern.captures_iter(page) {
            let candidate = captures[1].split_whitespace().collect::<Vec<_>>().join(" ");
            if candidate.split_whitespace().count() >= 2
                && candidate.chars().count() <= 120
                && candidate
                    .chars()
                    .all(|ch| ch.is_alphabetic() || matches!(ch, ' ' | '-' | '\'' | '.'))
            {
                names.insert(candidate);
            }
        }
    }
    let name = if names.len() == 1 {
        names.into_iter().next()
    } else {
        None
    };
    (name, labeled_patient_id(pages))
}

struct AutoDocument {
    filename: String,
    bytes: Vec<u8>,
    extracted: crate::ocr::ExtractedPages,
    name: Option<String>,
    patient_id: Option<String>,
}

async fn auto_import(State(state): State<Arc<AppState>>, multipart: Multipart) -> Response {
    let (uploads, decisions) = match read_batch_uploads(multipart).await {
        Ok(input) => input,
        Err(err) => return err.into_response(),
    };
    if decisions.is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "Automatic intake accepts PDF files only",
        );
    }
    let store = state.store.clone();
    let data_dir = state.data_dir.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<CaseSummary>, String> {
        let provider_numbers = provider_phone_numbers(&data_dir)?;
        // Extract every document before modifying stored cases.
        let mut documents = Vec::new();
        for (filename, bytes) in uploads {
            let scratch = TempIntakePdf::save(&bytes)
                .map_err(|_| "Could not stage uploaded PDF locally".to_string())?;
            let extracted = crate::ocr::extract_pdf(&scratch.0, false)
                .map_err(|_| format!("Could not read PDF: {filename}"))?;
            let (name, patient_id) = auto_identity(&extracted.pages);
            documents.push(AutoDocument {
                filename,
                bytes,
                extracted,
                name,
                patient_id,
            });
        }

        let existing = store
            .list()
            .map_err(|_| "Could not read existing people".to_string())?;
        // A duplicate patient ID in existing cases is ambiguous. Never pick
        // one arbitrarily; create Unassigned items instead.
        let mut existing_ids: std::collections::BTreeMap<String, Vec<Case>> =
            std::collections::BTreeMap::new();
        for case in existing {
            if !case.patient.patient_id.is_empty() {
                existing_ids
                    .entry(case.patient.patient_id.to_ascii_uppercase())
                    .or_default()
                    .push(case);
            }
        }

        // Resolve ID-only documents against a unique named document in this
        // upload or a unique existing profile. Never guess from filenames.
        let mut names_for_ids: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        let mut ids_for_names: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for doc in &documents {
            if let (Some(name), Some(id)) = (&doc.name, &doc.patient_id) {
                names_for_ids.entry(id.clone()).or_default().insert(name.to_lowercase());
                ids_for_names.entry(name.to_lowercase()).or_default().insert(id.clone());
            }
        }
        let mut canonical_names_for_ids = std::collections::BTreeMap::<String, String>::new();
        for doc in &documents {
            if let (Some(name), Some(id)) = (&doc.name, &doc.patient_id) {
                canonical_names_for_ids.entry(id.clone()).or_insert_with(|| name.clone());
            }
        }
        let mut groups: std::collections::BTreeMap<String, Vec<AutoDocument>> =
            std::collections::BTreeMap::new();
        for mut document in documents {
            let mut name = document.name.clone();
            let mut id = document.patient_id.clone();
            let mut valid = true;
            if let Some(patient_id) = &id {
                if names_for_ids.get(patient_id).is_some_and(|names| names.len() != 1) {
                    valid = false; // Multiple names tied to one ID.
                }
                if let Some(matches) = existing_ids.get(patient_id) {
                    if matches.len() != 1
                        || matches[0].patient.name.trim().is_empty()
                    {
                        valid = false;
                    }
                    else if let Some(ref labeled) = name {
                        if !matches[0].patient.name.eq_ignore_ascii_case(labeled) { valid = false; }
                    } else {
                        name = Some(matches[0].patient.name.clone());
                    }
                    if names_for_ids.get(patient_id).is_some_and(|batch_names| {
                        !batch_names.contains(&matches[0].patient.name.to_lowercase())
                    }) { valid = false; }
                }
                if name.is_none() {
                    if let Some(candidates) = names_for_ids.get(patient_id) {
                        if candidates.len() == 1 {
                            // Preserve display casing from a labeled batch record.
                            name = canonical_names_for_ids.get(patient_id).cloned()
                                .or_else(|| candidates.iter().next().cloned());
                        }
                    }
                }
            } else if let Some(label) = &name {
                // Name-only records in the current batch can join an ID group
                // only when the name uniquely corresponds to exactly one ID.
                if let Some(ids) = ids_for_names.get(&label.to_lowercase()) {
                    if ids.len() == 1 { id = ids.iter().next().cloned(); }
                    else { valid = false; }
                }
            }
            if !valid || name.is_none() {
                // Preserve the extracted metadata for later inspection, but
                // do not associate mismatched or unidentified medical records.
                let key = format!("unknown:{}", Uuid::new_v4());
                groups.entry(key).or_default().push(document);
                continue;
            }
            // A unique ID uniquely identifies a group within this batch. A
            // name with no ID can group only with same-name documents in the
            // current upload, not an existing person by name alone.
            document.name = name;
            document.patient_id = id.clone();
            let key = if let Some(patient_id) = id {
                format!("id:{patient_id}")
            } else {
                format!("name:{}", document.name.as_deref().unwrap().to_lowercase())
            };
            groups.entry(key).or_default().push(document);
        }

        let mut summaries = Vec::new();
        for (key, documents) in groups {
            let name = documents[0].name.clone().unwrap_or_default();
            let patient_id = documents[0].patient_id.clone().unwrap_or_default();
            let matched = if key.starts_with("id:") {
                existing_ids
                    .get(&patient_id)
                    .and_then(|matches| matches.first())
            } else {
                None
            };
            let (id, mut case) = if let Some(case) = matched {
                (case.id.clone(), case.clone())
            } else {
                let id = Uuid::new_v4().simple().to_string();
                let case = Case {
                    id: id.clone(),
                    created: Utc::now(),
                    patient: PatientProfile {
                        name: name.clone(),
                        patient_id,
                        ..Default::default()
                    },
                    files: vec![],
                    documents: vec![],
                    entries: vec![],
                    warnings: vec![],
                };
                (id, case)
            };
            let dir = store
                .case_dir(&id)
                .map_err(|_| "Invalid case directory".to_string())?;
            std::fs::create_dir_all(&dir)
                .map_err(|_| "Could not create local case folder".to_string())?;
            crate::storage::restrict_permissions(&dir)
                .map_err(|_| "Could not secure local case folder".to_string())?;
            for document in documents {
                // A rerun with the identical filename and bytes is idempotent.
                if case.files.iter().any(|old| {
                    old == &document.filename
                        && crate::crypto::read(&dir.join(old))
                            .is_ok_and(|previous| previous == document.bytes)
                }) {
                    continue;
                }
                let filename = unique_filename(&document.filename, &case.files);
                let path = dir.join(&filename);
                crate::crypto::write(&path, &document.bytes)
                    .map_err(|_| "Could not save local PDF".to_string())?;
                crate::storage::restrict_permissions(&path)
                    .map_err(|_| "Could not secure local PDF".to_string())?;
                let names = if name.is_empty() {
                    vec![]
                } else {
                    vec![name.clone()]
                };
                let (mut entries, mut warnings) =
                    entries_for_document(&filename, &document.extracted.pages, &names);
                // Identity is OCR-derived, not verified by a person.
                // Keep all candidate events unverified, including text-PDFs.
                for entry in &mut entries {
                    entry.verified = false;
                }
                if document.extracted.ocr_pages > 0 {
                    for entry in &mut entries {
                        entry.extractor = "rules+ocr".into();
                    }
                    case.warnings.push(format!(
                        "{filename}: {} pages processed with OCR; verify document and identity",
                        document.extracted.ocr_pages
                    ));
                }
                if name.is_empty() {
                    case.warnings.push(format!(
                        "{filename}: identity could not be determined reliably; kept Unassigned"
                    ));
                } else {
                    case.warnings.push(format!(
                        "{filename}: identity assigned locally from labeled fields; extracted chronology still requires review"
                    ));
                }
                let mut metadata = document_metadata_with_provider_numbers(
                    &filename, &document.extracted.pages, &entries, &provider_numbers,
                );
                // Name detection with no contradictory ID is sufficient for
                // automatic document identity confirmation. A name-less record
                // can also be confirmed by a unique matched patient ID.
                if !case.patient.name.is_empty() {
                    let id_conflict = !metadata.detected_patient_id.is_empty()
                        && !case.patient.patient_id.is_empty()
                        && !metadata.detected_patient_id.eq_ignore_ascii_case(&case.patient.patient_id);
                    if !id_conflict && !metadata.detected_name.is_empty()
                        && metadata.detected_name.eq_ignore_ascii_case(&case.patient.name) {
                        metadata.fields.name = true;
                        metadata.confirmed = true;
                        metadata.auto_confirmed = true;
                    } else if metadata.detected_name.is_empty()
                        && !metadata.detected_patient_id.is_empty()
                        && metadata.detected_patient_id.eq_ignore_ascii_case(&case.patient.patient_id) {
                        metadata.fields.patient_id = true;
                        metadata.confirmed = true;
                        metadata.auto_confirmed = true;
                    }
                }
                case.documents.push(metadata);
                case.entries.append(&mut entries);
                case.warnings.append(&mut warnings);
                case.warnings.extend(
                    document
                        .extracted
                        .warnings
                        .into_iter()
                        .map(|w| format!("{filename}: {w}")),
                );
                case.files.push(filename);
            }
            case.entries.sort_by(|a, b| {
                a.date
                    .cmp(&b.date)
                    .then(a.source_file.cmp(&b.source_file))
                    .then(a.page.cmp(&b.page))
            });
            store
                .save(&case)
                .map_err(|_| "Could not save local case".to_string())?;
            summaries.push(CaseSummary {
                id,
                name: case.patient.name.clone(),
                created: case.created,
                file_count: case.files.len(),
                entry_count: case.entries.len(),
            });
        }
        Ok(summaries)
    })
    .await;
    match result {
        Ok(Ok(summaries)) => no_store(Json(summaries).into_response()),
        Ok(Err(message)) => error(StatusCode::UNPROCESSABLE_ENTITY, &message),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Automatic local import failed",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn intake_request(
        uri: &str,
        count: usize,
        decisions: Option<serde_json::Value>,
    ) -> Request<Body> {
        intake_request_with_pdf(
            uri,
            count,
            decisions,
            include_bytes!("../samples/01_jordan_test_02_imaging.pdf"),
        )
    }

    fn intake_request_with_pdf(
        uri: &str,
        count: usize,
        decisions: Option<serde_json::Value>,
        pdf: &[u8],
    ) -> Request<Body> {
        let boundary = "medchron-test-boundary";
        let mut body = Vec::new();
        for _ in 0..count {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"notes.pdf\"\r\nContent-Type: application/pdf\r\n\r\n").as_bytes());
            body.extend_from_slice(pdf);
            body.extend_from_slice(b"\r\n");
        }
        if let Some(decisions) = decisions {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"decisions\"\r\n\r\n{decisions}\r\n").as_bytes());
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        Request::post(uri)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap()
    }

    async fn response_json(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn batch_upload_honors_document_size_limits() {
        let dir = tempfile::tempdir().unwrap();
        let app = router(dir.path().to_path_buf());
        let mut document =
            lopdf::Document::load_mem(include_bytes!("../samples/01_jordan_test_02_imaging.pdf"))
                .unwrap();
        document.trailer.set(
            "SyntheticTestPadding",
            lopdf::Object::string_literal(vec![b'x'; 2 * 1024 * 1024]),
        );
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert!(bytes.len() > 2 * 1024 * 1024 && bytes.len() < MAX_PDF_BYTES);
        let response = app
            .clone()
            .oneshot(intake_request_with_pdf(
                "/api/intake/analyze",
                1,
                None,
                &bytes,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response_json(response).await[0]["suggested_name"],
            "Jordan Test"
        );

        bytes.resize(MAX_PDF_BYTES + 1, b' ');
        let response = app
            .oneshot(intake_request_with_pdf(
                "/api/intake/analyze",
                1,
                None,
                &bytes,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn analysis_and_unconfirmed_import_do_not_create_cases() {
        let dir = tempfile::tempdir().unwrap();
        let app = router(dir.path().to_path_buf());
        let response = app
            .clone()
            .oneshot(intake_request("/api/intake/analyze", 1, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let preview = response_json(response).await;
        assert_eq!(preview[0]["suggested_name"], "Jordan Test");
        assert_eq!(preview[0]["ocr_pages"], 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

        let response = app
            .oneshot(intake_request(
                "/api/intake/import",
                1,
                Some(serde_json::json!({
                    "names": ["Jordan Test"], "confirmed": false
                })),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn reviewed_batch_groups_files_and_merges_only_explicit_destinations() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let existing_id = Uuid::new_v4().simple().to_string();
        store
            .save(&Case {
                id: existing_id.clone(),
                created: Utc::now(),
                patient: PatientProfile {
                    name: "Taylor Example".into(),
                    ..Default::default()
                },
                files: vec![],
                documents: vec![],
                entries: vec![],
                warnings: vec![],
            })
            .unwrap();
        let app = router(dir.path().to_path_buf());
        let response = app
            .clone()
            .oneshot(intake_request(
                "/api/intake/import",
                3,
                Some(serde_json::json!({
                    "names": ["Jordan Test", "Jordan Test", "Taylor Example"], "confirmed": true
                })),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let summaries = response_json(response).await;
        assert_eq!(summaries.as_array().unwrap().len(), 2);
        let jordan_id = summaries[0]["id"].as_str().unwrap();
        let jordan = store.load(jordan_id).unwrap();
        assert_eq!(jordan.patient.name, "Jordan Test");
        assert_eq!(jordan.files, ["notes.pdf", "notes-2.pdf"]);
        assert!(!jordan.entries.is_empty());
        assert!(jordan
            .entries
            .iter()
            .all(|e| e.page == 1 && e.status == Status::Pending));
        assert_ne!(summaries[1]["id"].as_str().unwrap(), existing_id);
        assert!(store.load(&existing_id).unwrap().files.is_empty());

        let response = app
            .clone()
            .oneshot(intake_request(
                "/api/intake/import",
                1,
                Some(serde_json::json!({
                    "names": ["Taylor Example"], "case_ids": [existing_id], "confirmed": true
                })),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let summaries = response_json(response).await;
        assert_eq!(summaries[0]["id"], existing_id);
        assert_eq!(store.list().unwrap().len(), 3);
        assert_eq!(store.load(&existing_id).unwrap().files, ["notes.pdf"]);
        assert_eq!(
            store.load(jordan_id).unwrap().files,
            ["notes.pdf", "notes-2.pdf"]
        );

        let response = app
            .oneshot(
                Request::get(format!("/api/cases/{existing_id}/files/notes.pdf"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            bytes.as_ref(),
            include_bytes!("../samples/01_jordan_test_02_imaging.pdf")
        );
    }

    #[test]
    fn profile_rejects_control_chars() {
        assert!(validate_profile(PatientProfile {
            name: "Jane\nDoe".into(),
            ..Default::default()
        })
        .is_err());
    }
    #[test]
    fn csv_formula_prefixes_are_escaped() {
        assert_eq!(safe_formula("=1+2"), "'=1+2");
        assert_eq!(safe_formula("  @payload"), "'  @payload");
        assert_eq!(safe_formula("normal"), "normal");
    }
    #[test]
    fn duplicate_filenames_are_disambiguated() {
        assert_eq!(
            unique_filename("notes.pdf", &["notes.pdf".into()]),
            "notes-2.pdf"
        );
    }
    #[test]
    fn only_explicit_name_label_is_suggested() {
        let pages =
            vec!["Patient: Jordan Test | Fictional identity\nDate of service: 2024-01-01".into()];
        assert_eq!(labeled_name(&pages).0.as_deref(), Some("Jordan Test"));
        assert_eq!(
            labeled_name(&["Dr. Jane Example saw Jordan Test".into()]).0,
            None
        );
    }
    #[test]
    fn conflicting_names_are_not_assigned() {
        let pages = vec!["Patient: Jordan Test\nPatient: Taylor Example".into()];
        assert_eq!(labeled_name(&pages).0, None);
    }
    #[test]
    fn automatic_identity_requires_explicit_name_label() {
        let pages = vec![
            "Patient Name: Jordan Test\nPatient ID: DEMO-JT-101\nRecord Date: 2024-02-05".into(),
        ];
        let (name, id) = auto_identity(&pages);
        assert_eq!(name.as_deref(), Some("Jordan Test"));
        assert_eq!(id.as_deref(), Some("DEMO-JT-101"));
        let (name, id) = auto_identity(&["A doctor saw Taylor Example".into()]);
        assert!(name.is_none());
        assert!(id.is_none());
        let (_, id) = auto_identity(&["Patient ID: DEMO-X-1\nPatient ID: DEMO-Y-2".into()]);
        assert!(id.is_none());
    }

    #[test]
    fn automatic_identity_accepts_pdf_label_and_value_on_separate_lines() {
        let pages = vec!["Patient Name:\nJordan Test\nPatient ID:\nDEMO-JT-101".into()];
        let (name, id) = auto_identity(&pages);
        assert_eq!(name.as_deref(), Some("Jordan Test"));
        assert_eq!(id.as_deref(), Some("DEMO-JT-101"));
    }

    #[test]
    fn document_metadata_has_unconfirmed_identity_and_document_date() {
        let pages = vec!["Patient Name: Jordan Test\nPatient ID: DEMO-JT-101\nRecord Date: 2024-03-18\nPhone: (902) 555-0188".into()];
        let document = document_metadata("fictional.pdf", &pages, &[]);
        assert_eq!(document.detected_name, "Jordan Test");
        assert_eq!(document.detected_patient_id, "DEMO-JT-101");
        assert_eq!(document.detected_date, "2024-03-18");
        assert_eq!(document.detected_phone, "(902) 555-0188");
        assert!(!document.confirmed);
        assert!(document.calendar_dates.contains(&"2024-03-18".into()));
    }

    #[tokio::test]
    async fn automatic_intake_groups_five_identity_documents() {
        let dir = tempfile::tempdir().unwrap();
        let app = router(dir.path().to_path_buf());
        let files = [
            (
                "jordan-a.pdf",
                include_bytes!("../samples/01_jordan_test_02_imaging.pdf").as_slice(),
            ),
            (
                "jordan-b.pdf",
                include_bytes!("../samples/01_jordan_test_04_family_physician.pdf").as_slice(),
            ),
            (
                "taylor.pdf",
                include_bytes!("../samples/02_taylor_example_02_imaging.pdf").as_slice(),
            ),
        ];
        let mut body = Vec::new();
        let boundary = "medchron-autobatch-test";
        for (filename, pdf) in files {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{filename}\"\r\nContent-Type: application/pdf\r\n\r\n").as_bytes());
            body.extend_from_slice(pdf);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let request = Request::post("/api/intake/auto")
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let summaries = response_json(response).await;
        assert_eq!(summaries.as_array().unwrap().len(), 2);
        let cases = Store::new(dir.path().to_path_buf()).list().unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(
            cases
                .iter()
                .find(|c| c.patient.name == "Jordan Test")
                .unwrap()
                .files
                .len(),
            2
        );
        assert_eq!(
            cases
                .iter()
                .find(|c| c.patient.name == "Taylor Example")
                .unwrap()
                .files
                .len(),
            1
        );
        assert!(cases
            .iter()
            .flat_map(|case| case.documents.iter())
            .all(|d| d.confirmed && d.fields.name));
        // Auto-confirming identity must never accept unverified chronology entries.
        assert!(cases
            .iter()
            .flat_map(|case| case.entries.iter())
            .all(|e| !e.verified));
    }

    #[test]
    fn patient_phone_is_kept_when_clinic_phone_is_also_present() {
        let pages = vec!["Patient Name: Jordan Test\nClinic phone: (902) 555-0100\nPatient phone: (902) 555-0188".into()];
        let doc = document_metadata_with_provider_numbers(
            "two-phones.pdf",
            &pages,
            &[],
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(doc.detected_phone, "(902) 555-0188");
        assert!(!doc.phone_is_provider);
        assert!(!doc.fields.phone);
    }

    #[test]
    fn provider_labeled_phone_cannot_propose_person() {
        let case = Case {
            id: "0001".into(),
            created: Utc::now(),
            patient: PatientProfile {
                name: "Jordan Test".into(),
                ..Default::default()
            },
            files: vec!["provider.pdf".into()],
            documents: vec![DocumentMetadata {
                filename: "provider.pdf".into(),
                detected_name: "Jordan Test".into(),
                detected_phone: "9025550100".into(),
                phone_is_provider: true,
                confirmed: true,
                fields: crate::models::FieldConfirmations {
                    name: true,
                    ..Default::default()
                },
                ..Default::default()
            }],
            entries: vec![],
            warnings: vec![],
        };
        assert!(phone_match_candidate(&[case], "9025550100", &Default::default()).is_none());
    }

    #[test]
    fn provider_whitelist_marks_phone_but_cannot_confirm_identity() {
        let provider_numbers = std::collections::BTreeSet::from(["9025550188".into()]);
        let doc = document_metadata_with_provider_numbers(
            "provider.pdf",
            &["Provider phone: (902) 555-0188\nPhone: (902) 555-0188".into()],
            &[],
            &provider_numbers,
        );
        assert!(doc.phone_is_provider);
        assert!(doc.fields.phone);
        assert!(!doc.confirmed);
    }

    #[test]
    fn local_phone_matching_excludes_providers_and_requires_unique_identity() {
        let make_case = |id: &str, name: &str| Case {
            id: id.into(),
            created: Utc::now(),
            patient: PatientProfile {
                name: name.into(),
                ..Default::default()
            },
            files: vec!["a.pdf".into()],
            documents: vec![DocumentMetadata {
                filename: "a.pdf".into(),
                detected_name: name.into(),
                detected_phone: "(902) 555-0188".into(),
                confirmed: true,
                fields: crate::models::FieldConfirmations {
                    name: true,
                    ..Default::default()
                },
                ..Default::default()
            }],
            entries: vec![],
            warnings: vec![],
        };
        let jordan = make_case("0001", "Jordan Test");
        let empty = std::collections::BTreeSet::new();
        let candidate =
            phone_match_candidate(std::slice::from_ref(&jordan), "1-902-555-0188", &empty);
        assert_eq!(candidate, Some(("0001".into(), "Jordan Test".into())));
        let provider = std::collections::BTreeSet::from(["9025550188".into()]);
        assert!(
            phone_match_candidate(std::slice::from_ref(&jordan), "902-555-0188", &provider)
                .is_none()
        );
        let taylor = make_case("0002", "Taylor Example");
        assert!(phone_match_candidate(&[jordan, taylor], "9025550188", &empty).is_none());
        assert!(normalize_patient_phone("555-0188").is_none());
    }

    #[tokio::test]
    async fn approved_phone_move_is_explicit_and_rejects_provider_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("data"));
        let target_id = Uuid::new_v4().simple().to_string();
        let source_id = Uuid::new_v4().simple().to_string();
        let phone = "(902) 555-0188";
        let file = "nameless.pdf";
        store
            .save(&Case {
                id: target_id.clone(),
                created: Utc::now(),
                patient: PatientProfile {
                    name: "Jordan Test".into(),
                    ..Default::default()
                },
                files: vec!["known.pdf".into()],
                documents: vec![DocumentMetadata {
                    filename: "known.pdf".into(),
                    detected_name: "Jordan Test".into(),
                    detected_phone: phone.into(),
                    confirmed: true,
                    fields: crate::models::FieldConfirmations {
                        name: true,
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                entries: vec![],
                warnings: vec![],
            })
            .unwrap();
        store
            .save(&Case {
                id: source_id.clone(),
                created: Utc::now(),
                patient: PatientProfile::default(),
                files: vec![file.into()],
                documents: vec![DocumentMetadata {
                    filename: file.into(),
                    detected_phone: phone.into(),
                    ..Default::default()
                }],
                entries: vec![],
                warnings: vec![],
            })
            .unwrap();
        crate::crypto::write(
            &store.case_dir(&source_id).unwrap().join(file),
            b"%PDF-synthetic",
        )
        .unwrap();
        let app = router(dir.path().join("data"));
        // Provider whitelist blocks using an organization number as a patient ID.
        std::fs::write(dir.path().join("data/provider-phones.txt"), "9025550188\n").unwrap();
        let req = || {
            Request::post(format!("/api/cases/{source_id}/documents/{file}/move"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({"target_case_id":target_id.clone(),"confirm_phone":true})
                        .to_string(),
                ))
                .unwrap()
        };
        assert_eq!(
            app.clone().oneshot(req()).await.unwrap().status(),
            StatusCode::CONFLICT
        );
        std::fs::remove_file(dir.path().join("data/provider-phones.txt")).unwrap();
        assert_eq!(
            app.clone().oneshot(req()).await.unwrap().status(),
            StatusCode::OK
        );
        let target = store.load(&target_id).unwrap();
        assert!(target
            .documents
            .iter()
            .any(|d| d.detected_phone == phone && d.confirmed && d.fields.phone));
    }

    #[tokio::test]
    async fn document_verification_toggles_and_unassigned_move_remains_unverified() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let unknown_id = Uuid::new_v4().simple().to_string();
        let target_id = Uuid::new_v4().simple().to_string();
        let filename = "synthetic.pdf".to_string();
        let bytes = include_bytes!("../samples/01_jordan_test_02_imaging.pdf");
        let unknown = Case {
            id: unknown_id.clone(),
            created: Utc::now(),
            patient: PatientProfile::default(),
            files: vec![filename.clone()],
            documents: vec![DocumentMetadata {
                filename: filename.clone(),
                detected_name: "Jordan Test".into(),
                detected_patient_id: "DEMO-JT-101".into(),
                calendar_dates: vec!["2024-02-05".into()],
                ..Default::default()
            }],
            entries: vec![],
            warnings: vec![],
        };
        store.save(&unknown).unwrap();
        crate::crypto::write(&store.case_dir(&unknown_id).unwrap().join(&filename), bytes).unwrap();
        store
            .save(&Case {
                id: target_id.clone(),
                created: Utc::now(),
                patient: PatientProfile {
                    name: "Jordan Test".into(),
                    patient_id: "DEMO-JT-101".into(),
                    ..Default::default()
                },
                files: vec![],
                documents: vec![],
                entries: vec![],
                warnings: vec![],
            })
            .unwrap();
        let app = router(dir.path().to_path_buf());
        let request = Request::post(format!("/api/cases/{unknown_id}/documents/{filename}/move"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({"target_case_id": target_id})).unwrap(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(store.load(&unknown_id).unwrap().files.is_empty());
        let dest = store.load(&target_id).unwrap();
        assert_eq!(dest.files.len(), 1);
        assert!(!dest.documents[0].confirmed);
        assert_eq!(
            crate::crypto::read(&store.case_dir(&target_id).unwrap().join(&filename)).unwrap(),
            bytes
        );
        let request = Request::patch(format!(
            "/api/cases/{target_id}/documents/{filename}/confirm"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"fields":{"name":true,"date":false,"phone":false,"patient_id":false}}"#,
        ))
        .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["confirmed"], true);
        let request = Request::patch(format!(
            "/api/cases/{target_id}/documents/{filename}/confirm"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"fields":{"name":false,"date":false,"phone":false,"patient_id":false}}"#,
        ))
        .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["confirmed"], false);
    }
}
