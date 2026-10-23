# Stage

A small native app that opens `.stage` presentation decks, presents them, and edits them. Built on Tauri, so it uses the
system webview (WebView2 on Windows, WebKit on macOS) instead of bundling a browser.

## The `.stage` format

A `.stage` file is a zip archive:

| entry | what |
|---|---|
| `deck.src.html` | slides, theme CSS, notes, optional hooks (stagecraft source) |
| `brief.json` | sources, claim ledger, outline, motion concept |
| `assets/` | images, video, fonts used by the deck |
| `sources/` | files the claim quotes are verified against (txt, md, code, docx, pptx, pdf) |

Source `ref` paths in `brief.json` resolve inside the zip. The app injects the engine at load and runs the stagecraft
lint; a deck with lint errors cannot be presented until they are fixed.

## Develop

```bash
npm install
npm run tauri dev
```

Tests: `cargo test` in `src-tauri`.

## License

[PolyForm Noncommercial 1.0.0](LICENSE). Commercial use needs a paid license, see [COMMERCIAL.md](COMMERCIAL.md).
