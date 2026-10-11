use std::path::{Path, PathBuf};

pub fn normalize_workspace_path(path: impl AsRef<Path>) -> PathBuf {
    match std::fs::canonicalize(path.as_ref()) {
        Ok(canon) => canon,
        Err(_) => path.as_ref().to_path_buf(),
    }
}
