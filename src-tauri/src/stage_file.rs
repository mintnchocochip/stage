//! `.stage` file: a zip of deck.src.html, brief.json, assets/ and sources/.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

pub const SRC: &str = "deck.src.html";
pub const BRIEF: &str = "brief.json";

#[derive(Default)]
pub struct Deck {
    pub path: Option<PathBuf>,
    /// ponytail: whole archive held in memory; stream entries if decks with large video become common.
    entries: BTreeMap<String, Vec<u8>>,
}

impl Deck {
    pub fn open(path: &Path) -> Result<Deck, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut deck = Deck::from_zip(&bytes).map_err(|e| format!("{} is not a valid .stage file: {e}", path.display()))?;
        if deck.get(SRC).is_none() {
            return Err(format!("{} has no {SRC}", path.display()));
        }
        deck.path = Some(path.to_path_buf());
        Ok(deck)
    }

    pub fn from_zip(bytes: &[u8]) -> Result<Deck, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let mut entries = BTreeMap::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            if f.is_dir() { continue; }
            // enclosed_name rejects absolute and ../ paths (zip slip); such entries are skipped.
            let Some(name) = f.enclosed_name() else { continue };
            let name = name.to_string_lossy().replace('\\', "/");
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            entries.insert(name, buf);
        }
        Ok(Deck { path: None, entries })
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries.get(name).map(Vec::as_slice)
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.get(name).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn set(&mut self, name: &str, bytes: Vec<u8>) {
        self.entries.insert(name.to_string(), bytes);
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.entries.retain(|k, _| keep(k));
    }

    pub fn to_zip(&self) -> Result<Vec<u8>, String> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in &self.entries {
            w.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
            w.write_all(bytes).map_err(|e| e.to_string())?;
        }
        Ok(w.finish().map_err(|e| e.to_string())?.into_inner())
    }

    /// Write to a temp file next to the target, then rename over it, so a crash mid-save never corrupts the deck.
    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let bytes = self.to_zip()?;
        let tmp = path.with_extension("stage.tmp");
        let mut f = File::create(&tmp).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        f.write_all(&bytes).and_then(|_| f.sync_all()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        drop(f);
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("cannot replace {}: {e}", path.display())
        })?;
        self.path = Some(path.to_path_buf());
        Ok(())
    }

    /// The bundled example deck, with its claim sources packed under sources/.
    pub fn example() -> Deck {
        use crate::build::assemble::{ENGINE_CSS, ENGINE_JS};
        let mut d = Deck::default();
        d.set(SRC, include_bytes!("../../engine/example/deck.src.html").to_vec());
        d.set(BRIEF, include_bytes!("../../engine/example/brief.json").to_vec());
        d.set("sources/build.py", include_bytes!("../../engine/example/sources/build.py").to_vec());
        d.set("sources/engine.js", ENGINE_JS.as_bytes().to_vec());
        d.set("sources/engine.css", ENGINE_CSS.as_bytes().to_vec());
        for id in ["fraunces", "instrument-sans", "jetbrains-mono"] {
            crate::fonts::embed(&mut d, &crate::fonts::Store::default(), id).expect("example fonts are bundled");
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_round_trip_and_atomic_save() {
        let dir = std::env::temp_dir().join(format!("stage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.stage");
        let mut d = Deck::example();
        d.save_as(&path).unwrap();
        d.set(SRC, b"<html>changed</html>".to_vec());
        d.save_as(&path).unwrap();
        assert!(!path.with_extension("stage.tmp").exists());
        let back = Deck::open(&path).unwrap();
        assert_eq!(back.text(SRC).unwrap(), "<html>changed</html>");
        assert!(back.get("sources/build.py").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
