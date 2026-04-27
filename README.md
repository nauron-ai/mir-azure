# Nauron MIR Azure

Nauron MIR Azure is a worker that converts document-processing requests into Markdown artifacts using Azure Document Intelligence.

It sits behind the Nauron gateway and consumes MIR requests from Kafka. The worker isolates Azure-specific document handling from the API layer, so upstream services use shared Nauron contracts instead of calling Azure directly.

## Responsibilities

| Area | Responsibility |
| --- | --- |
| Input | Consume `MirRequest` messages from Kafka. |
| Processing | Submit supported documents to Azure Document Intelligence. |
| Preparation | Prepare large PDFs and supported Office documents before Azure submission. |
| Output | Upload generated Markdown artifacts to S3-compatible storage. |
| Events | Publish MIR progress and terminal result events. |

## Requirements

- Rust 1.95.0
- Kafka-compatible broker
- Azure Document Intelligence endpoint and API key
- S3-compatible object storage for source and output artifacts
- Ghostscript for PDF preparation
- LibreOffice for Office document conversion

## Configuration

| Variable | Required | Description |
| --- | --- | --- |
| `KAFKA_BROKERS` | No | Kafka bootstrap servers. Defaults to `127.0.0.1:9093`. |
| `KAFKA_GROUP_ID` | No | Consumer group. Defaults to `mir-azure-worker`. |
| `MIR_REQUEST_TOPIC` | No | Request topic. Defaults to the value from `nauron-contracts`. |
| `MIR_PROGRESS_TOPIC` | No | Progress topic. Defaults to the value from `nauron-contracts`. |
| `MIR_RESULT_TOPIC` | No | Result topic. Defaults to the value from `nauron-contracts`. |
| `MIR_RETRY_TOPIC` | No | Retry topic. Defaults to the value from `nauron-contracts`. |
| `AZURE_DI_ENDPOINT` | Yes | Azure Document Intelligence endpoint. |
| `AZURE_DI_KEY` | Yes | Azure Document Intelligence API key. |
| `AZURE_DI_MODEL_ID` | No | Azure model id. Defaults to `prebuilt-layout`. |
| `AZURE_DI_API_VERSION` | No | Azure API version. Defaults to `2024-11-30`. |
| `MIR_OUTPUT_ROOT` | No | Local workspace for per-job temporary files. |
| `S3_ENDPOINT` | Yes for S3 input/output | S3-compatible endpoint. |
| `S3_ACCESS_KEY` | Yes for S3 input/output | S3 access key. |
| `S3_SECRET_KEY` | Yes for S3 input/output | S3 secret key. |
| `S3_REGION` | No | S3 region. Defaults to `us-east-1`. |
| `S3_FORCE_PATH_STYLE` | No | Whether to use path-style addressing. Defaults to `true`. |
| `AZURE_DI_PDF_OPTIMIZE_THRESHOLD_BYTES` | No | PDF size threshold before optimization. |
| `AZURE_DI_MAX_PDF_BYTES` | No | Maximum PDF size accepted after optimization. |
| `AZURE_DI_PDF_SPLIT_PAGE_COUNT` | No | Page count used for PDF chunking. |
| `AZURE_DI_SUBPROCESS_TIMEOUT_SECS` | No | Ghostscript and LibreOffice timeout. |
| `KAFKA_TLS_CA` | No | Kafka TLS CA path. |
| `KAFKA_TLS_CERT` | No | Kafka TLS certificate path. |
| `KAFKA_TLS_KEY` | No | Kafka TLS key path. |

`S3_ENDPOINT`, `S3_ACCESS_KEY`, and `S3_SECRET_KEY` must be provided together. Partial S3 configuration is rejected at startup.

## Container

The published image runs `mir-azure` as its entrypoint:

```bash
docker run --rm ghcr.io/nauron-ai/mir-azure:0.1.0
```

Images are published for version tags in `MAJOR.MINOR.PATCH` format.

## Development

```bash
cargo +1.95.0 fmt --check
python3 scripts/loc_check.py 250 rs
cargo +1.95.0 clippy --workspace --all-targets -- -D warnings
cargo +1.95.0 test --workspace --all-targets
```
