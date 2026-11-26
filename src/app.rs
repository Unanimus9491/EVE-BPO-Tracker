//! The tracker window: tabs, filters, a virtualised card grid with group headers, and a detail panel.

use crate::data::{self, Collection, Edit, Entry, ME_MAX, Progress, TABS, TE_MAX};
use crate::esi::{self, Account, SyncedOriginal};
use std::collections::HashMap;
use crate::icons::Icons;
use crate::theme::*;
use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, Layout, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, Vec2, pos2, vec2,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CARD_W: f32 = 300.0;
const CARD_H: f32 = 196.0;
const GAP: f32 = 10.0;
const CELL_W: f32 = CARD_W + GAP;
const CELL_H: f32 = CARD_H + GAP;
const HEADER_H: f32 = 52.0;
const SLOT_SIZE: f32 = 50.0;

const STATUS: [&str; 10] = [
    "Any status",
    "Missing",
    "Owned",
    "Owned – not fully researched",
    "Fully researched",
    "Duplicates (qty > 1)",
    "Favourites",
    "Ignored",
    "Researching (ME/TE)",
    "In industry (any job)",
];
const MARKET: [&str; 3] = ["Market + non-market", "Market listed only", "Not on market only"];
const SORT: [&str; 4] = ["Sort: Name", "Sort: Group", "Sort: Missing first", "Sort: Owned first"];
const SORT_GROUP: usize = 1;

enum Item {
    Header { group: String, category: String, ids: Vec<u32>, collapsed: bool },
    Card(usize), // index into entries
}

enum Row {
    Header(usize),           // item index
    Cards(usize, usize),     // item range
}

enum Dialog {
    None,
    FirstRun,
    Message { title: String, body: String },
    ConfirmRestore(PathBuf),
    About,
}

#[derive(Clone, PartialEq)]
struct Filters {
    tab: usize,
    search: String,
    status: usize,
    tech: Option<String>,
    group: Option<String>,
    market: usize,
    sort: usize,
    hide_ignored: bool,
    /// Tech II BPOs essentially can't be obtained (only from rare events or dev gifts),
    /// and Tech III only comes as reverse-engineered copies.
    hide_t2_t3: bool,
    /// Characters whose synced blueprints are shown; None = everything (all characters + hand-ticked).
    chars: Option<BTreeSet<String>>,
}

pub struct TrackerApp {
    dir: PathBuf,
    entries: Vec<Entry>,
    coll: Collection,
    icons: Icons,
    f: Filters,
    applied: Option<Filters>,
    /// Collapsed groups as (category, group); remembered between sessions in settings.json.
    collapsed: BTreeSet<(String, String)>,
    collapse_changed: bool,
    applied_rev: u64,
    items: Vec<Item>,
    shown: usize,
    techs: Vec<String>,
    groups: Vec<String>,
    selected: Option<u32>,
    notes: (u32, String),
    build: Option<Progress>,
    dialog: Dialog,
    test: Option<TestHook>,
    // EVE character sync
    accounts: Vec<Account>,
    details: HashMap<u32, Vec<SyncedOriginal>>,
    login: Option<esi::Login>,
    sync: Option<esi::Sync>,
    sync_manual: bool,
    sync_status: String,
    /// Longer explanation, shown when hovering the status.
    sync_detail: String,
    sync_failed: bool,
    jobs: Vec<esi::Job>,
    animating: bool,
    started_sync: bool,
    ctx: Option<egui::Context>,
}

/// For automated screenshots: BPO_SCREENSHOT=out.png [BPO_TEST_TAB=1] [BPO_TEST_SORT=1] [BPO_TEST_SELECT=id]
struct TestHook {
    out: PathBuf,
    frame: u32,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl TrackerApp {
    pub fn new(ctx: egui::Context) -> Self {
        let dir = data::data_dir();
        let entries = data::load_entries(&dir.join("bpos.json")).unwrap_or_default();
        let coll = Collection::load(dir.join("collection.json"));
        let icons = Icons::new(dir.join("icons"), ctx);
        let mut app = Self {
            dialog: if entries.is_empty() { Dialog::FirstRun } else { Dialog::None },
            dir,
            entries,
            coll,
            icons,
            f: Filters {
                tab: 0,
                search: String::new(),
                status: 0,
                tech: None,
                group: None,
                market: 1, // most blueprints that aren't market listed can't be obtained as BPOs
                sort: SORT_GROUP,
                hide_ignored: true,
                hide_t2_t3: true,
                chars: None,
            },
            applied: None,
            collapsed: BTreeSet::new(),
            collapse_changed: false,
            applied_rev: 0,
            items: Vec::new(),
            shown: 0,
            techs: Vec::new(),
            groups: Vec::new(),
            selected: None,
            notes: (0, String::new()),
            build: None,
            test: None,
            accounts: Vec::new(),
            details: HashMap::new(),
            login: None,
            sync: None,
            sync_manual: false,
            sync_status: String::new(),
            sync_detail: String::new(),
            sync_failed: false,
            jobs: Vec::new(),
            animating: false,
            started_sync: false,
            ctx: None,
        };
        app.accounts = esi::load_accounts(&esi::accounts_path(&app.dir));
        app.details = esi::load_details(&esi::details_path(&app.dir));
        app.jobs = esi::load_jobs(&esi::jobs_path(&app.dir));
        app.load_settings();
        if let Some(out) = std::env::var_os("BPO_SCREENSHOT") {
            let num = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
            app.f.tab = num("BPO_TEST_TAB").unwrap_or(0);
            app.f.sort = num("BPO_TEST_SORT").unwrap_or(SORT_GROUP);
            app.f.market = num("BPO_TEST_MARKET").unwrap_or(1);
            app.f.status = num("BPO_TEST_STATUS").unwrap_or(0);
            app.selected = num("BPO_TEST_SELECT").map(|v| v as u32);
            app.f.search = std::env::var("BPO_TEST_SEARCH").unwrap_or_default();
            if std::env::var_os("BPO_TEST_ABOUT").is_some() {
                app.dialog = Dialog::About;
            }
            if let Ok(c) = std::env::var("BPO_TEST_CHARS") {
                app.f.chars = Some(c.split(',').map(str::to_string).collect());
            }
            if let Ok(g) = std::env::var("BPO_TEST_COLLAPSE") {
                app.collapsed.insert(("Ship".into(), g));
            }
            app.test = Some(TestHook { out: out.into(), frame: 0, done: Default::default() });
        }
        app.refresh_options();
        app
    }

    // ------------------------------------------------------------------ EVE login + sync
    fn start_login(&mut self, ctx: &egui::Context) {
        if self.login.is_none() {
            self.login = Some(esi::start_login(ctx.clone()));
        }
    }

    fn start_sync(&mut self, manual: bool) {
        if self.sync.is_some() || self.accounts.is_empty() {
            return;
        }
        if let Some(ctx) = &self.ctx {
            self.sync = Some(esi::start_sync(self.accounts.clone(), ctx.clone()));
            self.sync_manual = manual;
            self.sync_status = "Syncing with EVE…".into();
            self.sync_detail.clear();
            self.sync_failed = false;
        }
    }

    fn poll_esi(&mut self) {
        let login_result = self.login.as_ref().and_then(|l| l.state.lock().ok().and_then(|mut g| g.result.take()));
        if let Some(result) = login_result {
            self.login = None;
            match result {
                Ok(acc) => {
                    self.accounts.retain(|a| a.character_id != acc.character_id);
                    self.accounts.push(acc);
                    self.accounts.sort_by_key(|a| a.name.to_lowercase());
                    esi::save_accounts(&esi::accounts_path(&self.dir), &self.accounts);
                    self.start_sync(true);
                }
                Err(e) => {
                    self.dialog = Dialog::Message { title: "EVE login didn't complete".into(), body: e };
                }
            }
        }
        let sync_result = self.sync.as_ref().and_then(|s| s.result.lock().ok().and_then(|mut g| g.take()));
        if let Some(r) = sync_result {
            self.sync = None;
            // Keep accounts' rotated tokens, but don't resurrect characters removed mid-sync.
            for updated in r.accounts {
                if let Some(a) = self.accounts.iter_mut().find(|a| a.character_id == updated.character_id) {
                    *a = updated;
                }
            }
            esi::save_accounts(&esi::accounts_path(&self.dir), &self.accounts);
            if r.synced.is_empty() {
                self.sync_failed = true;
                self.sync_status = "Sync failed".into();
            } else {
                // Only apply when every character synced, so a temporary error can't
                // mark another character's blueprints as missing.
                if r.errors.is_empty() {
                    let keep: std::collections::HashSet<u32> = r.in_jobs_unlisted.iter().copied().collect();
                    let (added, changed, removed) = self.coll.apply_sync(&r.owned, &keep);
                    self.details = r.details;
                    self.jobs = r.jobs;
                    esi::save_jobs(&esi::jobs_path(&self.dir), &self.jobs);
                    esi::save_details(&esi::details_path(&self.dir), &self.details);
                    self.coll.save();
                    self.refilter_if_needed(true);
                    let mut parts = vec![format!("{added} new"), format!("{changed} updated")];
                    if removed > 0 {
                        parts.push(format!("{removed} no longer owned"));
                    }
                    self.sync_status = format!("Synced {} · {}", now_hhmm(), parts.join(", "));
                    if !r.without_job_scope.is_empty() {
                        self.sync_status += " · log in again for research";
                        self.sync_detail = format!(
                            "To track research and other industry jobs, log these characters in again \
                             (Characters › Log in with EVE): {}",
                            r.without_job_scope.join(", ")
                        );
                    }
                } else {
                    self.sync_failed = true;
                    self.sync_status = "Sync incomplete, nothing changed".into();
                }
            }
            if !r.errors.is_empty() && (self.sync_manual || self.accounts.iter().any(|a| a.needs_login)) {
                self.dialog = Dialog::Message { title: "EVE sync problems".into(), body: r.errors.join("\n") };
            } else if !r.errors.is_empty() {
                self.sync_detail = r.errors.join("\n");
            }
        }
    }

    /// "All characters" or a chosen subset; doesn't touch logins.
    fn character_picker(&mut self, ui: &mut Ui) {
        let all: BTreeSet<String> = self.accounts.iter().map(|a| a.name.clone()).collect();
        let label = match &self.f.chars {
            None => "All characters".to_string(),
            Some(c) if c.len() == 1 => c.iter().next().cloned().unwrap_or_default(),
            Some(c) => format!("{} characters", c.len()),
        };
        egui::ComboBox::from_id_salt("chars").width(170.0).selected_text(label).show_ui(ui, |ui| {
            if ui.selectable_label(self.f.chars.is_none(), "All characters").clicked() {
                self.f.chars = None;
            }
            ui.separator();
            let mut set = self.f.chars.clone().unwrap_or_else(|| all.clone());
            let mut changed = false;
            for name in &all {
                let mut on = set.contains(name);
                if ui.checkbox(&mut on, name).changed() {
                    changed = true;
                    if on {
                        set.insert(name.clone());
                    } else {
                        set.remove(name);
                    }
                }
            }
            if changed {
                self.f.chars = if set.is_empty() || set == all { None } else { Some(set) };
            }
        })
        .response
        .on_hover_text("Show the blueprints of some characters only. Hand-ticked blueprints appear under All characters.");
    }

    fn characters_menu(&mut self, ui: &mut Ui) {
        if ui.button("Log in with EVE (add character)…").clicked() {
            self.start_login(ui.ctx());
            ui.close();
        }
        let can_sync = !self.accounts.is_empty() && self.sync.is_none();
        if ui.add_enabled(can_sync, egui::Button::new("Sync now")).clicked() {
            self.start_sync(true);
            ui.close();
        }
        if !self.accounts.is_empty() {
            ui.separator();
        }
        let mut remove = None;
        let mut relogin = false;
        for a in &self.accounts {
            ui.menu_button(if a.needs_login { format!("{}  (log in again)", a.name) } else { a.name.clone() }, |ui| {
                if a.needs_login && ui.button("Log in again…").clicked() {
                    relogin = true;
                    ui.close();
                }
                if ui.button("Remove this character").clicked() {
                    remove = Some(a.character_id);
                    ui.close();
                }
            });
        }
        if relogin {
            self.start_login(ui.ctx());
        }
        if let Some(id) = remove {
            self.accounts.retain(|a| a.character_id != id);
            esi::save_accounts(&esi::accounts_path(&self.dir), &self.accounts);
            self.f.chars = None;
            self.sync_status = "Character removed. Sync to update owned blueprints.".into();
        }
    }

    fn load_settings(&mut self) {
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct Settings {
            collapsed: Vec<(String, String)>,
        }
        let s: Settings = std::fs::read_to_string(self.dir.join("settings.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        self.collapsed = s.collapsed.into_iter().collect();
    }

    fn save_settings(&self) {
        let json = serde_json::json!({ "collapsed": self.collapsed.iter().collect::<Vec<_>>() });
        if let Ok(bytes) = serde_json::to_vec_pretty(&json) {
            let _ = data::atomic_write(&self.dir.join("settings.json"), &bytes);
        }
    }

    fn set_collapsed(&mut self, keys: Vec<(String, String)>, collapse: bool) {
        for k in keys {
            if collapse {
                self.collapsed.insert(k);
            } else {
                self.collapsed.remove(&k);
            }
        }
        self.collapse_changed = true;
        self.save_settings();
    }

    fn visible_group_keys(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .filter_map(|it| match it {
                Item::Header { group, category, .. } => Some((category.clone(), group.clone())),
                _ => None,
            })
            .collect()
    }

    fn entry_by_id(&self, id: u32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn in_tab(&self, e: &Entry) -> bool {
        self.f.tab == 0 || e.tab == TABS[self.f.tab]
    }

    fn refresh_options(&mut self) {
        let techs: BTreeSet<String> = self.entries.iter().map(|e| e.tech.clone()).collect();
        self.techs = techs.into_iter().collect();
        let groups: BTreeSet<String> =
            self.entries.iter().filter(|e| self.in_tab(e)).map(|e| e.group.clone()).collect();
        self.groups = groups.into_iter().collect();
        if self.f.group.as_ref().is_some_and(|g| !self.groups.contains(g)) {
            self.f.group = None;
        }
    }

    // ------------------------------------------------------------------ filtering
    fn matches(&self, e: &Entry, q: &str) -> bool {
        let f = &self.f;
        if !self.in_tab(e) {
            return false;
        }
        if !q.is_empty()
            && !e.name.to_lowercase().contains(q)
            && !e.product_name.to_lowercase().contains(q)
            && !e.group.to_lowercase().contains(q)
        {
            return false;
        }
        if f.tech.as_ref().is_some_and(|t| *t != e.tech) || f.group.as_ref().is_some_and(|g| *g != e.group) {
            return false;
        }
        if (f.market == 1 && !e.market) || (f.market == 2 && e.market) {
            return false;
        }
        if self.hides_tech(e) {
            return false;
        }
        let s = &self.view_state(e.id);
        if f.hide_ignored && s.ignored && f.status != 7 {
            return false;
        }
        match f.status {
            1 => !s.owned,
            2 => s.owned,
            3 => s.owned && !s.fully_researched(),
            4 => s.fully_researched(),
            5 => s.qty > 1,
            6 => s.fav,
            7 => s.ignored,
            8 => self
                .jobs_for(e.id)
                .iter()
                .any(|j| matches!(j.activity, esi::Activity::MeResearch | esi::Activity::TeResearch)),
            9 => !self.jobs_for(e.id).is_empty(),
            _ => true,
        }
    }

    /// Rebuild the visible list when filters change (not on every tick, so a card you
    /// just ticked doesn't vanish from under the mouse while a status filter is on).
    fn refilter_if_needed(&mut self, force: bool) {
        if !force && !self.collapse_changed && self.applied.as_ref() == Some(&self.f) {
            return;
        }
        self.collapse_changed = false;
        if self.applied.as_ref().is_none_or(|a| a.tab != self.f.tab) {
            self.refresh_options();
        }
        let q = self.f.search.trim().to_lowercase();
        let mut idx: Vec<usize> = (0..self.entries.len()).filter(|&i| self.matches(&self.entries[i], &q)).collect();
        let key = |i: &usize| self.entries[*i].name.to_lowercase();
        match self.f.sort {
            SORT_GROUP => idx.sort_by_cached_key(|i| {
                let e = &self.entries[*i];
                // Ships follow the in-game ship tree; other groups are alphabetical.
                (e.category.to_lowercase(), data::group_rank(&e.category, &e.group), e.group.to_lowercase(), e.name.to_lowercase())
            }),
            2 | 3 => {
                let missing_first = self.f.sort == 2;
                idx.sort_by_cached_key(|i| (self.view_state(self.entries[*i].id).owned == missing_first, key(i)))
            }
            _ => idx.sort_by_cached_key(key),
        }
        self.shown = idx.len();
        self.items.clear();
        let mut header: Option<usize> = None; // index of the current group's header in items
        for i in idx {
            if self.f.sort == SORT_GROUP {
                let e = &self.entries[i];
                let same = header.is_some_and(|h| {
                    matches!(&self.items[h], Item::Header { group, category, .. } if *group == e.group && *category == e.category)
                });
                if !same {
                    let collapsed = self.collapsed.contains(&(e.category.clone(), e.group.clone()));
                    self.items.push(Item::Header {
                        group: e.group.clone(),
                        category: e.category.clone(),
                        ids: Vec::new(),
                        collapsed,
                    });
                    header = Some(self.items.len() - 1);
                }
                if let Some(Item::Header { ids, collapsed, .. }) = header.map(|h| &mut self.items[h]) {
                    ids.push(e.id);
                    if *collapsed {
                        continue; // count it in the header, but don't show the card
                    }
                }
            }
            self.items.push(Item::Card(i));
        }
        self.applied = Some(self.f.clone());
        self.applied_rev = self.coll.revision;
    }

    fn layout_rows(&self, cols: usize) -> Vec<(Row, f32, f32)> {
        let mut rows = Vec::new();
        let mut y = 0.0;
        let mut i = 0;
        while i < self.items.len() {
            match self.items[i] {
                Item::Header { .. } => {
                    rows.push((Row::Header(i), y, HEADER_H));
                    y += HEADER_H;
                    i += 1;
                }
                Item::Card(_) => {
                    let start = i;
                    while i < self.items.len() && i - start < cols && matches!(self.items[i], Item::Card(_)) {
                        i += 1;
                    }
                    rows.push((Row::Cards(start, i), y, CELL_H));
                    y += CELL_H;
                }
            }
        }
        rows
    }

    // ------------------------------------------------------------------ data actions
    fn start_build(&mut self, zip: Option<PathBuf>, ctx: &egui::Context) {
        if self.build.is_none() {
            self.build = Some(data::spawn_build(zip, ctx.clone()));
        }
    }

    fn poll_build(&mut self) {
        let Some(p) = &self.build else { return };
        let result = p.lock().ok().and_then(|mut g| g.result.take());
        let Some(result) = result else { return };
        self.build = None;
        match result {
            Ok(entries) => {
                if let Ok(json) = serde_json::to_vec(&entries) {
                    let _ = data::atomic_write(&self.dir.join("bpos.json"), &json);
                }
                let n = entries.len();
                self.entries = data::load_entries(&self.dir.join("bpos.json")).unwrap_or(entries);
                self.refresh_options();
                self.refilter_if_needed(true);
                self.dialog = Dialog::Message { title: "Game data updated".into(), body: format!("Loaded {n} blueprints.") };
            }
            Err(e) => {
                self.dialog = Dialog::Message {
                    title: "Couldn't build the blueprint list".into(),
                    body: format!(
                        "{e}\n\nYou can download the JSONL zip yourself from developers.eveonline.com (Static Data) \
                         and use File › Build from a local SDE zip."
                    ),
                };
            }
        }
    }

    fn export_csv(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_file_name("bpo_collection.csv").add_filter("CSV", &["csv"]).save_file()
        else {
            return;
        };
        self.dialog = match data::export_csv(&path, &self.entries, &self.coll) {
            Ok(()) => Dialog::Message { title: "Exported".into(), body: format!("Saved to {}", path.display()) },
            Err(e) => Dialog::Message { title: "Export failed".into(), body: e.to_string() },
        };
    }

    fn backup(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("bpo_collection_backup.json")
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return;
        };
        self.coll.save();
        if let Err(e) = std::fs::copy(self.dir.join("collection.json"), &path) {
            self.dialog = Dialog::Message { title: "Backup failed".into(), body: e.to_string() };
        }
    }

    fn open_data_folder(&self) {
        let cmd = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(cmd).arg(&self.dir).spawn();
    }

    // ------------------------------------------------------------------ UI: top area
    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Download / update game data").clicked() {
                    self.start_build(None, ui.ctx());
                }
                if ui.button("Build from a local SDE zip…").clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("Zip", &["zip"]).pick_file() {
                        self.start_build(Some(p), ui.ctx());
                    }
                ui.separator();
                if ui.button("Export collection to CSV…").clicked() {
                    self.export_csv();
                }
                if ui.button("Back up collection…").clicked() {
                    self.backup();
                }
                if ui.button("Restore collection from backup…").clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                        self.dialog = Dialog::ConfirmRestore(p);
                    }
                ui.separator();
                if ui.button("Open data folder").clicked() {
                    self.open_data_folder();
                }
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Characters", |ui| self.characters_menu(ui));
            ui.menu_button("Help", |ui| {
                if ui.button("About EVE BPO Tracker").clicked() {
                    self.dialog = Dialog::About;
                    ui.close();
                }
            });
        });
    }

    /// What a card shows. With a character view active, ownership and ME/TE come only from
    /// those characters' synced originals; favourites, notes and ignores are kept.
    fn view_state(&self, id: u32) -> data::State {
        let mut s = self.coll.get(id).clone();
        let Some(chars) = &self.f.chars else { return s };
        let mut qty = 0;
        let (mut me, mut te) = (0, 0);
        for d in self.details.get(&id).into_iter().flatten().filter(|d| chars.contains(&d.character)) {
            qty += d.count;
            me = me.max(d.me);
            te = te.max(d.te);
        }
        s.owned = qty > 0;
        s.qty = qty;
        s.me = me;
        s.te = te;
        s.synced = s.owned;
        s
    }

    /// Industry jobs using this blueprint, for the characters in view. Soonest first.
    fn jobs_for(&self, id: u32) -> Vec<&esi::Job> {
        let mut v: Vec<&esi::Job> = self
            .jobs
            .iter()
            .filter(|j| j.type_id == id && self.f.chars.as_ref().is_none_or(|c| c.contains(&j.character)))
            .collect();
        v.sort_by_key(|j| j.end);
        v
    }

    fn character_view_active(&self) -> bool {
        self.f.chars.is_some()
    }

    /// Hidden as Tech II/III, unless the tech filter explicitly asks for that tech level.
    fn hides_tech(&self, e: &Entry) -> bool {
        self.f.hide_t2_t3
            && (e.tech == "Tech II" || e.tech == "Tech III")
            && self.f.tech.as_deref() != Some(e.tech.as_str())
    }

    fn stats_text(&self) -> (usize, usize, usize) {
        let (mut counted, mut owned, mut full) = (0, 0, 0);
        // Respect the market filter, so "market listed only" gives an achievable total.
        let market_ok = |e: &Entry| match self.f.market {
            1 => e.market,
            2 => !e.market,
            _ => true,
        };
        for e in self.entries.iter().filter(|e| self.in_tab(e) && market_ok(e) && !self.hides_tech(e)) {
            let s = self.view_state(e.id);
            if s.ignored {
                continue;
            }
            counted += 1;
            owned += s.owned as usize;
            full += s.fully_researched() as usize;
        }
        (counted, owned, full)
    }

    fn header_area(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Blueprint Collection").font(font_bold(26.0)));
            ui.add_space(16.0);
            let (counted, owned, full) = self.stats_text();
            let label = if self.f.tab == 0 { "All categories" } else { TABS[self.f.tab] };
            let pct = if counted > 0 { owned as f32 * 100.0 / counted as f32 } else { 0.0 };
            ui.label(RichText::new(format!("{label}:")).color(MUTED));
            ui.label(RichText::new(format!("{owned}")).font(font_bold(14.0)));
            ui.label(RichText::new(format!("/ {counted} owned ({pct:.1}%)")).color(MUTED));
            ui.label(RichText::new("·").color(MUTED));
            ui.label(RichText::new(format!("{full} fully researched")).color(GOLD));
            let in_industry: BTreeSet<u32> = self
                .jobs
                .iter()
                .filter(|j| self.f.chars.as_ref().is_none_or(|c| c.contains(&j.character)))
                .map(|j| j.type_id)
                .collect();
            if !in_industry.is_empty() {
                ui.label(RichText::new("·").color(MUTED));
                ui.label(RichText::new(format!("{} in industry", in_industry.len())).color(GOLD_PALE));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.accounts.is_empty() {
                    if ui.button("Log in with EVE").on_hover_text("Fill in your collection from your characters' blueprints").clicked() {
                        self.start_login(ui.ctx());
                    }
                } else {
                    let names: Vec<_> = self.accounts.iter().map(|a| a.name.as_str()).collect();
                    let sync_btn = ui
                        .add_enabled(self.sync.is_none(), egui::Button::new("Sync"))
                        .on_hover_text(format!("Read blueprints from: {}", names.join(", ")));
                    if sync_btn.clicked() {
                        self.start_sync(true);
                    }
                    if self.sync.is_some() {
                        ui.spinner();
                    }
                    let color = if self.sync_failed { MISSING_WARN } else { MUTED };
                    // Keep the status short so it can't push the header around; details on hover.
                    let status = ui.add(egui::Label::new(RichText::new(&self.sync_status).color(color)).truncate());
                    if !self.sync_detail.is_empty() {
                        status.on_hover_text(&self.sync_detail);
                    }
                    if self.accounts.len() > 1 {
                        self.character_picker(ui);
                    }
                }
            });
        });
        ui.add_space(4.0);

        // tabs + search
        ui.horizontal(|ui| {
            for (i, name) in TABS.iter().enumerate() {
                let on = self.f.tab == i;
                let text = RichText::new(*name).font(if on { font_bold(13.5) } else { font(13.5) });
                let btn = egui::Button::new(text.color(if on { GROUND } else { TEXT }))
                    .fill(if on { GOLD } else { CHARCOAL })
                    .stroke(Stroke::new(1.0, if on { GOLD } else { BORDER }))
                    .corner_radius(6)
                    .min_size(vec2(0.0, 30.0));
                if ui.add(btn).clicked() {
                    self.f.tab = i;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.f.search)
                        .hint_text("Search blueprint or product…")
                        .desired_width(260.0)
                        .margin(vec2(8.0, 6.0)),
                );
                ui.label(RichText::new(format!("Showing {}", self.shown)).color(MUTED));
            });
        });

        // filters (wrap onto a second line in narrow windows)
        ui.horizontal_wrapped(|ui| {
            combo(ui, "status", 150.0, &mut self.f.status, &STATUS);
            let tech_label = self.f.tech.clone().unwrap_or_else(|| "Any tech".into());
            egui::ComboBox::from_id_salt("tech").width(100.0).selected_text(tech_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.f.tech, None, "Any tech");
                for t in &self.techs {
                    ui.selectable_value(&mut self.f.tech, Some(t.clone()), t);
                }
            });
            let group_label = self.f.group.clone().unwrap_or_else(|| "Any group".into());
            egui::ComboBox::from_id_salt("group").width(160.0).height(400.0).selected_text(group_label).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut self.f.group, None, "Any group");
                    for g in &self.groups {
                        ui.selectable_value(&mut self.f.group, Some(g.clone()), g);
                    }
                },
            );
            combo(ui, "market", 150.0, &mut self.f.market, &MARKET);
            combo(ui, "sort", 120.0, &mut self.f.sort, &SORT);
            if self.f.sort == SORT_GROUP {
                if ui.button("Expand all").clicked() {
                    let keys = self.visible_group_keys();
                    self.set_collapsed(keys, false);
                }
                if ui.button("Collapse all").clicked() {
                    let keys = self.visible_group_keys();
                    self.set_collapsed(keys, true);
                }
            }
            ui.checkbox(&mut self.f.hide_ignored, "Hide ignored");
            ui.checkbox(&mut self.f.hide_t2_t3, "Hide Tech II/III").on_hover_text(
                "Tech II BPOs can't normally be obtained (only from rare events or dev gifts), \
                 and Tech III only comes as reverse-engineered copies",
            );
        });
        ui.add_space(6.0);
    }

    // ------------------------------------------------------------------ UI: grid
    fn grid(&mut self, ui: &mut Ui) {
        if self.entries.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No blueprint data yet").font(font_bold(18.0)));
                ui.label(RichText::new("Use File › Download / update game data to get started.").color(MUTED));
            });
            return;
        }
        self.animating = false;
        let width = ui.available_width();
        let cols = ((width + 2.0) / CELL_W).floor().max(1.0) as usize;
        let rows = self.layout_rows(cols);
        let total_h = rows.last().map(|(_, y, h)| y + h).unwrap_or(0.0);
        let mut edits: Vec<(u32, Edit)> = Vec::new();
        let mut select: Option<u32> = None;
        let mut toggle: Option<((String, String), bool)> = None;
        let shift = ui.input(|i| i.modifiers.shift);

        egui::ScrollArea::vertical().auto_shrink(false).wheel_scroll_multiplier(vec2(1.0, 1.6)).show_viewport(
            ui,
            |ui, viewport| {
                ui.set_height(total_h);
                let origin = ui.max_rect().min;
                for (row, y, h) in &rows {
                    if y + h < viewport.min.y || *y > viewport.max.y {
                        continue;
                    }
                    match row {
                        Row::Header(i) => {
                            if let Item::Header { group, category, ids, collapsed } = &self.items[*i] {
                                let rect = Rect::from_min_size(origin + vec2(0.0, *y), vec2(cols as f32 * CELL_W, HEADER_H));
                                if self.header(ui, rect, group, category, ids, *collapsed) {
                                    toggle = Some(((category.clone(), group.clone()), !*collapsed));
                                }
                            }
                        }
                        Row::Cards(a, b) => {
                            for (col, item) in (*a..*b).enumerate() {
                                if let Item::Card(ei) = self.items[item] {
                                    let rect = Rect::from_min_size(
                                        origin + vec2(col as f32 * CELL_W + GAP / 2.0, y + GAP / 2.0),
                                        vec2(CARD_W, CARD_H),
                                    );
                                    self.card(ui, rect, ei, shift, &mut edits, &mut select);
                                }
                            }
                        }
                    }
                }
            },
        );
        for (id, edit) in edits {
            self.coll.edit(id, edit);
        }
        if self.animating {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        if select.is_some() {
            self.selected = select;
        }
        if let Some((key, collapse)) = toggle {
            self.set_collapsed(vec![key], collapse);
        }
    }

    /// Draws a group header; returns true when it was clicked (to collapse/expand).
    fn header(&self, ui: &mut Ui, rect: Rect, group: &str, category: &str, ids: &[u32], collapsed: bool) -> bool {
        let r = Rect::from_min_max(rect.min + vec2(GAP / 2.0 + 2.0, 0.0), rect.max - vec2(GAP / 2.0 + 2.0, 0.0));
        let resp = ui
            .interact(r, ui.scope_id().with(("header", category, group)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(if collapsed { "Click to expand" } else { "Click to collapse" });
        let p = ui.painter();
        let states: Vec<_> = ids.iter().map(|i| self.view_state(*i)).collect();
        let owned = states.iter().filter(|s| s.owned).count();
        let full = states.iter().filter(|s| s.fully_researched()).count();
        let complete = !ids.is_empty() && owned == ids.len();
        let mid = r.top() + 27.0;
        // chevron: points right when collapsed, down when open
        let c = pos2(r.left() + 6.0, mid);
        let pts = if collapsed {
            vec![c + vec2(-3.0, -5.0), c + vec2(3.5, 0.0), c + vec2(-3.0, 5.0)]
        } else {
            vec![c + vec2(-5.0, -3.0), c + vec2(5.0, -3.0), c + vec2(0.0, 3.5)]
        };
        let chevron = if resp.hovered() { GOLD_PALE } else { MUTED };
        p.add(egui::Shape::convex_polygon(pts, chevron, Stroke::NONE));
        let title_color = if resp.hovered() { GOLD_PALE } else { TEXT };
        let title = p.text(pos2(r.left() + 20.0, mid), Align2::LEFT_CENTER, group, font_bold(17.0), title_color);
        p.text(pos2(title.right() + 12.0, mid + 1.0), Align2::LEFT_CENTER, category.to_uppercase(), font(11.0), MUTED);
        // progress bar + count
        let bar = Rect::from_min_size(pos2(r.right() - 120.0, mid - 2.5), vec2(120.0, 5.0));
        p.rect_filled(bar, 3, SLOT);
        if owned > 0 {
            let w = bar.width() * owned as f32 / ids.len() as f32;
            p.rect_filled(Rect::from_min_size(bar.min, vec2(w, bar.height())), 3, if complete { GOLD } else { GOLD_PALE });
        }
        let mut count = format!("{owned} / {} owned", ids.len());
        if full > 0 {
            count += &format!("  ·  {full} researched");
        }
        p.text(pos2(bar.left() - 12.0, mid), Align2::RIGHT_CENTER, count, font_bold(12.5), if complete { GOLD } else { MUTED });
        // divider with a short gold lead-in
        let base = r.bottom() - 8.0;
        p.rect_filled(Rect::from_min_size(pos2(r.left(), base), vec2(r.width(), 1.0)), 0, BORDER);
        p.rect_filled(Rect::from_min_size(pos2(r.left(), base - 1.0), vec2(36.0, 3.0)), 1, GOLD);
        resp.clicked()
    }

    fn slot(
        p: &egui::Painter,
        r: Rect,
        label: &str,
        value: &str,
        lit: bool,
        maxed: bool,
        frac: Option<f32>,
        badge: Option<String>,
    ) {
        let (bg, border) = if maxed {
            (SLOT_GOLD, GOLD)
        } else if lit {
            (SLOT_LIT, pale_gold_a(140))
        } else {
            (SLOT, SLOT_BORDER)
        };
        p.rect_filled(r, 9, bg);
        p.rect_stroke(r, 9, Stroke::new(1.4, border), StrokeKind::Inside);
        p.text(pos2(r.center().x, r.top() + 10.0), Align2::CENTER_CENTER, label, font(10.5), if lit || maxed { TEXT } else { MUTED });
        let vc = if maxed { GOLD } else if lit { TEXT } else { MUTED };
        // Shrink long values like "10→20" so they stay clear of the slot edges.
        let size = [14.0, 12.5, 11.0]
            .into_iter()
            .find(|sz| p.layout_no_wrap(value.to_string(), font_bold(*sz), vc).size().x <= r.width() - 10.0)
            .unwrap_or(10.0);
        p.text(pos2(r.center().x, r.top() + 28.0), Align2::CENTER_CENTER, value, font_bold(size), vc);
        if let Some(frac) = frac {
            let bar = Rect::from_min_size(pos2(r.left() + 7.0, r.bottom() - 9.0), vec2(r.width() - 14.0, 4.0));
            p.rect_filled(bar, 1, Color32::from_black_alpha(120));
            if frac > 0.0 {
                p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, 4.0)), 1, if maxed { GOLD } else { GOLD_PALE });
            }
        }
        if let Some(b) = badge {
            let g = p.layout_no_wrap(b, font_bold(11.0), TEXT);
            let w = g.size().x + 10.0;
            let br = Rect::from_min_size(pos2(r.right() - w + 5.0, r.top() - 6.0), vec2(w, 16.0));
            p.rect_filled(br, 5, Color32::from_rgb(0x0d, 0x0c, 0x0b));
            p.rect_stroke(br, 5, Stroke::new(1.0, SLOT_BORDER), StrokeKind::Inside);
            p.galley(br.center() - g.size() / 2.0, g, TEXT);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &mut Ui, rect: Rect, ei: usize, shift: bool, edits: &mut Vec<(u32, Edit)>, select: &mut Option<u32>) {
        let e = &self.entries[ei];
        let id = e.id;
        let s = self.view_state(id);
        let researched = s.fully_researched();
        let selected = self.selected == Some(id);
        let can_edit = !self.character_view_active();
        let now = esi::now_unix();
        let jobs: Vec<esi::Job> = self.jobs_for(id).into_iter().cloned().collect();
        let running = jobs.iter().any(|j| j.is_running(now));
        // (screenshot tests jump ahead so the sweep is mid-card)
        let anim_t = ui.input(|i| i.time) as f32 + if self.test.is_some() { 2.6 } else { 0.0 };

        let base = ui.scope_id().with(("card", id));
        let card_resp = ui.interact(rect, base, Sense::click());
        let x = rect.left();
        let y = rect.top();
        let star_r = Rect::from_min_size(pos2(x + 8.0, y + 8.0), vec2(24.0, 24.0));
        let sx = x + 116.0;
        let bpo_r = Rect::from_min_size(pos2(sx, y + 50.0), Vec2::splat(SLOT_SIZE));
        let me_r = Rect::from_min_size(pos2(sx + SLOT_SIZE + 8.0, y + 50.0), Vec2::splat(SLOT_SIZE));
        let te_r = Rect::from_min_size(pos2(sx + 2.0 * (SLOT_SIZE + 8.0), y + 50.0), Vec2::splat(SLOT_SIZE));
        let research_ok = e.researchable && s.owned && can_edit;

        let star = ui.interact(star_r, base.with("star"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let bpo = ui
            .interact(bpo_r, base.with("bpo"), Sense::click())
            .on_hover_cursor(if can_edit { CursorIcon::PointingHand } else { CursorIcon::Default })
            .on_hover_text(if !can_edit {
                "Switch to All characters to edit"
            } else if s.owned {
                "Click: mark missing"
            } else {
                "Click: mark owned"
            });
        let (me, te) = if e.researchable {
            let me = ui.interact(me_r, base.with("me"), Sense::click());
            let te = ui.interact(te_r, base.with("te"), Sense::click());
            if research_ok {
                (
                    Some(me.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Click: ME +1 · Shift-click: −1")),
                    Some(te.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Click: TE +2 · Shift-click: −2")),
                )
            } else {
                (Some(me), Some(te))
            }
        } else {
            (None, None)
        };

        if star.clicked() {
            edits.push((id, Edit::Fav(!s.fav)));
        }
        if bpo.clicked() && can_edit {
            edits.push((id, Edit::Owned(!s.owned)));
        }
        if research_ok {
            if me.as_ref().is_some_and(|r| r.clicked()) {
                let v = if shift { s.me.saturating_sub(1) } else { (s.me + 1).min(ME_MAX) };
                edits.push((id, Edit::Me(v)));
            }
            if te.as_ref().is_some_and(|r| r.clicked()) {
                let v = if shift { s.te.saturating_sub(2) } else { (s.te + 2).min(TE_MAX) };
                edits.push((id, Edit::Te(v)));
            }
        }
        let any_click = card_resp.clicked()
            || star.clicked()
            || bpo.clicked()
            || me.as_ref().is_some_and(|r| r.clicked())
            || te.as_ref().is_some_and(|r| r.clicked());
        if any_click || card_resp.secondary_clicked() {
            *select = Some(id);
        }

        // ---- paint
        let mut p = ui.painter_at(rect.expand(8.0));
        if s.ignored {
            p.multiply_opacity(0.35);
        }
        let hovered = card_resp.hovered() || star.hovered() || bpo.hovered();
        let border = if selected {
            SELECT
        } else if researched {
            GOLD
        } else if s.owned {
            pale_gold_a(if hovered { 170 } else { 110 })
        } else if hovered {
            Color32::from_rgb(0x4a, 0x44, 0x3e)
        } else {
            BORDER
        };
        p.rect_filled(rect, 10, if s.owned { CARD_OWNED } else { CHARCOAL });
        if running {
            paint_sweep(&ui.painter_at(rect.shrink(1.0)), rect, anim_t);
            self.animating = true;
        }
        p.rect_stroke(rect, 10, Stroke::new(if selected || researched { 2.0 } else { 1.2 }, border), StrokeKind::Inside);

        // header
        p.text(star_r.center(), Align2::CENTER_CENTER, if s.fav { "★" } else { "☆" }, font(17.0), if s.fav { GOLD } else { MUTED });
        let name_w = rect.width() - 50.0;
        let name = elide(&p, e.short_name(), font_bold(15.0), name_w);
        p.text(pos2(x + 36.0, y + 20.0), Align2::LEFT_CENTER, name, font_bold(15.0), TEXT);

        // image
        let img_r = Rect::from_min_size(pos2(x + 12.0, y + 44.0), Vec2::splat(92.0));
        let tex = self.icons.get(e).map(|t| t.id());
        if let Some(tex) = tex {
            p.image(tex, img_r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            p.rect_filled(img_r, 8, Color32::from_black_alpha(60));
            let initials: String = e.short_name().split_whitespace().take(2).filter_map(|w| w.chars().next()).collect();
            p.text(img_r.center(), Align2::CENTER_CENTER, initials.to_uppercase(), font_bold(14.0), MUTED);
        }

        // slots
        Self::slot(&p, bpo_r, "BPO", if s.owned { "✓" } else { "—" }, s.owned, false, None, (s.qty > 1).then(|| s.qty.to_string()));
        if e.researchable {
            // While research is running, the slot shows "from → to" and the bar shows job progress.
            let research = |act: esi::Activity, cur: u8| {
                jobs.iter().find(|j| j.activity == act).map(|j| {
                    let from = j.from_level.unwrap_or(cur);
                    let to = j.to_level().unwrap_or(from);
                    (format!("{from}→{to}"), j.progress(now))
                })
            };
            match research(esi::Activity::MeResearch, s.me) {
                Some((txt, prog)) => Self::slot(&p, me_r, "ME", &txt, true, false, Some(prog), None),
                None => Self::slot(&p, me_r, "ME", &s.me.to_string(), s.owned && s.me > 0, s.owned && s.me >= ME_MAX,
                    Some(if s.owned { s.me as f32 / ME_MAX as f32 } else { 0.0 }), None),
            }
            match research(esi::Activity::TeResearch, s.te) {
                Some((txt, prog)) => Self::slot(&p, te_r, "TE", &txt, true, false, Some(prog), None),
                None => Self::slot(&p, te_r, "TE", &s.te.to_string(), s.owned && s.te > 0, s.owned && s.te >= TE_MAX,
                    Some(if s.owned { s.te as f32 / TE_MAX as f32 } else { 0.0 }), None),
            }
        }
        let info_w = rect.right() - sx - 12.0;
        p.text(pos2(sx, y + 118.0), Align2::LEFT_CENTER, elide(&p, &e.group, font(11.5), info_w), font(11.5), MUTED);
        if let Some(j) = jobs.first() {
            // Industry status replaces the tech line while a job is on.
            let (txt, col) = if j.is_ready(now) {
                (format!("{} ready to deliver", j.activity.label()), GOLD)
            } else if j.status == "paused" {
                (format!("{} paused", j.activity.label()), MUTED)
            } else {
                (format!("{} · {}", j.activity.label(), esi::format_remaining(j.end - now)), GOLD_PALE)
            };
            let txt = if jobs.len() > 1 { format!("{txt}  +{}", jobs.len() - 1) } else { txt };
            p.text(pos2(sx, y + 134.0), Align2::LEFT_CENTER, elide(&p, &txt, font_bold(11.5), info_w), font_bold(11.5), col);
        } else {
            let tech = if e.market { e.tech.clone() } else { format!("{}  ·  not on market", e.tech) };
            p.text(pos2(sx, y + 134.0), Align2::LEFT_CENTER, tech, font(11.5), MUTED);
        }

        // footer
        let fy = rect.bottom() - 21.0;
        p.rect_filled(Rect::from_min_size(pos2(x + 12.0, rect.bottom() - 40.0), vec2(rect.width() - 24.0, 1.0)), 0,
            Color32::from_white_alpha(18));
        let (text, color) = if s.ignored {
            ("Ignored", MUTED)
        } else if researched {
            ("★ Fully researched", GOLD)
        } else if s.owned {
            ("✓ Owned", GOLD_PALE)
        } else {
            ("✗ Missing", MISSING)
        };
        p.text(pos2(rect.right() - 12.0, fy), Align2::RIGHT_CENTER, text, font_bold(12.5), color);
        let kind = if e.tab == "Other" { e.category.clone() } else { e.tab.trim_end_matches('s').to_string() };
        let k = p.text(pos2(x + 12.0, fy), Align2::LEFT_CENTER, kind, font(11.5), MUTED);
        let mut fx = k.right() + 12.0;
        if s.synced {
            fx = p.text(pos2(fx, fy), Align2::LEFT_CENTER, "• EVE", font(11.5), MUTED).right() + 10.0;
        }
        if !s.notes.is_empty() {
            p.text(pos2(fx, fy), Align2::LEFT_CENTER, "• note", font(11.5), MUTED);
        }

        // context menu
        let researchable = e.researchable;
        let name_for_copy = e.name.clone();
        card_resp.context_menu(|ui| {
            let mut push = |ui: &mut Ui, label: &str, edit: Edit| {
                if ui.button(label).clicked() {
                    edits.push((id, edit));
                    ui.close();
                }
            };
            if can_edit {
                push(ui, if s.owned { "Mark missing" } else { "Mark owned" }, Edit::Owned(!s.owned));
                push(ui, "Add another copy (qty +1)", Edit::Qty(s.qty + 1));
                if s.qty > 1 {
                    push(ui, "Remove a copy (qty −1)", Edit::Qty(s.qty - 1));
                }
                if s.owned && researchable {
                    push(ui, "Set fully researched (ME 10 / TE 20)", Edit::MaxResearch);
                    push(ui, "Reset research", Edit::ResetResearch);
                }
                ui.separator();
            }
            push(ui, if s.fav { "Unfavourite" } else { "Favourite" }, Edit::Fav(!s.fav));
            push(ui, if s.ignored { "Un-ignore" } else { "Ignore (not part of my collection)" }, Edit::Ignored(!s.ignored));
            ui.separator();
            if ui.button("Copy name").clicked() {
                ui.ctx().copy_text(name_for_copy.clone());
                ui.close();
            }
        });
    }

    // ------------------------------------------------------------------ UI: detail panel
    fn detail(&mut self, ui: &mut Ui) {
        let Some(e) = self.selected.and_then(|id| self.entry_by_id(id)).cloned() else {
            ui.add_space(40.0);
            ui.label(RichText::new("Select a blueprint").font(font_bold(17.0)));
            ui.label(RichText::new("Click a card to edit it here.").color(MUTED));
            return;
        };
        let s = self.view_state(e.id);
        let can_edit = !self.character_view_active();
        if self.notes.0 != e.id {
            self.notes = (e.id, s.notes.clone());
        }
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(128.0), Sense::hover());
            if let Some(t) = self.icons.get(&e) {
                ui.painter().image(t.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            } else {
                ui.painter().rect_filled(r, 10, Color32::from_black_alpha(60));
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new(&e.name).font(font_bold(17.0)));
        let info = format!(
            "Makes: {}\n{} › {}\n{} · {}\nType ID {}",
            e.product_name,
            e.category,
            e.group,
            e.tech,
            if e.market { "Market listed" } else { "Not on market" },
            e.id
        );
        ui.label(RichText::new(info).color(MUTED));
        ui.add_space(10.0);

        let list: Vec<SyncedOriginal> = self
            .details
            .get(&e.id)
            .filter(|_| s.synced)
            .map(|l| {
                l.iter().filter(|d| self.f.chars.as_ref().is_none_or(|c| c.contains(&d.character))).cloned().collect()
            })
            .unwrap_or_default();
        let jobs: Vec<esi::Job> = self.jobs_for(e.id).into_iter().cloned().collect();
        if !jobs.is_empty() {
            let now = esi::now_unix();
            ui.label(RichText::new("In industry").font(font_bold(14.0)));
            egui::Frame::new().fill(GROUND).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for j in &jobs {
                    let levels = match (j.activity, j.from_level, j.to_level()) {
                        (esi::Activity::MeResearch | esi::Activity::TeResearch, Some(f), Some(t)) => format!(" {f} → {t}"),
                        (esi::Activity::MeResearch | esi::Activity::TeResearch, _, _) => format!(" +{} levels", j.runs),
                        (_, _, _) => format!(" ×{}", j.runs),
                    };
                    ui.label(RichText::new(format!("{}{levels}", j.activity.label())).color(GOLD_PALE));
                    let when = if j.is_ready(now) {
                        "Ready to deliver".to_string()
                    } else if j.status == "paused" {
                        "Paused".to_string()
                    } else {
                        format!("{} left", esi::format_remaining(j.end - now))
                    };
                    ui.label(RichText::new(format!("{} · {when}", j.character)).color(MUTED).font(font(11.5)));
                    ui.add(egui::ProgressBar::new(j.progress(now)).fill(if j.is_ready(now) { GOLD } else { GOLD_PALE }).desired_height(4.0));
                    ui.add_space(4.0);
                }
            });
            ui.add_space(8.0);
        }
        if !list.is_empty() {
            let total: u32 = list.iter().map(|d| d.count).sum();
            ui.label(RichText::new(format!("In EVE: {total} original{}", if total == 1 { "" } else { "s" })).font(font_bold(14.0)));
            egui::Frame::new().fill(GROUND).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("originals").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
                    for (i, d) in list.iter().enumerate() {
                        let best = i == 0;
                        let c = if best { GOLD_PALE } else { TEXT };
                        ui.label(RichText::new(&d.character).color(c));
                        ui.label(RichText::new(format!("ME {} / TE {}", d.me, d.te)).color(c));
                        ui.label(RichText::new(if d.count > 1 { format!("×{}", d.count) } else { String::new() }).color(MUTED));
                        ui.end_row();
                    }
                });
            });
            ui.label(RichText::new("The card shows the best-researched one. Changes below are replaced on the next sync.").color(MUTED).font(font(11.5)));
            ui.add_space(8.0);
        }
        if !can_edit {
            ui.label(RichText::new("Showing selected characters only. Switch to All characters to edit ownership and research.").color(MUTED).font(font(11.5)));
            ui.add_space(4.0);
        }
        ui.add_enabled_ui(can_edit, |ui| self.ownership_controls(ui, &e, &s));
        ui.add_space(4.0);
        let mut fav = s.fav;
        if ui.checkbox(&mut fav, "Favourite").changed() {
            self.coll.edit(e.id, Edit::Fav(fav));
        }
        let mut ignored = s.ignored;
        if ui.checkbox(&mut ignored, "Ignore (exclude from collection)").changed() {
            self.coll.edit(e.id, Edit::Ignored(ignored));
        }
        ui.add_space(8.0);
        ui.label("Notes");
        let resp = ui.add(
            egui::TextEdit::multiline(&mut self.notes.1)
                .hint_text("Where it is, what you paid, plans…")
                .desired_width(f32::INFINITY)
                .desired_rows(8),
        );
        if resp.changed() {
            self.coll.edit(e.id, Edit::Notes(self.notes.1.clone()));
        }
    }

    fn ownership_controls(&mut self, ui: &mut Ui, e: &Entry, s: &data::State) {
        let mut owned = s.owned;
        if ui.checkbox(&mut owned, "Owned").changed() {
            self.coll.edit(e.id, Edit::Owned(owned));
        }
        let research = s.owned && e.researchable;
        egui::Grid::new("detail_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Quantity");
            if let Some(v) = stepper(ui, s.qty as i32, 0, 999, 1, true) {
                self.coll.edit(e.id, Edit::Qty(v as u32));
            }
            ui.end_row();
            ui.label("Material Eff.");
            if let Some(v) = stepper(ui, s.me as i32, 0, ME_MAX as i32, 1, research) {
                self.coll.edit(e.id, Edit::Me(v as u8));
            }
            ui.end_row();
            ui.label("Time Eff.");
            if let Some(v) = stepper(ui, s.te as i32, 0, TE_MAX as i32, 2, research) {
                self.coll.edit(e.id, Edit::Te(v as u8));
            }
            ui.end_row();
        });
        ui.add_space(4.0);
        if ui
            .add_enabled(e.researchable, egui::Button::new("Max research (ME 10 / TE 20)").min_size(vec2(ui.available_width(), 30.0)))
            .clicked()
        {
            self.coll.edit(e.id, Edit::MaxResearch);
        }
    }

    // ------------------------------------------------------------------ dialogs
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(login) = &self.login {
            let url = login.state.lock().map(|g| g.url.clone()).unwrap_or_default();
            let mut cancel = false;
            egui::Modal::new(egui::Id::unique("login")).show(ctx, |ui| {
                ui.set_width(440.0);
                ui.label(RichText::new("Log in with EVE").font(font_bold(17.0)));
                ui.add_space(6.0);
                ui.label(
                    "Your browser should have opened EVE's login page. Pick a character and approve access \
                     to its blueprints, then come back here.",
                );
                ui.label(RichText::new("The tracker only asks to read blueprints. It can't see or change anything else.").color(MUTED));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    if ui.button("Open the page again").clicked() {
                        esi::open_in_browser(&url);
                    }
                    if ui.button("Copy link").clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if cancel {
                login.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                self.login = None;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        if let Some(p) = &self.build {
            let (msg, done, total) = p.lock().map(|g| (g.message.clone(), g.done, g.total)).unwrap_or_default();
            egui::Modal::new(egui::Id::unique("build")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(RichText::new("Updating game data").font(font_bold(16.0)));
                ui.add_space(6.0);
                let (frac, text) = if total > 100 {
                    (done as f32 / total as f32, format!("{msg}  {:.1} / {:.1} MB", done as f64 / 1e6, total as f64 / 1e6))
                } else if total > 0 {
                    (done as f32 / total as f32, msg)
                } else {
                    (0.0, format!("{msg}  {:.1} MB", done as f64 / 1e6))
                };
                ui.label(text);
                ui.add(egui::ProgressBar::new(frac).fill(GOLD).desired_height(8.0));
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        let mut close = false;
        match &self.dialog {
            Dialog::None => {}
            Dialog::FirstRun => {
                let mut action = 0;
                egui::Modal::new(egui::Id::unique("first")).show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.label(RichText::new("Game data needed").font(font_bold(17.0)));
                    ui.add_space(6.0);
                    ui.label(
                        "The tracker needs the list of blueprints from EVE's official Static Data Export.\n\n\
                         Download it now? It's a one-off download of roughly 100 MB; only the blueprint list is kept.",
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new("Download").color(GROUND).font(font_bold(13.5))).fill(GOLD)).clicked() {
                            action = 1;
                        }
                        if ui.button("Use a local SDE zip…").clicked() {
                            action = 2;
                        }
                        if ui.button("Later").clicked() {
                            action = 3;
                        }
                    });
                });
                match action {
                    1 => {
                        close = true;
                        self.start_build(None, ctx);
                    }
                    2 => {
                        close = true;
                        if let Some(p) = rfd::FileDialog::new().add_filter("Zip", &["zip"]).pick_file() {
                            self.start_build(Some(p), ctx);
                        }
                    }
                    3 => close = true,
                    _ => {}
                }
            }
            Dialog::Message { title, body } => {
                let r = egui::Modal::new(egui::Id::unique("msg")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(RichText::new(title).font(font_bold(16.0)));
                    ui.add_space(6.0);
                    ui.label(body);
                    ui.add_space(10.0);
                    ui.button("OK").clicked()
                });
                close = r.inner || r.should_close();
            }
            Dialog::About => {
                let mut open_folder = false;
                let r = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
                    ui.set_width(400.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(data::APP_NAME).font(font_bold(18.0)));
                        ui.label(RichText::new(format!("v{}", data::VERSION)).font(font_bold(14.0)).color(GOLD));
                    });
                    ui.label(RichText::new("by Solus Interactive").color(MUTED));
                    ui.add_space(8.0);
                    ui.label("Track your EVE Online blueprint original collection.");
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("Data folder: {}", self.dir.display())).color(MUTED).font(font(11.5)));
                    ui.label(
                        RichText::new("Game data and images © CCP hf. UI font: Inter (SIL Open Font License).")
                            .color(MUTED)
                            .font(font(11.5)),
                    );
                    ui.add_space(10.0);
                    let mut close = false;
                    ui.horizontal(|ui| {
                        if ui.button("Open data folder").clicked() {
                            open_folder = true;
                        }
                        close = ui.button("Close").clicked();
                    });
                    close
                });
                if open_folder {
                    self.open_data_folder();
                }
                close = r.inner || r.should_close();
            }
            Dialog::ConfirmRestore(path) => {
                let path = path.clone();
                let mut answer = None;
                egui::Modal::new(egui::Id::unique("restore")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(RichText::new("Restore collection?").font(font_bold(16.0)));
                    ui.add_space(6.0);
                    ui.label(format!("Replace your current collection with\n{}?", path.display()));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Replace").clicked() {
                            answer = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            answer = Some(false);
                        }
                    });
                });
                if let Some(yes) = answer {
                    close = true;
                    if yes {
                        let parsed = std::fs::read_to_string(&path)
                            .map_err(|e| e.to_string())
                            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()));
                        match parsed {
                            Ok(states) => {
                                self.coll.replace_all(states);
                                self.refilter_if_needed(true);
                            }
                            Err(e) => {
                                self.dialog = Dialog::Message { title: "Not a valid backup".into(), body: e };
                                return;
                            }
                        }
                    }
                }
            }
        }
        if close {
            self.dialog = Dialog::None;
        }
    }

    fn test_hook(&mut self, ctx: &egui::Context) {
        let Some(t) = &mut self.test else { return };
        t.frame += 1;
        if t.frame == 1 && std::env::var_os("BPO_TEST_LOGIN").is_some() {
            self.start_login(ctx);
        }
        let Some(t) = &mut self.test else { return };
        if t.frame == 8 {
            let out = t.out.clone();
            let done = t.done.clone();
            let ctx2 = ctx.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    let _ = buf.save(&out);
                }
                done.store(true, std::sync::atomic::Ordering::SeqCst);
                ctx2.request_repaint();
            })));
        }
        if t.done.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

impl eframe::App for TrackerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.icons.poll(&ctx);
        self.ctx.get_or_insert_with(|| ctx.clone());
        if !self.started_sync && self.test.is_none() && !self.accounts.is_empty() && !self.entries.is_empty() {
            self.started_sync = true;
            self.start_sync(false); // pick up changes since last time
        }
        self.poll_build();
        self.poll_esi();
        self.refilter_if_needed(false);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::right("detail")
            .exact_size(300.0)
            .frame(egui::Frame::new().fill(CHARCOAL).inner_margin(egui::Margin::symmetric(14, 8)).stroke(Stroke::new(1.0, BORDER)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.detail(ui));
            });
        egui::CentralPanel::default_margins()
            .frame(egui::Frame::new().fill(GROUND).inner_margin(egui::Margin { left: 16, right: 8, top: 6, bottom: 0 }))
            .show(ui, |ui| {
                self.header_area(ui);
                self.grid(ui);
            });
        self.dialogs(&ctx);

        self.coll.save_if_due();
        if self.coll.has_unsaved() {
            ctx.request_repaint_after(std::time::Duration::from_millis(450));
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.coll.save();
        }
        self.test_hook(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.coll.save();
    }
}

// ------------------------------------------------------------------ small widgets
fn combo(ui: &mut Ui, id: &str, width: f32, value: &mut usize, options: &[&str]) {
    // Tall enough to show every option without scrolling.
    egui::ComboBox::from_id_salt(id).width(width).height(500.0).selected_text(options[*value]).show_ui(ui, |ui| {
        for (i, o) in options.iter().enumerate() {
            ui.selectable_value(value, i, *o);
        }
    });
}

/// − value + control. Returns the new value when changed.
fn stepper(ui: &mut Ui, value: i32, min: i32, max: i32, step: i32, enabled: bool) -> Option<i32> {
    let mut out = None;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            let b = |t: &str| egui::Button::new(RichText::new(t).font(font_bold(14.0))).min_size(vec2(30.0, 26.0));
            if ui.add_enabled(value > min, b("−")).clicked() {
                out = Some((value - step).max(min));
            }
            let mut v = value;
            let r = ui.add(egui::DragValue::new(&mut v).range(min..=max).speed(0.1 * step as f64).min_decimals(0));
            if r.changed() {
                out = Some(v);
            }
            if ui.add_enabled(value < max, b("+")).clicked() {
                out = Some((value + step).min(max));
            }
        });
    });
    out
}

fn elide(p: &egui::Painter, text: &str, font: egui::FontId, max_w: f32) -> String {
    let w = |s: &str| p.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x;
    if w(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if w(&s) <= max_w { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_string() + "…"
}

fn now_hhmm() -> String {
    // Local time isn't in std; UTC matches EVE time, which players know well.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{:02}:{:02} EVE", (secs / 3600) % 24, (secs / 60) % 60)
}

/// A soft diagonal band of light that sweeps across a card while one of its jobs is running.
fn paint_sweep(p: &egui::Painter, rect: Rect, t: f32) {
    let band = 140.0;
    let skew = 70.0;
    let travel = rect.width() + band + skew * 2.0;
    let x0 = rect.left() - band - skew + (t * 110.0) % (travel + 160.0); // pause a beat between passes
    let (top, bot) = (rect.top(), rect.bottom());
    let clear = Color32::TRANSPARENT;
    let glow = Color32::from_rgba_unmultiplied(0xFF, 0xD2, 0x7A, 26);
    let mut mesh = egui::Mesh::default();
    let pts = [
        (pos2(x0 + skew, top), clear),
        (pos2(x0, bot), clear),
        (pos2(x0 + band / 2.0 + skew, top), glow),
        (pos2(x0 + band / 2.0, bot), glow),
        (pos2(x0 + band + skew, top), clear),
        (pos2(x0 + band, bot), clear),
    ];
    for (pos, color) in pts {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 3);
    mesh.add_triangle(0, 3, 2);
    mesh.add_triangle(2, 3, 5);
    mesh.add_triangle(2, 5, 4);
    p.add(egui::Shape::mesh(mesh));
}
