//! Per-element styling. Each styled element gets a `kx-sN` class; its rule lives in one managed `<style id="kx-styles">`
//! block, so lint still sees the fonts and the user's choice wins over deck rules (`!important`).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::tags::{line_of, nth, start_tags};
use crate::build::assemble::head_insert_at;
use crate::fonts::css::{drop_unused_rules, ensure_faces, fallback};
use crate::fonts::Manifest;

pub const PROPS: &[&str] = &["font-family", "font-size", "color", "font-weight", "font-style", "text-decoration-line", "letter-spacing", "line-height", "text-align"];

pub type Props = BTreeMap<String, String>;

#[derive(Serialize)]
pub struct ElementInfo {
    pub tag: String,
    pub line: u64,
    pub class: Option<String>,
    pub props: Props,
}

const OPEN: &str = "<style id=\"kx-styles\">";

static CLASS_ATTR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)\s+class\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>"']+))"#).unwrap());
static KX_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"kx-s(\d+)").unwrap());

/// The `kx-sN` class in a class list.
fn kx_class<'a>(classes: &[&'a str]) -> Option<&'a str> {
    classes.iter().copied().find(|c| c.strip_prefix("kx-s").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
}

/// Class list of a start tag: the attribute's byte range and its classes.
fn class_of(tag: &str) -> Option<(std::ops::Range<usize>, Vec<&str>)> {
    let c = CLASS_ATTR.captures(tag)?;
    let value = (1..=3).find_map(|i| c.get(i)).map_or("", |m| m.as_str());
    Some((c.get(0).unwrap().range(), value.split_whitespace().collect()))
}

/// Contents of the managed block, as a byte range.
fn block(src: &str) -> Option<std::ops::Range<usize>> {
    let start = src.find(OPEN)? + OPEN.len();
    Some(start..start + src[start..].find("</style>")?)
}

fn rule_re(class: &str) -> Regex {
    Regex::new(&format!(r"\.{}\{{([^}}]*)\}}\n?", regex::escape(class))).unwrap()
}

/// Declarations of `class`'s rule in the managed block.
fn rule_props(src: &str, class: &str) -> Props {
    let Some(b) = block(src) else { return Props::new() };
    let Some(c) = rule_re(class).captures(&src[b]) else { return Props::new() };
    c[1].split(';').filter_map(|d| {
        let (k, v) = d.split_once(':')?;
        Some((k.trim().to_string(), v.trim().trim_end_matches("!important").trim().to_string()))
    }).collect()
}

fn family(props: &Props) -> Option<String> {
    let first = props.get("font-family")?.split(',').next()?.trim().trim_matches(['\'', '"']);
    (!first.is_empty()).then(|| first.to_string())
}

pub fn element_info(src: &str, n: usize) -> Option<ElementInfo> {
    let t = nth(src, n)?;
    let class = class_of(&src[t.start..t.end]).and_then(|(_, cs)| kx_class(&cs).map(String::from));
    let props = class.as_deref().map(|c| rule_props(src, c)).unwrap_or_default();
    Some(ElementInfo { tag: t.name, line: line_of(src, t.start), class, props })
}

/// Replace the element's managed properties with `props` (plus `font`'s family, when given). An empty set unstyles it.
pub fn set_style(src: &str, n: usize, props: &Props, font: Option<&Manifest>) -> Result<String, String> {
    let mut props = props.clone();
    if let Some(m) = font {
        props.insert("font-family".into(), format!("'{}', {}", m.family, fallback(None, &m.category)));
    }
    for (k, v) in &props {
        if !PROPS.contains(&k.as_str()) {
            return Err(format!("can't style '{k}'"));
        }
        if v.contains([';', '{', '}', '<', '>', '\\', '"', '\n', '\r']) {
            return Err(format!("invalid value for '{k}'"));
        }
    }
    let tag = nth(src, n).ok_or("no such element")?;
    let text = &src[tag.start..tag.end];
    let (attr, mut classes) = class_of(text).map_or((None, vec![]), |(r, cs)| (Some(r), cs));
    let existing = kx_class(&classes).map(String::from);
    if props.is_empty() && existing.is_none() {
        return Ok(src.to_string());
    }
    // A class copied onto another element (e.g. a duplicated slide) stays with the others; this one gets its own.
    let shared = existing.as_deref().is_some_and(|k| {
        start_tags(src).iter().filter(|t| class_of(&src[t.start..t.end]).is_some_and(|(_, cs)| cs.contains(&k))).count() > 1
    });
    let old = existing.as_deref().filter(|_| !shared).map(|c| rule_props(src, c)).unwrap_or_default();
    let class = existing.clone().filter(|_| !shared).unwrap_or_else(|| {
        let next = KX_NUM.captures_iter(src).filter_map(|c| c[1].parse::<u64>().ok()).max().unwrap_or(0) + 1;
        format!("kx-s{next}")
    });

    // Tag first: it sits before anything added below, so its offsets are still valid.
    classes.retain(|c| *c != class && Some(*c) != existing.as_deref());
    if !props.is_empty() {
        classes.push(&class);
    }
    let new_attr = if classes.is_empty() { String::new() } else { format!(" class=\"{}\"", classes.join(" ")) };
    let mut out = src.to_string();
    match attr {
        Some(r) => out.replace_range(tag.start + r.start..tag.start + r.end, &new_attr),
        None => out.insert_str(tag.name_end, &new_attr),
    }

    if block(&out).is_none() && !props.is_empty() {
        let at = out.to_ascii_lowercase().find("</head>").or_else(|| head_insert_at(&out)).ok_or("no <head> in source")?;
        out.insert_str(at, &format!("{OPEN}\n</style>\n"));
    }
    if let Some(b) = block(&out) {
        let mut css = rule_re(&class).replace_all(&out[b.clone()], "").into_owned();
        if !props.is_empty() {
            let decls: Vec<_> = PROPS.iter().filter_map(|p| props.get(*p).map(|v| format!("{p}:{v} !important"))).collect();
            css.push_str(&format!(".{class}{{{}}}\n", decls.join(";")));
        }
        out.replace_range(b, &css);
    }
    if let Some(m) = font {
        out = ensure_faces(&out, m)?;
    }
    match family(&old) {
        Some(f) if family(&props).as_ref() != Some(&f) => Ok(drop_unused_rules(out, &f)),
        _ => Ok(out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::lint::lint;
    use crate::fonts::css::fonts_in_use;
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

    /// Index of the first start tag called `name`.
    fn idx(src: &str, name: &str) -> usize {
        start_tags(src).iter().position(|t| t.name == name).unwrap()
    }

    fn props(kv: &[(&str, &str)]) -> Props {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn tag_text(src: &str, n: usize) -> String {
        let t = crate::edit::tags::nth(src, n).unwrap();
        src[t.start..t.end].to_string()
    }

    #[test]
    fn style_sets_rule_and_class() {
        let src = example_src();
        let h2 = idx(&src, "h2");
        let out = set_style(&src, h2, &props(&[("font-size", "96px"), ("color", "#e4482b")]), None).unwrap();
        let h2 = idx(&out, "h2");
        assert!(tag_text(&out, h2).contains("kx-s1\""), "{}", tag_text(&out, h2));
        assert!(out.contains("<style id=\"kx-styles\">"));
        assert!(out.contains(".kx-s1{font-size:96px !important;color:#e4482b !important}"));
        let info = element_info(&out, h2).unwrap();
        assert_eq!((info.tag.as_str(), info.class.as_deref()), ("h2", Some("kx-s1")));
        assert_eq!(info.props, props(&[("font-size", "96px"), ("color", "#e4482b")]));
        assert!(info.line > 1);

        let again = set_style(&out, h2, &props(&[("color", "#000")]), None).unwrap();
        assert!(again.contains(".kx-s1{color:#000 !important}"));
        assert!(!again.contains("font-size:96px"));
        assert_eq!(again.matches("kx-s1").count(), out.matches("kx-s1").count());

        let p = idx(&again, "p");
        let two = set_style(&again, p, &props(&[("color", "red")]), None).unwrap();
        assert!(two.contains(".kx-s2{color:red !important}"));
        assert!(tag_text(&two, idx(&two, "p")).contains("kx-s2"));
    }

    #[test]
    fn clearing_removes_class_and_rule() {
        let src = "<html><head></head><body><p>x</p></body></html>";
        let styled = set_style(src, idx(src, "p"), &props(&[("color", "red")]), None).unwrap();
        assert!(styled.contains("<p class=\"kx-s1\">"));
        let cleared = set_style(&styled, idx(&styled, "p"), &Props::new(), None).unwrap();
        assert!(!cleared.contains("class="), "{cleared}");
        assert!(!cleared.contains(".kx-s1"), "{cleared}");
        // Nothing to clear: untouched.
        assert_eq!(set_style(src, idx(src, "p"), &Props::new(), None).unwrap(), src);
    }

    #[test]
    fn duplicated_class_splits_on_restyle() {
        let src = "<html><head><style id=\"kx-styles\">\n.kx-s1{color:red !important}\n</style>\n</head><body><p class=\"kx-s1\">a</p><p class=\"kx-s1\">b</p></body></html>";
        let second = start_tags(src).iter().rposition(|t| t.name == "p").unwrap();
        let out = set_style(src, second, &props(&[("color", "red"), ("font-size", "9px")]), None).unwrap();
        assert!(out.contains("<p class=\"kx-s1\">a</p><p class=\"kx-s2\">b</p>"), "{out}");
        assert!(out.contains(".kx-s1{color:red !important}"), "{out}");
        assert!(out.contains(".kx-s2{font-size:9px !important;color:red !important}"), "{out}");
        // Clearing a shared-class element unstyles only it; the rule stays for the other.
        let cleared = set_style(src, second, &Props::new(), None).unwrap();
        assert!(cleared.contains("<p class=\"kx-s1\">a</p><p>b</p>"), "{cleared}");
        assert!(cleared.contains(".kx-s1{color:red !important}"), "{cleared}");
    }

    #[test]
    fn class_attribute_variants() {
        for (open, want) in [
            ("<p class='a b'>", "<p class=\"a b kx-s1\">"),
            ("<p class=a>", "<p class=\"a kx-s1\">"),
            ("<p>", "<p class=\"kx-s1\">"),
            ("<p id=x class=\"a\" lang=en>", "<p id=x class=\"a kx-s1\" lang=en>"),
        ] {
            let src = format!("<html><head></head><body>{open}x</p></body></html>");
            let out = set_style(&src, idx(&src, "p"), &props(&[("color", "red")]), None).unwrap();
            assert!(out.contains(want), "{out}");
        }
        // A class that ends up empty takes the attribute with it, other classes stay.
        let src = "<html><head></head><body><p class='a'>x</p></body></html>";
        let styled = set_style(src, idx(src, "p"), &props(&[("color", "red")]), None).unwrap();
        let cleared = set_style(&styled, idx(&styled, "p"), &Props::new(), None).unwrap();
        assert!(cleared.contains("<p class=\"a\">"), "{cleared}");
    }

    #[test]
    fn rejects_injection_and_unknown_props() {
        let src = example_src();
        let h2 = idx(&src, "h2");
        for bad in [props(&[("color", "red;}</style><script>")]), props(&[("font-size", "1px\n")]), props(&[("position", "fixed")])] {
            assert!(set_style(&src, h2, &bad, None).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn style_font_is_linted() {
        let src = example_src();
        let out = set_style(&src, idx(&src, "h2"), &Props::new(), Some(&fam("inter", "Inter", "sans-serif"))).unwrap();
        assert!(out.contains("font-family:'Inter', sans-serif !important"));
        assert!(lint(&out, None, &Deck::default()).errors.iter().any(|e| e.message.starts_with("font: 'inter'")));
    }

    #[test]
    fn restyle_font_drops_old_faces() {
        let src = example_src();
        let a = set_style(&src, idx(&src, "h2"), &Props::new(), Some(&fam("inter", "Inter", "sans-serif"))).unwrap();
        let b = set_style(&a, idx(&a, "h2"), &Props::new(), Some(&fam("geist", "Geist", "sans-serif"))).unwrap();
        assert!(!fonts_in_use(&b).contains("inter"));
        assert!(fonts_in_use(&b).contains("geist"));
        assert!(lint(&b, None, &Deck::default()).errors.iter().all(|e| !e.message.starts_with("font:")));
    }
}
