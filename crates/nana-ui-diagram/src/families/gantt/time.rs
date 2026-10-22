//! Dates for gantt charts: Day.js-style input formats, strftime-style axis
//! formats and durations. Every instant is Unix milliseconds read as UTC,
//! so a chart never depends on the machine's zone or clock.

use nana_ui_charts::scale::{civil_from_unix_ms, unix_ms_from_civil};
use nana_ui_core::date::{CivilDate, WeekStart, days_in_month};

pub(crate) const SECOND: i64 = 1_000;
pub(crate) const MINUTE: i64 = 60 * SECOND;
pub(crate) const HOUR: i64 = 60 * MINUTE;
pub(crate) const DAY: i64 = 24 * HOUR;
pub(crate) const WEEK: i64 = 7 * DAY;

/// Years an instant may name; far enough for any plan, small enough that
/// every sum stays inside `i64` milliseconds.
const YEAR_RANGE: std::ops::RangeInclusive<i64> = -9_999..=9_999;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Monday = 0 … Sunday = 6.
pub(crate) fn weekday(ms: i64) -> u32 {
    CivilDate::from_days_since_epoch(ms.div_euclid(DAY))
        .weekday()
        .index_from(WeekStart::Monday)
}

/// A weekday name, full or three letters, any case: Monday = 0.
pub(crate) fn weekday_named(name: &str) -> Option<u32> {
    let name = name.trim();
    WEEKDAYS
        .iter()
        .position(|w| {
            w.eq_ignore_ascii_case(name) || (name.len() == 3 && w[..3].eq_ignore_ascii_case(name))
        })
        .map(|i| i as u32)
}

/// Midnight of the day `ms` falls on.
pub(crate) fn day_start(ms: i64) -> i64 {
    ms.div_euclid(DAY) * DAY
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Lit(char),
    Year4,
    Year2,
    Month { width: u8 },
    MonthName { long: bool },
    Day { width: u8 },
    DayOrdinal,
    Hour24 { width: u8 },
    Hour12 { width: u8 },
    Meridiem,
    Minute { width: u8 },
    Second { width: u8 },
    Fraction { digits: u8 },
    UnixSeconds,
    UnixMillis,
    Offset,
    WeekdayName,
}

fn tokens(fmt: &str) -> Vec<Tok> {
    const TABLE: &[(&str, Tok)] = &[
        ("YYYY", Tok::Year4),
        ("YY", Tok::Year2),
        ("MMMM", Tok::MonthName { long: true }),
        ("MMM", Tok::MonthName { long: false }),
        ("MM", Tok::Month { width: 2 }),
        ("M", Tok::Month { width: 1 }),
        ("Do", Tok::DayOrdinal),
        ("DD", Tok::Day { width: 2 }),
        ("D", Tok::Day { width: 1 }),
        ("dddd", Tok::WeekdayName),
        ("ddd", Tok::WeekdayName),
        ("HH", Tok::Hour24 { width: 2 }),
        ("H", Tok::Hour24 { width: 1 }),
        ("hh", Tok::Hour12 { width: 2 }),
        ("h", Tok::Hour12 { width: 1 }),
        ("A", Tok::Meridiem),
        ("a", Tok::Meridiem),
        ("mm", Tok::Minute { width: 2 }),
        ("m", Tok::Minute { width: 1 }),
        ("ss", Tok::Second { width: 2 }),
        ("s", Tok::Second { width: 1 }),
        ("SSS", Tok::Fraction { digits: 3 }),
        ("SS", Tok::Fraction { digits: 2 }),
        ("S", Tok::Fraction { digits: 1 }),
        ("X", Tok::UnixSeconds),
        ("x", Tok::UnixMillis),
        ("ZZ", Tok::Offset),
        ("Z", Tok::Offset),
    ];
    let mut out = Vec::new();
    let mut rest = fmt;
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            let inner = &rest[1..];
            let end = inner.find(']').unwrap_or(inner.len());
            out.extend(inner[..end].chars().map(Tok::Lit));
            rest = inner.get(end + 1..).unwrap_or("");
            continue;
        }
        if let Some((name, tok)) = TABLE.iter().find(|(name, _)| rest.starts_with(name)) {
            out.push(*tok);
            rest = &rest[name.len()..];
            continue;
        }
        out.push(Tok::Lit(c));
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Up to `max` ASCII digits (at least `min`) taken from the front of `s`.
fn digits(s: &mut &str, min: usize, max: usize) -> Option<i64> {
    let len = s.bytes().take(max).take_while(u8::is_ascii_digit).count();
    if len < min {
        return None;
    }
    let v = s[..len].parse().ok()?;
    *s = &s[len..];
    Some(v)
}

#[derive(Default)]
struct Fields {
    year: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
    hour: i64,
    pm: Option<bool>,
    minute: i64,
    second: i64,
    millis: i64,
    offset_minutes: i64,
    unix: Option<i64>,
}

/// Parse `input` strictly against a Day.js format string.
pub(crate) fn parse_with(input: &str, fmt: &str) -> Option<i64> {
    let mut s = input.trim();
    let mut f = Fields::default();
    for tok in tokens(fmt.trim()) {
        match tok {
            Tok::Lit(c) if c.is_whitespace() => {
                let trimmed = s.trim_start();
                if trimmed.len() == s.len() {
                    return None;
                }
                s = trimmed;
            }
            Tok::Lit(c) => s = s.strip_prefix(c)?,
            Tok::Year4 => f.year = Some(digits(&mut s, 4, 4)?),
            Tok::Year2 => {
                let v = digits(&mut s, 2, 2)?;
                f.year = Some(v + if v > 68 { 1900 } else { 2000 });
            }
            Tok::Month { width } => f.month = Some(digits(&mut s, usize::from(width), 2)?),
            Tok::MonthName { long } => {
                let lower = s.to_ascii_lowercase();
                let (i, n) = MONTHS.iter().enumerate().find_map(|(i, m)| {
                    let m = m.to_ascii_lowercase();
                    let name = if long { m.as_str() } else { &m[..3] };
                    lower.starts_with(name).then_some((i, name.len()))
                })?;
                f.month = Some(i as i64 + 1);
                s = &s[n..];
            }
            Tok::Day { width } => f.day = Some(digits(&mut s, usize::from(width), 2)?),
            Tok::DayOrdinal => {
                f.day = Some(digits(&mut s, 1, 2)?);
                s = ["st", "nd", "rd", "th"]
                    .iter()
                    .find_map(|x| s.strip_prefix(x))?;
            }
            Tok::Hour24 { width } | Tok::Hour12 { width } => {
                f.hour = digits(&mut s, usize::from(width), 2)?
            }
            Tok::Meridiem => {
                f.pm = Some(match s.get(..2)?.to_ascii_lowercase().as_str() {
                    "am" => false,
                    "pm" => true,
                    _ => return None,
                });
                s = &s[2..];
            }
            Tok::Minute { width } => f.minute = digits(&mut s, usize::from(width), 2)?,
            Tok::Second { width } => f.second = digits(&mut s, usize::from(width), 2)?,
            Tok::Fraction { digits: count } => {
                let count = usize::from(count);
                f.millis = digits(&mut s, count, count)? * 10_i64.pow(3 - count.min(3) as u32);
            }
            Tok::UnixSeconds | Tok::UnixMillis => {
                let neg = s.starts_with('-');
                if neg {
                    s = &s[1..];
                }
                let mut value = digits(&mut s, 1, 15)? as f64;
                if tok == Tok::UnixSeconds {
                    if let Some(rest) = s.strip_prefix('.') {
                        let mut frac = rest;
                        if let Some(v) = digits(&mut frac, 1, 3) {
                            value += v as f64 / 10_f64.powi((rest.len() - frac.len()) as i32);
                            s = frac;
                        }
                    }
                    value *= 1000.0;
                }
                f.unix = Some(if neg { -value } else { value } as i64);
            }
            Tok::Offset => {
                if let Some(rest) = s.strip_prefix('Z') {
                    s = rest;
                    continue;
                }
                let sign = match s.chars().next()? {
                    '+' => 1,
                    '-' => -1,
                    _ => return None,
                };
                s = &s[1..];
                let h = digits(&mut s, 2, 2)?;
                s = s.strip_prefix(':').unwrap_or(s);
                f.offset_minutes = sign * (h * 60 + digits(&mut s, 2, 2)?);
            }
            Tok::WeekdayName => {
                let n = s.chars().take_while(char::is_ascii_alphabetic).count();
                weekday_named(&s[..n])?;
                s = &s[n..];
            }
        }
    }
    if !s.trim().is_empty() {
        return None;
    }
    if let Some(unix) = f.unix {
        return (unix.abs() < 250_000_000_000_000).then_some(unix);
    }
    let year = f.year.unwrap_or(1970);
    if !YEAR_RANGE.contains(&year) {
        return None;
    }
    let month = f.month.unwrap_or(1);
    let day = f.day.unwrap_or(1);
    CivilDate::new(
        year as i32,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    )?;
    let mut hour = f.hour;
    if let Some(pm) = f.pm {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = hour % 12 + if pm { 12 } else { 0 };
    }
    if hour > 24 || f.minute > 59 || f.second > 59 {
        return None;
    }
    let ms = unix_ms_from_civil(
        year,
        month as u32,
        day as u32,
        hour as u32,
        f.minute as u32,
        f.second as u32,
        f.millis as u32,
        0,
    );
    Some(ms - f.offset_minutes * MINUTE)
}

/// Parse a date the way Mermaid does: the chart's `dateFormat` first, then
/// the ISO forms a browser would accept.
pub(crate) fn parse_date(input: &str, fmt: &str) -> Option<i64> {
    const ISO: [&str; 8] = [
        "YYYY-MM-DD",
        "YYYY-MM-DDTHH:mm",
        "YYYY-MM-DDTHH:mm:ss",
        "YYYY-MM-DDTHH:mm:ss.SSS",
        "YYYY-MM-DDTHH:mm:ssZ",
        "YYYY-MM-DDTHH:mm:ss.SSSZ",
        "YYYY-MM-DD HH:mm",
        "YYYY-MM-DD HH:mm:ss",
    ];
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    parse_with(input, fmt).or_else(|| ISO.iter().find_map(|f| parse_with(input, f)))
}

/// A task length: fixed milliseconds or calendar months.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Span {
    Fixed(i64),
    Months(f64),
}

/// `3d`, `1.5h`, `2w`, `30m`, `500ms`, `1M`, `1y`.
pub(crate) fn parse_duration(input: &str) -> Option<Span> {
    let s = input.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let value: f64 = num
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite() && *v >= 0.0)?;
    if value > 1.0e6 {
        return None;
    }
    let fixed = |ms: i64| Some(Span::Fixed((value * ms as f64).round() as i64));
    match unit.trim() {
        "ms" => fixed(1),
        "s" => fixed(SECOND),
        "m" => fixed(MINUTE),
        "h" => fixed(HOUR),
        "d" => fixed(DAY),
        "w" => fixed(WEEK),
        "M" => Some(Span::Months(value)),
        "y" => Some(Span::Months(value * 12.0)),
        _ => None,
    }
}

/// `at` moved by `months` calendar months, the day clamped into the month
/// (Jan 31 + 1M is the last day of February); a fraction adds its share of
/// an average month.
pub(crate) fn add_months(at: i64, months: f64) -> i64 {
    let whole = months.trunc();
    let frac = months - whole;
    let (year, month, day, h, m, s, ms) = civil_from_unix_ms(at, 0);
    let total = year * 12 + i64::from(month) - 1 + whole as i64;
    let (y, mo) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    let y = y.clamp(*YEAR_RANGE.start(), *YEAR_RANGE.end());
    let last = u32::from(days_in_month(y as i32, mo as u8));
    let base = unix_ms_from_civil(y, mo, day.min(last), h, m, s, ms, 0);
    base.saturating_add((frac * 30.436_875 * DAY as f64) as i64)
}

pub(crate) fn add(at: i64, span: Span) -> i64 {
    match span {
        Span::Fixed(ms) => at.saturating_add(ms),
        Span::Months(m) => add_months(at, m),
    }
}

/// Format an instant with a d3 / strftime pattern: `%Y %y %m %d %e %H %I
/// %M %S %L %p %a %A %b %B %j %w %u %%`, each optionally padded with `-`
/// (none), `_` (space) or `0`.
pub(crate) fn format(ms: i64, pattern: &str) -> String {
    let (year, month, day, hour, minute, second, millis) = civil_from_unix_ms(ms, 0);
    let wd = weekday(ms);
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut pad = None;
        if let Some(&p) = chars.peek()
            && matches!(p, '-' | '_' | '0')
        {
            pad = Some(p);
            chars.next();
        }
        let Some(spec) = chars.next() else {
            out.push('%');
            break;
        };
        let num = |out: &mut String, v: i64, width: usize, default: char| {
            let fill = pad.unwrap_or(default);
            match fill {
                '-' => out.push_str(&v.to_string()),
                '_' => out.push_str(&format!("{v:>width$}")),
                _ => out.push_str(&format!("{v:0width$}")),
            }
        };
        match spec {
            'Y' => out.push_str(&year.to_string()),
            'y' => num(&mut out, year.rem_euclid(100), 2, '0'),
            'm' => num(&mut out, i64::from(month), 2, '0'),
            'd' => num(&mut out, i64::from(day), 2, '0'),
            'e' => num(&mut out, i64::from(day), 2, '_'),
            'H' => num(&mut out, i64::from(hour), 2, '0'),
            'I' => num(&mut out, i64::from((hour + 11) % 12 + 1), 2, '0'),
            'M' => num(&mut out, i64::from(minute), 2, '0'),
            'S' => num(&mut out, i64::from(second), 2, '0'),
            'L' => num(&mut out, i64::from(millis), 3, '0'),
            'p' => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            'a' => out.push_str(&WEEKDAYS[wd as usize][..3]),
            'A' => out.push_str(WEEKDAYS[wd as usize]),
            'b' | 'h' => out.push_str(&MONTHS[month as usize - 1][..3]),
            'B' => out.push_str(MONTHS[month as usize - 1]),
            'j' => {
                let jan1 = unix_ms_from_civil(year, 1, 1, 0, 0, 0, 0, 0);
                num(&mut out, (ms - jan1).div_euclid(DAY) + 1, 3, '0');
            }
            'w' => out.push_str(&((wd + 1) % 7).to_string()),
            'u' => out.push_str(&(wd + 1).to_string()),
            '%' => out.push('%'),
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

/// A `tickInterval` unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TickUnit {
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
}

/// `1day`, `2week`, `15minute`, `1month`.
pub(crate) fn parse_tick_interval(input: &str) -> Option<(i64, TickUnit)> {
    let s = input.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit) = s.split_at(split);
    let n: i64 = num.parse().ok().filter(|n| (1..=10_000).contains(n))?;
    let unit = match unit.trim() {
        "millisecond" => TickUnit::Millisecond,
        "second" => TickUnit::Second,
        "minute" => TickUnit::Minute,
        "hour" => TickUnit::Hour,
        "day" => TickUnit::Day,
        "week" => TickUnit::Week,
        "month" => TickUnit::Month,
        _ => return None,
    };
    Some((n, unit))
}

/// Ticks every `n` units over `min..=max`, aligned to the unit (weeks start
/// on `week_start`, Monday = 0). `None` past `cap` ticks.
pub(crate) fn interval_ticks(
    min: i64,
    max: i64,
    n: i64,
    unit: TickUnit,
    week_start: u32,
    cap: usize,
) -> Option<Vec<i64>> {
    let mut out = Vec::new();
    let fixed = |ms: i64| ms.checked_mul(n);
    match unit {
        TickUnit::Month => {
            let (year, month, ..) = civil_from_unix_ms(min, 0);
            let mut index = year * 12 + i64::from(month) - 1;
            index -= index.rem_euclid(n);
            loop {
                let t = unix_ms_from_civil(
                    index.div_euclid(12),
                    index.rem_euclid(12) as u32 + 1,
                    1,
                    0,
                    0,
                    0,
                    0,
                    0,
                );
                if t > max {
                    break;
                }
                if t >= min {
                    out.push(t);
                }
                if out.len() > cap {
                    return None;
                }
                index += n;
            }
        }
        _ => {
            let (step, origin) = match unit {
                TickUnit::Millisecond => (fixed(1)?, 0),
                TickUnit::Second => (fixed(SECOND)?, 0),
                TickUnit::Minute => (fixed(MINUTE)?, 0),
                TickUnit::Hour => (fixed(HOUR)?, 0),
                TickUnit::Day => (fixed(DAY)?, 0),
                // 1970-01-01 was a Thursday (3 when Monday = 0).
                _ => (
                    fixed(WEEK)?,
                    (i64::from(week_start) - 3).rem_euclid(7) * DAY,
                ),
            };
            if (max - min) / step > cap as i64 {
                return None;
            }
            let mut t = (min - origin).div_euclid(step) * step + origin;
            if t < min {
                t += step;
            }
            while t <= max {
                out.push(t);
                t += step;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        unix_ms_from_civil(y, mo, d, h, mi, 0, 0, 0)
    }

    #[test]
    fn day_js_formats_parse_strictly() {
        assert_eq!(
            parse_with("2024-03-07", "YYYY-MM-DD"),
            Some(utc(2024, 3, 7, 0, 0))
        );
        assert_eq!(
            parse_with("07/03/24 14:05", "DD/MM/YY HH:mm"),
            Some(utc(2024, 3, 7, 14, 5))
        );
        assert_eq!(parse_with("2024-02-30", "YYYY-MM-DD"), None);
        assert_eq!(parse_with("2024-3-7", "YYYY-MM-DD"), None);
        assert_eq!(
            parse_with("2024-3-7", "YYYY-M-D"),
            Some(utc(2024, 3, 7, 0, 0))
        );
        assert_eq!(
            parse_with("Mar 7th 2024 2:30 pm", "MMM Do YYYY h:mm a"),
            Some(utc(2024, 3, 7, 14, 30))
        );
        assert_eq!(parse_with("1700000000", "X"), Some(1_700_000_000_000));
        assert_eq!(parse_with("1700000000.5", "X"), Some(1_700_000_000_500));
        assert_eq!(
            parse_with("2024-03-07T08:00:00+02:00", "YYYY-MM-DDTHH:mm:ssZ"),
            Some(utc(2024, 3, 7, 6, 0))
        );
        assert_eq!(parse_with("7xx", "Do"), None);
        assert_eq!(parse_with("10:30", "HH:mm"), Some(utc(1970, 1, 1, 10, 30)));
        assert_eq!(
            parse_date("2024-03-07T08:00", "HH:mm"),
            Some(utc(2024, 3, 7, 8, 0))
        );
    }

    #[test]
    fn durations_and_months() {
        assert_eq!(parse_duration("3d"), Some(Span::Fixed(3 * DAY)));
        assert_eq!(parse_duration("1.5h"), Some(Span::Fixed(90 * MINUTE)));
        assert_eq!(parse_duration("2w"), Some(Span::Fixed(14 * DAY)));
        assert_eq!(parse_duration("3x"), None);
        assert_eq!(
            add(utc(2024, 1, 31, 0, 0), Span::Months(1.0)),
            utc(2024, 2, 29, 0, 0)
        );
    }

    #[test]
    fn strftime_subset() {
        let t = utc(2024, 3, 7, 14, 5);
        assert_eq!(format(t, "%Y-%m-%d"), "2024-03-07");
        assert_eq!(format(t, "%b %-d %H:%M"), "Mar 7 14:05");
        assert_eq!(format(t, "%a %I%p %j"), "Thu 02PM 067");
        assert_eq!(weekday(t), 3);
        assert_eq!(weekday_named("sunday"), Some(6));
        assert_eq!(weekday_named("Sat"), Some(5));
    }

    #[test]
    fn interval_ticks_align() {
        let min = utc(2024, 3, 7, 0, 0);
        let max = utc(2024, 3, 31, 0, 0);
        let weeks = interval_ticks(min, max, 1, TickUnit::Week, 0, 100).unwrap();
        assert_eq!(weeks[0], utc(2024, 3, 11, 0, 0));
        assert!(weeks.iter().all(|&t| weekday(t) == 0));
        let months =
            interval_ticks(min, utc(2024, 9, 1, 0, 0), 2, TickUnit::Month, 0, 100).unwrap();
        assert_eq!(
            months,
            vec![
                utc(2024, 5, 1, 0, 0),
                utc(2024, 7, 1, 0, 0),
                utc(2024, 9, 1, 0, 0)
            ]
        );
        assert!(interval_ticks(0, 1_000_000_000, 1, TickUnit::Millisecond, 0, 100).is_none());
    }
}
