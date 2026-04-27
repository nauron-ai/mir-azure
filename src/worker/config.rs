use clap::Parser;
use nauron_contracts::{MIR_PROGRESS_TOPIC, MIR_REQUEST_TOPIC, MIR_RESULT_TOPIC, MIR_RETRY_TOPIC};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

const DEFAULT_PDF_MAX_BYTES: u64 = 500 * 1024 * 1024;
const DEFAULT_PDF_OPTIMIZE_THRESHOLD_BYTES: u64 = 400 * 1024 * 1024;
const DEFAULT_PDF_SPLIT_PAGE_COUNT: usize = 50;
const DEFAULT_SUBPROCESS_TIMEOUT_SECS: u64 = 600;

#[derive(Parser, Debug, Clone)]
#[command(name = "mir-azure-worker")]
pub struct WorkerArgs {
    #[arg(long, env = "KAFKA_BROKERS", default_value = "127.0.0.1:9093")]
    pub brokers: String,

    #[arg(long, env = "KAFKA_GROUP_ID", default_value = "mir-azure-worker")]
    pub group_id: String,

    #[arg(long, env = "MIR_REQUEST_TOPIC", default_value = MIR_REQUEST_TOPIC)]
    pub request_topic: String,

    #[arg(long, env = "MIR_PROGRESS_TOPIC", default_value = MIR_PROGRESS_TOPIC)]
    pub progress_topic: String,

    #[arg(long, env = "MIR_RESULT_TOPIC", default_value = MIR_RESULT_TOPIC)]
    pub result_topic: String,

    #[arg(long, env = "MIR_RETRY_TOPIC", default_value = MIR_RETRY_TOPIC)]
    pub retry_topic: String,

    #[arg(long, env = "AZURE_DI_ENDPOINT")]
    pub azure_di_endpoint: String,

    #[arg(long, env = "AZURE_DI_KEY")]
    pub azure_di_key: String,

    #[arg(long, env = "AZURE_DI_MODEL_ID", default_value = "prebuilt-layout")]
    pub azure_di_model_id: String,

    #[arg(long, env = "AZURE_DI_API_VERSION", default_value = "2024-11-30")]
    pub azure_di_api_version: String,

    #[arg(long, env = "MIR_OUTPUT_ROOT", default_value = "/tmp/mir-azure-output")]
    pub output_root: PathBuf,

    #[arg(
        long,
        env = "AZURE_DI_PDF_OPTIMIZE_THRESHOLD_BYTES",
        default_value_t = DEFAULT_PDF_OPTIMIZE_THRESHOLD_BYTES
    )]
    pub pdf_optimize_threshold_bytes: u64,

    #[arg(
        long,
        env = "AZURE_DI_MAX_PDF_BYTES",
        default_value_t = DEFAULT_PDF_MAX_BYTES
    )]
    pub pdf_max_bytes: u64,

    #[arg(
        long,
        env = "AZURE_DI_PDF_SPLIT_PAGE_COUNT",
        default_value_t = DEFAULT_PDF_SPLIT_PAGE_COUNT,
        value_parser = parse_positive_usize
    )]
    pub pdf_split_page_count: usize,

    #[arg(
        long,
        env = "AZURE_DI_SUBPROCESS_TIMEOUT_SECS",
        default_value_t = DEFAULT_SUBPROCESS_TIMEOUT_SECS,
        value_parser = parse_positive_u64
    )]
    pub subprocess_timeout_secs: u64,

    #[arg(long, env = "KAFKA_TLS_CA")]
    pub tls_ca: Option<PathBuf>,

    #[arg(long, env = "KAFKA_TLS_CERT")]
    pub tls_cert: Option<PathBuf>,

    #[arg(long, env = "KAFKA_TLS_KEY")]
    pub tls_key: Option<PathBuf>,

    #[arg(long, env = "S3_ENDPOINT")]
    pub s3_endpoint: Option<String>,

    #[arg(long, env = "S3_ACCESS_KEY")]
    pub s3_access_key: Option<String>,

    #[arg(long, env = "S3_SECRET_KEY")]
    pub s3_secret_key: Option<String>,

    #[arg(long, env = "S3_REGION", default_value = "us-east-1")]
    pub s3_region: String,

    #[arg(long, env = "S3_FORCE_PATH_STYLE", default_value_t = true)]
    pub s3_force_path_style: bool,
}

impl WorkerArgs {
    pub fn effective_pdf_optimize_threshold_bytes(&self) -> u64 {
        self.pdf_optimize_threshold_bytes.min(self.pdf_max_bytes)
    }

    pub fn tls_enabled(&self) -> bool {
        self.tls_ca.is_some() || self.tls_cert.is_some() || self.tls_key.is_some()
    }

    pub fn subprocess_timeout(&self) -> Duration {
        Duration::from_secs(self.subprocess_timeout_secs)
    }
}

fn parse_positive_u64(value: &str) -> Result<u64, String> {
    parse_positive_number::<u64>(value)
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    parse_positive_number::<usize>(value)
}

fn parse_positive_number<T>(value: &str) -> Result<T, String>
where
    T: FromStr + PartialEq + Default,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| format!("invalid positive number: {value}"))?;

    if parsed == T::default() {
        return Err(String::from("value must be greater than zero"));
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::WorkerArgs;

    #[test]
    fn rejects_zero_pdf_split_page_count() {
        let result = WorkerArgs::try_parse_from([
            "mir-azure-worker",
            "--azure-di-endpoint",
            "https://example.com",
            "--azure-di-key",
            "key",
            "--pdf-split-page-count",
            "0",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn rejects_zero_subprocess_timeout_secs() {
        let result = WorkerArgs::try_parse_from([
            "mir-azure-worker",
            "--azure-di-endpoint",
            "https://example.com",
            "--azure-di-key",
            "key",
            "--subprocess-timeout-secs",
            "0",
        ]);

        assert!(result.is_err());
    }
}
