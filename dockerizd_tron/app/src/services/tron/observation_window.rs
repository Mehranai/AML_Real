pub const ALL_HISTORY_WINDOW_DAYS: u16 = 0;
const MAX_BOUNDED_WINDOW_DAYS: u16 = 3_650;

pub fn normalize_window_days(requested_days: Option<u16>) -> u16 {
    match requested_days {
        None | Some(ALL_HISTORY_WINDOW_DAYS) => ALL_HISTORY_WINDOW_DAYS,
        Some(days) => days.min(MAX_BOUNDED_WINDOW_DAYS),
    }
}

pub fn window_start_unix_ms(now_unix_ms: u64, window_days: u16) -> u64 {
    if window_days == ALL_HISTORY_WINDOW_DAYS {
        return 0;
    }

    now_unix_ms.saturating_sub(u64::from(window_days) * 24 * 60 * 60 * 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_all_indexed_history() {
        assert_eq!(normalize_window_days(None), ALL_HISTORY_WINDOW_DAYS);
        assert_eq!(
            normalize_window_days(Some(ALL_HISTORY_WINDOW_DAYS)),
            ALL_HISTORY_WINDOW_DAYS
        );
        assert_eq!(window_start_unix_ms(123_456, ALL_HISTORY_WINDOW_DAYS), 0);
    }

    #[test]
    fn bounds_explicit_windows_and_computes_the_start() {
        assert_eq!(normalize_window_days(Some(90)), 90);
        assert_eq!(normalize_window_days(Some(u16::MAX)), 3_650);
        assert_eq!(window_start_unix_ms(100_000_000, 1), 13_600_000);
    }
}
