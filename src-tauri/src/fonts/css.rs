//! Rewriting deck.src.html: `@font-face` rules live in one managed `<style id="kx-fonts">`, roles are `--font-*` variables.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::{Manifest, ROLES};
use crate::build::assemble::head_insert_at;

const OPEN: &str = "<style id=\"kx-fonts\">";

static IN_USE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"assets/fonts/([a-z0-9-]+)/").unwrap());

pub fn font_face_css(m: &Manifest) -> String {
    m.files.iter().map(|f| format!(
        "@font-face{{font-family:'{}';src:url(assets/fonts/{}/{}) format('woff2');font-weight:{};font-style:{};font-display:block}}\n",
        m.family, m.id, f.file, f.weight, f.style,
    )).collect()
}

/// Make sure the managed block exists and holds `m`'s `@font-face` rules (once).
pub fn ensure_faces(src: &str, m: &Manifest) -> Result<String, String> {
    let mut out = src.to_string();
    if !out.contains(OPEN) {
        let at = head_insert_at(&out).ok_or("no <head> in source")?;
        out.insert_str(at, &format!("\n{OPEN}\n</style>"));
    }
    if !out.contains(&format!("assets/fonts/{}/", m.id)) {
        out.insert_str(close(&out), &font_face_css(m));
    }
    Ok(out)
}

/// End of the managed block's content (the offset of its `</style>`).
fn close(s: &str) -> usize {
    let i = s.find(OPEN).unwrap();
    i + s[i..].find("</style>").unwrap()
}

/// Generic family a font falls back to: monospace for the mono role, otherwise by category.
pub(crate) fn fallback(role: Option<&str>, category: &str) -> &'static str {
    match (role, category) {
        (Some("mono"), _) => "monospace",
        (_, "serif") => "serif",
        (_, "handwriting") => "cursive",
        _ => "sans-serif",
    }
}

/// Embed `m`'s `@font-face` rules (once) and point `--font-<role>` at it: the first existing declaration is rewritten,
/// otherwise a `:root` rule is added to the managed block.
pub fn apply_font(src: &str, role: &str, m: &Manifest) -> Result<String, String> {
    if !ROLES.contains(&role) {
        return Err(format!("unknown font role '{role}'"));
    }
    let mut out = ensure_faces(src, m)?;
    let value = format!("'{}', {}", m.family, fallback(Some(role), &m.category));
    let decl = Regex::new(&format!(r"(--font-{role}\s*:\s*)[^;}}]*")).unwrap();
    if let Some(c) = decl.captures(&out) {
        let (whole, prefix) = (c.get(0).unwrap().range(), c.get(1).unwrap().end());
        let old = out[prefix..whole.end].split(',').next().unwrap_or("").trim().trim_matches(['\'', '"']).to_string();
        out.replace_range(prefix..whole.end, &value);
        if !old.is_empty() && old != m.family {
            out = drop_unused_rules(out, &old);
        }
    } else {
        out.insert_str(close(&out), &format!(":root{{--font-{role}:{value}}}\n"));
    }
    Ok(out)
}

/// Remove `family`'s `@font-face` rules from the managed block when nothing outside the block names it any more, so a
/// replaced font stops tripping lint and gets pruned from the zip on save.
pub(crate) fn drop_unused_rules(src: String, family: &str) -> String {
    let Some(start) = src.find(OPEN) else { return src };
    let end = start + src[start..].find("</style>").unwrap();
    if src[..start].contains(family) || src[end..].contains(family) {
        return src;
    }
    let rule = Regex::new(&format!(r"@font-face\{{font-family:'{}';[^}}]*\}}\n?", regex::escape(family))).unwrap();
    let block = rule.replace_all(&src[start..end], "");
    format!("{}{block}{}", &src[..start], &src[end..])
}

/// Family ids the source references through `assets/fonts/<id>/`.
pub fn fonts_in_use(src: &str) -> BTreeSet<String> {
    IN_USE.captures_iter(src).map(|c| c[1].to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::lint::lint;
    use crate::fonts::{FontFile, Manifest};
    use crate::stage_file::{Deck, SRC};

    fn fam(id: &str, family: &str, category: &str) -> Manifest {
        let file = |f: &str, s: &str| FontFile { file: f.into(), style: s.into(), weight: "200 800".into() };
        Manifest { id: id.into(), family: family.into(), category: category.into(),
            files: vec![file("latin-wght-normal.woff2", "normal"), file("latin-wght-italic.woff2", "italic")] }
    }

    fn example_src() -> String {
        Deck::example().text(SRC).unwrap()
    }

    #[test]
    fn apply_creates_block_and_replaces_var() {
        let src = example_src();
        let out = apply_font(&src, "display", &fam("newsreader", "Newsreader", "serif")).unwrap();
        assert_eq!(out.matches("<style id=\"kx-fonts\">").count(), 1);
        assert!(out.contains("--font-display: 'Newsreader', serif"));
        assert!(!out.contains("--font-display: 'Fraunces', serif;"));
        assert!(out.contains("src:url(assets/fonts/newsreader/latin-wght-normal.woff2) format('woff2')"));
        assert!(out.contains("font-display:block"));
        let again = apply_font(&out, "display", &fam("newsreader", "Newsreader", "serif")).unwrap();
        assert_eq!(again, out);
    }

    #[test]
    fn apply_adds_root_when_var_missing() {
        let src = "<html><head><meta charset=\"utf-8\"><title>t</title></head><body></body></html>";
        let out = apply_font(src, "body", &fam("geist", "Geist", "sans-serif")).unwrap();
        let block = &out[out.find("<style id=\"kx-fonts\">").unwrap()..];
        let block = &block[..block.find("</style>").unwrap()];
        assert!(block.contains(":root{--font-body:'Geist', sans-serif}"), "{block}");
        assert!(out.find("kx-fonts").unwrap() > out.find("charset").unwrap());
    }

    #[test]
    fn mono_role_falls_back_to_monospace() {
        let src = "<html><head></head><body></body></html>";
        let out = apply_font(src, "mono", &fam("geist", "Geist", "sans-serif")).unwrap();
        assert!(out.contains("--font-mono:'Geist', monospace"));
        assert!(apply_font(src, "title", &fam("geist", "Geist", "sans-serif")).is_err());
    }

    #[test]
    fn fonts_in_use_reads_urls() {
        let src = apply_font(&example_src(), "display", &fam("newsreader", "Newsreader", "serif")).unwrap();
        let src = apply_font(&src, "body", &fam("geist", "Geist", "sans-serif")).unwrap();
        let ids: Vec<String> = fonts_in_use(&src).into_iter().collect();
        assert!(ids.contains(&"geist".to_string()) && ids.contains(&"newsreader".to_string()), "{ids:?}");
    }

    #[test]
    fn replacing_a_font_drops_its_rules() {
        let src = apply_font(&example_src(), "body", &fam("inter", "Inter", "sans-serif")).unwrap();
        let src = apply_font(&src, "body", &fam("geist", "Geist", "sans-serif")).unwrap();
        assert!(!fonts_in_use(&src).contains("inter"), "stale Inter rules left behind");
        assert!(lint(&src, None, &Deck::default()).errors.iter().all(|e| !e.message.starts_with("font:")));

        // Still used by another role: its rules stay.
        let src = apply_font(&src, "display", &fam("geist", "Geist", "sans-serif")).unwrap();
        let src = apply_font(&src, "body", &fam("newsreader", "Newsreader", "serif")).unwrap();
        assert!(fonts_in_use(&src).contains("geist"));
    }

    #[test]
    fn banned_font_still_errors() {
        let src = apply_font(&example_src(), "body", &fam("inter", "Inter", "sans-serif")).unwrap();
        let has = |s: &str| lint(s, None, &Deck::default()).errors.iter().any(|e| e.message.starts_with("font: 'inter'"));
        assert!(has(&src));
        let allowed = src.replacen("<head>", "<head><meta name=\"kx-allow-fonts\" content=\"inter\">", 1);
        assert!(!has(&allowed));
    }
}
