#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Local, Datelike};
use eframe::egui;
use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, atomic::{AtomicU32, Ordering}, mpsc::{self, Receiver}};
use std::thread;
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Spool {
    id: i64,
    name: String,
    manufacturer: String,
    material: String,
    color: String,
    initial_g: f64,
    remaining_g: f64,
    empty_spool_g: f64,
    price_eur: f64,
    location: String,
    ams_unit: i32,
    ams_slot: i32,
    notes: String,
}

#[derive(Debug, Clone, Default)]
struct PrintUsage {
    tool: usize,
    grams: f64,
    label: String,
}

#[derive(Debug, Clone, Default)]
struct ImportedPrint {
    source_path: PathBuf,
    display_name: String,
    usages: Vec<PrintUsage>,
    total_g: f64,
    parsed_from: String,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct HistoryRow {
    timestamp: String,
    filename: String,
    status: String,
    spool_name: String,
    grams: f64,
    note: String,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Import,
    History,
    Statistics,
    Settings,
    About,
}

fn tr<'a>(english: bool, de: &'a str, en: &'a str) -> &'a str { if english { en } else { de } }
fn color_display<'a>(english: bool, value: &'a str) -> &'a str {
    if !english { return value; }
    match value {
        "Schwarz" => "Black", "Weiß" | "Weiss" => "White", "Grau" => "Grey", "Silber" => "Silver",
        "Rot" => "Red", "Orange" => "Orange", "Gelb" => "Yellow", "Grün" | "Gruen" => "Green",
        "Blau" => "Blue", "Dunkelblau" => "Dark Blue", "Türkis" => "Turquoise", "Lila" => "Purple",
        "Rosa" => "Pink", "Braun" => "Brown", "Beige" => "Beige", "Transparent" => "Transparent",
        "Mehrfarbig" => "Multicolor", _ => value,
    }
}


struct FilamentApp {
    conn: Connection,
    db_path: PathBuf,
    page: Page,
    spools: Vec<Spool>,
    history: Vec<HistoryRow>,
    search: String,
    message: String,
    dark_mode: bool,
    english: bool,
    editing: Spool,
    show_spool_editor: bool,
    imported: Option<ImportedPrint>,
    assignment: HashMap<usize, i64>,
    partial_percent: f64,
    manual_grams: f64,
    bambu_path: String,
    selected_qr_spool: Option<i64>,
    weigh_total_g: f64,
    calc_progress: Arc<AtomicU32>,
    calc_rx: Option<Receiver<std::result::Result<ImportedPrint, String>>>,
    calc_active: bool,
    layer_height: f64,
    nozzle_mm: f64,
    wall_count: f64,
    infill_percent: f64,
    support_percent: f64,
    spool_textures: HashMap<String, egui::TextureHandle>,
}

impl FilamentApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (conn, db_path) = open_database().expect("Datenbank konnte nicht geöffnet werden");
        init_database(&conn).expect("Datenbank konnte nicht initialisiert werden");
        let dark_mode = true;
        let english = load_setting(&conn, "language").map(|v| v == "en").unwrap_or(false);
        apply_theme(&cc.egui_ctx, true);
        let mut style = (*cc.egui_ctx.style()).clone();
        for font_id in style.text_styles.values_mut() { font_id.size += 3.0; }
        style.spacing.button_padding = egui::vec2(12.0, 9.0);
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        cc.egui_ctx.set_style(style);
        let bambu_path = load_setting(&conn, "bambu_path").unwrap_or_default();
        let mut app = Self {
            conn,
            db_path,
            page: Page::Overview,
            spools: vec![],
            history: vec![],
            search: String::new(),
            message: String::new(),
            dark_mode,
            english,
            editing: default_spool(),
            show_spool_editor: false,
            imported: None,
            assignment: HashMap::new(),
            partial_percent: 50.0,
            manual_grams: 0.0,
            bambu_path,
            selected_qr_spool: None,
            weigh_total_g: 0.0,
            calc_progress: Arc::new(AtomicU32::new(0)),
            calc_rx: None,
            calc_active: false,
            layer_height: 0.20,
            nozzle_mm: 0.40,
            wall_count: 3.0,
            infill_percent: 15.0,
            support_percent: 0.0,
            spool_textures: HashMap::new(),
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        self.spools = load_spools(&self.conn).unwrap_or_default();
        self.history = load_history(&self.conn).unwrap_or_default();
    }

    fn ui_top(&mut self, ctx: &egui::Context) {
        let header_bg = if self.dark_mode { egui::Color32::from_rgb(7, 12, 17) } else { egui::Color32::from_rgb(238, 244, 247) };
        let border = if self.dark_mode { egui::Color32::from_rgb(28, 43, 52) } else { egui::Color32::from_rgb(190, 205, 214) };
        let title_color = if self.dark_mode { egui::Color32::WHITE } else { egui::Color32::from_rgb(25, 38, 45) };
        egui::TopBottomPanel::top("top_header")
            .exact_height(88.0)
            .frame(egui::Frame::default().fill(header_bg).stroke(egui::Stroke::new(1.0_f32, border)).inner_margin(egui::Margin::symmetric(18, 10)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let logo = load_logo_small(ctx);
                    ui.image((logo.id(), egui::vec2(68.0, 68.0)));
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.add_space(13.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("SUNLU").size(27.0).strong().color(egui::Color32::from_rgb(0, 190, 198)));
                            ui.label(egui::RichText::new("Filament Tracker").size(27.0).strong().color(title_color));
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dark_button_mode(ui, tr(self.english, "Einstellungen", "Settings"), 130.0, self.dark_mode).clicked() { self.page = Page::Settings; }
                        if dark_button_mode(ui, if self.english { "DE" } else { "EN" }, 58.0, self.dark_mode).clicked() {
                            self.english = !self.english;
                            let _ = save_setting(&self.conn, "language", if self.english { "en" } else { "de" });
                        }
                    });
                });
            });
    }


    fn ui_footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("app_footer")
            .exact_height(32.0)
            .frame(
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(7, 12, 17))
                    .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(28, 43, 52)))
                    .inner_margin(egui::Margin::symmetric(14, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Version 1.10.1").size(12.5).color(egui::Color32::from_rgb(150, 164, 173)));
                    ui.separator();
                    ui.label(egui::RichText::new("filamentbestand.db").size(12.5).color(egui::Color32::from_rgb(150, 164, 173)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let status = if self.calc_active { tr(self.english, "Berechnung läuft …", "Calculating …") } else { tr(self.english, "Bereit", "Ready") };
                        ui.label(egui::RichText::new(status).size(12.5).color(egui::Color32::from_rgb(0, 205, 212)));
                    });
                });
            });
    }

    fn ui_sidebar(&mut self, ctx: &egui::Context) {
        let bg = if self.dark_mode { egui::Color32::from_rgb(7, 13, 18) } else { egui::Color32::from_rgb(232, 240, 244) };
        let normal_text = if self.dark_mode { egui::Color32::from_rgb(210, 218, 223) } else { egui::Color32::from_rgb(35, 50, 58) };
        egui::SidePanel::left("sidebar")
            .exact_width(205.0)
            .frame(egui::Frame::default().fill(bg).stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(25, 80, 88))).inner_margin(egui::Margin::symmetric(11, 20)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 7.0;
                let entries = [
                    (Page::Overview, tr(self.english, "Übersicht Spulen", "Spool Overview")),
                    (Page::History, tr(self.english, "Verlauf", "History")),
                    (Page::Statistics, tr(self.english, "Statistik", "Statistics")),
                    (Page::Settings, tr(self.english, "Einstellungen", "Settings")),
                ];
                for (page, label) in entries {
                    let selected = self.page == page;
                    let button = egui::Button::new(egui::RichText::new(label).size(16.0).color(if selected { egui::Color32::WHITE } else { normal_text }))
                        .fill(if selected { egui::Color32::from_rgb(7, 104, 112) } else { egui::Color32::TRANSPARENT })
                        .stroke(egui::Stroke::new(1.0_f32, if selected { egui::Color32::from_rgb(0, 181, 190) } else { egui::Color32::TRANSPARENT }))
                        .corner_radius(8.0);
                    if ui.add_sized([181.0, if label.len() > 18 { 54.0 } else { 43.0 }], button).clicked() { self.page = page; }
                }
                ui.add_space(48.0);
                let selected = self.page == Page::About;
                let about = tr(self.english, "Über", "About");
                let button = egui::Button::new(egui::RichText::new(about).size(16.0).color(if selected { egui::Color32::WHITE } else { normal_text }))
                    .fill(if selected { egui::Color32::from_rgb(7, 104, 112) } else { egui::Color32::TRANSPARENT })
                    .stroke(egui::Stroke::new(1.0_f32, if selected { egui::Color32::from_rgb(0, 181, 190) } else { egui::Color32::TRANSPARENT }))
                    .corner_radius(8.0);
                if ui.add_sized([181.0, 43.0], button).clicked() { self.page = Page::About; }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(egui::RichText::new("v1.10.1  •  Portable Version").size(12.0).color(if self.dark_mode { egui::Color32::from_rgb(130, 145, 154) } else { egui::Color32::from_rgb(80, 100, 110) }));
                });
            });
    }


    fn ui_overview(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(if self.dark_mode { egui::Color32::from_rgb(8, 14, 19) } else { egui::Color32::from_rgb(245, 248, 250) }).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                let full = ui.available_size();
                let left_w = (full.x * 0.39).clamp(520.0, 690.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(left_w, full.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                        dark_panel(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.heading(egui::RichText::new(tr(self.english, "Übersicht Spulen", "Spool Overview")).size(25.0).strong().color(if self.dark_mode { egui::Color32::WHITE } else { egui::Color32::from_rgb(25, 38, 45) }));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if teal_button(ui, tr(self.english, "+  Neue Spule", "+  New Spool"), 148.0).clicked() { self.editing = default_spool(); self.show_spool_editor = true; }
                                });
                            });
                            ui.add_space(8.0);
                            ui.add_sized([ui.available_width(), 41.0], egui::TextEdit::singleline(&mut self.search).hint_text(tr(self.english, "Spulen suchen …", "Search spools …")));
                            ui.add_space(8.0);
                            let needle = self.search.to_lowercase();
                            let rows: Vec<Spool> = self.spools.iter().filter(|s| needle.is_empty() || format!("{} {} {} {} {}", s.name, s.manufacturer, s.material, s.color, s.location).to_lowercase().contains(&needle)).cloned().collect();
                            egui::ScrollArea::vertical().id_salt("spool_scroll").auto_shrink([false, false]).show(ui, |ui| {
                                for spool in &rows { self.spool_card(ui, spool); ui.add_space(9.0); }
                                let total: f64 = rows.iter().map(|s| s.remaining_g).sum();
                                ui.label(egui::RichText::new(if self.english { format!("Total: {} spools   •   Total remaining: {:.1} g", rows.len(), total) } else { format!("Gesamt: {} Spulen   •   Gesamt Rest: {:.1} g", rows.len(), total) }).size(14.0).color(if self.dark_mode { egui::Color32::from_rgb(150, 164, 173) } else { egui::Color32::from_rgb(70, 90, 100) }));
                            });
                        });
                    });
                    ui.add_space(10.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), full.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                        dark_panel(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("import_panel_scroll")
                                .auto_shrink([false, false])
                                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                                .show(ui, |ui| {
                                    self.import_panel(ui);
                                    ui.add_space(12.0);
                                });
                        });
                    });
                });
            });
        self.spool_editor(ctx);
        self.qr_window(ctx);
    }

    fn spool_texture(&mut self, ctx: &egui::Context, color_name: &str) -> egui::TextureHandle {
        let key = match color_name.to_lowercase().as_str() {
            "weiß" | "weiss" => "Weiss",
            "grün" | "gruen" => "Gruen",
            "grau" => "Grau", "blau" => "Blau", "rot" => "Rot", "gelb" => "Gelb",
            "orange" => "Orange", "braun" => "Braun", "lila" => "Lila", "rosa" => "Rosa",
            "transparent" | "natur" => "Transparent",
            "silber" | "silver" => "Grau", "dunkelblau" | "türkis" | "tuerkis" => "Blau",
            "beige" => "Braun", "mehrfarbig" => "Orange", _ => "Schwarz",
        }.to_string();
        if let Some(t) = self.spool_textures.get(&key) { return t.clone(); }
        let bytes: &[u8] = match key.as_str() {
            "Weiss" => include_bytes!("../assets/Weiss.png"), "Gruen" => include_bytes!("../assets/Gruen.png"),
            "Grau" => include_bytes!("../assets/Grau.png"), "Blau" => include_bytes!("../assets/Blau.png"),
            "Rot" => include_bytes!("../assets/Rot.png"), "Gelb" => include_bytes!("../assets/Gelb.png"),
            "Orange" => include_bytes!("../assets/Orange.png"), "Braun" => include_bytes!("../assets/Braun.png"),
            "Lila" => include_bytes!("../assets/Lila.png"), "Rosa" => include_bytes!("../assets/Rosa.png"),
            "Transparent" => include_bytes!("../assets/Transparent.png"), _ => include_bytes!("../assets/Schwarz.png"),
        };
        let img = image::load_from_memory(bytes).expect("Spulenbild").to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let tex = ctx.load_texture(format!("spool_{key}"), egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR);
        self.spool_textures.insert(key, tex.clone()); tex
    }

    fn spool_card(&mut self, ui: &mut egui::Ui, spool: &Spool) {
        let pct = if spool.initial_g > 0.0 { (spool.remaining_g / spool.initial_g).clamp(0.0, 1.0) } else { 0.0 };
        let tex = self.spool_texture(ui.ctx(), &spool.color);
        egui::Frame::default()
            .fill(if self.dark_mode { egui::Color32::from_rgb(14, 23, 30) } else { egui::Color32::WHITE })
            .stroke(egui::Stroke::new(1.0_f32, if self.dark_mode { egui::Color32::from_rgb(43, 60, 70) } else { egui::Color32::from_rgb(190, 205, 214) }))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_height(118.0);
                ui.horizontal(|ui| {
                    let image_response = ui.add(egui::Image::new((tex.id(), egui::vec2(110.0, 110.0))));
                    let badge_text = spool.material.to_uppercase();
                    let badge_width = (badge_text.chars().count() as f32 * 8.2 + 18.0).clamp(48.0, 104.0);
                    let badge_rect = egui::Rect::from_min_size(
                        image_response.rect.left_bottom() + egui::vec2(3.0, -31.0),
                        egui::vec2(badge_width, 27.0),
                    );
                    let is_white = matches!(spool.color.to_lowercase().as_str(), "weiß" | "weiss" | "transparent" | "natur");
                    let badge_fill = if is_white { egui::Color32::from_rgb(42, 48, 53) } else { color_from_name(&spool.color) };
                    let badge_text_color = if is_white { egui::Color32::WHITE } else {
                        let c = badge_fill;
                        let luminance = 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
                        if luminance > 155.0 { egui::Color32::from_rgb(18, 22, 25) } else { egui::Color32::WHITE }
                    };
                    ui.painter().rect_filled(badge_rect, 5.0, badge_fill);
                    ui.painter().rect_stroke(badge_rect, 5.0, egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(120)), egui::StrokeKind::Inside);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        badge_text,
                        egui::FontId::proportional(13.0),
                        badge_text_color,
                    );
                    ui.add_space(9.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let title = if spool.name.trim().is_empty() { format!("{} {}", spool.manufacturer, spool.material) } else { spool.name.clone() };
                                    ui.label(egui::RichText::new(title).size(18.0).strong().color(if self.dark_mode { egui::Color32::WHITE } else { egui::Color32::from_rgb(25, 38, 45) }));
                                    if spool.ams_slot > 0 {
                                        egui::Frame::default().fill(egui::Color32::from_rgb(11, 28, 35)).stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(91, 111, 122))).corner_radius(5.0).inner_margin(egui::Margin::symmetric(7, 3)).show(ui, |ui| {
                                            ui.label(egui::RichText::new(format!("AMS {}-{}", spool.ams_unit.max(1), spool.ams_slot)).size(12.0).color(egui::Color32::from_rgb(225, 232, 236)));
                                        });
                                    }
                                });
                                ui.label(egui::RichText::new(color_display(self.english, &spool.color)).size(15.0).color(if self.dark_mode { egui::Color32::from_rgb(188, 198, 205) } else { egui::Color32::from_rgb(65, 80, 88) }));
                                ui.add_space(3.0);
                                ui.label(egui::RichText::new(if self.english { format!("Remaining: {:.1} g   /   {:.0} g", spool.remaining_g, spool.initial_g) } else { format!("Rest: {:.1} g   /   {:.0} g", spool.remaining_g, spool.initial_g) }).size(15.0).color(if self.dark_mode { egui::Color32::from_rgb(224, 231, 235) } else { egui::Color32::from_rgb(35, 50, 58) }));
                            });
                        });
                        ui.add_space(7.0);
                        ui.horizontal(|ui| {
                            let bar_w = (ui.available_width() - 58.0).clamp(150.0, 310.0);
                            ui.add_sized([bar_w, 14.0], egui::ProgressBar::new(pct as f32).fill(egui::Color32::from_rgb(0, 184, 190)).text(""));
                            ui.label(egui::RichText::new(format!("{:.0}%", pct*100.0)).size(15.0).strong().color(egui::Color32::from_rgb(0, 225, 230)));
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        ui.menu_button(egui::RichText::new("⋮").size(23.0).color(egui::Color32::from_rgb(190,200,207)), |ui| {
                            if ui.button(tr(self.english, "QR-Code", "QR Code")).clicked() { self.selected_qr_spool = Some(spool.id); ui.close_menu(); }
                            if ui.button(tr(self.english, "Bearbeiten", "Edit")).clicked() { self.editing = spool.clone(); self.show_spool_editor = true; ui.close_menu(); }
                            if ui.button(tr(self.english, "Löschen", "Delete")).clicked() { let _ = self.conn.execute("DELETE FROM spools WHERE id=?1", [spool.id]); self.reload(); ui.close_menu(); }
                        });
                    });
                });
            });
    }

    fn spool_editor(&mut self, ctx: &egui::Context) {
        if !self.show_spool_editor { return; }
        let mut open = self.show_spool_editor;
        let mut close_after = false;
        egui::Window::new(if self.editing.id == 0 { tr(self.english, "Neue Rolle", "New Spool") } else { tr(self.english, "Rolle bearbeiten", "Edit Spool") })
            .open(&mut open).resizable(true).show(ctx, |ui| {
                egui::Grid::new("editor").num_columns(2).show(ui, |ui| {
                    field(ui, tr(self.english, "Eigener Name", "Custom Name"), &mut self.editing.name);
                    field(ui, tr(self.english, "Hersteller", "Manufacturer"), &mut self.editing.manufacturer);
                    ui.label("Material");
                    egui::ComboBox::from_id_salt("material_select").selected_text(&self.editing.material).show_ui(ui, |ui| {
                        for m in MATERIALS { ui.selectable_value(&mut self.editing.material, (*m).to_string(), *m); }
                    }); ui.end_row();
                    ui.label(tr(self.english, "Farbe", "Color"));
                    egui::ComboBox::from_id_salt("color_select").selected_text(color_display(self.english, &self.editing.color)).show_ui(ui, |ui| {
                        for c in COLORS { ui.selectable_value(&mut self.editing.color, (*c).to_string(), color_display(self.english, c)); }
                    }); ui.end_row();
                    number(ui, tr(self.english, "Anfangsgewicht (g)", "Initial Weight (g)"), &mut self.editing.initial_g);
                    number(ui, tr(self.english, "Restbestand (g)", "Remaining Weight (g)"), &mut self.editing.remaining_g);
                    number(ui, tr(self.english, "Leergewicht Spule (g)", "Empty Spool Weight (g)"), &mut self.editing.empty_spool_g);
                    ui.label(tr(self.english, "Gewogenes Gesamtgewicht (g)", "Measured Total Weight (g)")); ui.add(egui::DragValue::new(&mut self.weigh_total_g).range(0.0..=5000.0)); ui.end_row();
                    ui.label(""); if ui.button(tr(self.english, "Rest aus Waage berechnen", "Calculate Remaining from Scale")).clicked() { self.editing.remaining_g = (self.weigh_total_g - self.editing.empty_spool_g).max(0.0); } ui.end_row();
                    number(ui, tr(self.english, "Preis (€)", "Price (€)"), &mut self.editing.price_eur);
                    field(ui, tr(self.english, "Lagerort", "Storage Location"), &mut self.editing.location);
                    ui.label(tr(self.english, "AMS Einheit", "AMS Unit")); ui.add(egui::DragValue::new(&mut self.editing.ams_unit).range(0..=4)); ui.end_row();
                    ui.label(tr(self.english, "AMS Fach", "AMS Slot")); ui.add(egui::DragValue::new(&mut self.editing.ams_slot).range(0..=4)); ui.end_row();
                    field(ui, tr(self.english, "Notizen", "Notes"), &mut self.editing.notes);
                });
                ui.horizontal(|ui| {
                    if ui.button(tr(self.english, "Speichern", "Save")).clicked() {
                        match save_spool(&self.conn, &self.editing) {
                            Ok(_) => { self.message = "Rolle gespeichert.".into(); close_after = true; self.reload(); }
                            Err(e) => self.message = format!("Fehler: {e:#}"),
                        }
                    }
                    if ui.button(tr(self.english, "Abbrechen", "Cancel")).clicked() { close_after = true; }
                });
            });
        if close_after { open = false; }
        self.show_spool_editor = open;
    }

    fn ui_import(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::default().fill(if self.dark_mode { egui::Color32::from_rgb(8, 14, 19) } else { egui::Color32::from_rgb(245, 248, 250) }).inner_margin(egui::Margin::same(18))).show(ctx, |ui| {
            self.import_panel(ui);
        });
    }

    fn import_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new(tr(self.english, "Druckdatei einlesen", "Read Print File")).size(25.0).strong());
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if teal_button(ui, tr(self.english, "3MF Datei öffnen", "Open 3MF File"), 160.0).clicked() {
                if let Some(path) = rfd::FileDialog::new().set_title(tr(self.english, "3MF-Datei auswählen", "Select 3MF File")).add_filter("3MF-Dateien", &["3mf"]).pick_file() { self.start_calculation(path); }
            }
            if dark_button(ui, tr(self.english, "Bambu Studio öffnen", "Open Bambu Studio"), 175.0).clicked() {
                match find_bambu_studio(&self.bambu_path) { Some(exe) => { let _ = Command::new(exe).spawn(); self.message = "Bambu Studio wurde gestartet.".into(); }, None => self.message = "Bambu Studio wurde nicht gefunden. Bitte den Pfad unter Einstellungen auswählen.".into() }
            }
        });
        ui.add_space(10.0);
        egui::CollapsingHeader::new(tr(self.english, "Berechnungseinstellungen", "Calculation Settings")).default_open(true).show(ui, |ui| {
            egui::Grid::new("estimate_settings").num_columns(4).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(tr(self.english, "Düse", "Nozzle")); ui.add(egui::DragValue::new(&mut self.nozzle_mm).range(0.2..=1.0).speed(0.05).suffix(" mm"));
                ui.label(tr(self.english, "Schichthöhe", "Layer Height")); ui.add(egui::DragValue::new(&mut self.layer_height).range(0.05..=0.6).speed(0.05).suffix(" mm")); ui.end_row();
                ui.label(tr(self.english, "Wände", "Walls")); ui.add(egui::DragValue::new(&mut self.wall_count).range(1.0..=10.0).speed(1.0));
                ui.label("Infill"); ui.add(egui::DragValue::new(&mut self.infill_percent).range(0.0..=100.0).speed(5.0).suffix(" %")); ui.end_row();
                ui.label(tr(self.english, "Support-Zuschlag", "Support Allowance")); ui.add(egui::DragValue::new(&mut self.support_percent).range(0.0..=100.0).speed(5.0).suffix(" %"));
                ui.label(""); ui.label(""); ui.end_row();
            });
        });
        if self.calc_active {
            let p = self.calc_progress.load(Ordering::Relaxed) as f32 / 100.0;
            ui.add_space(8.0);
            ui.add(egui::ProgressBar::new(p).show_percentage().text("3MF wird analysiert und berechnet …"));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
        }

        if self.imported.is_none() {
            egui::Frame::default().stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0, 180, 188))).corner_radius(10.0).inner_margin(egui::Margin::same(22)).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("3MF").size(28.0).color(egui::Color32::from_rgb(0, 205, 212)).strong());
                    ui.label(egui::RichText::new(tr(self.english, "3MF-Datei auswählen", "Select 3MF File")).size(18.0).strong());
                    ui.label(tr(self.english, "Geslicte Dateien werden exakt ausgelesen. Normale 3MF-Dateien werden geschätzt.", "Sliced files are read precisely. Normal 3MF files are estimated."));
                });
            });
        }
        if !self.message.is_empty() {
            ui.add_space(10.0);
            let err = self.message.contains("nicht") || self.message.contains("Keine") || self.message.contains("Fehler") || self.message.contains("leer");
            let bg = if err { egui::Color32::from_rgb(132, 42, 34) } else { egui::Color32::from_rgb(18, 94, 82) };
            egui::Frame::default().fill(bg).corner_radius(8.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| { ui.label(egui::RichText::new(&self.message).size(16.0).strong().color(egui::Color32::WHITE)); });
        }
        ui.add_space(10.0);
        if let Some(imported) = self.imported.clone() {
            ui.label(egui::RichText::new(&imported.display_name).size(19.0).strong());
            ui.label(egui::RichText::new(format!("Gesamtverbrauch: {:.2} g", imported.total_g)).size(18.0).color(egui::Color32::from_rgb(0, 210, 215)));
            ui.separator();
            egui::Grid::new("usage_grid_dashboard").num_columns(3).spacing([14.0, 10.0]).show(ui, |ui| {
                ui.strong(tr(self.english, "Material / Farbe", "Material / Color")); ui.strong(tr(self.english, "Verbrauch", "Consumption")); ui.strong(tr(self.english, "Zielspule", "Target Spool")); ui.end_row();
                for usage in &imported.usages {
                    ui.label(if usage.label.is_empty() { format!("Filament {}", usage.tool + 1) } else { usage.label.clone() });
                    ui.label(format!("{:.2} g", usage.grams));
                    let selected = self.assignment.get(&usage.tool).copied().unwrap_or(0);
                    egui::ComboBox::from_id_salt(format!("assign_dash_{}", usage.tool)).width(220.0)
                        .selected_text(self.spools.iter().find(|s| s.id == selected).map(|s| format!("{} – {}", s.name, color_display(self.english, &s.color))).unwrap_or_else(|| tr(self.english, "Bitte wählen", "Please select").into()))
                        .show_ui(ui, |ui| { for spool in &self.spools { ui.selectable_value(self.assignment.entry(usage.tool).or_insert(0), spool.id, format!("{} – {} – {:.1} g", spool.name, color_display(self.english, &spool.color), spool.remaining_g)); } });
                    ui.end_row();
                }
            });
            self.inventory_warnings(ui, &imported);
            ui.separator();
            ui.label(egui::RichText::new(tr(self.english, "Nach dem Druck", "After Printing")).size(18.0).strong());
            ui.horizontal_wrapped(|ui| {
                if ui.add_sized([135.0, 58.0], egui::Button::new(tr(self.english, "✓ Druck\nerfolgreich", "✓ Print\nsuccessful"))).clicked() { self.book_print("Erfolgreich", 1.0, None); }
                if ui.add_sized([135.0, 58.0], egui::Button::new(tr(self.english, "✕ Druck\nfehlgeschlagen", "✕ Print\nfailed"))).clicked() { self.book_print("Fehlgeschlagen", 0.0, None); }
                if ui.add_sized([135.0, 58.0], egui::Button::new(tr(self.english, "◔ Teilweise\nverbraucht", "◔ Partially\nused"))).clicked() { self.book_print("Teilweise", self.partial_percent / 100.0, None); }
                if ui.add_sized([135.0, 58.0], egui::Button::new(tr(self.english, "⊖ Nichts\nabbuchen", "⊖ Deduct\nnothing"))).clicked() { self.book_print("Nicht abgebucht", 0.0, None); }
            });
            ui.horizontal(|ui| {
                ui.label(tr(self.english, "Teilmenge:", "Partial amount:")); ui.add(egui::DragValue::new(&mut self.partial_percent).range(0.0..=100.0).suffix(" %"));
                ui.label(tr(self.english, "Manuell:", "Manual:")); ui.add(egui::DragValue::new(&mut self.manual_grams).range(0.0..=10000.0).suffix(" g"));
                if ui.button(tr(self.english, "Manuell abbuchen", "Deduct Manually")).clicked() { self.book_print("Manuell", 0.0, Some(self.manual_grams)); }
            });
        }
    }

    fn inventory_warnings(&self, ui: &mut egui::Ui, imported: &ImportedPrint) {
        for usage in &imported.usages {
            if let Some(id) = self.assignment.get(&usage.tool) {
                if let Some(spool) = self.spools.iter().find(|s| s.id == *id) {
                    if spool.remaining_g < usage.grams {
                        ui.colored_label(egui::Color32::LIGHT_RED, if self.english { format!("WARNING: {} has only {:.1} g; {:.1} g are required.", spool.name, spool.remaining_g, usage.grams) } else { format!("WARNUNG: {} hat nur {:.1} g, benötigt werden {:.1} g.", spool.name, spool.remaining_g, usage.grams) });
                    }
                }
            }
        }
    }

    fn start_calculation(&mut self, path: PathBuf) {
        self.imported = None;
        self.assignment.clear();
        self.message = format!("Berechnung gestartet: {}", path.display());
        self.calc_progress.store(2, Ordering::Relaxed);
        self.calc_active = true;
        let progress = self.calc_progress.clone();
        let settings = EstimateSettings {
            nozzle_mm: self.nozzle_mm,
            layer_height: self.layer_height,
            wall_count: self.wall_count,
            infill_percent: self.infill_percent,
            support_percent: self.support_percent,
        };
        let (tx, rx) = mpsc::channel();
        self.calc_rx = Some(rx);
        thread::spawn(move || {
            progress.store(12, Ordering::Relaxed);
            let result = parse_3mf(&path).or_else(|_| {
                progress.store(35, Ordering::Relaxed);
                estimate_3mf(&path, settings)
            }).map_err(|e| format!("{e:#}"));
            progress.store(100, Ordering::Relaxed);
            let _ = tx.send(result);
        });
    }

    fn import_file(&mut self, path: PathBuf) {
        self.imported = None;
        self.assignment.clear();
        match fs::metadata(&path) {
            Ok(meta) if meta.len() == 0 => {
                self.message = format!("Die ausgewählte Datei ist leer (0 Byte) und kann nicht gelesen werden:\n{}\n\nBitte die Datei erneut von MakerWorld herunterladen oder in Bambu Studio erneut als geslicte 3MF exportieren.", path.display());
                return;
            }
            Err(e) => {
                self.message = format!("Die Datei konnte nicht geöffnet werden:\n{}\n\nFehler: {e}", path.display());
                return;
            }
            _ => {}
        }
        self.message = format!("3MF wird eingelesen: {}", path.display());
        match parse_3mf(&path) {
            Ok(data) => {
                self.assignment.clear();
                for usage in &data.usages {
                    if let Some(spool) = self.spools.iter().find(|s| s.ams_slot as usize == usage.tool + 1) {
                        self.assignment.insert(usage.tool, spool.id);
                    }
                }
                self.message = format!("Datei eingelesen: {:.2} g erkannt.", data.total_g);
                self.imported = Some(data);
            }
            Err(e) => self.message = format!("3MF konnte nicht ausgewertet werden: {e:#}"),
        }
    }

    fn slice_then_import(&mut self, input: PathBuf) {
        match fs::metadata(&input) {
            Ok(meta) if meta.len() == 0 => {
                self.message = format!("Die ausgewählte Quelldatei ist leer (0 Byte):\n{}", input.display());
                return;
            }
            Err(e) => {
                self.message = format!("Die Quelldatei konnte nicht gelesen werden: {e}");
                return;
            }
            _ => {}
        }
        let Some(exe) = find_bambu_studio(&self.bambu_path) else {
            self.message = "Pfad zu Bambu Studio unter Einstellungen hinterlegen.".into();
            return;
        };

        let out = std::env::temp_dir().join(format!("sunlu_tracker_{}.gcode.3mf", Local::now().format("%Y%m%d_%H%M%S")));
        let attempts: Vec<Vec<String>> = vec![
            vec!["--slice".into(), "0".into(), "--debug".into(), "2".into(), "--export-3mf".into(), out.display().to_string(), input.display().to_string()],
            vec!["--slice".into(), "0".into(), "--debug".into(), "2".into(), format!("--export-3mf={}", out.display()), input.display().to_string()],
            vec![input.display().to_string(), "--slice".into(), "0".into(), "--debug".into(), "2".into(), "--export-3mf".into(), out.display().to_string()],
        ];

        let mut _diagnostics = Vec::new();
        for (idx, args) in attempts.iter().enumerate() {
            let _ = fs::remove_file(&out);
            self.message = format!("Bambu Studio verarbeitet die Datei … Versuch {} von {}", idx + 1, attempts.len());
            match Command::new(&exe).args(args).output() {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    _diagnostics.push(format!("Versuch {}: Status {}\n{}\n{}", idx + 1, o.status, stdout, stderr));
                    if o.status.success() {
                        if let Ok(meta) = fs::metadata(&out) {
                            if meta.len() > 0 {
                                self.import_file(out);
                                return;
                            }
                        }
                    }
                }
                Err(e) => _diagnostics.push(format!("Versuch {} konnte nicht gestartet werden: {e}", idx + 1)),
            }
        }

        self.message = "Keine Verbrauchsdaten gefunden. Bitte die Datei in Bambu Studio öffnen, slicen und anschließend als geslicte .gcode.3mf exportieren.".into();

    }

    fn book_print(&mut self, status: &str, factor: f64, manual_total: Option<f64>) {
        let Some(imported) = self.imported.clone() else { return; };
        let tx = match self.conn.transaction() {
            Ok(tx) => tx,
            Err(e) => { self.message = e.to_string(); return; }
        };
        let now: DateTime<Local> = Local::now();
        let mut errors = vec![];
        for usage in &imported.usages {
            let Some(spool_id) = self.assignment.get(&usage.tool).copied() else {
                errors.push(format!("Für Filament {} wurde keine Rolle gewählt.", usage.tool + 1));
                continue;
            };
            let grams = if let Some(total) = manual_total {
                if imported.total_g > 0.0 { total * usage.grams / imported.total_g } else { 0.0 }
            } else { usage.grams * factor };
            if grams > 0.0 {
                let _ = tx.execute("UPDATE spools SET remaining_g = MAX(0, remaining_g - ?1) WHERE id=?2", params![grams, spool_id]);
            }
            let spool_name: String = tx.query_row("SELECT name FROM spools WHERE id=?1", [spool_id], |r| r.get(0)).unwrap_or_else(|_| "Unbekannt".into());
            let _ = tx.execute(
                "INSERT INTO print_history(timestamp, filename, status, spool_id, spool_name, grams, note) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![now.to_rfc3339(), imported.display_name, status, spool_id, spool_name, grams, imported.parsed_from],
            );
        }
        if !errors.is_empty() {
            self.message = errors.join(" ");
            return;
        }
        if let Err(e) = tx.commit() { self.message = format!("Buchung fehlgeschlagen: {e}"); return; }
        let _ = automatic_backup(&self.db_path);
        self.message = format!("Druck als „{status}“ gespeichert und Bestand aktualisiert.");
        self.reload();
    }

    fn ui_about(&mut self, ctx: &egui::Context) {
        let bg = if self.dark_mode { egui::Color32::from_rgb(8, 14, 19) } else { egui::Color32::from_rgb(245, 248, 250) };
        let text_color = if self.dark_mode { egui::Color32::from_rgb(210, 220, 226) } else { egui::Color32::from_rgb(35, 48, 55) };
        egui::CentralPanel::default().frame(egui::Frame::default().fill(bg).inner_margin(egui::Margin::same(24))).show(ctx, |ui| {
            panel_mode(ui, self.dark_mode, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let logo = load_logo(ctx);
                        ui.image((logo.id(), egui::vec2(120.0, 120.0)));
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("SUNLU Filament Tracker").size(30.0).strong());
                        ui.label(egui::RichText::new("Version 1.10.1").size(16.0).color(egui::Color32::from_rgb(0, 180, 190)));
                        ui.add_space(16.0);
                        let year = Local::now().year();
                        ui.label(egui::RichText::new(format!("© by Ralf Ebert {year}")).size(22.0).strong());
                        ui.add_space(20.0);
                    });
                    ui.label(egui::RichText::new(tr(self.english, "Programm-Erklärung", "Program Description")).size(22.0).strong());
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Freeware").size(20.0).strong().color(egui::Color32::from_rgb(0, 180, 190)));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tr(self.english, "Freeware / Rechte", "Freeware / Rights")).size(18.0).strong());
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr(self.english,
                        "Dieses Programm ist Freeware und darf kostenlos genutzt werden. Es ist ein unabhängiges Werkzeug und steht in keiner Verbindung zum offiziellen SUNLU-Projekt.",
                        "This program is freeware and may be used free of charge. It is an independent tool and is not affiliated with the official SUNLU project.")).size(16.0).color(text_color));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(self.english,
                        "Alle Rechte am Programm, am Design und am Quellcode verbleiben bei Ralf Ebert. Der veröffentlichte Quellcode dient der Transparenz und der Nachvollziehbarkeit.",
                        "All rights to the program, design and source code remain with Ralf Ebert. Published source code is provided for transparency and traceability.")).size(16.0).color(text_color));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(self.english,
                        "Ohne vorherige schriftliche Genehmigung sind insbesondere nicht erlaubt: Verkauf, Umbenennung, Veröffentlichung geänderter Versionen, Weitergabe geänderter Quelltexte, Nutzung unter fremdem Namen oder die kommerzielle Verwertung des Programms oder von Teilen davon.",
                        "Without prior written permission, the following are not permitted: sale, renaming, publication of modified versions, redistribution of modified source code, use under another name, or commercial exploitation of the program or parts of it.")).size(16.0).color(text_color));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(self.english,
                        "Die Nutzung erfolgt auf eigene Gefahr. Für Datenverlust, unvollständige Sicherungen oder Schäden durch die Nutzung wird keine Haftung übernommen.",
                        "Use is at your own risk. No liability is accepted for data loss, incomplete backups or damage resulting from use.")).size(16.0).color(text_color));
                    ui.add_space(20.0);
                    ui.separator();
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(tr(self.english, "So wird der Filamentverbrauch berechnet", "How Filament Consumption Is Calculated")).size(21.0).strong());
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tr(self.english,
                        "Bei einer geslicten .gcode.3mf liest das Programm die vom Slicer gespeicherten Grammwerte aus. Diese Werte berücksichtigen Modell, Stützmaterial und – sofern enthalten – Spül- bzw. Farbwechselmaterial.",
                        "For a sliced .gcode.3mf file, the program reads the gram values stored by the slicer. These values include the model, support material and, when available, purge or color-change material.")).size(16.0).color(text_color));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(self.english,
                        "Bei einer normalen, ungeslicten 3MF wird der Verbrauch aus Modellvolumen und Oberfläche geschätzt. Einbezogen werden Düsendurchmesser, Schichthöhe, Wandanzahl, Infill und der eingestellte Support-Zuschlag. Die Materialdichte wird zur Umrechnung von Volumen in Gramm verwendet.",
                        "For a normal unsliced 3MF file, consumption is estimated from model volume and surface area. Nozzle diameter, layer height, wall count, infill and the selected support allowance are included. Material density is used to convert volume to grams.")).size(16.0).color(text_color));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(self.english,
                        "Eine ungeslicte Berechnung bleibt eine Schätzung. Für den genauesten Wert sollte die Datei zuerst in Bambu Studio geslicet und anschließend als geslicte .gcode.3mf eingelesen werden.",
                        "An unsliced calculation remains an estimate. For the most accurate value, slice the file in Bambu Studio first and then import the sliced .gcode.3mf file.")).size(16.0).color(text_color));
                });
            });
        });
    }


    fn ui_history(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(tr(self.english, "Druckhistorie", "Print History"));
                if ui.button(tr(self.english, "CSV exportieren", "Export CSV")).clicked() { self.export_history_csv(); }
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("hist").striped(true).show(ui, |ui| {
                    ui.strong(tr(self.english, "Datum", "Date")); ui.strong(tr(self.english, "Datei", "File")); ui.strong(tr(self.english, "Status", "Status")); ui.strong(tr(self.english, "Rolle", "Spool")); ui.strong(tr(self.english, "Verbrauch", "Consumption")); ui.strong(tr(self.english, "Hinweis", "Note")); ui.end_row();
                    for h in &self.history {
                        ui.label(&h.timestamp); ui.label(&h.filename); ui.label(&h.status); ui.label(&h.spool_name); ui.label(format!("{:.2} g", h.grams)); ui.label(&h.note); ui.end_row();
                    }
                });
            });
        });
    }

    fn ui_statistics(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(tr(self.english, "Statistik und Kosten", "Statistics and Costs"));
            let total_remaining: f64 = self.spools.iter().map(|s| s.remaining_g).sum();
            let total_used: f64 = self.history.iter().map(|h| h.grams).sum();
            ui.horizontal(|ui| {
                ui.group(|ui| { ui.strong(tr(self.english, "Restbestand", "Remaining Stock")); ui.heading(format!("{:.1} g", total_remaining)); });
                ui.group(|ui| { ui.strong(tr(self.english, "Gebuchter Verbrauch", "Recorded Consumption")); ui.heading(format!("{:.1} g", total_used)); });
            });
            ui.separator();
            ui.strong(tr(self.english, "Restbestand je Rolle", "Remaining Stock per Spool"));
            let max = self.spools.iter().map(|s| s.remaining_g).fold(1.0_f64, f64::max);
            for s in &self.spools {
                ui.horizontal(|ui| {
                    ui.label(format!("{} – {}", s.name, color_display(self.english, &s.color)));
                    ui.add(egui::ProgressBar::new((s.remaining_g / max) as f32).desired_width(420.0).text(format!("{:.1} g", s.remaining_g)));
                });
            }
        });
    }

    fn qr_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.selected_qr_spool else { return; };
        let Some(spool) = self.spools.iter().find(|s| s.id == id).cloned() else { self.selected_qr_spool = None; return; };
        let mut open = true;
        egui::Window::new(tr(self.english, "QR-Code der Rolle", "Spool QR Code")).open(&mut open).show(ctx, |ui| {
            let payload = format!("SUNLU|{}|{}|{}|{:.1}g|AMS{}-{}", spool.name, spool.material, spool.color, spool.remaining_g, spool.ams_unit, spool.ams_slot);
            match qrcode::QrCode::new(payload.as_bytes()) {
                Ok(code) => {
                    let width = code.width();
                    let mut rgba = Vec::with_capacity(width*width*4);
                    for y in 0..width { for x in 0..width { let dark = code[(x,y)] == qrcode::Color::Dark; let v = if dark {0} else {255}; rgba.extend_from_slice(&[v,v,v,255]); } }
                    let tex = ctx.load_texture(format!("qr_{}", id), egui::ColorImage::from_rgba_unmultiplied([width,width], &rgba), egui::TextureOptions::NEAREST);
                    ui.image((tex.id(), egui::vec2(240.0,240.0)));
                    ui.label(payload);
                }
                Err(e) => { ui.label(format!("QR-Code konnte nicht erzeugt werden: {e}")); }
            }
        });
        if !open { self.selected_qr_spool = None; }
    }

    fn ui_settings(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(tr(self.english, "Einstellungen", "Settings"));
            ui.label(tr(self.english, "Version 1.10.1 – portable, ohne Installation", "Version 1.10.1 – portable, no installation required"));
            ui.label(format!("{}: {}", tr(self.english, "Datenbank", "Database"), self.db_path.display()));
            ui.horizontal(|ui| {
                ui.label(tr(self.english, "Bambu Studio EXE:", "Bambu Studio EXE:"));
                ui.text_edit_singleline(&mut self.bambu_path);
                if ui.button(tr(self.english, "Suchen", "Browse")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Programme", &["exe"]).pick_file() {
                        self.bambu_path = path.display().to_string();
                    }
                }
                if ui.button(tr(self.english, "Speichern", "Save")).clicked() {
                    let _ = save_setting(&self.conn, "bambu_path", &self.bambu_path);
                    self.message = tr(self.english, "Einstellungen gespeichert.", "Settings saved.").into();
                }
            });
            ui.horizontal(|ui| {
                if ui.button(tr(self.english, "Datenbank jetzt sichern", "Back Up Database Now")).clicked() {
                    match automatic_backup(&self.db_path) {
                        Ok(path) => self.message = format!("Backup erfolgreich erstellt:
{}", path.display()),
                        Err(e) => self.message = format!("Backup fehlgeschlagen: {e:#}"),
                    }
                }
                if ui.button(tr(self.english, "Datenbank laden / wiederherstellen", "Load / Restore Database")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title(tr(self.english, "Filament-Tracker-Datenbank auswählen", "Select Filament Tracker Database"))
                        .add_filter(tr(self.english, "SQLite-Datenbank", "SQLite Database"), &["db", "sqlite", "sqlite3"])
                        .pick_file()
                    {
                        match automatic_backup(&self.db_path) {
                            Ok(backup_path) => match restore_database_from_file(&mut self.conn, &self.db_path, &path) {
                                Ok((spools, history)) => {
                                    self.reload();
                                    self.message = format!(
                                        "Datenbank erfolgreich geladen.
{} Rollen und {} Verlaufseinträge wurden übernommen.
Sicherheitsbackup: {}",
                                        spools, history, backup_path.display()
                                    );
                                }
                                Err(e) => self.message = format!("Datenbank konnte nicht geladen werden: {e:#}"),
                            },
                            Err(e) => self.message = format!("Import abgebrochen: Sicherheitsbackup konnte nicht erstellt werden: {e:#}"),
                        }
                    } else {
                        self.message = tr(self.english, "Datenbankauswahl abgebrochen.", "Database selection cancelled.").into();
                    }
                }
            });
            let settings_message = self.message.starts_with("Backup")
                || self.message.starts_with("Datenbank")
                || self.message.starts_with("Import")
                || self.message.starts_with(tr(self.english, "Einstellungen", "Settings"));
            if settings_message {
                let failed = self.message.to_lowercase().contains("fehl") || self.message.to_lowercase().contains("abgebrochen");
                let fill = if failed { egui::Color32::from_rgb(92, 34, 34) } else { egui::Color32::from_rgb(15, 77, 70) };
                egui::Frame::default().fill(fill).corner_radius(7.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.label(egui::RichText::new(&self.message).size(15.0).strong().color(egui::Color32::WHITE));
                });
            }
            if ui.button(tr(self.english, "Datenordner öffnen", "Open Data Folder")).clicked() {
                if let Some(parent) = self.db_path.parent() { let _ = open::that(parent); }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(tr(self.english, "Nach Updates suchen", "Check for Updates")).clicked() {
                    self.message = "Für echte automatische Updates wird später eine feste Download-Adresse benötigt. Aktuell wird die SUNLU-Seite geöffnet.".into();
                    let _ = open::that("https://www.sunlu.com");
                }
            });
            ui.label(tr(self.english, "Portable Nutzung: Die EXE kann ohne Installation gestartet werden. Die Datenbank liegt standardmäßig im Benutzer-Datenordner, damit Windows Schreibrechte erlaubt.", "Portable use: The EXE can be started without installation. The database is stored in the user data folder so Windows grants write access."));
        });
    }

    fn export_spools_csv(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_file_name("filamentrollen.csv").save_file() else { return; };
        match export_spools(&path, &self.spools) { Ok(_) => self.message = "Rollen exportiert.".into(), Err(e) => self.message = e.to_string() }
    }

    fn export_history_csv(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_file_name("druckhistorie.csv").save_file() else { return; };
        match export_history(&path, &self.history) { Ok(_) => self.message = "Historie exportiert.".into(), Err(e) => self.message = e.to_string() }
    }
}

impl eframe::App for FilamentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let calc_result = self.calc_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(result) = calc_result {
            self.calc_active = false;
            self.calc_rx = None;
            match result {
                Ok(data) => {
                    self.assignment.clear();
                    for usage in &data.usages {
                        if let Some(spool) = self.spools.iter().find(|s| s.ams_slot as usize == usage.tool + 1) { self.assignment.insert(usage.tool, spool.id); }
                    }
                    self.message = if data.parsed_from.contains("Schätzung") {
                        if self.english { format!("Estimated consumption: {:.2} g. Slice the file first for maximum accuracy.", data.total_g) } else { format!("Verbrauch geschätzt: {:.2} g. Für höchste Genauigkeit die Datei vorher slicen.", data.total_g) }
                    } else { if self.english { format!("Sliced consumption data detected: {:.2} g.", data.total_g) } else { format!("Geslicte Verbrauchsdaten erkannt: {:.2} g.", data.total_g) } };
                    self.imported = Some(data);
                }
                Err(e) => self.message = if self.english { format!("3MF could not be calculated: {e}") } else { format!("3MF konnte nicht berechnet werden: {e}") },
            }
        }
        self.ui_top(ctx);
        self.ui_footer(ctx);
        self.ui_sidebar(ctx);
        match self.page {
            Page::Overview => self.ui_overview(ctx),
            Page::Import => self.ui_import(ctx),
            Page::History => self.ui_history(ctx),
            Page::Statistics => self.ui_statistics(ctx),
            Page::Settings => self.ui_settings(ctx),
            Page::About => self.ui_about(ctx),
        }
    }
}


fn color_from_name(name: &str) -> egui::Color32 {
    match name.to_lowercase().as_str() {
        "schwarz" => egui::Color32::from_rgb(22, 24, 27),
        "weiß" | "weiss" => egui::Color32::from_rgb(235, 238, 240),
        "grau" => egui::Color32::from_rgb(105, 110, 116),
        "rot" => egui::Color32::from_rgb(210, 50, 45),
        "blau" => egui::Color32::from_rgb(30, 105, 215),
        "grün" | "gruen" => egui::Color32::from_rgb(35, 155, 85),
        "gelb" => egui::Color32::from_rgb(240, 195, 35),
        "orange" => egui::Color32::from_rgb(235, 95, 15),
        "braun" => egui::Color32::from_rgb(125, 80, 45),
        "lila" | "violett" => egui::Color32::from_rgb(125, 75, 190),
        "rosa" | "pink" => egui::Color32::from_rgb(230, 105, 160),
        "natur" | "transparent" => egui::Color32::from_rgb(205, 198, 175),
        _ => egui::Color32::from_rgb(0, 175, 185),
    }
}

fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    if dark {
        v.panel_fill = egui::Color32::from_rgb(8, 14, 19);
        v.window_fill = egui::Color32::from_rgb(14, 23, 30);
        v.extreme_bg_color = egui::Color32::from_rgb(5, 10, 14);
        v.faint_bg_color = egui::Color32::from_rgb(15, 25, 32);
        v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(14, 23, 30);
        v.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(218, 227, 232);
        v.widgets.inactive.bg_fill = egui::Color32::from_rgb(20, 30, 38);
        v.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(223, 231, 235);
    } else {
        v.panel_fill = egui::Color32::from_rgb(245, 248, 250);
        v.window_fill = egui::Color32::WHITE;
        v.extreme_bg_color = egui::Color32::from_rgb(226, 234, 238);
        v.faint_bg_color = egui::Color32::from_rgb(236, 242, 245);
        v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(250, 252, 253);
        v.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(35, 50, 58);
        v.widgets.inactive.bg_fill = egui::Color32::from_rgb(226, 234, 238);
        v.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(35, 50, 58);
    }
    v.selection.bg_fill = egui::Color32::from_rgb(0, 145, 155);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(15, 145, 155);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(0, 119, 130);
    ctx.set_visuals(v);
}

fn panel_mode<R>(ui: &mut egui::Ui, dark: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let fill = if dark { egui::Color32::from_rgb(12, 20, 27) } else { egui::Color32::WHITE };
    let stroke = if dark { egui::Color32::from_rgb(37, 52, 62) } else { egui::Color32::from_rgb(190, 205, 214) };
    egui::Frame::default().fill(fill).stroke(egui::Stroke::new(1.0_f32, stroke)).corner_radius(11.0).inner_margin(egui::Margin::same(16)).show(ui, add).inner
}

fn dark_panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let dark = ui.visuals().dark_mode;
    panel_mode(ui, dark, add)
}

fn teal_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.add_sized([width, 40.0], egui::Button::new(egui::RichText::new(text).size(15.0).strong().color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(0, 120, 130)).stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0, 180, 188))).corner_radius(7.0))
}

fn dark_button_mode(ui: &mut egui::Ui, text: &str, width: f32, dark: bool) -> egui::Response {
    let fill = if dark { egui::Color32::from_rgb(15, 24, 31) } else { egui::Color32::from_rgb(228, 236, 240) };
    let fg = if dark { egui::Color32::from_rgb(226, 233, 237) } else { egui::Color32::from_rgb(35, 50, 58) };
    ui.add_sized([width, 40.0], egui::Button::new(egui::RichText::new(text).size(15.0).color(fg)).fill(fill).stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(53, 100, 108))).corner_radius(7.0))
}

fn dark_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    dark_button_mode(ui, text, width, ui.visuals().dark_mode)
}

fn load_logo_small(ctx: &egui::Context) -> egui::TextureHandle {
    let bytes = include_bytes!("../sunlu_logo_small.png");
    let img = image::load_from_memory(bytes).expect("Kleines SUNLU-Logo ungültig").to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    ctx.load_texture("sunlu_logo_small", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
}

fn load_logo(ctx: &egui::Context) -> egui::TextureHandle {
    let bytes = include_bytes!("../sunlu_logo.jpg");
    let img = image::load_from_memory(bytes).expect("SUNLU-Logo ungültig").to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    ctx.load_texture("sunlu_logo", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label); ui.text_edit_singleline(value); ui.end_row();
}
fn number(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.label(label); ui.add(egui::DragValue::new(value).speed(1.0).range(0.0..=100000.0)); ui.end_row();
}
fn default_spool() -> Spool {
    Spool { manufacturer: "SUNLU".into(), material: "PLA+".into(), color: "Schwarz".into(), initial_g: 1000.0, remaining_g: 1000.0, empty_spool_g: 215.0, ams_unit: 1, ..Default::default() }
}

fn open_database() -> Result<(Connection, PathBuf)> {
    let dirs = directories::ProjectDirs::from("de", "Ebert", "SunluFilamentTracker").ok_or_else(|| anyhow!("Kein Datenordner verfügbar"))?;
    fs::create_dir_all(dirs.data_dir())?;
    let path = dirs.data_dir().join("filamentbestand.db");
    Ok((Connection::open(&path)?, path))
}
fn init_database(conn: &Connection) -> Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS spools(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL, manufacturer TEXT NOT NULL, material TEXT NOT NULL, color TEXT NOT NULL,
            initial_g REAL NOT NULL, remaining_g REAL NOT NULL, empty_spool_g REAL NOT NULL,
            price_eur REAL NOT NULL DEFAULT 0, location TEXT NOT NULL DEFAULT '',
            ams_unit INTEGER NOT NULL DEFAULT 0, ams_slot INTEGER NOT NULL DEFAULT 0, notes TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS print_history(
            id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp TEXT NOT NULL, filename TEXT NOT NULL,
            status TEXT NOT NULL, spool_id INTEGER, spool_name TEXT NOT NULL, grams REAL NOT NULL, note TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
    "#)?;
    Ok(())
}
fn load_spools(conn: &Connection) -> Result<Vec<Spool>> {
    let mut st = conn.prepare("SELECT id,name,manufacturer,material,color,initial_g,remaining_g,empty_spool_g,price_eur,location,ams_unit,ams_slot,notes FROM spools ORDER BY manufacturer, material, color")?;
    let rows = st.query_map([], |r| {
        Ok(Spool {
            id: r.get(0)?,
            name: r.get(1)?,
            manufacturer: r.get(2)?,
            material: r.get(3)?,
            color: r.get(4)?,
            initial_g: r.get(5)?,
            remaining_g: r.get(6)?,
            empty_spool_g: r.get(7)?,
            price_eur: r.get(8)?,
            location: r.get(9)?,
            ams_unit: r.get(10)?,
            ams_slot: r.get(11)?,
            notes: r.get(12)?,
        })
    })?;
    let spools = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(spools)
}
fn save_spool(conn: &Connection, s: &Spool) -> Result<()> {
    if s.name.trim().is_empty() { return Err(anyhow!("Ein Rollenname ist erforderlich.")); }
    if s.id == 0 {
        conn.execute("INSERT INTO spools(name,manufacturer,material,color,initial_g,remaining_g,empty_spool_g,price_eur,location,ams_unit,ams_slot,notes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)", params![s.name,s.manufacturer,s.material,s.color,s.initial_g,s.remaining_g,s.empty_spool_g,s.price_eur,s.location,s.ams_unit,s.ams_slot,s.notes])?;
    } else {
        conn.execute("UPDATE spools SET name=?1,manufacturer=?2,material=?3,color=?4,initial_g=?5,remaining_g=?6,empty_spool_g=?7,price_eur=?8,location=?9,ams_unit=?10,ams_slot=?11,notes=?12 WHERE id=?13", params![s.name,s.manufacturer,s.material,s.color,s.initial_g,s.remaining_g,s.empty_spool_g,s.price_eur,s.location,s.ams_unit,s.ams_slot,s.notes,s.id])?;
    }
    Ok(())
}
fn load_history(conn: &Connection) -> Result<Vec<HistoryRow>> {
    let mut st = conn.prepare("SELECT timestamp,filename,status,spool_name,grams,note FROM print_history ORDER BY id DESC LIMIT 1000")?;
    let rows = st.query_map([], |r| {
        Ok(HistoryRow {
            timestamp: r.get(0)?,
            filename: r.get(1)?,
            status: r.get(2)?,
            spool_name: r.get(3)?,
            grams: r.get(4)?,
            note: r.get(5)?,
        })
    })?;
    let history = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(history)
}
fn save_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value])?; Ok(())
}
fn load_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).ok()
}


#[derive(Debug, Clone, Copy)]
struct EstimateSettings {
    nozzle_mm: f64,
    layer_height: f64,
    wall_count: f64,
    infill_percent: f64,
    support_percent: f64,
}

fn estimate_3mf(path: &Path, settings: EstimateSettings) -> Result<ImportedPrint> {
    let file = File::open(path).with_context(|| format!("Datei nicht gefunden: {}", path.display()))?;
    let mut zip = ZipArchive::new(file).context("Die Datei ist kein gültiges 3MF/ZIP-Archiv")?;
    let mut model_documents: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        if f.name().to_lowercase().ends_with(".model") && f.size() < 100_000_000 {
            let mut xml = String::new();
            if f.read_to_string(&mut xml).is_ok() && !xml.trim().is_empty() { model_documents.push(xml); }
        }
    }
    if model_documents.is_empty() { return Err(anyhow!("In der 3MF-Datei wurde kein 3D-Modell gefunden.")); }

    #[derive(Clone, Default)]
    struct Geo { volume: f64, area: f64, components: Vec<(String, f64)> }
    let mut objects: HashMap<String, Geo> = HashMap::new();
    let mut build_items: Vec<(String, f64)> = Vec::new();
    let mut unit_scale = 1.0_f64;

    for xml in &model_documents {
        let doc = roxmltree::Document::parse(xml).context("3MF-Modellstruktur konnte nicht gelesen werden")?;
        if let Some(root) = doc.descendants().find(|n| n.has_tag_name("model")) {
            unit_scale = match root.attribute("unit").unwrap_or("millimeter").to_lowercase().as_str() {
                "micron" => 0.001, "centimeter" => 10.0, "meter" => 1000.0,
                "inch" => 25.4, "foot" => 304.8, _ => 1.0,
            };
        }
        for obj in doc.descendants().filter(|n| n.has_tag_name("object")) {
            let Some(id) = obj.attribute("id") else { continue; };
            let mut geo = Geo::default();
            let mut vertices: Vec<[f64;3]> = Vec::new();
            for v in obj.descendants().filter(|n| n.has_tag_name("vertex")) {
                let x=v.attribute("x").and_then(|x| x.parse().ok()).unwrap_or(0.0)*unit_scale;
                let y=v.attribute("y").and_then(|x| x.parse().ok()).unwrap_or(0.0)*unit_scale;
                let z=v.attribute("z").and_then(|x| x.parse().ok()).unwrap_or(0.0)*unit_scale;
                vertices.push([x,y,z]);
            }
            for t in obj.descendants().filter(|n| n.has_tag_name("triangle")) {
                let (Some(a),Some(b),Some(c))=(t.attribute("v1").and_then(|x|x.parse::<usize>().ok()),t.attribute("v2").and_then(|x|x.parse::<usize>().ok()),t.attribute("v3").and_then(|x|x.parse::<usize>().ok())) else { continue; };
                if a>=vertices.len() || b>=vertices.len() || c>=vertices.len() { continue; }
                let p=vertices[a]; let q=vertices[b]; let r=vertices[c];
                let cross=[(q[1]-p[1])*(r[2]-p[2])-(q[2]-p[2])*(r[1]-p[1]),(q[2]-p[2])*(r[0]-p[0])-(q[0]-p[0])*(r[2]-p[2]),(q[0]-p[0])*(r[1]-p[1])-(q[1]-p[1])*(r[0]-p[0])];
                geo.area += 0.5*(cross[0]*cross[0]+cross[1]*cross[1]+cross[2]*cross[2]).sqrt();
                geo.volume += (p[0]*(q[1]*r[2]-q[2]*r[1])-p[1]*(q[0]*r[2]-q[2]*r[0])+p[2]*(q[0]*r[1]-q[1]*r[0]))/6.0;
            }
            geo.volume = geo.volume.abs();
            for c in obj.descendants().filter(|n| n.has_tag_name("component")) {
                if let Some(oid)=c.attribute("objectid") {
                    let det=c.attribute("transform").map(transform_determinant).unwrap_or(1.0).abs();
                    geo.components.push((oid.to_string(),det));
                }
            }
            objects.insert(id.to_string(), geo);
        }
        for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
            if let Some(oid)=item.attribute("objectid") {
                let det=item.attribute("transform").map(transform_determinant).unwrap_or(1.0).abs();
                build_items.push((oid.to_string(),det));
            }
        }
    }

    fn resolve(id:&str, objects:&HashMap<String,Geo>, stack:&mut Vec<String>) -> (f64,f64) {
        if stack.iter().any(|x| x==id) { return (0.0,0.0); }
        let Some(g)=objects.get(id) else { return (0.0,0.0); };
        stack.push(id.to_string());
        let mut v=g.volume; let mut a=g.area;
        for (child,det) in &g.components {
            let (cv,ca)=resolve(child,objects,stack);
            v += cv*det; a += ca*det.powf(2.0/3.0);
        }
        stack.pop(); (v,a)
    }

    let mut total_volume=0.0; let mut total_area=0.0;
    if !build_items.is_empty() {
        for (id,det) in build_items { let (v,a)=resolve(&id,&objects,&mut Vec::new()); total_volume+=v*det; total_area+=a*det.powf(2.0/3.0); }
    } else {
        for id in objects.keys() { let (v,a)=resolve(id,&objects,&mut Vec::new()); total_volume+=v; total_area+=a; }
    }
    if total_volume <= 0.01 {
        return Err(anyhow!("Das Modell enthält keine geschlossene, berechenbare Geometrie. Bitte eine normale MakerWorld-3MF oder eine geslicte .gcode.3mf auswählen."));
    }
    let wall_thickness=(settings.wall_count*settings.nozzle_mm).max(settings.nozzle_mm);
    let shell_volume=(total_area*wall_thickness*0.72).min(total_volume);
    let infill=(settings.infill_percent/100.0).clamp(0.0,1.0);
    let mut plastic_volume=shell_volume+(total_volume-shell_volume)*infill;
    plastic_volume*=1.0+(settings.support_percent/100.0).clamp(0.0,2.0);
    plastic_volume*=1.04;
    let grams=plastic_volume/1000.0*1.24;
    Ok(ImportedPrint { source_path:path.to_path_buf(), display_name:path.file_name().unwrap_or_default().to_string_lossy().into_owned(), usages:vec![PrintUsage{tool:0,grams,label:"SUNLU Filament (Schätzung)".into()}], total_g:grams, parsed_from:format!("Geometrische Schätzung • {:.2} mm Düse • {:.2} mm Schicht • {:.0} Wände • {:.0}% Infill • {:.0}% Support",settings.nozzle_mm,settings.layer_height,settings.wall_count,settings.infill_percent,settings.support_percent), warnings:vec!["Schätzwert. Eine geslicte .gcode.3mf liefert den genaueren Wert inklusive Spül- und Stützmaterial.".into()] })
}

fn transform_determinant(s: &str) -> f64 {
    let n: Vec<f64> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if n.len()!=12 { return 1.0; }
    let (a,b,c,d,e,f,g,h,i)=(n[0],n[1],n[2],n[3],n[4],n[5],n[6],n[7],n[8]);
    a*(e*i-f*h)-b*(d*i-f*g)+c*(d*h-e*g)
}

fn parse_3mf(path: &Path) -> Result<ImportedPrint> {
    let file = File::open(path).with_context(|| format!("Datei nicht gefunden: {}", path.display()))?;
    let mut zip = ZipArchive::new(file).context("Die Datei ist kein gültiges 3MF/ZIP-Archiv")?;
    let mut texts = vec![];
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().to_lowercase();
        if name.ends_with(".json") || name.ends_with(".config") || name.ends_with(".xml") || name.ends_with(".gcode") || name.ends_with(".txt") {
            if f.size() > 20_000_000 { continue; }
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() { texts.push((name, s)); }
        }
    }
    let mut usages = extract_usage(&texts);
    usages.retain(|u| u.grams.is_finite() && u.grams > 0.001 && u.grams < 100_000.0);
    if usages.is_empty() {
        return Err(anyhow!("Kein geslicter Filamentverbrauch gefunden. Datei bitte in Bambu Studio öffnen, slicen und anschließend als Projekt/geslicte 3MF speichern."));
    }
    let total_g = usages.iter().map(|u| u.grams).sum();
    Ok(ImportedPrint {
        source_path: path.to_path_buf(),
        display_name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        usages,
        total_g,
        parsed_from: "Bambu/Orca-Metadaten oder G-Code-Kommentare".into(),
        warnings: vec!["Die Werte stammen aus dem Slicer. Reale Abweichungen durch Abbruch, Purge-Reste oder manuelle Filamentwechsel sind möglich.".into()],
    })
}

fn extract_usage(texts: &[(String, String)]) -> Vec<PrintUsage> {
    // Bambu/Orca commonly stores arrays such as filament_used_g = "12.3,4.5" or JSON values.
    let patterns = [
        r#"(?i)filament_used_g[\"'\s:=]+\[?\"?([0-9.,;\s]+)"#,
        r#"(?i)filament_weight_total[\"'\s:=]+\[?\"?([0-9.,;\s]+)"#,
        r#"(?i)total_filament_used_g[\"'\s:=]+\[?\"?([0-9.,;\s]+)"#,
        r#"(?i);\s*filament used \[g\]\s*=\s*([0-9.,;\s]+)"#,
    ];
    for (_, text) in texts {
        for pat in patterns {
            let re = Regex::new(pat).unwrap();
            if let Some(c) = re.captures(text) {
                if let Some(m) = c.get(1) {
                    let vals = parse_number_list(m.as_str());
                    if !vals.is_empty() {
                        return vals.into_iter().enumerate().map(|(i,g)| PrintUsage { tool:i, grams:g, label:format!("Filament {}", i+1) }).collect();
                    }
                }
            }
        }
    }
    // G-code fallback: aggregate per extruder if explicit comments exist.
    let re = Regex::new(r"(?i)(?:filament|extruder)\s*(\d+).*?(?:used|verbrauch).*?([0-9]+(?:\.[0-9]+)?)\s*g").unwrap();
    let mut map: HashMap<usize, f64> = HashMap::new();
    for (_, text) in texts {
        for c in re.captures_iter(text) {
            let tool = c[1].parse::<usize>().unwrap_or(1).saturating_sub(1);
            let grams = c[2].parse::<f64>().unwrap_or(0.0);
            map.entry(tool).and_modify(|v| *v = v.max(grams)).or_insert(grams);
        }
    }
    let mut out: Vec<_> = map.into_iter().map(|(tool,grams)| PrintUsage { tool,grams,label:format!("Filament {}",tool+1) }).collect();
    out.sort_by_key(|u| u.tool);
    out
}
fn parse_number_list(s: &str) -> Vec<f64> {
    // Decimal points expected in machine files; commas/semicolons separate values.
    s.trim_matches(|c: char| c == '"' || c == ']' || c.is_whitespace())
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter_map(|x| x.trim().parse::<f64>().ok())
        .collect()
}
fn find_bambu_studio(configured: &str) -> Option<PathBuf> {
    if !configured.trim().is_empty() {
        let p = PathBuf::from(configured.trim()); if p.exists() { return Some(p); }
    }
    [
        r"C:\Program Files\Bambu Studio\bambu-studio.exe",
        r"C:\Program Files\Bambu Studio\BambuStudio.exe",
        r"C:\Users\Public\Bambu Studio\bambu-studio.exe",
    ].iter().map(PathBuf::from).find(|p| p.exists())
}

fn restore_database_from_file(current: &mut Connection, current_path: &Path, source_path: &Path) -> Result<(usize, usize)> {
    let current_canon = current_path.canonicalize().unwrap_or_else(|_| current_path.to_path_buf());
    let source_canon = source_path.canonicalize().unwrap_or_else(|_| source_path.to_path_buf());
    if current_canon == source_canon {
        return Err(anyhow!("Die ausgewählte Datei ist bereits die aktuell verwendete Datenbank."));
    }
    if !source_path.is_file() {
        return Err(anyhow!("Die ausgewählte Datenbankdatei wurde nicht gefunden."));
    }

    let source = Connection::open(source_path).with_context(|| format!("Datenbank konnte nicht geöffnet werden: {}", source_path.display()))?;
    for table in ["spools", "print_history", "settings"] {
        let exists: i64 = source.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(anyhow!("Die Datei ist keine vollständige Filament-Tracker-Datenbank. Tabelle „{}“ fehlt.", table));
        }
    }

    let spools = load_spools(&source).context("Rollen konnten aus der ausgewählten Datenbank nicht gelesen werden")?;

    let mut history_stmt = source.prepare(
        "SELECT id,timestamp,filename,status,spool_id,spool_name,grams,note FROM print_history ORDER BY id"
    )?;
    let history_rows = history_stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?,
            r.get::<_, Option<i64>>(4)?, r.get::<_, String>(5)?, r.get::<_, f64>(6)?, r.get::<_, String>(7)?,
        ))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;

    let mut settings_stmt = source.prepare("SELECT key,value FROM settings")?;
    let settings_rows = settings_stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let tx = current.transaction()?;
    tx.execute("DELETE FROM print_history", [])?;
    tx.execute("DELETE FROM spools", [])?;
    tx.execute("DELETE FROM settings", [])?;

    for s in &spools {
        tx.execute(
            "INSERT INTO spools(id,name,manufacturer,material,color,initial_g,remaining_g,empty_spool_g,price_eur,location,ams_unit,ams_slot,notes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![s.id,s.name,s.manufacturer,s.material,s.color,s.initial_g,s.remaining_g,s.empty_spool_g,s.price_eur,s.location,s.ams_unit,s.ams_slot,s.notes],
        )?;
    }
    for (id, timestamp, filename, status, spool_id, spool_name, grams, note) in &history_rows {
        tx.execute(
            "INSERT INTO print_history(id,timestamp,filename,status,spool_id,spool_name,grams,note) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id,timestamp,filename,status,spool_id,spool_name,grams,note],
        )?;
    }
    for (key, value) in &settings_rows {
        tx.execute("INSERT INTO settings(key,value) VALUES(?1,?2)", params![key,value])?;
    }
    tx.execute("DELETE FROM sqlite_sequence WHERE name IN ('spools','print_history')", [])?;
    tx.execute("INSERT INTO sqlite_sequence(name,seq) SELECT 'spools', COALESCE(MAX(id),0) FROM spools", [])?;
    tx.execute("INSERT INTO sqlite_sequence(name,seq) SELECT 'print_history', COALESCE(MAX(id),0) FROM print_history", [])?;
    tx.commit()?;
    Ok((spools.len(), history_rows.len()))
}

fn automatic_backup(db_path: &Path) -> Result<PathBuf> {
    let dir = db_path.parent().unwrap_or_else(|| Path::new(".")).join("backups");
    fs::create_dir_all(&dir)?;
    let out = dir.join(format!("filamentbestand_{}.db", Local::now().format("%Y-%m-%d_%H-%M-%S")));
    fs::copy(db_path, &out)?;
    let mut entries: Vec<_> = fs::read_dir(&dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    while entries.len() > 10 {
        if let Some(e) = entries.first() { let _ = fs::remove_file(e.path()); }
        entries.remove(0);
    }
    Ok(out)
}
fn export_spools(path: &Path, rows: &[Spool]) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(&[0xEF, 0xBB, 0xBF])?;
    let mut w = csv::WriterBuilder::new().delimiter(b';').from_writer(file);
    w.write_record(["Name","Hersteller","Material","Farbe","Anfang_g","Rest_g","Prozent","Leergewicht_g","Preis_EUR","Lagerort","AMS","Fach","Notizen"])?;
    for s in rows {
        let pct = if s.initial_g > 0.0 { s.remaining_g / s.initial_g * 100.0 } else { 0.0 };
        w.write_record([&s.name,&s.manufacturer,&s.material,&s.color,&format!("{:.2}",s.initial_g),&format!("{:.2}",s.remaining_g),&format!("{:.1}",pct),&format!("{:.2}",s.empty_spool_g),&format!("{:.2}",s.price_eur),&s.location,&s.ams_unit.to_string(),&s.ams_slot.to_string(),&s.notes])?;
    }
    w.flush()?; Ok(())
}
fn export_history(path: &Path, rows: &[HistoryRow]) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(&[0xEF, 0xBB, 0xBF])?;
    let mut w = csv::WriterBuilder::new().delimiter(b';').from_writer(file);
    w.write_record(["Datum","Datei","Status","Rolle","Verbrauch_g","Hinweis"])?;
    for h in rows { w.write_record([&h.timestamp,&h.filename,&h.status,&h.spool_name,&format!("{:.2}",h.grams),&h.note])?; }
    w.flush()?; Ok(())
}


const MATERIALS: &[&str] = &[
    "PLA", "PLA+", "PLA+ 2.0", "PLA Matte",
    "PETG", "PETG HS", "PETG Matte",
    "ABS", "ASA", "TPU",
];

const COLORS: &[&str] = &[
    "Schwarz", "Weiß", "Grau", "Silber", "Rot", "Orange", "Gelb",
    "Grün", "Blau", "Dunkelblau", "Türkis", "Lila", "Rosa",
    "Braun", "Beige", "Transparent", "Mehrfarbig",
];

fn main() -> eframe::Result<()> {
    let icon_img = image::load_from_memory(include_bytes!("../sunlu_logo.jpg")).expect("Logo").to_rgba8();
    let icon = egui::IconData { rgba: icon_img.to_vec(), width: icon_img.width(), height: icon_img.height() };
    let opts = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 760.0]).with_min_inner_size([1000.0, 650.0]).with_icon(icon), ..Default::default() };
    eframe::run_native("SUNLU Filament Tracker", opts, Box::new(|cc| Ok(Box::new(FilamentApp::new(cc)))))
}
