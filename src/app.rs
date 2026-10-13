use crate::{
    chronology::{entries_for_document, extract_pdf_pages, MAX_PDF_BYTES},
    models::{Case, CaseSummary, EntryPatch, PatientProfile, Status},
    storage::Store,
};
use axum::{
    body::Body,
    extract::{Multipart, Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, patch},
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

#[derive(Clone)]
struct AppState {
    store: Store,
}
#[derive(Deserialize)]
struct ExportQuery {
    include: Option<String>,
}

pub fn router(data_dir: PathBuf) -> Router {
    let state = Arc::new(AppState {
        store: Store::new(data_dir),
    });
    Router::new()
        .route("/", get(index))
        .route("/api/cases", get(list_cases).post(create_case))
        .route("/api/samples", get(list_samples))
        .route("/api/samples/:filename", get(get_sample_file))
        .route("/api/cases/:id", get(get_case))
        .route("/api/cases/:id/profile", patch(patch_profile))
        .route("/api/cases/:id/entries/:entry_id", patch(patch_entry))
        .route("/api/cases/:id/files/:filename", get(get_file))
        .route("/api/cases/:id/export.csv", get(export_csv))
        .route("/api/cases/:id/export.md", get(export_md))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(100 * 1024 * 1024))
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
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
        entries: vec![],
        warnings: vec![],
    };
    for (filename, bytes) in uploads {
        let filename = unique_filename(&filename, &case.files);
        let path = dir.join(&filename);
        if std::fs::write(&path, &bytes).is_err() {
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
        match extract_pdf_pages(&path) {
            Ok(pages) => {
                let (mut entries, mut warnings) = entries_for_document(&filename, &pages, &names);
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
    if profile.name.chars().count() > 120 || profile.name.chars().any(char::is_control) {
        return Err("Name must be at most 120 characters and contain no control characters");
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
    match tokio::fs::read(path).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "application/pdf")
            .header(header::CACHE_CONTROL, "no-store")
            .header("x-content-type-options", "nosniff")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Response error")),
        Err(_) => error(StatusCode::NOT_FOUND, "File not found"),
    }
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
#[cfg(test)]
mod tests {
    use super::*;
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
}
