use std::time::Duration;

const INITIAL_RETRY_DELAY_SECS: u64 = 1;
const MAX_RETRY_DELAY_SECS: u64 = 8;
const MAX_RETRY_SHIFT: u32 = 3;

pub(super) fn retry_delay(attempt: usize) -> Duration {
    let shift = (attempt as u32).min(MAX_RETRY_SHIFT);
    let delay_secs = (INITIAL_RETRY_DELAY_SECS << shift).min(MAX_RETRY_DELAY_SECS);

    Duration::from_secs(delay_secs)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::retry_delay;

    #[test]
    fn caps_exponential_retry_delay() {
        let delays = (0..6).map(retry_delay).collect::<Vec<_>>();

        assert_eq!(
            delays,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
                Duration::from_secs(8),
                Duration::from_secs(8),
            ]
        );
    }
}
