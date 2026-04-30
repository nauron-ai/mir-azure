use std::path::Path;

use nauron_contracts::{MirRequest, MirStage};
use tracing::info;

use super::document::{file_size, format_size, PreparedDocument};
use super::events::EventRecorder;
use super::progress::build_progress_event;
use super::submission_error::DocumentSubmissionError;
use crate::worker::processor::WorkerContext;

pub async fn convert_office_before_send(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
) -> Result<PreparedDocument, DocumentSubmissionError> {
    let original_size = file_size(input_path).await?;
    let output_dir = match input_path.parent() {
        Some(path) => path,
        None => Path::new("/tmp"),
    };
    info!(
        job_id = %request.job_id,
        before = %format_size(original_size),
        "Converting Office document to PDF for Azure retry"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        45,
        format!(
            "converting office document to pdf for Azure retry ({})",
            format_size(original_size)
        ),
    ));
    let converted_path = super::office::convert_office_document_to_pdf(
        input_path,
        output_dir,
        ctx.config().subprocess_timeout(),
    )
    .await
    .map_err(DocumentSubmissionError::OfficeConversion)?;
    let converted_size = file_size(&converted_path).await?;
    info!(
        job_id = %request.job_id,
        before = %format_size(original_size),
        after = %format_size(converted_size),
        "Converted Office document to PDF for Azure retry"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        50,
        format!(
            "converted office document to pdf for Azure retry: {} -> {}",
            format_size(original_size),
            format_size(converted_size)
        ),
    ));

    super::pdf_prepare::prepare_pdf_before_send(request, ctx, &converted_path, events, 52, 57).await
}
