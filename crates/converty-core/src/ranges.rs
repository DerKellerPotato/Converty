use crate::{Error, Result};

/// Parses a page selection like "1,3-5,8-" against `page_count`.
/// Returns 1-based pages in the order written, duplicates removed.
/// Empty or whitespace-only input = all pages. "8-" = 8 to end, "-3" = 1 to 3.
/// Out of range or malformed parts → `Error::InvalidRange` with a message the UI can show.
pub fn parse_ranges(input: &str, page_count: u32) -> Result<Vec<u32>> {
    let input = input.trim();
    if input.is_empty() {
        return Ok((1..=page_count).collect());
    }
    let bad = |m: String| Error::InvalidRange(m);
    let page = |s: &str, part: &str| -> Result<u32> {
        let n: u32 = s
            .parse()
            .map_err(|_| bad(format!("\u{201c}{part}\u{201d} is not a valid page or range")))?;
        if n == 0 {
            return Err(bad("Pages start at 1".into()));
        }
        if n > page_count {
            return Err(bad(format!(
                "Page {n} does not exist (the document has {page_count} page{})",
                if page_count == 1 { "" } else { "s" }
            )));
        }
        Ok(n)
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for part in input.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (a, b) = match part.split_once('-') {
            None => {
                let n = page(part, part)?;
                (n, n)
            }
            Some((l, r)) => {
                let (l, r) = (l.trim(), r.trim());
                if l.is_empty() && r.is_empty() {
                    return Err(bad(format!("\u{201c}{part}\u{201d} is not a valid page or range")));
                }
                let a = if l.is_empty() { 1 } else { page(l, part)? };
                let b = if r.is_empty() {
                    if page_count == 0 {
                        return Err(bad("The document has no pages".into()));
                    }
                    page_count
                } else {
                    page(r, part)?
                };
                (a, b)
            }
        };
        if a > b {
            return Err(bad(format!("Range {part} is backwards (start is after end)")));
        }
        for n in a..=b {
            if seen.insert(n) {
                out.push(n);
            }
        }
    }
    if out.is_empty() {
        return Err(bad("No pages selected".into()));
    }
    Ok(out)
}
