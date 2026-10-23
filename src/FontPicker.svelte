<script module lang="ts">
  export type Entry = { id: string; family: string; category: string; installed: boolean; bundled: boolean; blocked: boolean };

  const CATEGORIES = [["", "All"], ["serif", "Serif"], ["sans-serif", "Sans"], ["display", "Display"], ["handwriting", "Handwriting"], ["monospace", "Mono"]] as const;

  // Shared by every picker so the search survives closing and reopening it.
  let query = $state("");
  let category = $state("");

  // Each installed family's preview file registers once, the first time its name scrolls into view.
  const registered = new Set<string>();
  export function preview(node: HTMLElement, arg: { e: Entry | undefined; base: string }) {
    const io = new IntersectionObserver(([hit]) => {
      const id = arg.e?.installed ? arg.e.id : "";
      if (!hit.isIntersecting || !id || registered.has(id)) return;
      registered.add(id);
      new FontFace(`pv-${id}`, `url(${arg.base}_stage/fonts/${id}/preview.woff2)`).load()
        .then((f) => document.fonts.add(f)).catch(() => registered.delete(id));
    });
    io.observe(node);
    // Re-observing fires the callback again, for rows already on screen when the catalog arrives.
    return { update: (next: typeof arg) => { arg = next; io.unobserve(node); io.observe(node); }, destroy: () => io.disconnect() };
  }
  /** Font stack for a family's preview face; needs `--ui` set on an ancestor. */
  export const face = (e: Entry | undefined) => (e?.installed ? `'pv-${e.id}', var(--ui)` : "var(--ui)");
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  let { base, onpick, busy, error = "" }: { base: string; onpick: (e: Entry) => void; busy: string; error?: string } = $props();

  let catalog = $state<Entry[]>([]);
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return catalog.filter((e) => (!category || e.category === category) && (!q || e.family.toLowerCase().includes(q)));
  });

  const reload = async () => { catalog = await invoke<Entry[]>("font_catalog"); };

  onMount(() => {
    reload();
    const un = listen<{ running: boolean }>("font-library", (ev) => { if (!ev.payload.running) reload(); });
    return () => { un.then((u) => u()); };
  });
</script>

<section class="picker">
  <input type="search" placeholder="Search {catalog.length.toLocaleString()} families" bind:value={query} aria-label="Search fonts" />
  <div class="chips" role="group" aria-label="Category">
    {#each CATEGORIES as [value, label]}
      <button class:on={category === value} onclick={() => (category = value)}>{label}</button>
    {/each}
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  <ul>
    {#each shown as e (e.id)}
      <li>
        <button class="row" onclick={() => onpick(e)} disabled={!!busy} use:preview={{ e, base }}>
          <span class="family" style:font-family={face(e)}>{e.family}</span>
          {#if e.blocked}<span class="badge" title="Lint rejects this font unless the deck allows it with kx-allow-fonts">Lint error</span>{/if}
          {#if busy === e.id}<span class="muted">Installing…</span>
          {:else if !e.installed}<span class="get" title="Downloads when picked" aria-label="Not downloaded">↓</span>{/if}
        </button>
      </li>
    {:else}
      <li class="muted empty">No families match.</li>
    {/each}
  </ul>
</section>

<style>
  .picker { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  button { font: inherit; color: inherit; cursor: pointer; border: 0; background: none; }
  button:disabled { cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  input { margin: 12px 14px 8px; padding: 6px 10px; border: 1px solid var(--line); border-radius: 6px; background: var(--bg); color: var(--text); font: inherit; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; padding: 0 14px 8px; }
  .chips button { padding: 2px 10px; border: 1px solid var(--line); border-radius: 999px; font-size: 12px; color: var(--dim); }
  .chips button.on { border-color: var(--accent); color: var(--text); }
  ul { list-style: none; margin: 0; padding: 0 0 8px; overflow: auto; flex: 1; }
  li { content-visibility: auto; contain-intrinsic-size: auto 40px; }
  .row { display: flex; align-items: center; gap: 10px; width: 100%; padding: 8px 14px; text-align: left; }
  .row:hover:not(:disabled) { background: var(--raised); }
  .family { flex: 1; min-width: 0; font-size: 17px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge { padding: 0 6px; border-radius: 9px; background: #ee6f661f; color: var(--err); font-size: 11px; line-height: 18px; }
  .get { color: var(--faint); font-size: 12px; }
  .muted { color: var(--faint); font-size: 12px; }
  .empty { padding: 12px 14px; }
  .error { margin: 0 14px 8px; color: var(--err); font-size: 12px; }
</style>
