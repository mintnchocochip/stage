# Bundled Fonts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decks stop loading fonts from Google at show time. Stage ships 30 curated Google font families, offers the full Google catalog as an optional download, and embeds every font a deck uses inside its `.stage` file.

**Architecture:** Font files come from Fontsource (Google Fonts repackaged as woff2, served via jsDelivr). Each family is a folder holding its woff2 files, a `LICENSE` and a `family.json` manifest. Curated families are compiled into the binary with `include_bytes!`, the same way GSAP is. Downloaded ones live in the app data folder. Picking a font for a role copies its folder into the deck zip under `assets/fonts/<id>/` and rewrites `deck.src.html`. The rewrite adds `@font-face` rules to a managed `<style id="kx-fonts">` block and sets the role's `--font-*` variable. So the source stays the single truth and lint keeps working unchanged.

**Tech Stack:** Rust (Tauri v2, `ureq` 3 for HTTP, `serde_json`), Svelte 5, CodeMirror 6.

**Spec:** this conversation's decisions, restated here:
- 30-family curated set chosen by Claude (list below).
- Full catalog offered on first launch; the user can decline and download later.
- Banned-font lint stays an error.

## Global Constraints

- Leave all changes uncommitted. The user stages and commits; no Claude co-author line.
- Catalog = Fontsource entries with `"type": "google"` (1,984 families as of 2026-10-21).
- One subset per family: `latin` if the family has it, else its `defSubset`.
- Every request to `api.fontsource.org` sends `User-Agent: Stage/<CARGO_PKG_VERSION>`. Without it the API answers 403.
- Every embedded or bundled family carries its `LICENSE` (OFL requires the license to travel with the font).
- `@font-face` rules use `font-display: block` (no flash of fallback text mid-presentation).
- First-launch prompt copy states the size as "about 100 MB". That figure comes from 80 random families averaging 49 KB each.
- Curated set (Fontsource ids):
  - **Serif:** `fraunces`, `newsreader`, `source-serif-4`, `literata`, `instrument-serif`, `playfair-display`, `eb-garamond`, `young-serif`
  - **Sans:** `instrument-sans`, `ibm-plex-sans`, `manrope`, `dm-sans`, `schibsted-grotesk`, `figtree`, `hanken-grotesk`, `onest`, `geist`, `public-sans`, `bricolage-grotesque`, `archivo`
  - **Display:** `syne`, `unbounded`, `big-shoulders-display`, `bebas-neue`
  - **Mono:** `jetbrains-mono`, `ibm-plex-mono`, `geist-mono`, `dm-mono`, `space-mono`, `fira-code`
  - Measured total: 2.9 MB.

## Review Focus

1. **A file listed by the API returns 404 from the CDN.** This happens with the variable file for `playwrite-au-qld` and static `700 italic` for `trochut`. Expected: fall back from variable to static files, and skip individual missing static files. Fail the family only when zero files download. (Task 2 test `plan_falls_back_and_skips_missing`.)
2. **The download is interrupted (app quit, network drop), or `install_font` and the library download hit the same family.** Expected: no half-written family ever counts as installed, and a rerun resumes by skipping installed families. (Task 2 test `download_is_atomic`.)
3. **A font is picked, then undone in the editor, then the deck is saved.** Expected: its `assets/fonts/<id>/` files are pruned from the zip. (Task 4 test `prune_drops_unreferenced_families`.)
4. **A deck with embedded fonts is opened on a machine without the library.** Expected: it renders from the zip's own files. (Task 4 test `example_deck_embeds_its_fonts`.)
5. **A banned family is picked (Inter, Roboto, Montserrat…).** Expected:
   - lint reports the same error as today;
   - the picker shows a "Lint error" badge on that family but still allows it;
   - `<meta name="kx-allow-fonts">` still clears the error.

   (Task 1 test `banned_font_still_errors`.)

---

### Task 1: Manifest types and source rewriting

**Files:**
- Create: `src-tauri/src/fonts/mod.rs` (types only in this task), `src-tauri/src/fonts/css.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod fonts;`), `src-tauri/src/build/lint.rs:21` (make `BANNED_FONTS` `pub(crate)`)

**Interfaces:**
- Produces, in `fonts/mod.rs`:
  ```rust
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
  pub struct FontFile { pub file: String, pub style: String, pub weight: String } // weight "400" or "100 900"
  #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
  pub struct Manifest { pub id: String, pub family: String, pub category: String, pub files: Vec<FontFile> }
  pub const ROLES: &[&str] = &["display", "body", "mono"];
  ```
- Produces, in `fonts/css.rs`:
  - `pub fn font_face_css(m: &Manifest) -> String`: one rule per file, `src:url(assets/fonts/{id}/{file}) format('woff2')`, `font-display:block`.
  - `pub fn apply_font(src: &str, role: &str, m: &Manifest) -> Result<String, String>`
  - `pub fn fonts_in_use(src: &str) -> BTreeSet<String>`: the ids referenced as `assets/fonts/<id>/`.

`apply_font` rules:
- **The `kx-fonts` block.** Create it right after `<meta charset>` if missing (same insertion point `assemble` uses). If the family's `@font-face` rules are already in it, don't duplicate them.
- **The role variable.**
  - If the source declares `--font-<role>` anywhere, replace the value of the first declaration.
  - Otherwise add `:root{--font-<role>:…}` inside the block.
  - The value is `'<Family>', <fallback>`. The fallback is `monospace` for the mono role, otherwise by category: `serif`→`serif`, `handwriting`→`cursive`, anything else→`sans-serif`.
- **Unknown role:** return `Err`.

- [ ] **Step 1: Write failing tests in `css.rs`**
  - `apply_creates_block_and_replaces_var`: the example deck source, with `apply_font(src, "display", &newsreader)`. Then:
    - the result contains exactly one `<style id="kx-fonts">`;
    - it contains `--font-display: 'Newsreader', serif`;
    - it no longer contains `'Fraunces', serif;` on the `--font-display` line;
    - applying the same font again returns identical text.
  - `apply_adds_root_when_var_missing`: a source with no `--font-body` gets `:root{--font-body:'Geist', sans-serif}` inside `kx-fonts`.
  - `fonts_in_use_reads_urls`: returns `{"geist", "newsreader"}` for a source with both blocks.
  - `banned_font_still_errors`:
    - `lint` on a source after `apply_font(.., "body", &inter)` has an error starting with `font: 'inter'`;
    - adding `<meta name="kx-allow-fonts" content="inter">` removes it.
- [ ] **Step 2:** Run `cargo test fonts::css` in `src-tauri`. Expected: FAIL, functions not defined.
- [ ] **Step 3:** Implement the three functions. Use `regex` like the rest of the crate: `LazyLock<Regex>` statics, with the `re!` pattern in `lint.rs` as the style reference.
- [ ] **Step 4:** Run `cargo test fonts::css`. Expected: PASS.

### Task 2: Fontsource fetcher

**Files:**
- Create: `src-tauri/src/fonts/fetch.rs`
- Modify: `src-tauri/Cargo.toml`: add `ureq = { version = "3", features = ["json"] }`.

**Interfaces:**
- Consumes: `Manifest`, `FontFile` (Task 1).
- Produces:
  - `pub fn agent() -> ureq::Agent`, which sets the User-Agent.
  - `pub struct FamilyMeta`: serde of `GET https://api.fontsource.org/v1/fonts/<id>`. Fields: `id, family, category, subsets, defSubset, weights, styles, variable, variants`.
  - `pub struct Planned { pub url: String, pub file: String, pub style: String, pub weight: String }`
  - `pub fn plan_variable(meta: &FamilyMeta, axes: &serde_json::Map<String, Value>) -> Vec<Planned>`
  - `pub fn plan_static(meta: &FamilyMeta) -> Vec<Planned>`
  - `pub fn download_family(agent: &ureq::Agent, id: &str, root: &Path) -> Result<Manifest, String>`

URL rules (verified against the live CDN on 2026-10-21):
- **Variable families:**
  - Axes come from `GET https://api.fontsource.org/v1/variable/<id>` → `axes`, ignoring `ital`.
  - `kind` is `full` if any axis name has an uppercase letter (custom axes such as `SOFT`), `wght` if the only axis is `wght`, otherwise `standard`.
  - URL: `https://cdn.jsdelivr.net/fontsource/fonts/<id>:vf@latest/<subset>-<kind>-<style>.woff2`. File name: `<subset>-<kind>-<style>.woff2`.
  - Weight: `"<wght.min> <wght.max>"`, or `"400"` when there is no `wght` axis.
- **Static families:** for each weight×style, `variants[w][style][subset].url.woff2`. File name: `<subset>-<w>-<style>.woff2`.
- **License:** `https://cdn.jsdelivr.net/npm/@fontsource/<id>@latest/LICENSE`.

`download_family` behavior:
- **Write:** into `root/<id>.part-<thread id>/` (files, `LICENSE`, `family.json`), then `rename` to `root/<id>/`.
- **Already installed:** if `root/<id>/family.json` exists before starting, return it. If the rename fails because the target exists, delete the part dir and return the existing manifest.
- **Missing files:** if any variable file 404s, discard the variable plan and use `plan_static`. Skip static files that 404. Zero files → `Err("<id>: no font files available")`.

- [ ] **Step 1: Write failing tests** (pure, no network)
  - `kind_rule`:
    - Fraunces axes (`opsz, wght, SOFT, WONK`) → files `latin-full-normal.woff2`, `latin-full-italic.woff2`, weight `"100 900"`;
    - Newsreader (`opsz, wght`) → `standard`;
    - Playfair Display (`wght`) → `wght`.
  - `subset_rule`: meta with `subsets: ["tamil"], defSubset: "tamil"` plans `tamil-…` files.
  - `plan_falls_back_and_skips_missing`: download against a local `std::net::TcpListener` stub that 404s the vf URL and one static URL. The manifest lists the remaining static files. Put the base URLs behind a `const`, overridable in tests through a `#[cfg(test)]` parameter.
  - `download_is_atomic`:
    - a stub that drops the connection mid-file leaves no `root/<id>/` and returns `Err`;
    - a second call against a healthy stub succeeds;
    - a third call returns early without any requests.
- [ ] **Step 2:** Run `cargo test fonts::fetch`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test fonts::fetch`. Expected: PASS. Also add one `#[ignore]` network test, `live_fraunces`, that downloads into a temp dir and checks there are 2 files plus `LICENSE`. Run it once with `cargo test live_fraunces -- --ignored`.

### Task 3: Bundle generator and the curated set

**Files:**
- Create: `src-tauri/src/bin/fetch_fonts.rs`, plus its generated outputs: `engine/fonts/<id>/…` (30 families), `engine/fonts/catalog.json`, `src-tauri/src/fonts/bundled.rs`
- Modify: `src-tauri/Cargo.toml`: `default-run = "stage"` under `[package]`.

**Interfaces:**
- Consumes: `fetch::agent`, `fetch::download_family` (Task 2). For that, make `fonts` a `pub mod` in `lib.rs` so the bin can `use stage_lib::fonts`.
- Produces:
  - `engine/fonts/catalog.json`: `[{"id","family","category"}]` for google-type entries, sorted by family (about 185 KB).
  - `src-tauri/src/fonts/bundled.rs`: `pub const BUNDLED: &[(&str, &[u8])] = &[("fraunces/family.json", include_bytes!("../../../engine/fonts/fraunces/family.json")), …];`. It lists every file in every curated family folder and carries a header comment saying it is generated by `cargo run --bin fetch-fonts`.

- [ ] **Step 1:** Write the bin. The curated id list is a `const` copied from Global Constraints. The bin rebuilds `engine/fonts/` from scratch (delete, then download), writes `catalog.json` from `GET /v1/fonts`, and writes `bundled.rs`.
- [ ] **Step 2:** Run `cargo run --bin fetch-fonts` in `src-tauri`. Expected:
  - it prints 30 families;
  - `du -sh engine/fonts` reports about 3 MB;
  - `catalog.json` has 1984 entries.
- [ ] **Step 3:** Run `cargo build`. Expected: compiles with `bundled.rs` included from `fonts/mod.rs`.

### Task 4: Font store, embedding, serving, example deck

**Files:**
- Modify: `src-tauri/src/fonts/mod.rs`, `src-tauri/src/stage_file.rs`, `src-tauri/src/lib.rs` (`serve`), `engine/example/deck.src.html:7-9` (replace the three Google `<link>` tags with the `kx-fonts` block for `fraunces`, `instrument-sans`, `jetbrains-mono`, generated with `apply_font`)

**Interfaces:**
- Consumes: Task 1 types and css functions, `BUNDLED` (Task 3).
- Produces:
  - `#[derive(Serialize)] pub struct CatalogEntry { pub id: String, pub family: String, pub category: String, pub installed: bool, pub bundled: bool, pub blocked: bool }`. `blocked` = `BANNED_FONTS` contains the lowercased family.
  - `pub struct Store { pub root: PathBuf }` with:
    - `fn manifest(&self, id: &str) -> Option<Manifest>`: bundled first, then `root/<id>/family.json`;
    - `fn file(&self, id: &str, file: &str) -> Option<Vec<u8>>`;
    - `fn catalog(&self) -> Vec<CatalogEntry>`;
    - `fn installed_count(&self) -> usize`.
  - `pub fn embed(deck: &mut Deck, store: &Store, id: &str) -> Result<Manifest, String>`: copies every file plus `LICENSE` to `assets/fonts/<id>/`.
  - `pub fn prune(deck: &mut Deck, src: &str)`: removes `assets/fonts/<id>/*` for ids not in `fonts_in_use(src)`.
  - `Deck::retain(&mut self, keep: impl Fn(&str) -> bool)` in `stage_file.rs`.
  - `Deck::example()` now also embeds its three fonts from `BUNDLED`.
  - `serve(app: &App, store: &Store, path: &str)`:
    - serves `_stage/fonts/<id>/preview.woff2`: the family's first `normal` file, else its first file;
    - the protocol handler adds `Access-Control-Allow-Origin: *` only for paths under `_stage/fonts/`. The editor window is a different origin, and fonts need CORS.

- [ ] **Step 1: Write failing tests**
  - `example_deck_embeds_its_fonts`:
    - `Deck::example()` source has no `fonts.googleapis.com`;
    - every `url(assets/fonts/…)` in it resolves to a deck entry;
    - each of the 3 folders has a `LICENSE`;
    - `example_deck_is_clean` in `lint.rs` still passes.
  - `prune_drops_unreferenced_families`: embed `geist` into the example deck, then `prune` with the original source. No `assets/fonts/geist/` entries remain and the three example families stay.
  - `catalog_flags`: `catalog()` marks `fraunces` bundled+installed, `inter` blocked and not installed, and has 1984 entries.
  - In the existing `serves_deck_vendor_and_entries`, also assert that `/_stage/fonts/fraunces/preview.woff2` returns the bytes of `latin-full-normal.woff2`.
- [ ] **Step 2:** Run `cargo test`. Expected: the new tests FAIL.
- [ ] **Step 3:** Implement, and edit the example deck.
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS, including the lint parity tests.

### Task 5: Commands and the library download

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 2 and 4.
- Produces these Tauri commands. All are `async` and do blocking work in `tauri::async_runtime::spawn_blocking`.
  - `font_catalog() -> Vec<CatalogEntry>`
  - `library_status() -> LibraryStatus { installed: usize, total: usize, running: bool }`
  - `install_font(id: String) -> Result<(), String>`
  - `apply_font(src: String, role: String, id: String) -> Result<String, String>`:
    - installs the font if needed;
    - `embed`s it into the open deck in memory (saved on Ctrl+S);
    - returns the rewritten source.
  - `download_library()`: returns immediately and is a no-op if already running.
    - Six worker threads share an index over the not-yet-installed catalog ids.
    - After each family it emits a `font-library` event: `{ done, total, failed, running }`.
    - It checks a cancel `AtomicBool` between families.
  - `cancel_library()`
  - `save_deck` calls `fonts::prune(deck, &src)` before `save_as`.
- `Store` is managed state, created in `setup` with root `app.path().app_data_dir()?.join("fonts")`.

- [ ] **Step 1:** Implement and register the commands in `generate_handler!`.
- [ ] **Step 2:** Run `cargo test && cargo clippy`. Expected: PASS, no new warnings.

### Task 6: Fonts tab and first-launch prompt

**Files:**
- Create: `src/Fonts.svelte`
- Modify: `src/App.svelte`, `src/Editor.svelte`

**Interfaces:**
- Consumes: the Task 5 commands, the `font-library` event (`listen` from `@tauri-apps/api/event`), and `BASE` from `App.svelte` (pass it as a prop).
- Produces:
  - `Editor.replaceAll(text: string)`: one CodeMirror transaction over the whole document, so Ctrl+Z undoes a font change.
  - `<Fonts src={src} base={BASE} onapply={(next) => …} />`

UI:
- **Tab:** `"fonts"` joins the `tab` union. The tab bar shows `deck.src.html · brief.json | Fonts`, with Fonts in the UI font, not mono, after a 1px separator. It's hidden while no deck is open.
- **Role rows:** Display, Body, Mono. Each shows the current family, read from `src` with `/--font-(display|body|mono)\s*:\s*['"]?([^'",;]+)/g`, set in that typeface. Clicking a row opens the picker for that role.
- **Picker:**
  - A search box plus category chips: All, Serif, Sans, Display, Handwriting, Mono. These map to the Fontsource categories `serif`, `sans-serif`, `display`, `handwriting`, `monospace`.
  - Rows show the family name. When the font is installed, the name is drawn in its own face: register a `FontFace('pv-<id>', url(<base>_stage/fonts/<id>/preview.woff2))` when the row first scrolls into view (`IntersectionObserver`). Rows use `content-visibility: auto`.
  - Uninstalled rows show a small download glyph. Blocked rows show a "Lint error" badge.
  - Clicking a row calls `apply_font` (with a row spinner while it installs), then `srcEditor.replaceAll(next)`, and marks the deck dirty.
- **Footer:**
  - Idle: `30 of 1,984 families installed · Download all (about 100 MB)`.
  - While running: a progress bar plus `412 of 1,984 · Pause`. Pause calls `cancel_library`; Download resumes.
  - Show the failed count when it isn't 0.
- **First-launch strip:**
  - Shown above `main` when `localStorage["stage.fontLibrary"]` is unset and `installed < total`. Copy: `Download the full Google Fonts library (about 100 MB)? You can do this later from the Fonts tab.` with buttons `Download` and `Not now`.
  - Either button sets the key, and Download starts `download_library`.
  - Wrap localStorage access in try/catch.
- **Style:** use the existing CSS variables in `App.svelte`. No new colors.

- [ ] **Step 1:** Implement.
- [ ] **Step 2:** Run `npm run check`. Expected: 0 errors, 0 warnings.
- [ ] **Step 3:** Run `npm run tauri dev`. Then:
  - New deck: the preview renders in Fraunces with networking off.
  - Fonts → Display → pick Newsreader → Ctrl+S: the preview changes and the `.stage` file contains `assets/fonts/newsreader/`.
  - Ctrl+Z → Ctrl+S: the folder is gone.
  - Pick Inter for Body: a lint error appears and Present is disabled.
  - Clear `stage.fontLibrary` and restart: the strip appears. Download, then Pause, then Download again: the count resumes from where it stopped.
