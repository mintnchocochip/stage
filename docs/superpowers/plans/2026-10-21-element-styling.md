# Element Styling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the editor's preview, the user turns on Edit, clicks any element on a slide, and changes its font, size, color, weight, italic, underline, letter spacing, line height and alignment. The preview updates live and every change is an undoable edit of `deck.src.html`.

**Architecture:**
- **Selection.** When the deck is built for the preview, every start tag in the source gets `data-kx-i="<N>"`, where N is the tag's ordinal from one scanner (`edit/tags.rs`). An edit bridge in the preview iframe reports the clicked element's N. Rust finds the Nth start tag in the unstamped source with the same scanner, so both sides always agree.
- **Writing styles.** Each styled element gets a class `kx-sN`. Its rule lives in a managed `<style id="kx-styles">` block, so lint keeps seeing fonts. Fonts reuse the bundled-fonts store, embedding and `@font-face` code.
- **Live preview.** A new command rebuilds from unsaved text, and the iframe runs at `?motion=0` while editing.

**Tech Stack:** Rust (Tauri v2, `regex`), Svelte 5, CodeMirror 6, plain JS for the bridge.

**Spec:** this conversation's decisions, restated here:
- Styles go in a managed class block, not inline.
- Whole-element selection only for now; text ranges come later.
- Live preview at Still motion while editing.
- The animations tab and imports are a separate plan.

## Global Constraints

- Leave all changes uncommitted. The user stages and commits; no Claude co-author line.
- Styleable properties and nothing else: `font-family`, `font-size`, `color`, `font-weight`, `font-style`, `text-decoration-line`, `letter-spacing`, `line-height`, `text-align`.
- Every rule declaration ends with `!important`. Deck rules such as `.s-cover .cover-title { font-size: 300px }` out-rank a single class, and the user's explicit choice must win.
- Values are rejected (`Err`, nothing written) if they contain any of `; { } < > \ "` or a newline. That stops CSS or HTML injection through the managed block.
- Class names are `kx-s<N>`, where N is 1 + the highest `kx-s` number already in the source. A restyled element keeps its class.
- `data-kx-i` exists only in the served preview (`App.built`). It never reaches the saved `deck.src.html`.
- Selection is limited to elements inside `.slide`, the slide `<section>` included. Scaling the stage is the engine's job: overlay boxes use `getBoundingClientRect` in viewport space.

## Review Focus

1. **Markup that fools a naive tag scanner:**
   - `<` inside comments, `<script>`, `<style>`, `<textarea>` or `<title>`;
   - `>` inside quoted attribute values;
   - `<!doctype>`, closing tags, self-closing `<br/>`, uppercase tag names.

   Expected: the scanner yields exactly the real start tags, in order, and stamping and restyling hit the element the user clicked. (Task 1 test `start_tags_skip_non_tags`.)
2. **The user types in the code editor while an element is selected, adding or removing tags before it.** Expected: the selection clears instead of silently pointing at a different element. (Task 5: the selection clears on any source change that did not come from the Style panel.)
3. **Font-related lint holes:**
   - a blocked font chosen in the Style panel must raise the same lint error as anywhere else;
   - after it is replaced, its `@font-face` rules must go, or the error sticks (the bundled-fonts review bug).

   (Task 2 tests `style_font_is_linted` and `restyle_font_drops_old_faces`.)
4. **Presenting (F5) while Edit is on.** Expected: edit mode switches off, the deck plays at full motion, and clicks reach the engine again. (Task 5 manual check.)
5. **Live rebuilds that produce lint errors mid-typing.** Expected: the preview shows the existing "Preview paused" error list rather than a stale or broken frame, and recovers on the next clean rebuild. (Task 3 test `rebuild_unsaved_does_not_touch_disk`, which covers both directions.)

---

### Task 1: Start-tag scanner and preview stamping

**Files:**
- Create: `src-tauri/src/edit/mod.rs` (`pub mod tags; pub mod style;`), `src-tauri/src/edit/tags.rs`
- Modify: `src-tauri/src/lib.rs` (`mod edit;`; in `rebuild`, assemble `edit::tags::stamp(&src)` instead of `src`, while lint still runs on `src`)

**Interfaces:**
- Produces, in `edit/tags.rs`:
  ```rust
  pub struct Tag { pub start: usize, pub name_end: usize, pub end: usize, pub name: String } // byte offsets: '<', end of name, after '>'; name lowercased
  pub fn start_tags(src: &str) -> Vec<Tag>
  pub fn stamp(src: &str) -> String                     // inserts ` data-kx-i="N"` at each tag's name_end
  pub fn nth(src: &str, n: usize) -> Option<Tag>
  pub fn line_of(src: &str, offset: usize) -> u64        // 1-based
  ```

Scanner rules:
- A start tag is `<` followed by an ASCII letter.
- Skip `<!--…-->`, `<!…>`, `<?…>` and `</…>`.
- Inside a tag, `>` inside a `"` or `'` quoted value doesn't end it.
- After `script`, `style`, `textarea`, `title` or `template`, skip ahead to its case-insensitive closing tag. `template` content is inert in the DOM, so it must not be numbered.

- [ ] **Step 1: Write failing tests**
  - `start_tags_skip_non_tags`: the source below yields names `["html","head","title","style","body","section","h2","br","img","script","div","p"]`. `<p>` comes once, from the body, not from the comment.
    ```html
    <!doctype html><html><head><title>a<b</title><style>a>b{}</style></head>
    <!-- <p>no</p> --><body><section class="slide"><h2 title="x>y">Hi</h2><br/>
    <img src=a.png><script>if (a<b) document.write("<div>")</script><DIV></DIV><p>t</p></section></body></html>
    ```
  - `stamp_numbers_in_order`: `stamp` of that source contains `<h2 data-kx-i="6" title="x>y">` and `<DIV data-kx-i="10">`. `nth(src, 6).unwrap().name == "h2"`.
  - `stamped_example_still_lints_clean`: `lint(&stamp(&example_src), …)` has 0 errors. This proves the attribute is harmless.
- [ ] **Step 2:** Run `cargo test edit::tags` in `src-tauri`. Expected: FAIL, not defined.
- [ ] **Step 3:** Implement, and wire `stamp` into `rebuild`.
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS. The existing `serves_deck_vendor_and_entries` should now also assert that the served HTML contains `data-kx-i=`.

### Task 2: Managed style block

**Files:**
- Create: `src-tauri/src/edit/style.rs`
- Modify: `src-tauri/src/fonts/css.rs`:
  - extract the "ensure `kx-fonts` block + family's `@font-face` rules" half of `apply_font` into `pub fn ensure_faces(src: &str, m: &Manifest) -> Result<String, String>`, and have `apply_font` call it;
  - make `drop_unused_rules` `pub(crate)`.

**Interfaces:**
- Consumes: `tags::nth`, `css::ensure_faces`, `css::drop_unused_rules`, `fonts::Manifest`.
- Produces, in `edit/style.rs`:
  ```rust
  pub const PROPS: &[&str] = &["font-family","font-size","color","font-weight","font-style","text-decoration-line","letter-spacing","line-height","text-align"];
  pub type Props = BTreeMap<String, String>;
  #[derive(Serialize)] pub struct ElementInfo { pub tag: String, pub line: u64, pub class: Option<String>, pub props: Props }
  pub fn element_info(src: &str, n: usize) -> Option<ElementInfo>
  pub fn set_style(src: &str, n: usize, props: &Props, font: Option<&Manifest>) -> Result<String, String>
  ```

`set_style` rules:
- **Validation:** reject any key not in `PROPS` and any value that breaks the Global Constraints value rule.
- **`font`:** when `Some(m)`, call `ensure_faces` and set `font-family` to `'<family>', <fallback>`, with the same category fallback `apply_font` uses.
- **No properties left:** if `props` ends up empty, remove `kx-sN` from the element's class (and the attribute itself if it becomes empty), and remove the rule.
- **Otherwise:**
  - Add or keep the class. The class attribute may be double-quoted, single-quoted, unquoted or missing; write it back double-quoted.
  - Write the rule as one line, `.kx-sN{prop:value !important;…}`, with props in `PROPS` order.
  - The rule goes inside `<style id="kx-styles">`, created just before `</head>` if missing. That position is late in the cascade.
- **Old font:** if the previous rule had a `font-family` whose family differs from the new one, call `drop_unused_rules(src, old_family)` afterwards.

`element_info` returns the tag name, the line of the tag, its `kx-s` class, and the props parsed from that class's rule (empty when unstyled).

- [ ] **Step 1: Write failing tests**
  - `style_sets_rule_and_class`:
    - Find the example deck's first `h2` with `tags::start_tags`, then `set_style` with `{"font-size":"96px","color":"#e4482b"}`. The element gets `class="… kx-s1"` (its existing classes kept).
    - The block has `.kx-s1{font-size:96px !important;color:#e4482b !important}`. That is PROPS order, so color comes after font-size: font-size is index 1 and color index 2.
    - `element_info` returns those props.
    - Restyling the same element with `{"color":"#000"}` keeps `kx-s1` and leaves no `font-size`. A second element gets `kx-s2`.
  - `clearing_removes_class_and_rule`: an empty props map, on an element that had no class before, leaves no `class=` attribute and no `.kx-s1` rule.
  - `class_attribute_variants`: `class='a b'`, `class=a`, and no class all end up as `class="… kx-sN"`.
  - `rejects_injection_and_unknown_props`: values `"red;}</style><script>"` and `"1px\n"`, and key `"position"`, each return `Err`, and the source is unchanged.
  - `style_font_is_linted`: `set_style` with `font = Inter` gives lint error `font: 'inter'`.
  - `restyle_font_drops_old_faces`: the font set to Inter, then to Geist, means `fonts_in_use` has no `inter` and lint has no `font:` error.
  - The existing `fonts::css` tests still pass after the `ensure_faces` extraction.
- [ ] **Step 2:** Run `cargo test edit::style`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS.

### Task 3: Commands and live rebuild

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 2, plus `ensure_font`, `fonts::embed` and `rebuild` from the bundled-fonts work.
- Produces Tauri commands:
  - `preview_src(src: String, brief: String) -> Report`: lint and assemble the unsaved text into `App.built` without saving.
    - Factor `rebuild` into `fn rebuild_from(app: &mut App, src: &str, brief: &str) -> Report`; `rebuild` reads the deck's text and calls it.
    - The deck's zip entries are not modified, so fonts embedded since the last save still serve, because `embed` already wrote them to the in-memory deck.
  - `element_info(src: String, index: usize) -> Option<ElementInfo>`
  - `style_element(src: String, index: usize, props: Props, font: Option<String>) -> Result<String, String>`: `async`.
    - When `font` is `Some(id)`: `ensure_font`, then `fonts::embed` into the open deck.
    - Then `set_style`. Returns the new source; the frontend puts it in the editor.

- [ ] **Step 1: Write failing test** `rebuild_unsaved_does_not_touch_disk`:
  - Load the example deck, saved to a temp path.
  - `rebuild_from` with source containing `<h2>Changed</h2>` inside a slide: the served `index.html` contains `Changed` and the file on disk does not.
  - `rebuild_from` with broken source: `serve("/index.html")` is `None` and errors are reported.
  - `rebuild_from` with the original: served again.
- [ ] **Step 2:** Run `cargo test rebuild_unsaved`. Expected: FAIL.
- [ ] **Step 3:** Implement and register the commands.
- [ ] **Step 4:** Run `cargo test && cargo clippy --all-targets`. Expected: PASS, and no clippy warnings in new code.

### Task 4: Edit bridge in the preview

**Files:**
- Create: `src-tauri/src/bridge.js`. Move the current `BRIDGE` script body here and extend it. It's served as `<script>` + `include_str!("bridge.js")` + `</script>`, so `BRIDGE` becomes a `LazyLock<String>` or a `concat!` of three parts.
- Modify: `src-tauri/src/lib.rs` (`BRIDGE` definition)

**Interfaces:**
- Produces the message protocol. All messages carry `stage: 1`.
  - **Parent → frame:**
    - `{ edit: boolean }` turns edit mode on or off;
    - `{ select: number | null }` draws or clears the selection box for `[data-kx-i="N"]`.
  - **Frame → parent:**
    - `{ select: { i: number, tag: string, text: string, style: { fontFamily, fontSize, color, fontWeight, fontStyle, textDecorationLine, letterSpacing, lineHeight, textAlign } } }` on click in edit mode.
    - `text` is the first 60 characters of `innerText`; `style` comes from `getComputedStyle`.
  - The existing `slide` and `key` messages are unchanged.

Behavior:
- **Hover:** in edit mode, `pointermove` outlines `e.target.closest('[data-kx-i]')` when it is inside `.slide.is-active`. The outline is a `position:fixed; pointer-events:none` div with a 1.5px accent border.
- **Click:** a capture-phase `click` listener on `window` calls `preventDefault()` and `stopImmediatePropagation()`, so the engine's stage click handler never runs. It then posts `select` and draws a solid selection box.
- **Keeping boxes in place:** boxes redraw on `resize` and on hash change; the selection box hides when its element is not on the active slide.
- **Exiting:** Escape in edit mode posts `{ key: 'edit-off' }` instead of `escape`.
- **Trust:** only messages from `parent` are honored (`e.source === parent`).

- [ ] **Step 1: Write failing test.** In the existing serve test, assert the served HTML contains `data-kx-i` and the bridge's `stopImmediatePropagation` capture listener (string check). Run `cargo test serves_deck`. Expected: FAIL.
- [ ] **Step 2:** Implement `bridge.js` and the include.
- [ ] **Step 3:** Run `cargo test`. Expected: PASS.

### Task 5: Style panel and edit mode in the editor

**Files:**
- Create:
  - `src/FontPicker.svelte`: extracted from `Fonts.svelte`'s search, chips, list and preview code, unchanged in behavior. Props `{ base, onpick: (e: Entry) => void, busy: string }`.
  - `src/Style.svelte`
- Modify: `src/Fonts.svelte` (use `FontPicker`), `src/App.svelte`

**Interfaces:**
- Consumes: Task 3 commands and the Task 4 message protocol.
- Produces, in `App.svelte`:
  - `editing` state, toggled by an Edit button in the preview's meta bar (next to "Slide N of M", with keyboard hint `E`) and by the frame's `edit-off` key.
  - A `"style"` tab after Fonts. Selecting an element switches to it.
  - `selected: number | null`.

Behavior:
- **Turning Edit on:**
  - The iframe URL becomes `index.html?v=<version>&motion=0#<slide>`.
  - On each iframe `load`, post `{edit: true}` and `{select: selected}` to `frame.contentWindow`, targeting origin `new URL(BASE).origin`.
- **Live rebuild:** while `editing`, a 300 ms debounced `$effect` on `src`/`brief` calls `preview_src`, then sets `report`, increments `version` and reloads the frame on the same slide. The Problems panel therefore stays live, and its "Save to re-check" hint is hidden while editing.
- **Selection versus code edits:** when `src` changes from the code editor while something is selected, clear `selected`. To tell edits apart, track a `fromPanel` flag set just before `srcEditor.replaceAll` from the Style panel.
- **F5, Shift+F5 and Present** set `editing = false` first and post `{edit: false}`. The present URL has no `motion=0`.
- **Style panel layout** (`Style.svelte`, props `{ src, base, selected, live, onapply }`, where `live` is the computed style from the bridge):
  - **Header:** a `<tag>` pill, the first 60 characters of the text, and a "Ln N" link that jumps the code editor to the tag.
  - **Rows:**
    - Font: the current family set in its own face, plus "Change", which opens `FontPicker`.
    - Size: a number input in px.
    - Color: `<input type="color">` plus a hex text field.
    - Weight: a select of 100–900.
    - Italic and Underline: toggles.
    - Letter spacing: number in em.
    - Line height: unitless number.
    - Align: a segmented control (left, center, right, justify).
  - **Values:** each control shows the override from `element_info` when set; otherwise it shows `live`'s computed value as a placeholder, dimmed.
  - **Clear:** a "Clear" link per row removes that override, and "Clear all" at the bottom removes all of them.
  - **Applying:** every change calls `style_element` with the full override map, then `onapply(next)`. Number inputs apply on `change`, not on every keystroke.
  - **Empty state:** "Turn on Edit and click something on a slide to style it."
- Use the existing CSS variables only.

- [ ] **Step 1:** Extract `FontPicker`. `npm run check` reports 0/0, and the Fonts tab behaves as before.
- [ ] **Step 2:** Implement edit mode and `Style.svelte`.
- [ ] **Step 3:** Run `npm run check`. Expected: 0 errors, 0 warnings.
- [ ] **Step 4:** Manual check with `npm run tauri dev`. If computer-use access is declined, the user runs it.
  - Edit on: animations stop, and hovering outlines elements.
  - Click a title: the Style tab opens with its tag and text.
  - Size 120: the preview updates within about half a second, and the source has the `kx-s1` class and rule.
  - Ctrl+Z: it reverts and the preview follows.
  - Font Newsreader: the title renders in Newsreader.
  - Font Inter: a lint error appears, then clears after switching to Geist.
  - Type a new tag above the selection in the code editor: the selection clears.
  - F5: presents at full motion, with clicks working normally.
  - Ctrl+S, then reopen the file: the styles persist, and the saved source has no `data-kx-i`.
