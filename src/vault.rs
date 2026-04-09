use ignore::WalkBuilder;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum VaultError {
    PathTraversal(String),
    NotFound(String),
    AlreadyExists(String),
    Io(std::io::Error),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathTraversal(p) => write!(f, "path traversal rejected: {p}"),
            Self::NotFound(p) => write!(f, "not found: {p}"),
            Self::AlreadyExists(p) => write!(f, "already exists: {p}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl From<std::io::Error> for VaultError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, VaultError>;

/// Resolve a relative path against the vault root, with traversal protection.
/// Appends `.md` if the path doesn't already have that extension.
/// Expects `vault_root` to already be canonicalized.
pub fn resolve_path(vault_root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.contains("..") {
        return Err(VaultError::PathTraversal(relative.to_string()));
    }

    let mut rel = PathBuf::from(relative);
    if rel.extension().is_none_or(|ext| ext != "md") {
        rel.set_extension("md");
    }

    let full = vault_root.join(&rel);

    if full.exists() {
        let canon = full.canonicalize()?;
        if !canon.starts_with(vault_root) {
            return Err(VaultError::PathTraversal(relative.to_string()));
        }
        Ok(canon)
    } else {
        if !full.starts_with(vault_root) {
            return Err(VaultError::PathTraversal(relative.to_string()));
        }
        Ok(full)
    }
}

/// Walk all .md files in the vault (or a subfolder), respecting .gitignore and .ignore.
pub fn walk_notes(vault_root: &Path, subfolder: Option<&str>) -> Vec<PathBuf> {
    let start = match subfolder {
        Some(folder) => vault_root.join(folder),
        None => vault_root.to_path_buf(),
    };

    if !start.is_dir() {
        return Vec::new();
    }

    let walker = WalkBuilder::new(&start)
        .hidden(true)       // skip hidden files/dirs
        .git_ignore(true)   // respect .gitignore
        .git_global(false)
        .git_exclude(false)
        .build();

    walker
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .map(|entry| entry.into_path())
        .collect()
}

/// Get a note's path relative to the vault root.
pub fn relative_path(vault_root: &Path, full_path: &Path) -> String {
    full_path
        .strip_prefix(vault_root)
        .unwrap_or(full_path)
        .to_string_lossy()
        .to_string()
}

pub fn read_note(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => VaultError::NotFound(path.display().to_string()),
        _ => VaultError::Io(e),
    })
}

pub fn write_note(path: &Path, content: &str, overwrite: bool) -> Result<usize> {
    if path.exists() && !overwrite {
        return Err(VaultError::AlreadyExists(path.display().to_string()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(content.len())
}
