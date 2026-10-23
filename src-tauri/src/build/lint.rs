//! Port of stagecraft `scripts/build.py` lint. Messages match the Python ones so decks lint the same in both.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

use super::sources::source_text;
use crate::stage_file::Deck;

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
const CODE_TAGS: &[&str] = &["code", "pre", "kbd", "samp"];
const TEXT_ATTRS: &[&str] = &["data-title", "data-tip", "alt", "aria-label", "title"];
const BLOCK: &[&str] = &["p", "li", "h1", "h2", "h3", "h4", "h5", "h6", "div", "td", "th", "figcaption", "blockquote", "button", "dt", "dd", "text", "tspan", "label"];
pub(crate) const BANNED_FONTS: &[&str] = &["inter", "roboto", "arial", "helvetica", "open sans", "lato", "montserrat", "poppins", "space grotesk", "segoe ui", "system-ui"];
const GENERIC_TITLES: &[&str] = &["introduction", "intro", "overview", "agenda", "summary", "conclusion", "conclusions", "thank you", "thanks",
    "q&a", "questions", "key takeaways", "takeaways", "background", "next steps", "about us", "the problem", "the solution"];
const BANNED_PHRASES: &[&str] = &[
    "delve", "leverage", "leveraging", "seamless", "seamlessly", "unlock", "unlocking", "elevate", "game-changer", "game changer",
    "revolutionize", "revolutionary", "tapestry", "realm", "harness the power", "cutting-edge", "cutting edge", "fast-paced",
    "ever-evolving", "ever-changing", "landscape", "empower", "supercharge", "next-level", "embark", "testament", "paradigm shift",
    "synergy", "robust solution", "in today's", "dive into", "deep dive", "let's explore", "at the end of the day", "it's worth noting",
    "furthermore", "moreover", "in conclusion", "a myriad of", "plethora", "transformative", "unleash", "journey", "holistic",
    "best-in-class", "world-class", "state-of-the-art", "mission-critical", "pivotal", "crucial role", "navigate the",
];

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}
re!(DASHES, "[‒–—―]");
re!(SPACED_HYPHEN, r"\w\s+-{1,2}\s+\w");
re!(EMOJI, "[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{1F000}-\u{1F2FF}\u{FE0F}]");
re!(NUMBER_AT, r"^\d+(?:[.,]\d+)*");
re!(ANY_NUMBER, r"\d+(?:[.,]\d+)*");
re!(NOT_X_BUT_Y, r"(?i)\b(not just|not only|isn't just|is not just|more than just|it's not about)\b[^.]{0,80}\b(but|it's|it is)\b");
re!(CITATION, r"(?i)(?:table|tab\.|section|sec\.|chapter|ch\.|figure|fig\.|page|p\.|pp\.|slide|appendix|eq\.|equation|ref\.|§)\s*$");
re!(WS, r"\s+");
re!(UNMARK, r"(?:^|\s)(?:#+:?|//+|\*|--)");
re!(GF_FAMILY, r"family=([^&:]+)");
re!(FS_FAMILY, r"f\[\]=([\w-]+)");
re!(CSS_FONT_FAMILY, r"font-family\s*:\s*([^;}{]+)");
re!(CSS_FONT_VAR, r"--font[\w-]*\s*:\s*([^;}{]+)");
re!(URL_SCHEME, r"^[a-z]+://");
re!(BODY_TAG, r"<body[^>]*>");
re!(DATA_MOTION, r#"\bdata-motion="(\d)""#);
re!(DATA_LEVELS, r#"\bdata-levels="([^"]*)""#);
re!(LEVEL_SPLIT, r"[\s,]+");

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub message: String,
    /// 1-based line of the slide in deck.src.html, when the finding belongs to a slide.
    pub line: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Report {
    pub slides: usize,
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
    /// Claims from model knowledge the user should verify.
    pub verify: Vec<String>,
}

#[derive(Default)]
struct Slide {
    id: Option<String>,
    layout: Option<String>,
    claims: Vec<String>,
    title_attr: Option<String>,
    text: Vec<String>,
    notes: Vec<String>,
    headings: Vec<String>,
    anims: usize,
    has_notes: bool,
    line: u64,
}

type Attrs = HashMap<String, String>;

#[derive(Default)]
struct Parser {
    stack: Vec<(String, Attrs)>,
    slides: Vec<Slide>,
    cur: Option<usize>,
    fonts_css: Vec<String>,
    in_style: bool,
    allow_fonts: HashSet<String>,
    has_stage: bool,
    has_viewport: bool,
    pending: String,
}

impl Parser {
    fn skip(&self) -> bool {
        self.stack.iter().any(|(t, a)| {
            matches!(t.as_str(), "script" | "style" | "template") || a.contains_key("data-deco") || a.get("aria-hidden").is_some_and(|v| v == "true")
        })
    }
    fn within(&self, names: &[&str]) -> bool {
        self.stack.iter().any(|(t, _)| names.contains(&t.as_str()))
    }
    fn in_notes(&self) -> bool {
        self.stack.iter().any(|(_, a)| classes(a).any(|c| c == "notes"))
    }
    fn cur(&mut self) -> Option<&mut Slide> {
        self.cur.map(|i| &mut self.slides[i])
    }

    fn start(&mut self, tag: &str, a: Attrs, line: u64) {
        let cls: Vec<String> = classes(&a).map(str::to_string).collect();
        let has = |c: &str| cls.iter().any(|x| x == c);
        if has("kx-stage") { self.has_stage = true; }
        if has("kx-viewport") { self.has_viewport = true; }
        if tag == "meta" && a.get("name").is_some_and(|v| v == "kx-allow-fonts") {
            for f in a.get("content").map(String::as_str).unwrap_or("").split(',') {
                let f = f.trim();
                if !f.is_empty() { self.allow_fonts.insert(f.to_lowercase()); }
            }
        }
        let href = a.get("href").map(String::as_str).unwrap_or("");
        if tag == "link" && href.contains("fonts.googleapis.com") {
            let h = href.replace('+', " ");
            self.fonts_css.extend(GF_FAMILY.captures_iter(&h).map(|c| c[1].to_string()));
        }
        if tag == "link" && href.contains("api.fontshare.com") {
            self.fonts_css.extend(FS_FAMILY.captures_iter(href).map(|c| c[1].replace('-', " ")));
        }
        if tag == "section" && has("slide") {
            self.slides.push(Slide {
                id: a.get("id").cloned(),
                layout: a.get("data-layout").cloned(),
                claims: a.get("data-claims").map(|c| c.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
                title_attr: a.get("data-title").cloned(),
                line,
                ..Default::default()
            });
            self.cur = Some(self.slides.len() - 1);
        }
        if self.cur.is_some() {
            let skip = self.skip();
            let s = self.cur().unwrap();
            if a.contains_key("data-anim") || a.contains_key("data-move") { s.anims += 1; }
            if has("notes") { s.has_notes = true; }
            for k in TEXT_ATTRS {
                if let Some(v) = a.get(*k).filter(|v| !v.is_empty()) {
                    if !skip { s.text.push(format!("\n{v}\n")); }
                }
            }
        }
        if tag == "style" { self.in_style = true; }
        if BLOCK.contains(&tag) {
            if let Some(s) = self.cur() { s.text.push("\n".into()); s.notes.push("\n".into()); }
        }
        if !VOID.contains(&tag) { self.stack.push((tag.to_string(), a)); }
        if tag == "h1" || tag == "h2" {
            if let Some(s) = self.cur() { s.headings.push(String::new()); }
        }
    }

    fn end(&mut self, tag: &str) {
        if tag == "style" { self.in_style = false; }
        if BLOCK.contains(&tag) {
            if let Some(s) = self.cur() { s.text.push("\n".into()); s.notes.push("\n".into()); }
        }
        if let Some(i) = self.stack.iter().rposition(|(t, _)| t == tag) {
            self.stack.truncate(i);
        }
        if tag == "section" && self.cur.is_some() && !self.stack.iter().any(|(t, _)| t == "section") {
            self.cur = None;
        }
    }

    fn data(&mut self, data: &str) {
        if self.in_style {
            self.fonts_css.extend(CSS_FONT_FAMILY.captures_iter(data).map(|c| c[1].to_string()));
            self.fonts_css.extend(CSS_FONT_VAR.captures_iter(data).map(|c| c[1].to_string()));
            return;
        }
        if self.cur.is_none() || data.trim().is_empty() || self.skip() { return; }
        let (notes, code, heading) = (self.in_notes(), self.within(CODE_TAGS), self.within(&["h1", "h2"]));
        let s = self.cur().unwrap();
        if notes {
            s.notes.push(data.to_string());
        } else if !code {
            s.text.push(data.to_string());
            if heading {
                if let Some(h) = s.headings.last_mut() { h.push_str(data); }
            }
        }
    }

    /// Python's HTMLParser delivers the text between two tags as one chunk; html5ever splits it, so buffer until the next token.
    fn flush(&mut self) {
        if !self.pending.is_empty() {
            let d = std::mem::take(&mut self.pending);
            self.data(&d);
        }
    }
}

fn classes(a: &Attrs) -> impl Iterator<Item = &str> {
    a.get("class").map(String::as_str).unwrap_or("").split_whitespace()
}

struct Sink(RefCell<Parser>);

impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<()> {
        let mut p = self.0.borrow_mut();
        match token {
            Token::CharacterTokens(t) => p.pending.push_str(&t),
            Token::NullCharacterToken => p.pending.push('\0'),
            Token::ParseError(_) => {}
            Token::TagToken(t) => {
                p.flush();
                let name = t.name.to_string();
                match t.kind {
                    TagKind::StartTag => {
                        let a: Attrs = t.attrs.iter().map(|x| (x.name.local.to_string(), x.value.to_string())).collect();
                        p.start(&name, a, line);
                        if t.self_closing && !VOID.contains(&name.as_str()) && p.stack.last().is_some_and(|(n, _)| *n == name) {
                            p.stack.pop();
                        }
                        // Python's HTMLParser treats only script and style content as raw text.
                        if !t.self_closing {
                            match name.as_str() {
                                "script" => return TokenSinkResult::RawData(RawKind::ScriptData),
                                "style" => return TokenSinkResult::RawData(RawKind::Rawtext),
                                _ => {}
                            }
                        }
                    }
                    TagKind::EndTag => p.end(&name),
                }
            }
            _ => p.flush(),
        }
        TokenSinkResult::Continue
    }
}

fn parse(src: &str) -> Parser {
    let tok = Tokenizer::new(Sink(RefCell::new(Parser::default())), TokenizerOpts::default());
    let buf = BufferQueue::default();
    buf.push_back(StrTendril::from(src));
    let _ = tok.feed(&buf);
    tok.end();
    let mut p = tok.sink.0.into_inner();
    p.flush();
    p
}

pub fn ws(s: &str) -> String {
    WS.replace_all(s, " ").trim().to_string()
}

fn norm_num(s: &str) -> String {
    s.replace(',', "")
}

/// Drop comment markers so a quote that wraps across commented source lines still matches.
/// Emulates the Python `(?=\s)` lookahead: a marker only counts when whitespace follows it.
pub fn unmark(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut pos, mut last) = (0, 0);
    while let Some(m) = UNMARK.find_at(s, pos) {
        if s[m.end()..].chars().next().is_some_and(char::is_whitespace) {
            out.push_str(&s[last..m.start()]);
            last = m.end();
            pos = m.end();
        } else {
            pos = m.start() + s[m.start()..].chars().next().map_or(1, char::len_utf8);
        }
        if pos >= s.len() { break; }
    }
    out.push_str(&s[last..]);
    out
}

/// `(?<![A-Za-z0-9\-_/#.])\d+(?:[.,]\d+)*` scanned the way Python's finditer does. Returns (start byte, match).
fn numbers(text: &str) -> Vec<(usize, &str)> {
    let mut out = vec![];
    let mut prev: Option<char> = None;
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        let blocked = prev.is_some_and(|p| p.is_ascii_alphanumeric() || "-_/#.".contains(p));
        if !blocked && c.is_numeric() {
            if let Some(m) = NUMBER_AT.find(&text[i..]) {
                let end = i + m.end();
                out.push((i, &text[i..end]));
                prev = Some(c);
                while iter.peek().is_some_and(|(j, _)| *j < end) {
                    prev = iter.next().map(|(_, ch)| ch);
                }
                continue;
            }
        }
        prev = Some(c);
    }
    out
}

/// Chars `before` chars before byte `start` up to `after` chars after byte `end`, like Python slicing.
fn context(s: &str, start: usize, end: usize, before: usize, after: usize) -> &str {
    let a = s[..start].char_indices().rev().nth(before.saturating_sub(1)).map_or(0, |(i, _)| i);
    let b = s[end..].char_indices().nth(after).map_or(s.len(), |(i, _)| end + i);
    &s[a..b]
}

fn first_chars(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

/// `re.split(r"(?<=[.?!])\s+", s)`.
fn sentences(s: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut last = 0;
    for m in WS.find_iter(s) {
        if s[..m.start()].ends_with(['.', '?', '!']) {
            out.push(&s[last..m.start()]);
            last = m.end();
        }
    }
    out.push(&s[last..]);
    out
}

fn get_str<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
    }
}

/// Lint `src` (deck.src.html) against `brief` (parsed brief.json). Source refs resolve inside `deck`.
/// Whether `f` (one of BANNED_FONTS) trips the font check for these font-family values: named anywhere, and either the
/// first choice of a declaration or one of the always-flagged faces. Shared with the font picker's "Lint error" badge.
pub(crate) fn banned_font(f: &str, families: &[String]) -> bool {
    // (named anywhere, first choice) per banned face, compiled once: the picker checks the whole catalog.
    static RES: LazyLock<HashMap<&str, (Regex, Regex)>> = LazyLock::new(|| BANNED_FONTS.iter().map(|f| {
        let e = regex::escape(f);
        (*f, (Regex::new(&format!(r#"(^|[\s,'"|]){e}($|[\s,'"|;])"#)).unwrap(), Regex::new(&format!(r#"^\s*['"]?{e}\b"#)).unwrap()))
    }).collect());
    let Some((named, first)) = RES.get(f) else { return false };
    if !named.is_match(&families.join(" | ").to_lowercase()) {
        return false;
    }
    families.iter().any(|x| first.is_match(&x.to_lowercase())) || ["inter", "space grotesk", "poppins", "montserrat"].contains(&f)
}

pub fn lint(src: &str, brief: Option<&Value>, deck: &Deck) -> Report {
    let mut errors: Vec<Finding> = vec![];
    let mut warnings: Vec<Finding> = vec![];
    let mut verify = vec![];
    let err = |v: &mut Vec<Finding>, m: String, line: Option<u64>| v.push(Finding { message: m, line });
    let p = parse(src);
    // Python treats an empty brief dict as no brief.
    let brief = brief.filter(|b| truthy(Some(b)));

    if !(p.has_viewport && p.has_stage) {
        err(&mut errors, "structure: deck needs <div class=\"kx-viewport\"><div class=\"kx-stage\"> wrapping the slides".into(), None);
    }
    if p.slides.is_empty() {
        err(&mut errors, "structure: no <section class=\"slide\"> found".into(), None);
    }

    // fonts
    for f in BANNED_FONTS {
        if p.allow_fonts.contains(*f) { continue; }
        if banned_font(f, &p.fonts_css) {
            err(&mut errors, format!("font: '{f}' is a default-AI tell. Pick a distinctive face (references/design.md) or allow it via <meta name=\"kx-allow-fonts\" content=\"{f}\"> when a brand book requires it"), None);
        }
    }

    // ledger
    let mut claims: Map<String, Value> = Map::new();
    let mut srcs: Map<String, Value> = Map::new();
    let mut cache = HashMap::new();
    let mut unverified = BTreeSet::new();
    if let Some(b) = brief {
        let policy = get_str(b, "source_policy").unwrap_or("strict");
        for s in b.get("sources").and_then(Value::as_array).into_iter().flatten() {
            srcs.insert(get_str(s, "id").unwrap_or("").to_string(), s.clone());
        }
        for c in b.get("claims").and_then(Value::as_array).into_iter().flatten() {
            claims.insert(get_str(c, "id").unwrap_or("").to_string(), c.clone());
        }
        for (cid, c) in &claims {
            let sid = c.get("source").and_then(Value::as_str);
            let Some(s) = sid.and_then(|k| srcs.get(k)) else {
                let shown = match c.get("source") { Some(Value::String(x)) => x.clone(), None | Some(Value::Null) => "None".into(), Some(v) => v.to_string() };
                err(&mut errors, format!("ledger: claim {cid} cites unknown source '{shown}'"), None);
                continue;
            };
            let kind = get_str(s, "kind");
            if kind == Some("model") {
                if policy == "strict" {
                    err(&mut errors, format!("ledger: claim {cid} uses model knowledge but source_policy is strict"), None);
                } else {
                    verify.push(format!("{cid}: {}", get_str(c, "text").unwrap_or("None")));
                }
            }
            let r = get_str(s, "ref").unwrap_or("");
            let Some(q) = get_str(c, "quote").filter(|q| !q.is_empty()) else { continue };
            if matches!(kind, Some("user") | Some("model")) || URL_SCHEME.is_match(r) { continue; }
            let sid = get_str(s, "id").unwrap_or("");
            match source_text(deck, r, &mut cache) {
                None => { unverified.insert(format!("{sid} ({r})")); }
                Some(body) => {
                    let wq = ws(q);
                    let ub = unmark(&body);
                    if !body.contains(&wq) && !ub.contains(&unmark(&wq)) {
                        let words: Vec<&str> = wq.split(' ').collect();
                        let mut k = 0;
                        while k < words.len() && ub.contains(&unmark(&words[..=k].join(" "))) { k += 1; }
                        let hint = if k > 0 {
                            format!("; it matches up to \"{}\" and diverges at \"{}\"", words[..k].join(" "), words[k..(k + 6).min(words.len())].join(" "))
                        } else {
                            "; not even its first word matches, check the ref path".into()
                        };
                        err(&mut errors, format!("ledger: claim {cid} quote not found verbatim in {r}{hint}"), None);
                    }
                }
            }
        }
        for u in &unverified {
            err(&mut warnings, format!("ledger: quotes from source {u} were not verified (file missing or unreadable format). Use a path relative to brief.json or absolute, or extract the text to a .txt source"), None);
        }
    } else {
        err(&mut warnings, "ledger: no brief.json found, so slide content is not checked against sources".into(), None);
    }
    let claim_text = |c: &Value| norm_num(&format!("{} {}", get_str(c, "text").unwrap_or(""), get_str(c, "quote").unwrap_or("")));
    let ledger_text = claims.values().map(claim_text).collect::<Vec<_>>().join(" ");
    let ledger_nums: HashSet<&str> = ANY_NUMBER.find_iter(&ledger_text).map(|m| m.as_str()).collect();
    let claim_nums: HashMap<&str, HashSet<String>> = claims.iter()
        .map(|(k, c)| (k.as_str(), ANY_NUMBER.find_iter(&claim_text(c)).map(|m| m.as_str().to_string()).collect()))
        .collect();

    let mut prev_layout: Option<&Option<String>> = None;
    let mut layouts: Vec<&Option<String>> = vec![];
    for (i, s) in p.slides.iter().enumerate() {
        let line = Some(s.line);
        let tag = format!("slide {}", i + 1) + &s.id.as_deref().filter(|x| !x.is_empty()).map(|x| format!(" #{x}")).unwrap_or_default();
        let visible = ws(&s.text.join(" "));
        let notes = ws(&s.notes.join(" "));
        let everything = format!("{visible} {notes} {}", s.title_attr.as_deref().unwrap_or(""));

        for m in DASHES.find_iter(&everything) {
            let ctx = context(&everything, m.start(), m.end(), 30, 30);
            err(&mut errors, format!("{tag}: dash character '{}' in \"{ctx}\". Rewrite as two sentences, a comma, or a colon", m.as_str()), line);
        }
        for m in SPACED_HYPHEN.find_iter(&everything) {
            let ctx = context(&everything, m.start(), m.end(), 30, 30);
            err(&mut errors, format!("{tag}: spaced hyphen used as a dash in \"{ctx}\""), line);
        }
        if EMOJI.is_match(&everything) {
            err(&mut errors, format!("{tag}: emoji in slide text. Use type, rules, or drawn marks instead"), line);
        }
        for cid in &s.claims {
            if brief.is_some() && !claims.contains_key(cid) {
                err(&mut errors, format!("{tag}: data-claims references unknown claim '{cid}'"), line);
            }
        }

        if brief.is_some() {
            let own: Option<HashSet<&str>> = (!s.claims.is_empty())
                .then(|| s.claims.iter().filter_map(|c| claim_nums.get(c.as_str())).flatten().map(String::as_str).collect());
            let text = format!("{visible} {notes}");
            for (start, n) in numbers(&text) {
                let k = norm_num(n);
                if CITATION.is_match(context(&text, start, start, 14, 0)) { continue; }
                if !ledger_nums.contains(k.as_str()) {
                    err(&mut errors, format!("{tag}: number '{n}' is not in any claim in brief.json. Add a sourced claim, or mark decorative numerals (folios, step markers) with data-deco"), line);
                } else if own.as_ref().is_some_and(|o| !o.contains(k.as_str())) {
                    err(&mut warnings, format!("{tag}: number '{n}' is in the ledger but not in this slide's data-claims. Cite the claim it comes from"), line);
                }
            }
        }

        let layout = s.layout.as_ref().filter(|l| !l.is_empty());
        if layout.is_none() {
            err(&mut errors, format!("{tag}: missing data-layout (needed for the variety check)"), line);
        } else if prev_layout.is_some_and(|pl| pl.as_ref() == s.layout.as_ref()) {
            err(&mut warnings, format!("{tag}: same data-layout '{}' as the previous slide", s.layout.as_deref().unwrap()), line);
        }
        prev_layout = Some(&s.layout);
        layouts.push(&s.layout);

        let title = ws(s.headings.first().map(String::as_str).unwrap_or(s.title_attr.as_deref().unwrap_or("")));
        if GENERIC_TITLES.contains(&title.to_lowercase().trim_matches([' ', '.', '!', ':'])) {
            err(&mut warnings, format!("{tag}: generic title '{title}'. Say the point instead"), line);
        }
        let low = format!("{visible} {notes}").to_lowercase();
        for ph in BANNED_PHRASES {
            if Regex::new(&format!(r"\b{}\b", regex::escape(ph))).unwrap().is_match(&low) {
                err(&mut warnings, format!("{tag}: stock phrase '{ph}'"), line);
            }
        }
        if NOT_X_BUT_Y.is_match(&format!("{visible} {notes}")) {
            err(&mut warnings, format!("{tag}: 'not X but Y' construction. State the point directly"), line);
        }
        if visible.contains('!') {
            err(&mut warnings, format!("{tag}: exclamation mark on a slide"), line);
        }
        let raw = s.text.iter().chain(&s.notes).map(String::as_str).collect::<Vec<_>>().join("\n");
        for chunk in raw.split('\n') {
            let c = ws(chunk);
            for sent in sentences(&c) {
                let n = sent.split_whitespace().count();
                if n > 24 {
                    err(&mut warnings, format!("{tag}: long sentence ({n} words): \"{}...\"", first_chars(sent, 70)), line);
                }
            }
        }
        if !s.has_notes {
            err(&mut warnings, format!("{tag}: no <aside class=\"notes\"> speaker notes"), line);
        }
        if s.anims > 9 {
            err(&mut warnings, format!("{tag}: {} animated elements. Choreograph fewer, larger moves", s.anims), line);
        }
    }

    let n = p.slides.len();
    let distinct: HashSet<_> = layouts.iter().collect();
    if n >= 6 && distinct.len() < 4.min(n / 3 + 1) {
        err(&mut warnings, format!("deck: only {} distinct layouts across {n} slides", distinct.len()), None);
    }

    let body = BODY_TAG.find(src).map_or("", |m| m.as_str());
    let default: u32 = DATA_MOTION.captures(body).map_or(2, |c| c[1].parse().unwrap());
    let lv = DATA_LEVELS.captures(body);
    let mut offered: Vec<u32> = lv.as_ref().map(|c| {
        let set: BTreeSet<u32> = LEVEL_SPLIT.split(&c[1]).filter(|x| !x.is_empty() && x.chars().all(|ch| ch.is_ascii_digit())).filter_map(|x| x.parse().ok()).collect();
        set.into_iter().collect()
    }).unwrap_or_default();
    match &lv {
        None => err(&mut warnings, "deck: <body> has no data-levels. Offer only the motion levels the user chose, for example data-levels=\"0 2 4\"".into(), None),
        Some(c) if offered.is_empty() || *offered.last().unwrap() > 4 => err(&mut errors, format!("deck: data-levels=\"{}\" must list levels 0 to 4", &c[1]), None),
        Some(_) if !offered.contains(&default) => err(&mut errors, format!("deck: default data-motion={default} is not one of the offered data-levels {offered:?}"), None),
        _ => {}
    }
    offered.push(default);
    let level = *offered.iter().max().unwrap();
    let authored: usize = ["KX_CUT", "KX_ENTER", "data-move=", "data-keep="].iter().map(|k| src.matches(k).count()).sum();
    let concept = brief.and_then(|b| b.get("motion")).and_then(|m| m.get("concept"));
    if level >= 2 && !truthy(concept) {
        err(&mut warnings, "deck: brief.json has no \"motion\" concept. Design the motion for this subject (references/motion.md)".into(), None);
    }
    if level >= 3 && authored < 3 {
        err(&mut errors, "deck: motion is engine presets only. When Cinematic or Insane is offered, author this deck's own cut and hero moves (KX_CUT, data-move, data-keep, KX_ENTER) from its motion concept (references/motion.md)".into(), None);
    } else if level == 2 && authored == 0 {
        err(&mut warnings, "deck: motion is engine presets only. Give the hero slides authored moves (data-move, data-keep)".into(), None);
    }

    Report { slides: n, errors, warnings, verify }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage_file::{BRIEF, SRC};

    /// Same layout as `build.py --lint-only` prints.
    fn render(r: &Report) -> String {
        let mut s = format!("stagecraft build: {} slides, {} errors, {} warnings\n", r.slides, r.errors.len(), r.warnings.len());
        for e in &r.errors { s += &format!("  ERROR  {}\n", e.message); }
        for w in &r.warnings { s += &format!("  warn   {}\n", w.message); }
        if !r.verify.is_empty() {
            s += "  Claims from model knowledge (tell the user to verify):\n";
            for v in &r.verify { s += &format!("    - {v}\n"); }
        }
        s
    }

    fn lint_deck(d: &Deck) -> Report {
        let brief: Option<Value> = d.text(BRIEF).map(|t| serde_json::from_str(&t).unwrap());
        lint(&d.text(SRC).unwrap(), brief.as_ref(), d)
    }

    #[test]
    fn example_deck_is_clean() {
        let r = lint_deck(&Deck::example());
        assert_eq!(render(&r), "stagecraft build: 6 slides, 0 errors, 0 warnings\n");
    }

    /// expected.txt is the output of `python build.py deck.src.html --lint-only` in the fixture folder.
    #[test]
    fn bad_deck_matches_build_py() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bad");
        let mut d = Deck::default();
        for f in [SRC, BRIEF, "sources/facts.txt"] {
            d.set(f, std::fs::read(dir.join(f)).unwrap());
        }
        let expected = std::fs::read_to_string(dir.join("expected.txt")).unwrap().replace("\r\n", "\n");
        assert_eq!(render(&lint_deck(&d)), expected);
    }

    #[test]
    fn number_scan_matches_python_lookbehind() {
        let got: Vec<&str> = numbers("H2O v1.2 a1,5 is 1,920 by 1080 #3 x/4 .5").into_iter().map(|(_, n)| n).collect();
        // Python: re.findall(r"(?<![A-Za-z0-9\-_/#.])\d+(?:[.,]\d+)*", ...) -> ['5', '1,920', '1080']
        assert_eq!(got, ["5", "1,920", "1080"]);
    }

    #[test]
    fn unmark_needs_trailing_space() {
        assert_eq!(unmark("a // b #x c # d"), "a b #x c d");
    }

    #[test]
    fn sentences_split_after_punctuation() {
        assert_eq!(sentences("One. Two? Three four"), ["One.", "Two?", "Three four"]);
    }
}
