#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod mkv;
mod theme;
mod worker;

use eframe::egui;
use std::sync::Arc;

fn app_icon() -> Option<Arc<egui::IconData>> {
    let image = image::load_from_memory(include_bytes!("../../logo.png"))
        .ok()?
        .into_rgba8();
    Some(Arc::new(egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }))
}

fn main() -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_app_id("langmux")
        .with_title("LangMux")
        .with_inner_size([1440.0, 920.0])
        .with_min_inner_size([1060.0, 700.0])
        .with_icon(app_icon().unwrap_or_default());
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "LangMux",
        options,
        Box::new(|cc| Ok(Box::new(app::LangMuxApp::new(cc)))),
    )
}
