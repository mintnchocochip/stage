<script module lang="ts">
  type Computed = {
    fontFamily: string; fontSize: string; color: string; fontWeight: string; fontStyle: string;
    textDecorationLine: string; letterSpacing: string; lineHeight: string; textAlign: string;
  };
  /** What the preview bridge reports for a clicked element. */
  export type Picked = { i: number; tag: string; text: string; style: Computed };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import FontPicker, { type Entry, preview, face } from "./FontPicker.svelte";

  type Info = { tag: string; line: number; class: string | null; props: Record<string, string> };
  type Styled = { src: string; index: number };

  let { src, base, selected, live, onapply, ongoto }: {
    src: string; base: string; selected: number | null; live: Picked | null;
    onapply: (next: string, index: number) => void; ongoto: (line: number) => void;
  } = $props();

  const WEIGHTS = [["100", "Thin"], ["200", "Extra Light"], ["300", "Light"], ["400", "Regular"], ["500", "Medium"], ["600", "Semibold"], ["700", "Bold"], ["800", "Extra Bold"], ["900", "Black"]] as const;
  const ALIGNS = [
    ["left", "Left", "M1 1.5h12M1 4.5h8M1 7.5h12M1 10.5h8"],
    ["center", "Center", "M1 1.5h12M3 4.5h8M1 7.5h12M3 10.5h8"],
    ["right", "Right", "M1 1.5h12M5 4.5h8M1 7.5h12M5 10.5h8"],
    ["justify", "Justify", "M1 1.5h12M1 4.5h12M1 7.5h12M1 10.5h12"],
  ] as const;

  let info = $state<Info | null>(null);
  let catalog = $state<Entry[]>([]);
  let picking = $state(false);
  let busy = $state("");
  let error = $state("");

  $effect(() => {
    const s = src, i = selected;
    if (i == null) { info = null; return; }
    let stale = false;
    invoke<Info | null>("element_info", { src: s, index: i }).then((r) => { if (!stale) info = r; });
    return () => { stale = true; };
  });

  onMount(async () => { catalog = await invoke<Entry[]>("font_catalog"); });

  const o = $derived(info?.props ?? {});
  const cs = $derived(live?.style);
  const any = $derived(Object.keys(o).length > 0);

  // Canvas normalises any CSS colour to #rrggbb (or rgba() when translucent).
  const ctx = document.createElement("canvas").getContext("2d")!;
  const hex = (c: string | undefined) => { if (!c) return ""; ctx.fillStyle = "#000"; ctx.fillStyle = c; return String(ctx.fillStyle); };
  const first = (f: string | undefined) => (f ?? "").split(",")[0].trim().replace(/^['"]|['"]$/g, "");
  /** The number in `v` when it is written in `unit` ("" for unitless), else "". */
  const num = (v: string | undefined, unit: string) => v?.trim().match(new RegExp(`^(-?[\\d.]+)${unit}$`))?.[1] ?? "";
  const round = (n: number, d: number) => (Number.isFinite(n) ? String(+n.toFixed(d)) : "");
  const align = (a: string | undefined) => (a === "start" ? "left" : a === "end" ? "right" : a ?? "");

  const family = $derived(first(o["font-family"] ?? cs?.fontFamily));
  const fontEntry = $derived(catalog.find((e) => e.family.toLowerCase() === family.toLowerCase()));
  const px = $derived(parseFloat(cs?.fontSize ?? ""));
  const ph = $derived({
    size: round(px, 0),
    spacing: cs?.letterSpacing === "normal" ? "0" : round(parseFloat(cs?.letterSpacing ?? "") / px, 3),
    height: cs?.lineHeight === "normal" ? "normal" : round(parseFloat(cs?.lineHeight ?? "") / px, 2),
  });
  const weightLive = $derived(cs?.fontWeight ?? "");
  const italic = $derived((o["font-style"] ?? cs?.fontStyle) === "italic");
  const underline = $derived((o["text-decoration-line"] ?? cs?.textDecorationLine ?? "").includes("underline"));
  const aligned = $derived(align(o["text-align"] ?? cs?.textAlign));

  // Applies run one at a time and each reads the element's overrides from the current text,
  // so a change committed before the panel has refreshed still builds on the one before it.
  let queue: Promise<unknown> = Promise.resolve();
  /** Merge `patch` into the element's overrides (null clears them all) and write the result. */
  function apply(patch: Record<string, string> | null, font: string | null = null) {
    const run = queue.then(async () => {
      error = "";
      try {
        for (let tries = 0; tries < 2 && selected != null; tries++) {
          const before = src, index = selected;
          const cur = (await invoke<Info | null>("element_info", { src: before, index }))?.props ?? {};
          const r = await invoke<Styled>("style_element", { src: before, index, props: patch ? { ...cur, ...patch } : {}, font });
          // Edited while it ran (a font downloading, say): apply again to the current text, which is quick now.
          if (src === before && selected === index) { onapply(r.src, r.index); return true; }
        }
      } catch (e) { error = String(e); }
      return false;
    });
    queue = run;
    return run;
  }

  const clear = (...keys: string[]) => apply(Object.fromEntries(keys.map((k) => [k, ""])));
  const unitValue = (v: string, unit: string) => (v.trim() === "" ? "" : `${v.trim()}${unit}`);

  async function pickFont(e: Entry) {
    if (busy) return;
    busy = e.id;
    // The server sets font-family from the font itself.
    if (await apply({ "font-family": "" }, e.id)) {
      e.installed = true;
      const own = fontEntry ?? catalog.find((c) => c.id === e.id);
      if (own) own.installed = true;
      picking = false;
    }
    busy = "";
  }
</script>

{#snippet clearLink(keys: string[])}
  {#if keys.some((k) => o[k])}
    <button class="clear" onclick={() => clear(...keys)}>Clear</button>
  {:else}<span></span>{/if}
{/snippet}

<div class="style">
  {#if selected == null || !live}
    <p class="empty">Turn on Edit and click something on a slide to style it.</p>
  {:else}
    <header class="head">
      <span class="tag">{info?.tag ?? live.tag}</span>
      <span class="text">{live.text}</span>
      {#if info}<button class="ln" onclick={() => info && ongoto(info.line)} title="Show in deck.src.html">Ln {info.line}</button>{/if}
    </header>

    <section class="rows">
      <div class="row">
        <span class="label">Font</span>
        <button class="font" class:on={picking} onclick={() => { picking = !picking; error = ""; }} use:preview={{ e: fontEntry, base }}>
          <span class="family" class:unset={!o["font-family"]} style:font-family={face(fontEntry)}>{family || "Default"}</span>
          <span class="action">{picking ? "Cancel" : "Change"}</span>
        </button>
        {@render clearLink(["font-family"])}
      </div>

      {#if !picking}
        <div class="row">
          <span class="label">Size</span>
          <label class="field num">
            <input type="number" min="1" step="1" value={num(o["font-size"], "px")} placeholder={o["font-size"] ?? ph.size} aria-label="Font size in pixels"
              onchange={(e) => apply({ "font-size": unitValue(e.currentTarget.value, "px") })} />
            <span class="unit">px</span>
          </label>
          {@render clearLink(["font-size"])}
        </div>

        <div class="row">
          <span class="label">Color</span>
          <span class="pair">
            <input class="swatch" type="color" value={hex(o.color ?? cs?.color).startsWith("#") ? hex(o.color ?? cs?.color) : "#000000"} aria-label="Pick color"
              onchange={(e) => apply({ color: e.currentTarget.value })} />
            <input class="field hex" type="text" spellcheck="false" value={o.color ?? ""} placeholder={hex(cs?.color)} aria-label="Color value"
              onchange={(e) => {
                const v = e.currentTarget.value.trim();
                if (v && !CSS.supports("color", v)) { e.currentTarget.value = o.color ?? ""; return; }
                apply({ color: v });
              }} />
          </span>
          {@render clearLink(["color"])}
        </div>

        <div class="row">
          <span class="label">Weight</span>
          <select class="field weight" class:unset={!o["font-weight"]} value={o["font-weight"] ?? ""} aria-label="Font weight"
            onchange={(e) => apply({ "font-weight": e.currentTarget.value })}>
            <option value="">{weightLive} {WEIGHTS.find(([w]) => w === weightLive)?.[1] ?? ""}</option>
            {#each WEIGHTS as [w, n]}<option value={w}>{w} {n}</option>{/each}
          </select>
          {@render clearLink(["font-weight"])}
        </div>

        <div class="row">
          <span class="label">Style</span>
          <span class="pair">
            <button class="tog" class:inherit={!o["font-style"]} aria-pressed={italic} aria-label="Italic" title="Italic"
              onclick={() => apply({ "font-style": italic ? "normal" : "italic" })}><i>I</i></button>
            <button class="tog" class:inherit={!o["text-decoration-line"]} aria-pressed={underline} aria-label="Underline" title="Underline"
              onclick={() => apply({ "text-decoration-line": underline ? "none" : "underline" })}><u>U</u></button>
          </span>
          {@render clearLink(["font-style", "text-decoration-line"])}
        </div>

        <div class="row">
          <span class="label">Spacing</span>
          <label class="field num">
            <input type="number" step="0.01" value={num(o["letter-spacing"], "em")} placeholder={o["letter-spacing"] ?? ph.spacing} aria-label="Letter spacing in em"
              onchange={(e) => apply({ "letter-spacing": unitValue(e.currentTarget.value, "em") })} />
            <span class="unit">em</span>
          </label>
          {@render clearLink(["letter-spacing"])}
        </div>

        <div class="row">
          <span class="label">Line height</span>
          <label class="field num">
            <input type="number" min="0" step="0.05" value={num(o["line-height"], "")} placeholder={o["line-height"] ?? ph.height} aria-label="Line height"
              onchange={(e) => apply({ "line-height": e.currentTarget.value.trim() })} />
          </label>
          {@render clearLink(["line-height"])}
        </div>

        <div class="row">
          <span class="label">Align</span>
          <span class="seg" role="group" aria-label="Text align">
            {#each ALIGNS as [value, label, d]}
              <button class:inherit={!o["text-align"]} aria-pressed={aligned === value} aria-label={label} title={label}
                onclick={() => apply({ "text-align": value })}>
                <svg width="14" height="12" viewBox="0 0 14 12" aria-hidden="true"><path {d} stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"/></svg>
              </button>
            {/each}
          </span>
          {@render clearLink(["text-align"])}
        </div>
      {/if}
    </section>

    {#if picking}
      <FontPicker {base} onpick={pickFont} {busy} {error} />
    {:else if any || error}
      <footer class="foot">
        {#if error}<span class="error">{error}</span>{/if}
        {#if any}<button class="clear" onclick={() => apply(null)}>Clear all</button>{/if}
      </footer>
    {/if}
  {/if}
</div>

<style>
  .style { display: flex; flex-direction: column; height: 100%; min-height: 0; --ui: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; }
  button { font: inherit; color: inherit; cursor: pointer; border: 0; background: none; padding: 0; }
  button:disabled { cursor: default; }
  button:focus-visible, input:focus-visible, select:focus-visible, .num:focus-within { outline: 2px solid var(--accent); outline-offset: 1px; }

  .empty { margin: 0; padding: 20px 14px; color: var(--dim); }

  .head { display: flex; align-items: center; gap: 10px; padding: 12px 14px; border-bottom: 1px solid var(--line); }
  .tag { flex: none; padding: 1px 6px; border-radius: 4px; background: var(--raised); color: var(--dim); font: 11px/18px var(--mono); }
  .text { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ln { flex: none; color: var(--faint); font: 11px var(--mono); }
  .ln:hover { color: var(--dim); }

  .rows { flex: 0 1 auto; min-height: 0; padding: 6px 0; overflow: auto; }
  .row { display: grid; grid-template-columns: 72px minmax(0, 1fr) auto; align-items: center; gap: 12px; min-height: 40px; padding: 0 14px; }
  .label { font-size: 12px; color: var(--faint); }
  .clear { font-size: 12px; color: var(--faint); }
  .clear:hover { color: var(--text); }

  .font { display: flex; align-items: baseline; gap: 12px; box-sizing: border-box; width: 100%; min-width: 0; padding: 4px 8px; border-radius: 6px; text-align: left; }
  .font:hover, .font.on { background: var(--raised); }
  .family { flex: 1; min-width: 0; font-size: 17px; line-height: 1.3; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .action { font-size: 12px; color: var(--dim); }
  .font.on .action { color: var(--accent); }

  .field { box-sizing: border-box; height: 28px; border: 1px solid var(--line); border-radius: 6px; background: var(--bg); color: var(--text); font: inherit; font-size: 12px; }
  .unset, ::placeholder { color: var(--faint); }
  .num { display: inline-flex; align-items: center; gap: 4px; width: 92px; padding: 0 8px; }
  .num input { flex: 1; min-width: 0; padding: 0; border: 0; outline: 0; background: none; color: inherit; font: inherit; text-align: right; font-variant-numeric: tabular-nums; appearance: textfield; }
  .num input::-webkit-inner-spin-button, .num input::-webkit-outer-spin-button { appearance: none; margin: 0; }
  .unit { color: var(--faint); font-size: 11px; }

  .pair { display: flex; align-items: center; gap: 6px; }
  .swatch { box-sizing: border-box; width: 28px; height: 28px; padding: 0; border: 1px solid var(--line); border-radius: 6px; background: var(--bg); cursor: pointer; }
  .swatch::-webkit-color-swatch-wrapper { padding: 3px; }
  .swatch::-webkit-color-swatch { border: 0; border-radius: 3px; }
  .hex { width: 92px; padding: 0 8px; font: 12px var(--mono); }
  .weight { width: 140px; padding: 0 6px; }
  .weight option { background: var(--panel); color: var(--text); }

  .tog { box-sizing: border-box; width: 28px; height: 28px; border: 1px solid var(--line); border-radius: 6px; color: var(--dim); font-size: 13px; }
  .tog i { font-family: Georgia, serif; }
  .tog:hover, .seg button:hover { color: var(--text); }
  .seg { display: inline-flex; box-sizing: border-box; height: 28px; border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }
  .seg button { display: grid; place-items: center; width: 30px; color: var(--dim); }
  .seg button + button { border-left: 1px solid var(--line); }
  .tog[aria-pressed="true"], .seg button[aria-pressed="true"] { background: var(--raised); color: var(--text); }
  .tog.inherit[aria-pressed="true"], .seg button.inherit[aria-pressed="true"] { color: var(--faint); }

  .foot { display: flex; align-items: center; gap: 10px; margin-top: auto; padding: 8px 14px; border-top: 1px solid var(--line); font-size: 12px; }
  .error { flex: 1; color: var(--err); }
  .foot .clear { margin-left: auto; }
</style>
