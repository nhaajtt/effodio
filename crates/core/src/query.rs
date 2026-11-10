//! What someone typed, taken apart: the words to look for, and the filters written in with them
//! (`kind:video in:Downloads after:2026-10 -beach "exact words"`).

use crate::kinds::FileKind;
use crate::text::fold;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// What's left after the filters: this is what the model reads.
    pub text: String,
    /// `kind:` filters; empty means every kind.
    pub kinds: Vec<FileKind>,
    /// `in:` filters: parts of the path, folded.
    pub folders: Vec<String>,
    /// `ext:` filters, lowercase without the dot.
    pub extensions: Vec<String>,
    /// Seconds since 1970.
    pub after: Option<f64>,
    pub before: Option<f64>,
    /// `-word`: what the results should stay away from.
    pub negative: Vec<String>,
    /// `"exact words"`: must be in the file as written.
    pub phrases: Vec<String>,
}

impl Query {
    pub fn has_filters(&self) -> bool {
        !self.kinds.is_empty() || !self.folders.is_empty() || !self.extensions.is_empty() || self.after.is_some()
            || self.before.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.phrases.is_empty()
    }
}

fn kind_named(name: &str) -> Option<Vec<FileKind>> {
    Some(match fold(name).as_str() {
        "image" | "images" | "picture" | "pictures" | "anh" | "hinh" => vec![FileKind::Image, FileKind::Screenshot],
        "photo" | "photos" => vec![FileKind::Image],
        "screenshot" | "screenshots" | "screen" => vec![FileKind::Screenshot],
        "pdf" | "pdfs" => vec![FileKind::Pdf],
        "doc" | "docs" | "document" | "documents" | "text" | "tailieu" | "vanban" => vec![FileKind::Doc],
        "audio" | "sound" | "music" | "recording" | "amthanh" | "ghiam" => vec![FileKind::Audio],
        "video" | "videos" | "movie" | "clip" => vec![FileKind::Video],
        "media" => vec![FileKind::Audio, FileKind::Video],
        _ => return None,
    })
}

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `2026`, `2026-10`, `2026-10-05`, or an age like `7d`, `2w`, `3m`, `1y` (counted back from `now`). `end`: the
/// last second of the period rather than its first.
pub fn parse_date(value: &str, now: f64, end: bool) -> Option<f64> {
    let value = value.trim();
    if let Some(unit) = value.chars().last().filter(|c| "dwmy".contains(*c)) {
        if let Ok(count) = value[..value.len() - 1].parse::<f64>() {
            let days = match unit {
                'd' => 1.0,
                'w' => 7.0,
                'm' => 30.44,
                _ => 365.25,
            };
            return Some(now - count * days * 86_400.0);
        }
    }
    let parts: Vec<i64> = value.split(['-', '/', '.']).map(|part| part.parse().ok()).collect::<Option<_>>()?;
    let (year, month, day) = match parts[..] {
        [year] if (1970..=2200).contains(&year) => (year, None, None),
        [year, month] if (1..=12).contains(&month) => (year, Some(month), None),
        [year, month, day] if (1..=12).contains(&month) && (1..=31).contains(&day) => (year, Some(month), Some(day)),
        _ => return None,
    };
    let start = days_from_civil(year, month.unwrap_or(1), day.unwrap_or(1));
    if !end {
        return Some(start as f64 * 86_400.0);
    }
    let next = match (month, day) {
        (None, _) => days_from_civil(year + 1, 1, 1),
        (Some(12), None) => days_from_civil(year + 1, 1, 1),
        (Some(month), None) => days_from_civil(year, month + 1, 1),
        (Some(_), Some(_)) => start + 1,
    };
    Some(next as f64 * 86_400.0 - 1.0)
}

/// Splits on spaces, keeping "quoted words" together (the quotes stay on the token).
fn tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' | '“' | '”' => {
                quoted = !quoted;
                current.push('"');
            }
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn parse(input: &str, now: f64) -> Query {
    let mut query = Query::default();
    let mut words: Vec<String> = Vec::new();
    for token in tokens(input) {
        if let Some((key, value)) = token.split_once(':') {
            let value = value.trim_matches('"');
            let handled = !value.is_empty()
                && match key.to_lowercase().as_str() {
                    "kind" | "type" | "loai" => match kind_named(value) {
                        Some(kinds) => {
                            for kind in kinds {
                                if !query.kinds.contains(&kind) {
                                    query.kinds.push(kind);
                                }
                            }
                            true
                        }
                        None => false,
                    },
                    "in" | "folder" | "trong" => {
                        query.folders.push(fold(value));
                        true
                    }
                    "ext" => {
                        query.extensions.push(value.trim_start_matches('.').to_lowercase());
                        true
                    }
                    "after" | "since" | "sau" => match parse_date(value, now, false) {
                        Some(date) => {
                            query.after = Some(date);
                            true
                        }
                        None => false,
                    },
                    "before" | "truoc" => match parse_date(value, now, true) {
                        Some(date) => {
                            query.before = Some(date);
                            true
                        }
                        None => false,
                    },
                    _ => false,
                };
            if handled {
                continue;
            }
        }
        if token.len() > 2 && token.starts_with('"') && token.ends_with('"') {
            let phrase = token.trim_matches('"').trim();
            if !phrase.is_empty() {
                query.phrases.push(phrase.to_string());
                words.push(phrase.to_string());
            }
            continue;
        }
        // A lone "-" or a negative number is not an exclusion.
        if let Some(negative) = token.strip_prefix('-') {
            if negative.chars().next().is_some_and(char::is_alphabetic) {
                query.negative.push(negative.trim_matches('"').to_string());
                continue;
            }
        }
        words.push(token.replace('"', ""));
    }
    query.text = words.join(" ");
    query
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_come_out_of_the_text() {
        let query = parse(r#"dog on the beach kind:video in:Downloads ext:.MP4 -cat "golden hour""#, 0.0);
        assert_eq!(query.text, "dog on the beach golden hour");
        assert_eq!(query.kinds, vec![FileKind::Video]);
        assert_eq!(query.folders, vec!["downloads"]);
        assert_eq!(query.extensions, vec!["mp4"]);
        assert_eq!(query.negative, vec!["cat"]);
        assert_eq!(query.phrases, vec!["golden hour"]);
        assert!(query.has_filters());
    }

    #[test]
    fn what_is_not_a_filter_stays_text() {
        let query = parse("error: 0x80070005 kind:spaceship -5 degrees re: lease", 0.0);
        assert_eq!(query.text, "error: 0x80070005 kind:spaceship -5 degrees re: lease");
        assert!(!query.has_filters());
        assert!(query.negative.is_empty());
        assert!(parse("   ", 0.0).is_empty());
    }

    #[test]
    fn dates_and_ages() {
        assert_eq!(parse_date("1970-01-02", 0.0, false), Some(86_400.0));
        assert_eq!(parse_date("2026-10", 0.0, false), Some(1_790_812_800.0));
        // The end of October 2026 is a second before November.
        assert_eq!(parse_date("2026-10", 0.0, true), Some(1_793_491_200.0 - 1.0));
        assert_eq!(parse_date("2024-02-29", 0.0, false), Some(1_709_164_800.0));
        assert_eq!(parse_date("7d", 1_000_000.0, false), Some(1_000_000.0 - 7.0 * 86_400.0));
        assert_eq!(parse_date("soon", 0.0, false), None);
        let query = parse("receipt after:2026-10 before:2026-10", 0.0);
        assert!(query.after.unwrap() < query.before.unwrap());
        assert_eq!(query.text, "receipt");
    }

    #[test]
    fn vietnamese_filter_words() {
        let query = parse("hóa đơn loai:ảnh trong:Tải_xuống", 0.0);
        assert_eq!(query.text, "hóa đơn");
        assert_eq!(query.kinds, vec![FileKind::Image, FileKind::Screenshot]);
        assert_eq!(query.folders, vec!["tai_xuong"]);
    }
}
