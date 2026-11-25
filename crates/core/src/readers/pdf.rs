//! PDF text, page by page. Pages with little text (scans, slides saved as pictures) are rendered by Windows' own
//! PDF reader instead (`winrt::PdfFile`).

use anyhow::{anyhow, Result};
use std::path::Path;

/// A page with fewer letters and digits than this is read as a picture.
pub const SPARSE: usize = 200;

/// The text of every page. A PDF the text reader can't follow gives an error, and its pages are read as pictures.
pub fn page_texts(path: &Path) -> Result<Vec<String>> {
    let path = path.to_path_buf();
    // The parser panics on some malformed files; one bad PDF mustn't end the run.
    let result = std::panic::catch_unwind(move || pdf_extract::extract_text_by_pages(&path));
    match result {
        Ok(Ok(pages)) => Ok(pages.into_iter().map(|page| crate::text::tidy(&page)).collect()),
        Ok(Err(error)) => Err(anyhow!("can't read the PDF's text: {error}")),
        Err(_) => Err(anyhow!("the PDF's text can't be read")),
    }
}

/// Whether a page's text says enough to stand for it.
pub fn has_text(page: &str) -> bool {
    page.chars().filter(|c| c.is_alphanumeric()).count() >= SPARSE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF whose catalog holds an array nested 100,000 deep: the parser has to give up, not overflow the stack
    /// (which no `catch_unwind` can stop, and which would end the whole process).
    #[test]
    fn deeply_nested_objects_end_in_an_error_not_a_crash() {
        let depth = 100_000;
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let nested = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        for body in [
            format!("<< /Type /Catalog /Pages 2 0 R /Deep {nested} >>"),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_string(),
        ] {
            offsets.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{body}\nendobj\n", offsets.len()).bytes());
        }
        let xref = pdf.len();
        pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).bytes());
        for offset in &offsets {
            pdf.extend(format!("{offset:010} 00000 n \n").bytes());
        }
        pdf.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).bytes(),
        );
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("deep.pdf");
        std::fs::write(&path, pdf).unwrap();
        // Whatever the reader makes of it, it comes back.
        let _ = page_texts(&path);
    }
}
