<script lang="ts">
  import { onMount } from "svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState } from "@codemirror/state";
  import { undo as cmUndo, redo as cmRedo, isolateHistory } from "@codemirror/commands";
  import { html } from "@codemirror/lang-html";
  import { json } from "@codemirror/lang-json";
  import { oneDark } from "@codemirror/theme-one-dark";

  let { lang, onchange }: { lang: "html" | "json"; onchange: (text: string) => void } = $props();

  let host: HTMLDivElement;
  let view: EditorView;

  const extensions = () => [
    basicSetup,
    lang === "html" ? html() : json(),
    // Listed before oneDark so it wins: keep oneDark's syntax colours, use the app's surfaces.
    EditorView.theme({
      "&": { height: "100%", backgroundColor: "var(--panel)", fontSize: "12.5px" },
      ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.6" },
      ".cm-gutters": { backgroundColor: "var(--panel)", color: "var(--faint)", border: "none" },
      ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "#ffffff08" },
      ".cm-activeLineGutter": { color: "var(--dim)" },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "#e9783e33 !important" },
      ".cm-cursor": { borderLeftColor: "var(--accent)" },
      ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 16px" },
    }, { dark: true }),
    oneDark,
    EditorView.updateListener.of((u) => { if (u.docChanged) onchange(u.state.doc.toString()); }),
  ];

  onMount(() => {
    view = new EditorView({ parent: host, extensions: extensions() });
    return () => view.destroy();
  });

  /** Replace the document with a freshly opened deck's text; resets undo history. */
  export function load(text: string) {
    view.setState(EditorState.create({ doc: text, extensions: extensions() }));
  }

  /** Swap in new text as its own undo step, touching only the span that changed so the cursor stays put. */
  export function replaceAll(text: string) {
    const old = view.state.doc.toString();
    let a = 0, b = 0;
    while (a < old.length && a < text.length && old[a] === text[a]) a++;
    while (b < old.length - a && b < text.length - a && old[old.length - 1 - b] === text[text.length - 1 - b]) b++;
    view.dispatch({ changes: { from: a, to: old.length - b, insert: text.slice(a, text.length - b) }, annotations: isolateHistory.of("full") });
  }

  export function undo() { cmUndo(view); }
  export function redo() { cmRedo(view); }

  export function goto(line: number) {
    const l = view.state.doc.line(Math.min(Math.max(line, 1), view.state.doc.lines));
    view.dispatch({ selection: { anchor: l.from }, effects: EditorView.scrollIntoView(l.from, { y: "center" }) });
    view.focus();
  }
</script>

<div class="cm-host" bind:this={host}></div>

<style>
  .cm-host { height: 100%; overflow: hidden; }
</style>
