mod build;
mod edit;
pub mod fonts;
mod stage_file;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::http::{Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};

use build::assemble::{assemble, GSAP_FILES, VENDOR_PREFIX};
use build::lint::{lint, Finding, Report};
use stage_file::{Deck, BRIEF, SRC};

const VENDOR: &[&[u8]] = &[
    include_bytes!("../../engine/vendor/gsap.min.js"),
    include_bytes!("../../engine/vendor/SplitText.min.js"),
    include_bytes!("../../engine/vendor/ScrambleTextPlugin.min.js"),
];

#[derive(Default)]
struct App {
    deck: Option<Deck>,
    /// Assembled deck, only when lint has no errors.
    built: Option<String>,
    fonts: fonts::Store,
}

/// Full-library download: one at a time, cancellable between families.
#[derive(Default)]
struct Library {
    running: AtomicBool,
    cancel: AtomicBool,
    /// Failures of the last run, until the next one finishes.
    failed: AtomicUsize,
}

#[derive(Serialize, Clone)]
struct LibraryStatus {
    installed: usize,
    total: usize,
    failed: usize,
    running: bool,
}

#[derive(Serialize)]
struct Opened {
    path: String,
    src: String,
    brief: String,
    report: Report,
}

/// Lint the deck and assemble it when clean.
fn rebuild(app: &mut App) -> Report {
    let deck = app.deck.as_ref().expect("rebuild without a deck");
    let (src, brief) = (deck.text(SRC).unwrap_or_default(), deck.text(BRIEF).unwrap_or_default());
    rebuild_from(app, &src, &brief)
}

/// Lint and assemble the given text against the open deck's assets, without touching the deck. An empty `brief` means
/// none; invalid brief JSON is reported as an error rather than failing the call.
fn rebuild_from(app: &mut App, src: &str, brief: &str) -> Report {
    let deck = app.deck.as_ref().expect("rebuild without a deck");
    let (brief, brief_err) = if brief.trim().is_empty() {
        (None, None)
    } else {
        match serde_json::from_str(brief) {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(Finding { message: format!("brief.json: invalid JSON: {e}"), line: Some(e.line() as u64) })),
        }
    };
    let mut report = lint(src, brief.as_ref(), deck);
    report.errors.extend(brief_err);
    app.built = None;
    if report.errors.is_empty() {
        match assemble(&edit::tags::stamp(src), brief.as_ref()) {
            Ok(html) => app.built = Some(html),
            Err(e) => report.errors.push(Finding { message: e, line: None }),
        }
    }
    report
}

fn load(app: &mut App, deck: Deck) -> Opened {
    app.deck = Some(deck);
    let report = rebuild(app);
    let d = app.deck.as_ref().unwrap();
    Opened {
        path: d.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        src: d.text(SRC).unwrap_or_default(),
        brief: d.text(BRIEF).unwrap_or_default(),
        report,
    }
}

#[tauri::command]
fn startup_path() -> Option<String> {
    std::env::args().skip(1).find(|a| a.to_lowercase().ends_with(".stage"))
}

#[tauri::command]
fn open_deck(path: String, state: State<Mutex<App>>) -> Result<Opened, String> {
    let deck = Deck::open(&PathBuf::from(path))?;
    Ok(load(&mut state.lock().unwrap(), deck))
}

#[tauri::command]
fn new_deck(path: String, state: State<Mutex<App>>) -> Result<Opened, String> {
    let mut deck = Deck::example();
    deck.save_as(&PathBuf::from(path))?;
    Ok(load(&mut state.lock().unwrap(), deck))
}

#[tauri::command]
fn save_deck(src: String, brief: String, state: State<Mutex<App>>) -> Result<Report, String> {
    let mut app = state.lock().unwrap();
    let App { deck, fonts: store, .. } = &mut *app;
    let deck = deck.as_mut().ok_or("no deck open")?;
    let path = deck.path.clone().ok_or("deck has no file path")?;
    fonts::sync_embedded(deck, store, &src);
    deck.set(SRC, src.into_bytes());
    deck.set(BRIEF, brief.into_bytes());
    deck.save_as(&path)?;
    Ok(rebuild(&mut app))
}

/// The store, cloned out of the lock: catalog work stats ~2000 folders and must not block the stage protocol.
fn store(app: &AppHandle) -> fonts::Store {
    app.state::<Mutex<App>>().lock().unwrap().fonts.clone()
}

#[tauri::command]
async fn font_catalog(app: AppHandle) -> Result<Vec<fonts::CatalogEntry>, String> {
    let store = store(&app);
    tauri::async_runtime::spawn_blocking(move || store.catalog()).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn library_status(app: AppHandle) -> Result<LibraryStatus, String> {
    let store = store(&app);
    let lib = app.state::<Arc<Library>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cat = store.catalog();
        LibraryStatus {
            installed: cat.iter().filter(|c| c.installed).count(), total: cat.len(),
            failed: lib.failed.load(Ordering::SeqCst), running: lib.running.load(Ordering::SeqCst),
        }
    }).await.map_err(|e| e.to_string())
}

/// Download `id` unless installed. Only catalog ids: the id becomes a folder name and a URL path.
async fn ensure_font(app: &AppHandle, id: &str) -> Result<(), String> {
    if !fonts::in_catalog(id) { return Err(format!("'{id}' is not a Google font")); }
    let store = store(app);
    if store.manifest(id).is_some() { return Ok(()); }
    let root = store.root;
    let id = id.to_string();
    tauri::async_runtime::spawn_blocking(move || fonts::fetch::download_family(&fonts::fetch::agent(), &id, &root).map(|_| ()))
        .await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn install_font(app: AppHandle, id: String) -> Result<(), String> {
    ensure_font(&app, &id).await
}

/// Embed `id` in the open deck (saved with it on Ctrl+S) and return the source pointed at it for `role`.
#[tauri::command]
async fn apply_font(app: AppHandle, src: String, role: String, id: String) -> Result<String, String> {
    ensure_font(&app, &id).await?;
    let state = app.state::<Mutex<App>>();
    let mut a = state.lock().unwrap();
    let App { deck, fonts: store, .. } = &mut *a;
    let m = fonts::embed(deck.as_mut().ok_or("no deck open")?, store, &id)?;
    fonts::css::apply_font(&src, &role, &m)
}

/// Lint and assemble unsaved editor text for the preview; the deck itself is unchanged.
#[tauri::command]
fn preview_src(src: String, brief: String, state: State<Mutex<App>>) -> Result<Report, String> {
    let mut app = state.lock().unwrap();
    if app.deck.is_none() { return Err("no deck open".into()); }
    Ok(rebuild_from(&mut app, &src, &brief))
}

#[tauri::command]
fn element_info(src: String, index: usize) -> Option<edit::style::ElementInfo> {
    edit::style::element_info(&src, index)
}

#[derive(Serialize)]
struct Styled {
    src: String,
    /// The element's index in `src`; creating a <style> block in <head> shifts it.
    index: usize,
}

/// Where element `n` of `old` is in `new`: tags inserted by an edit all land in <head>, before any element.
fn shifted(old: &str, new: &str, n: usize) -> usize {
    let (o, w) = (edit::tags::start_tags(old).len(), edit::tags::start_tags(new).len());
    n.saturating_add(w.saturating_sub(o))
}

/// Style one element. An empty value clears that property. With `font`, it is installed and embedded in the open deck first.
#[tauri::command]
async fn style_element(app: AppHandle, src: String, index: usize, mut props: edit::style::Props, font: Option<String>) -> Result<Styled, String> {
    props.retain(|_, v| !v.trim().is_empty());
    if let Some(id) = &font { ensure_font(&app, id).await?; }
    let manifest = match &font {
        Some(id) => {
            let state = app.state::<Mutex<App>>();
            let mut a = state.lock().unwrap();
            let App { deck, fonts: store, .. } = &mut *a;
            Some(fonts::embed(deck.as_mut().ok_or("no deck open")?, store, id)?)
        }
        None => None,
    };
    let out = edit::style::set_style(&src, index, &props, manifest.as_ref())?;
    let index = shifted(&src, &out, index);
    Ok(Styled { src: out, index })
}

#[tauri::command]
fn download_library(app: AppHandle, lib: State<Arc<Library>>) {
    if lib.running.swap(true, Ordering::SeqCst) { return; }
    lib.cancel.store(false, Ordering::SeqCst);
    let lib = lib.inner().clone();
    let store = store(&app);
    std::thread::spawn(move || {
        let (root, cat) = (store.root.clone(), store.catalog());
        let total = cat.len();
        let before = cat.iter().filter(|c| c.installed).count();
        let ids: Vec<String> = cat.into_iter().filter(|c| !c.installed).map(|c| c.id).collect();
        let agent = fonts::fetch::agent();
        let emit = |done: usize, failed: usize, running: bool| {
            let _ = app.emit("font-library", LibraryStatus { installed: before + done, total, failed, running });
        };
        let (done, failed) = fonts::download_all(&ids, 6, &lib.cancel,
            |id| fonts::fetch::download_family(&agent, id, &root).map(|_| ()), |d, f| emit(d, f, true));
        lib.failed.store(failed, Ordering::SeqCst);
        lib.running.store(false, Ordering::SeqCst);
        emit(done, failed, false);
    });
}

#[tauri::command]
fn cancel_library(lib: State<Arc<Library>>) {
    lib.cancel.store(true, Ordering::SeqCst);
}

fn mime(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    }
}

/// Added only when the app serves the deck (not part of the build output). Tells the editor which slide is showing and
/// forwards app shortcuts, since the deck iframe is cross-origin. Capture phase so it sees Escape before the engine closes overlays.
const BRIDGE: &str = concat!("<script>", include_str!("bridge.js"), "</script>");

/// Font previews for the editor's picker (a different origin, so these responses carry CORS).
const FONTS_PREFIX: &str = "_stage/fonts/";

/// `stage://localhost/index.html` is the assembled deck; bundled GSAP under `_stage/vendor/`; font previews under
/// `_stage/fonts/<id>/preview.woff2`; anything else is a zip entry.
fn serve(app: &App, path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let path = path.trim_start_matches('/');
    let path = percent_decode(path);
    if path.is_empty() || path == "index.html" {
        return app.built.as_ref().map(|h| {
            let i = h.to_ascii_lowercase().rfind("</body>").unwrap_or(h.len());
            (Cow::Owned(format!("{}{BRIDGE}{}", &h[..i], &h[i..]).into_bytes()), mime("index.html"))
        });
    }
    if let Some(f) = path.strip_prefix(VENDOR_PREFIX) {
        let i = GSAP_FILES.iter().position(|(n, _)| *n == f)?;
        return Some((Cow::Borrowed(VENDOR[i]), mime(f)));
    }
    if let Some(id) = path.strip_prefix(FONTS_PREFIX).and_then(|p| p.strip_suffix("/preview.woff2")) {
        let m = app.fonts.manifest(id)?;
        let f = m.files.iter().find(|f| f.style == "normal").or(m.files.first())?;
        return Some((Cow::Owned(app.fonts.file(id, &f.file)?), mime(&f.file)));
    }
    let bytes = app.deck.as_ref()?.get(&path)?.to_vec();
    Some((Cow::Owned(bytes), mime(&path)))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_deck_vendor_and_entries() {
        let mut app = App { fonts: fonts::Store::default(), ..App::default() };
        let mut deck = Deck::example();
        deck.set("assets/a b.png", vec![1, 2, 3]);
        load(&mut app, deck);

        let (html, ty) = serve(&app, "/index.html").expect("clean deck is served");
        let html = String::from_utf8(html.into_owned()).unwrap();
        assert!(ty.starts_with("text/html"));
        assert!(html.contains("window.gsap||document.write('<script src=\"_stage/vendor/gsap.min.js\">"));
        assert!(html.contains(BRIDGE) && html.contains("id=\"kx-engine\""));
        assert!(html.contains("data-kx-i="));
        assert!(html.contains("stopImmediatePropagation") && html.contains("'edit-off'"));

        assert_eq!(serve(&app, "/_stage/vendor/gsap.min.js").unwrap().0.len(), VENDOR[0].len());
        let preview = serve(&app, "/_stage/fonts/fraunces/preview.woff2").expect("bundled preview");
        assert_eq!(preview.0.as_ref(), fonts::Store::default().file("fraunces", "latin-full-normal.woff2").unwrap().as_slice());
        assert_eq!(preview.1, "font/woff2");
        assert!(serve(&app, "/_stage/fonts/..%2Fgeist/preview.woff2").is_none());
        assert!(serve(&app, "/_stage/fonts/not-a-font/preview.woff2").is_none());
        assert_eq!(serve(&app, "/assets/a%20b.png").unwrap().0.as_ref(), &[1, 2, 3]);
        assert!(serve(&app, "/nope.png").is_none());

        // Lint errors: no deck served, so the preview stays blank.
        app.deck.as_mut().unwrap().set(SRC, b"<html><head></head><body></body></html>".to_vec());
        assert!(!rebuild(&mut app).errors.is_empty());
        assert!(serve(&app, "/index.html").is_none());
    }

    #[test]
    fn rebuild_unsaved_does_not_touch_disk() {
        let path = std::env::temp_dir().join(format!("stage-unsaved-{}.stage", std::process::id()));
        let mut deck = Deck::example();
        deck.save_as(&path).unwrap();
        let on_disk = std::fs::read(&path).unwrap();
        let mut app = App::default();
        let opened = load(&mut app, deck);
        let served = |app: &App| serve(app, "/index.html").map(|(b, _)| String::from_utf8(b.into_owned()).unwrap());

        let i = opened.src.find("<h2").unwrap();
        let (a, b) = (i + opened.src[i..].find('>').unwrap() + 1, i + opened.src[i..].find("</h2>").unwrap());
        let changed = format!("{}Changed{}", &opened.src[..a], &opened.src[b..]);
        let r = rebuild_from(&mut app, &changed, &opened.brief);
        assert!(r.errors.is_empty(), "{:?}", r.errors.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert!(served(&app).unwrap().contains("Changed"));
        assert_eq!(std::fs::read(&path).unwrap(), on_disk);

        let broken = "<html><head></head><body></body></html>";
        assert!(!rebuild_from(&mut app, broken, &opened.brief).errors.is_empty());
        assert!(served(&app).is_none());

        assert!(rebuild_from(&mut app, &opened.src, &opened.brief).errors.is_empty());
        assert!(!served(&app).unwrap().contains("Changed"));
        assert!(rebuild_from(&mut app, &opened.src, "{nope").errors.iter().any(|e| e.message.contains("invalid JSON")));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn style_index_follows_inserted_style_block() {
        let old = Deck::example().text(SRC).unwrap();
        let n = edit::tags::start_tags(&old).iter().position(|t| t.name == "h2").unwrap();
        let props = edit::style::Props::from([("color".to_string(), "red".to_string())]);
        let new = edit::style::set_style(&old, n, &props, None).unwrap();
        let m = shifted(&old, &new, n);
        assert!(m > n);
        let t = edit::tags::nth(&new, m).unwrap();
        assert_eq!(t.name, "h2");
        assert!(new[t.start..t.end].contains("kx-s1"));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(App::default()))
        .manage(Arc::new(Library::default()))
        .setup(|app| {
            let root = app.path().app_data_dir()?.join("fonts");
            // Leftovers of downloads interrupted by a quit or crash.
            for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
                if e.file_name().to_string_lossy().contains(".part-") { let _ = std::fs::remove_dir_all(e.path()); }
            }
            app.state::<Mutex<App>>().lock().unwrap().fonts.root = root;
            // Built here instead of from config alone so the engine's window.open (presenter view, P key) gets a native
            // window; the webview blocks popups otherwise. The popup keeps its opener so the engine can drive its DOM.
            let handle = app.handle().clone();
            let opened = std::sync::atomic::AtomicUsize::new(0);
            let conf = app.config().app.windows.iter().find(|w| w.label == "main").expect("main window config").clone();
            tauri::WebviewWindowBuilder::from_config(app, &conf)?
                .on_new_window(move |_url, features| {
                    let n = opened.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    match tauri::WebviewWindowBuilder::new(&handle, format!("presenter-{n}"), tauri::WebviewUrl::External("about:blank".parse().unwrap()))
                        .window_features(features)
                        .title("Presenter")
                        .build()
                    {
                        Ok(window) => tauri::webview::NewWindowResponse::Create { window },
                        Err(_) => tauri::webview::NewWindowResponse::Deny,
                    }
                })
                .build()?;
            Ok(())
        })
        .register_uri_scheme_protocol("stage", |ctx, req| {
            let state = ctx.app_handle().state::<Mutex<App>>();
            let app = state.lock().unwrap();
            let mut res = Response::builder().header("Cache-Control", "no-store");
            if req.uri().path().trim_start_matches('/').starts_with(FONTS_PREFIX) {
                res = res.header("Access-Control-Allow-Origin", "*");
            }
            match serve(&app, req.uri().path()) {
                Some((body, ty)) => res.header("Content-Type", ty).body(body),
                None => res.status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&b"not found"[..])),
            }
            .unwrap()
        })
        .invoke_handler(tauri::generate_handler![startup_path, open_deck, new_deck, save_deck,
            font_catalog, library_status, install_font, apply_font, download_library, cancel_library,
            preview_src, element_info, style_element])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
