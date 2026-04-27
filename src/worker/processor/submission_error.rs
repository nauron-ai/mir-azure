use thiserror::Error;

use crate::azure::AzureClientError;

use super::office::OfficeConversionError;
use super::pdf::{PdfOptimizationError, PdfSplitError};
use super::pdf_raster::PdfRasterizationError;

#[derive(Debug, Error)]
pub enum DocumentSubmissionError {
    #[error("Azure API error: {0}")]
    Azure(#[from] AzureClientError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF optimization failed before Azure submission: {0}")]
    PdfOptimization(#[source] PdfOptimizationError),
    #[error("Azure rejected the PDF and recovery failed: {source}. Azure error: {azure}")]
    PdfRetry {
        azure: AzureClientError,
        #[source]
        source: Box<DocumentSubmissionError>,
    },
    #[error(
        "PDF remains too large after optimization: before {before}, after {after}, limit {limit}"
    )]
    PdfTooLargeAfterOptimization {
        before: String,
        after: String,
        limit: String,
    },
    #[error("PDF chunk split failed before Azure submission: {0}")]
    PdfSplit(#[source] PdfSplitError),
    #[error("PDF rasterization failed before Azure submission: {0}")]
    PdfRasterization(#[source] PdfRasterizationError),
    #[error("Office to PDF conversion failed before Azure submission: {0}")]
    OfficeConversion(#[source] OfficeConversionError),
    #[error(
        "Azure rejected the Office document and PDF recovery failed: {source}. Azure error: {azure}"
    )]
    OfficeRetry {
        azure: AzureClientError,
        #[source]
        source: Box<DocumentSubmissionError>,
    },
}

impl DocumentSubmissionError {
    pub fn wrap_office_retry(azure: AzureClientError, source: DocumentSubmissionError) -> Self {
        Self::OfficeRetry {
            azure,
            source: Box::new(source),
        }
    }

    pub fn wrap_pdf_retry(azure: AzureClientError, source: DocumentSubmissionError) -> Self {
        Self::PdfRetry {
            azure,
            source: Box::new(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use crate::azure::AzureClientError;

    use super::DocumentSubmissionError;

    #[test]
    fn wraps_any_error_for_office_retry() {
        let wrapped = DocumentSubmissionError::wrap_office_retry(
            AzureClientError::ApiError(StatusCode::BAD_REQUEST, String::from("azure")),
            DocumentSubmissionError::PdfTooLargeAfterOptimization {
                before: String::from("1 MiB"),
                after: String::from("2 MiB"),
                limit: String::from("1.5 MiB"),
            },
        );

        assert!(matches!(
            wrapped,
            DocumentSubmissionError::OfficeRetry { .. }
        ));
    }

    #[test]
    fn wraps_any_error_for_pdf_retry() {
        let wrapped = DocumentSubmissionError::wrap_pdf_retry(
            AzureClientError::ApiError(StatusCode::BAD_REQUEST, String::from("azure")),
            DocumentSubmissionError::PdfTooLargeAfterOptimization {
                before: String::from("1 MiB"),
                after: String::from("2 MiB"),
                limit: String::from("1.5 MiB"),
            },
        );

        assert!(matches!(wrapped, DocumentSubmissionError::PdfRetry { .. }));
    }
}
