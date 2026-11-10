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
