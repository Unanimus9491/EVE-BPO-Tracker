//! The tracker window: tabs, filters, a virtualised card grid with group headers, and a detail panel.

use crate::data::{self, Collection, Edit, Entry, ME_MAX, Progress, TABS, TE_MAX};
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

const STATUS: [&str; 8] = [
    "Any status",
    "Missing",
    "Owned",
    "Owned – not fully researched",
    "Fully researched",
    "Duplicates (qty > 1)",
    "Favourites",
    "Ignored",
];
const MARKET: [&str; 3] = ["Market + non-market", "Market listed only", "Not on market only"];
const SORT: [&str; 4] = ["Sort: Name", "Sort: Group", "Sort: Missing first", "Sort: Owned first"];
const SORT_GROUP: usize = 1;

enum Item {
    Header { group: String, category: String, ids: Vec<u32> },
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
}

pub struct TrackerApp {
    dir: PathBuf,
    entries: Vec<Entry>,
    coll: Collection,
    icons: Icons,
    f: Filters,
    applied: Option<Filters>,
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
                market: 0,
                sort: 0,
                hide_ignored: true,
            },
            applied: None,
            applied_rev: 0,
            items: Vec::new(),
            shown: 0,
            techs: Vec::new(),
            groups: Vec::new(),
            selected: None,
            notes: (0, String::new()),
            build: None,
            test: None,
        };
        if let Some(out) = std::env::var_os("BPO_SCREENSHOT") {
            let num = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
            app.f.tab = num("BPO_TEST_TAB").unwrap_or(0);
            app.f.sort = num("BPO_TEST_SORT").unwrap_or(0);
            app.f.status = num("BPO_TEST_STATUS").unwrap_or(0);
            app.selected = num("BPO_TEST_SELECT").map(|v| v as u32);
            app.f.search = std::env::var("BPO_TEST_SEARCH").unwrap_or_default();
            app.test = Some(TestHook { out: out.into(), frame: 0, done: Default::default() });
        }
        app.refresh_options();
        app
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
        let s = self.coll.get(e.id);
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
            _ => true,
        }
    }

    /// Rebuild the visible list when filters change (not on every tick, so a card you
    /// just ticked doesn't vanish from under the mouse while a status filter is on).
    fn refilter_if_needed(&mut self, force: bool) {
        if !force && self.applied.as_ref() == Some(&self.f) {
            return;
        }
        if self.applied.as_ref().is_none_or(|a| a.tab != self.f.tab) {
            self.refresh_options();
        }
        let q = self.f.search.trim().to_lowercase();
        let mut idx: Vec<usize> = (0..self.entries.len()).filter(|&i| self.matches(&self.entries[i], &q)).collect();
        let key = |i: &usize| self.entries[*i].name.to_lowercase();
        match self.f.sort {
            SORT_GROUP => idx.sort_by_cached_key(|i| {
                let e = &self.entries[*i];
                (e.category.to_lowercase(), e.group.to_lowercase(), e.name.to_lowercase())
            }),
            2 | 3 => {
                let missing_first = self.f.sort == 2;
                idx.sort_by_cached_key(|i| (self.coll.get(self.entries[*i].id).owned == missing_first, key(i)))
            }
            _ => idx.sort_by_cached_key(key),
        }
        self.shown = idx.len();
        self.items.clear();
        for i in idx {
            if self.f.sort == SORT_GROUP {
                let e = &self.entries[i];
                let new_group = !matches!(self.items.iter().rev().find(|it| matches!(it, Item::Header { .. })),
                    Some(Item::Header { group, category, .. }) if *group == e.group && *category == e.category);
                if new_group {
                    self.items.push(Item::Header { group: e.group.clone(), category: e.category.clone(), ids: Vec::new() });
                }
                if let Some(Item::Header { ids, .. }) = self.items.iter_mut().rev().find(|it| matches!(it, Item::Header { .. })) {
                    ids.push(e.id);
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
        });
    }

    fn stats_text(&self) -> (usize, usize, usize) {
        let (mut counted, mut owned, mut full) = (0, 0, 0);
        for e in self.entries.iter().filter(|e| self.in_tab(e)) {
            let s = self.coll.get(e.id);
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
            });
        });

        // filters
        ui.horizontal(|ui| {
            combo(ui, "status", 190.0, &mut self.f.status, &STATUS);
            let tech_label = self.f.tech.clone().unwrap_or_else(|| "Any tech".into());
            egui::ComboBox::from_id_salt("tech").width(120.0).selected_text(tech_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.f.tech, None, "Any tech");
                for t in &self.techs {
                    ui.selectable_value(&mut self.f.tech, Some(t.clone()), t);
                }
            });
            let group_label = self.f.group.clone().unwrap_or_else(|| "Any group".into());
            egui::ComboBox::from_id_salt("group").width(210.0).height(400.0).selected_text(group_label).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut self.f.group, None, "Any group");
                    for g in &self.groups {
                        ui.selectable_value(&mut self.f.group, Some(g.clone()), g);
                    }
                },
            );
            combo(ui, "market", 170.0, &mut self.f.market, &MARKET);
            combo(ui, "sort", 140.0, &mut self.f.sort, &SORT);
            ui.checkbox(&mut self.f.hide_ignored, "Hide ignored");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("Showing {}", self.shown)).color(MUTED));
            });
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
        let width = ui.available_width();
        let cols = ((width + 2.0) / CELL_W).floor().max(1.0) as usize;
        let rows = self.layout_rows(cols);
        let total_h = rows.last().map(|(_, y, h)| y + h).unwrap_or(0.0);
        let mut edits: Vec<(u32, Edit)> = Vec::new();
        let mut select: Option<u32> = None;
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
                            if let Item::Header { group, category, ids } = &self.items[*i] {
                                let rect = Rect::from_min_size(origin + vec2(0.0, *y), vec2(cols as f32 * CELL_W, HEADER_H));
                                self.paint_header(ui, rect, group, category, ids);
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
        if select.is_some() {
            self.selected = select;
        }
    }

    fn paint_header(&self, ui: &Ui, rect: Rect, group: &str, category: &str, ids: &[u32]) {
        let r = Rect::from_min_max(rect.min + vec2(GAP / 2.0 + 2.0, 0.0), rect.max - vec2(GAP / 2.0 + 2.0, 0.0));
        let p = ui.painter();
        let owned = ids.iter().filter(|i| self.coll.get(**i).owned).count();
        let full = ids.iter().filter(|i| self.coll.get(**i).fully_researched()).count();
        let complete = !ids.is_empty() && owned == ids.len();
        let mid = r.top() + 27.0;
        let title = p.text(pos2(r.left(), mid), Align2::LEFT_CENTER, group, font_bold(17.0), TEXT);
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
        p.text(pos2(r.center().x, r.top() + 28.0), Align2::CENTER_CENTER, value, font_bold(14.0), vc);
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
        let s = self.coll.get(id).clone();
        let researched = s.fully_researched();
        let selected = self.selected == Some(id);

        let base = ui.scope_id().with(("card", id));
        let card_resp = ui.interact(rect, base, Sense::click());
        let x = rect.left();
        let y = rect.top();
        let star_r = Rect::from_min_size(pos2(x + 8.0, y + 8.0), vec2(24.0, 24.0));
        let sx = x + 116.0;
        let bpo_r = Rect::from_min_size(pos2(sx, y + 50.0), Vec2::splat(SLOT_SIZE));
        let me_r = Rect::from_min_size(pos2(sx + SLOT_SIZE + 8.0, y + 50.0), Vec2::splat(SLOT_SIZE));
        let te_r = Rect::from_min_size(pos2(sx + 2.0 * (SLOT_SIZE + 8.0), y + 50.0), Vec2::splat(SLOT_SIZE));
        let research_ok = e.researchable && s.owned;

        let star = ui.interact(star_r, base.with("star"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let bpo = ui
            .interact(bpo_r, base.with("bpo"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(if s.owned { "Click: mark missing" } else { "Click: mark owned" });
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
        if bpo.clicked() {
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
            Self::slot(&p, me_r, "ME", &s.me.to_string(), s.owned && s.me > 0, s.owned && s.me >= ME_MAX,
                Some(if s.owned { s.me as f32 / ME_MAX as f32 } else { 0.0 }), None);
            Self::slot(&p, te_r, "TE", &s.te.to_string(), s.owned && s.te > 0, s.owned && s.te >= TE_MAX,
                Some(if s.owned { s.te as f32 / TE_MAX as f32 } else { 0.0 }), None);
        }
        let info_w = rect.right() - sx - 12.0;
        p.text(pos2(sx, y + 118.0), Align2::LEFT_CENTER, elide(&p, &e.group, font(11.5), info_w), font(11.5), MUTED);
        let tech = if e.market { e.tech.clone() } else { format!("{}  ·  not on market", e.tech) };
        p.text(pos2(sx, y + 134.0), Align2::LEFT_CENTER, tech, font(11.5), MUTED);

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
        if !s.notes.is_empty() {
            p.text(pos2(k.right() + 12.0, fy), Align2::LEFT_CENTER, "• note", font(11.5), MUTED);
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
        let s = self.coll.get(e.id).clone();
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

    // ------------------------------------------------------------------ dialogs
    fn dialogs(&mut self, ctx: &egui::Context) {
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
        self.poll_build();
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
    egui::ComboBox::from_id_salt(id).width(width).selected_text(options[*value]).show_ui(ui, |ui| {
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
