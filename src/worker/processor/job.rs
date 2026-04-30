use std::path::Path;
use std::time::Instant;

use nauron_contracts::{ArtifactRef, MirEvent, MirRequest, MirStage, SourceRef};
use thiserror::Error;

use super::events::EventRecorder;
use super::media::source_extension;
use super::progress::build_progress_event;
use super::result::{create_failure, create_success};
use super::submission::analyze_document;
use super::submission_error::DocumentSubmissionError;
use crate::worker::{processor::WorkerContext, WorkerOutput};

const TEXT_MARKDOWN: &str = "text/markdown";

#[derive(Debug, Error)]
pub enum JobError {
    #[error("Storage error: {0}")]
    Storage(#[from] Box<crate::worker::storage::StorageError>),
    #[error("{0}")]
    Document(#[from] DocumentSubmissionError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("S3 storage is required to upload artifacts")]
    StorageUnavailable,
}

impl From<crate::worker::storage::StorageError> for JobError {
    fn from(err: crate::worker::storage::StorageError) -> Self {
        JobError::Storage(Box::new(err))
    }
}

pub async fn process_request(request: &MirRequest, ctx: &WorkerContext) -> WorkerOutput {
    process_request_streaming(request, ctx, |_| {}).await
}

pub async fn process_request_streaming(
    request: &MirRequest,
    ctx: &WorkerContext,
    mut publish: impl FnMut(MirEvent),
) -> WorkerOutput {
    let started_at = Instant::now();
    tracing::info!(
        job_id = %request.job_id,
        context_id = request.context_id,
        attempt = request.attempt,
        "mir_azure_job_started"
    );
    let mut events = EventRecorder::new(&mut publish);
    events.push(build_progress_event(
        request,
        MirStage::Received,
        0,
        format!("job accepted (attempt #{})", request.attempt),
    ));

    match run_job(request, ctx, &mut events).await {
        Ok(artifacts) => push_success_events(request, &mut events, artifacts, started_at),
        Err(err) => push_failure_event(request, &mut events, err, started_at),
    }

    WorkerOutput {
        events: events.into_events(),
    }
}

async fn run_job(
    request: &MirRequest,
    ctx: &WorkerContext,
    events: &mut EventRecorder<'_>,
) -> Result<Vec<ArtifactRef>, JobError> {
    let job_dir = ctx.config().output_root.join(request.job_id.to_string());
    tokio::fs::create_dir_all(&job_dir).await?;
    let result = run_job_in_dir(request, ctx, events, &job_dir).await;
    finalize_job_dir(&job_dir, result).await
}

async fn run_job_in_dir(
    request: &MirRequest,
    ctx: &WorkerContext,
    events: &mut EventRecorder<'_>,
    job_dir: &Path,
) -> Result<Vec<ArtifactRef>, JobError> {
    let input_path = build_input_path(job_dir, &request.source);
    let doc_path = job_dir.join("document.md");

    events.push(build_progress_event(
        request,
        MirStage::Detect,
        10,
        "downloading input document",
    ));
    download_source_document(request, ctx, &input_path).await?;

    let markdown = analyze_document(request, ctx, &input_path, events).await?;
    events.push(build_progress_event(
        request,
        MirStage::ProcessingAssemble,
        80,
        "saving markdown result",
    ));
    tokio::fs::write(&doc_path, &markdown).await?;

    events.push(build_progress_event(
        request,
        MirStage::Upload,
        90,
        "uploading artifacts",
    ));
    let artifacts = upload_markdown(request, ctx, &doc_path).await?;
    Ok(artifacts)
}

async fn finalize_job_dir(
    job_dir: &Path,
    result: Result<Vec<ArtifactRef>, JobError>,
) -> Result<Vec<ArtifactRef>, JobError> {
    match tokio::fs::remove_dir_all(job_dir).await {
        Ok(()) => result,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => result,
        Err(error) => {
            tracing::warn!(path = %job_dir.display(), ?error, "failed to remove job directory");
            result
        }
    }
}

fn build_input_path(job_dir: &Path, source: &SourceRef) -> std::path::PathBuf {
    match source_extension(source) {
        Some(extension) => job_dir.join(format!("input_document.{extension}")),
        None => job_dir.join("input_document"),
    }
}

async fn download_source_document(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
) -> Result<(), JobError> {
    match &request.source {
        SourceRef::S3 { bucket, key, .. } => {
            let Some(storage) = ctx.storage() else {
                return Err(JobError::StorageUnavailable);
            };
            storage.download_file(bucket, key, input_path).await?;
        }
        SourceRef::LocalPath { path } => {
            tokio::fs::copy(path, input_path).await?;
        }
    }

    Ok(())
}

async fn upload_markdown(
    request: &MirRequest,
    ctx: &WorkerContext,
    doc_path: &Path,
) -> Result<Vec<ArtifactRef>, JobError> {
    let Some(storage) = ctx.storage() else {
        return Err(JobError::StorageUnavailable);
    };

    let prefix = match request.output.prefix.as_ref() {
        Some(prefix) => format!("{}/{}", prefix.trim_matches('/'), request.job_id),
        None => request.job_id.to_string(),
    };
    let doc_key = format!("{prefix}/document.md");

    storage
        .upload_file(
            &request.output.bucket,
            &doc_key,
            doc_path,
            Some(TEXT_MARKDOWN),
        )
        .await?;
    let size_bytes = tokio::fs::metadata(doc_path).await?.len();

    Ok(vec![ArtifactRef {
        bucket: request.output.bucket.clone(),
        key: doc_key,
        content_type: Some(String::from(TEXT_MARKDOWN)),
        size_bytes: Some(size_bytes),
    }])
}

fn push_success_events(
    request: &MirRequest,
    events: &mut EventRecorder<'_>,
    artifacts: Vec<ArtifactRef>,
    started_at: Instant,
) {
    let duration_ms = duration_ms(started_at);
    tracing::info!(
        job_id = %request.job_id,
        context_id = request.context_id,
        duration_ms,
        "mir_azure_job_finished"
    );
    events.push(build_progress_event(
        request,
        MirStage::Completed,
        100,
        "job completed",
    ));
    events.push(MirEvent::Result(create_success(
        request,
        artifacts,
        duration_ms,
    )));
}

fn push_failure_event(
    request: &MirRequest,
    events: &mut EventRecorder<'_>,
    err: JobError,
    started_at: Instant,
) {
    tracing::error!(
        job_id = %request.job_id,
        context_id = request.context_id,
        duration_ms = duration_ms(started_at),
        error = ?err,
        "mir_azure_job_failed"
    );
    events.push(MirEvent::Result(create_failure(request, err)));
}

fn duration_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
