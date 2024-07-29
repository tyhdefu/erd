use std::path::PathBuf;

pub mod artifacts;

/// Get the directory where erd stores its per-project information
pub fn get_local_dir() -> PathBuf {
    let mut path = PathBuf::new();
    path.push(".erd");
    return path;
}