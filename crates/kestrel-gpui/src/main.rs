#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod app;
mod assets;
mod ctl;
mod pages;
mod palette;
mod preset_ui;
mod preview_ui;
mod shell;
mod state;
mod theme;
mod widgets;

use std::borrow::Cow;
use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size,
};

use crate::app::{DebugOptions, Kestrel};

fn main() {
    // File paths given on the command line are added as sources at start.
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let debug = DebugOptions::from_env();
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            let fonts = assets::INTER
                .iter()
                .copied()
                .chain([assets::JETBRAINS_MONO])
                .map(Cow::Borrowed)
                .collect();
            if let Err(error) = cx.text_system().add_fonts(fonts) {
                eprintln!("Could not load the bundled fonts; using system fonts: {error}");
            }
            let names = cx.text_system().all_font_names();
            for family in [widgets::UI, widgets::MONO] {
                if !names.iter().any(|name| name == family) {
                    eprintln!("Bundled font {family} did not register; falling back to system fonts");
                }
            }
            if std::env::var_os("KESTREL_FONT_DEBUG").is_some() {
                let resolved = |weight| {
                    cx.text_system()
                        .resolve_font(&widgets::tabular_font(weight))
                };
                eprintln!(
                    "fonts: Inter registered={} JetBrains Mono registered={} ids regular={:?} medium={:?} semibold={:?}",
                    names.iter().any(|name| name == widgets::UI),
                    names.iter().any(|name| name == widgets::MONO),
                    resolved(gpui::FontWeight::NORMAL),
                    resolved(gpui::FontWeight::MEDIUM),
                    resolved(gpui::FontWeight::SEMIBOLD),
                );
            }
            widgets::input::bind_keys(cx);
            actions::bind_keys(cx);
            let bounds = Bounds::centered(None, size(px(1360.0), px(860.0)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1040.0), px(640.0))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Kestrel".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(gpui::point(px(16.0), px(19.0))),
                    }),
                    ..Default::default()
                },
                move |window, cx| cx.new(|cx| Kestrel::new(window, cx, paths, debug)),
            );
            if let Err(error) = opened {
                eprintln!("Could not open the Kestrel window: {error}");
                cx.quit();
                return;
            }
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            // Preview files belong to this process: remove them however the app quits. The
            // window view is already gone by the time its own quit hook would run.
            cx.on_app_quit(|_| {
                kestrel_core::preview::remove_preview_directory();
                async {}
            })
            .detach();
            cx.activate(true);
        });
}
