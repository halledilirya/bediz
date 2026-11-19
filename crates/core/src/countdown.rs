//! The number a countdown shows for the time left, and when that number changes next.

use std::time::Duration;

pub fn number(remaining: Duration) -> u64 {
    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
}

pub fn until_change(remaining: Duration) -> Duration {
    let after = number(remaining).saturating_sub(1);
    remaining.saturating_sub(Duration::from_secs(after))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{number, until_change};

    #[test]
    fn a_countdown_shows_whole_seconds_left_rounded_up() {
        assert_eq!(number(Duration::from_secs(3)), 3);
        assert_eq!(number(Duration::from_millis(2_001)), 3);
        assert_eq!(number(Duration::from_millis(2_000)), 2);
        assert_eq!(number(Duration::from_millis(1)), 1);
        assert_eq!(number(Duration::ZERO), 0);
    }

    #[test]
    fn a_countdown_changes_when_its_number_would() {
        let changes = |milliseconds| until_change(Duration::from_millis(milliseconds)).as_millis();
        assert_eq!(changes(10_000), 1_000);
        assert_eq!(changes(3_000), 1_000);
        assert_eq!(changes(2_500), 500);
        assert_eq!(changes(2_001), 1);
        assert_eq!(changes(2_000), 1_000);
        assert_eq!(changes(1), 1);
        assert_eq!(changes(0), 0);
        for left in [10_000, 5_000, 2_750, 999] {
            let left = Duration::from_millis(left);
            assert_eq!(number(left - until_change(left)), number(left) - 1);
        }
    }
}
