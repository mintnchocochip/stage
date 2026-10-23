//! Fonts: curated families compiled in, the rest downloaded on demand, and the ones a deck uses embedded in its zip.

pub mod css;
pub mod fetch;
mod bundled;

pub use bundled::BUNDLED;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::build::lint::{banned_font, BANNED_FONTS};
use crate::stage_file::Deck;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FontFile {
    pub file: String,
    pub style: String,
    /// "400", or "100 900" for a variable weight range.
    pub weight: String,
}

/// `family.json` in each family folder.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub family: String,
    pub category: String,
    pub files: Vec<FontFile>,
}

pub const ROLES: &[&str] = &["display", "body", "mono"];

const CATALOG: &str = include_str!("../../../engine/fonts/catalog.json");

#[derive(Deserialize)]
struct CatalogRow { id: String, family: String, category: String }

#[derive(Serialize)]
pub struct CatalogEntry {
    pub id: String,
    pub family: String,
    pub category: String,
    pub installed: bool,
    pub bundled: bool,
    /// Lint rejects it (unless the deck allows it with kx-allow-fonts).
    pub blocked: bool,
}

static ROWS: LazyLock<Vec<CatalogRow>> = LazyLock::new(|| serde_json::from_str(CATALOG).expect("bundled catalog.json"));
static IDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| ROWS.iter().map(|r| r.id.as_str()).collect());

/// Whether `id` is a Google family Stage knows. Ids become folder names and URL paths, so nothing else is fetched.
pub fn in_catalog(id: &str) -> bool {
    IDS.contains(id)
}

fn bundled_file(id: &str, file: &str) -> Option<&'static [u8]> {
    let key = format!("{id}/{file}");
    BUNDLED.iter().find(|(k, _)| *k == key).map(|(_, b)| *b)
}

/// Ids and file names come from URLs and decks: plain names only, never paths.
fn plain(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', ':'])
}

/// Font families Stage can embed: compiled-in ones, then downloads under `root`. `Store::default()` is bundled-only.
#[derive(Default, Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn manifest(&self, id: &str) -> Option<Manifest> {
        serde_json::from_slice(&self.file(id, "family.json")?).ok()
    }

    pub fn file(&self, id: &str, file: &str) -> Option<Vec<u8>> {
        if !plain(id) || !plain(file) {
            return None;
        }
        if let Some(b) = bundled_file(id, file) {
            return Some(b.to_vec());
        }
        if self.root.as_os_str().is_empty() {
            return None;
        }
        std::fs::read(self.root.join(id).join(file)).ok()
    }

    fn downloaded(&self, id: &str) -> bool {
        !self.root.as_os_str().is_empty() && self.root.join(id).join("family.json").is_file()
    }

    pub fn catalog(&self) -> Vec<CatalogEntry> {
        ROWS.iter().map(|r| {
            let bundled = bundled_file(&r.id, "family.json").is_some();
            CatalogEntry {
                installed: bundled || self.downloaded(&r.id),
                blocked: BANNED_FONTS.iter().any(|f| banned_font(f, std::slice::from_ref(&r.family))),
                bundled, id: r.id.clone(), family: r.family.clone(), category: r.category.clone(),
            }
        }).collect()
    }

    pub fn installed_count(&self) -> usize {
        self.catalog().iter().filter(|c| c.installed).count()
    }
}

/// Copy a family into the deck under `assets/fonts/<id>/`. `family.json` goes too: it marks the folder as Stage's to prune.
pub fn embed(deck: &mut Deck, store: &Store, id: &str) -> Result<Manifest, String> {
    let m = store.manifest(id).ok_or_else(|| format!("font '{id}' is not installed"))?;
    for name in m.files.iter().map(|f| f.file.as_str()).chain(["LICENSE", "family.json"]) {
        let bytes = store.file(id, name).ok_or_else(|| format!("font '{id}' is missing {name}"))?;
        deck.set(&format!("assets/fonts/{id}/{name}"), bytes);
    }
    Ok(m)
}

/// Run `fetch` over `ids` on `workers` threads, checking `cancel` between families. `progress(done, failed)` after each
/// one. Returns (done, failed); a cancelled run just stops early, and the next run skips what is installed.
pub fn download_all(
    ids: &[String], workers: usize, cancel: &AtomicBool,
    fetch: impl Fn(&str) -> Result<(), String> + Sync, progress: impl Fn(usize, usize) + Sync,
) -> (usize, usize) {
    let (next, done, failed) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
    std::thread::scope(|s| for _ in 0..workers {
        s.spawn(|| while !cancel.load(Ordering::SeqCst) {
            let Some(id) = ids.get(next.fetch_add(1, Ordering::SeqCst)) else { break };
            match fetch(id) {
                Ok(()) => done.fetch_add(1, Ordering::SeqCst),
                Err(_) => failed.fetch_add(1, Ordering::SeqCst),
            };
            progress(done.load(Ordering::SeqCst), failed.load(Ordering::SeqCst));
        });
    });
    (done.into_inner(), failed.into_inner())
}

/// Make the zip match the source on save: drop families it no longer uses, and re-embed referenced ones that went
/// missing (redo after a prune, or a kx-fonts block pasted from another deck) when the store has them.
pub fn sync_embedded(deck: &mut Deck, store: &Store, src: &str) {
    prune(deck, src);
    for id in css::fonts_in_use(src) {
        if deck.get(&format!("assets/fonts/{id}/family.json")).is_none() && deck.get(&format!("assets/fonts/{id}/LICENSE")).is_none() {
            let _ = embed(deck, store, &id);
        }
    }
}

/// Drop embedded families the source no longer references. Folders without `family.json` were added by hand: kept.
pub fn prune(deck: &mut Deck, src: &str) {
    let used = css::fonts_in_use(src);
    let managed: Vec<String> = deck.names()
        .filter_map(|n| n.strip_prefix("assets/fonts/")?.strip_suffix("/family.json").map(str::to_string))
        .filter(|id| plain(id) && !used.contains(id))
        .collect();
    for id in managed {
        let prefix = format!("assets/fonts/{id}/");
        deck.retain(|n| !n.starts_with(&prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::lint::lint;
    use crate::stage_file::{Deck, SRC};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stage-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn example_deck_embeds_its_fonts() {
        let d = Deck::example();
        let src = d.text(SRC).unwrap();
        assert!(!src.contains("fonts.googleapis.com"));
        let urls: Vec<&str> = src.match_indices("url(assets/fonts/").map(|(i, _)| { let s = &src[i + 4..]; &s[..s.find(')').unwrap()] }).collect();
        assert!(urls.len() >= 3, "{urls:?}");
        for u in urls { assert!(d.get(u).is_some(), "{u} missing from deck"); }
        for id in ["fraunces", "instrument-sans", "jetbrains-mono"] {
            assert!(d.get(&format!("assets/fonts/{id}/LICENSE")).is_some(), "{id} LICENSE");
        }
        assert!(lint(&src, None, &d).errors.iter().all(|e| !e.message.starts_with("font:")));
    }

    #[test]
    fn prune_drops_unreferenced_families() {
        let mut d = Deck::example();
        embed(&mut d, &Store::default(), "geist").unwrap();
        d.set("assets/fonts/brand.woff2", vec![1]);
        d.set("assets/fonts/Brand Sans/regular.woff2", vec![2]);
        assert!(d.get("assets/fonts/geist/family.json").is_some());
        let src = d.text(SRC).unwrap();
        prune(&mut d, &src);
        assert!(d.names().all(|n| !n.starts_with("assets/fonts/geist/")));
        assert!(d.get("assets/fonts/fraunces/LICENSE").is_some());
        assert!(d.get("assets/fonts/brand.woff2").is_some() && d.get("assets/fonts/Brand Sans/regular.woff2").is_some(),
            "hand-added fonts are never pruned");
    }

    #[test]
    fn sync_restores_families_redo_brings_back() {
        let mut d = Deck::example();
        let original = d.text(SRC).unwrap();
        let m = embed(&mut d, &Store::default(), "geist").unwrap();
        let with_geist = css::apply_font(&original, "body", &m).unwrap();
        sync_embedded(&mut d, &Store::default(), &original); // saved after undo: geist pruned
        assert!(d.get("assets/fonts/geist/family.json").is_none());
        sync_embedded(&mut d, &Store::default(), &with_geist); // saved after redo
        assert!(d.get("assets/fonts/geist/LICENSE").is_some(), "redo must bring the files back");
    }

    #[test]
    fn catalog_flags() {
        let root = tmp("catalog");
        let store = Store { root: root.clone() };
        let cat = store.catalog();
        assert_eq!(cat.len(), 1984);
        let f = cat.iter().find(|c| c.id == "fraunces").unwrap();
        assert!(f.bundled && f.installed && !f.blocked);
        let i = cat.iter().find(|c| c.id == "inter").unwrap();
        assert!(i.blocked && !i.installed && !i.bundled);
        for fam in ["Roboto Mono", "Inter Tight", "Montserrat Alternates", "Roboto"] {
            assert!(cat.iter().find(|c| c.family == fam).unwrap().blocked, "{fam} is rejected by lint, so it must be flagged");
        }
        assert!(!cat.iter().find(|c| c.family == "Arimo").unwrap().blocked);
        assert_eq!(store.installed_count(), 30);

        std::fs::create_dir_all(root.join("inter")).unwrap();
        std::fs::write(root.join("inter/family.json"), r#"{"id":"inter","family":"Inter","category":"sans-serif","files":[]}"#).unwrap();
        std::fs::create_dir_all(root.join("roboto.part-1-0")).unwrap();
        assert!(store.catalog().iter().find(|c| c.id == "inter").unwrap().installed);
        assert_eq!(store.installed_count(), 31);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn download_all_counts_and_cancels() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let ids: Vec<String> = (0..20).map(|i| format!("f{i}")).collect();
        let cancel = AtomicBool::new(false);
        let calls = AtomicUsize::new(0);
        let (done, failed) = download_all(&ids, 4, &cancel, |id| {
            calls.fetch_add(1, Ordering::SeqCst);
            if id == "f3" { Err("boom".into()) } else { Ok(()) }
        }, |_, _| {});
        assert_eq!((done, failed, calls.load(Ordering::SeqCst)), (19, 1, 20));

        let calls = AtomicUsize::new(0);
        let (done, failed) = download_all(&ids, 1, &cancel, |_| {
            if calls.fetch_add(1, Ordering::SeqCst) == 4 { cancel.store(true, Ordering::SeqCst); }
            Ok(())
        }, |_, _| {});
        assert_eq!((done, failed), (5, 0), "stops between families once cancelled");
    }

    #[test]
    fn in_catalog_is_exact() {
        assert!(in_catalog("fraunces") && in_catalog("roboto-mono"));
        assert!(!in_catalog("../fraunces") && !in_catalog("Fraunces") && !in_catalog(""));
    }

    #[test]
    fn store_rejects_path_tricks() {
        let s = Store::default();
        assert!(s.file("fraunces", "LICENSE").is_some());
        for (id, f) in [("..", "Cargo.toml"), ("fraunces", "../geist/LICENSE"), ("fraunces/..", "LICENSE"), ("c:", "x")] {
            assert!(s.file(id, f).is_none(), "{id}/{f}");
        }
    }
}
