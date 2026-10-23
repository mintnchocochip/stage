//! Start-tag scanner. Numbers every real start tag the way the browser would, so the preview's
//! `data-kx-i` stamps can be mapped back to byte offsets in the unstamped source.

#[derive(Debug, Clone)]
pub struct Tag {
    /// Byte offsets: the `<`, the end of the tag name, and just after the `>`.
    pub start: usize,
    pub name_end: usize,
    pub end: usize,
    /// Lowercased.
    pub name: String,
}

pub fn start_tags(src: &str) -> Vec<Tag> {
    let b = src.as_bytes();
    let lower = src.to_ascii_lowercase(); // same byte offsets as src
    let find = |pat: &str, from: usize| lower[from..].find(pat).map_or(b.len(), |i| from + i);
    let mut tags = vec![];
    let mut i = 0;
    while let Some(p) = src[i..].find('<').map(|p| p + i) {
        let next = b.get(p + 1).copied().unwrap_or(0);
        if next.is_ascii_alphabetic() {
            let name_end = (p + 1..b.len()).find(|&j| b[j].is_ascii_whitespace() || b[j] == b'/' || b[j] == b'>').unwrap_or(b.len());
            let mut q = name_end;
            let mut quote = 0u8;
            while q < b.len() {
                match (quote, b[q]) {
                    (0, b'>') => break,
                    (0, c @ (b'"' | b'\'')) => quote = c,
                    (c, d) if c == d => quote = 0,
                    _ => {}
                }
                q += 1;
            }
            let end = (q + 1).min(b.len());
            let name = lower[p + 1..name_end].to_string();
            i = end;
            if matches!(name.as_str(), "script" | "style" | "textarea" | "title" | "template") {
                i = find(&format!("</{name}"), end);
            }
            tags.push(Tag { start: p, name_end, end, name });
        } else if src[p..].starts_with("<!--") {
            i = (find("-->", p + 4) + 3).min(b.len());
        } else if next == b'!' || next == b'?' || next == b'/' {
            i = (find(">", p) + 1).min(b.len());
        } else {
            i = p + 1;
        }
    }
    tags
}

pub fn stamp(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 16 * 64);
    let mut at = 0;
    for (n, t) in start_tags(src).iter().enumerate() {
        out.push_str(&src[at..t.name_end]);
        out.push_str(&format!(" data-kx-i=\"{n}\""));
        at = t.name_end;
    }
    out.push_str(&src[at..]);
    out
}

pub fn nth(src: &str, n: usize) -> Option<Tag> {
    start_tags(src).into_iter().nth(n)
}

/// 1-based.
pub fn line_of(src: &str, offset: usize) -> u64 {
    1 + src.as_bytes()[..offset.min(src.len())].iter().filter(|&&c| c == b'\n').count() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::lint::lint;
    use crate::stage_file::{Deck, SRC};

    const DOC: &str = "<!doctype html><html><head><title>a<b</title><style>a>b{}</style></head>\n<!-- <p>no</p> --><body><section class=\"slide\"><h2 title=\"x>y\">Hi</h2><br/>\n<img src=a.png><script>if (a<b) document.write(\"<div>\")</script><DIV></DIV><p>t</p></section></body></html>";

    #[test]
    fn start_tags_skip_non_tags() {
        let names: Vec<_> = start_tags(DOC).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["html", "head", "title", "style", "body", "section", "h2", "br", "img", "script", "div", "p"]);
    }

    #[test]
    fn stamp_numbers_in_order() {
        let s = stamp(DOC);
        assert!(s.contains("<h2 data-kx-i=\"6\" title=\"x>y\">"));
        assert!(s.contains("<DIV data-kx-i=\"10\">"));
        assert_eq!(nth(DOC, 6).unwrap().name, "h2");
        assert_eq!(line_of(DOC, nth(DOC, 8).unwrap().start), 3);
    }

    #[test]
    fn stamped_example_still_lints_clean() {
        let deck = Deck::example();
        let src = deck.text(SRC).unwrap();
        assert!(lint(&stamp(&src), None, &deck).errors.is_empty());
    }
}
