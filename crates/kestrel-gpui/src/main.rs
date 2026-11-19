#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ui;
use kestrel_gpui::{state::AppState, widgets};
use ui::Kestrel;

use std::borrow::Cow;

use gpui::{
    App, AppContext, Application, AssetSource, Bounds, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "kestrel.png" => Some(Cow::Borrowed(include_bytes!(
                "../../kestrel-app/assets/kestrel.png"
            ))),
            _ => None,
        })
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(if path.is_empty() {
            vec!["kestrel.png".into()]
        } else {
            vec![]
        })
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        widgets::text_input::init(cx);
        if let Err(error) = cx
            .text_system()
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../../kestrel-app/assets/Inter-Regular.ttf"
            ))])
        {
            eprintln!("Could not load the bundled Inter font; using system fonts: {error}");
        }
        let bounds = Bounds::centered(None, size(px(1200.0), px(760.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(960.0), px(600.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Kestrel".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                let state = cx.new(AppState::new);
                cx.new(|cx| Kestrel::new(state, cx))
            },
        ) {
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
        cx.activate(true);
    });
}
