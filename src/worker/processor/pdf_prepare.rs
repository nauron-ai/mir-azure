use std::path::Path;

use nauron_contracts::{MirRequest, MirStage};
use tracing::{error, info};

use super::document::{file_size, format_size, PreparedDocument};
use super::events::EventRecorder;
use super::progress::build_progress_event;
use super::submission_error::DocumentSubmissionError;
use crate::worker::processor::WorkerContext;

pub async fn prepare_pdf_before_send(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
    start_percent: u8,
    done_percent: u8,
) -> Result<PreparedDocument, DocumentSubmissionError> {
    let original_size = file_size(input_path).await?;
    if original_size <= ctx.config().effective_pdf_optimize_threshold_bytes() {
        return Ok(PreparedDocument::new(
            input_path,
            "application/pdf",
            original_size,
            false,
        ));
    }

    let optimized_path = input_path.with_extension("optimized.pdf");
    info!(
        job_id = %request.job_id,
        before = %format_size(original_size),
        threshold = %format_size(ctx.config().effective_pdf_optimize_threshold_bytes()),
        "Optimizing oversized PDF before Azure submission"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        start_percent,
        format!(
            "optimizing oversized pdf before Azure submission ({})",
            format_size(original_size)
        ),
    ));
    super::pdf::optimize_pdf_for_azure_retry(
        input_path,
        &optimized_path,
        ctx.config().subprocess_timeout(),
    )
    .await
    .map_err(DocumentSubmissionError::PdfOptimization)?;
    let optimized_size = file_size(&optimized_path).await?;
    info!(
        job_id = %request.job_id,
        before = %format_size(original_size),
        after = %format_size(optimized_size),
        limit = %format_size(ctx.config().pdf_max_bytes),
        "Optimized PDF before Azure submission"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        done_percent,
        format!(
            "optimized pdf before Azure submission: {} -> {}",
            format_size(original_size),
            format_size(optimized_size)
        ),
    ));
    if optimized_size > ctx.config().pdf_max_bytes {
        error!(
            job_id = %request.job_id,
            before = %format_size(original_size),
            after = %format_size(optimized_size),
            limit = %format_size(ctx.config().pdf_max_bytes),
            "Optimized PDF still exceeds Azure submission limit"
        );
        return Err(DocumentSubmissionError::PdfTooLargeAfterOptimization {
            before: format_size(original_size),
            after: format_size(optimized_size),
            limit: format_size(ctx.config().pdf_max_bytes),
        });
    }

    Ok(PreparedDocument::new(
        optimized_path,
        "application/pdf",
        optimized_size,
        true,
    ))
}
