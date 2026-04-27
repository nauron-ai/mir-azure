use aws_config::BehaviorVersion;
use aws_sdk_s3::client::Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Credentials, Region};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::put_object::PutObjectError;
use aws_sdk_s3::primitives::ByteStream;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Clone)]
pub struct StorageSettings {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub force_path_style: bool,
}

#[derive(Clone)]
pub struct StorageClient {
    client: Client,
}

impl StorageClient {
    pub async fn new(settings: StorageSettings) -> Result<Self, StorageError> {
        let credentials = Credentials::new(
            settings.access_key,
            settings.secret_key,
            None,
            None,
            "mir-azure",
        );

        let loader = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(Region::new(settings.region))
            .endpoint_url(settings.endpoint);

        let shared = loader.load().await;
        let config = S3ConfigBuilder::from(&shared)
            .force_path_style(settings.force_path_style)
            .build();

        Ok(Self {
            client: Client::from_conf(config),
        })
    }

    pub async fn download_file(
        &self,
        bucket: &str,
        key: &str,
        dest_path: &Path,
    ) -> Result<(), StorageError> {
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let bytes = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(StorageError::from)?;

        let data = bytes
            .body
            .collect()
            .await
            .map_err(|err| StorageError::DownloadBody(err.to_string()))?
            .into_bytes();

        fs::write(dest_path, &data).await?;
        Ok(())
    }

    pub async fn upload_file(
        &self,
        bucket: &str,
        key: &str,
        src_path: &Path,
        content_type: Option<&str>,
    ) -> Result<(), StorageError> {
        let bytes = fs::read(src_path).await?;
        let body = ByteStream::from(bytes);

        let mut req = self.client.put_object().bucket(bucket).key(key).body(body);

        if let Some(ct) = content_type {
            req = req.content_type(ct);
        }

        req.send().await.map_err(StorageError::from)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("s3 download: {0}")]
    Download(#[from] SdkError<GetObjectError>),
    #[error("s3 download body: {0}")]
    DownloadBody(String),
    #[error("s3 upload: {0}")]
    Upload(#[from] SdkError<PutObjectError>),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
