//! Port of build.py `assemble`: inject the engine into deck.src.html.

use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

pub const ENGINE_CSS: &str = include_str!("../../../engine/engine.css");
pub const ENGINE_JS: &str = include_str!("../../../engine/engine.js");

const GSAP_CDN: &str = "https://cdn.jsdelivr.net/npm/gsap@3.13.0/dist/";
/// (file, global it defines). The bundled copies are served by the stage protocol under `VENDOR_PREFIX`.
pub const GSAP_FILES: &[(&str, &str)] = &[("gsap.min.js", "gsap"), ("SplitText.min.js", "SplitText"), ("ScrambleTextPlugin.min.js", "ScrambleTextPlugin")];
pub const VENDOR_PREFIX: &str = "_stage/vendor/";

static HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<head[^>]*>").unwrap());
static CHARSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<meta[^>]+charset[^>]*>").unwrap());

/// CDN first; if a global is missing after its CDN script ran, document.write the bundled copy (blocks parsing like the original tag).
fn gsap_tags() -> String {
    GSAP_FILES.iter().map(|(f, global)| {
        format!("<script src=\"{GSAP_CDN}{f}\"></script>\n<script>window.{global}||document.write('<script src=\"{VENDOR_PREFIX}{f}\"><\\/script>')</script>")
    }).collect::<Vec<_>>().join("\n")
}

pub fn assemble(src: &str, brief: Option<&Value>) -> Result<String, String> {
    let mut payload = Map::new();
    if let Some(b) = brief.filter(|b| b.as_object().is_some_and(|o| !o.is_empty())) {
        let list = |k: &str| b.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let sources: Map<String, Value> = list("sources").iter()
            .map(|s| (id(s), json!({"ref": s.get("ref"), "kind": s.get("kind")}))).collect();
        let claims: Map<String, Value> = list("claims").iter()
            .map(|c| (id(c), json!({"text": c.get("text"), "source": c.get("source"), "locator": c.get("locator")}))).collect();
        payload.insert("sources".into(), sources.into());
        payload.insert("claims".into(), claims.into());
    }
    let data = Value::Object(payload).to_string().replace('<', "\\u003c");

    let at = head_insert_at(src).ok_or("build: no <head> in source")?;
    let src = format!("{}\n<style id=\"kx-engine\">\n{ENGINE_CSS}\n</style>{}", &src[..at], &src[at..]);

    let tail = format!("<script type=\"application/json\" id=\"kx-sources\">{data}</script>\n{}\n<script>\n{ENGINE_JS}\n</script>\n", gsap_tags());
    let i = src.to_ascii_lowercase().rfind("</body>").ok_or("build: no </body> in source")?;
    Ok(format!("{}{tail}{}", &src[..i], &src[i..]))
}

/// Just inside `<head>`, after `<meta charset>` when it comes first.
pub(crate) fn head_insert_at(src: &str) -> Option<usize> {
    let m = HEAD.find(src)?;
    Some(m.end() + CHARSET.find(&src[m.end()..]).filter(|c| c.start() < 200).map_or(0, |c| c.end()))
}

fn id(v: &Value) -> String {
    v.get("id").and_then(Value::as_str).unwrap_or("").to_string()
}
