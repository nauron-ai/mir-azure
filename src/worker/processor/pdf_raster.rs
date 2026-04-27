use std::path::Path;
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

use super::process::{run_command, ProcessError};

const GHOSTSCRIPT_BIN: &str = "gs";
const PNGMONO_DEVICE: &str = "pngmono";
const RASTERIZED_PAGE_DPI: u16 = 150;

#[derive(Debug, Error)]
pub enum PdfRasterizationError {
    #[error("ghostscript execution failed: {0}")]
    Execution(#[from] ProcessError),
    #[error("ghostscript failed: {0}")]
    CommandFailed(String),
}

pub async fn rasterize_pdf_page_to_png(
    input_path: &Path,
    output_path: &Path,
    timeout: Duration,
) -> Result<(), PdfRasterizationError> {
    let mut command = Command::new(GHOSTSCRIPT_BIN);
    command
        .arg(format!("-sDEVICE={PNGMONO_DEVICE}"))
        .arg(format!("-r{RASTERIZED_PAGE_DPI}"))
        .arg("-dFirstPage=1")
        .arg("-dLastPage=1")
        .arg("-dNOPAUSE")
        .arg("-dQUIET")
        .arg("-dBATCH")
        .arg("-dSAFER")
        .arg(format!("-sOutputFile={}", output_path.display()))
        .arg(input_path);
    let output = run_command(command, GHOSTSCRIPT_BIN, timeout).await?;

    if output.status.success() {
        return Ok(());
    }

    Err(PdfRasterizationError::CommandFailed(command_error(&output)))
}

fn command_error(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    format!("exit status {}", output.status)
}
