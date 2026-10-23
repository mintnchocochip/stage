<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import FontPicker, { type Entry, preview, face } from "./FontPicker.svelte";

  export type LibraryStatus = { installed: number; total: number; failed: number; running: boolean };

  let { src, base, onapply }: { src: string; base: string; onapply: (next: string) => void } = $props();

  const ROLES = [["display", "Display"], ["body", "Body"], ["mono", "Mono"]] as const;

  let catalog = $state<Entry[]>([]);
  let status = $state<LibraryStatus | null>(null);
  let picking = $state<string | null>(null);
  let busy = $state("");
  let error = $state("");

  const current = $derived.by(() => {
    const out: Record<string, string> = {};
    for (const m of src.matchAll(/--font-(display|body|mono)\s*:\s*['"]?([^'",;}]+)/g)) out[m[1]] ??= m[2].trim();
    return out;
  });
  const byFamily = $derived(new Map(catalog.map((e) => [e.family.toLowerCase(), e])));

  async function reload() {
    catalog = await invoke<Entry[]>("font_catalog");
    status = await invoke<LibraryStatus>("library_status");
  }

  async function pick(e: Entry) {
    if (!picking || busy) return;
    const role = picking, before = src;
    busy = e.id; error = "";
    try {
      let next = await invoke<string>("apply_font", { src: before, role, id: e.id });
      // Edited while the font downloaded: apply again to the current text (installed now, so this is quick).
      if (src !== before) next = await invoke<string>("apply_font", { src, role, id: e.id });
      onapply(next);
      e.installed = true;
      const own = byFamily.get(e.family.toLowerCase());
      if (own) own.installed = true;
      picking = null;
    } catch (err) { error = String(err); }
    finally { busy = ""; }
  }

  onMount(() => {
    reload();
    const un = listen<LibraryStatus>("font-library", (ev) => {
      status = ev.payload;
      if (!ev.payload.running) reload();
    });
    return () => { un.then((u) => u()); };
  });
</script>

<div class="fonts">
  <section class="roles">
    <h3>Deck fonts</h3>
    {#each ROLES as [role, label]}
      {@const e = byFamily.get(current[role]?.toLowerCase() ?? "")}
      <button class="role" class:on={picking === role} onclick={() => { picking = picking === role ? null : role; error = ""; }} use:preview={{ e, base }}>
        <span class="role-label">{label}</span>
        <span class="role-family" style:font-family={face(e)}>{current[role] ?? "Not set"}</span>
        <span class="role-action">{picking === role ? "Cancel" : "Change"}</span>
      </button>
    {/each}
  </section>

  {#if picking}
    <FontPicker {base} onpick={pick} {busy} {error} />
  {/if}

  {#if status}
    <footer class="library">
      {#if status.running}
        <progress max={status.total} value={status.installed}></progress>
        <span>{status.installed.toLocaleString()} of {status.total.toLocaleString()}</span>
        <button class="link" onclick={() => invoke("cancel_library")}>Pause</button>
      {:else}
        <span>{status.installed.toLocaleString()} of {status.total.toLocaleString()} families installed</span>
        {#if status.installed < status.total}
          <button class="link" onclick={() => invoke("download_library")}>Download all (about 100 MB)</button>
        {/if}
      {/if}
      {#if status.failed}<span class="error">{status.failed} failed</span>{/if}
    </footer>
  {/if}
</div>

<style>
  .fonts { display: flex; flex-direction: column; height: 100%; min-height: 0; --ui: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; }
  h3 { margin: 0 0 8px; font-size: 12px; font-weight: 600; color: var(--dim); }
  .roles { padding: 16px 14px 12px; border-bottom: 1px solid var(--line); }
  button { font: inherit; color: inherit; cursor: pointer; border: 0; background: none; }
  button:disabled { cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .role { display: grid; grid-template-columns: 64px 1fr auto; align-items: baseline; gap: 12px; width: 100%; padding: 8px 10px; border-radius: 6px; text-align: left; }
  .role:hover, .role.on { background: var(--raised); }
  .role-label { font-size: 12px; color: var(--faint); }
  .role-family { font-size: 20px; line-height: 1.3; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .role-action { font-size: 12px; color: var(--dim); }
  .role.on .role-action { color: var(--accent); }

  .error { margin: 0 14px 8px; color: var(--err); font-size: 12px; }

  .library { display: flex; align-items: center; gap: 10px; margin-top: auto; padding: 8px 14px; border-top: 1px solid var(--line); font-size: 12px; color: var(--dim); }
  .link { color: var(--accent); padding: 0; }
  .link:hover { text-decoration: underline; }
  progress { width: 120px; height: 4px; accent-color: var(--accent); }
</style>
