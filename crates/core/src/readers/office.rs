//! The text of documents, spreadsheets and presentations. Office and OpenDocument files are zip archives of XML;
//! the rest is plain text with or without markup.

use crate::text::tidy;
use anyhow::{anyhow, bail, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;
use std::path::Path;

/// One part of a document: a sheet, a slide, or the whole text.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// "Sheet: Budget", "Slide 3".
    pub label: Option<String>,
    pub text: String,
}

/// More than this is a data dump: the start is enough to find it by.
const MAX_CHARS: usize = 4_000_000;
const MAX_ENTRY_BYTES: u64 = 200 << 20;

pub fn read(path: &Path) -> Result<Vec<Part>> {
    let extension = crate::kinds::extension(path);
    let parts = match extension.as_str() {
        "docx" => vec![Part { label: None, text: docx(path)? }],
        "xlsx" => xlsx(path)?,
        "pptx" => pptx(path)?,
        "odt" => vec![Part { label: None, text: open_document(path)? }],
        "ods" | "odp" => vec![Part { label: None, text: open_document(path)? }],
        "epub" => vec![Part { label: None, text: epub(path)? }],
        "rtf" => vec![Part { label: None, text: rtf(&plain(path)?) }],
        "html" | "htm" => vec![Part { label: None, text: html(&plain(path)?) }],
        _ => vec![Part { label: None, text: plain(path)? }],
    };
    Ok(parts
        .into_iter()
        .map(|part| Part { label: part.label, text: tidy(&part.text).chars().take(MAX_CHARS).collect() })
        .filter(|part| !part.text.is_empty())
        .collect())
}

/// A text file as a string: UTF-8, UTF-16 with a byte-order mark, or whatever else read leniently.
pub fn plain(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(decode(&bytes))
}

pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], little: bool| -> String {
        let units: Vec<u16> = rest
            .as_chunks::<2>().0.iter()
            .map(|pair| if little { u16::from_le_bytes([pair[0], pair[1]]) } else { u16::from_be_bytes([pair[0], pair[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        // Windows-1252 covers what's left on most Western machines: every byte is a character.
        Err(_) => {
            let lossy = String::from_utf8_lossy(bytes);
            if lossy.chars().filter(|&c| c == '\u{FFFD}').count() * 20 > lossy.chars().count() {
                bytes.iter().map(|&b| b as char).collect()
            } else {
                lossy.into_owned()
            }
        }
    }
}

fn entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String> {
    let file = archive.by_name(name).map_err(|_| anyhow!("no {name} in the file"))?;
    if file.size() > MAX_ENTRY_BYTES {
        bail!("{name} is too large");
    }
    let mut bytes = Vec::new();
    file.take(MAX_ENTRY_BYTES).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn open(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    Ok(zip::ZipArchive::new(std::fs::File::open(path)?)?)
}

fn local(name: &[u8]) -> &[u8] {
    name.rsplit(|&b| b == b':').next().unwrap_or(name)
}

/// The text of an XML part: the content of `text_tags`, a line break after each of `break_tags`.
fn xml_text(xml: &str, text_tags: &[&[u8]], break_tags: &[&[u8]], tab_tags: &[&[u8]]) -> String {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut inside = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                if text_tags.contains(&local(tag.name().as_ref())) {
                    inside += 1;
                }
            }
            Ok(Event::Empty(tag)) => {
                let name = tag.name();
                let name = local(name.as_ref());
                if tab_tags.contains(&name) {
                    out.push('\t');
                } else if break_tags.contains(&name) {
                    out.push('\n');
                }
            }
            Ok(Event::End(tag)) => {
                let name = tag.name();
                let name = local(name.as_ref());
                if text_tags.contains(&name) {
                    inside = inside.saturating_sub(1);
                }
                if break_tags.contains(&name) {
                    out.push('\n');
                }
            }
            Ok(Event::Text(text)) if inside > 0 => {
                if let Ok(text) = text.unescape() {
                    out.push_str(&text);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

fn docx(path: &Path) -> Result<String> {
    let mut archive = open(path)?;
    let mut text = xml_text(&entry(&mut archive, "word/document.xml")?, &[b"t"], &[b"p", b"br", b"tr"], &[b"tab"]);
    for extra in ["word/footnotes.xml", "word/endnotes.xml", "word/comments.xml"] {
        if let Ok(xml) = entry(&mut archive, extra) {
            let more = xml_text(&xml, &[b"t"], &[b"p", b"br"], &[b"tab"]);
            if !more.trim().is_empty() {
                text.push_str("\n\n");
                text.push_str(&more);
            }
        }
    }
    Ok(text)
}

/// Numbered entries of an archive folder ("ppt/slides/slide12.xml"), in number order.
fn numbered(archive: &zip::ZipArchive<std::fs::File>, prefix: &str) -> Vec<(u32, String)> {
    let mut names: Vec<(u32, String)> = archive
        .file_names()
        .filter(|name| name.starts_with(prefix) && name.ends_with(".xml"))
        .filter_map(|name| {
            let number: String = name[prefix.len()..name.len() - 4].chars().filter(char::is_ascii_digit).collect();
            number.parse().ok().map(|n| (n, name.to_string()))
        })
        .collect();
    names.sort();
    names
}

fn pptx(path: &Path) -> Result<Vec<Part>> {
    let mut archive = open(path)?;
    let slides = numbered(&archive, "ppt/slides/slide");
    let mut parts = Vec::new();
    for (number, name) in slides {
        let mut text = xml_text(&entry(&mut archive, &name)?, &[b"t"], &[b"p", b"br"], &[]);
        if let Ok(notes) = entry(&mut archive, &format!("ppt/notesSlides/notesSlide{number}.xml")) {
            let notes = xml_text(&notes, &[b"t"], &[b"p", b"br"], &[]);
            // The notes page repeats the slide number as its own line.
            let notes: Vec<&str> =
                notes.lines().filter(|line| !line.trim().is_empty() && line.trim() != number.to_string()).collect();
            if !notes.is_empty() {
                text.push_str("\nNotes: ");
                text.push_str(&notes.join("\n"));
            }
        }
        parts.push(Part { label: Some(format!("Slide {number}")), text });
    }
    Ok(parts)
}

fn xlsx(path: &Path) -> Result<Vec<Part>> {
    let mut archive = open(path)?;
    // Shared strings: one <si> per string, its text in one or more <t>.
    let mut shared = Vec::new();
    if let Ok(xml) = entry(&mut archive, "xl/sharedStrings.xml") {
        let mut reader = Reader::from_str(&xml);
        let (mut current, mut in_text, mut in_phonetic) = (String::new(), false, false);
        loop {
            match reader.read_event() {
                Ok(Event::Start(tag)) => match local(tag.name().as_ref()) {
                    b"si" => current.clear(),
                    b"rPh" => in_phonetic = true,
                    b"t" => in_text = !in_phonetic,
                    _ => {}
                },
                Ok(Event::End(tag)) => match local(tag.name().as_ref()) {
                    b"si" => shared.push(std::mem::take(&mut current)),
                    b"rPh" => in_phonetic = false,
                    b"t" => in_text = false,
                    _ => {}
                },
                Ok(Event::Text(text)) if in_text => {
                    if let Ok(text) = text.unescape() {
                        current.push_str(&text);
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    // Sheet names, in order.
    let mut names = Vec::new();
    if let Ok(xml) = entry(&mut archive, "xl/workbook.xml") {
        let mut reader = Reader::from_str(&xml);
        loop {
            match reader.read_event() {
                Ok(Event::Empty(tag)) | Ok(Event::Start(tag)) if local(tag.name().as_ref()) == b"sheet" => {
                    for attribute in tag.attributes().flatten() {
                        if attribute.key.as_ref() == b"name" {
                            names.push(attribute.unescape_value().map(|v| v.into_owned()).unwrap_or_default());
                        }
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    let mut parts = Vec::new();
    for (index, (_, name)) in numbered(&archive, "xl/worksheets/sheet").into_iter().enumerate() {
        let xml = entry(&mut archive, &name)?;
        let mut reader = Reader::from_str(&xml);
        let mut text = String::new();
        let (mut cell_type, mut in_value, mut row_has_cells) = (String::new(), false, false);
        loop {
            match reader.read_event() {
                Ok(Event::Start(tag)) => match local(tag.name().as_ref()) {
                    b"row" => row_has_cells = false,
                    b"c" => {
                        cell_type.clear();
                        for attribute in tag.attributes().flatten() {
                            if attribute.key.as_ref() == b"t" {
                                cell_type = String::from_utf8_lossy(&attribute.value).into_owned();
                            }
                        }
                    }
                    b"v" | b"t" => in_value = true,
                    _ => {}
                },
                Ok(Event::End(tag)) => match local(tag.name().as_ref()) {
                    b"row" => {
                        if row_has_cells {
                            text.push('\n');
                        }
                    }
                    b"v" | b"t" => in_value = false,
                    _ => {}
                },
                Ok(Event::Text(value)) if in_value => {
                    let Ok(value) = value.unescape() else { continue };
                    let value = if cell_type == "s" {
                        value.trim().parse::<usize>().ok().and_then(|i| shared.get(i)).cloned().unwrap_or_default()
                    } else {
                        value.into_owned()
                    };
                    if !value.is_empty() {
                        if row_has_cells {
                            text.push('\t');
                        }
                        text.push_str(&value);
                        row_has_cells = true;
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            if text.len() > MAX_CHARS {
                break;
            }
        }
        let label = names.get(index).cloned().unwrap_or_else(|| format!("Sheet {}", index + 1));
        if !text.trim().is_empty() {
            parts.push(Part { label: Some(format!("Sheet: {label}")), text: format!("{label}\n{text}") });
        }
    }
    Ok(parts)
}

fn open_document(path: &Path) -> Result<String> {
    let mut archive = open(path)?;
    let xml = entry(&mut archive, "content.xml")?;
    Ok(xml_text(&xml, &[b"p", b"h", b"span", b"a"], &[b"p", b"h", b"table-row", b"line-break"], &[b"tab", b"table-cell"]))
}

fn epub(path: &Path) -> Result<String> {
    let mut archive = open(path)?;
    let mut names: Vec<String> = archive
        .file_names()
        .filter(|name| name.ends_with(".xhtml") || name.ends_with(".html") || name.ends_with(".htm"))
        .map(str::to_string)
        .collect();
    names.sort();
    let mut out = String::new();
    for name in names {
        if let Ok(page) = entry(&mut archive, &name) {
            out.push_str(&html(&page));
            out.push_str("\n\n");
        }
        if out.len() > MAX_CHARS {
            break;
        }
    }
    Ok(out)
}

/// The visible text of an HTML page: tags dropped, scripts and styles with their content, blocks as lines.
pub fn html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() / 2);
    let mut rest = source;
    let mut skip_until: Option<&str> = None;
    while let Some(open) = rest.find('<') {
        if skip_until.is_none() {
            out.push_str(&entities(&rest[..open]));
        }
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else { break };
        let tag = after[..close].trim().to_ascii_lowercase();
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if let Some(until) = skip_until {
            if tag.starts_with('/') && name == until {
                skip_until = None;
            }
        } else if !tag.starts_with('/') && !tag.ends_with('/') {
            skip_until = match name.as_str() {
                "script" => Some("script"),
                "style" => Some("style"),
                "noscript" => Some("noscript"),
                "svg" => Some("svg"),
                _ => None,
            };
        }
        if matches!(name.as_str(), "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            | "section" | "article" | "blockquote" | "pre" | "title" | "table" | "ul" | "ol")
        {
            out.push('\n');
        } else if matches!(name.as_str(), "td" | "th") {
            out.push('\t');
        }
        rest = &after[close + 1..];
    }
    if skip_until.is_none() {
        out.push_str(&entities(rest));
    }
    out
}

fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail.find(';').filter(|&end| end <= 10);
        let replaced = end.and_then(|end| {
            let name = &tail[..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|n| n.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match replaced {
            Some((c, end)) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text of an RTF file: control words dropped, \par as a line, \'hh and \uN as their characters. Font tables,
/// pictures and the like (destinations) are skipped.
pub fn rtf(source: &str) -> String {
    const SKIPPED: &[&str] = &["fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "header", "footer",
        "themedata", "datastore", "latentstyles", "generator", "listtable", "listoverridetable", "rsidtbl", "xmlnstbl"];
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut depth = 0usize;
    let mut skip_depth: Option<usize> = None;
    let mut i = 0;
    let mut pending_skip = 0usize; // characters to drop after \uN
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'\\' | b'{' | b'}' => {
                        if skip_depth.is_none() {
                            out.push(bytes[i] as char);
                        }
                        i += 1;
                    }
                    b'\'' => {
                        let hex = source.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
                        if let Some(byte) = hex {
                            if pending_skip > 0 {
                                pending_skip -= 1;
                            } else if skip_depth.is_none() {
                                out.push(byte as char);
                            }
                        }
                        i += 3;
                    }
                    b'*' => {
                        skip_depth.get_or_insert(depth);
                        i += 1;
                    }
                    b'~' => {
                        out.push(' ');
                        i += 1;
                    }
                    _ => {
                        let start = i;
                        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        let word = &source[start..i];
                        let number_start = i;
                        if i < bytes.len() && bytes[i] == b'-' {
                            i += 1;
                        }
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                        let number: Option<i32> = source[number_start..i].parse().ok();
                        if i < bytes.len() && bytes[i] == b' ' {
                            i += 1;
                        }
                        if word.is_empty() {
                            i = i.max(start + 1);
                            continue;
                        }
                        if SKIPPED.contains(&word) {
                            skip_depth.get_or_insert(depth);
                        } else if skip_depth.is_none() {
                            match word {
                                "par" | "line" | "row" | "sect" | "page" => out.push('\n'),
                                "tab" | "cell" => out.push('\t'),
                                "u" => {
                                    if let Some(code) = number {
                                        let code = if code < 0 { (code + 65536) as u32 } else { code as u32 };
                                        if let Some(c) = char::from_u32(code) {
                                            out.push(c);
                                        }
                                        pending_skip = 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                let c = source[i..].chars().next().unwrap();
                if pending_skip > 0 {
                    pending_skip -= 1;
                } else if skip_depth.is_none() {
                    out.push(c);
                }
                i += c.len_utf8();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(path: &Path, entries: &[(&str, &str)]) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, content) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn word_documents() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("a.docx");
        archive(&path, &[("word/document.xml", r#"<w:document xmlns:w="x"><w:body>
            <w:p><w:r><w:t>Hợp đồng thuê nhà</w:t></w:r></w:p>
            <w:p><w:r><w:t>Pets are </w:t></w:r><w:r><w:t>allowed &amp; welcome.</w:t></w:r></w:p>
            </w:body></w:document>"#)]);
        let parts = read(&path).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].text, "Hợp đồng thuê nhà\nPets are allowed & welcome.");
    }

    #[test]
    fn spreadsheets_by_sheet() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("a.xlsx");
        archive(&path, &[
            ("xl/sharedStrings.xml", "<sst><si><t>Item</t></si><si><r><t>Cof</t></r><r><t>fee</t></r></si></sst>"),
            ("xl/workbook.xml", r#"<workbook><sheets><sheet name="Budget" sheetId="1"/></sheets></workbook>"#),
            ("xl/worksheets/sheet1.xml", r#"<worksheet><sheetData>
                <row><c t="s"><v>0</v></c><c><v>2026</v></c></row>
                <row><c t="s"><v>1</v></c><c><v>4.5</v></c></row></sheetData></worksheet>"#),
        ]);
        let parts = read(&path).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].label.as_deref(), Some("Sheet: Budget"));
        assert_eq!(parts[0].text, "Budget\nItem 2026\nCoffee 4.5");
    }

    #[test]
    fn slides_with_notes() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("a.pptx");
        archive(&path, &[
            ("ppt/slides/slide2.xml", "<p:sld><a:p><a:r><a:t>Roadmap</a:t></a:r></a:p></p:sld>"),
            ("ppt/slides/slide1.xml", "<p:sld><a:p><a:r><a:t>Welcome</a:t></a:r></a:p></p:sld>"),
            ("ppt/notesSlides/notesSlide1.xml", "<p:notes><a:p><a:r><a:t>Say hello first</a:t></a:r></a:p><a:p><a:r><a:t>1</a:t></a:r></a:p></p:notes>"),
        ]);
        let parts = read(&path).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].label.as_deref(), Some("Slide 1"));
        assert_eq!(parts[0].text, "Welcome\n\nNotes: Say hello first");
        assert_eq!(parts[1].text, "Roadmap");
    }

    #[test]
    fn html_and_rtf_lose_their_markup() {
        let page = "<html><head><style>p{color:red}</style><script>var a='<p>no</p>';</script></head>\
                    <body><h1>Title</h1><p>Fish &amp; chips &#233;</p></body></html>";
        assert_eq!(tidy(&html(page)), "Title\n\nFish & chips é");
        let document = r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}\f0 Hello \b world\b0\par caf\'e9 \u272?{\*\generator x;}}";
        assert_eq!(tidy(&rtf(document)), "Hello world\ncafé Đ");
    }

    #[test]
    fn text_encodings() {
        assert_eq!(decode(b"\xEF\xBB\xBFhello"), "hello");
        assert_eq!(decode(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
        assert_eq!(decode("xin chào".as_bytes()), "xin chào");
    }
}
