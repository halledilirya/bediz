#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub fn screenshot(time: &LocalTime, attempt: u32) -> String {
    let LocalTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    } = *time;
    let stem = format!("Bediz {year:04}-{month:02}-{day:02} {hour:02}{minute:02}{second:02}");
    if attempt <= 1 {
        format!("{stem}.png")
    } else {
        format!("{stem} ({attempt}).png")
    }
}

pub fn temporary(name: &str) -> String {
    format!("{name}.tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: LocalTime = LocalTime {
        year: 2026,
        month: 3,
        day: 7,
        hour: 9,
        minute: 5,
        second: 2,
    };

    #[test]
    fn a_screenshot_is_named_by_its_date_and_time() {
        assert_eq!(screenshot(&TIME, 1), "Bediz 2026-03-07 090502.png");
    }

    #[test]
    fn later_attempts_get_a_number() {
        assert_eq!(screenshot(&TIME, 2), "Bediz 2026-03-07 090502 (2).png");
        assert_eq!(screenshot(&TIME, 10), "Bediz 2026-03-07 090502 (10).png");
        assert_eq!(screenshot(&TIME, 0), screenshot(&TIME, 1));
    }

    #[test]
    fn a_temporary_name_keeps_the_final_name_in_front() {
        assert_eq!(
            temporary(&screenshot(&TIME, 1)),
            "Bediz 2026-03-07 090502.png.tmp"
        );
    }
}
