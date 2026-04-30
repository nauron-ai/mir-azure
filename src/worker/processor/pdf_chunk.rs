use std::path::Path;

use nauron_contracts::{MirRequest, MirStage};
use tracing::info;

use super::document::{file_size, format_size, PreparedDocument};
use super::events::EventRecorder;
use super::pdf_chunk_support::{
    build_chunk_path, build_initial_ranges, build_rasterized_chunk_path, finalize_chunk_path,
    PageRange,
};
use super::progress::build_progress_event;
use super::submission::submit_prepared_document;
use super::submission_error::DocumentSubmissionError;
use crate::worker::processor::WorkerContext;

const CHUNK_PROGRESS_START: u8 = 66;
const CHUNK_PROGRESS_END: u8 = 78;

pub async fn analyze_pdf_in_chunks(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
) -> Result<String, DocumentSubmissionError> {
    let page_count = super::pdf::count_pdf_pages(input_path, ctx.config().subprocess_timeout())
        .await
        .map_err(DocumentSubmissionError::PdfSplit)?;
    let chunk_size = ctx.config().pdf_split_page_count;
    let ranges = build_initial_ranges(page_count, chunk_size);
    info!(
        job_id = %request.job_id,
        pages = page_count,
        chunk_size,
        chunk_count = ranges.len(),
        "Splitting PDF into chunks for Azure retry"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        65,
        format!(
            "splitting pdf into {} chunks for Azure retry ({} pages)",
            ranges.len(),
            page_count
        ),
    ));

    let mut markdown_parts = Vec::with_capacity(ranges.len());
    let range_count = ranges.len();
    for (index, range) in ranges.into_iter().enumerate() {
        let percent = chunk_progress_percent(index, range_count);
        let markdown = analyze_pdf_range(request, ctx, input_path, events, range, percent).await?;
        markdown_parts.push(markdown);
    }

    Ok(markdown_parts.join("\n\n"))
}

async fn analyze_pdf_range(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
    range: PageRange,
    percent: u8,
) -> Result<String, DocumentSubmissionError> {
    let chunk_path = build_chunk_path(input_path, range)?;
    let result = async {
        super::pdf::split_pdf_range(
            input_path,
            &chunk_path,
            range.first,
            range.last,
            ctx.config().subprocess_timeout(),
        )
        .await
        .map_err(DocumentSubmissionError::PdfSplit)?;
        let chunk_size_bytes = file_size(&chunk_path).await?;
        let chunk = PreparedDocument::new(&chunk_path, "application/pdf", chunk_size_bytes, false);
        events.push(build_progress_event(
            request,
            MirStage::ProcessingRun,
            percent,
            format!(
                "analyzing pdf chunk pages {}-{} via Azure DI ({})",
                range.first,
                range.last,
                format_size(chunk.size_bytes)
            ),
        ));

        match submit_prepared_document(request, ctx, &chunk, events, percent).await {
            Ok(markdown) => Ok(markdown),
            Err(DocumentSubmissionError::Azure(error))
                if error.is_invalid_content_length() && range.page_count() > 1 =>
            {
                split_failing_range(request, ctx, input_path, events, range, percent).await
            }
            Err(DocumentSubmissionError::Azure(error))
                if error.is_invalid_content_length() && range.page_count() == 1 =>
            {
                retry_single_page_as_rasterized_pdf(
                    request,
                    ctx,
                    &chunk_path,
                    events,
                    range,
                    percent,
                )
                .await
            }
            Err(error) => Err(error),
        }
    }
    .await;
    finalize_chunk_path(&chunk_path, result).await
}

async fn split_failing_range(
    request: &MirRequest,
    ctx: &WorkerContext,
    input_path: &Path,
    events: &mut EventRecorder<'_>,
    range: PageRange,
    percent: u8,
) -> Result<String, DocumentSubmissionError> {
    let (left, right) = range.split();
    info!(
        job_id = %request.job_id,
        first = range.first,
        last = range.last,
        left_first = left.first,
        left_last = left.last,
        right_first = right.first,
        right_last = right.last,
        "Splitting failing PDF chunk for Azure retry"
    );
    events.push(build_progress_event(
        request,
        MirStage::ProcessingRun,
        percent,
        format!(
            "splitting failing pdf chunk pages {}-{} into {}-{} and {}-{}",
            range.first, range.last, left.first, left.last, right.first, right.last
        ),
    ));

    let left_markdown = Box::pin(analyze_pdf_range(
        request, ctx, input_path, events, left, percent,
    ))
    .await?;
    let right_markdown = Box::pin(analyze_pdf_range(
        request, ctx, input_path, events, right, percent,
    ))
    .await?;
    Ok(format!("{left_markdown}\n\n{right_markdown}"))
}

async fn retry_single_page_as_rasterized_pdf(
    request: &MirRequest,
    ctx: &WorkerContext,
    chunk_path: &Path,
    events: &mut EventRecorder<'_>,
    range: PageRange,
    percent: u8,
) -> Result<String, DocumentSubmissionError> {
    let rasterized_path = build_rasterized_chunk_path(chunk_path, range)?;
    let result = async {
        info!(
            job_id = %request.job_id,
            first = range.first,
            last = range.last,
            "Rasterizing single-page PDF chunk for Azure retry"
        );
        events.push(build_progress_event(
            request,
            MirStage::ProcessingRun,
            percent,
            format!(
                "rasterizing single-page pdf chunk {}-{} to png for Azure retry",
                range.first, range.last
            ),
        ));
        super::pdf_raster::rasterize_pdf_page_to_png(
            chunk_path,
            &rasterized_path,
            ctx.config().subprocess_timeout(),
        )
        .await
        .map_err(DocumentSubmissionError::PdfRasterization)?;
        let rasterized_size = file_size(&rasterized_path).await?;
        let rasterized =
            PreparedDocument::new(&rasterized_path, "image/png", rasterized_size, true);
        submit_prepared_document(request, ctx, &rasterized, events, percent).await
    }
    .await;
    finalize_chunk_path(&rasterized_path, result).await
}

fn chunk_progress_percent(index: usize, total: usize) -> u8 {
    if total <= 1 {
        return CHUNK_PROGRESS_START;
    }

    let span = usize::from(CHUNK_PROGRESS_END - CHUNK_PROGRESS_START);
    let offset = span * index / total.saturating_sub(1);
    CHUNK_PROGRESS_START + offset as u8
}

#[cfg(test)]
mod tests {
    use super::chunk_progress_percent;

    #[test]
    fn chunk_progress_is_stable_for_single_chunk() {
        assert_eq!(chunk_progress_percent(0, 1), 66);
    }

    #[test]
    fn chunk_progress_reaches_final_chunk_percent() {
        assert_eq!(chunk_progress_percent(0, 3), 66);
        assert_eq!(chunk_progress_percent(1, 3), 72);
        assert_eq!(chunk_progress_percent(2, 3), 78);
    }
}
