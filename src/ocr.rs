//! Offline PDF text extraction. Tesseract/Poppler are local executables; no HTTP/LLM calls.
//! A trusted operator can provide a GPU-capable offline OCR executable via the
//! MEDCHRON_OCR_EXECUTABLE environment variable; it receives one local PNG path
//! and must print UTF-8 text on stdout. Never point this at a cloud wrapper.
use lopdf::Document;
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;

type OcrError = Box<dyn Error + Send + Sync>;
pub const MAX_PAGES: usize = 120;

fn tool_path(override_name: &str, fallback: &str, bundled_relative: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(override_name) {
        return PathBuf::from(path);
    }
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent().and_then(Path::parent) {
            let bundled = root.join(bundled_relative);
            if bundled.is_file() {
                return bundled;
            }
        }
    }
    #[cfg(not(windows))]
    let _ = bundled_relative;
    PathBuf::from(fallback)
}

pub struct ExtractedPages {
    pub pages: Vec<String>,
    pub ocr_pages: usize,
    pub warnings: Vec<String>,
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self, OcrError> {
        let path = std::env::temp_dir().join(format!("medchron-ocr-{}", Uuid::new_v4()));
        fs::create_dir(&path)?;
        crate::storage::restrict_permissions(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn extract_pdf(path: &Path, preview: bool) -> Result<ExtractedPages, OcrError> {
    let document = Document::load(path)?;
    let page_ids: Vec<u32> = document.get_pages().keys().copied().collect();
    if page_ids.len() > MAX_PAGES {
        return Err(format!("PDF exceeds {MAX_PAGES} pages").into());
    }
    let work = Scratch::new()?;
    let mut pages = Vec::new();
    let mut warnings = Vec::new();
    let mut ocr_pages = 0;
    for (index, page_id) in page_ids.iter().enumerate() {
        if preview && index >= 2 {
            break;
        }
        let native = document.extract_text(&[*page_id]).unwrap_or_default();
        if native.chars().filter(|c| c.is_alphanumeric()).count() >= 35 {
            pages.push(native);
            continue;
        }
        match ocr_page(path, *page_id, &work.0, index) {
            Ok(scanned) if !scanned.trim().is_empty() => {
                pages.push(scanned);
                ocr_pages += 1;
            }
            Ok(_) => {
                pages.push(native);
                warnings.push(format!("Page {}: OCR yielded no readable text", index + 1));
            }
            Err(err) => {
                pages.push(native);
                warnings.push(format!("Page {}: local OCR unavailable ({err})", index + 1));
            }
        }
    }
    Ok(ExtractedPages {
        pages,
        ocr_pages,
        warnings,
    })
}

fn ocr_page(pdf: &Path, page_no: u32, dir: &Path, index: usize) -> Result<String, OcrError> {
    let prefix = dir.join(format!("page-{}", index + 1));
    let poppler = tool_path(
        "MEDCHRON_POPPLER_EXECUTABLE",
        "pdftoppm",
        "ocr/poppler/pdftoppm.exe",
    );
    let result = Command::new(poppler)
        .arg("-f")
        .arg(page_no.to_string())
        .arg("-l")
        .arg(page_no.to_string())
        .arg("-r")
        .arg("175")
        .arg("-singlefile")
        .arg("-png")
        .arg(pdf)
        .arg(&prefix)
        .output()?;
    if !result.status.success() {
        return Err("pdftoppm failed to rasterize page".into());
    }
    let image = prefix.with_extension("png");
    crate::storage::restrict_permissions(&image)?;
    // Explicit external executable is a local, trusted GPU integration point.
    // Paths/flags are never accepted from an HTTP request; no shell is invoked.
    let output = if let Some(executable) = std::env::var_os("MEDCHRON_OCR_EXECUTABLE") {
        Command::new(executable).arg(&image).output()?
    } else {
        let language = std::env::var("MEDCHRON_OCR_LANGUAGE").unwrap_or_else(|_| "eng".into());
        if language.is_empty()
            || !language
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == '+')
        {
            return Err("Invalid OCR language setting".into());
        }
        let executable = tool_path(
            "MEDCHRON_TESSERACT_EXECUTABLE",
            "tesseract",
            "ocr/tesseract/tesseract.exe",
        );
        let mut command = Command::new(&executable);
        command
            .arg(&image)
            .arg("stdout")
            .arg("-l")
            .arg(language)
            .arg("--psm")
            .arg("3");
        if let Some(tessdata) = executable.parent().map(|p| p.join("tessdata")) {
            if tessdata.is_dir() {
                command.arg("--tessdata-dir").arg(tessdata);
            }
        }
        command.output()?
    };
    if !output.status.success() {
        return Err("OCR engine failed to recognize page".into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_sample_pdf() {
        let sample = Path::new("samples/gp_clinic_notes.pdf");
        if sample.exists() {
            let result = extract_pdf(sample, true).expect("sample PDF should be readable");
            assert!(!result.pages.is_empty());
        }
    }
}
