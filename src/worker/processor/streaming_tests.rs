use nauron_contracts::{
    FailureKind, MirEvent, MirRequest, MirResult, MirStage, OutputTarget, SchemaVersion, SourceRef,
};
use uuid::Uuid;

use super::{process_request_streaming, WorkerContext};
use crate::azure::AzureDocumentClient;
use crate::worker::WorkerArgs;

#[tokio::test]
async fn streams_events_before_returning_output() {
    let context = context().await;
    let request = request();
    let mut streamed = Vec::new();

    let output = process_request_streaming(&request, &context, |event| {
        streamed.push(event);
    })
    .await;

    assert_eq!(streamed.len(), output.events.len());
    assert!(matches!(
        streamed.first(),
        Some(MirEvent::Progress(progress)) if progress.stage == MirStage::Received
    ));
    assert!(matches!(
        streamed.last(),
        Some(MirEvent::Result(MirResult::Failure {
            kind: FailureKind::Storage,
            ..
        }))
    ));
}

async fn context() -> WorkerContext {
    let client = AzureDocumentClient::new(
        String::from("https://example.com"),
        String::from("key"),
        String::from("prebuilt-layout"),
        String::from("2024-11-30"),
    )
    .unwrap();

    WorkerContext::new(args(), client).await.unwrap()
}

fn args() -> WorkerArgs {
    WorkerArgs {
        brokers: String::from("localhost:9093"),
        group_id: String::from("test"),
        request_topic: String::from("mir.requests"),
        progress_topic: String::from("mir.progress"),
        result_topic: String::from("mir.results"),
        retry_topic: String::from("mir.retry"),
        azure_di_endpoint: String::from("https://example.com"),
        azure_di_key: String::from("key"),
        azure_di_model_id: String::from("prebuilt-layout"),
        azure_di_api_version: String::from("2024-11-30"),
        output_root: std::env::temp_dir().join(Uuid::new_v4().to_string()),
        pdf_optimize_threshold_bytes: 400 * 1024 * 1024,
        pdf_max_bytes: 500 * 1024 * 1024,
        pdf_split_page_count: 50,
        subprocess_timeout_secs: 600,
        tls_ca: None,
        tls_cert: None,
        tls_key: None,
        s3_endpoint: None,
        s3_access_key: None,
        s3_secret_key: None,
        s3_region: String::from("us-east-1"),
        s3_force_path_style: true,
    }
}

fn request() -> MirRequest {
    MirRequest {
        schema_version: SchemaVersion::V1,
        job_id: Uuid::new_v4(),
        context_id: 1,
        user_id: None,
        source: SourceRef::S3 {
            bucket: String::from("bucket"),
            key: String::from("input.pdf"),
            version_id: None,
        },
        output: OutputTarget::new("bucket", None),
        dry_run: false,
        attempt: 1,
        submitted_at: None,
    }
}
