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
    let name = format!("Bediz {year:04}-{month:02}-{day:02} {hour:02}{minute:02}{second:02}.png");
    numbered(&name, attempt)
}

pub fn temporary(name: &str) -> String {
    format!("{name}.tmp")
}

pub fn scratch(attempt: u32) -> String {
    temporary(&numbered("~Bediz", attempt))
}

pub fn numbered(name: &str, attempt: u32) -> String {
    if attempt <= 1 {
        return name.to_owned();
    }
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => format!("{stem} ({attempt}).{extension}"),
        _ => format!("{name} ({attempt})"),
    }
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
    fn numbers_go_before_the_extension() {
        assert_eq!(numbered("notes.png", 1), "notes.png");
        assert_eq!(numbered("notes.png", 3), "notes (3).png");
        assert_eq!(numbered("archive.tar.png", 2), "archive.tar (2).png");
        assert_eq!(numbered("README", 2), "README (2)");
        assert_eq!(numbered(".png", 2), ".png (2)");
    }

    #[test]
    fn scratch_names_stay_short_whatever_the_final_name() {
        assert_eq!(scratch(1), "~Bediz.tmp");
        assert_eq!(scratch(2), "~Bediz (2).tmp");
        assert!(scratch(1000).len() < 20);
    }

    #[test]
    fn a_temporary_name_keeps_the_final_name_in_front() {
        assert_eq!(
            temporary(&screenshot(&TIME, 1)),
            "Bediz 2026-03-07 090502.png.tmp"
        );
    }
}
