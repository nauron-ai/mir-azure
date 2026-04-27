use chrono::Utc;
use nauron_contracts::{MirEvent, MirRequest, MirStage, SchemaVersion};

pub fn build_progress_event(
    request: &MirRequest,
    stage: MirStage,
    percent: u8,
    message: impl Into<String>,
) -> MirEvent {
    MirEvent::Progress(nauron_contracts::MirProgress {
        schema_version: SchemaVersion::V1,
        job_id: request.job_id,
        context_id: request.context_id,
        stage,
        percent,
        message: Some(message.into()),
        timestamp: Utc::now(),
    })
}
