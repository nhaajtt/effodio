//! Text as keyword search keeps it: lowercased, without accents (so "hoa don" finds "hóa đơn"), plus the cutting of
//! long text into passages the model handles well.

use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// One character folded for matching: lowercased, its accents dropped, đ as d. Usually one character out.
fn fold_char(c: char, out: &mut String) {
    match c {
        'đ' | 'Đ' => out.push('d'),
        'ß' => out.push_str("ss"),
        _ => {
            for part in std::iter::once(c).nfd() {
                if !is_combining_mark(part) {
                    out.extend(part.to_lowercase());
                }
            }
        }
    }
}

/// `text` folded for matching.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        fold_char(c, &mut out);
    }
    out
}

/// `text` folded, with the byte offset in `text` each folded byte came from (one more entry marks the end).
pub fn fold_with_offsets(text: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut offsets = Vec::with_capacity(text.len() + 1);
    for (at, c) in text.char_indices() {
        let before = out.len();
        fold_char(c, &mut out);
        offsets.extend(std::iter::repeat_n(at, out.len() - before));
    }
    offsets.push(text.len());
    (out, offsets)
}

/// Words of a folded text: runs of letters and digits.
pub fn words(folded: &str) -> impl Iterator<Item = &str> {
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

/// Text worth a vector: it has words, not just numbers (a data export is matched by its exact text only).
pub fn has_words(text: &str) -> bool {
    let (mut letters, mut digits) = (0usize, 0usize);
    for c in text.chars() {
        if c.is_alphabetic() {
            letters += 1;
        } else if c.is_numeric() {
            digits += 1;
        }
    }
    letters >= 20 && letters * 2 >= digits
}

/// Whitespace runs as one space, lines kept apart, control characters dropped.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_lines = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank_lines += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank_lines > 0 { "\n\n" } else { "\n" });
        }
        blank_lines = 0;
        out.extend(line.chars().filter(|c| !c.is_control()));
    }
    out
}

/// The passage length the model reads well, in characters.
pub const PASSAGE: usize = 1800;

/// Cuts text into passages of up to `limit` characters, at paragraph, line or sentence ends where it can. Each
/// passage comes with its character offset in `text`.
pub fn passages(text: &str, limit: usize) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        if start >= chars.len() {
            break;
        }
        let mut end = (start + limit).min(chars.len());
        if end < chars.len() {
            let floor = start + limit / 2;
            let cut = (floor..end)
                .rev()
                .find(|&i| chars[i] == '\n' && chars.get(i + 1) == Some(&'\n'))
                .or_else(|| (floor..end).rev().find(|&i| chars[i] == '\n'))
                .or_else(|| {
                    (floor..end).rev().find(|&i| {
                        matches!(chars[i], '.' | '!' | '?' | '。') && chars.get(i + 1).is_some_and(|c| c.is_whitespace())
                    })
                })
                .or_else(|| (floor..end).rev().find(|&i| chars[i].is_whitespace()));
            if let Some(cut) = cut {
                end = cut + 1;
            }
        }
        let passage: String = chars[start..end].iter().collect();
        let passage = passage.trim();
        if !passage.is_empty() {
            out.push((start, passage.to_string()));
        }
        start = end;
    }
    out
}

/// The start of a text, for showing in a result: up to `limit` characters, cut at a word.
pub fn excerpt(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let cut: String = flat.chars().take(limit).collect();
    let cut = cut.rfind(' ').map(|at| &cut[..at]).unwrap_or(&cut);
    format!("{cut}…")
}

/// A stretch of `original` around the first of `terms` (folded words), with every match wrapped in « ». None when
/// none of them is in it.
pub fn snippet(original: &str, terms: &[String], radius: usize) -> Option<String> {
    let (folded, offsets) = fold_with_offsets(original);
    let mut matches: Vec<(usize, usize)> = Vec::new();
    let mut at = 0;
    for word in folded.split(|c: char| !c.is_alphanumeric()) {
        let start = at;
        at += word.len() + folded[at + word.len()..].chars().next().map_or(0, char::len_utf8);
        if word.is_empty() {
            continue;
        }
        let hit = terms.iter().any(|term| {
            word == term || (term.len() >= 3 && word.starts_with(term.as_str())) || stem_equal(word, term)
        });
        if hit {
            matches.push((offsets[start], offsets[start + word.len()]));
        }
    }
    let &(first, _) = matches.first()?;
    let mut from = first.saturating_sub(radius);
    while !original.is_char_boundary(from) {
        from -= 1;
    }
    if from > 0 {
        if let Some(space) = original[from..first].find(' ') {
            from += space + 1;
        }
    }
    let mut to = (first + radius * 2).min(original.len());
    while !original.is_char_boundary(to) {
        to += 1;
    }
    if to < original.len() {
        if let Some(space) = original[from..to].rfind(' ') {
            to = from + space;
        }
    }
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let mut cursor = from;
    for &(start, end) in matches.iter().filter(|&&(start, end)| start >= from && end <= to) {
        out.push_str(&original[cursor..start]);
        out.push('«');
        out.push_str(&original[start..end]);
        out.push('»');
        cursor = end;
    }
    out.push_str(&original[cursor..to]);
    if to < original.len() {
        out.push('…');
    }
    Some(out.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A cheap stand-in for the index's stemming, for highlighting: the same word with a common English ending.
fn stem_equal(word: &str, term: &str) -> bool {
    let strip = |w: &str| -> String {
        for ending in ["ing", "ed", "es", "s", "ly"] {
            if w.len() > ending.len() + 2 && w.ends_with(ending) {
                return w[..w.len() - ending.len()].to_string();
            }
        }
        w.to_string()
    };
    word.is_ascii() && term.is_ascii() && strip(word) == strip(term)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_drops_accents() {
        assert_eq!(fold("Hóa Đơn Điện Tử"), "hoa don dien tu");
        assert_eq!(fold("Café Zürich"), "cafe zurich");
        let (folded, offsets) = fold_with_offsets("Hóa đơn");
        assert_eq!(folded, "hoa don");
        assert_eq!(offsets.len(), folded.len() + 1);
        assert_eq!(*offsets.last().unwrap(), "Hóa đơn".len());
    }

    #[test]
    fn numbers_alone_are_not_words() {
        assert!(has_words("The lease says pets are allowed with a deposit."));
        assert!(!has_words("1203 4411 9982 1200 3321 8870 1123 9981 2231 a b"));
    }

    #[test]
    fn passages_cut_at_paragraphs_and_keep_everything() {
        let paragraph = "One sentence about the lease. Another about the deposit and pets.";
        let text = (0..80).map(|_| paragraph).collect::<Vec<_>>().join("\n\n");
        let parts = passages(&text, PASSAGE);
        assert!(parts.len() > 2);
        assert!(parts.iter().all(|(_, p)| p.chars().count() <= PASSAGE));
        assert!(parts.iter().all(|(_, p)| p.ends_with("pets.")));
        let total: usize = parts.iter().map(|(_, p)| p.matches("lease").count()).sum();
        assert_eq!(total, 80);
    }

    #[test]
    fn snippets_mark_matches_in_the_original_spelling() {
        let text = "Tổng cộng trên hóa đơn điện tử số 0012345 là 2.500.000 đồng, thanh toán trước ngày 15.";
        let found = snippet(text, &["hoa".into(), "don".into()], 30).unwrap();
        assert!(found.contains("«hóa» «đơn»"), "{found}");
        assert_eq!(snippet(text, &["zebra".into()], 30), None);
    }
}
