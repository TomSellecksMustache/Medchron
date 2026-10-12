use crate::{
    chronology::{entries_for_document, extract_pdf_pages, MAX_PDF_BYTES},
    models::{Case, EntryPatch, Status},
    storage::Store,
};
use axum::{
    body::Body,
    extract::{Multipart, Path as AxumPath, Query, State},
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
        .route("/api/cases", post(create_case))
        .route("/api/cases/:id", get(get_case))
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
    eprintln!("DEBUG: Upload request reached Rust backend");
    let id = Uuid::new_v4().simple().to_string();
    let mut names = Vec::<String>::new();
    let mut uploads: Vec<(String, Vec<u8>)> = Vec::new();
    eprintln!("DEBUG 1: Starting multipart processing");
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(err) => {
                eprintln!("DEBUG: Multipart error: {err:?}");
                return error(StatusCode::BAD_REQUEST, "Invalid multipart form");
            }
        };
        let field_name = field.name().unwrap_or("").to_string();
        eprintln!("DEBUG 2: Received field: {}", field_name);
        if field_name == "patient_names" {
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
    eprintln!("DEBUG 3: Multipart complete. Files: {}", uploads.len());
    if uploads.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Select at least one PDF");
    }
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
    Json(case).into_response()
}

async fn get_case(State(s): State<Arc<AppState>>, AxumPath(id): AxumPath<String>) -> Response {
    match s.store.load(&id) {
        Ok(c) => Json(c).into_response(),
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
    Json(result).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;
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
