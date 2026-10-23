<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open, save, ask, message } from "@tauri-apps/plugin-dialog";
  import Editor from "./Editor.svelte";
  import Fonts, { type LibraryStatus } from "./Fonts.svelte";
  import Style, { type Picked } from "./Style.svelte";

  type Finding = { message: string; line: number | null };
  type Report = { slides: number; errors: Finding[]; warnings: Finding[]; verify: string[] };
  type Opened = { path: string; src: string; brief: string; report: Report };

  // Custom protocols are http://<scheme>.localhost on Windows, <scheme>://localhost elsewhere.
  const BASE = navigator.userAgent.includes("Windows") ? "http://stage.localhost/" : "stage://localhost/";
  // Non-special schemes have an opaque ("null") origin, which postMessage cannot target; the bridge checks the sender anyway.
  const ORIGIN = new URL(BASE).origin === "null" ? "*" : new URL(BASE).origin;
  const FILTERS = [{ name: "Stage deck", extensions: ["stage"] }];
  const win = getCurrentWindow();

  let path = $state("");
  let src = $state("");
  let brief = $state("");
  let report = $state<Report | null>(null);
  let dirty = $state(false);
  let tab = $state<"src" | "brief" | "fonts" | "style">("src");
  let slide = $state(1);
  let version = $state(0);
  let frameUrl = $state("");
  let presenting = $state(false);
  let split = $state(45);
  let askLibrary = $state(false);
  let editing = $state(false);
  let selected = $state<number | null>(null);
  let live = $state<Picked | null>(null);

  let srcEditor: Editor;
  let briefEditor: Editor;
  let frame = $state<HTMLIFrameElement>();
  // Set while the Style panel writes the source, so its own edits keep the selection.
  let fromPanel = false;
  // The src and brief the served preview was last built from.
  let built = "";

  const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;
  const name = $derived(path.split(/[\\/]/).pop() ?? "");
  const blocked = $derived(!report || report.errors.length > 0);

  $effect(() => { win.setTitle(name ? `${dirty ? "* " : ""}${name} - Stage` : "Stage"); });

  function showPreview(at: number) {
    frameUrl = blocked ? "" : `${BASE}index.html?v=${version}${editing ? "&motion=0" : ""}#${at}`;
  }

  const post = (m: object) => frame?.contentWindow?.postMessage({ stage: 1, ...m }, ORIGIN);

  function frameLoaded() {
    if (editing) { post({ edit: true }); post({ select: selected }); }
  }

  function setEditing(on: boolean) {
    if (on === editing || (on && (!path || presenting))) return;
    editing = on;
    // The preview stops reporting the selection, so the Style panel would go stale.
    if (!on) { post({ edit: false }); selected = null; live = null; }
    showPreview(slide);
  }

  // Edit mode rebuilds the preview from the unsaved text, so the Problems list stays live.
  $effect(() => {
    if (!editing) return;
    const s = src, b = brief;
    if (s + "\0" + b === built) return;
    const t = setTimeout(async () => {
      try {
        report = await invoke<Report>("preview_src", { src: s, brief: b });
        built = s + "\0" + b; version++;
        showPreview(slide);
      } catch { /* no deck open */ }
    }, 300);
    return () => clearTimeout(t);
  });

  function styled(next: string, index: number) {
    selected = index;
    fromPanel = true;
    srcEditor.replaceAll(next);
    fromPanel = false;
  }

  function srcChanged(t: string) {
    src = t; dirty = true;
    if (!fromPanel && selected != null) { selected = null; live = null; post({ select: null }); }
  }

  async function confirmDiscard() {
    return !dirty || (await ask("You have unsaved changes. Discard them?", { title: "Stage", kind: "warning" }));
  }

  async function loaded(o: Opened) {
    path = o.path; src = o.src; brief = o.brief; report = o.report; dirty = false; slide = 1; version++;
    built = o.src + "\0" + o.brief; selected = null; live = null;
    srcEditor.load(o.src); briefEditor.load(o.brief);
    showPreview(1);
  }

  async function run(cmd: string, args: Record<string, unknown>) {
    try { await loaded(await invoke<Opened>(cmd, args)); }
    catch (e) { await message(String(e), { title: "Stage", kind: "error" }); }
  }

  async function openDeck() {
    if (!(await confirmDiscard())) return;
    const p = await open({ filters: FILTERS, multiple: false, directory: false });
    if (p) await run("open_deck", { path: p });
  }

  async function newDeck() {
    if (!(await confirmDiscard())) return;
    const p = await save({ filters: FILTERS, defaultPath: "untitled.stage" });
    if (p) await run("new_deck", { path: p });
  }

  async function saveDeck() {
    if (!path) return;
    // Typing during the save must not count as built.
    const s = src, b = brief;
    try {
      report = await invoke<Report>("save_deck", { src: s, brief: b });
      built = s + "\0" + b; dirty = false; version++;
      showPreview(slide);
    } catch (e) { await message(String(e), { title: "Stage", kind: "error" }); }
  }

  async function present(fromStart: boolean) {
    if (blocked || presenting) return;
    setEditing(false);
    if (fromStart) { slide = 1; frameUrl = `${BASE}index.html?v=${version}#1`; }
    presenting = true;
    await win.setFullscreen(true);
    await tick();
    frame?.contentWindow?.focus();
  }

  async function exitPresent() {
    presenting = false;
    await win.setFullscreen(false);
  }

  let lastEsc = 0;
  function key(k: string) {
    if (k === "f5") present(false);
    else if (k === "shift+f5") present(true);
    else if (k === "ctrl+s") saveDeck();
    else if (k === "ctrl+o") openDeck();
    else if (k === "ctrl+n") newDeck();
    else if (k === "edit-off") setEditing(false);
    // Style edits land in the (possibly hidden) code editor; undo them there.
    else if (["ctrl+z", "ctrl+shift+z", "ctrl+y"].includes(k)) {
      if ((tab === "style" || editing) && !presenting) k === "ctrl+z" ? srcEditor.undo() : srcEditor.redo();
    }
    else if (k === "escape" && presenting) {
      const now = Date.now();
      if (now - lastEsc < 600) { lastEsc = 0; exitPresent(); } else lastEsc = now;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    // Leave E and Escape to the code editor and form fields.
    const typing = (e.target as HTMLElement).closest?.("input, textarea, select, [contenteditable]");
    if (!typing && !mod && !e.altKey && e.key.toLowerCase() === "e") { e.preventDefault(); setEditing(!editing); return; }
    if (!typing && e.key === "Escape" && editing && !presenting) { setEditing(false); return; }
    // Undo/redo: CodeMirror handles its own; form fields keep theirs outside the Style tab.
    const z = mod ? ({ z: e.shiftKey ? "ctrl+shift+z" : "ctrl+z", y: "ctrl+y" } as Record<string, string>)[e.key.toLowerCase()] : undefined;
    if (z && !(e.target as HTMLElement).closest?.(".cm-editor") && (tab === "style" || (editing && !typing)) && !presenting) {
      e.preventDefault(); key(z); return;
    }
    const k = e.key === "F5" ? (e.shiftKey ? "shift+f5" : "f5")
      : mod && ["s", "o", "n"].includes(e.key.toLowerCase()) ? `ctrl+${e.key.toLowerCase()}`
      : e.key === "Escape" ? "escape" : "";
    if (!k) return;
    if (k !== "escape") e.preventDefault();
    key(k);
  }

  function onMessage(e: MessageEvent) {
    if (!BASE.startsWith(e.origin) || !e.data?.stage) return;
    if (e.data.slide) slide = e.data.slide;
    if (e.data.select) {
      const p = e.data.select as Picked;
      // The bridge re-reports the selection after each reload; that only refreshes the computed style.
      if (e.data.refresh) { if (p.i === selected) live = p; }
      // A click on a preview built from older text would index the wrong element.
      else if (src + "\0" + brief === built) { live = p; selected = p.i; tab = "style"; }
    }
    if (e.data.key) key(e.data.key);
  }

  function drag(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) => { split = Math.min(80, Math.max(20, (m.clientX / innerWidth) * 100)); };
    const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }

  function jump(f: Finding) {
    if (f.line == null) return;
    tab = "src";
    srcEditor.goto(f.line);
  }

  // First launch: offer the full font library once. Either answer is remembered; the Fonts tab can download it later.
  const ASKED = "stage.fontLibrary";
  function answerLibrary(download: boolean) {
    try { localStorage.setItem(ASKED, download ? "download" : "later"); } catch {}
    askLibrary = false;
    if (download) invoke("download_library");
  }

  onMount(() => {
    let asked = true;
    try { asked = localStorage.getItem(ASKED) != null; } catch {}
    if (!asked) invoke<LibraryStatus>("library_status").then((s) => { askLibrary = s.installed < s.total; });
    const unlisten = win.onCloseRequested(async (ev) => { if (!(await confirmDiscard())) ev.preventDefault(); });
    (async () => {
      const p = await invoke<string | null>("startup_path");
      if (p) await run("open_deck", { path: p });
      else await openDeck();
    })();
    return () => { unlisten.then((u) => u()); };
  });
</script>

<svelte:window onkeydown={onKeydown} onmessage={onMessage} />

<div class="app" class:presenting class:nodeck={!path}>
  <header class="bar">
    <div class="group">
      <span class="mark" aria-hidden="true"></span>
      <button class="ghost" onclick={newDeck} title="New deck (Ctrl+N)">New</button>
      <button class="ghost" onclick={openDeck} title="Open deck (Ctrl+O)">Open</button>
      <button class="ghost" onclick={saveDeck} disabled={!path || !dirty} title="Save (Ctrl+S)">Save</button>
    </div>
    <div class="doc">
      {#if name}
        <span class="doc-name">{name}</span>
        {#if dirty}<span class="dot" title="Unsaved changes"></span>{/if}
      {/if}
    </div>
    <div class="group end">
      <button class="present" onclick={() => present(false)} disabled={blocked}
        title={blocked && report ? "Fix errors to present" : "Present from this slide (F5) · from start (Shift+F5)"}>
        <svg width="10" height="12" viewBox="0 0 10 12" aria-hidden="true"><path d="M0 0v12l10-6z" fill="currentColor"/></svg>
        Present
      </button>
    </div>
  </header>

  {#if askLibrary}
    <div class="strip" role="region" aria-label="Font library">
      <span>Download the full Google Fonts library (about 100 MB)? You can do this later from the Fonts tab.</span>
      <button class="present small" onclick={() => answerLibrary(true)}>Download</button>
      <button class="ghost" onclick={() => answerLibrary(false)}>Not now</button>
    </div>
  {/if}

  <main style="grid-template-columns: minmax(0, {split}%) 1px minmax(0, 1fr)">
    <section class="left">
      <div class="tabs" role="tablist">
        <button role="tab" aria-selected={tab === "src"} class:on={tab === "src"} onclick={() => (tab = "src")}>deck.src.html</button>
        <button role="tab" aria-selected={tab === "brief"} class:on={tab === "brief"} onclick={() => (tab = "brief")}>brief.json</button>
        <span class="tab-sep" aria-hidden="true"></span>
        <button role="tab" class="ui" aria-selected={tab === "fonts"} class:on={tab === "fonts"} onclick={() => (tab = "fonts")}>Fonts</button>
        <button role="tab" class="ui" aria-selected={tab === "style"} class:on={tab === "style"} onclick={() => (tab = "style")}>Style</button>
      </div>
      <div class="editors">
        <div class="pane" class:hidden={tab !== "src"}>
          <Editor bind:this={srcEditor} lang="html" onchange={srcChanged} />
        </div>
        <div class="pane" class:hidden={tab !== "brief"}>
          <Editor bind:this={briefEditor} lang="json" onchange={(t) => { brief = t; dirty = true; }} />
        </div>
        <div class="pane" class:hidden={tab !== "fonts"}>
          <Fonts {src} base={BASE} onapply={(next) => srcEditor.replaceAll(next)} />
        </div>
        <div class="pane" class:hidden={tab !== "style"}>
          <Style {src} base={BASE} {selected} {live} onapply={styled} ongoto={(line) => { tab = "src"; srcEditor.goto(line); }} />
        </div>
      </div>
      {#if report}
        <div class="lint">
          <div class="lint-head">
            <span class="label">Problems</span>
            {#if report.errors.length}<span class="count err">{report.errors.length}</span>{/if}
            {#if report.warnings.length}<span class="count warn">{report.warnings.length}</span>{/if}
            <span class="spacer"></span>
            <span class="muted">{dirty && !editing ? "Save to re-check" : plural(report.slides, "slide")}</span>
          </div>
          {#if report.errors.length || report.warnings.length || report.verify.length}
            <ul>
              {#each [...report.errors.map((f) => ["err", f] as const), ...report.warnings.map((f) => ["warn", f] as const)] as [kind, f]}
                <li>
                  <button class="item" onclick={() => jump(f)} disabled={f.line == null}>
                    <span class="sev {kind}" aria-label={kind === "err" ? "Error" : "Warning"}></span>
                    <span class="msg">{f.message}</span>
                    {#if f.line != null}<span class="ln">Ln {f.line}</span>{/if}
                  </button>
                </li>
              {/each}
              {#each report.verify as v}
                <li><div class="item"><span class="sev check" aria-label="To verify"></span><span class="msg muted">{v}</span></div></li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </section>

    <div class="divider" onpointerdown={drag} role="separator" aria-orientation="vertical"></div>

    <section class="right">
      {#if frameUrl}
        <div class="stage"><iframe bind:this={frame} src={frameUrl} title="Deck preview" allow="fullscreen" onload={frameLoaded}></iframe></div>
        <div class="meta">
          <span>Slide {slide} of {report?.slides ?? "?"}</span>
          <span class="meta-end">
            {#if editing}<span class="muted">Editing · <kbd>Esc</kbd> to stop</span>
            {:else}<span class="muted"><kbd>F5</kbd> to present</span>{/if}
            <button class="ghost edit" aria-pressed={editing} onclick={() => setEditing(!editing)} title="Click elements on the slide to style them (E)">
              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M8.2 1.3l2.5 2.5-6.6 6.6-3.1.6.6-3.1z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
              Edit <kbd>E</kbd>
            </button>
          </span>
        </div>
      {:else if report}
        <div class="empty">
          <h2>Preview paused</h2>
          <p>{report.errors.length === 1 ? "One error needs" : `${report.errors.length} errors need`} fixing before the deck can render.</p>
          <ul>
            {#each report.errors as f}
              <li><button class="link" onclick={() => jump(f)} disabled={f.line == null}>{f.message}{#if f.line != null}<span class="ln">Ln {f.line}</span>{/if}</button></li>
            {/each}
          </ul>
        </div>
      {:else}
        <div class="empty">
          <h2>No deck open</h2>
          <p>Open a <code>.stage</code> file or start a new one.</p>
          <div class="actions">
            <button class="present" onclick={openDeck}>Open deck…</button>
            <button class="ghost outline" onclick={newDeck}>New deck</button>
          </div>
          <p class="hint"><kbd>Ctrl</kbd> <kbd>O</kbd> open · <kbd>Ctrl</kbd> <kbd>N</kbd> new</p>
        </div>
      {/if}
    </section>
  </main>
</div>

<style>
  :global(:root) {
    color-scheme: dark;
    --bg: #121212;
    --panel: #19191a;
    --raised: #222224;
    --line: #2b2b2e;
    --text: #e8e6e3;
    --dim: #a19e98;
    --faint: #6e6c68;
    --accent: #e9783e;
    --accent-hi: #f08a52;
    --err: #ee6f66;
    --warn: #d9a845;
    --mono: "Cascadia Mono", ui-monospace, Consolas, monospace;
  }
  :global(html, body) { margin: 0; height: 100%; background: var(--bg); color: var(--text); font: 13px/1.5 "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; -webkit-font-smoothing: antialiased; }
  :global(#app) { height: 100%; }

  .app { display: grid; grid-template-rows: 44px auto 1fr; height: 100%; }

  .strip { display: flex; align-items: center; gap: 10px; padding: 8px 14px; background: var(--raised); border-bottom: 1px solid var(--line); }
  .strip span { flex: 1; color: var(--dim); }
  .present.small { padding: 4px 12px; }

  /* Top bar */
  .bar { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; padding: 0 8px 0 14px; background: var(--panel); border-bottom: 1px solid var(--line); user-select: none; }
  .group { display: flex; align-items: center; gap: 2px; }
  .group.end { justify-content: flex-end; }
  .mark { width: 10px; height: 10px; border-radius: 50%; background: var(--accent); margin-right: 12px; box-shadow: 0 0 0 3px #e9783e22; }
  .doc { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .doc-name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--dim); flex: none; }

  button { font: inherit; color: inherit; cursor: pointer; border: 0; background: none; }
  button:disabled { cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .ghost { padding: 5px 10px; border-radius: 6px; color: var(--dim); }
  .ghost:hover:not(:disabled) { background: var(--raised); color: var(--text); }
  .ghost:disabled { color: var(--faint); opacity: 0.6; }
  .ghost.outline { border: 1px solid var(--line); color: var(--text); }
  .present { display: inline-flex; align-items: center; gap: 8px; padding: 6px 14px; border-radius: 6px; background: var(--accent); color: #1a0e07; font-weight: 600; }
  .present:hover:not(:disabled) { background: var(--accent-hi); }
  .present:disabled { background: var(--raised); color: var(--faint); }

  main { display: grid; min-height: 0; }

  /* Left: editor + problems */
  .left { display: grid; grid-template-rows: 36px minmax(0, 1fr) auto; grid-template-columns: minmax(0, 1fr); min-width: 0; min-height: 0; background: var(--panel); }
  .tabs { display: flex; gap: 2px; padding: 0 8px; border-bottom: 1px solid var(--line); }
  .tabs button { position: relative; padding: 0 12px; color: var(--faint); font: 12px var(--mono); }
  .tabs button:hover { color: var(--dim); }
  .tabs button.on { color: var(--text); }
  .tabs button.ui { font: 12px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; }
  .tab-sep { width: 1px; margin: 10px 6px; background: var(--line); }
  .tabs button.on::after { content: ""; position: absolute; left: 12px; right: 12px; bottom: -1px; height: 2px; background: var(--accent); border-radius: 2px 2px 0 0; }
  .editors, .pane { min-height: 0; height: 100%; }
  .hidden { display: none; }

  .lint { max-height: 32vh; overflow: auto; border-top: 1px solid var(--line); }
  .lint-head { position: sticky; top: 0; display: flex; align-items: center; gap: 6px; height: 32px; padding: 0 14px; background: var(--panel); font-size: 12px; }
  .label { font-weight: 600; color: var(--dim); margin-right: 2px; }
  .spacer { flex: 1; }
  .count { min-width: 18px; padding: 0 5px; border-radius: 9px; text-align: center; font: 600 11px/18px var(--mono); }
  .count.err { background: #ee6f661f; color: var(--err); }
  .count.warn { background: #d9a8451f; color: var(--warn); }
  .lint ul, .empty ul { list-style: none; margin: 0; padding: 0 0 6px; }
  .item { display: flex; align-items: baseline; gap: 10px; width: 100%; padding: 4px 14px; text-align: left; box-sizing: border-box; }
  button.item:hover:not(:disabled) { background: var(--raised); }
  .sev { flex: none; width: 6px; height: 6px; border-radius: 50%; transform: translateY(-1px); }
  .sev.err { background: var(--err); }
  .sev.warn { background: var(--warn); }
  .sev.check { border: 1px solid var(--faint); box-sizing: border-box; }
  .msg { flex: 1; min-width: 0; }
  .ln { flex: none; margin-left: auto; color: var(--faint); font: 11px var(--mono); }
  .muted { color: var(--faint); }

  /* Divider: 1px line, wider grab area. */
  .divider { position: relative; z-index: 1; background: var(--line); cursor: col-resize; }
  .divider::before { content: ""; position: absolute; inset: 0 -4px; }
  .divider:hover { background: var(--accent); }

  /* Right: preview */
  .right { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; min-width: 0; min-height: 0; padding: 28px; background: var(--bg); }
  .stage { width: 100%; aspect-ratio: 16 / 9; max-height: calc(100% - 32px); border-radius: 4px; overflow: hidden; box-shadow: 0 0 0 1px #ffffff12, 0 12px 40px #00000080; }
  iframe { display: block; width: 100%; height: 100%; border: 0; background: #000; }
  .meta { display: flex; justify-content: space-between; align-items: center; width: 100%; font-size: 12px; color: var(--dim); font-variant-numeric: tabular-nums; }
  .meta-end { display: flex; align-items: center; gap: 12px; }
  .ghost.edit { display: inline-flex; align-items: center; gap: 6px; margin: -3px -8px -3px 0; padding: 3px 8px; }
  .ghost.edit[aria-pressed="true"] { color: var(--accent); }
  .ghost.edit[aria-pressed="true"] kbd { color: inherit; }

  .empty { max-width: 420px; }
  .empty h2 { margin: 0 0 4px; font-size: 15px; font-weight: 600; }
  .empty p { margin: 0 0 16px; color: var(--dim); }
  .empty .actions { display: flex; gap: 8px; margin-bottom: 20px; }
  .empty .hint { font-size: 12px; color: var(--faint); }
  .empty li { border-top: 1px solid var(--line); }
  .link { display: flex; gap: 12px; width: 100%; padding: 8px 0; text-align: left; color: var(--err); }
  .link:hover:not(:disabled) { color: #ff8e86; }
  code, kbd { font: 11px var(--mono); }
  kbd { padding: 1px 5px; border: 1px solid var(--line); border-bottom-width: 2px; border-radius: 4px; color: var(--dim); }

  /* No deck: just the welcome. */
  .nodeck .left, .nodeck .divider { display: none; }
  .nodeck main { grid-template-columns: 1fr !important; }

  /* Present: only the deck, filling the fullscreen window. */
  .presenting { grid-template-rows: 1fr; }
  .presenting .strip,
  .presenting .bar, .presenting .left, .presenting .divider, .presenting .meta { display: none; }
  .presenting main { grid-template-columns: 1fr !important; }
  .presenting .right { padding: 0; background: #000; }
  .presenting .stage { aspect-ratio: auto; height: 100%; max-height: none; border-radius: 0; box-shadow: none; }
</style>

