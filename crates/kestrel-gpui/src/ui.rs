use gpui::{prelude::*, *};
use kestrel_core::{
    compat,
    formats::Container,
    inputs::{ExistingPolicy, InputItem, OutputTarget},
    mkv::{self, MediaMetadata},
    settings::{self, Preferences},
    worker::{FileStatus, RemuxEngine},
};
use kestrel_gpui::{
    state::{
        AppState,
        chips::TrackKind,
        reducer::{self, StateData},
    },
    theme::Theme,
    widgets::{
        self,
        button::{Button, ButtonVariant},
        dropdown::{Select, SelectionChanged},
        table::{Column, RowSelected, Table},
        text_input::{TextChanged, TextInput},
        toast::{ToastKind, Toasts},
    },
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
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
}

pub struct Kestrel {
    state: Entity<AppState>,
    page: Page,
    collapsed: bool,
    theme: Theme,
    search: Entity<TextInput>,
    output: Entity<TextInput>,
    audio: Entity<TextInput>,
    subtitles: Entity<TextInput>,
    engine: Entity<Select>,
    container: Entity<Select>,
    collision: Entity<Select>,
    table: Entity<Table>,
    toasts: Entity<Toasts>,
    visible_paths: Vec<PathBuf>,
    selected: BTreeSet<PathBuf>,
    last_output: String,
    log_count: usize,
    ticking: bool,
    _subscriptions: Vec<Subscription>,
}

impl Kestrel {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let prefs = &state.read(cx).data().preferences;
        let theme = palette(prefs.light_theme);
        let engine_index = usize::from(prefs.remux_engine == RemuxEngine::Ffmpeg);
        let container_index = container_index(prefs.current_settings.container);
        let collision_index = match prefs.existing {
            ExistingPolicy::Skip => 0,
            ExistingPolicy::Overwrite => 1,
            ExistingPolicy::Rename => 2,
        };
        let search = cx.new(|cx| TextInput::new("Search file names", theme, cx));
        let output = cx.new(|cx| TextInput::new("Output file or folder", theme, cx));
        let audio = cx.new(|cx| TextInput::new("Custom code, e.g. ger", theme, cx));
        let subtitles = cx.new(|cx| TextInput::new("Custom code, e.g. spa", theme, cx));
        let engine = cx.new(|cx| {
            Select::new(vec!["MKVToolNix".into(), "FFmpeg".into()], theme, cx)
                .selected(engine_index)
        });
        let container = cx.new(|cx| {
            Select::new(
                vec!["MKV".into(), "MP4".into(), "WebM".into(), "MOV".into()],
                theme,
                cx,
            )
            .selected(container_index)
        });
        let collision = cx.new(|cx| {
            Select::new(
                vec!["Skip".into(), "Overwrite".into(), "Rename".into()],
                theme,
                cx,
            )
            .selected(collision_index)
        });
        let table = cx.new(|_| {
            Table::new(
                vec![
                    Column::new("Name", 260.),
                    Column::new("Size", 90.),
                    Column::new("Tracks", 275.),
                    Column::new("Status", 100.),
                    Column::new("Progress", 135.),
                ],
                vec![],
                theme,
            )
            .progress_column(4)
        });
        let toasts = cx.new(|_| Toasts::new(theme));
        let subscriptions = vec![
            cx.observe(&state, |s, _, cx| s.sync(cx)),
            cx.subscribe(&search, |s, _, _: &TextChanged, cx| s.refresh_rows(cx)),
            cx.subscribe(&table, |s, _, event: &RowSelected, cx| {
                if let Some(path) = s.visible_paths.get(event.0)
                    && !s.selected.remove(path)
                {
                    s.selected.insert(path.clone());
                }
                s.refresh_rows(cx);
            }),
            cx.subscribe(&engine, |s, _, event: &SelectionChanged, cx| {
                s.preferences(
                    |p| {
                        p.remux_engine = if event.0 == 0 {
                            RemuxEngine::MkvToolNix
                        } else {
                            RemuxEngine::Ffmpeg
                        }
                    },
                    cx,
                )
            }),
            cx.subscribe(&container, |s, _, event: &SelectionChanged, cx| {
                s.preferences(|p| p.current_settings.container = containers()[event.0], cx)
            }),
            cx.subscribe(&collision, |s, _, event: &SelectionChanged, cx| {
                s.preferences(
                    |p| {
                        p.existing = [
                            ExistingPolicy::Skip,
                            ExistingPolicy::Overwrite,
                            ExistingPolicy::Rename,
                        ][event.0]
                    },
                    cx,
                )
            }),
        ];
        let mut this = Self {
            state,
            page: Page::Dashboard,
            collapsed: false,
            theme,
            search,
            output,
            audio,
            subtitles,
            engine,
            container,
            collision,
            table,
            toasts,
            visible_paths: vec![],
            selected: BTreeSet::new(),
            last_output: String::new(),
            log_count: 0,
            ticking: false,
            _subscriptions: subscriptions,
        };
        this.sync(cx);
        this
    }
    fn toast(&self, message: impl Into<SharedString>, kind: ToastKind, cx: &mut Context<Self>) {
        self.toasts.update(cx, |s, cx| {
            s.push(message, kind, Duration::from_secs(5), cx);
        });
    }
    fn result(&self, result: std::io::Result<()>, cx: &mut Context<Self>) {
        if let Err(error) = result {
            self.toast(
                format!("Could not save settings: {error}"),
                ToastKind::Error,
                cx,
            );
        }
    }
    fn preferences(&mut self, change: impl FnOnce(&mut Preferences), cx: &mut Context<Self>) {
        let result = self
            .state
            .update(cx, |s, cx| s.update_preferences(change, cx));
        self.result(result, cx);
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        let data = self.state.read(cx).data();
        let theme = palette(data.preferences.light_theme);
        let text = target_path(&effective_target(data))
            .to_string_lossy()
            .into_owned();
        let logs = data
            .logs
            .iter()
            .skip(self.log_count)
            .filter(|line| {
                let text = line.text.to_lowercase();
                ["failed", "error", "could not", "unavailable"]
                    .iter()
                    .any(|word| text.contains(word))
            })
            .map(|l| l.text.clone())
            .take(3)
            .collect::<Vec<_>>();
        self.log_count = data.logs.len();
        let active = data.batch.phase != reducer::QueuePhase::Idle;
        if theme != self.theme {
            self.theme = theme;
            for input in [&self.search, &self.output, &self.audio, &self.subtitles] {
                input.update(cx, |s, cx| s.set_theme(theme, cx));
            }
            for select in [&self.engine, &self.container, &self.collision] {
                select.update(cx, |s, cx| s.set_theme(theme, cx));
            }
            self.table.update(cx, |s, cx| s.set_theme(theme, cx));
            self.toasts.update(cx, |s, cx| s.set_theme(theme, cx));
        }
        if self.output.read(cx).value() == self.last_output {
            self.last_output = text.clone();
            if self.output.read(cx).value() != text {
                self.output.update(cx, |s, cx| s.set_value(&text, cx));
            }
        }
        for message in logs {
            self.toast(message, ToastKind::Error, cx);
        }
        self.refresh_rows(cx);
        if active && !self.ticking {
            self.ticking = true;
            cx.spawn(async move |entity, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let active = entity
                        .update(cx, |s, cx| {
                            let active =
                                s.state.read(cx).data().batch.phase != reducer::QueuePhase::Idle;
                            s.ticking = active;
                            cx.notify();
                            active
                        })
                        .unwrap_or(false);
                    if !active {
                        break;
                    }
                }
            })
            .detach();
        }
        cx.notify();
    }
    fn refresh_rows(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_lowercase();
        let data = self.state.read(cx).data();
        self.selected
            .retain(|p| data.sources.iter().any(|s| s.source.path == *p));
        self.visible_paths.clear();
        let rows = data
            .sources
            .iter()
            .filter(|s| matches_name(&s.source.path, &query))
            .map(|source| {
                self.visible_paths.push(source.source.path.clone());
                let job = data
                    .queue
                    .iter()
                    .rev()
                    .find(|j| j.spec.input == source.source.path);
                let status = if source.error.is_some() {
                    "Probe failed"
                } else if let Some(job) = job {
                    status_text(job.status)
                } else if source.metadata.is_some() {
                    "Ready"
                } else {
                    "Scanning"
                };
                vec![
                    file_name(&source.source.path).into(),
                    format_size(source.size).into(),
                    source
                        .metadata
                        .as_ref()
                        .map(tracks_summary)
                        .unwrap_or_else(|| "Reading tracks".into())
                        .into(),
                    status.into(),
                    format!("{}%", job.map_or(0, |j| j.percent)).into(),
                ]
            })
            .collect();
        let selected = self
            .visible_paths
            .iter()
            .enumerate()
            .filter_map(|(i, p)| self.selected.contains(p).then_some(i))
            .collect();
        self.table.update(cx, |s, cx| {
            s.set_rows(rows, cx);
            s.set_selected_rows(selected, cx);
        });
        cx.notify();
    }
    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let recursive = self.state.read(cx).data().preferences.include_subfolders;
        let (items, rejected) = input_items(paths, recursive);
        if items.is_empty() {
            self.toast(
                "No supported media files or folders were selected",
                ToastKind::Warning,
                cx,
            );
            return;
        }
        let count = items.len();
        let result = self.state.update(cx, |s, cx| s.add_inputs(items, cx));
        if result.is_ok() {
            self.toast(format!("Added {count} source(s)"), ToastKind::Success, cx);
        }
        self.result(result, cx);
        if rejected > 0 {
            self.toast(
                format!("Ignored {rejected} unsupported or missing path(s)"),
                ToastKind::Warning,
                cx,
            );
        }
    }
    fn pick_inputs(&mut self, folder: bool, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: !folder,
            prompt: Some(if folder { "Add folder" } else { "Add files" }.into()),
        });
        cx.spawn(async move |entity, cx| {
            let result = receiver.await;
            let _ = entity.update(cx, |s, cx| match result {
                Ok(Ok(Some(paths))) => s.add_paths(paths, cx),
                Ok(Ok(None)) => {}
                Ok(Err(error)) => s.toast(
                    format!("Could not open picker: {error}"),
                    ToastKind::Error,
                    cx,
                ),
                Err(error) => s.toast(format!("Picker closed: {error}"), ToastKind::Error, cx),
            });
        })
        .detach();
    }
    fn browse_output(&mut self, cx: &mut Context<Self>) {
        let target = effective_target(self.state.read(cx).data());
        if let OutputTarget::File(path) = target {
            let receiver = cx.prompt_for_new_path(
                path.parent().unwrap_or_else(|| Path::new(".")),
                path.file_name().and_then(|n| n.to_str()),
            );
            cx.spawn(async move |entity, cx| {
                let result = receiver.await;
                let _ = entity.update(cx, |s, cx| match result {
                    Ok(Ok(Some(path))) => s.save_output(Some(OutputTarget::File(path)), cx),
                    Ok(Ok(None)) => {}
                    other => s.toast(
                        format!("Could not choose output: {other:?}"),
                        ToastKind::Error,
                        cx,
                    ),
                });
            })
            .detach();
        } else {
            let receiver = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Output folder".into()),
            });
            cx.spawn(async move |entity, cx| {
                let result = receiver.await;
                let _ = entity.update(cx, |s, cx| match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            s.save_output(Some(OutputTarget::Folder(path)), cx)
                        }
                    }
                    Ok(Ok(None)) => {}
                    other => s.toast(
                        format!("Could not choose output: {other:?}"),
                        ToastKind::Error,
                        cx,
                    ),
                });
            })
            .detach();
        }
    }
    fn typed_target(&self, cx: &App) -> Option<OutputTarget> {
        let value = self.output.read(cx).value().trim();
        if value.is_empty() {
            None
        } else if matches!(
            effective_target(self.state.read(cx).data()),
            OutputTarget::File(_)
        ) {
            Some(OutputTarget::File(value.into()))
        } else {
            Some(OutputTarget::Folder(value.into()))
        }
    }
    fn save_output(&mut self, target: Option<OutputTarget>, cx: &mut Context<Self>) {
        self.last_output = target
            .as_ref()
            .map(|t| target_path(t).to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = self.last_output.clone();
        self.output.update(cx, |s, cx| s.set_value(&text, cx));
        let result = self
            .state
            .update(cx, |s, cx| s.set_output_target(target, cx));
        self.result(result, cx);
    }
    fn enqueue(&mut self, start: bool, cx: &mut Context<Self>) {
        let data = self.state.read(cx).data();
        let paths = data
            .sources
            .iter()
            .map(|s| s.source.path.clone())
            .collect::<Vec<_>>();
        let target = self.typed_target(cx);
        let changed = self.output.read(cx).value() != self.last_output;
        let queued_target = target.clone().or_else(|| {
            let paths = if matches!(data.inputs(), [InputItem::File(_)]) {
                paths.as_slice()
            } else {
                &[]
            };
            Some(settings::effective_output_target(
                None,
                false,
                paths,
                data.inputs(),
                data.preferences.current_settings.container,
            ))
        });
        let result = self
            .state
            .update(cx, |s, cx| s.enqueue_sources(&paths, queued_target, cx));
        match result {
            Ok(ids) => {
                if changed {
                    self.save_output(target, cx);
                }
                if start {
                    if self.state.update(cx, |s, cx| s.start_queue(cx)) {
                        self.toast("Queue started", ToastKind::Success, cx);
                    } else {
                        self.toast(
                            "Files added; the queue cannot start yet",
                            ToastKind::Warning,
                            cx,
                        );
                    }
                } else {
                    self.toast(
                        format!("Added {} job(s) to queue", ids.len()),
                        ToastKind::Success,
                        cx,
                    );
                }
            }
            Err(error) => self.toast(error, ToastKind::Error, cx),
        }
    }
    fn add_language(&mut self, kind: TrackKind, cx: &mut Context<Self>) {
        let input = if kind == TrackKind::Audio {
            self.audio.clone()
        } else {
            self.subtitles.clone()
        };
        let code = input.read(cx).value().to_owned();
        if mkv::parse_language_field(&code).is_empty() {
            self.toast("Enter a language code", ToastKind::Warning, cx);
            return;
        }
        let result = self
            .state
            .update(cx, |s, cx| s.add_language(kind, &code, cx));
        if result.is_ok() {
            input.update(cx, |s, cx| s.reset(cx));
            self.toast("Language selection updated", ToastKind::Success, cx);
        }
        self.result(result, cx);
    }
    fn table_panel(&self, height: Option<f32>, cx: &Context<Self>) -> Div {
        let data = self.state.read(cx).data();
        let t = self.theme;
        widgets::card(t)
            .p_3()
            .gap_2()
            .min_h_0()
            .when_some(height, |d, h| d.h(px(h)).flex_shrink_0())
            .when(height.is_none(), |d| d.flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(widgets::label("Source files", t).text_lg())
                    .child(div().w(px(260.)).child(self.search.clone())),
            )
            .child(div().flex_1().min_h_0().child(self.table.clone()))
            .when(self.visible_paths.is_empty(), |d| {
                d.child(widgets::muted(
                    if data.scanning {
                        "Scanning sources…"
                    } else if data.sources.is_empty() {
                        "Drop media files or folders anywhere, or add them on the Remux page."
                    } else {
                        "No file names match your search."
                    },
                    t,
                ))
            })
    }
    fn dashboard(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let data = self.state.read(cx).data();
        let controls = data.controls();
        let stats = data.queue_stats();
        let active = data
            .queue
            .iter()
            .find(|j| j.status == FileStatus::Processing);
        let speed = active
            .and_then(|j| j.speed)
            .map(|s| format!("{s:.1}×"))
            .unwrap_or_else(|| "—".into());
        let eta = active
            .and_then(|j| j.eta_seconds)
            .map(|s| format_duration(s as u64))
            .unwrap_or_else(|| "—".into());
        let progress = overall_progress(data);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_5()
            .p_6()
            .child(
                div().flex().gap_4().children(
                    [
                        ("Files", data.sources.len().to_string()),
                        ("Total size", format_size(data.source_stats.total_size)),
                        (
                            "Elapsed",
                            format_duration(data.elapsed(SystemTime::now()).as_secs()),
                        ),
                        ("Speed / ETA", format!("{speed} / {eta}")),
                    ]
                    .into_iter()
                    .map(|(label, value)| {
                        widgets::card(t)
                            .p_4()
                            .flex_1()
                            .min_w_0()
                            .child(widgets::muted(label, t))
                            .child(widgets::label(value, t).text_2xl())
                    }),
                ),
            )
            .child(
                widgets::card(t)
                    .p_5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(widgets::label("Current batch", t).text_lg())
                            .child(widgets::muted(
                                format!(
                                    "{} jobs · {} completed · {} failed",
                                    stats.total, stats.succeeded, stats.failed
                                ),
                                t,
                            )),
                    )
                    .child(widgets::progress::progress(progress, t))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(widgets::muted(
                                format!("{:?} · {:.0}%", data.batch.phase, progress * 100.),
                                t,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("batch-start", "Start", t)
                                            .variant(ButtonVariant::Primary)
                                            .disabled(!controls.start)
                                            .on_click(cx.listener(|s, _, _, cx| {
                                                s.state.update(cx, |s, cx| s.start_queue(cx));
                                            })),
                                    )
                                    .child(
                                        Button::new(
                                            "batch-pause",
                                            if controls.resume { "Resume" } else { "Pause" },
                                            t,
                                        )
                                        .disabled(!controls.pause && !controls.resume)
                                        .on_click(
                                            cx.listener(|s, _, _, cx| {
                                                s.state.update(cx, |s, cx| {
                                                    if s.data().controls().resume {
                                                        s.resume_queue(cx)
                                                    } else {
                                                        s.pause_queue(cx)
                                                    }
                                                });
                                            }),
                                        ),
                                    )
                                    .child(
                                        Button::new("batch-stop", "Stop", t)
                                            .variant(ButtonVariant::Danger)
                                            .disabled(!controls.stop)
                                            .on_click(cx.listener(|s, _, _, cx| {
                                                s.state.update(cx, |s, cx| s.stop_queue(cx));
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(self.table_panel(None, cx))
    }
    fn language_section(&self, kind: TrackKind, cx: &Context<Self>) -> Div {
        let t = self.theme;
        let state = self.state.read(cx);
        let include = state.data().preferences.current_settings.subtitles.include;
        let disabled = kind == TrackKind::Subtitles && !include;
        let input = if kind == TrackKind::Audio {
            self.audio.clone()
        } else {
            self.subtitles.clone()
        };
        let id = if kind == TrackKind::Audio {
            "audio"
        } else {
            "subtitles"
        };
        widgets::card(t)
            .p_3()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        widgets::label(
                            if kind == TrackKind::Audio {
                                "Audio languages"
                            } else {
                                "Subtitle languages"
                            },
                            t,
                        )
                        .text_base(),
                    )
                    .when(kind == TrackKind::Subtitles, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(widgets::muted("Include subtitles", t))
                                .child(
                                    widgets::toggle::toggle("include-subtitles", include, t)
                                        .on_click(cx.listener(|s, _, _, cx| {
                                            s.preferences(
                                                |p| {
                                                    p.current_settings.subtitles.include =
                                                        !p.current_settings.subtitles.include
                                                },
                                                cx,
                                            )
                                        })),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .when(disabled, |d| d.opacity(0.4))
                    .children(state.language_chips(kind).into_iter().map(|chip| {
                        let code = chip.code.clone();
                        widgets::chip::chip(
                            SharedString::from(format!("{id}-{code}")),
                            chip.code,
                            chip.selected,
                            Some(chip.count),
                            t,
                        )
                        .when(!disabled, |d| {
                            d.on_click(cx.listener(move |s, _, _, cx| {
                                let result = s
                                    .state
                                    .update(cx, |s, cx| s.toggle_language(kind, &code, cx));
                                s.result(result, cx);
                            }))
                        })
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(disabled, |d| d.opacity(0.4))
                    .child(div().flex_1().min_w_0().child(input))
                    .child(
                        Button::new(SharedString::from(format!("add-language-{id}")), "Add", t)
                            .disabled(disabled)
                            .on_click(cx.listener(move |s, _, _, cx| s.add_language(kind, cx))),
                    ),
            )
    }
    fn remux(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let data = self.state.read(cx).data();
        let prefs = &data.preferences;
        let tool = if prefs.remux_engine == RemuxEngine::MkvToolNix {
            &data.tools.mkvmerge
        } else {
            &data.tools.ffmpeg
        };
        let availability = tool
            .as_ref()
            .map(|tool| {
                tool.version
                    .clone()
                    .unwrap_or_else(|| tool.path.display().to_string())
            })
            .unwrap_or_else(|| "Not found".into());
        let warnings = compatibility_warnings(data);
        let output_file = matches!(effective_target(data), OutputTarget::File(_));
        let ready = !data.scanning
            && !data.sources.is_empty()
            && data.sources.iter().all(|s| s.metadata.is_some());
        let can_start = ready && data.controls().edit && data.tools_ready;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("remux-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("add-files", "Add files", t)
                                    .variant(ButtonVariant::Primary)
                                    .on_click(cx.listener(|s, _, _, cx| s.pick_inputs(false, cx))),
                            )
                            .child(
                                Button::new("add-folder", "Add folder", t)
                                    .on_click(cx.listener(|s, _, _, cx| s.pick_inputs(true, cx))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .ml_3()
                                    .child(
                                        widgets::toggle::toggle(
                                            "recursive",
                                            prefs.include_subfolders,
                                            t,
                                        )
                                        .on_click(
                                            cx.listener(|s, _, _, cx| {
                                                s.preferences(
                                                    |p| {
                                                        p.include_subfolders =
                                                            !p.include_subfolders;
                                                        for item in &mut p.input_items {
                                                            if let InputItem::Folder {
                                                                recursive,
                                                                ..
                                                            } = item
                                                            {
                                                                *recursive = p.include_subfolders;
                                                            }
                                                        }
                                                    },
                                                    cx,
                                                )
                                            }),
                                        ),
                                    )
                                    .child(widgets::muted("Include subfolders", t)),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("remove-selected", "Remove selected", t)
                                    .disabled(self.selected.is_empty())
                                    .on_click(cx.listener(|s, _, _, cx| {
                                        let paths = s.selected.iter().cloned().collect::<Vec<_>>();
                                        let result = s
                                            .state
                                            .update(cx, |s, cx| s.remove_sources(&paths, cx));
                                        s.result(result, cx);
                                    })),
                            )
                            .child(
                                Button::new("clear-sources", "Clear", t)
                                    .disabled(data.inputs().is_empty())
                                    .on_click(cx.listener(|s, _, _, cx| {
                                        let result =
                                            s.state.update(cx, |s, cx| s.clear_sources(cx));
                                        s.result(result, cx);
                                    })),
                            ),
                    )
                    .child(self.table_panel(Some(210.), cx))
                    .child(
                        widgets::card(t)
                            .p_3()
                            .flex_shrink_0()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .items_end()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(widgets::muted(
                                                if output_file {
                                                    "Output file"
                                                } else {
                                                    "Output folder · source structure is preserved"
                                                },
                                                t,
                                            ))
                                            .child(self.output.clone()),
                                    )
                                    .child(
                                        Button::new("browse-output", "Browse", t).on_click(
                                            cx.listener(|s, _, _, cx| s.browse_output(cx)),
                                        ),
                                    )
                                    .child(Button::new("apply-output", "Apply", t).on_click(
                                        cx.listener(|s, _, _, cx| {
                                            s.save_output(s.typed_target(cx), cx)
                                        }),
                                    ))
                                    .child(
                                        div()
                                            .w(px(125.))
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(widgets::muted("If output exists", t))
                                            .child(self.collision.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(165.))
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(widgets::muted("Engine", t))
                                            .child(self.engine.clone()),
                                    )
                                    .child(
                                        div()
                                            .w(px(125.))
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(widgets::muted("Container", t))
                                            .child(self.container.clone()),
                                    )
                                    .child(
                                        div().flex_1().min_w_0().pt_5().child(
                                            widgets::muted(availability, t)
                                                .text_size(px(11.))
                                                .overflow_hidden(),
                                        ),
                                    ),
                            )
                            .children(warnings.into_iter().map(|warning| {
                                widgets::label(warning, t).text_xs().text_color(t.warning)
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .gap_3()
                            .child(self.language_section(TrackKind::Audio, cx))
                            .child(self.language_section(TrackKind::Subtitles, cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_6()
                    .py_3()
                    .flex_shrink_0()
                    .bg(t.card)
                    .border_t_1()
                    .border_color(t.border)
                    .child(
                        widgets::toggle::toggle("dry-run", prefs.dry_run, t).on_click(
                            cx.listener(|s, _, _, cx| {
                                s.preferences(|p| p.dry_run = !p.dry_run, cx)
                            }),
                        ),
                    )
                    .child(widgets::label("Dry run", t))
                    .child(widgets::muted("Preview commands without writing files", t))
                    .child(div().flex_1())
                    .child(
                        Button::new("enqueue", "Add to queue", t)
                            .disabled(!ready)
                            .on_click(cx.listener(|s, _, _, cx| s.enqueue(false, cx))),
                    )
                    .child(
                        Button::new("enqueue-start", "Start", t)
                            .variant(ButtonVariant::Primary)
                            .disabled(!can_start)
                            .on_click(cx.listener(|s, _, _, cx| s.enqueue(true, cx))),
                    ),
            )
    }
}

impl Render for Kestrel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let light = self.state.read(cx).data().preferences.light_theme;
        let collapsed = self.collapsed;
        let page = match self.page {
            Page::Dashboard => self.dashboard(cx),
            Page::Remux => self.remux(cx),
            _ => div().p_6().child(
                widgets::card(t)
                    .child(widgets::label(self.page.title(), t).text_lg())
                    .child(widgets::muted(
                        "This workspace will be connected in a later step.",
                        t,
                    )),
            ),
        };
        div()
            .id("root")
            .relative()
            .flex()
            .size_full()
            .font_family("Inter")
            .text_size(px(13.))
            .bg(t.background)
            .text_color(t.text)
            .on_drop(cx.listener(|s, paths: &ExternalPaths, _, cx| {
                s.add_paths(paths.paths().to_vec(), cx)
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(if collapsed { 64. } else { 190. }))
                    .h_full()
                    .flex_shrink_0()
                    .bg(t.sidebar)
                    .border_r_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .h_16()
                            .px_5()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                img(ImageSource::Resource(Resource::Embedded(
                                    "kestrel.png".into(),
                                )))
                                .size_8(),
                            )
                            .when(!collapsed, |d| {
                                d.child(widgets::label("Kestrel", t).text_xl())
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px_3()
                            .py_4()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(Page::ALL.into_iter().map(|page| {
                                widgets::button(page.title(), t)
                                    .when(page == self.page, |d| {
                                        d.bg(t.selected).text_color(t.accent)
                                    })
                                    .child(if collapsed {
                                        page.title()[..1].to_owned()
                                    } else {
                                        page.title().to_owned()
                                    })
                                    .on_click(cx.listener(move |s, _, _, cx| {
                                        s.page = page;
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .p_3()
                            .border_t_1()
                            .border_color(t.border)
                            .when(!collapsed, |d| {
                                d.child(widgets::muted(
                                    if light { "Light theme" } else { "Dark theme" },
                                    t,
                                ))
                            })
                            .child(widgets::toggle::toggle("theme", light, t).on_click(
                                cx.listener(|s, _, _, cx| {
                                    s.preferences(|p| p.light_theme = !p.light_theme, cx)
                                }),
                            )),
                    )
                    .child(
                        widgets::button("collapse-sidebar", t)
                            .m_2()
                            .justify_center()
                            .child(if collapsed {
                                "›"
                            } else {
                                "‹  Collapse sidebar"
                            })
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.collapsed = !s.collapsed;
                                cx.notify();
                            })),
                    )
                    .when(!collapsed, |d| {
                        d.child(widgets::muted("Kestrel 4.0", t).px_4().pb_4())
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h_16()
                            .flex_shrink_0()
                            .px_6()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(t.border)
                            .child(widgets::label(self.page.title(), t).text_2xl())
                            .child(widgets::muted("Drop files or folders anywhere", t)),
                    )
                    .child(page),
            )
            .child(
                div()
                    .absolute()
                    .bottom_5()
                    .right_5()
                    .max_w(px(480.))
                    .child(self.toasts.clone()),
            )
    }
}

fn palette(light: bool) -> Theme {
    if light { Theme::light() } else { Theme::dark() }
}
fn containers() -> [Container; 4] {
    [
        Container::Mkv,
        Container::Mp4,
        Container::WebM,
        Container::Mov,
    ]
}
fn container_index(container: Container) -> usize {
    containers()
        .iter()
        .position(|c| *c == container)
        .unwrap_or(0)
}
fn target_path(target: &OutputTarget) -> &Path {
    match target {
        OutputTarget::File(p) | OutputTarget::Folder(p) => p,
    }
}
fn effective_target(data: &StateData) -> OutputTarget {
    let paths = if matches!(data.inputs(), [InputItem::File(_)]) {
        data.sources
            .iter()
            .map(|s| s.source.path.clone())
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    settings::effective_output_target(
        data.preferences.output_target.as_ref(),
        data.preferences.output_target_explicit,
        &paths,
        data.inputs(),
        data.preferences.current_settings.container,
    )
}
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
fn matches_name(path: &Path, query: &str) -> bool {
    file_name(path).to_lowercase().contains(query)
}
fn format_size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}
fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
fn tracks_summary(metadata: &MediaMetadata) -> String {
    format!(
        "{} video, {} audio, {} subtitles",
        usize::from(metadata.video.is_some() || metadata.video_codec.is_some()),
        metadata.audio_tracks.len(),
        metadata.subtitle_tracks.len()
    )
}
fn status_text(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Pending => "Pending",
        FileStatus::Processing => "Processing",
        FileStatus::Success => "Completed",
        FileStatus::Skipped => "Skipped",
        FileStatus::Error => "Failed",
        FileStatus::Cancelled => "Cancelled",
    }
}
fn overall_progress(data: &StateData) -> f32 {
    let jobs = data
        .queue
        .iter()
        .filter(|j| data.batch.job_ids.contains(&j.spec.id))
        .collect::<Vec<_>>();
    if jobs.is_empty() {
        0.
    } else {
        jobs.iter()
            .map(|j| {
                if reducer::finished(j.status) {
                    100.
                } else {
                    j.percent.min(100) as f32
                }
            })
            .sum::<f32>()
            / (jobs.len() as f32 * 100.)
    }
}
fn compatibility_warnings(data: &StateData) -> Vec<String> {
    data.sources
        .iter()
        .filter_map(|s| s.metadata.as_ref().map(|m| (s, m)))
        .flat_map(|(s, m)| {
            compat::job_warnings(m, &data.preferences.current_settings)
                .into_iter()
                .map(move |warning| format!("{}: {warning}", file_name(&s.source.path)))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn input_items(paths: Vec<PathBuf>, recursive: bool) -> (Vec<InputItem>, usize) {
    let mut rejected = 0;
    let items = paths
        .into_iter()
        .filter_map(|p| {
            if p.is_dir() {
                Some(InputItem::Folder { path: p, recursive })
            } else if p.is_file() && kestrel_core::formats::is_supported_input(&p) {
                Some(InputItem::File(p))
            } else {
                rejected += 1;
                None
            }
        })
        .collect();
    (items, rejected)
}

#[cfg(test)]
mod tests {
    use super::{
        effective_target, format_duration, format_size, input_items, matches_name,
        overall_progress, tracks_summary,
    };
    use kestrel_core::{
        inputs::{InputItem, OutputTarget},
        mkv::MediaMetadata,
        settings::Preferences,
    };
    use kestrel_gpui::state::reducer::StateData;
    use std::path::Path;
    #[test]
    fn output_rules_distinguish_single_files_from_folders_with_one_file() {
        let mut data = StateData::new(Preferences::default());
        data.preferences.input_items = vec![InputItem::File("movies/film.mkv".into())];
        assert!(matches!(effective_target(&data), OutputTarget::File(_)));
        data.preferences.input_items = vec![InputItem::Folder {
            path: "movies".into(),
            recursive: true,
        }];
        data.sources
            .push(kestrel_gpui::state::reducer::SourceEntry {
                source: kestrel_core::inputs::SourceFile {
                    path: "movies/film.mkv".into(),
                    relative: "film.mkv".into(),
                },
                size: 0,
                metadata: None,
                error: None,
            });
        assert!(matches!(effective_target(&data), OutputTarget::Folder(_)));
        data.preferences.input_items = vec![
            InputItem::File("a.mkv".into()),
            InputItem::File("b.mkv".into()),
        ];
        assert!(matches!(effective_target(&data), OutputTarget::Folder(_)));
    }
    #[test]
    fn file_search_and_summaries_cover_empty_and_unicode() {
        assert!(matches_name(Path::new("/media/Épisode.MKV"), "épisode"));
        assert!(matches_name(Path::new("movie.mkv"), ""));
        assert!(!matches_name(Path::new("movie.mkv"), "absent"));
        assert_eq!(
            tracks_summary(&MediaMetadata::default()),
            "0 video, 0 audio, 0 subtitles"
        );
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_duration(3661), "01:01:01");
        assert_eq!(
            overall_progress(&StateData::new(Preferences::default())),
            0.
        );
    }
    #[test]
    fn input_selection_accepts_folders_and_media_and_rejects_missing_or_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("clip.mkv");
        let text = dir.path().join("notes.txt");
        std::fs::write(&media, []).unwrap();
        std::fs::write(&text, []).unwrap();
        let (items, rejected) = input_items(
            vec![
                dir.path().into(),
                media.clone(),
                text,
                dir.path().join("missing.mkv"),
            ],
            false,
        );
        assert_eq!(rejected, 2);
        assert_eq!(
            items,
            vec![
                InputItem::Folder {
                    path: dir.path().into(),
                    recursive: false
                },
                InputItem::File(media)
            ]
        );
        assert_eq!(input_items(vec![], true), (vec![], 0));
    }
}
