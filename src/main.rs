// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod data;
mod esi;
mod icons;
mod theme;

use eframe::egui;
use std::sync::Arc;

fn window_icon() -> egui::IconData {
    match image::load_from_memory(include_bytes!("../assets/icon.png")) {
        Ok(img) => {
            let img = img.to_rgba8();
            egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
        }
        Err(_) => egui::IconData::default(),
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(data::APP_NAME)
            .with_inner_size([1500.0, 900.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(Arc::new(window_icon())),
        ..Default::default()
    };
    eframe::run_native(
        data::APP_NAME,
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::TrackerApp::new(cc.egui_ctx.clone())))
        }),
    )
}
