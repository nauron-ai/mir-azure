use chrono::Utc;
use nauron_contracts::events::MirStats;
use nauron_contracts::{ArtifactRef, FailureKind, MirRequest, MirResult, SchemaVersion};
use reqwest::StatusCode;

use super::job::JobError;
use super::submission_error::DocumentSubmissionError;
use crate::azure::AzureClientError;

pub fn create_success(
    request: &MirRequest,
    artifacts: Vec<ArtifactRef>,
    duration_ms: u64,
) -> MirResult {
    MirResult::Success {
        schema_version: SchemaVersion::V1,
        job_id: request.job_id,
        context_id: request.context_id,
        artifacts,
        stats: Some(MirStats {
            duration_ms: Some(duration_ms),
            media_count: None,
            ocr_sections: None,
            sources: None,
        }),
        completed_at: Utc::now(),
    }
}

pub fn create_failure(request: &MirRequest, err: JobError) -> MirResult {
    let (kind, retryable, message, details) = classify_error(&err);
    if retryable {
        return MirResult::Retryable {
            schema_version: SchemaVersion::V1,
            job_id: request.job_id,
            context_id: request.context_id,
            kind,
            message,
            details,
            occurred_at: Utc::now(),
        };
    }

    MirResult::Failure {
        schema_version: SchemaVersion::V1,
        job_id: request.job_id,
        context_id: request.context_id,
        kind,
        message,
        details,
        occurred_at: Utc::now(),
    }
}

fn classify_error(err: &JobError) -> (FailureKind, bool, String, Option<String>) {
    let kind = match err {
        JobError::Storage(_) | JobError::StorageUnavailable => FailureKind::Storage,
        JobError::Document(error) => classify_document_error(error),
        JobError::Io(_) => FailureKind::Internal,
    };
    let retryable = matches!(err, JobError::Document(error) if is_retryable_document_error(error));

    (kind, retryable, err.to_string(), Some(format!("{err:?}")))
}

fn classify_document_error(error: &DocumentSubmissionError) -> FailureKind {
    match error {
        DocumentSubmissionError::Azure(AzureClientError::Request(_))
        | DocumentSubmissionError::Azure(AzureClientError::Timeout(_))
        | DocumentSubmissionError::Azure(AzureClientError::ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            _,
        )) => FailureKind::Upstream,
        DocumentSubmissionError::Azure(_) => FailureKind::Processing,
        DocumentSubmissionError::Io(_)
        | DocumentSubmissionError::PdfOptimization(_)
        | DocumentSubmissionError::PdfRetry { .. }
        | DocumentSubmissionError::PdfTooLargeAfterOptimization { .. }
        | DocumentSubmissionError::PdfSplit(_)
        | DocumentSubmissionError::PdfRasterization(_)
        | DocumentSubmissionError::OfficeConversion(_)
        | DocumentSubmissionError::OfficeRetry { .. } => FailureKind::Processing,
    }
}

fn is_retryable_document_error(error: &DocumentSubmissionError) -> bool {
    matches!(
        error,
        DocumentSubmissionError::Azure(AzureClientError::Request(_))
            | DocumentSubmissionError::Azure(AzureClientError::Timeout(_))
            | DocumentSubmissionError::Azure(AzureClientError::ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                _
            ))
    )
}

#[cfg(test)]
mod tests {
    use nauron_contracts::{FailureKind, MirResult, OutputTarget, SchemaVersion, SourceRef};
    use uuid::Uuid;

    use super::{create_failure, create_success};
    use crate::azure::AzureClientError;
    use crate::worker::processor::job::JobError;
    use crate::worker::processor::submission_error::DocumentSubmissionError;

    #[test]
    fn success_contains_duration_stats() {
        let result = create_success(&request(), Vec::new(), 123);

        assert!(matches!(
            result,
            MirResult::Success {
                stats: Some(nauron_contracts::events::MirStats {
                    duration_ms: Some(123),
                    ..
                }),
                ..
            }
        ));
    }

    #[test]
    fn storage_unavailable_is_storage_failure() {
        let result = create_failure(&request(), JobError::StorageUnavailable);

        assert!(matches!(
            result,
            MirResult::Failure {
                kind: FailureKind::Storage,
                ..
            }
        ));
    }

    #[test]
    fn azure_rate_limit_is_retryable_upstream_failure() {
        let error = JobError::Document(DocumentSubmissionError::Azure(AzureClientError::ApiError(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            String::new(),
        )));
        let result = create_failure(&request(), error);

        assert!(matches!(
            result,
            MirResult::Retryable {
                kind: FailureKind::Upstream,
                ..
            }
        ));
    }

    fn request() -> nauron_contracts::MirRequest {
        nauron_contracts::MirRequest {
            schema_version: SchemaVersion::V1,
            job_id: Uuid::nil(),
            context_id: 1,
            user_id: None,
            source: SourceRef::LocalPath {
                path: String::from("/tmp/input.pdf"),
            },
            output: OutputTarget::new("bucket", None),
            dry_run: false,
            attempt: 1,
            submitted_at: None,
        }
    }
}
