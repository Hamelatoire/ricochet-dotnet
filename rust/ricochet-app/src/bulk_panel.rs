//! The bulk run panel of the window (ADR 0015, ADR 0017), as the C# bulk page: a placeholder, how many ids go out at
//! once, the ids (pasted or loaded from a file), Start, Resume and Stop, a progress bar and the log. The logic is in
//! `ricochet_app::bulk_page`, where tests reach it. Progress is saved in state.json and every log line is appended to
//! bulk-log.txt in the personal folder, as the C# app does.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui::{ClickEvent, Context, Entity, FontWeight, ScrollHandle, SharedString, Window, div, prelude::*, px};
use ricochet_app::bulk_page::{self, BulkShared, RunTracker};
use ricochet_app::text_area::TextArea;
use ricochet_app::text_input::TextInput;
use ricochet_app::theme::rgb;
use ricochet_app::tree::find_request;
use ricochet_core::bulk_run_progress;
use ricochet_core::bulk_runner;
use ricochet_core::personal_state::{self, BulkProgress};

use super::{ACCENT, BORDER, CHROME_BG, ERROR, FAINT, FIELD_BG, MONO, MUTED, SUCCESS, Shell, TEXT, muted_note};

/// The most log lines drawn at once. The panel keeps every line of the run, and bulk-log.txt keeps every run.
const LOG_LINES_SHOWN: usize = 500;

/// One line of the log on screen, and whether its id succeeded. None for the run's own lines.
struct LogLine {
    text: String,
    succeeded: Option<bool>,
}

/// The bulk run panel. It stays alive while hidden, so a run keeps going and its log is kept, as on the C# page.
pub struct BulkPanel {
    item_id: String,
    request_name: String,
    visible: bool,
    placeholder: Entity<TextInput>,
    threads: usize,
    ids: Entity<TextArea>,
    /// A list too long for the id box (see `bulk_page::MAX_IDS_IN_BOX`), and where it came from. The run sends it in
    /// place of the box's text. None while the ids are in the box.
    kept: Option<(String, Vec<String>)>,
    status: String,
    /// The latest run, and how many of its results the panel has taken so far. None before the first run.
    shared: Option<Arc<BulkShared>>,
    taken: usize,
    tracker: Option<RunTracker>,
    /// The bar's numbers when no run has started since the panel opened: the saved count and the pasted ids.
    idle_processed: usize,
    log: Vec<LogLine>,
    log_scroll: ScrollHandle,
}

impl BulkPanel {
    fn running(&self) -> bool {
        self.shared.as_ref().is_some_and(|shared| !shared.done.load(Ordering::SeqCst))
    }

    /// Hides the panel. A run still going carries on, as when the C# page is closed.
    pub fn hide(&mut self) {
        self.visible = false;
    }
}

impl Shell {
    /// The progress saved in state.json for a Request's bulk run, if any.
    fn saved_bulk(&self, item_id: &str) -> Option<&BulkProgress> {
        self.bulk_progress.iter().find(|progress| progress.request_id == item_id)
    }

    /// Keeps a Request's bulk progress, and saves it in state.json.
    fn save_bulk(&mut self, progress: BulkProgress) {
        if let Some(dir) = self.personal_dir.as_deref() {
            if let Err(error) = personal_state::save_bulk_progress(dir, &progress) {
                self.notice = Some((format!("Bulk run progress could not be saved: {error}"), true));
            }
        }
        self.bulk_progress.retain(|saved| saved.request_id != progress.request_id);
        self.bulk_progress.push(progress);
    }

    /// Adds lines to bulk-log.txt, each after the Request's id, as the C# app writes them.
    fn append_bulk_log(&mut self, item_id: &str, lines: &[String]) {
        let Some(dir) = self.personal_dir.as_deref() else {
            return;
        };
        let lines: Vec<String> = lines.iter().map(|line| format!("{item_id} {line}")).collect();
        if let Err(error) = personal_state::append_bulk_log(dir, &lines) {
            self.notice = Some((format!("The bulk run log could not be written: {error}"), true));
        }
    }

    /// Opens the bulk run panel for the selected Request. The panel already open for it, or one with a run still
    /// going, is shown again as it was. Otherwise the ids saved for the Request are filled in.
    pub(super) fn open_bulk(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = self.bulk.as_mut().filter(|panel| panel.running()) {
            panel.visible = true;
            cx.notify();
            return;
        }
        let selected = self.state.selected_item().map(str::to_string);
        let Some(item_id) = selected.filter(|id| find_request(&self.model.workspace, id).is_some()) else {
            self.notice = Some(("Pick a request to run it for each id.".to_string(), true));
            cx.notify();
            return;
        };
        let name = find_request(&self.model.workspace, &item_id).map(|request| request.name.clone()).unwrap_or_default();
        let request_name = if name.is_empty() { "this request".to_string() } else { format!("\"{name}\"") };
        let saved = self.saved_bulk(&item_id).cloned();
        let status = bulk_page::opening_status(saved.as_ref());
        let idle_processed = saved.as_ref().map_or(0, |saved| saved.processed.max(0) as usize);

        if let Some(panel) = self.bulk.as_mut().filter(|panel| panel.item_id == item_id) {
            panel.visible = true;
            panel.request_name = request_name;
            if panel.tracker.is_none() {
                panel.status = status;
                panel.idle_processed = idle_processed;
            }
            cx.notify();
            return;
        }

        // The placeholder and the number of threads carry over from the last panel, as they do on the C# page.
        let (placeholder_text, threads) = match self.bulk.as_ref() {
            Some(panel) => (panel.placeholder.read(cx).text().to_string(), panel.threads),
            None => (bulk_runner::DEFAULT_PLACEHOLDER.to_string(), bulk_page::DEFAULT_THREADS),
        };
        let saved_ids = saved.as_ref().map(|saved| bulk_runner::parse_ids(&saved.ids)).unwrap_or_default();
        let placeholder = cx.new(|cx| TextInput::new(placeholder_text, "id", cx));
        let ids = cx.new(|cx| TextArea::new(String::new(), "Paste ids here, one per line", cx));
        self.bulk = Some(BulkPanel {
            item_id,
            request_name,
            visible: true,
            placeholder,
            threads,
            ids,
            kept: None,
            status,
            shared: None,
            taken: 0,
            tracker: None,
            idle_processed,
            log: Vec::new(),
            log_scroll: ScrollHandle::new(),
        });
        self.set_bulk_ids(saved_ids, "the last run", cx);
        cx.notify();
    }

    /// The ids the next run sends: the long list kept beside the box, or the ids in the box.
    fn bulk_ids(&self, cx: &Context<Self>) -> Vec<String> {
        match self.bulk.as_ref() {
            Some(BulkPanel { kept: Some((_, ids)), .. }) => ids.clone(),
            Some(panel) => bulk_runner::parse_ids(panel.ids.read(cx).text()),
            None => Vec::new(),
        }
    }

    /// Replaces the ids on the panel. A short list goes in the box, which is made again, as that is how a text area
    /// gets new text. A list too long for the box is kept beside it, named by `source`, and the box is emptied.
    fn set_bulk_ids(&mut self, ids: Vec<String>, source: &str, cx: &mut Context<Self>) {
        let Some(panel) = self.bulk.as_mut() else {
            return;
        };
        let text = if bulk_page::fits_in_box(ids.len()) {
            panel.kept = None;
            ids.join("\n")
        } else {
            panel.kept = Some((source.to_string(), ids));
            String::new()
        };
        panel.ids = cx.new(|cx| TextArea::new(text, "Paste ids here, one per line", cx));
    }

    /// Loads ids from a text file the person picks, one per line, in place of the ids in the box. The file is only
    /// read, never changed.
    fn import_bulk_ids(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a text file of ids".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            // A large file is read and split on a background thread, so the window does not stall.
            let file_name = name.clone();
            let read = cx
                .background_executor()
                .spawn(async move { std::fs::read_to_string(&path).map(|text| bulk_page::load_ids(&text, &file_name)) })
                .await;
            this.update(cx, |shell, cx| {
                match read {
                    Ok((ids, status)) => {
                        shell.set_bulk_ids(ids, &name, cx);
                        if let Some(panel) = shell.bulk.as_mut() {
                            panel.status = status;
                        }
                    }
                    Err(error) => {
                        if let Some(panel) = shell.bulk.as_mut() {
                            panel.status = format!("Could not read {name}: {error}");
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Starts a run from the first id, or (`resume`) after the last id saved for the Request. The ids go out on a
    /// background thread, at most the chosen number at once, and the window takes the results in turns.
    fn start_bulk(&mut self, resume: bool, cx: &mut Context<Self>) {
        let Some(panel) = self.bulk.as_ref() else {
            return;
        };
        if panel.running() {
            return;
        }
        let item_id = panel.item_id.clone();
        let placeholder = bulk_page::placeholder_or_default(panel.placeholder.read(cx).text()).to_string();
        let threads = bulk_page::clamp_threads(panel.threads as i64);
        let plan = match bulk_page::plan_run(self.bulk_ids(cx), self.saved_bulk(&item_id), resume) {
            Ok(plan) => plan,
            Err(error) => {
                if let Some(panel) = self.bulk.as_mut() {
                    panel.status = error;
                }
                cx.notify();
                return;
            }
        };
        // Starting over forgets the last id, so a Resume after it begins from the first.
        if !resume {
            self.save_bulk(bulk_page::starting_progress(&item_id, &plan.ids));
        }

        let first = plan.ids[plan.start].clone();
        let count = plan.ids.len() - plan.start;
        let header = bulk_page::run_header(&first, threads, count, &chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        self.append_bulk_log(&item_id, std::slice::from_ref(&header));
        let ids: Vec<String> = plan.ids[plan.start..].to_vec();
        let shared = Arc::new(BulkShared::default());
        let Some(panel) = self.bulk.as_mut() else {
            return;
        };
        // Each run starts with an empty log, so its lines aren't mixed with an earlier run's.
        panel.log.clear();
        panel.log.push(LogLine { text: header, succeeded: None });
        if let Some(notice) = plan.notice {
            panel.log.push(LogLine { text: notice, succeeded: None });
        }
        panel.tracker = Some(RunTracker::new(&item_id, plan.ids, plan.start));
        panel.shared = Some(shared.clone());
        panel.taken = 0;
        panel.status = bulk_page::starting_status(&first, threads);

        let workspace = self.model.workspace.clone();
        let environment = self.state.active_environment_id().map(str::to_string);
        let overrides = self.dot_env.clone();
        let sender = self.sender.clone();
        let worker = shared.clone();
        let work = cx.background_executor().spawn(async move {
            bulk_page::run(&workspace, &item_id, environment.as_deref(), &ids, &placeholder, threads, &overrides, &sender, &worker)
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                let done = shared.done.load(Ordering::SeqCst);
                this.update(cx, |shell, cx| shell.take_bulk_results(&shared, cx)).ok();
                if done {
                    break;
                }
            }
            let outcome = work.await;
            this.update(cx, |shell, cx| {
                shell.take_bulk_results(&shared, cx);
                if let Some(panel) = shell.bulk.as_mut().filter(|panel| panel.shared.as_ref().is_some_and(|own| Arc::ptr_eq(own, &shared))) {
                    panel.status = match (&outcome, panel.tracker.as_ref()) {
                        (Err(error), _) => error.clone(),
                        (Ok(finished), Some(tracker)) => tracker.ended_status(*finished),
                        (Ok(_), None) => String::new(),
                    };
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Takes the results that came in since the last turn: they go in the log and bulk-log.txt, and the progress is
    /// saved in state.json, so a run stopped by a crash still resumes from about where it was.
    fn take_bulk_results(&mut self, shared: &Arc<BulkShared>, cx: &mut Context<Self>) {
        let Some(panel) = self.bulk.as_mut() else {
            return;
        };
        if !panel.shared.as_ref().is_some_and(|own| Arc::ptr_eq(own, shared)) {
            return;
        }
        let results = shared.results_since(panel.taken);
        if results.is_empty() {
            return;
        }
        panel.taken += results.len();
        let Some(tracker) = panel.tracker.as_mut() else {
            return;
        };
        let mut lines = Vec::with_capacity(results.len());
        for result in &results {
            tracker.record(result);
            let line = result.log_line();
            lines.push(line.clone());
            panel.log.push(LogLine { text: line, succeeded: Some(result.succeeded) });
        }
        panel.status = tracker.running_status();
        let progress = tracker.progress();
        let item_id = panel.item_id.clone();
        panel.log_scroll.scroll_to_bottom();
        self.append_bulk_log(&item_id, &lines);
        self.save_bulk(progress);
        cx.notify();
    }

    fn stop_bulk(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = self.bulk.as_mut() {
            if let Some(shared) = panel.shared.as_ref().filter(|_| panel.running()) {
                shared.stop.store(true, Ordering::SeqCst);
                panel.status = "Stopping after the current id…".to_string();
            }
        }
        cx.notify();
    }

    /// A button that does nothing while `enabled` is false, drawn faded.
    fn bulk_button(
        id: &'static str,
        label: &str,
        enabled: bool,
        primary: bool,
        cx: &mut Context<Self>,
        action: fn(&mut Shell, &mut Context<Shell>),
    ) -> gpui::Stateful<gpui::Div> {
        let button = Self::small_button(SharedString::from(id), label.to_string()).when(primary, |button| {
            button.border_color(rgb(ACCENT)).text_color(rgb(TEXT))
        });
        if enabled {
            button.on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                action(this, cx)
            }))
        } else {
            button.opacity(0.4).cursor_default()
        }
    }

    /// The bulk run panel, or None while it is closed or hidden.
    pub(super) fn bulk_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let panel = self.bulk.as_ref().filter(|panel| panel.visible)?;
        let running = panel.running();
        let idle = !running;
        let saved = self.saved_bulk(&panel.item_id);
        let has_resume_point = saved.is_some_and(|saved| saved.last_id.is_some());
        let (processed, total) = match panel.tracker.as_ref() {
            Some(tracker) => (tracker.processed(), tracker.total()),
            None => (
                panel.idle_processed,
                panel.kept.as_ref().map_or_else(|| bulk_runner::parse_ids(panel.ids.read(cx).text()).len(), |(_, ids)| ids.len()),
            ),
        };
        let processed = processed.min(total);
        let percent = bulk_run_progress::percent(processed as i32, total as i32);
        let shown_from = panel.log.len().saturating_sub(LOG_LINES_SHOWN);
        let lines: Vec<_> = panel.log[shown_from..]
            .iter()
            .map(|line| {
                let color = match line.succeeded {
                    Some(true) => SUCCESS,
                    Some(false) => ERROR,
                    None => MUTED,
                };
                div().text_xs().font_family(MONO).text_color(rgb(color)).child(line.text.clone())
            })
            .collect();

        let label = |text: &'static str| div().w(px(90.0)).text_sm().child(text);
        let field = |id: &'static str| {
            div().id(id).flex_1().px_2().py_1().rounded_sm().border_1().border_color(rgb(BORDER)).bg(rgb(FIELD_BG))
        };

        Some(
            div()
                .id("bulk-panel")
                // Clicks on the panel stay on it, rather than reaching the Request fields beneath.
                .occlude()
                .absolute()
                .top(px(60.0))
                .bottom(px(16.0))
                .left(px(300.0))
                .right(px(16.0))
                .max_w(px(760.0))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .p_4()
                .rounded_md()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CHROME_BG))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Bulk run"))
                                .child(muted_note(format!("Sends {} once for each id. Escape hides this panel; a run keeps going.", panel.request_name)).text_xs()),
                        )
                        .child(Self::bulk_button("bulk-close", "Done", true, true, cx, |this, cx| {
                            if let Some(panel) = this.bulk.as_mut() {
                                panel.hide();
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(label("Placeholder"))
                        .child(field("bulk-placeholder").child(panel.placeholder.clone())),
                )
                .child(muted_note("Write {{id}} (or the placeholder above) in the URL, body or headers. Each id replaces it.").text_xs())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(label("Threads"))
                        .child(Self::bulk_button("bulk-threads-down", "−", idle && panel.threads > bulk_page::MIN_THREADS, false, cx, |this, cx| {
                            if let Some(panel) = this.bulk.as_mut() {
                                panel.threads = bulk_page::step_threads(panel.threads, -1);
                            }
                            cx.notify();
                        }))
                        .child(div().w(px(28.0)).text_sm().font_family(MONO).flex().justify_center().child(panel.threads.to_string()))
                        .child(Self::bulk_button("bulk-threads-up", "+", idle && panel.threads < bulk_page::MAX_THREADS, false, cx, |this, cx| {
                            if let Some(panel) = this.bulk.as_mut() {
                                panel.threads = bulk_page::step_threads(panel.threads, 1);
                            }
                            cx.notify();
                        }))
                        .child(div().text_xs().text_color(rgb(FAINT)).child(format!(
                            "How many ids are sent at once, {} to {}. 1 sends them one at a time.",
                            bulk_page::MIN_THREADS,
                            bulk_page::MAX_THREADS
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().text_xs().text_color(rgb(MUTED)).child("Ids, one per line"))
                        .child(Self::bulk_button("bulk-import", "Import from file…", idle, false, cx, |this, cx| this.import_bulk_ids(cx)))
                        .child(Self::bulk_button("bulk-clear-ids", "Remove ids", idle, false, cx, |this, cx| {
                            // Saved progress and the log are kept, so Resume still knows where it stopped.
                            this.set_bulk_ids(Vec::new(), "", cx);
                            if let Some(panel) = this.bulk.as_mut() {
                                panel.status = "Ids removed.".to_string();
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("bulk-ids")
                        .h(px(120.0))
                        .flex_none()
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(FIELD_BG))
                        .overflow_y_scroll()
                        .when(running, |ids| ids.opacity(0.6))
                        .map(|ids| match panel.kept.as_ref() {
                            Some((source, kept)) => ids.child(muted_note(bulk_page::kept_beside_box(kept.len(), source)).text_sm()),
                            None => ids.font_family(MONO).child(panel.ids.clone()),
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(Self::bulk_button("bulk-start", "Start", idle, !has_resume_point, cx, |this, cx| this.start_bulk(false, cx)))
                        .child(Self::bulk_button("bulk-resume", "Resume", idle, has_resume_point, cx, |this, cx| this.start_bulk(true, cx)))
                        .child(Self::bulk_button("bulk-stop", "Stop", running, false, cx, |this, cx| this.stop_bulk(cx)))
                        .child(div().flex_1().text_sm().text_color(rgb(MUTED)).child(panel.status.clone())),
                )
                .child(div().text_xs().text_color(rgb(MUTED)).child(bulk_page::action_hint(running, saved)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .h(px(8.0))
                                .rounded_sm()
                                .bg(rgb(FIELD_BG))
                                .child(div().h_full().rounded_sm().bg(rgb(ACCENT)).w(gpui::relative(percent as f32 / 100.0))),
                        )
                        .child(div().text_xs().child(bulk_run_progress::describe(processed as i32, total as i32))),
                )
                .child(
                    div()
                        .id("bulk-log")
                        .track_scroll(&panel.log_scroll)
                        .flex_1()
                        .min_h(px(60.0))
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(FIELD_BG))
                        .overflow_y_scroll()
                        .when(lines.is_empty(), |log| {
                            log.child(muted_note("Nothing has run yet. Paste the ids above, then press Start to send this request for each one.").text_xs())
                        })
                        .children(lines),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(div().flex_1().text_xs().text_color(rgb(MUTED)).child(if shown_from > 0 {
                            format!("The last {LOG_LINES_SHOWN} lines of this run. The saved log, bulk-log.txt, keeps every run.")
                        } else {
                            "The log on this screen. The saved log, bulk-log.txt, keeps every run.".to_string()
                        }))
                        .child(Self::bulk_button("bulk-clear-log", "Clear log", idle, false, cx, |this, cx| {
                            if let Some(panel) = this.bulk.as_mut() {
                                panel.log.clear();
                            }
                            cx.notify();
                        })),
                ),
        )
    }
}
