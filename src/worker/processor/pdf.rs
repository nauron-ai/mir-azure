use std::path::Path;
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

use super::process::{run_command, ProcessError};

const COLOR_IMAGE_RESOLUTION_DPI: u16 = 150;
const GRAY_IMAGE_RESOLUTION_DPI: u16 = 150;
const GHOSTSCRIPT_BIN: &str = "gs";
const MONO_IMAGE_RESOLUTION_DPI: u16 = 300;
const PDFWRITE_DEVICE: &str = "pdfwrite";

#[derive(Debug, Error)]
pub enum PdfOptimizationError {
    #[error("ghostscript execution failed: {0}")]
    Execution(#[from] ProcessError),
    #[error("ghostscript failed: {0}")]
    CommandFailed(String),
}

#[derive(Debug, Error)]
pub enum PdfSplitError {
    #[error("ghostscript execution failed: {0}")]
    Execution(#[from] ProcessError),
    #[error("ghostscript failed: {0}")]
    CommandFailed(String),
    #[error("invalid PDF page count: {0}")]
    InvalidPageCount(String),
}

pub async fn optimize_pdf_for_azure_retry(
    input_path: &Path,
    output_path: &Path,
    timeout: Duration,
) -> Result<(), PdfOptimizationError> {
    let mut command = Command::new(GHOSTSCRIPT_BIN);
    command
        .arg(format!("-sDEVICE={PDFWRITE_DEVICE}"))
        .arg("-dCompatibilityLevel=1.7")
        .arg("-dNOPAUSE")
        .arg("-dQUIET")
        .arg("-dBATCH")
        .arg("-dSAFER")
        .arg("-dDownsampleColorImages=true")
        .arg("-dColorImageDownsampleType=/Bicubic")
        .arg(format!(
            "-dColorImageResolution={COLOR_IMAGE_RESOLUTION_DPI}"
        ))
        .arg("-dDownsampleGrayImages=true")
        .arg("-dGrayImageDownsampleType=/Bicubic")
        .arg(format!("-dGrayImageResolution={GRAY_IMAGE_RESOLUTION_DPI}"))
        .arg("-dDownsampleMonoImages=true")
        .arg(format!("-dMonoImageResolution={MONO_IMAGE_RESOLUTION_DPI}"))
        .arg(format!("-sOutputFile={}", output_path.display()))
        .arg(input_path);
    let output = run_command(command, GHOSTSCRIPT_BIN, timeout).await?;

    if output.status.success() {
        return Ok(());
    }

    Err(PdfOptimizationError::CommandFailed(command_error(&output)))
}

pub async fn count_pdf_pages(input_path: &Path, timeout: Duration) -> Result<usize, PdfSplitError> {
    let mut command = Command::new(GHOSTSCRIPT_BIN);
    command
        .arg("-q")
        .arg("-dBATCH")
        .arg("-dNODISPLAY")
        .arg("-dSAFER")
        .arg("-c")
        .arg(build_page_count_script(input_path));
    let output = run_command(command, GHOSTSCRIPT_BIN, timeout).await?;

    if !output.status.success() {
        return Err(PdfSplitError::CommandFailed(command_error(&output)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_page_count(&stdout)
}

pub async fn split_pdf_range(
    input_path: &Path,
    output_path: &Path,
    first_page: usize,
    last_page: usize,
    timeout: Duration,
) -> Result<(), PdfSplitError> {
    let mut command = Command::new(GHOSTSCRIPT_BIN);
    command
        .arg(format!("-sDEVICE={PDFWRITE_DEVICE}"))
        .arg("-dNOPAUSE")
        .arg("-dQUIET")
        .arg("-dBATCH")
        .arg("-dSAFER")
        .arg(format!("-dFirstPage={first_page}"))
        .arg(format!("-dLastPage={last_page}"))
        .arg(format!("-sOutputFile={}", output_path.display()))
        .arg(input_path);
    let output = run_command(command, GHOSTSCRIPT_BIN, timeout).await?;

    if output.status.success() {
        return Ok(());
    }

    Err(PdfSplitError::CommandFailed(command_error(&output)))
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

fn build_page_count_script(input_path: &Path) -> String {
    format!(
        "{} (r) file runpdfbegin pdfpagecount = quit",
        postscript_hex_path(input_path)
    )
}

fn parse_page_count(value: &str) -> Result<usize, PdfSplitError> {
    let page_count = value
        .parse::<usize>()
        .map_err(|_| PdfSplitError::InvalidPageCount(value.to_string()))?;
    if page_count == 0 {
        return Err(PdfSplitError::InvalidPageCount(value.to_string()));
    }

    Ok(page_count)
}

fn postscript_hex_path(input_path: &Path) -> String {
    let path = input_path.to_string_lossy();
    let hex = path
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();

    format!("<{hex}>")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{build_page_count_script, parse_page_count, postscript_hex_path, PdfSplitError};

    #[test]
    fn builds_hex_postscript_path_literal() {
        let literal = postscript_hex_path(Path::new(r"/tmp/a(b)\c.pdf"));

        assert_eq!(literal, "<2F746D702F612862295C632E706466>");
    }

    #[test]
    fn builds_page_count_script_with_hex_path() {
        let script = build_page_count_script(Path::new(r"/tmp/a(b)\c.pdf"));

        assert_eq!(
            script,
            "<2F746D702F612862295C632E706466> (r) file runpdfbegin pdfpagecount = quit"
        );
    }

    #[test]
    fn rejects_zero_page_count() {
        let result = parse_page_count("0");

        assert!(matches!(result, Err(PdfSplitError::InvalidPageCount(value)) if value == "0"));
    }
}
