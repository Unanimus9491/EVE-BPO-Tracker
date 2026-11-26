//! Data layer: blueprint list (built from EVE's Static Data Export), the user's
//! collection, and the data folder. File formats match the earlier Python version,
//! so existing `collection.json`, `bpos.json` and the icon cache carry straight over.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const APP_NAME: &str = "EVE BPO Tracker";
/// From Cargo.toml at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SDE_URL: &str =
    "https://developers.eveonline.com/static-data/eve-online-static-data-latest-jsonl.zip";
pub const USER_AGENT: &str = "EVE-BPO-Collection-Tracker/2.0 (personal collection tool)";
pub const ME_MAX: u8 = 10;
pub const TE_MAX: u8 = 20;

pub const TABS: [&str; 9] = [
    "All", "Ships", "Modules", "Charges", "Drones", "Structures", "Components", "Reactions", "Other",
];

/// Product category (from the SDE) -> tab in the tracker.
pub fn tab_for(kind: &str, category: &str) -> &'static str {
    if kind == "reaction" {
        return "Reactions";
    }
    match category {
        "Ship" => "Ships",
        "Module" | "Subsystem" => "Modules", // T3 subsystems fit like modules
        "Charge" => "Charges",
        "Drone" | "Fighter" => "Drones",
        "Structure" | "Structure Module" | "Starbase" | "Deployable" | "Orbitals"
        | "Sovereignty Structures" | "Infrastructure Upgrades" => "Structures",
        "Commodity" | "Material" => "Components",
        _ => "Other",
    }
}

/// Ship groups in in-game ship tree order, read along each line of the tree:
/// the Tech I hull first (Navy hulls share its group), then its branches, nearest first.
/// Empire lines come first, then capitals, haulers and the ORE (mining) tree.
/// The SDE has no layout for the ship tree, so this list is maintained by hand.
/// Groups not listed here go after these, alphabetically.
pub const SHIP_TREE_ORDER: &[&str] = &[
    // starter hulls
    "Corvette", "Shuttle",
    // frigate line
    "Frigate", "Interceptor", "Assault Frigate", "Covert Ops", "Stealth Bomber",
    "Electronic Attack Ship", "Logistics Frigate", "Prototype Exploration Ship",
    // destroyer line
    "Destroyer", "Interdictor", "Command Destroyer", "Tactical Destroyer",
    // cruiser line
    "Cruiser", "Combat Recon Ship", "Force Recon Ship", "Heavy Assault Cruiser",
    "Heavy Interdiction Cruiser", "Logistics", "Strategic Cruiser", "Flag Cruiser",
    // battlecruiser line
    "Combat Battlecruiser", "Attack Battlecruiser", "Command Ship",
    // battleship line
    "Battleship", "Black Ops", "Marauder",
    // capitals
    // (command carriers share the Carrier group)
    "Dreadnought", "Lancer Dreadnought", "Carrier", "Supercarrier", "Force Auxiliary", "Titan",
    // empire haulers
    "Hauler", "Industrial", "Deep Space Transport", "Blockade Runner", "Freighter", "Jump Freighter",
    // ORE (mining) tree; mining destroyers share the Destroyer group, the ORE hauler shares Hauler
    "Mining Frigate", "Expedition Frigate", "Mining Barge", "Exhumer",
    "Industrial Command Ship", "Capital Industrial Ship",
];

/// Sort position of a group: ship groups follow the ship tree, everything else is alphabetical.
pub fn group_rank(category: &str, group: &str) -> usize {
    if category == "Ship" {
        SHIP_TREE_ORDER.iter().position(|g| *g == group).unwrap_or(SHIP_TREE_ORDER.len())
    } else {
        0
    }
}

fn meta_group_name(id: u32) -> Option<&'static str> {
    Some(match id {
        1 => "Tech I",
        2 => "Tech II",
        3 => "Storyline",
        4 => "Faction",
        5 => "Officer",
        6 => "Deadspace",
        14 => "Tech III",
        _ => return None,
    })
}

// ----------------------------------------------------------------------------- paths
pub fn data_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
    .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join(APP_NAME);
    let _ = fs::create_dir_all(dir.join("icons"));
    dir
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

// ----------------------------------------------------------------------------- blueprint entries
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u32,
    pub name: String,
    pub product_id: u32,
    #[serde(default)]
    pub product_name: String,
    #[serde(default)]
    pub tab: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub tech: String,
    #[serde(default)]
    pub market: bool,
    #[serde(default)]
    pub kind: String,
    #[serde(default = "yes")]
    pub researchable: bool,
}
fn yes() -> bool {
    true
}

impl Entry {
    /// Card title: drop the " Blueprint" / " Reaction Formula" suffix.
    pub fn short_name(&self) -> &str {
        self.name
            .strip_suffix(" Blueprint")
            .or_else(|| self.name.strip_suffix(" Reaction Formula"))
            .unwrap_or(&self.name)
    }
}

pub fn load_entries(path: &Path) -> Option<Vec<Entry>> {
    let text = fs::read_to_string(path).ok()?;
    let mut entries: Vec<Entry> = serde_json::from_str(&text).ok()?;
    for e in &mut entries {
        // Apply current tab rules, so changes don't need a data rebuild.
        e.tab = tab_for(&e.kind, &e.category).to_string();
    }
    Some(entries)
}

// ----------------------------------------------------------------------------- collection
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    #[serde(skip_serializing_if = "is_false")]
    pub owned: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub qty: u32,
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub me: u8,
    #[serde(skip_serializing_if = "is_zero_u8")]
    pub te: u8,
    #[serde(skip_serializing_if = "is_false")]
    pub fav: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub ignored: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Owned/qty/ME/TE came from an EVE character sync (and will be updated by the next one).
    #[serde(skip_serializing_if = "is_false")]
    pub synced: bool,
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
fn is_zero_u8(n: &u8) -> bool {
    *n == 0
}

impl State {
    pub fn fully_researched(&self) -> bool {
        self.owned && self.me >= ME_MAX && self.te >= TE_MAX
    }
}

pub enum Edit {
    Owned(bool),
    Qty(u32),
    Me(u8),
    Te(u8),
    MaxResearch,
    ResetResearch,
    Fav(bool),
    Ignored(bool),
    Notes(String),
}

pub struct Collection {
    path: PathBuf,
    /// Keyed by blueprint type ID (written as strings in JSON, as before).
    pub states: BTreeMap<u32, State>,
    default: State,
    dirty_since: Option<Instant>,
    /// Bumped on every change, so views can tell when to refresh.
    pub revision: u64,
}

impl Collection {
    pub fn load(path: PathBuf) -> Self {
        let mut states = BTreeMap::new();
        if let Ok(text) = fs::read_to_string(&path) {
            match serde_json::from_str(&text) {
                Ok(s) => states = s,
                Err(_) => {
                    let _ = fs::copy(&path, path.with_extension("corrupt.json"));
                }
            }
        }
        Self { path, states, default: State::default(), dirty_since: None, revision: 0 }
    }

    pub fn get(&self, id: u32) -> &State {
        self.states.get(&id).unwrap_or(&self.default)
    }

    pub fn edit(&mut self, id: u32, edit: Edit) {
        let mut s = self.get(id).clone();
        match edit {
            Edit::Owned(v) => {
                s.owned = v;
                if v && s.qty < 1 {
                    s.qty = 1;
                }
                if !v {
                    s.qty = 0;
                }
            }
            Edit::Qty(q) => {
                s.qty = q;
                s.owned = q > 0;
            }
            Edit::Me(v) => s.me = v,
            Edit::Te(v) => s.te = v,
            Edit::MaxResearch => {
                s.owned = true;
                s.qty = s.qty.max(1);
                s.me = ME_MAX;
                s.te = TE_MAX;
            }
            Edit::ResetResearch => {
                s.me = 0;
                s.te = 0;
            }
            Edit::Fav(v) => s.fav = v,
            Edit::Ignored(v) => s.ignored = v,
            Edit::Notes(n) => s.notes = n,
        }
        if !s.owned {
            s.me = 0;
            s.te = 0;
            s.synced = false;
        }
        s.me = s.me.min(ME_MAX);
        s.te = s.te.min(TE_MAX);
        if s == State::default() {
            self.states.remove(&id);
        } else {
            self.states.insert(id, s);
        }
        self.revision += 1;
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    /// Apply blueprint originals read from EVE. Synced data wins for what EVE reports;
    /// anything a previous sync marked owned that's no longer there is marked missing.
    /// Manually ticked blueprints, favourites, notes and ignores are left alone.
    /// Returns (newly owned, updated, no longer owned).
    /// `keep`: blueprint types EVE didn't list because they're in a research or copy job.
    pub fn apply_sync(
        &mut self,
        owned: &HashMap<u32, crate::esi::Owned>,
        keep: &std::collections::HashSet<u32>,
    ) -> (usize, usize, usize) {
        let (mut added, mut changed, mut removed) = (0, 0, 0);
        for (id, o) in owned {
            let old = self.get(*id).clone();
            let mut s = old.clone();
            s.owned = true;
            s.qty = o.qty.max(1);
            s.me = o.me.min(ME_MAX);
            s.te = o.te.min(TE_MAX);
            s.synced = true;
            if s != old {
                if old.owned { changed += 1 } else { added += 1 }
                self.states.insert(*id, s);
            }
        }
        let gone: Vec<u32> =
            self.states
            .iter()
            .filter(|(id, s)| s.synced && !owned.contains_key(id) && !keep.contains(id))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            if let Some(s) = self.states.get_mut(&id) {
                s.owned = false;
                s.qty = 0;
                s.me = 0;
                s.te = 0;
                s.synced = false;
                removed += 1;
                if *s == State::default() {
                    self.states.remove(&id);
                }
            }
        }
        self.revision += 1;
        self.dirty_since.get_or_insert_with(Instant::now);
        (added, changed, removed)
    }

    pub fn replace_all(&mut self, states: BTreeMap<u32, State>) {
        self.states = states;
        self.revision += 1;
        self.save();
    }

    pub fn save(&mut self) {
        if let Ok(json) = serde_json::to_vec_pretty(&self.states) {
            let _ = atomic_write(&self.path, &json);
        }
        self.dirty_since = None;
    }

    /// Save a little after the last change, so rapid clicking doesn't hammer the disk.
    pub fn save_if_due(&mut self) {
        if self.dirty_since.is_some_and(|t| t.elapsed().as_millis() > 400) {
            self.save();
        }
    }

    pub fn has_unsaved(&self) -> bool {
        self.dirty_since.is_some()
    }
}

// ----------------------------------------------------------------------------- SDE builder
#[derive(Default)]
pub struct BuildProgress {
    pub message: String,
    pub done: u64,
    pub total: u64,
    pub result: Option<Result<Vec<Entry>, String>>,
}
pub type Progress = Arc<Mutex<BuildProgress>>;

fn report(p: &Progress, msg: &str, done: u64, total: u64) {
    if let Ok(mut g) = p.lock() {
        g.message = msg.to_string();
        g.done = done;
        g.total = total;
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Name {
    Langs(HashMap<String, String>),
    Plain(String),
}
impl Name {
    fn en(self) -> String {
        match self {
            Name::Plain(s) => s,
            Name::Langs(mut m) => m
                .remove("en")
                .or_else(|| m.into_values().next())
                .unwrap_or_default(),
        }
    }
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct NamedRow {
    _key: u32,
    name: Option<Name>,
    categoryID: Option<u32>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct TypeRow {
    _key: u32,
    name: Option<Name>,
    groupID: Option<u32>,
    #[serde(default)]
    published: bool,
    marketGroupID: Option<u32>,
    metaGroupID: Option<u32>,
    techLevel: Option<u32>,
}

struct TypeInfo {
    name: String,
    group_id: Option<u32>,
    published: bool,
    market: bool,
    meta_group: Option<u32>,
    tech_level: Option<u32>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct BlueprintRow {
    _key: u32,
    #[serde(default)]
    activities: HashMap<String, Activity>,
}
#[derive(Deserialize, Default)]
struct Activity {
    #[serde(default)]
    products: Vec<Product>,
}
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct Product {
    typeID: u32,
}

fn for_each_line<R: Read>(reader: R, mut f: impl FnMut(&str)) -> std::io::Result<()> {
    for line in BufReader::new(reader).lines() {
        let line = line?;
        let line = line.trim();
        if !line.is_empty() {
            f(line);
        }
    }
    Ok(())
}

fn open_jsonl<'a, R: Read + std::io::Seek>(
    zip: &'a mut zip::ZipArchive<R>,
    basename: &str,
) -> Result<zip::read::ZipFile<'a, R>, String> {
    let name = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.to_string()))
        .find(|n| n.rsplit('/').next() == Some(basename))
        .ok_or_else(|| format!("{basename} not found in the SDE archive"))?;
    zip.by_name(&name).map_err(|e| e.to_string())
}

pub fn build_dataset(zip_path: &Path, p: &Progress) -> Result<Vec<Entry>, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("Couldn't open {}: {e}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Not a valid zip: {e}"))?;
    let io = |e: std::io::Error| e.to_string();

    report(p, "Reading categories…", 0, 5);
    let mut categories: HashMap<u32, String> = HashMap::new();
    for_each_line(open_jsonl(&mut zip, "categories.jsonl")?, |l| {
        if let Ok(r) = serde_json::from_str::<NamedRow>(l) {
            categories.insert(r._key, r.name.map(Name::en).unwrap_or_default());
        }
    })
    .map_err(io)?;

    report(p, "Reading groups…", 1, 5);
    let mut groups: HashMap<u32, (String, Option<u32>)> = HashMap::new();
    for_each_line(open_jsonl(&mut zip, "groups.jsonl")?, |l| {
        if let Ok(r) = serde_json::from_str::<NamedRow>(l) {
            groups.insert(r._key, (r.name.map(Name::en).unwrap_or_default(), r.categoryID));
        }
    })
    .map_err(io)?;

    report(p, "Reading item types…", 2, 5);
    let mut types: HashMap<u32, TypeInfo> = HashMap::new();
    for_each_line(open_jsonl(&mut zip, "types.jsonl")?, |l| {
        if let Ok(r) = serde_json::from_str::<TypeRow>(l) {
            types.insert(
                r._key,
                TypeInfo {
                    name: r.name.map(Name::en).unwrap_or_default(),
                    group_id: r.groupID,
                    published: r.published,
                    market: r.marketGroupID.is_some(),
                    meta_group: r.metaGroupID,
                    tech_level: r.techLevel,
                },
            );
        }
    })
    .map_err(io)?;

    report(p, "Reading blueprints…", 3, 5);
    let mut out = Vec::new();
    for_each_line(open_jsonl(&mut zip, "blueprints.jsonl")?, |l| {
        let Ok(bp) = serde_json::from_str::<BlueprintRow>(l) else { return };
        let Some(bpt) = types.get(&bp._key).filter(|t| t.published) else { return };
        let (kind, act) = if let Some(a) = bp.activities.get("manufacturing").filter(|a| !a.products.is_empty()) {
            ("manufacturing", a)
        } else if let Some(a) = bp.activities.get("reaction").filter(|a| !a.products.is_empty()) {
            ("reaction", a)
        } else {
            return;
        };
        let prod_id = act.products[0].typeID;
        let prod = types.get(&prod_id);
        let (group, cat_id) = prod
            .and_then(|t| t.group_id)
            .and_then(|g| groups.get(&g).cloned())
            .unwrap_or_else(|| ("Unknown".into(), None));
        let category = cat_id.and_then(|c| categories.get(&c).cloned()).unwrap_or_else(|| "Unknown".into());
        let tech = prod
            .and_then(|t| t.meta_group)
            .and_then(meta_group_name)
            .map(str::to_string)
            .unwrap_or_else(|| {
                let tl = prod.and_then(|t| t.tech_level).unwrap_or(1).clamp(1, 3);
                format!("Tech {}", ["I", "II", "III"][tl as usize - 1])
            });
        out.push(Entry {
            id: bp._key,
            name: bpt.name.clone(),
            product_id: prod_id,
            product_name: prod.map(|t| t.name.clone()).unwrap_or_default(),
            tab: tab_for(kind, &category).to_string(),
            category,
            group,
            tech,
            market: bpt.market,
            kind: kind.to_string(),
            researchable: bp.activities.contains_key("research_material")
                || bp.activities.contains_key("research_time"),
        });
    })
    .map_err(io)?;
    report(p, "Done", 5, 5);
    out.sort_by_key(|e| e.name.to_lowercase());
    Ok(out)
}

pub fn download_sde(dest: &Path, p: &Progress) -> Result<(), String> {
    let mut resp = ureq::get(SDE_URL)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.body_mut().as_reader();
    let mut out = fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        report(p, "Downloading game data…", done, total);
    }
    Ok(())
}

/// Runs a build on a background thread. `zip_path = None` downloads the latest SDE first.
pub fn spawn_build(zip_path: Option<PathBuf>, ctx: eframe::egui::Context) -> Progress {
    let progress: Progress = Arc::new(Mutex::new(BuildProgress {
        message: "Starting…".into(),
        ..Default::default()
    }));
    let p = progress.clone();
    std::thread::spawn(move || {
        let result = match zip_path {
                Some(path) => build_dataset(&path, &p),
                None => {
                    let tmp = std::env::temp_dir().join("eve-bpo-tracker-sde.zip");
                    let r = download_sde(&tmp, &p).and_then(|_| build_dataset(&tmp, &p));
                    let _ = fs::remove_file(&tmp);
                    r
                }
            };
        if let Ok(mut g) = p.lock() {
            g.result = Some(result);
        }
        ctx.request_repaint();
    });
    progress
}

// ----------------------------------------------------------------------------- CSV export
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn export_csv(path: &Path, entries: &[Entry], coll: &Collection) -> std::io::Result<()> {
    let mut out = String::from(
        "Blueprint,Type ID,Product,Tab,Group,Tech,Market listed,Owned,Quantity,ME,TE,Favourite,Ignored,Notes\r\n",
    );
    for e in entries {
        let s = coll.get(e.id);
        let row = [
            csv_field(&e.name),
            e.id.to_string(),
            csv_field(&e.product_name),
            csv_field(&e.tab),
            csv_field(&e.group),
            csv_field(&e.tech),
            e.market.to_string(),
            s.owned.to_string(),
            s.qty.to_string(),
            s.me.to_string(),
            s.te.to_string(),
            s.fav.to_string(),
            s.ignored.to_string(),
            csv_field(&s.notes),
        ];
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_rules() {
        let dir = std::env::temp_dir().join("bpo-test-coll");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::remove_file(dir.join("c.json"));
        let mut c = Collection::load(dir.join("c.json"));
        c.edit(1, Edit::Me(5));
        assert_eq!(c.get(1).me, 0, "can't research what you don't own");
        c.edit(1, Edit::Owned(true));
        assert_eq!(c.get(1).qty, 1);
        c.edit(1, Edit::Me(50));
        assert_eq!(c.get(1).me, ME_MAX);
        c.edit(1, Edit::Qty(0));
        assert!(!c.get(1).owned && c.get(1).me == 0);
        assert!(c.states.is_empty(), "default states are dropped");
        c.edit(2, Edit::MaxResearch);
        assert!(c.get(2).fully_researched());
        c.save();
        let text = fs::read_to_string(dir.join("c.json")).unwrap();
        assert!(text.contains("\"2\"") && !text.contains("notes"));
    }

    #[test]
    fn sync_rules() {
        use crate::esi::Owned;
        let dir = std::env::temp_dir().join("bpo-test-sync");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::remove_file(dir.join("c.json"));
        let mut c = Collection::load(dir.join("c.json"));
        c.edit(1, Edit::Owned(true)); // ticked by hand
        c.edit(2, Edit::Fav(true));
        c.edit(2, Edit::Notes("keep".into()));
        let mut owned = HashMap::new();
        owned.insert(2, Owned { qty: 2, me: 10, te: 20 });
        owned.insert(3, Owned { qty: 1, me: 5, te: 0 });
        assert_eq!(c.apply_sync(&owned, &Default::default()), (2, 0, 0));
        assert!(c.get(1).owned && !c.get(1).synced, "manual ticks untouched");
        assert!(c.get(2).fully_researched() && c.get(2).fav && c.get(2).notes == "keep");
        owned.remove(&3);
        owned.insert(2, Owned { qty: 1, me: 10, te: 20 });
        assert_eq!(c.apply_sync(&owned, &Default::default()), (0, 1, 1));
        assert!(!c.get(3).owned, "sold/moved blueprints drop out");
        assert!(c.get(1).owned, "still untouched");
        assert_eq!(c.apply_sync(&owned, &Default::default()), (0, 0, 0));
        // a blueprint away in a research job isn't listed by EVE, but is still owned
        owned.remove(&2);
        let keep = [2u32].into_iter().collect();
        assert_eq!(c.apply_sync(&owned, &keep), (0, 0, 0));
        assert!(c.get(2).owned);
    }

    #[test]
    fn reads_python_era_collection() {
        let dir = std::env::temp_dir().join("bpo-test-py");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("c.json"), r#"{"1001": {"owned": true, "qty": 1, "me": 10, "te": 20, "fav": true}, "1057": {"ignored": true}}"#).unwrap();
        let c = Collection::load(dir.join("c.json"));
        assert!(c.get(1001).fully_researched() && c.get(1001).fav);
        assert!(c.get(1057).ignored);
    }

    #[test]
    fn ship_tree_order() {
        let mut groups = vec!["Titan", "Battleship", "Zzz New Hull", "Frigate", "Interceptor", "Cruiser"];
        groups.sort_by_key(|g| (group_rank("Ship", g), g.to_lowercase()));
        assert_eq!(groups, ["Frigate", "Interceptor", "Cruiser", "Battleship", "Titan", "Zzz New Hull"]);
        let mut frig = vec!["Logistics Frigate", "Covert Ops", "Assault Frigate", "Interceptor", "Electronic Attack Ship"];
        frig.sort_by_key(|g| group_rank("Ship", g));
        assert_eq!(frig, ["Interceptor", "Assault Frigate", "Covert Ops", "Electronic Attack Ship", "Logistics Frigate"]);
        assert_eq!(group_rank("Module", "Anything"), 0);
    }

    #[test]
    fn tabs() {
        assert_eq!(tab_for("manufacturing", "Subsystem"), "Modules");
        assert_eq!(tab_for("reaction", "Material"), "Reactions");
        assert_eq!(tab_for("manufacturing", "Implant"), "Other");
    }

    #[test]
    fn csv_quoting() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}

#[cfg(test)]
mod build_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn builds_from_sde_layout() {
        let dir = std::env::temp_dir().join("bpo-test-sde");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("sde.zip");
        let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let files = [
            ("sde/categories.jsonl", r#"{"_key":6,"name":{"en":"Ship"},"published":true}
{"_key":32,"name":{"en":"Subsystem"},"published":true}
{"_key":4,"name":{"en":"Material"},"published":true}"#),
            ("sde/groups.jsonl", r#"{"_key":25,"name":{"en":"Frigate"},"categoryID":6,"published":true}
{"_key":954,"name":{"en":"Defensive Subsystem"},"categoryID":32,"published":true}
{"_key":429,"name":{"en":"Composite"},"categoryID":4,"published":true}"#),
            ("sde/types.jsonl", r#"{"_key":587,"name":{"en":"Rifter","de":"Rifter"},"groupID":25,"published":true,"metaGroupID":1,"portionSize":1,"marketGroupID":64}
{"_key":691,"name":{"en":"Rifter Blueprint"},"groupID":105,"published":true,"portionSize":1,"marketGroupID":2}
{"_key":100,"name":{"en":"Loki Defensive"},"groupID":954,"published":true,"metaGroupID":14,"portionSize":1}
{"_key":101,"name":{"en":"Loki Defensive Blueprint"},"groupID":105,"published":true,"portionSize":1}
{"_key":200,"name":{"en":"Carbon Fiber"},"groupID":429,"published":true,"portionSize":1}
{"_key":201,"name":{"en":"Carbon Fiber Reaction Formula"},"groupID":1888,"published":true,"portionSize":1,"marketGroupID":3}
{"_key":301,"name":{"en":"Hidden Blueprint"},"groupID":105,"published":false,"portionSize":1}"#),
            ("sde/blueprints.jsonl", r#"{"_key":691,"maxProductionLimit":10,"activities":{"manufacturing":{"time":1,"products":[{"typeID":587,"quantity":1}],"materials":[{"typeID":34,"quantity":10}]},"research_material":{"time":1},"research_time":{"time":1},"copying":{"time":1}}}
{"_key":101,"maxProductionLimit":1,"activities":{"manufacturing":{"time":1,"products":[{"typeID":100,"quantity":1}]}}}
{"_key":201,"maxProductionLimit":1,"activities":{"reaction":{"time":1,"products":[{"typeID":200,"quantity":200}],"materials":[],"skills":[]}}}
{"_key":301,"maxProductionLimit":1,"activities":{"manufacturing":{"time":1,"products":[{"typeID":587,"quantity":1}]}}}"#),
        ];
        for (name, body) in files {
            z.start_file(name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let p: Progress = Default::default();
        let out = build_dataset(&path, &p).unwrap();
        let names: Vec<_> = out.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Carbon Fiber Reaction Formula", "Loki Defensive Blueprint", "Rifter Blueprint"]);
        let rifter = &out[2];
        assert_eq!((rifter.tab.as_str(), rifter.group.as_str(), rifter.tech.as_str()), ("Ships", "Frigate", "Tech I"));
        assert!(rifter.market && rifter.researchable);
        let loki = &out[1];
        assert_eq!((loki.tab.as_str(), loki.tech.as_str()), ("Modules", "Tech III"));
        assert!(!loki.market && !loki.researchable);
        assert_eq!(out[0].tab, "Reactions");
    }
}
