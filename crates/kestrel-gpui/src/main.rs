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
            if let Err(error) = cx.text_system().add_fonts(vec![
                Cow::Borrowed(assets::INTER),
                Cow::Borrowed(assets::JETBRAINS_MONO),
            ]) {
                eprintln!("Could not load the bundled fonts; using system fonts: {error}");
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
