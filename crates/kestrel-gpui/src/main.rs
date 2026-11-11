#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod theme;
mod widgets;

use std::borrow::Cow;

use gpui::{
    App, Application, AssetSource, Bounds, Context, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, img, prelude::*, px, size,
};
use theme::Theme;

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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Page {
    #[default]
    Dashboard,
    Remux,
    Transcode,
    Queue,
    Logs,
    Settings,
}

impl Page {
    const ALL: [Self; 6] = [
        Self::Dashboard,
        Self::Remux,
        Self::Transcode,
        Self::Queue,
        Self::Logs,
        Self::Settings,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Remux => "Remux",
            Self::Transcode => "Transcode",
            Self::Queue => "Queue",
            Self::Logs => "Logs",
            Self::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Dashboard => "⌂",
            Self::Remux => "⇄",
            Self::Transcode => "↗",
            Self::Queue => "≡",
            Self::Logs => "≣",
            Self::Settings => "☷",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Dashboard => "An overview of your media files and recent work.",
            Self::Remux => "Select audio and subtitle tracks and remux files without re-encoding.",
            Self::Transcode => "Choose output formats, video encoders, and conversion presets.",
            Self::Queue => "Review pending jobs and follow batch progress.",
            Self::Logs => "View processing details, warnings, and errors.",
            Self::Settings => {
                "Manage tool locations, output preferences, and application settings."
            }
        }
    }
}

#[derive(Default)]
struct Kestrel {
    page: Page,
    light: bool,
    collapsed: bool,
}

impl Kestrel {
    fn content(&self, theme: Theme) -> gpui::Div {
        let content = div().flex().flex_col().gap_6().p_6();
        if self.page != Page::Dashboard {
            return content.child(
                widgets::card(theme)
                    .child(widgets::label(self.page.title(), theme).text_lg())
                    .child(widgets::muted(self.page.description(), theme))
                    .child(widgets::muted(
                        "This workspace is not connected to the engine yet.",
                        theme,
                    )),
            );
        }
        content
            .child(
                div().flex().gap_4().children(
                    [
                        ("Files", "0", theme.accent),
                        ("Total size", "0 B", theme.warning),
                        ("Completed", "0", theme.success),
                        ("Failed", "0", theme.danger),
                    ]
                    .into_iter()
                    .map(|(title, value, color)| {
                        widgets::card(theme)
                            .flex_1()
                            .child(widgets::muted(title, theme))
                            .child(widgets::label(value, theme).text_2xl().text_color(color))
                    }),
                ),
            )
            .child(
                widgets::card(theme)
                    .child(widgets::label("Files", theme).text_lg())
                    .child(
                        widgets::table_header(theme)
                            .child(div().flex_1().child("Name"))
                            .child(div().w_24().child("Size"))
                            .child(div().w_24().child("Status"))
                            .child(div().w_24().child("Progress")),
                    )
                    .child(
                        widgets::table_row(theme)
                            .h(px(160.0))
                            .justify_center()
                            .child(widgets::muted("No files added", theme)),
                    ),
            )
    }
}

impl Render for Kestrel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = if self.light {
            Theme::light()
        } else {
            Theme::dark()
        };
        let collapsed = self.collapsed;
        div()
            .flex()
            .size_full()
            .font_family("Inter")
            .text_size(px(13.0))
            .bg(theme.background)
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h_full()
                    .flex_shrink_0()
                    .w(px(if collapsed { 64.0 } else { 220.0 }))
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .h_16()
                            .px_4()
                            .child(img("kestrel.png").size_8().flex_shrink_0())
                            .when(!collapsed, |this| {
                                this.child(widgets::label("Kestrel", theme).text_lg())
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .px_2()
                            .py_6()
                            .children(Page::ALL.into_iter().map(|page| {
                                widgets::button(page.title(), theme)
                                    .when(self.page == page, |this| {
                                        this.bg(theme.selected).text_color(theme.accent)
                                    })
                                    .when(collapsed, |this| this.justify_center())
                                    .child(div().w_5().text_lg().child(page.icon()))
                                    .when(!collapsed, |this| this.child(page.title()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.page = page;
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        widgets::button("collapse-sidebar", theme)
                            .m_2()
                            .justify_center()
                            .child(if collapsed {
                                "›"
                            } else {
                                "‹   Collapse sidebar"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.collapsed = !this.collapsed;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h_16()
                            .flex_shrink_0()
                            .px_6()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(widgets::label(self.page.title(), theme).text_xl())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(widgets::muted(
                                        if self.light {
                                            "Light theme"
                                        } else {
                                            "Dark theme"
                                        },
                                        theme,
                                    ))
                                    .child(widgets::toggle(self.light, theme).on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.light = !this.light;
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    .child(widgets::scrollable_list("page-content").child(self.content(theme))),
            )
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../../kestrel-app/assets/Inter-Regular.ttf"
            ))])
            .expect("Inter font should load");
        let bounds = Bounds::centered(None, size(px(1200.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(960.0), px(600.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Kestrel".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Kestrel::default()),
        )
        .expect("Kestrel window should open");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
