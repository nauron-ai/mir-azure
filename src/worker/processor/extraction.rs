use std::path::Path;
use std::process::ExitStatus;

use nauron_contracts::{MirRequest, MirStage};
use tokio::process::Command;
use tracing::info;

use super::events::EventRecorder;
use super::media::source_extension;
use super::progress::build_progress_event;
use super::submission::analyze_document;
use super::submission_error::DocumentSubmissionError;
use crate::worker::processor::WorkerContext;

const DOCUMENT_MARKDOWN: &str = "document.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionEngine {
    AzureDi,
    MarkItDown,
}

pub async fn extract_markdown(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    output_dir: &Path,
    events: &mut EventRecorder<'_>,
) -> Result<String, DocumentSubmissionError> {
    match select_engine(request) {
        ExtractionEngine::AzureDi => analyze_document(request, ctx, input_path, events).await,
        ExtractionEngine::MarkItDown => {
            run_markitdown(request, input_path, output_dir, events).await
        }
    }
}

pub fn select_engine(request: &MirRequest) -> ExtractionEngine {
    let Some(extension) = source_extension(&request.source) else {
        return ExtractionEngine::AzureDi;
    };

    if is_markitdown_extension(&extension) {
        return ExtractionEngine::MarkItDown;
    }

    ExtractionEngine::AzureDi
}

async fn run_markitdown(
    request: &MirRequest,
    input_path: &Path,
    output_dir: &Path,
    events: &mut EventRecorder<'_>,
) -> Result<String, DocumentSubmissionError> {
    let output_path = output_dir.join(DOCUMENT_MARKDOWN);
    info!(
        job_id = %request.job_id,
        input_path = %input_path.display(),
        "Sending document to MarkItDown"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        30,
        "converting document with MarkItDown",
    ));

    let output = Command::new("markitdown")
        .arg(input_path)
        .arg("-o")
        .arg(&output_path)
        .output()
        .await?;
    ensure_success(output.status, &output.stderr)?;

    Ok(tokio::fs::read_to_string(output_path).await?)
}

fn ensure_success(status: ExitStatus, stderr: &[u8]) -> Result<(), DocumentSubmissionError> {
    if status.success() {
        return Ok(());
    }

    Err(DocumentSubmissionError::MarkItDown {
        status: format_exit_status(status),
        stderr: String::from_utf8_lossy(stderr).trim().to_string(),
    })
}

fn is_markitdown_extension(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "csv"
            | "docx"
            | "html"
            | "htm"
            | "md"
            | "markdown"
            | "pptx"
            | "txt"
            | "xls"
            | "xlsx"
            | "xml"
    )
}

fn format_exit_status(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => String::from("terminated by signal"),
    }
}

#[cfg(test)]
mod tests {
    use nauron_contracts::{MirRequest, OutputTarget, SchemaVersion, SourceRef};
    use uuid::Uuid;

    use super::{select_engine, ExtractionEngine};

    #[test]
    fn routes_pdf_to_azure_di() {
        let request = request("docs/input.pdf");

        assert_eq!(select_engine(&request), ExtractionEngine::AzureDi);
    }

    #[test]
    fn routes_docx_to_markitdown() {
        let request = request("docs/input.docx");

        assert_eq!(select_engine(&request), ExtractionEngine::MarkItDown);
    }

    #[test]
    fn routes_txt_to_markitdown() {
        let request = request("docs/input.txt");

        assert_eq!(select_engine(&request), ExtractionEngine::MarkItDown);
    }

    #[test]
    fn routes_png_to_azure_di() {
        let request = request("docs/input.png");

        assert_eq!(select_engine(&request), ExtractionEngine::AzureDi);
    }

    fn request(key: &str) -> MirRequest {
        MirRequest {
            schema_version: SchemaVersion::V1,
            job_id: Uuid::nil(),
            context_id: 1,
            user_id: None,
            source: SourceRef::S3 {
                bucket: String::from("bucket"),
                key: String::from(key),
                version_id: None,
            },
            output: OutputTarget::new("bucket", None),
            dry_run: false,
            attempt: 1,
            submitted_at: None,
        }
    }
}
