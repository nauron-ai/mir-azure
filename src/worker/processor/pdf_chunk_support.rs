use std::path::{Path, PathBuf};

use super::submission_error::DocumentSubmissionError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PageRange {
    pub(super) first: usize,
    pub(super) last: usize,
}

impl PageRange {
    pub(super) fn page_count(self) -> usize {
        self.last - self.first + 1
    }

    pub(super) fn split(self) -> (Self, Self) {
        debug_assert!(self.page_count() > 1);
        let midpoint = self.first + self.page_count() / 2 - 1;
        (
            Self {
                first: self.first,
                last: midpoint,
            },
            Self {
                first: midpoint + 1,
                last: self.last,
            },
        )
    }
}

pub(super) fn build_initial_ranges(page_count: usize, chunk_size: usize) -> Vec<PageRange> {
    let chunk_count = page_count.div_ceil(chunk_size);

    (0..chunk_count)
        .map(|index| {
            let first = index * chunk_size + 1;
            let last = (first + chunk_size - 1).min(page_count);
            PageRange { first, last }
        })
        .collect()
}

pub(super) fn build_chunk_path(
    input_path: &Path,
    range: PageRange,
) -> Result<PathBuf, std::io::Error> {
    let parent = input_path
        .parent()
        .ok_or_else(|| std::io::Error::other("input path has no parent"))?;
    let stem = input_path
        .file_stem()
        .ok_or_else(|| std::io::Error::other("input path has no file stem"))?
        .to_string_lossy()
        .to_string();

    Ok(parent.join(format!(
        "{stem}.chunk-{:04}-{:04}.pdf",
        range.first, range.last
    )))
}

pub(super) fn build_rasterized_chunk_path(
    chunk_path: &Path,
    range: PageRange,
) -> Result<PathBuf, std::io::Error> {
    let parent = chunk_path
        .parent()
        .ok_or_else(|| std::io::Error::other("chunk path has no parent"))?;
    let stem = chunk_path
        .file_stem()
        .ok_or_else(|| std::io::Error::other("chunk path has no file stem"))?
        .to_string_lossy()
        .to_string();

    Ok(parent.join(format!(
        "{stem}.rasterized-{:04}-{:04}.png",
        range.first, range.last
    )))
}

pub(super) async fn finalize_chunk_path(
    chunk_path: &Path,
    result: Result<String, DocumentSubmissionError>,
) -> Result<String, DocumentSubmissionError> {
    match tokio::fs::remove_file(chunk_path).await {
        Ok(()) => result,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => result,
        Err(_) => result,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{build_chunk_path, build_initial_ranges, build_rasterized_chunk_path, PageRange};

    #[test]
    fn builds_single_range_when_chunk_is_larger_than_document() {
        let ranges = build_initial_ranges(3, 50);

        assert_eq!(ranges, vec![PageRange { first: 1, last: 3 }]);
    }

    #[test]
    fn builds_multiple_ranges_for_exact_chunk_multiple() {
        let ranges = build_initial_ranges(100, 50);

        assert_eq!(
            ranges,
            vec![
                PageRange { first: 1, last: 50 },
                PageRange {
                    first: 51,
                    last: 100,
                },
            ]
        );
    }

    #[test]
    fn splits_range_into_two_non_overlapping_halves() {
        let range = PageRange { first: 1, last: 5 };

        assert_eq!(
            range.split(),
            (
                PageRange { first: 1, last: 2 },
                PageRange { first: 3, last: 5 },
            )
        );
    }

    #[test]
    fn rejects_chunk_path_without_parent() {
        let result = build_chunk_path(Path::new(""), PageRange { first: 1, last: 2 });

        assert!(result.is_err());
    }

    #[test]
    fn builds_rasterized_chunk_path_next_to_chunk() {
        let path = build_rasterized_chunk_path(
            Path::new("/tmp/input_document.chunk-0001-0003.pdf"),
            PageRange { first: 1, last: 3 },
        )
        .unwrap();

        assert_eq!(
            path,
            Path::new("/tmp/input_document.chunk-0001-0003.rasterized-0001-0003.png")
        );
    }
}
