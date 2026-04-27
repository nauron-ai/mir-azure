use crate::azure::AzureDocumentClient;
use crate::worker::config::WorkerArgs;
use crate::worker::storage::{StorageClient, StorageError, StorageSettings};
use nauron_contracts::MirEvent;
use thiserror::Error;

pub struct WorkerContext {
    config: WorkerArgs,
    storage: Option<StorageClient>,
    azure_client: AzureDocumentClient,
}

pub struct WorkerOutput {
    pub events: Vec<MirEvent>,
}

#[derive(Debug, Error)]
pub enum WorkerContextError {
    #[error("S3 storage config requires S3_ENDPOINT, S3_ACCESS_KEY, and S3_SECRET_KEY")]
    IncompleteS3Config,
    #[error("storage init failed: {0}")]
    Storage(Box<StorageError>),
}

impl From<StorageError> for WorkerContextError {
    fn from(error: StorageError) -> Self {
        Self::Storage(Box::new(error))
    }
}

impl WorkerContext {
    pub async fn new(
        config: WorkerArgs,
        azure_client: AzureDocumentClient,
    ) -> Result<Self, WorkerContextError> {
        let storage = match (
            config.s3_endpoint.clone(),
            config.s3_access_key.clone(),
            config.s3_secret_key.clone(),
        ) {
            (Some(endpoint), Some(access_key), Some(secret_key)) => {
                let settings = StorageSettings {
                    endpoint,
                    access_key,
                    secret_key,
                    region: config.s3_region.clone(),
                    force_path_style: config.s3_force_path_style,
                };
                Some(StorageClient::new(settings).await?)
            }
            (None, None, None) => None,
            _ => return Err(WorkerContextError::IncompleteS3Config),
        };

        Ok(Self {
            config,
            storage,
            azure_client,
        })
    }

    pub fn config(&self) -> &WorkerArgs {
        &self.config
    }

    pub fn azure_client(&self) -> &AzureDocumentClient {
        &self.azure_client
    }

    pub fn storage(&self) -> Option<&StorageClient> {
        self.storage.as_ref()
    }
}
