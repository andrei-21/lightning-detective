use std::time::Duration;

pub(crate) fn format_duration(duration: &Duration) -> String {
    let secs = duration.as_secs();
    let (days, hrs, mins, secs) = (
        secs / 86400,
        (secs % 86400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days} day{}", plural(days)));
    }
    if hrs > 0 {
        parts.push(format!("{hrs} hour{}", plural(hrs)));
    }
    if mins > 0 {
        parts.push(format!("{mins} min{}", plural(mins)));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{secs} second{}", plural(secs)));
    }
    parts.join(", ")
}

fn plural(number: u64) -> &'static str {
    if number == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::{format_duration, plural};
    use std::time::Duration;

    #[test]
    fn test_plural() {
        assert_eq!(plural(1), "");
        assert_eq!(plural(0), "s");
        assert_eq!(plural(2), "s");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(&Duration::from_secs(0)), "0 seconds");
        assert_eq!(format_duration(&Duration::from_secs(1)), "1 second");
        assert_eq!(format_duration(&Duration::from_secs(61)), "1 min, 1 second");
        assert_eq!(
            format_duration(&Duration::from_secs(90061)),
            "1 day, 1 hour, 1 min, 1 second"
        );
    }
}
