//! The Settings page (ADR 0007, ADR 0069) and the import dialogs (ADR 0002, 0014, 0033, 0038) of the window. The
//! logic they use is in `ricochet_app::settings_page` and `ricochet_app::import_page`, where tests reach it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{App, ClickEvent, Context, Entity, FontWeight, SharedString, Subscription, Window, div, prelude::*, px};
use ricochet_app::theme::{self, ThemeChoice, rgb};
use ricochet_app::import_page::{self, Applied};
use ricochet_app::settings_page::{self, TextSetting};
use ricochet_app::shell::{self, FolderChoice, FolderStep, ShellModel};
use ricochet_app::state::AppState;
use ricochet_app::text_input::{Changed, TextInput};
use ricochet_core::openapi::{self, OpenApiResult};
use ricochet_core::personal_state;
use ricochet_core::store::{self, WorkspaceFolder};
use ricochet_core::postman::{self, DumpEntry, EntryKind};
use ricochet_core::send::RequestSender;
use ricochet_core::{crash_log, settings_catalog::SettingItem};

use super::{ACCENT, BORDER, CHROME_BG, ERROR, FAINT, FIELD_BG, MONO, MUTED, ROW_HOVER, Shell, TEXT, WARNING, muted_note};

/// Global Variables have no item id of their own, so a change to them is tracked under this key.
pub const GLOBALS_KEY: &str = "globals";

/// The open Settings page.
pub struct SettingsPanel {
    search: Entity<TextInput>,
    query: String,
    inputs: HashMap<TextSetting, Entity<TextInput>>,
    /// An empty folder picked with Open folder…, waiting for the person to confirm the copy (C# `PendingCopyFolder`).
    pending_copy: Option<PathBuf>,
    /// The problem with what was typed into a field, shown beside it until the text can be used.
    errors: HashMap<TextSetting, String>,
    _subscriptions: Vec<Subscription>,
}

/// The choice shown after a Postman data dump is read (ADR 0038). Every export starts ticked.
pub struct DumpChoice {
    name: String,
    listing: Vec<DumpEntry>,
    ticked: HashSet<String>,
}

/// The OpenAPI dialog: an address to fetch, or a file, then a preview to apply (ADR 0014).
pub struct SpecDialog {
    url: Entity<TextInput>,
    busy: bool,
    error: Option<String>,
    preview: Option<OpenApiResult>,
}

fn overlay(id: &'static str, width: f32) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .absolute()
        .occlude()
        .top(px(56.0))
        .left(px(260.0))
        .w(px(width))
        .max_h(px(640.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CHROME_BG))
}

fn title(text: &'static str) -> gpui::Div {
    div().text_lg().font_weight(FontWeight::SEMIBOLD).child(text)
}

fn field(input: &Entity<TextInput>) -> gpui::Div {
    div().p_2().rounded_sm().border_1().border_color(rgb(BORDER)).bg(rgb(FIELD_BG)).font_family(MONO).child(input.clone())
}

fn toggle(id: SharedString, on: bool) -> gpui::Stateful<gpui::Div> {
    // A fixed width, so the page's column does not stretch it into something that looks like a text box.
    div()
        .id(id)
        .w(px(56.0))
        .flex()
        .justify_center()
        .py_1()
        .rounded_sm()
        .border_1()
        .border_color(rgb(if on { ACCENT } else { BORDER }))
        .bg(rgb(if on { ROW_HOVER } else { FIELD_BG }))
        .text_xs()
        .text_color(rgb(if on { TEXT } else { MUTED }))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(ROW_HOVER)))
        .child(if on { "On" } else { "Off" })
}

impl Shell {
    // ---- settings ----

    /// Opens the Settings page, or builds it again so its fields show the current settings.
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        let query = self.settings_panel.as_ref().map(|panel| panel.query.clone()).unwrap_or_default();
        let search = cx.new(|cx| TextInput::new(query.clone(), "Search settings", cx));
        let mut subscriptions = vec![cx.subscribe(&search, |this: &mut Shell, input, _: &Changed, cx| {
            if let Some(panel) = this.settings_panel.as_mut() {
                panel.query = input.read(cx).text().to_string();
            }
            cx.notify();
        })];

        let mut inputs = HashMap::new();
        for (field, placeholder) in [
            (TextSetting::Timeout, "Seconds"),
            (TextSetting::MaxResponse, "Megabytes"),
            (TextSetting::Proxy, "Empty uses the system proxy"),
            (TextSetting::ClientCertificate, "Path to a PEM file"),
        ] {
            let text = settings_page::text_of(&self.settings, field);
            let input = cx.new(|cx| TextInput::new(text, placeholder, cx));
            subscriptions.push(cx.subscribe(&input, move |this: &mut Shell, input, _: &Changed, cx| {
                let text = input.read(cx).text().to_string();
                this.change_text_setting(field, &text, cx);
            }));
            inputs.insert(field, input);
        }

        self.settings_panel = Some(SettingsPanel { search, query, inputs, pending_copy: None, errors: HashMap::new(), _subscriptions: subscriptions });
        cx.notify();
    }

    /// System, Light and Dark side by side, with the one in use marked. Choosing one applies and saves it at once.
    fn theme_control(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut row = div().flex().flex_row().gap_1();
        for choice in ThemeChoice::ALL {
            let on = choice == self.theme_choice;
            row = row.child(
                div()
                    .id(SharedString::from(format!("settings-theme-{}", choice.name().to_lowercase())))
                    .w(px(72.0))
                    .flex()
                    .justify_center()
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(if on { ACCENT } else { BORDER }))
                    .bg(rgb(if on { ROW_HOVER } else { FIELD_BG }))
                    .text_xs()
                    .text_color(rgb(if on { TEXT } else { MUTED }))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(ROW_HOVER)))
                    .child(choice.name())
                    .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, window: &mut Window, cx: &mut Context<Shell>| {
                        this.choose_theme(choice, window, cx)
                    })),
            );
        }
        row.into_any_element()
    }

    /// Uses this theme from now on and saves it in state.json, changing only the `theme` key (ADR 0013).
    fn choose_theme(&mut self, choice: ThemeChoice, window: &mut Window, cx: &mut Context<Self>) {
        if choice == self.theme_choice {
            return;
        }
        self.theme_choice = choice;
        if let Some(dir) = self.personal_dir.as_deref() {
            if let Err(error) = personal_state::save_theme(dir, choice.stored()) {
                self.notice = Some((format!("Your theme could not be saved, so it lasts only until you close Ricochet: {error}"), true));
            }
        }
        self.apply_theme(window, cx);
    }

    /// Draws the window in the chosen theme. System reads the window's light or dark appearance from the system.
    pub(super) fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let system_is_dark = matches!(window.appearance(), gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark);
        theme::set_dark(self.theme_choice.is_dark(system_is_dark));
        window.refresh();
        cx.notify();
    }

    fn change_text_setting(&mut self, field: TextSetting, text: &str, cx: &mut Context<Self>) {
        let mut settings = self.settings.clone();
        let outcome = settings_page::apply_text(&mut settings, field, text);
        if let Some(panel) = self.settings_panel.as_mut() {
            match &outcome {
                Ok(()) => panel.errors.remove(&field),
                Err(problem) => panel.errors.insert(field, problem.clone()),
            };
        }
        if outcome.is_ok() && settings != self.settings {
            self.commit_settings(settings, cx);
        }
        cx.notify();
    }

    /// Saves the settings in state.json and sends with them from now on, as `ChangeSettings` does in C#.
    pub(super) fn commit_settings(&mut self, settings: personal_state::PersonalSettings, cx: &mut Context<Self>) {
        self.settings = settings;
        if let Some(dir) = self.personal_dir.as_deref() {
            if let Err(error) = personal_state::save_settings(dir, &self.settings) {
                self.notice = Some((format!("Your settings could not be saved, so they last only until you close Ricochet: {error}"), true));
            }
        }
        let mut sender = RequestSender::new();
        self.connection_problem = sender.configure(self.settings.sender_settings());
        self.sender = std::sync::Arc::new(sender);
        cx.notify();
    }

    fn browse_client_certificate(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a client certificate".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |shell, cx| {
                let mut settings = shell.settings.clone();
                settings.client_certificate_path = path.to_string_lossy().into_owned();
                shell.commit_settings(settings, cx);
                // Built again so the field shows the chosen path.
                shell.open_settings(cx);
            })
            .ok();
        })
        .detach();
    }

    fn setting_control(&self, item: &SettingItem, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let panel = self.settings_panel.as_ref()?;
        let text_field = |field: TextSetting| panel.inputs.get(&field).map(|input| field_div(input).w(px(320.0)));
        let flip = |id: &'static str, on: bool, change: fn(&mut personal_state::PersonalSettings)| {
            toggle(SharedString::from(id), on)
                .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    let mut settings = this.settings.clone();
                    change(&mut settings);
                    this.commit_settings(settings, cx);
                }))
                .into_any_element()
        };
        let path_text = |path: Option<PathBuf>| {
            div()
                .text_xs()
                .font_family(MONO)
                .text_color(rgb(MUTED))
                .child(path.map(|p| p.display().to_string()).unwrap_or_else(|| "None".to_string()))
                .into_any_element()
        };
        Some(match item.key {
            "request-timeout" => text_field(TextSetting::Timeout)?.into_any_element(),
            "max-response-size" => text_field(TextSetting::MaxResponse)?.into_any_element(),
            "proxy-address" => text_field(TextSetting::Proxy)?.into_any_element(),
            "client-certificate-file" => div()
                .flex()
                .flex_row()
                .gap_2()
                .items_center()
                .children(text_field(TextSetting::ClientCertificate))
                .child(Shell::small_button(SharedString::from("settings-browse-certificate"), "Browse…".to_string()).on_click(
                    cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.browse_client_certificate(cx)),
                ))
                .into_any_element(),
            "client-certificate-password" => muted_note(
                "Not supported in this build: use a PEM file that holds the certificate and its unencrypted key.",
            )
            .into_any_element(),
            "follow-redirects" => flip("settings-follow-redirects", self.settings.follow_redirects, |s| s.follow_redirects = !s.follow_redirects),
            "verify-tls" => flip("settings-verify-tls", self.settings.verify_tls_certificates, |s| {
                s.verify_tls_certificates = !s.verify_tls_certificates
            }),
            "wrap-response" => flip("settings-wrap-response", self.settings.wrap_response, |s| s.wrap_response = !s.wrap_response),
            "crash-log-folder" => {
                let directory = self.personal_dir.as_deref().map(crash_log::directory_for);
                let mut column = div().flex().flex_col().gap_1().child(path_text(directory.clone()));
                if let Some(directory) = directory {
                    // The folder is created first, so it opens even before any crash, as in the C# app.
                    column = column.child(div().flex().flex_row().child(
                        Shell::small_button(SharedString::from("settings-open-crash-logs"), "Open crash log folder".to_string()).on_click(
                            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                let _ = std::fs::create_dir_all(&directory);
                                cx.open_with_system(&directory);
                            },
                        ),
                    ));
                }
                column.into_any_element()
            }
            "workspace-folder" => path_text(self.model.folder_path().map(PathBuf::from)),
            "workspace-folder" => self.workspace_folder_control(panel.pending_copy.as_deref(), cx).into_any_element(),
            _ => return None,
        })
    }

    /// The Settings page: a search box, then each section with its settings. Changes apply and save at once.
    pub(super) fn settings_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let panel = self.settings_panel.as_ref()?;
        let error_of = |key: &str| {
            let field = match key {
                "request-timeout" => TextSetting::Timeout,
                "max-response-size" => TextSetting::MaxResponse,
                "proxy-address" => TextSetting::Proxy,
                "client-certificate-file" => TextSetting::ClientCertificate,
                _ => return None,
            };
            panel.errors.get(&field).cloned()
        };

        let mut page = overlay("settings-page", 720.0)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(title("Settings"))
                    .child(div().flex_1())
                    .child(Shell::small_button(SharedString::from("settings-close"), "Close".to_string()).on_click(cx.listener(
                        |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.close_palette(cx),
                    ))),
            )
            .child(muted_note("Kept on this machine only, in state.json. Changes apply to the next request."))
            .child(field(&panel.search));

        if !self.settings.verify_tls_certificates {
            page = page.child(div().text_sm().text_color(rgb(WARNING)).child(ricochet_core::send::CERTIFICATE_CHECKS_OFF_WARNING));
        }
        if let Some(problem) = &self.connection_problem {
            page = page.child(div().text_sm().text_color(rgb(ERROR)).child(format!("Requests cannot be sent with these settings: {problem}")));
        }

        let theme_matches = theme::theme_setting_matches(&panel.query);
        if theme_matches {
            page = page
                .child(div().pt_2().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(FAINT)).child("APPEARANCE"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .py_1()
                        .child(div().text_sm().child(theme::THEME_SETTING_NAME))
                        .child(div().text_xs().text_color(rgb(MUTED)).child(theme::THEME_SETTING_HELP))
                        .child(self.theme_control(cx)),
                );
        }

        let sections = settings_page::visible_sections(&panel.query);
        if sections.is_empty() && !theme_matches {
            page = page.child(muted_note("No setting matches the search."));
        }
        for (section, items) in sections {
            page = page.child(div().pt_2().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(FAINT)).child(section.title.to_uppercase()));
            for item in items {
                let mut row = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .py_1()
                    .child(div().text_sm().child(item.name))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(item.help_text));
                if let Some(control) = self.setting_control(item, cx) {
                    row = row.child(control);
                }
                if let Some(error) = error_of(item.key) {
                    row = row.child(div().text_xs().text_color(rgb(ERROR)).child(error));
                }
                page = page.child(row);
            }
        }
        Some(page)
    }

    // ---- workspace folder ----

    /// The Workspace folder row: the path, Open folder… and Copy to new folder…, and the copy question when one waits.
    fn workspace_folder_control(&self, pending_copy: Option<&Path>, cx: &mut Context<Self>) -> gpui::Div {
        let path = self.model.folder_path().map_or_else(|| "No folder is open.".to_string(), |path| path.display().to_string());
        let buttons = div()
            .flex()
            .flex_row()
            .gap_2()
            .child(Shell::small_button(SharedString::from("settings-open-folder"), "Open folder…".to_string()).on_click(cx.listener(
                |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.pick_workspace_folder(FolderChoice::Open, cx),
            )))
            .child(Shell::small_button(SharedString::from("settings-copy-folder"), "Copy to new folder…".to_string()).on_click(cx.listener(
                |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.pick_workspace_folder(FolderChoice::CopyTo, cx),
            )));
        let mut control =
            div().flex().flex_col().gap_2().child(div().text_xs().font_family(MONO).text_color(rgb(MUTED)).child(path)).child(buttons);
        if let Some(target) = pending_copy {
            control = control.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(ACCENT))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Copy workspace here?"))
                    .child(div().text_xs().text_color(rgb(TEXT)).child(shell::pending_copy_message(target)))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(Shell::small_button(SharedString::from("settings-cancel-copy"), "Cancel".to_string()).on_click(cx.listener(
                                |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                    if let Some(panel) = this.settings_panel.as_mut() {
                                        panel.pending_copy = None;
                                    }
                                    cx.notify();
                                },
                            )))
                            .child(Shell::small_button(SharedString::from("settings-confirm-copy"), "Copy workspace".to_string()).on_click(
                                cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                    if let Some(path) = this.settings_panel.as_mut().and_then(|panel| panel.pending_copy.take()) {
                                        this.copy_workspace_to(&path, cx);
                                    }
                                }),
                            )),
                    ),
            );
        }
        control
    }

    /// Asks for a folder with the system's folder picker, then opens it or copies this Workspace into it.
    fn pick_workspace_folder(&mut self, choice: FolderChoice, cx: &mut Context<Self>) {
        let prompt = match choice {
            FolderChoice::Open => "Choose a workspace folder",
            FolderChoice::CopyTo => "Choose an empty folder for the copy",
        };
        let picked =
            cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(prompt.into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |shell, cx| shell.use_picked_folder(choice, &path, cx)).ok();
        })
        .detach();
    }

    fn use_picked_folder(&mut self, choice: FolderChoice, path: &Path, cx: &mut Context<Self>) {
        if let Some(panel) = self.settings_panel.as_mut() {
            panel.pending_copy = None;
        }
        match shell::folder_step(choice, path, self.model.folder_path()) {
            FolderStep::Open(path) => self.open_workspace_folder(&path, cx),
            FolderStep::Copy(path) => self.copy_workspace_to(&path, cx),
            FolderStep::ConfirmCopy(path) => {
                if let Some(panel) = self.settings_panel.as_mut() {
                    panel.pending_copy = Some(path);
                }
            }
            FolderStep::Refuse(reason) => self.notice = Some((reason, true)),
        }
        cx.notify();
    }

    /// Saves unsaved changes into the folder that is open before leaving it, as C# `SwitchFolderAsync` persists first.
    /// False, with the reason in the status bar, when they could not be saved, so nothing is lost by switching.
    fn save_before_switching(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.state.has_unsaved() {
            return true;
        }
        if self.store.is_none() {
            let text = "No folder is open, so your changes can't be saved first. Use Copy to new folder… to keep them.";
            self.notice = Some((text.to_string(), true));
            return false;
        }
        self.save_workspace(cx);
        if self.state.has_unsaved() {
            let reason = self.notice.take().map(|(text, _)| text).unwrap_or_default();
            self.notice = Some((format!("The folder was not changed, because your changes could not be saved first. {reason}"), true));
            return false;
        }
        true
    }

    /// Opens the Workspace in another folder. History and settings are personal, so they stay.
    fn open_workspace_folder(&mut self, path: &Path, cx: &mut Context<Self>) {
        cx.notify();
        if !self.save_before_switching(cx) {
            return;
        }
        let (model, store) = ShellModel::open(Some(path.to_path_buf()));
        let (Some(store), None) = (store, model.error.clone()) else {
            let reason = model.error.unwrap_or_default();
            self.notice = Some((format!("Could not open that folder. {reason}"), true));
            return;
        };
        let active = self.state.active_environment_id().map(str::to_string);
        let mut state = AppState::for_workspace(&model.workspace);
        if let Some(id) = active.filter(|id| model.workspace.environments.iter().any(|environment| &environment.id == id)) {
            state.set_active_environment(Some(&id));
        }
        let issues = model.issues.len();
        self.switch_to(model, store, state);
        self.notice = Some(if issues == 0 {
            (format!("Opened {}.", path.display()), false)
        } else {
            (format!("Opened {}. {issues} file(s) could not be read and were left alone.", path.display()), true)
        });
    }

    /// Copies this Workspace into an empty folder and switches to the copy. The folder it came from is left as it is.
    fn copy_workspace_to(&mut self, path: &Path, cx: &mut Context<Self>) {
        cx.notify();
        if !self.save_before_switching(cx) {
            return;
        }
        // Secrets live in the vault, never in the Workspace, so none are written into the new folder.
        match store::copy_into_empty_folder(path, &self.model.workspace) {
            Ok(store) => {
                let branch = ricochet_app::status::read_branch(path);
                let workspace = self.model.workspace.clone();
                let model = ShellModel::from_parts(Some(path.to_path_buf()), workspace, Vec::new(), branch);
                // The same items under the same ids, so the open tabs and the active Environment carry over.
                let state = std::mem::replace(&mut self.state, AppState::for_workspace(&model.workspace));
                self.switch_to(model, store, state);
                self.notice = Some((format!("Your workspace was copied to {}. The previous folder was left as it was.", path.display()), false));
            }
            Err(error) => self.notice = Some((format!("The workspace was not copied: {error}"), true)),
        }
    }

    /// Shows another folder's Workspace and remembers the folder in state.json for next time.
    fn switch_to(&mut self, model: ShellModel, store: WorkspaceFolder, state: AppState) {
        let folder = model.folder.clone();
        self.model = model;
        self.store = Some(store);
        self.state = state;
        self.sync = self.model.folder.as_deref().and_then(ricochet_app::status::read_sync);
        self.dot_env = super::read_dot_env(&self.model);
        self.inputs = None;
        self.container_inputs = None;
        self.editing_environment = None;
        self.tree_target = None;
        self.confirm_delete = None;
        self.renaming = None;
        self.reload_armed = false;
        if let (Some(dir), Some(folder)) = (self.personal_dir.as_deref(), folder.as_deref()) {
            let default = std::env::var("RICOCHET_DATA").ok().filter(|value| !value.trim().is_empty()).map(PathBuf::from);
            let remembered = shell::folder_to_remember(folder, default.as_deref());
            if let Err(error) = personal_state::save_workspace_folder(dir, remembered.as_deref()) {
                self.notice = Some((format!("This folder will not open next time, because state.json could not be saved: {error}"), true));
            }
        }
    }

    // ---- imports ----

    /// Asks for one or more export files and imports them. A data dump opens a choice instead (ADR 0038).
    pub(super) fn import_files(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Choose Postman or Insomnia exports, or a Postman data dump".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            this.update(cx, |shell, cx| shell.finish_import_files(&paths, cx)).ok();
        })
        .detach();
    }

    /// Imports each file on its own, so one bad file does not stop the others. Mirrors `ImportFilesAsync`.
    pub(super) fn finish_import_files(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let (dumps, files): (Vec<&PathBuf>, Vec<&PathBuf>) = paths.iter().partition(|path| import_page::is_dump(path));
        let mut report = Vec::new();
        for path in files {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match std::fs::read(path) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let applied = import_page::import_text(&mut self.model.workspace, text.trim_start_matches('\u{feff}'), &name);
                    report.push(self.mark_applied(applied));
                }
                Err(error) => report.push(format!("{name}\nNot imported. The file could not be read: {error}")),
            }
        }

        if dumps.len() > 1 {
            let skipped: Vec<String> = dumps[1..].iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
            report.push(format!("Import one data dump at a time. Not imported: {}", skipped.join(", ")));
        }
        if let Some(dump) = dumps.first() {
            let name = dump.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let listing = std::fs::File::open(dump)
                .map_err(|error| error.to_string())
                .and_then(|file| postman::read_dump(std::io::BufReader::new(file)).map_err(|error| error.to_string()));
            match listing {
                Ok(listing) => {
                    let ticked = listing.iter().map(|entry| entry.file_name.clone()).collect();
                    self.dump_choice = Some(DumpChoice { name, listing, ticked });
                }
                Err(error) => report.push(format!("{name}\nNot imported. {error}")),
            }
        }

        self.import_report = (!report.is_empty()).then(|| report.join("\n\n"));
        cx.notify();
    }

    /// Marks what an import added as unsaved, so the next save writes it, and returns its report.
    fn mark_applied(&mut self, applied: Applied) -> String {
        for id in &applied.changed_ids {
            self.state.mark_unsaved(id);
        }
        if applied.globals_changed {
            self.state.mark_unsaved(GLOBALS_KEY);
        }
        applied.report
    }

    fn import_ticked(&mut self, cx: &mut Context<Self>) {
        let Some(choice) = self.dump_choice.take() else {
            return;
        };
        let applied = import_page::apply_dump(&mut self.model.workspace, &choice.name, &choice.listing, &choice.ticked);
        self.import_report = Some(self.mark_applied(applied));
        cx.notify();
    }

    /// The import report: what each file added, and what could not be carried over.
    pub(super) fn import_report_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let report = self.import_report.as_ref()?;
        Some(
            overlay("import-report", 640.0)
                .child(title("Import"))
                .child(muted_note("The imported items are not on disk yet. Save to keep them."))
                .child(div().text_xs().font_family(MONO).text_color(rgb(TEXT)).children(report.lines().map(|line| div().min_h(px(14.0)).child(line.to_string()))))
                // In a row, so the dialog's column does not stretch the button across it.
                .child(div().flex().flex_row().child(
                    Shell::small_button(SharedString::from("import-report-close"), "Close".to_string()).on_click(cx.listener(
                        |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                            this.import_report = None;
                            cx.notify();
                        },
                    )),
                )),
        )
    }

    /// The data dump choice: every export with a tick box, then Import and Cancel (ADR 0038).
    pub(super) fn dump_choice_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let choice = self.dump_choice.as_ref()?;
        let rows = choice.listing.iter().enumerate().map(|(index, entry)| {
            let ticked = choice.ticked.contains(&entry.file_name);
            let file_name = entry.file_name.clone();
            let kind = match entry.kind {
                EntryKind::Collection => "Collection",
                EntryKind::Environment => "Environment",
                EntryKind::Globals => "Globals",
            };
            div()
                .id(SharedString::from(format!("dump-entry-{index}")))
                .flex()
                .flex_row()
                .gap_2()
                .items_center()
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(ROW_HOVER)))
                .child(div().w(px(16.0)).text_color(rgb(if ticked { ACCENT } else { FAINT })).child(if ticked { "[x]" } else { "[ ]" }))
                .child(div().w(px(90.0)).text_xs().text_color(rgb(MUTED)).child(kind))
                .child(div().flex_1().child(entry.name.clone()))
                .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    if let Some(choice) = this.dump_choice.as_mut() {
                        if !choice.ticked.remove(&file_name) {
                            choice.ticked.insert(file_name.clone());
                        }
                    }
                    cx.notify();
                }))
        });
        Some(
            overlay("dump-choice", 560.0)
                .child(title("Choose what to import"))
                .child(muted_note(format!("{}: {} of {} selected. Nothing else in the dump is touched.", choice.name, choice.ticked.len(), choice.listing.len())))
                .children(rows)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(Shell::small_button(SharedString::from("dump-import"), "Import selected".to_string()).on_click(cx.listener(
                            |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.import_ticked(cx),
                        )))
                        .child(Shell::small_button(SharedString::from("dump-cancel"), "Cancel".to_string()).on_click(cx.listener(
                            |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.dump_choice = None;
                                cx.notify();
                            },
                        ))),
                ),
        )
    }

    // ---- OpenAPI ----

    /// Opens the OpenAPI dialog, with the address of the selected Collection's description when it has one.
    pub(super) fn open_spec_dialog(&mut self, cx: &mut Context<Self>) {
        let selected = self.state.selected_item().map(str::to_string);
        let address = selected
            .and_then(|id| ricochet_app::scope::collection_of(&self.model.workspace, &id).and_then(|c| c.spec_url.clone()))
            .unwrap_or_default();
        let url = cx.new(|cx| TextInput::new(address, "https://example.com/openapi.json", cx));
        self.spec_dialog = Some(SpecDialog { url, busy: false, error: None, preview: None });
        cx.notify();
    }

    /// Fetches the description only when the person presses Fetch (ADR 0005), then shows what would change.
    fn fetch_spec(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.spec_dialog.as_mut() else {
            return;
        };
        let address = dialog.url.read(cx).text().trim().to_string();
        dialog.busy = true;
        dialog.error = None;
        dialog.preview = None;
        let work = cx.background_executor().spawn({
            let address = address.clone();
            async move { openapi::fetch(&address) }
        });
        cx.spawn(async move |this, cx| {
            let fetched = work.await;
            this.update(cx, |shell, cx| shell.show_spec_preview(fetched, &address, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn spec_from_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an OpenAPI 3 description (JSON)".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let text = std::fs::read(&path)
                .map(|bytes| String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
                .map_err(|error| format!("The file could not be read: {error}"));
            // A file has no address to update from later, so the path stands in for it, as the mock server's does.
            let address = path.to_string_lossy().into_owned();
            this.update(cx, |shell, cx| shell.show_spec_preview(text, &address, cx)).ok();
        })
        .detach();
    }

    fn show_spec_preview(&mut self, text: Result<String, String>, address: &str, cx: &mut Context<Self>) {
        let preview = text.and_then(|text| import_page::preview_spec(&self.model.workspace, &text, address));
        if let Some(dialog) = self.spec_dialog.as_mut() {
            dialog.busy = false;
            match preview {
                Ok(result) => dialog.preview = Some(result),
                Err(error) => dialog.error = Some(error),
            }
        }
        cx.notify();
    }

    fn apply_spec(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self.spec_dialog.as_mut().and_then(|dialog| dialog.preview.take()) else {
            return;
        };
        let applied = import_page::apply_spec(&mut self.model.workspace, result);
        self.import_report = Some(self.mark_applied(applied));
        self.spec_dialog = None;
        cx.notify();
    }

    /// The OpenAPI dialog: the address, Fetch or a file, then the preview and Apply.
    pub(super) fn spec_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let dialog = self.spec_dialog.as_ref()?;
        let mut page = overlay("spec-dialog", 640.0)
            .child(title("Collection from OpenAPI"))
            .child(muted_note("Fetched only when you press Fetch. Nothing changes until you press Apply. OpenAPI 3 in JSON only."))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(field(&dialog.url).flex_1())
                    .child(Shell::small_button(SharedString::from("spec-fetch"), if dialog.busy { "Fetching…" } else { "Fetch" }.to_string()).on_click(
                        cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                            if !this.spec_dialog.as_ref().is_some_and(|d| d.busy) {
                                this.fetch_spec(cx);
                            }
                        }),
                    ))
                    .child(Shell::small_button(SharedString::from("spec-file"), "From file…".to_string()).on_click(cx.listener(
                        |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.spec_from_file(cx),
                    ))),
            );
        if let Some(error) = &dialog.error {
            page = page.child(div().text_sm().text_color(rgb(ERROR)).child(error.clone()));
        }
        if let Some(preview) = &dialog.preview {
            let text = preview.summary.to_report_text();
            page = page.child(div().text_xs().font_family(MONO).children(text.lines().map(|line| div().min_h(px(14.0)).child(line.to_string()))));
        }
        let can_apply = dialog.preview.is_some();
        page = page.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    Shell::small_button(SharedString::from("spec-apply"), "Apply".to_string())
                        .when(!can_apply, |button| button.opacity(0.4).cursor_default())
                        .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.apply_spec(cx))),
                )
                .child(Shell::small_button(SharedString::from("spec-cancel"), "Cancel".to_string()).on_click(cx.listener(
                    |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.spec_dialog = None;
                        cx.notify();
                    },
                ))),
        );
        Some(page)
    }
}

fn field_div(input: &Entity<TextInput>) -> gpui::Div {
    field(input)
}
