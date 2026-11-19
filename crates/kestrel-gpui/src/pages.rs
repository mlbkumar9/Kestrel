use super::*;

impl Kestrel {
    pub(super) fn refresh_queue(&mut self, cx: &mut Context<Self>) {
        let data = self.state.read(cx).data();
        if !data
            .queue
            .iter()
            .any(|j| Some(j.spec.id) == self.selected_job)
        {
            self.selected_job = None;
        }
        let selected = data
            .queue
            .iter()
            .position(|j| Some(j.spec.id) == self.selected_job)
            .into_iter()
            .collect();
        let rows = data
            .queue
            .iter()
            .map(|job| {
                vec![
                    file_name(&job.spec.input).into(),
                    status_text(job.status).into(),
                    format!("{}%", job.percent).into(),
                    job.speed
                        .map(|s| format!("{s:.1}×"))
                        .unwrap_or_else(|| "—".into())
                        .into(),
                    job.eta_seconds
                        .map(|s| format_duration(s.max(0.) as u64))
                        .unwrap_or_else(|| "—".into())
                        .into(),
                ]
            })
            .collect();
        self.queue_table.update(cx, |s, cx| {
            s.set_rows(rows, cx);
            s.set_selected_rows(selected, cx);
        });
        cx.notify();
    }
    pub(super) fn queue_page(&self, cx: &Context<Self>) -> Div {
        let t = self.theme;
        let data = self.state.read(cx).data();
        let controls = data.controls();
        let selected = data
            .queue
            .iter()
            .find(|j| Some(j.spec.id) == self.selected_job);
        let retry = selected
            .is_some_and(|j| matches!(j.status, FileStatus::Error | FileStatus::Cancelled))
            && controls.edit;
        let id = self.selected_job;
        let mut page=div().flex_1().min_h_0().flex().flex_col().gap_4().p_6()
            .child(div().flex().items_center().gap_2()
                .child(Button::new("queue-start","Start",t).variant(ButtonVariant::Primary).disabled(!controls.start).on_click(cx.listener(|s,_,_,cx|{s.state.update(cx,|s,cx|s.start_queue(cx));})))
                .child(Button::new("queue-pause","Pause",t).disabled(!controls.pause).on_click(cx.listener(|s,_,_,cx|{s.state.update(cx,|s,cx|s.pause_queue(cx));})))
                .child(Button::new("queue-resume","Resume",t).disabled(!controls.resume).on_click(cx.listener(|s,_,_,cx|{s.state.update(cx,|s,cx|s.resume_queue(cx));})))
                .child(Button::new("queue-stop","Stop",t).variant(ButtonVariant::Danger).disabled(!controls.stop).on_click(cx.listener(|s,_,_,cx|{s.state.update(cx,|s,cx|s.stop_queue(cx));})))
                .child(div().flex_1())
                .child(Button::new("queue-retry","Retry",t).disabled(!retry).on_click(cx.listener(move |s,_,_,cx|{if let Some(id)=id{s.state.update(cx,|s,cx|s.retry_job(id,cx));}})))
                .child(Button::new("queue-remove","Remove",t).disabled(id.is_none()||!controls.edit).on_click(cx.listener(move |s,_,_,cx|{if let Some(id)=id{s.state.update(cx,|s,cx|s.remove_job(id,cx));}})))
                .child(Button::new("queue-clear-finished","Clear finished",t).disabled(!controls.edit||!data.queue.iter().any(|j|reducer::finished(j.status))).on_click(cx.listener(|s,_,_,cx|{s.state.update(cx,|s,cx|s.clear_finished(cx));}))))
            .child(widgets::muted("Pause takes effect between jobs. Stop ends the batch; Stop job affects only the selected job.",t))
            .child(widgets::card(t).p_3().flex_1().min_h(px(160.)).child(div().flex_1().min_h_0().child(self.queue_table.clone()))
                .when(data.queue.is_empty(),|d|d.child(widgets::muted("No queued jobs. Add source files from Remux.",t))));
        if let Some(job) = selected {
            let spec = &job.spec;
            let execution = spec.execution.as_ref();
            let engine = execution.map_or(data.preferences.remux_engine, |s| s.remux_engine);
            let collision = execution.map_or(data.preferences.existing, |s| s.existing);
            let dry = execution.is_some_and(|s| s.dry_run);
            let stoppable = matches!(job.status, FileStatus::Processing | FileStatus::Pending);
            let active = job.status == FileStatus::Processing;
            let id = spec.id;
            page=page.child(widgets::card(t).p_4().gap_3().flex_shrink_0()
                .child(div().flex().items_center().justify_between().child(widgets::label(format!("Job #{id} · {}",file_name(&spec.input)),t).text_lg())
                    .child(div().flex().gap_2()
                        .child(Button::new("job-pause","Pause",t).disabled(!active||!controls.pause).on_click(cx.listener(|s,_,_,cx|{s.state.update(cx,|s,cx|s.pause_queue(cx));})))
                        .child(Button::new("job-resume","Resume",t).disabled(!active||!controls.resume).on_click(cx.listener(|s,_,_,cx|{s.state.update(cx,|s,cx|s.resume_queue(cx));})))
                        .child(Button::new("job-stop","Stop job",t).disabled(!stoppable).on_click(cx.listener(move |s,_,_,cx|{s.state.update(cx,|s,cx|s.stop_job(id,cx));})))
                        .child(Button::new("job-retry","Retry",t).disabled(!retry).on_click(cx.listener(move |s,_,_,cx|{s.state.update(cx,|s,cx|s.retry_job(id,cx));})))
                        .child(Button::new("job-remove","Remove",t).disabled(!controls.edit).on_click(cx.listener(move |s,_,_,cx|{s.state.update(cx,|s,cx|s.remove_job(id,cx));})))))
                .child(div().flex().gap_6().child(widgets::label(format!("Engine: {}    Container: {}",engine.label(),spec.settings.container.label()),t))
                    .child(widgets::label(format!("Collision: {collision:?}    Dry run: {}",if dry{"Yes"}else{"No"}),t)))
                .child(widgets::label(format!("Audio languages: {}    Subtitle languages: {}    Include subtitles: {}",languages(&spec.settings.audio.languages),languages(&spec.settings.subtitles.languages),if spec.settings.subtitles.include{"Yes"}else{"No"}),t))
                .child(widgets::label(format!("Output: {}",spec.output.display()),t).text_xs())
                .when_some(job.detail.clone(),|d,detail|d.child(widgets::muted(detail,t))));
        } else {
            page = page.child(widgets::muted(
                "Select a job to inspect its saved settings and job actions.",
                t,
            ));
        }
        page
    }
    pub(super) fn logs_page(&self, cx: &Context<Self>) -> Div {
        let t = self.theme;
        let data = self.state.read(cx).data();
        let filter = if self.logs_selected_only {
            self.selected_job
        } else {
            None
        };
        let lines = log_lines(data, filter);
        let copied = lines.join("\n");
        let empty = lines.is_empty();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(widgets::label(format!("{} log lines", lines.len()), t))
                    .child(
                        Button::new(
                            "log-filter",
                            if self.logs_selected_only {
                                "Show all jobs"
                            } else {
                                "Selected job only"
                            },
                            t,
                        )
                        .disabled(self.selected_job.is_none())
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.logs_selected_only = !s.logs_selected_only;
                            cx.notify();
                        })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("logs-copy", "Copy", t)
                            .disabled(empty)
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                            }),
                    )
                    .child(
                        Button::new("logs-clear", "Clear", t)
                            .disabled(data.logs.is_empty())
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.state.update(cx, |s, cx| s.clear_logs(cx));
                            })),
                    ),
            )
            .child(
                div()
                    .id("log-lines")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .bg(t.card)
                    .border_1()
                    .border_color(t.border)
                    .rounded_lg()
                    .when(empty, |d| {
                        d.child(widgets::muted("No log lines to display.", t))
                    })
                    .children(lines.into_iter().map(|line| {
                        div()
                            .py_2()
                            .border_b_1()
                            .border_color(t.border)
                            .font_family("monospace")
                            .text_xs()
                            .child(line)
                    })),
            )
    }
    pub(super) fn settings_page(&self, cx: &Context<Self>) -> Div {
        let t = self.theme;
        let state = self.state.read(cx);
        let tools = &state.data().tools;
        div().p_6().flex().flex_col().gap_4().child(
            widgets::card(t)
                .child(widgets::label("Tool locations", t).text_lg())
                .children(
                    [
                        ("FFmpeg", &tools.ffmpeg),
                        ("MKVToolNix", &tools.mkvmerge),
                        ("FFprobe", &tools.ffprobe),
                    ]
                    .into_iter()
                    .map(|(name, tool)| {
                        widgets::label(
                            format!(
                                "{name}: {}",
                                tool.as_ref()
                                    .map(|t| t.path.display().to_string())
                                    .unwrap_or_else(|| "Not found".into())
                            ),
                            t,
                        )
                    }),
                )
                .child(widgets::label(
                    format!(
                        "Settings: {}",
                        state
                            .settings_path()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "Persistence unavailable".into())
                    ),
                    t,
                )),
        )
    }
    pub(super) fn banners(&self, cx: &Context<Self>) -> Div {
        let t = self.theme;
        let state = self.state.read(cx);
        let data = state.data();
        let missing = missing_tools(&data.tools);
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap_2()
            .when(!missing.is_empty() && data.tools_ready, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_5()
                        .py_3()
                        .bg(t.card)
                        .border_b_1()
                        .border_color(t.warning)
                        .child(
                            widgets::label(format!("Missing tools: {}", missing.join(", ")), t)
                                .flex_1(),
                        )
                        .child(
                            Button::new("download-ffmpeg", "Get FFmpeg", t).on_click(|_, _, cx| {
                                cx.open_url("https://ffmpeg.org/download.html")
                            }),
                        )
                        .child(
                            Button::new("download-mkvtoolnix", "Get MKVToolNix", t).on_click(
                                |_, _, cx| {
                                    cx.open_url("https://mkvtoolnix.download/downloads.html")
                                },
                            ),
                        ),
                )
            })
            .when(
                data.legacy_install.is_some() && !data.preferences.legacy_banner_dismissed,
                |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_5()
                            .py_3()
                            .bg(t.card)
                            .border_b_1()
                            .border_color(t.warning)
                            .child(
                                widgets::label("A LangMux 1.0 installation was found.", t).flex_1(),
                            )
                            .child(
                                Button::new(
                                    "remove-legacy",
                                    if state.legacy_running {
                                        "Removing…"
                                    } else {
                                        "Remove it"
                                    },
                                    t,
                                )
                                .disabled(state.legacy_running)
                                .on_click(cx.listener(
                                    |s, _, window, cx| {
                                        s.legacy_modal.update(cx, |s, cx| s.show(window, cx))
                                    },
                                )),
                            )
                            .child(Button::new("dismiss-legacy", "Not now", t).on_click(
                                cx.listener(|s, _, _, cx| {
                                    s.preferences(|p| p.legacy_banner_dismissed = true, cx)
                                }),
                            )),
                    )
                },
            )
    }
}

pub(super) fn engine_status(name: &str, tool: Option<&kestrel_core::tools::Tool>) -> String {
    match tool {
        None => format!("{name}: Not found"),
        Some(tool) => {
            let version = tool
                .version
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .find_map(|word| {
                    let word = word.trim_start_matches('v');
                    if word.starts_with(|c: char| c.is_ascii_digit()) {
                        Some(word.split('-').next().unwrap_or(word))
                    } else {
                        None
                    }
                });
            version
                .map(|v| format!("{name} {v}"))
                .unwrap_or_else(|| format!("{name}: Available"))
        }
    }
}
fn languages(codes: &[String]) -> String {
    if codes.is_empty() {
        "All".into()
    } else {
        codes.join(", ")
    }
}
fn missing_tools(tools: &kestrel_core::tools::Toolchain) -> Vec<&'static str> {
    [
        ("ffmpeg", tools.ffmpeg.is_none()),
        ("mkvmerge", tools.mkvmerge.is_none()),
        ("ffprobe", tools.ffprobe.is_none()),
    ]
    .into_iter()
    .filter_map(|(name, missing)| missing.then_some(name))
    .collect()
}
fn log_lines(data: &StateData, filter: Option<u64>) -> Vec<String> {
    data.logs
        .iter()
        .filter(|line| filter.is_none() || line.job_id == filter)
        .map(|line| {
            let seconds = line
                .timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                % 86400;
            let job = line
                .job_id
                .map(|id| format!(" [Job #{id}]"))
                .unwrap_or_default();
            format!("{} UTC{job}  {}", format_duration(seconds), line.text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{engine_status, languages, log_lines, missing_tools};
    use kestrel_core::{
        settings::Preferences,
        tools::{Tool, Toolchain},
    };
    use kestrel_gpui::state::reducer::StateData;
    use std::time::SystemTime;
    #[test]
    fn tool_statuses_trim_banners_and_cover_missing_and_unknown_versions() {
        let tool = |text: &str| Tool {
            path: "tool".into(),
            version: Some(text.into()),
        };
        assert_eq!(
            engine_status(
                "FFmpeg",
                Some(&tool("ffmpeg version 4.4.2-0ubuntu Copyright"))
            ),
            "FFmpeg 4.4.2"
        );
        assert_eq!(
            engine_status("MKVToolNix", Some(&tool("mkvmerge v82.0 ('name') 64-bit"))),
            "MKVToolNix 82.0"
        );
        assert_eq!(engine_status("FFmpeg", None), "FFmpeg: Not found");
        assert_eq!(
            engine_status("FFmpeg", Some(&tool(""))),
            "FFmpeg: Available"
        );
        assert_eq!(
            missing_tools(&Toolchain::default()),
            vec!["ffmpeg", "mkvmerge", "ffprobe"]
        );
    }
    #[test]
    fn logs_preserve_full_commands_timestamps_and_job_filters() {
        let mut data = StateData::new(Preferences::default());
        assert!(log_lines(&data, None).is_empty());
        data.log(
            Some(1),
            "ffmpeg -i 'a b.mkv' 'out.mkv'",
            SystemTime::UNIX_EPOCH,
        );
        data.log(None, "Queue finished", SystemTime::UNIX_EPOCH);
        assert_eq!(
            log_lines(&data, Some(1)),
            vec!["00:00:00 UTC [Job #1]  ffmpeg -i 'a b.mkv' 'out.mkv'"]
        );
        assert_eq!(log_lines(&data, None).len(), 2);
        assert!(log_lines(&data, Some(99)).is_empty());
        assert_eq!(languages(&[]), "All");
    }
}
