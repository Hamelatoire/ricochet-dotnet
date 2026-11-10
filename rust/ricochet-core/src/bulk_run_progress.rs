//! Port of `src/Ricochet.Core/BulkRunProgress.cs`. The numbers behind a bulk run's progress bar and status line.

/// Whole percent of the ids that are processed, from 0 to 100. Zero when there are no ids.
pub fn percent(processed: i32, total: i32) -> i32 {
    if total <= 0 {
        return 0;
    }

    (i64::from(processed) * 100 / i64::from(total)).clamp(0, 100) as i32
}

/// A short line for the progress bar, such as "3 of 10 processed (30%)". Empty when there are no ids.
pub fn describe(processed: i32, total: i32) -> String {
    if total <= 0 {
        return String::new();
    }

    format!("{processed} of {total} processed ({}%)", percent(processed, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_is_whole_and_never_passes_100() {
        assert_eq!(percent(0, 10), 0);
        assert_eq!(percent(3, 10), 30);
        assert_eq!(percent(9, 10), 90);
        assert_eq!(percent(10, 10), 100);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(0, 0), 0);
    }

    #[test]
    fn percent_stays_at_99_until_the_last_id_is_done() {
        assert_eq!(percent(99, 100), 99);
        assert_eq!(percent(100, 100), 100);
    }

    #[test]
    fn percent_clamps_out_of_range_counts() {
        assert_eq!(percent(-5, 10), 0);
        assert_eq!(percent(15, 10), 100);
    }

    #[test]
    fn describe_names_the_counts_and_the_percent() {
        assert_eq!(describe(3, 10), "3 of 10 processed (30%)");
        assert_eq!(describe(0, 0), "");
    }
}
