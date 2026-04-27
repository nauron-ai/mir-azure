use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

use super::process::{run_command, ProcessError};

const LIBREOFFICE_BIN: &str = "soffice";
const PDF_FILTER: &str = "pdf";
const USER_INSTALLATION_PREFIX: &str = "-env:UserInstallation=file://";

#[derive(Debug, Error)]
pub enum OfficeConversionError {
    #[error("libreoffice setup failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("libreoffice execution failed: {0}")]
    Execution(#[from] ProcessError),
    #[error("libreoffice failed: {0}")]
    CommandFailed(String),
    #[error("converted PDF was not created: {0}")]
    MissingOutput(PathBuf),
    #[error("input file name is missing")]
    MissingFileName,
}

pub async fn convert_office_document_to_pdf(
    input_path: &Path,
    output_dir: &Path,
    timeout: Duration,
) -> Result<PathBuf, OfficeConversionError> {
    let output_path = output_pdf_path(input_path, output_dir)?;
    let profile_dir = output_dir.join("soffice-profile");

    remove_existing_file(&output_path).await?;
    tokio::fs::create_dir_all(&profile_dir).await?;

    let mut command = Command::new(LIBREOFFICE_BIN);
    command
        .arg("--headless")
        .arg("--nologo")
        .arg("--nodefault")
        .arg("--norestore")
        .arg("--nolockcheck")
        .arg(format!(
            "{USER_INSTALLATION_PREFIX}{}",
            profile_dir.display()
        ))
        .arg("--convert-to")
        .arg(PDF_FILTER)
        .arg("--outdir")
        .arg(output_dir)
        .arg(input_path);
    let output = run_command(command, LIBREOFFICE_BIN, timeout).await?;

    if output.status.success() {
        return ensure_output_exists(output_path).await;
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return Err(OfficeConversionError::CommandFailed(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Err(OfficeConversionError::CommandFailed(stdout));
    }

    Err(OfficeConversionError::CommandFailed(format!(
        "exit status {}",
        output.status
    )))
}

async fn ensure_output_exists(output_path: PathBuf) -> Result<PathBuf, OfficeConversionError> {
    if tokio::fs::try_exists(&output_path).await? {
        return Ok(output_path);
    }

    Err(OfficeConversionError::MissingOutput(output_path))
}

async fn remove_existing_file(path: &Path) -> Result<(), std::io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn output_pdf_path(input_path: &Path, output_dir: &Path) -> Result<PathBuf, OfficeConversionError> {
    let Some(file_stem) = input_path.file_stem() else {
        return Err(OfficeConversionError::MissingFileName);
    };

    Ok(output_dir.join(format!("{}.pdf", file_stem.to_string_lossy())))
}
