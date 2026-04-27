use std::path::Path;

use nauron_contracts::{ArtifactRef, MirEvent, MirRequest, MirResult, MirStage, SourceRef};
use thiserror::Error;

use super::media::source_extension;
use super::progress::build_progress_event;
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
    #[error("Unsupported source type")]
    UnsupportedSource,
    #[error("S3 storage is required to upload artifacts")]
    StorageUnavailable,
}

impl From<crate::worker::storage::StorageError> for JobError {
    fn from(err: crate::worker::storage::StorageError) -> Self {
        JobError::Storage(Box::new(err))
    }
}

pub async fn process_request(request: &MirRequest, ctx: &WorkerContext) -> WorkerOutput {
    tracing::info!("Starting to process job: {}", request.job_id);
    let mut events = vec![build_progress_event(
        request,
        MirStage::Received,
        0,
        format!("job accepted (attempt #{})", request.attempt),
    )];

    match run_job(request, ctx, &mut events).await {
        Ok(artifacts) => push_success_events(request, &mut events, artifacts),
        Err(err) => push_failure_event(request, &mut events, err),
    }

    WorkerOutput { events }
}

async fn run_job(
    request: &MirRequest,
    ctx: &WorkerContext,
    events: &mut Vec<MirEvent>,
) -> Result<Vec<ArtifactRef>, JobError> {
    let job_dir = ctx.config().output_root.join(request.job_id.to_string());
    tokio::fs::create_dir_all(&job_dir).await?;
    let result = run_job_in_dir(request, ctx, events, &job_dir).await;
    finalize_job_dir(&job_dir, result).await
}

async fn run_job_in_dir(
    request: &MirRequest,
    ctx: &WorkerContext,
    events: &mut Vec<MirEvent>,
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
    let artifacts = upload_markdown(request, ctx, &doc_path, markdown.len()).await?;
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
                return Err(JobError::UnsupportedSource);
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
    markdown_len: usize,
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

    Ok(vec![ArtifactRef {
        bucket: request.output.bucket.clone(),
        key: doc_key,
        content_type: Some(String::from(TEXT_MARKDOWN)),
        size_bytes: Some(markdown_len as u64),
    }])
}

fn push_success_events(
    request: &MirRequest,
    events: &mut Vec<MirEvent>,
    artifacts: Vec<ArtifactRef>,
) {
    tracing::info!("Successfully completed job: {}", request.job_id);
    events.push(build_progress_event(
        request,
        MirStage::Completed,
        100,
        "job completed",
    ));
    events.push(MirEvent::Result(MirResult::Success {
        schema_version: nauron_contracts::SchemaVersion::V1,
        job_id: request.job_id,
        context_id: request.context_id,
        artifacts,
        stats: None,
        completed_at: chrono::Utc::now(),
    }));
}

fn push_failure_event(request: &MirRequest, events: &mut Vec<MirEvent>, err: JobError) {
    tracing::error!("Job {} failed with error: {:?}", request.job_id, err);
    events.push(MirEvent::Result(MirResult::Failure {
        schema_version: nauron_contracts::SchemaVersion::V1,
        job_id: request.job_id,
        context_id: request.context_id,
        kind: nauron_contracts::FailureKind::Internal,
        message: err.to_string(),
        details: None,
        occurred_at: chrono::Utc::now(),
    }));
}
