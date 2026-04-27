use std::path::Path;

use nauron_contracts::SourceRef;

const APPLICATION_DOC: &str = "application/msword";
const APPLICATION_DOCX: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const APPLICATION_OCTET_STREAM: &str = "application/octet-stream";
const APPLICATION_PDF: &str = "application/pdf";
const IMAGE_BMP: &str = "image/bmp";
const IMAGE_JPEG: &str = "image/jpeg";
const IMAGE_PNG: &str = "image/png";
const IMAGE_TIFF: &str = "image/tiff";

pub fn infer_content_type(source: &SourceRef) -> &'static str {
    let name = source_name(source);

    if has_extension(name, &["bmp"]) {
        return IMAGE_BMP;
    }

    if has_extension(name, &["doc"]) {
        return APPLICATION_DOC;
    }

    if has_extension(name, &["docx"]) {
        return APPLICATION_DOCX;
    }

    if has_extension(name, &["jpeg", "jpg"]) {
        return IMAGE_JPEG;
    }

    if has_extension(name, &["pdf"]) {
        return APPLICATION_PDF;
    }

    if has_extension(name, &["png"]) {
        return IMAGE_PNG;
    }

    if has_extension(name, &["tif", "tiff"]) {
        return IMAGE_TIFF;
    }

    APPLICATION_OCTET_STREAM
}

pub fn is_pdf_content_type(content_type: &str) -> bool {
    content_type.eq_ignore_ascii_case(APPLICATION_PDF)
}

pub fn is_convertible_office_content_type(content_type: &str) -> bool {
    content_type.eq_ignore_ascii_case(APPLICATION_DOC)
        || content_type.eq_ignore_ascii_case(APPLICATION_DOCX)
}

pub fn source_extension(source: &SourceRef) -> Option<String> {
    source_name_extension(source).and_then(validated_extension)
}

fn source_name(source: &SourceRef) -> &str {
    match source {
        SourceRef::S3 { key, .. } => key.as_str(),
        SourceRef::LocalPath { path } => path.as_str(),
    }
}

fn has_extension(name: &str, candidates: &[&str]) -> bool {
    let Some(extension) = name_extension(name) else {
        return false;
    };

    candidates
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn source_name_extension(source: &SourceRef) -> Option<&str> {
    name_extension(source_name(source))
}

fn name_extension(name: &str) -> Option<&str> {
    let file_name = Path::new(name).file_name()?.to_str()?;
    Path::new(file_name).extension()?.to_str()
}

fn validated_extension(extension: &str) -> Option<String> {
    extension
        .chars()
        .all(|character| character.is_ascii_alphanumeric())
        .then(|| extension.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        infer_content_type, is_convertible_office_content_type, is_pdf_content_type,
        source_extension,
    };
    use nauron_contracts::SourceRef;

    #[test]
    fn infers_pdf_content_type_from_s3_key() {
        let source = SourceRef::S3 {
            bucket: String::from("bucket"),
            key: String::from("docs/input.PDF"),
            version_id: None,
        };

        assert_eq!(infer_content_type(&source), "application/pdf");
    }

    #[test]
    fn infers_jpeg_content_type_from_local_path() {
        let source = SourceRef::LocalPath {
            path: String::from("/tmp/input.JpEg"),
        };

        assert_eq!(infer_content_type(&source), "image/jpeg");
    }

    #[test]
    fn detects_pdf_content_type_case_insensitively() {
        assert!(is_pdf_content_type("APPLICATION/PDF"));
    }

    #[test]
    fn infers_docx_content_type_from_s3_key() {
        let source = SourceRef::S3 {
            bucket: String::from("bucket"),
            key: String::from("docs/input.DocX"),
            version_id: None,
        };

        assert_eq!(
            infer_content_type(&source),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
    }

    #[test]
    fn detects_convertible_office_content_type() {
        assert!(is_convertible_office_content_type(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ));
    }

    #[test]
    fn extracts_source_extension_case_preserving() {
        let source = SourceRef::LocalPath {
            path: String::from("/tmp/file.Docx"),
        };

        assert_eq!(source_extension(&source), Some(String::from("Docx")));
    }

    #[test]
    fn ignores_dotted_parent_directory_without_file_extension() {
        let source = SourceRef::S3 {
            bucket: String::from("bucket"),
            key: String::from("incoming.v1/input"),
            version_id: None,
        };

        assert_eq!(infer_content_type(&source), "application/octet-stream");
        assert_eq!(source_extension(&source), None);
    }

    #[test]
    fn rejects_non_alphanumeric_extension() {
        let source = SourceRef::LocalPath {
            path: String::from("/tmp/file.do-cx"),
        };

        assert_eq!(source_extension(&source), None);
    }
}
