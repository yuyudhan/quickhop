//! Date formatting with Unicode (LDML) patterns, as Raycast's `{date format="..."}` uses, and
//! Raycast's date offsets (`+2d -3M`).
//!
//! Supported pattern letters: `G` era, `y` year, `Q`/`q` quarter, `M`/`L` month, `d` day,
//! `D` day of year, `F` weekday in month, `E` weekday, `a` AM/PM, `h` `H` `k` `K` hours, `m`
//! minutes, `s` seconds, `S` fractions, `z` zone abbreviation, `Z` offset. Text in single
//! quotes is literal (`''` is a quote); other letters are copied as they are.

use std::fmt::Write as _;

use jiff::{Span, Zoned};

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

/// `z` formatted with the LDML `pattern`, in English.
pub fn format(z: &Zoned, pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i += 1;
            if chars.get(i) == Some(&'\'') {
                out.push('\'');
                i += 1;
                continue;
            }
            while i < chars.len() {
                if chars[i] == '\'' {
                    if chars.get(i + 1) == Some(&'\'') {
                        out.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphabetic() {
            let run = chars[i..].iter().take_while(|&&next| next == c).count();
            field(z, c, run, &mut out);
            i += run;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `value` zero-padded to `width` digits.
fn padded(out: &mut String, value: i64, width: usize) {
    let _ = write!(out, "{value:0width$}");
}

fn name(names: &[&str], index: i8, width: usize) -> String {
    let full = usize::try_from(index)
        .ok()
        .and_then(|i| names.get(i))
        .copied()
        .unwrap_or_default();
    match width {
        0..=3 => full.chars().take(3).collect(),
        4 => full.to_owned(),
        5 => full.chars().take(1).collect(),
        _ => full.chars().take(2).collect(),
    }
}

/// The zone offset as `+0530` (`colon` false) or `+05:30`.
fn offset_text(z: &Zoned, colon: bool) -> String {
    let seconds = z.offset().seconds();
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    let (h, m) = (minutes / 60, minutes % 60);
    if colon {
        format!("{sign}{h:02}:{m:02}")
    } else {
        format!("{sign}{h:02}{m:02}")
    }
}

fn abbreviation(z: &Zoned) -> String {
    let info = z.time_zone().to_offset_info(z.timestamp());
    let abbreviation = info.abbreviation();
    // tzdb uses numeric abbreviations such as `+0530` where no name is customary.
    if abbreviation.is_empty() || abbreviation.starts_with(['+', '-']) {
        format!("GMT{}", offset_text(z, true))
    } else {
        abbreviation.to_owned()
    }
}

fn field(z: &Zoned, letter: char, width: usize, out: &mut String) {
    let hour = i64::from(z.hour());
    match letter {
        'G' => out.push_str(match width {
            4 => "Anno Domini",
            5 => "A",
            _ => "AD",
        }),
        'y' if width == 2 => padded(out, i64::from(z.year()).rem_euclid(100), 2),
        'y' => padded(out, i64::from(z.year()), width),
        'Q' | 'q' => {
            let quarter = (z.month() - 1) / 3 + 1;
            match width {
                3 => {
                    let _ = write!(out, "Q{quarter}");
                }
                4 => {
                    let ordinal = ["1st", "2nd", "3rd", "4th"];
                    let index = usize::try_from(quarter - 1).unwrap_or_default();
                    let _ = write!(out, "{} quarter", ordinal.get(index).unwrap_or(&""));
                }
                _ => padded(out, i64::from(quarter), width.min(2)),
            }
        }
        'M' | 'L' if width >= 3 => out.push_str(&name(&MONTHS, z.month() - 1, width.min(5))),
        'M' | 'L' => padded(out, i64::from(z.month()), width),
        'd' => padded(out, i64::from(z.day()), width),
        'D' => padded(out, i64::from(z.day_of_year()), width),
        'F' => padded(out, i64::from((z.day() - 1) / 7 + 1), width),
        'E' => {
            let index = z.weekday().to_monday_zero_offset();
            out.push_str(&name(&WEEKDAYS, index, width));
        }
        'a' => out.push_str(if hour < 12 { "AM" } else { "PM" }),
        'h' => padded(out, if hour % 12 == 0 { 12 } else { hour % 12 }, width),
        'H' => padded(out, hour, width),
        'k' => padded(out, if hour == 0 { 24 } else { hour }, width),
        'K' => padded(out, hour % 12, width),
        'm' => padded(out, i64::from(z.minute()), width),
        's' => padded(out, i64::from(z.second()), width),
        'S' => {
            let nanos = format!("{:09}", z.subsec_nanosecond());
            let digits: String = nanos
                .chars()
                .chain(std::iter::repeat('0'))
                .take(width)
                .collect();
            out.push_str(&digits);
        }
        'z' if width <= 3 => out.push_str(&abbreviation(z)),
        'z' => {
            let _ = write!(out, "GMT{}", offset_text(z, true));
        }
        'Z' if width <= 3 => out.push_str(&offset_text(z, false)),
        'Z' if width == 4 => {
            let _ = write!(out, "{}{}", abbreviation(z), offset_text(z, true));
        }
        'Z' if z.offset().seconds() == 0 => out.push('Z'),
        'Z' => out.push_str(&offset_text(z, true)),
        other => out.extend(std::iter::repeat_n(other, width)),
    }
}

/// `z` moved by Raycast offset terms separated by spaces: a sign, a number and a unit, `m`
/// minutes, `h` hours, `d` days, `w` weeks, `M` months, `y` years (`+2y -5M`). `None` when a
/// term is malformed or the result is out of range.
pub fn offset(z: &Zoned, terms: &str) -> Option<Zoned> {
    let mut moved: Option<Zoned> = None;
    for term in terms.split_whitespace() {
        let sign = match term.chars().next()? {
            '+' => 1,
            '-' => -1,
            _ => return None,
        };
        let unit = term.chars().last()?;
        let digits = term.get(1..term.len() - unit.len_utf8())?;
        if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let amount: i64 = sign * digits.parse::<i64>().ok()?;
        let span = Span::new();
        // One term at a time: a span has a single sign, and `+3M -5d` mixes them.
        let span = match unit {
            'm' => span.try_minutes(amount),
            'h' => span.try_hours(amount),
            'd' => span.try_days(amount),
            'w' => span.try_weeks(amount),
            'M' => span.try_months(amount),
            'y' => span.try_years(amount),
            _ => return None,
        }
        .ok()?;
        moved = Some(moved.as_ref().unwrap_or(z).checked_add(span).ok()?);
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Zoned {
        text.parse().unwrap()
    }

    /// The moment the Raycast manual's custom-format examples were taken.
    fn manual() -> Zoned {
        at("2022-06-15T13:44:39.945+00:00[UTC]")
    }

    #[test]
    fn raycast_manual_custom_formats() {
        let examples = [
            ("EEEE, MMM d, yyyy", "Wednesday, Jun 15, 2022"),
            ("MM/dd/yyyy", "06/15/2022"),
            ("MM-dd-yyyy HH:mm", "06-15-2022 13:44"),
            ("MMM d, h:mm a", "Jun 15, 1:44 PM"),
            ("MMMM yyyy", "June 2022"),
            ("MMM d, yyyy", "Jun 15, 2022"),
            (
                "E, d MMM yyyy HH:mm:ss Z",
                "Wed, 15 Jun 2022 13:44:39 +0000",
            ),
            ("yyyy-MM-dd'T'HH:mm:ssZ", "2022-06-15T13:44:39+0000"),
            ("dd.MM.yy", "15.06.22"),
            ("HH:mm:ss.SSS", "13:44:39.945"),
        ];
        for (pattern, expected) in examples {
            assert_eq!(format(&manual(), pattern), expected, "{pattern}");
        }
        let eve = at("2022-06-05T08:30:00+00:00[UTC]");
        assert_eq!(
            format(&eve, "h:mm 'on the eve of' MMMM d"),
            "8:30 on the eve of June 5"
        );
    }

    #[test]
    fn raycast_manual_alphabet() {
        // "The output is based on the time June 15th, 2022 2:45 PM UTC."
        let z = at("2022-06-15T14:45:06.753+00:00[UTC]");
        let letters = [
            ("y", "2022"),
            ("yy", "22"),
            ("yyyy", "2022"),
            ("Q", "2"),
            ("QQ", "02"),
            ("QQQ", "Q2"),
            ("QQQQ", "2nd quarter"),
            ("M", "6"),
            ("MM", "06"),
            ("MMM", "Jun"),
            ("MMMM", "June"),
            ("MMMMM", "J"),
            ("d", "15"),
            ("dd", "15"),
            ("F", "3"),
            ("E", "Wed"),
            ("EEEE", "Wednesday"),
            ("EEEEE", "W"),
            ("EEEEEE", "We"),
            ("h", "2"),
            ("hh", "02"),
            ("H", "14"),
            ("HH", "14"),
            ("a", "PM"),
            ("m", "45"),
            ("mm", "45"),
            ("s", "6"),
            ("ss", "06"),
            ("SSS", "753"),
        ];
        for (pattern, expected) in letters {
            assert_eq!(format(&z, pattern), expected, "{pattern}");
        }
        let india = at("2022-06-15T20:15:06+05:30[Asia/Kolkata]");
        assert_eq!(format(&india, "zzz"), "IST");
        assert_eq!(format(&india, "Z"), "+0530");
        assert_eq!(format(&india, "ZZZZZ"), "+05:30");
        assert_eq!(format(&india, "ZZZZ"), "IST+05:30");
        assert_eq!(format(&india, "zzzz"), "GMT+05:30");
        assert_eq!(format(&z, "ZZZZZ"), "Z");
    }

    #[test]
    fn quotes_and_unknown_letters() {
        assert_eq!(format(&manual(), "''yy'' 'it''s' B"), "'22' it's B");
        assert_eq!(format(&manual(), "'unterminated"), "unterminated");
        assert_eq!(
            format(&at("2022-06-15T00:05:00+00:00[UTC]"), "h k K a"),
            "12 24 0 AM"
        );
    }

    #[test]
    fn offsets_follow_the_manual() {
        let z = manual();
        let moved = |terms| offset(&z, terms).map(|z| format(&z, "yyyy-MM-dd HH:mm"));
        assert_eq!(moved("+2y +5M").as_deref(), Some("2024-11-15 13:44"));
        assert_eq!(moved("+3h +30m").as_deref(), Some("2022-06-15 17:14"));
        assert_eq!(moved("-3d").as_deref(), Some("2022-06-12 13:44"));
        assert_eq!(moved("+3M -5d").as_deref(), Some("2022-09-10 13:44"));
        assert_eq!(moved("+1w").as_deref(), Some("2022-06-22 13:44"));
        for bad in ["+ 2d", "2d", "+2x", "", "+d", "+9999999y", "+999999y"] {
            assert_eq!(moved(bad), None, "{bad}");
        }
    }
}
