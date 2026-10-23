//! Download one family from Fontsource (Google Fonts as woff2 on jsDelivr) into `<root>/<id>/`.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{FontFile, Manifest};

pub struct Bases { pub api: String, pub cdn: String, pub npm: String }

fn live() -> Bases {
    Bases {
        api: "https://api.fontsource.org/v1".into(),
        cdn: "https://cdn.jsdelivr.net/fontsource/fonts".into(),
        npm: "https://cdn.jsdelivr.net/npm".into(),
    }
}

/// `GET /v1/fonts/<id>`; only the fields the download needs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyMeta {
    pub id: String,
    pub family: String,
    pub category: String,
    pub subsets: Vec<String>,
    pub def_subset: String,
    pub weights: Vec<u32>,
    pub styles: Vec<String>,
    pub variable: bool,
}

impl FamilyMeta {
    fn subset(&self) -> &str {
        if self.subsets.iter().any(|s| s == "latin") { "latin" } else { &self.def_subset }
    }
}

pub struct Planned { pub url: String, pub file: String, pub style: String, pub weight: String }

/// The api rejects requests without a User-Agent (403).
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("Stage/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// One file per style. `full` keeps custom axes (SOFT, WONK), `wght` is weight only, `standard` is every registered axis.
pub fn plan_variable(cdn: &str, meta: &FamilyMeta, axes: &Map<String, Value>) -> Vec<Planned> {
    let names: Vec<&str> = axes.keys().map(String::as_str).filter(|a| *a != "ital").collect();
    let kind = if names.iter().any(|a| a.chars().any(|c| c.is_ascii_uppercase())) { "full" }
        else if names == ["wght"] { "wght" } else { "standard" };
    let weight = axes.get("wght").map_or("400".to_string(), |w| {
        let s = |k: &str| w.get(k).and_then(Value::as_str).unwrap_or("400").to_string();
        format!("{} {}", s("min"), s("max"))
    });
    meta.styles.iter().map(|style| {
        let file = format!("{}-{kind}-{style}.woff2", meta.subset());
        Planned { url: format!("{cdn}/{}:vf@latest/{file}", meta.id), file, style: style.clone(), weight: weight.clone() }
    }).collect()
}

pub fn plan_static(cdn: &str, meta: &FamilyMeta) -> Vec<Planned> {
    meta.weights.iter().flat_map(|w| meta.styles.iter().map(move |style| {
        let file = format!("{}-{w}-{style}.woff2", meta.subset());
        Planned { url: format!("{cdn}/{}@latest/{file}", meta.id), file, style: style.clone(), weight: w.to_string() }
    })).collect()
}

enum Fetch { NotFound, Other(String) }

fn get(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, Fetch> {
    match agent.get(url).call() {
        Ok(mut r) => r.body_mut().with_config().limit(32 << 20).read_to_vec().map_err(|e| Fetch::Other(format!("{url}: {e}"))),
        Err(ureq::Error::StatusCode(404)) => Err(Fetch::NotFound),
        Err(e) => Err(Fetch::Other(format!("{url}: {e}"))),
    }
}

fn get_json<T: for<'a> Deserialize<'a>>(agent: &ureq::Agent, url: &str) -> Result<T, String> {
    let bytes = get(agent, url).map_err(|e| match e { Fetch::NotFound => format!("{url}: not found"), Fetch::Other(e) => e })?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{url}: {e}"))
}

fn installed(dir: &Path) -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(dir.join("family.json")).ok()?).ok()
}

pub fn download_family(agent: &ureq::Agent, id: &str, root: &Path) -> Result<Manifest, String> {
    download_family_from(agent, &live(), id, root)
}

/// Writes into a private `.part-*` folder and renames it into place, so an interrupted download never looks installed
/// and two downloads of the same family cannot corrupt each other.
pub fn download_family_from(agent: &ureq::Agent, b: &Bases, id: &str, root: &Path) -> Result<Manifest, String> {
    let dir = root.join(id);
    if let Some(m) = installed(&dir) {
        return Ok(m);
    }
    let meta: FamilyMeta = get_json(agent, &format!("{}/fonts/{id}", b.api))?;
    let mut got: Vec<(Planned, Vec<u8>)> = Vec::new();
    if meta.variable {
        let axes: Value = get_json(agent, &format!("{}/variable/{id}", b.api))?;
        let axes = axes.get("axes").and_then(Value::as_object).cloned().unwrap_or_default();
        for p in plan_variable(&b.cdn, &meta, &axes) {
            match get(agent, &p.url) {
                Ok(bytes) => got.push((p, bytes)),
                // Some families list a variable build the CDN does not have: use the static files instead.
                Err(Fetch::NotFound) => { got.clear(); break; }
                Err(Fetch::Other(e)) => return Err(e),
            }
        }
    }
    if got.is_empty() {
        for p in plan_static(&b.cdn, &meta) {
            match get(agent, &p.url) {
                Ok(bytes) => got.push((p, bytes)),
                Err(Fetch::NotFound) => {}
                Err(Fetch::Other(e)) => return Err(e),
            }
        }
    }
    if got.is_empty() {
        return Err(format!("{id}: no font files available"));
    }
    let license = get(agent, &format!("{}/@fontsource/{id}@latest/LICENSE", b.npm))
        .map_err(|e| match e { Fetch::NotFound => format!("{id}: no LICENSE"), Fetch::Other(e) => e })?;

    let m = Manifest {
        id: meta.id.clone(), family: meta.family.clone(), category: meta.category.clone(),
        files: got.iter().map(|(p, _)| FontFile { file: p.file.clone(), style: p.style.clone(), weight: p.weight.clone() }).collect(),
    };
    static N: AtomicU64 = AtomicU64::new(0);
    let part = root.join(format!("{id}.part-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&part)?;
        for (p, bytes) in &got { std::fs::write(part.join(&p.file), bytes)?; }
        std::fs::write(part.join("LICENSE"), &license)?;
        std::fs::write(part.join("family.json"), serde_json::to_vec_pretty(&m).unwrap())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&part);
        return Err(format!("{id}: {e}"));
    }
    if std::fs::rename(&part, &dir).is_err() {
        let _ = std::fs::remove_dir_all(&part);
        // Lost a race with another download of the same family: theirs is complete.
        return installed(&dir).ok_or_else(|| format!("{id}: cannot install into {}", dir.display()));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn meta(v: Value) -> FamilyMeta {
        serde_json::from_value(v).unwrap()
    }

    fn axes(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn files(p: &[Planned]) -> Vec<&str> {
        p.iter().map(|p| p.file.as_str()).collect()
    }

    const CDN: &str = "https://cdn.example/fonts";

    #[test]
    fn kind_rule() {
        let m = meta(json!({"id":"fraunces","family":"Fraunces","category":"serif","subsets":["latin","vietnamese"],"defSubset":"latin",
            "weights":[100,900],"styles":["italic","normal"],"variable":true}));
        let p = plan_variable(CDN, &m, &axes(json!({"ital":{"min":"0","max":"1"},"opsz":{"min":"9","max":"144"},
            "wght":{"min":"100","max":"900"},"SOFT":{"min":"0","max":"100"},"WONK":{"min":"0","max":"1"}})));
        assert_eq!(files(&p), ["latin-full-italic.woff2", "latin-full-normal.woff2"]);
        assert_eq!(p[0].weight, "100 900");
        assert_eq!(p[1].url, "https://cdn.example/fonts/fraunces:vf@latest/latin-full-normal.woff2");

        let m = meta(json!({"id":"newsreader","family":"Newsreader","category":"serif","subsets":["latin"],"defSubset":"latin",
            "weights":[200],"styles":["normal"],"variable":true}));
        let p = plan_variable(CDN, &m, &axes(json!({"opsz":{"min":"6","max":"72"},"wght":{"min":"200","max":"800"}})));
        assert_eq!(files(&p), ["latin-standard-normal.woff2"]);

        let p = plan_variable(CDN, &m, &axes(json!({"wght":{"min":"400","max":"900"}})));
        assert_eq!(files(&p), ["latin-wght-normal.woff2"]);
        assert_eq!(p[0].weight, "400 900");
    }

    #[test]
    fn subset_rule() {
        let m = meta(json!({"id":"catamaran","family":"Catamaran","category":"sans-serif","subsets":["tamil"],"defSubset":"tamil",
            "weights":[400,700],"styles":["normal"],"variable":false}));
        let p = plan_static(CDN, &m);
        assert_eq!(files(&p), ["tamil-400-normal.woff2", "tamil-700-normal.woff2"]);
        assert_eq!(p[1].url, "https://cdn.example/fonts/catamaran@latest/tamil-700-normal.woff2");
        assert_eq!(p[1].weight, "700");
    }

    /// Minimal HTTP server: path → body; missing paths 404. `truncate` makes every font response close mid-body.
    struct Stub { base: String, hits: Arc<AtomicUsize>, truncate: Arc<AtomicBool> }

    fn stub(routes: HashMap<String, Vec<u8>>) -> Stub {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let (hits, truncate) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        let (h, t, routes) = (hits.clone(), truncate.clone(), Arc::new(Mutex::new(routes)));
        std::thread::spawn(move || for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut line = String::new();
            let mut r = BufReader::new(s.try_clone().unwrap());
            r.read_line(&mut line).unwrap();
            loop { let mut x = String::new(); r.read_line(&mut x).unwrap(); if x.trim().is_empty() { break } }
            h.fetch_add(1, Ordering::SeqCst);
            let path = line.split(' ').nth(1).unwrap_or("").to_string();
            match routes.lock().unwrap().get(&path) {
                None => { let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"); }
                Some(body) if t.load(Ordering::SeqCst) && path.ends_with(".woff2") => {
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len() + 1000).as_bytes());
                    let _ = s.write_all(body);
                }
                Some(body) => {
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
                    let _ = s.write_all(body);
                }
            }
        });
        Stub { base, hits, truncate }
    }

    fn bases(s: &Stub) -> Bases {
        Bases { api: format!("{}/api", s.base), cdn: format!("{}/cdn", s.base), npm: format!("{}/npm", s.base) }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stage-fetch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn plan_falls_back_and_skips_missing() {
        let routes = HashMap::from([
            ("/api/fonts/trochut".into(), json!({"id":"trochut","family":"Trochut","category":"display","subsets":["latin"],
                "defSubset":"latin","weights":[400,700],"styles":["normal"],"variable":true}).to_string().into_bytes()),
            ("/api/variable/trochut".into(), json!({"axes":{"wght":{"min":"400","max":"700"}}}).to_string().into_bytes()),
            ("/cdn/trochut@latest/latin-400-normal.woff2".into(), b"font400".to_vec()),
            ("/npm/@fontsource/trochut@latest/LICENSE".into(), b"OFL".to_vec()),
        ]);
        let s = stub(routes);
        let root = tmp("fallback");
        let m = download_family_from(&agent(), &bases(&s), "trochut", &root).unwrap();
        assert_eq!(m.files, vec![FontFile { file: "latin-400-normal.woff2".into(), style: "normal".into(), weight: "400".into() }]);
        assert_eq!(std::fs::read(root.join("trochut/latin-400-normal.woff2")).unwrap(), b"font400");
        assert_eq!(std::fs::read(root.join("trochut/LICENSE")).unwrap(), b"OFL");
        let back: Manifest = serde_json::from_slice(&std::fs::read(root.join("trochut/family.json")).unwrap()).unwrap();
        assert_eq!(back, m);

        let s = stub(HashMap::from([("/api/fonts/none".into(), json!({"id":"none","family":"None","category":"serif","subsets":["latin"],
            "defSubset":"latin","weights":[400],"styles":["normal"],"variable":false}).to_string().into_bytes())]));
        assert_eq!(download_family_from(&agent(), &bases(&s), "none", &root).unwrap_err(), "none: no font files available");
        assert!(!root.join("none").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn download_is_atomic() {
        let routes = HashMap::from([
            ("/api/fonts/geist".into(), json!({"id":"geist","family":"Geist","category":"sans-serif","subsets":["latin"],
                "defSubset":"latin","weights":[400],"styles":["normal"],"variable":false}).to_string().into_bytes()),
            ("/cdn/geist@latest/latin-400-normal.woff2".into(), b"geist".to_vec()),
            ("/npm/@fontsource/geist@latest/LICENSE".into(), b"OFL".to_vec()),
        ]);
        let s = stub(routes);
        let root = tmp("atomic");
        s.truncate.store(true, Ordering::SeqCst);
        assert!(download_family_from(&agent(), &bases(&s), "geist", &root).is_err());
        assert!(!root.join("geist").exists());

        s.truncate.store(false, Ordering::SeqCst);
        let m = download_family_from(&agent(), &bases(&s), "geist", &root).unwrap();
        let before = s.hits.load(Ordering::SeqCst);
        assert_eq!(download_family_from(&agent(), &bases(&s), "geist", &root).unwrap(), m);
        assert_eq!(s.hits.load(Ordering::SeqCst), before, "installed family must not be fetched again");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    #[ignore = "network"]
    fn live_fraunces() {
        let root = tmp("live");
        let m = download_family(&agent(), "fraunces", &root).unwrap();
        assert_eq!(m.files.len(), 2);
        assert!(root.join("fraunces/LICENSE").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
