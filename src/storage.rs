use crate::models::Case;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn case_dir(&self, id: &str) -> io::Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid case ID",
            ));
        }
        Ok(self.root.join(id))
    }
    pub fn save(&self, case: &Case) -> io::Result<()> {
        let dir = self.case_dir(&case.id)?;
        fs::create_dir_all(&dir)?;
        restrict_permissions(&dir)?;
        let bytes = serde_json::to_vec_pretty(case).map_err(io::Error::other)?;
        let tmp = dir.join("case.json.tmp");
        fs::write(&tmp, bytes)?;
        restrict_permissions(&tmp)?;
        fs::rename(tmp, dir.join("case.json"))
    }
    // List only case metadata, never document contents or filesystem paths.
    // Bad/missing cases are skipped so one damaged case does not hide others.
    pub fn list(&self) -> io::Result<Vec<Case>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let mut cases = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name();
            let Some(id) = id.to_str() else {
                continue;
            };
            if let Ok(case) = self.load(id) {
                cases.push(case);
            }
        }
        cases.sort_by_key(|a| std::cmp::Reverse(a.created));
        Ok(cases)
    }

    pub fn load(&self, id: &str) -> io::Result<Case> {
        let path = self.case_dir(id)?.join("case.json");
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }
}

#[cfg(unix)]
pub fn restrict_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}
#[cfg(not(unix))]
pub fn restrict_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}
