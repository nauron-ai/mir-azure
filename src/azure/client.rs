use reqwest::{Body, Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;
use tokio::time::sleep;
use tracing::{info, warn};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const POLL_MAX_ATTEMPTS: u16 = 120;

#[derive(Debug, Error)]
pub enum AzureClientError {
    #[error("API request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Azure API error (status {0}): {1}")]
    ApiError(StatusCode, String),
    #[error("Polling timeout for operation {0}")]
    Timeout(String),
    #[error("Missing Operation-Location header in response")]
    MissingOperationLocation,
    #[error("Invalid Operation-Location header")]
    InvalidOperationLocation,
    #[error("Azure operation succeeded without markdown content")]
    MissingAnalyzeContent,
    #[error("Unexpected Azure operation status: {0}")]
    UnexpectedOperationStatus(String),
    #[error("Failed to encode Azure error response: {0}")]
    Encode(#[from] serde_json::Error),
}

impl AzureClientError {
    pub fn is_invalid_content_length(&self) -> bool {
        let Self::ApiError(_, body) = self else {
            return false;
        };

        response_contains_code(body, "InvalidContentLength")
    }
}

#[derive(Clone)]
pub struct AzureDocumentClient {
    client: Client,
    endpoint: String,
    api_key: String,
    model_id: String,
    api_version: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeResult {
    pub content: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OperationResponse {
    pub status: String,
    pub analyze_result: Option<AnalyzeResult>,
    pub error: Option<serde_json::Value>,
}

fn response_contains_code(body: &str, code: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.contains(code);
    };

    value_contains_code(&value, code)
}

fn value_contains_code(value: &serde_json::Value, code: &str) -> bool {
    match value {
        serde_json::Value::Array(values) => {
            values.iter().any(|item| value_contains_code(item, code))
        }
        serde_json::Value::Object(map) => {
            let current = map
                .get("code")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|current| current == code);
            current || map.values().any(|item| value_contains_code(item, code))
        }
        _ => false,
    }
}

impl AzureDocumentClient {
    pub fn new(
        endpoint: String,
        api_key: String,
        model_id: String,
        api_version: String,
    ) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;

        Ok(Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_key,
            model_id,
            api_version,
        })
    }

    pub async fn analyze_document(
        &self,
        payload: Vec<u8>,
        content_type: &str,
    ) -> Result<String, AzureClientError> {
        self.analyze_document_body(Body::from(payload), content_type, None)
            .await
    }

    pub async fn analyze_document_stream(
        &self,
        payload: Body,
        content_type: &str,
        content_length: u64,
    ) -> Result<String, AzureClientError> {
        self.analyze_document_body(payload, content_type, Some(content_length))
            .await
    }

    async fn analyze_document_body(
        &self,
        payload: Body,
        content_type: &str,
        content_length: Option<u64>,
    ) -> Result<String, AzureClientError> {
        let url = format!(
            "{}/documentintelligence/documentModels/{}:analyze?api-version={}&outputContentFormat=markdown",
            self.endpoint, self.model_id, self.api_version
        );

        info!("Sending document to Azure DI API: {}", url);

        let mut request = self
            .client
            .post(&url)
            .header("Ocp-Apim-Subscription-Key", &self.api_key)
            .header("Content-Type", content_type);
        if let Some(content_length) = content_length {
            request = request.header("Content-Length", content_length);
        }
        let response = request.body(payload).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(AzureClientError::ApiError(status, text));
        }

        let operation_location = response
            .headers()
            .get("Operation-Location")
            .ok_or(AzureClientError::MissingOperationLocation)?
            .to_str()
            .map_err(|_| AzureClientError::InvalidOperationLocation)?
            .to_string();

        info!(
            "Document submitted. Polling operation: {}",
            operation_location
        );
        self.poll_for_result(&operation_location).await
    }

    async fn poll_for_result(&self, operation_url: &str) -> Result<String, AzureClientError> {
        let mut retries = 0;

        while retries < POLL_MAX_ATTEMPTS {
            sleep(POLL_INTERVAL).await;
            retries += 1;

            let response = self
                .client
                .get(operation_url)
                .header("Ocp-Apim-Subscription-Key", &self.api_key)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await?;
                warn!("Polling returned non-success error: {} - {}", status, text);
                return Err(AzureClientError::ApiError(status, text));
            }

            let op_res: OperationResponse = response.json().await?;
            info!("Operation status: {}", op_res.status);

            match op_res.status.as_str() {
                "succeeded" => {
                    let Some(analyze_result) = op_res.analyze_result else {
                        return Err(AzureClientError::MissingAnalyzeContent);
                    };
                    let Some(content) = analyze_result.content else {
                        return Err(AzureClientError::MissingAnalyzeContent);
                    };
                    return Ok(content);
                }
                "failed" => {
                    let err_str = serde_json::to_string(&op_res.error)?;
                    return Err(AzureClientError::ApiError(
                        StatusCode::BAD_REQUEST,
                        format!("Job failed: {}", err_str),
                    ));
                }
                "notStarted" | "running" => {}
                status => return Err(AzureClientError::UnexpectedOperationStatus(status.into())),
            }
        }

        Err(AzureClientError::Timeout(operation_url.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::AzureClientError;

    #[test]
    fn detects_nested_invalid_content_length_code() {
        let body =
            r#"{"error":{"code":"InvalidRequest","innererror":{"code":"InvalidContentLength"}}}"#;
        let error = AzureClientError::ApiError(StatusCode::BAD_REQUEST, body.to_string());

        assert!(error.is_invalid_content_length());
    }

    #[test]
    fn ignores_unrelated_api_errors() {
        let body = r#"{"error":{"code":"InvalidRequest","innererror":{"code":"OtherCode"}}}"#;
        let error = AzureClientError::ApiError(StatusCode::BAD_REQUEST, body.to_string());

        assert!(!error.is_invalid_content_length());
    }
}
