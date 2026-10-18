//! Durations and sizes as config files write them: `250ms`, `5s`, `2m`, `1h`, `1d`; `500KB`, `64MiB`.

use std::time::Duration;

/// A byte count, written as a number or with a unit such as `10MiB` or `500KB`.
#[must_use]
pub fn size(s: &str) -> Option<u64> {
    let s = s.trim();
    let at = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (n, unit) = (
        s[..at].parse::<u64>().ok()?,
        s[at..].trim().to_ascii_lowercase(),
    );
    let mul: u64 = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1000,
        "kib" => 1 << 10,
        "m" | "mb" => 1_000_000,
        "mib" => 1 << 20,
        "g" | "gb" => 1_000_000_000,
        "gib" => 1 << 30,
        _ => return None,
    };
    n.checked_mul(mul)
}

/// A duration, written as seconds or with a unit: `ms`, `s`, `m`, `h` or `d`.
#[must_use]
pub fn duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let at = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let n = s[..at].parse::<u64>().ok()?;
    Some(match s[at..].trim() {
        "ms" => Duration::from_millis(n),
        "" | "s" => Duration::from_secs(n),
        "m" => Duration::from_mins(n),
        "h" => Duration::from_hours(n),
        "d" => Duration::from_hours(n.checked_mul(24)?),
        _ => return None,
    })
}

/// The shortest way to write `d` that reads back the same.
#[must_use]
pub fn show(d: Duration) -> String {
    let ms = d.as_millis();
    let units: [(u128, &str); 4] = [
        (86_400_000, "d"),
        (3_600_000, "h"),
        (60_000, "m"),
        (1000, "s"),
    ];
    units
        .iter()
        .find(|(n, _)| ms >= *n && ms.is_multiple_of(*n))
        .map_or_else(|| format!("{ms}ms"), |(n, u)| format!("{}{u}", ms / n))
}

/// The shortest way to write `n` bytes that reads back the same.
#[must_use]
pub fn show_size(n: u64) -> String {
    [(1 << 30, "GiB"), (1 << 20, "MiB"), (1 << 10, "KiB")]
        .iter()
        .find(|(m, _)| n >= *m && n.is_multiple_of(*m))
        .map_or_else(|| n.to_string(), |(m, u)| format!("{}{u}", n / m))
}

#[cfg(test)]
mod tests {
    use super::{duration, show, show_size, size};
    use std::time::Duration;

    #[test]
    fn reads_and_writes_sizes_and_durations() {
        assert_eq!(size("10MiB"), Some(10 << 20));
        assert_eq!(size("500 KB"), Some(500_000));
        assert_eq!(size("7"), Some(7));
        assert_eq!(size("1PB"), None);
        assert_eq!(duration("250ms"), Some(Duration::from_millis(250)));
        assert_eq!(duration("2m"), Some(Duration::from_mins(2)));
        assert_eq!(duration("3"), Some(Duration::from_secs(3)));
        assert_eq!(duration("7d"), Some(Duration::from_hours(168)));
        assert_eq!(duration("1w"), None);
        assert_eq!(show(Duration::from_secs(90)), "90s");
        assert_eq!(show(Duration::from_hours(1)), "1h");
        assert_eq!(show(Duration::from_millis(1500)), "1500ms");
        assert_eq!(show_size(64 << 20), "64MiB");
        assert_eq!(show_size(1000), "1000");
    }
}
