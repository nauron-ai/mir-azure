use std::path::Path;

use nauron_contracts::{MirRequest, MirStage};
use reqwest::Body;
use reqwest::StatusCode;
use tokio::fs::File;
use tokio::time::sleep;
use tokio_util::io::ReaderStream;
use tracing::info;

use super::document::{file_size, format_size, PreparedDocument};
use super::events::EventRecorder;
use super::media::{infer_content_type, is_convertible_office_content_type, is_pdf_content_type};
use super::progress::build_progress_event;
use super::rate_limit::retry_delay;
use super::submission_error::DocumentSubmissionError;
use crate::azure::AzureClientError;
use crate::worker::processor::WorkerContext;

const ANALYZE_RATE_LIMIT_MAX_RETRIES: usize = 8;

pub async fn analyze_document(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
) -> Result<String, DocumentSubmissionError> {
    let content_type = infer_content_type(&request.source);
    let prepared = prepare_document(request, ctx, input_path, content_type, events).await?;
    match submit_prepared_document(request, ctx, &prepared, events, 30).await {
        Ok(markdown) => Ok(markdown),
        Err(DocumentSubmissionError::Azure(error)) => {
            retry_after_invalid_content_length(request, ctx, input_path, events, prepared, error)
                .await
        }
        Err(error) => Err(error),
    }
}
async fn prepare_document(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    content_type: &'static str,
    events: &mut EventRecorder<'_>,
) -> Result<PreparedDocument, DocumentSubmissionError> {
    let original_size = file_size(input_path).await?;
    if !is_pdf_content_type(content_type) {
        return Ok(PreparedDocument::new(
            input_path,
            content_type,
            original_size,
            false,
        ));
    }

    if original_size <= ctx.config().effective_pdf_optimize_threshold_bytes() {
        return Ok(PreparedDocument::new(
            input_path,
            content_type,
            original_size,
            false,
        ));
    }

    super::pdf_prepare::prepare_pdf_before_send(request, ctx, input_path, events, 20, 25).await
}
pub(super) async fn submit_prepared_document(
    request: &MirRequest,
    ctx: &WorkerContext,
    prepared: &PreparedDocument,
    events: &mut EventRecorder<'_>,
    percent: u8,
) -> Result<String, DocumentSubmissionError> {
    info!(
        job_id = %request.job_id,
        size = %format_size(prepared.size_bytes),
        optimized = prepared.optimized,
        content_type = prepared.content_type,
        "Sending document to Azure DI"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        percent,
        format!(
            "sending document to Azure DI ({})",
            format_size(prepared.size_bytes)
        ),
    ));
    submit_prepared_document_with_retries(ctx, prepared, request, events, percent).await
}
async fn build_payload_stream(prepared: &PreparedDocument) -> Result<Body, std::io::Error> {
    let file = File::open(&prepared.path).await?;
    Ok(Body::wrap_stream(ReaderStream::new(file)))
}
async fn submit_prepared_document_with_retries(
    ctx: &WorkerContext,
    prepared: &PreparedDocument,
    request: &MirRequest,
    events: &mut EventRecorder<'_>,
    percent: u8,
) -> Result<String, DocumentSubmissionError> {
    for attempt in 0..=ANALYZE_RATE_LIMIT_MAX_RETRIES {
        let payload = build_payload_stream(prepared).await?;
        match ctx
            .azure_client()
            .analyze_document_stream(payload, prepared.content_type, prepared.size_bytes)
            .await
        {
            Ok(markdown) => return Ok(markdown),
            Err(error)
                if is_rate_limited_error(&error) && attempt < ANALYZE_RATE_LIMIT_MAX_RETRIES =>
            {
                let delay = retry_delay(attempt);
                info!(
                    job_id = %request.job_id,
                    attempt = attempt + 1,
                    delay_secs = delay.as_secs(),
                    "Azure DI rate limited request, retrying"
                );
                events.push(build_progress_event(
                    request,
                    MirStage::ProcessingRun,
                    percent,
                    format!(
                        "Azure DI rate limited request, retrying after {}s",
                        delay.as_secs()
                    ),
                ));
                sleep(delay).await;
            }
            Err(error) => return Err(DocumentSubmissionError::from(error)),
        }
    }

    Err(DocumentSubmissionError::Azure(AzureClientError::ApiError(
        StatusCode::TOO_MANY_REQUESTS,
        String::from("Azure DI rate limit retries exhausted"),
    )))
}

fn is_rate_limited_error(error: &AzureClientError) -> bool {
    matches!(
        error,
        AzureClientError::ApiError(status, _) if *status == StatusCode::TOO_MANY_REQUESTS
    )
}

async fn retry_after_invalid_content_length(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
    prepared: PreparedDocument,
    azure_error: AzureClientError,
) -> Result<String, DocumentSubmissionError> {
    if !azure_error.is_invalid_content_length() {
        return Err(DocumentSubmissionError::Azure(azure_error));
    }

    if is_pdf_content_type(prepared.content_type) {
        if prepared.optimized {
            return super::pdf_chunk::analyze_pdf_in_chunks(request, ctx, &prepared.path, events)
                .await;
        }

        return retry_pdf_after_invalid_content_length(
            request,
            ctx,
            input_path,
            events,
            azure_error,
        )
        .await;
    }

    if is_convertible_office_content_type(prepared.content_type) {
        return retry_office_after_invalid_content_length(
            request,
            ctx,
            input_path,
            events,
            azure_error,
        )
        .await;
    }

    Err(DocumentSubmissionError::Azure(azure_error))
}

async fn retry_pdf_after_invalid_content_length(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
    azure_error: AzureClientError,
) -> Result<String, DocumentSubmissionError> {
    let optimized =
        super::pdf_prepare::prepare_pdf_before_send(request, ctx, input_path, events, 45, 50)
            .await
            .map_err(|error| DocumentSubmissionError::wrap_pdf_retry(azure_error, error))?;
    if !optimized.optimized {
        return super::pdf_chunk::analyze_pdf_in_chunks(request, ctx, input_path, events).await;
    }

    match submit_prepared_document(request, ctx, &optimized, events, 55).await {
        Ok(markdown) => Ok(markdown),
        Err(DocumentSubmissionError::Azure(error)) if error.is_invalid_content_length() => {
            super::pdf_chunk::analyze_pdf_in_chunks(request, ctx, &optimized.path, events).await
        }
        Err(error) => Err(error),
    }
}

async fn retry_office_after_invalid_content_length(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
    azure_error: AzureClientError,
) -> Result<String, DocumentSubmissionError> {
    let converted =
        super::office_prepare::convert_office_before_send(request, ctx, input_path, events)
            .await
            .map_err(|error| DocumentSubmissionError::wrap_office_retry(azure_error, error))?;

    match submit_prepared_document(request, ctx, &converted, events, 60).await {
        Ok(markdown) => Ok(markdown),
        Err(DocumentSubmissionError::Azure(error)) if error.is_invalid_content_length() => {
            super::pdf_chunk::analyze_pdf_in_chunks(request, ctx, &converted.path, events).await
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use crate::azure::AzureClientError;

    use super::is_rate_limited_error;

    #[test]
    fn detects_rate_limited_error() {
        let error = AzureClientError::ApiError(StatusCode::TOO_MANY_REQUESTS, String::new());

        assert!(is_rate_limited_error(&error));
    }
}
