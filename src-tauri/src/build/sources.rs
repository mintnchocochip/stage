//! Text of claim sources packed in the .stage zip, for quote verification (build.py `source_text`).

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::rc::Rc;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::lint::ws;
use crate::stage_file::Deck;

/// Resolve a brief.json `ref` to a zip entry name. brief.json sits at the zip root; refs that leave it resolve to nothing.
pub fn entry_name(r: &str) -> Option<String> {
    let mut parts: Vec<&str> = vec![];
    let norm = r.replace('\\', "/");
    for seg in norm.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop()?; }
            s => parts.push(s),
        }
    }
    (!parts.is_empty() && !r.starts_with('/') && !r.contains(':')).then(|| parts.join("/"))
}

/// Whitespace-normalized text of a source, or None if it cannot be read.
pub fn source_text(deck: &Deck, r: &str, cache: &mut HashMap<String, Option<Rc<str>>>) -> Option<Rc<str>> {
    if let Some(hit) = cache.get(r) {
        return hit.clone();
    }
    let text = entry_name(r).and_then(|name| {
        let bytes = deck.get(&name)?;
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "docx" => office_text(bytes, |n| n == "word/document.xml", "w:t", "w:p", &["w:tab"], &["w:br", "w:cr"]),
            "pptx" => office_text(bytes, |n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"), "a:t", "a:p", &[], &["a:br"]),
            "pdf" => pdf_extract::extract_text_from_mem(bytes).ok(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "mp4" => None,
            _ => Some(String::from_utf8_lossy(bytes).into_owned()),
        }
    });
    let out: Option<Rc<str>> = text.map(|t| ws(&t).into());
    cache.insert(r.to_string(), out.clone());
    out
}

/// Text runs of the XML parts of an Office zip, one line per paragraph.
/// ponytail: docx table text comes in document order, python-docx appends tables last; only quotes spanning a table edge differ.
fn office_text(bytes: &[u8], want: impl Fn(&str) -> bool, run: &str, para: &str, tabs: &[&str], breaks: &[&str]) -> Option<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().and_then(|f| f.name().ok().map(|n| n.into_owned()))).filter(|n| want(n)).collect();
    // slide2.xml before slide10.xml
    names.sort_by_key(|n| (n.len(), n.clone()));
    let mut out = String::new();
    for name in names {
        let mut xml = String::new();
        zip.by_name(&name).ok()?.read_to_string(&mut xml).ok()?;
        let mut rd = Reader::from_str(&xml);
        let mut in_run = false;
        loop {
            match rd.read_event().ok()? {
                Event::Start(e) if e.name().as_ref() == run => in_run = true,
                Event::End(e) if e.name().as_ref() == run => in_run = false,
                Event::End(e) if e.name().as_ref() == para => out.push('\n'),
                Event::Empty(e) if tabs.iter().any(|t| e.name().as_ref() == *t) => out.push('\t'),
                Event::Empty(e) if breaks.iter().any(|t| e.name().as_ref() == *t) => out.push('\n'),
                Event::Text(t) if in_run => out.push_str(&t.xml10_content()),
                Event::GeneralRef(r) if in_run => match r.resolve_char_ref().ok()? {
                    Some(c) => out.push(c),
                    None => out.push_str(match &*r.xml10_content() {
                        "amp" => "&", "lt" => "<", "gt" => ">", "quot" => "\"", "apos" => "'", _ => "",
                    }),
                },
                Event::Eof => break,
                _ => {}
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn office(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn docx_and_pptx_text() {
        let mut d = Deck::default();
        d.set("sources/a.docx", office(&[("word/document.xml",
            r#"<w:document><w:body><w:p><w:r><w:t>Tom &amp; Jerry</w:t></w:r><w:r><w:t xml:space="preserve"> ran&#33;</w:t></w:r></w:p><w:p><w:r><w:t>Next</w:t></w:r></w:p></w:body></w:document>"#)]));
        d.set("sources/b.pptx", office(&[
            ("ppt/slides/slide10.xml", "<p:sld><a:p><a:r><a:t>ten</a:t></a:r></a:p></p:sld>"),
            ("ppt/slides/slide2.xml", "<p:sld><a:p><a:r><a:t>two</a:t></a:r></a:p></p:sld>"),
        ]));
        let mut cache = HashMap::new();
        assert_eq!(source_text(&d, "sources/a.docx", &mut cache).as_deref(), Some("Tom & Jerry ran! Next"));
        assert_eq!(source_text(&d, "sources/b.pptx", &mut cache).as_deref(), Some("two ten"));
        assert_eq!(source_text(&d, "sources/none.pdf", &mut cache), None);
    }

    /// One-page PDF with a Helvetica text line, offsets computed so the xref table is valid.
    fn tiny_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offs = vec![];
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for o in offs { out.extend(format!("{o:010} 00000 n \n").bytes()); }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        out
    }

    #[test]
    fn pdf_text() {
        let mut d = Deck::default();
        d.set("sources/p.pdf", tiny_pdf("The stage is 1920 by 1080"));
        let t = source_text(&d, "sources/p.pdf", &mut HashMap::new()).unwrap();
        assert!(t.contains("The stage is 1920 by 1080"), "got {t:?}");
    }

    #[test]
    fn refs_stay_inside_zip() {
        assert_eq!(entry_name("sources/a.txt").as_deref(), Some("sources/a.txt"));
        assert_eq!(entry_name("./sources\\b.md").as_deref(), Some("sources/b.md"));
        assert_eq!(entry_name("x/../y.txt").as_deref(), Some("y.txt"));
        assert_eq!(entry_name("../engine.js"), None);
        assert_eq!(entry_name("C:/x.txt"), None);
        assert_eq!(entry_name(""), None);
    }
}
