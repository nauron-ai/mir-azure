use std::path::{Path, PathBuf};

pub struct PreparedDocument {
    pub path: PathBuf,
    pub content_type: &'static str,
    pub size_bytes: u64,
    pub optimized: bool,
}

impl PreparedDocument {
    pub fn new(
        path: impl Into<PathBuf>,
        content_type: &'static str,
        size_bytes: u64,
        optimized: bool,
    ) -> Self {
        Self {
            path: path.into(),
            content_type,
            size_bytes,
            optimized,
        }
    }
}

pub async fn file_size(path: &Path) -> Result<u64, std::io::Error> {
    Ok(tokio::fs::metadata(path).await?.len())
}

pub fn format_size(size_bytes: u64) -> String {
    let mebibytes = size_bytes as f64 / (1024.0 * 1024.0);
    format!("{mebibytes:.2} MiB")
}
