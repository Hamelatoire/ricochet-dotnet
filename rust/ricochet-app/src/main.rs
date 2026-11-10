// The Ricochet window (ADR 0091): a top bar with Save and the Environment, a sidebar of Collections and recent sends, a request
// pane with a URL bar and section tabs, a response pane and a status bar. Reads a Workspace folder from the first
// argument or RICOCHET_DATA. The URL, method and headers can be edited and saved; the body and auth are read-only.

use std::collections::HashMap;

use gpui::{
    App, Application, Bounds, ClickEvent, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight, KeyBinding, Render, SharedString, Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions,
    actions, div, prelude::*, px, rgb, size,
};
use ricochet_app::edit::{self, Field, set_secret};
use ricochet_core::personal_state::default_personal_directory;
use ricochet_core::secrets::{self, SecretValue, SecretVault, auth_key};
use std::collections::BTreeMap;
use ricochet_app::json_color::{SpanKind, color_line};
use ricochet_app::request_view::{RecentSend, RecentSends, UrlSegment, query_params, url_segments};
use ricochet_app::scope::variables_in_scope;
use ricochet_app::shell::{ShellModel, folder_from};
use ricochet_app::state::AppState;
use ricochet_app::status::NO_ENVIRONMENT;
use ricochet_app::tree::{RowKind, TreeRow, build_tree_folded, filter_rows, find_request};
use ricochet_core::models::{ApiRequest, KeyValue, auth_kinds};
use ricochet_core::response_status::{ResponseStatusClass, classify};
use ricochet_core::send::{ApiResponse, RequestSender};
use ricochet_core::store::WorkspaceFolder;
use ricochet_core::{dotenv, response_format, variables};
use ricochet_app::text_area::{self, TextArea};
use ricochet_app::text_input::{self, Changed, TextInput};

actions!(ricochet, [Save, SendRequest]);

/// How much of a response body the pane draws. The rest is cut, so a large body cannot stall the window.
const DISPLAY_CHARS: usize = 20_000;
const DISPLAY_LINES: usize = 1_500;
/// Width of one space of JSON indent.
const INDENT_STEP: f32 = 7.5;

const WINDOW_BG: u32 = 0x1a1c20;
const CHROME_BG: u32 = 0x1f2227;
const FIELD_BG: u32 = 0x15171a;
const BORDER: u32 = 0x2e323a;
const ROW_HOVER: u32 = 0x2a2e36;
const ROW_SELECTED: u32 = 0x25354f;
const ACCENT: u32 = 0x4c8dff;
const ACCENT_HOVER: u32 = 0x6aa1ff;
const CHIP_BG: u32 = 0x22344f;
const CHIP_TEXT: u32 = 0x8ab4ff;
const TEXT: u32 = 0xe6e8eb;
const MUTED: u32 = 0x9aa1ab;
const FAINT: u32 = 0x6b727d;
const SUCCESS: u32 = 0x6fcf97;
const WARNING: u32 = 0xf2c94c;
const ERROR: u32 = 0xeb5757;
const SIDEBAR_WIDTH: f32 = 280.0;
const MONO: &str = "Consolas";

/// What the response pane shows for a Request: a send in flight, the answer, or the reason it failed.
enum ResponseView {
    Sending,
    Done(Box<SentResponse>),
    Failed(String),
}

/// A response with its body already formatted for display, and the placeholders that had no value when it was sent.
struct SentResponse {
    response: ApiResponse,
    body: String,
    is_json: bool,
    unresolved: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResponseTab {
    Body,
    Headers,
}

/// The section of the Request on show under the URL bar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestTab {
    Params,
    Headers,
    Auth,
    Body,
    Checks,
}

fn status_color(status_code: u16) -> u32 {
    match classify(i32::from(status_code)) {
        ResponseStatusClass::Success => SUCCESS,
        ResponseStatusClass::Redirect => 0x56b6f2,
        ResponseStatusClass::ClientError => 0xf2994a,
        ResponseStatusClass::ServerError => ERROR,
        ResponseStatusClass::Neutral => MUTED,
    }
}

fn method_color(method: &str) -> u32 {
    match method {
        "GET" => SUCCESS,
        "POST" => WARNING,
        "PUT" => 0x56b6f2,
        "PATCH" => 0xbb87f5,
        "DELETE" => ERROR,
        _ => MUTED,
    }
}

fn span_color(kind: SpanKind) -> u32 {
    match kind {
        SpanKind::Key => CHIP_TEXT,
        SpanKind::String => SUCCESS,
        SpanKind::Number => WARNING,
        SpanKind::Literal => 0xbb87f5,
        SpanKind::Plain => TEXT,
    }
}

/// The auth kind in words. Credentials are never shown, only the kind.
fn auth_label(request: &ApiRequest) -> &'static str {
    match request.auth.as_ref().map(|auth| auth.kind.as_str()) {
        None | Some(auth_kinds::NONE) => "No auth",
        Some(auth_kinds::BEARER) => "Bearer token. The token is hidden.",
        Some(auth_kinds::BASIC) => "Basic. The password is hidden.",
        Some(auth_kinds::API_KEY) => "API key. The value is hidden.",
        Some(auth_kinds::DIGEST) => "Digest. The password is hidden.",
        Some(auth_kinds::OAUTH1) => "OAuth 1.0. The secrets are hidden.",
        Some(auth_kinds::AWS_SIG_V4) => "AWS Signature v4. The secrets are hidden.",
        Some(_) => "An auth kind this build does not know.",
    }
}

fn muted_note(text: impl Into<SharedString>) -> gpui::Div {
    div().text_color(rgb(MUTED)).child(text.into())
}

/// A two-column row of monospace text, as used for headers, parameters and checks.
fn pair_row(key: String, value: String, dimmed: bool) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .gap_4()
        .font_family(MONO)
        .child(div().w(px(220.0)).flex_none().overflow_hidden().whitespace_nowrap().text_color(rgb(if dimmed { FAINT } else { TEXT })).child(key))
        .child(div().flex_1().overflow_hidden().text_color(rgb(if dimmed { FAINT } else { MUTED })).child(value))
}

/// The text fields of the Request on show. They are rebuilt when another Request is picked or a header is added or
/// removed, so each field's index always matches the Request.
struct RequestInputs {
    item_id: String,
    url: Entity<TextInput>,
    /// The Request's name.
    name: Entity<TextInput>,
    /// The Request's body, as a multi-line field.
    body: Entity<TextArea>,
    /// The username of Basic auth, and the name of an API key's parameter. Neither is a Secret.
    auth_username: Entity<TextInput>,
    auth_key_name: Entity<TextInput>,
    /// Masked fields for the auth Secrets. None when this system has no vault, or the vault could not be read.
    auth_token: Option<Entity<TextInput>>,
    auth_password: Option<Entity<TextInput>>,
    auth_key_value: Option<Entity<TextInput>>,
    /// One (name, value) pair per header, in the Request's order.
    headers: Vec<(Entity<TextInput>, Entity<TextInput>)>,
    /// One (path, expected) pair per check, in the Request's order.
    checks: Vec<(Entity<TextInput>, Entity<TextInput>)>,
    _subscriptions: Vec<Subscription>,
}

struct Shell {
    model: ShellModel,
    /// The store that read the folder. A save goes through it, so only files it understood are changed.
    store: Option<WorkspaceFolder>,
    inputs: Option<RequestInputs>,
    /// The outcome of the last save, shown in the status bar. The flag is true for a problem.
    notice: Option<(String, bool)>,
    /// The Secrets vault for this system, the Secrets read from it, and why it could not be used, if it could not.
    vault: VaultState,
    /// The sidebar's search field, and the text it holds. The tree shows only matches while the text is not empty.
    search: Entity<TextInput>,
    search_text: String,
    _search_subscription: Subscription,
    focus_handle: FocusHandle,
    state: AppState,
    /// The latest response of each Request sent in this window, by Request id. Kept in memory only.
    responses: HashMap<String, ResponseView>,
    recent: RecentSends,
    request_tab: RequestTab,
    response_tab: ResponseTab,
    /// The Workspace's `.env` Variables, read at start and again before each send.
    dot_env: Vec<KeyValue>,
}

impl Shell {
    fn new(model: ShellModel, store: Option<WorkspaceFolder>, vault: VaultState, cx: &mut Context<Self>) -> Self {
        let state = AppState::for_workspace(&model.workspace);
        let dot_env = read_dot_env(&model);
        let search = cx.new(|cx| TextInput::new("", "Search requests", cx));
        let search_subscription = cx.subscribe(&search, |this: &mut Shell, input, _: &Changed, cx| {
            this.search_text = input.read(cx).text().to_string();
            cx.notify();
        });
        Self {
            model,
            store,
            inputs: None,
            notice: None,
            vault,
            search,
            search_text: String::new(),
            _search_subscription: search_subscription,
            focus_handle: cx.focus_handle(),
            state,
            responses: HashMap::new(),
            recent: RecentSends::default(),
            request_tab: RequestTab::Headers,
            response_tab: ResponseTab::Body,
            dot_env,
        }
    }

    fn selected_request(&self) -> Option<&ApiRequest> {
        self.state.selected_item().and_then(|id| find_request(&self.model.workspace, id))
    }

    fn selected_response(&self) -> Option<&ResponseView> {
        self.state.selected_item().and_then(|id| self.responses.get(id))
    }

    fn scope_for(&self, request_id: &str) -> Vec<KeyValue> {
        variables_in_scope(&self.model.workspace, request_id, self.state.active_environment_id(), &self.dot_env)
    }

    fn environment_name(&self) -> String {
        self.state
            .active_environment_id()
            .and_then(|id| self.model.workspace.environments.iter().find(|environment| environment.id == id))
            .map(|environment| environment.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| NO_ENVIRONMENT.to_string())
    }

    /// Makes the text fields for the Request on show, when they are not already the ones for it.
    fn ensure_inputs(&mut self, cx: &mut Context<Self>) {
        let selected = self.state.selected_item().filter(|id| find_request(&self.model.workspace, id).is_some());
        if self.inputs.as_ref().map(|inputs| inputs.item_id.as_str()) == selected {
            return;
        }
        let Some(item_id) = selected.map(str::to_string) else {
            self.inputs = None;
            return;
        };
        let Some(request) = find_request(&self.model.workspace, &item_id).cloned() else {
            return;
        };
        let mut subscriptions = Vec::new();
        let url = Self::make_input(Field::Url, request.url.clone(), "Enter a URL", &mut subscriptions, cx);
        let name = cx.new(|cx| TextInput::new(request.name.clone(), "Request name", cx));
        subscriptions.push(cx.subscribe(&name, |this: &mut Shell, input, _: &Changed, cx| {
            let text = input.read(cx).text().to_string();
            let Some(item_id) = this.state.selected_item().map(str::to_string) else {
                return;
            };
            if edit::rename_request(&mut this.model.workspace, &item_id, &text) {
                this.state.mark_unsaved(&item_id);
                this.notice = None;
                cx.notify();
            }
        }));
        let headers = request
            .headers
            .iter()
            .enumerate()
            .map(|(index, header)| {
                let value = if header.is_secret { String::new() } else { header.value.clone() };
                (
                    Self::make_input(Field::HeaderKey(index), header.key.clone(), "Name", &mut subscriptions, cx),
                    Self::make_input(Field::HeaderValue(index), value, "Value", &mut subscriptions, cx),
                )
            })
            .collect();
        let (auth_username_text, auth_key_name_text) = request
            .auth
            .as_ref()
            .map(|auth| (auth.username.clone(), auth.key_name.clone()))
            .unwrap_or_default();
        let auth_username = Self::make_input(Field::AuthUsername, auth_username_text, "Username", &mut subscriptions, cx);
        let auth_key_name = Self::make_input(Field::AuthKeyName, auth_key_name_text, "Parameter name", &mut subscriptions, cx);
        let checks = request
            .assertions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(index, check)| {
                (
                    Self::make_input(Field::CheckPath(index), check.path.clone(), "Path", &mut subscriptions, cx),
                    Self::make_input(Field::CheckExpected(index), check.expected.clone(), "Expected", &mut subscriptions, cx),
                )
            })
            .collect();
        let auth_token = self.make_secret_input(auth_key(&request.id, "token"), "Token", &mut subscriptions, cx);
        let auth_password = self.make_secret_input(auth_key(&request.id, "password"), "Password", &mut subscriptions, cx);
        let auth_key_value = self.make_secret_input(auth_key(&request.id, "keyValue"), "Value", &mut subscriptions, cx);
        let body = cx.new(|cx| TextArea::new(request.body.clone(), "Request body", cx));
        subscriptions.push(cx.subscribe(&body, |this: &mut Shell, input, _: &Changed, cx| {
            let text = input.read(cx).text().to_string();
            this.change_request(cx, |request| {
                let changed = request.body != text;
                request.body = text;
                changed
            });
        }));
        self.inputs = Some(RequestInputs {
            item_id,
            url,
            name,
            body,
            auth_username,
            auth_key_name,
            auth_token,
            auth_password,
            auth_key_value,
            headers,
            checks,
            _subscriptions: subscriptions,
        });
    }

    fn make_input(
        field: Field,
        text: String,
        placeholder: &'static str,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let input = cx.new(|cx| TextInput::new(text, placeholder, cx));
        subscriptions.push(cx.subscribe(&input, move |this: &mut Shell, input, _: &Changed, cx| {
            let text = input.read(cx).text().to_string();
            this.change_request(cx, |request| edit::set_field(request, field, &text));
        }));
        input
    }

    /// Applies a change to the Request on show. When it changed anything, the Request is marked unsaved.
    fn change_request(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ApiRequest) -> bool) {
        let Some(item_id) = self.state.selected_item().map(str::to_string) else {
            return;
        };
        let Some(request) = edit::find_request_mut(&mut self.model.workspace, &item_id) else {
            return;
        };
        if change(request) {
            self.state.mark_unsaved(&item_id);
            self.notice = None;
            cx.notify();
        }
    }

    /// Writes the Workspace through the store that read it. Nothing is written when the folder changed on disk since
    /// it was read, because the window does not hold those changes and a save could undo them.
    fn save_workspace(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        let Some(store) = self.store.as_mut() else {
            self.notice = Some(("No workspace folder is open, so there is nowhere to save.".to_string(), true));
            return;
        };
        if !self.state.has_unsaved() {
            self.notice = Some(("Nothing to save.".to_string(), false));
            return;
        }
        // A vault that could not be read is never written over. The Workspace still saves, and the unsaved marks stay,
        // so the secrets are not lost from view and can be saved again.
        let mut secrets_note = None;
        if let Some(problem) = self.vault.problem.clone() {
            secrets_note = Some(format!("The secrets were not saved. {problem}"));
        } else if let Some(vault) = self.vault.vault.as_ref() {
            if let Err(error) = vault.save(&self.vault.secrets) {
                secrets_note = Some(format!("The secrets were not saved. {}", error.message()));
            }
        }
        if store.has_outside_changes() {
            let text = "The folder changed on disk since it was opened, so nothing was saved.";
            self.notice = Some((text.to_string(), true));
            return;
        }
        self.notice = Some(match store.save(&self.model.workspace) {
            Ok(left_alone) => {
                if secrets_note.is_none() {
                    self.state.clear_unsaved();
                }
                match (secrets_note, left_alone.is_empty()) {
                    (Some(note), _) => (format!("The workspace was saved. {note}"), true),
                    (None, true) => ("Saved.".to_string(), false),
                    (None, false) => {
                        (format!("Saved. {} file(s) could not be read and were left alone.", left_alone.len()), true)
                    }
                }
            }
            Err(error) => (format!("The save failed: {error}"), true),
        });
    }

    /// A masked field for one Secret, holding its current value. None when no vault can hold it.
    fn make_secret_input(
        &self,
        key: String,
        placeholder: &'static str,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TextInput>> {
        if self.vault.vault.is_none() || self.vault.problem.is_some() {
            return None;
        }
        let current = self.vault.secrets.get(&key).map(|value| value.expose().to_string()).unwrap_or_default();
        let input = cx.new(|cx| TextInput::new_masked(current, placeholder, cx));
        subscriptions.push(cx.subscribe(&input, move |this: &mut Shell, input, _: &Changed, cx| {
            let text = input.read(cx).text().to_string();
            this.set_secret_text(cx, &key, &text);
        }));
        Some(input)
    }

    /// Stores a typed Secret. It is saved with the next save, and the item is marked unsaved.
    fn set_secret_text(&mut self, cx: &mut Context<Self>, key: &str, text: &str) {
        if set_secret(&mut self.vault.secrets, key, text) {
            if let Some(item_id) = self.state.selected_item().map(str::to_string) {
                self.state.mark_unsaved(&item_id);
            }
            self.notice = None;
            cx.notify();
        }
    }

    /// A copy of the Request with its auth Secrets filled in, for sending. Returns whether a Secret the auth kind
    /// needs is not stored. The stored Request is never changed.
    fn with_secrets(&self, mut request: ApiRequest) -> (ApiRequest, bool) {
        let id = request.id.clone();
        let lookup = |field: &str| self.vault.secrets.get(&auth_key(&id, field)).map(|value| value.expose().to_string());
        let mut missing = false;
        if let Some(auth) = request.auth.as_mut() {
            let (field, target) = match auth.kind.as_str() {
                ricochet_core::models::auth_kinds::BEARER => ("token", &mut auth.token),
                ricochet_core::models::auth_kinds::BASIC => ("password", &mut auth.password),
                ricochet_core::models::auth_kinds::API_KEY => ("keyValue", &mut auth.key_value),
                _ => return (request, false),
            };
            match lookup(field) {
                Some(value) => *target = value,
                None => missing = true,
            }
        }
        (request, missing)
    }

    /// Sends the selected Request off the UI thread, with its Variables resolved the way the C# app layers them.
    /// A placeholder with no value is sent as written, and the response pane names it.
    fn send_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.state.selected_item().map(str::to_string) else {
            return;
        };
        if matches!(self.responses.get(&id), Some(ResponseView::Sending)) {
            return;
        }
        self.dot_env = read_dot_env(&self.model);
        let Some(request) = find_request(&self.model.workspace, &id) else {
            return;
        };
        let (name, method) = (request.name.clone(), request.method.clone());
        let (sent, missing_secret) = self.with_secrets(variables::apply(request, &self.scope_for(&id)));
        if self.vault.problem.is_some() || missing_secret {
            self.notice = Some((
                "A secret this request needs is not stored, so it was sent without it.".to_string(),
                true,
            ));
        }
        let unresolved = variables::find_placeholders(&sent);

        self.responses.insert(id.clone(), ResponseView::Sending);
        let work = cx.background_executor().spawn(async move {
            RequestSender::new().send(&sent).map_err(|error| error.message().to_string()).map(|response| {
                let formatted = response_format::format(&response);
                let mut body: String = formatted.text.chars().take(DISPLAY_CHARS).collect();
                if body.len() < formatted.text.len() {
                    body.push_str("\n… (body cut for display)");
                }
                Box::new(SentResponse { response, body, is_json: formatted.is_json, unresolved })
            })
        });
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            this.update(cx, |shell, cx| {
                let (status_code, elapsed_ms) = match &outcome {
                    Ok(sent) => (Some(sent.response.status_code), sent.response.elapsed.as_millis()),
                    Err(_) => (None, 0),
                };
                shell.recent.record(RecentSend { request_id: id.clone(), name, method, status_code, elapsed_ms });
                let view = match outcome {
                    Ok(sent) => ResponseView::Done(sent),
                    Err(message) => ResponseView::Failed(message),
                };
                shell.responses.insert(id, view);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let has_environments = !self.model.workspace.environments.is_empty();
        let is_set = self.state.active_environment_id().is_some();
        let unsaved = self.state.unsaved_count();
        div()
            .h(px(40.0))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .bg(rgb(CHROME_BG))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(div().flex_1())
            .child(
                div()
                    .id("save-workspace")
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if unsaved > 0 { ACCENT } else { BORDER }))
                    .text_color(rgb(if unsaved > 0 { TEXT } else { FAINT }))
                    .when(unsaved > 0, |style| style.cursor_pointer().hover(|style| style.bg(rgb(ROW_HOVER))))
                    .child(if unsaved > 0 { format!("Save ({unsaved})") } else { "Saved".to_string() })
                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.save_workspace(cx);
                    })),
            )
            .child(div().text_xs().text_color(rgb(FAINT)).child("Environment"))
            .child(
                div()
                    .id("environment-picker")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_color(rgb(if is_set { SUCCESS } else { MUTED }))
                    .when(has_environments, |style| style.cursor_pointer().hover(|style| style.bg(rgb(ROW_HOVER))))
                    .child(self.environment_name())
                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        let workspace = &this.model.workspace;
                        this.state.cycle_environment(workspace);
                        cx.notify();
                    })),
            )
    }

    fn tree_row(&self, row: &TreeRow, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_selected = self.state.selected_item() == Some(row.id.as_str());
        let is_request = row.kind == RowKind::Request;
        let id = row.id.clone();
        let indent = px(6.0 + 14.0 * row.depth as f32);
        let label_weight = if row.kind == RowKind::Collection { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
        let text_color = if row.hidden {
            FAINT
        } else if row.kind == RowKind::Folder {
            MUTED
        } else {
            TEXT
        };

        let lead = match &row.method {
            Some(method) => div()
                .w(px(40.0))
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(method_color(method)))
                .child(method.clone()),
            None => div()
                .w(px(12.0))
                .flex_none()
                .text_xs()
                .text_color(rgb(FAINT))
                .child(if self.state.is_collapsed(&row.id) { "▸" } else { "▾" }),
        };

        div()
            .id(SharedString::from(format!("row-{}", row.id)))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(26.0))
            .pl(indent)
            .pr_2()
            .rounded_sm()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(ROW_HOVER)))
            .when(is_selected, |style| style.bg(rgb(ROW_SELECTED)))
            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                if is_request {
                    this.state.pick_request(&id);
                } else {
                    this.state.toggle_collapsed(&id);
                }
                cx.notify();
            }))
            .child(lead)
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(rgb(text_color))
                    .font_weight(label_weight)
                    .child(row.label.clone()),
            )
    }

    fn recent_row(&self, index: usize, send: &RecentSend, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = send.request_id.clone();
        let (status, color) = match send.status_code {
            Some(code) => (code.to_string(), status_color(code)),
            None => ("ERR".to_string(), ERROR),
        };
        div()
            .id(SharedString::from(format!("recent-{index}")))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(22.0))
            .px_2()
            .rounded_sm()
            .text_xs()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(ROW_HOVER)))
            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                if find_request(&this.model.workspace, &id).is_some() {
                    this.state.pick_request(&id);
                    cx.notify();
                }
            }))
            .child(div().w(px(28.0)).flex_none().font_weight(FontWeight::SEMIBOLD).text_color(rgb(color)).child(status))
            .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_color(rgb(MUTED)).child(send.name.clone()))
            .child(div().flex_none().text_color(rgb(FAINT)).child(format!("{} ms", send.elapsed_ms)))
    }

    fn section_label(text: &'static str) -> gpui::Div {
        div().px_2().text_xs().text_color(rgb(FAINT)).child(text)
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = &self.model;
        let tree = build_tree_folded(&model.workspace, false, self.state.collapsed());
        let shown = filter_rows(&tree, &self.search_text);
        let rows: Vec<_> = shown.iter().map(|row| self.tree_row(row, cx)).collect();

        let mut column = div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(rgb(CHROME_BG))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(Self::section_label("Collections").pt_1())
            .child(Self::input_cell(&self.search).flex_none())
            .child(div().id("collection-tree").flex_1().flex().flex_col().overflow_y_scroll().children(rows));

        if tree.is_empty() && model.error.is_none() {
            column = column.child(muted_note("This workspace has no requests yet.").px_2());
        } else if shown.is_empty() {
            column = column.child(muted_note("No request matches the search.").px_2());
        }
        if !model.issues.is_empty() {
            let text = format!("{} file(s) could not be read and were left alone.", model.issues.len());
            column = column.child(div().px_2().text_xs().text_color(rgb(WARNING)).child(text));
        }

        let recent = self.recent.entries();
        if !recent.is_empty() {
            let rows: Vec<_> = recent.iter().enumerate().map(|(index, send)| self.recent_row(index, send, cx)).collect();
            column = column
                .child(div().h(px(1.0)).flex_none().bg(rgb(BORDER)))
                .child(Self::section_label("Recent sends").pt_1())
                .child(div().id("recent-sends").max_h(px(200.0)).flex().flex_col().overflow_y_scroll().children(rows));
        }
        column
    }

    fn request_name(&self, id: &str) -> String {
        find_request(&self.model.workspace, id).map(|request| request.name.clone()).unwrap_or_else(|| id.to_string())
    }

    /// One tab: its Request's name, which shows it, and a close mark. The label and the mark are separate clicks.
    fn tab(&self, id: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_active = self.state.active_tab() == Some(id);
        let label = self.request_name(id);
        let show_id = id.to_string();
        let close_id = id.to_string();
        div()
            .id(SharedString::from(format!("tab-{id}")))
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap_2()
            .h_full()
            .px_3()
            .border_b_2()
            .border_color(rgb(if is_active { ACCENT } else { CHROME_BG }))
            .when(is_active, |style| style.bg(rgb(WINDOW_BG)))
            .child(
                div()
                    .id(SharedString::from(format!("tab-label-{id}")))
                    .cursor_pointer()
                    .whitespace_nowrap()
                    .text_color(rgb(if is_active { TEXT } else { MUTED }))
                    .child(if self.state.is_unsaved(id) { format!("{label} •") } else { label })
                    .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.state.activate_tab(&show_id);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id(SharedString::from(format!("tab-close-{id}")))
                    .cursor_pointer()
                    .text_color(rgb(FAINT))
                    .hover(|style| style.text_color(rgb(TEXT)))
                    .child("×")
                    .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.state.close_tab(&close_id);
                        cx.notify();
                    })),
            )
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tabs: Vec<_> = self.state.open_tabs().iter().map(|id| self.tab(id, cx)).collect();
        div()
            .id("tab-strip")
            .h(px(34.0))
            .flex_none()
            .flex()
            .flex_row()
            .bg(rgb(CHROME_BG))
            .border_b_1()
            .border_color(rgb(BORDER))
            .overflow_x_scroll()
            .children(tabs)
    }

    /// The method, the URL and the Send button, in one control. The URL shows its placeholders as chips until it is
    /// clicked, and is then a text field showing the text as written.
    fn url_bar(&self, request: &ApiRequest, scope: &[KeyValue], url_focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sending = matches!(self.selected_response(), Some(ResponseView::Sending));
        let url_area = div().id("url-field").flex_1().h_full().flex().flex_row().items_center().px_3().overflow_hidden().font_family(MONO);
        let url_area = match self.inputs.as_ref().filter(|_| url_focused) {
            Some(inputs) => url_area.child(inputs.url.clone()),
            None => {
                let chips = url_area.whitespace_nowrap().cursor_text().on_click(cx.listener(
                    |this: &mut Shell, _: &ClickEvent, window: &mut Window, cx: &mut Context<Shell>| {
                        if let Some(inputs) = &this.inputs {
                            window.focus(&inputs.url.focus_handle(cx));
                            inputs.url.update(cx, |input, cx| input.move_to_end(cx));
                            cx.notify();
                        }
                    },
                ));
                if request.url.is_empty() {
                    chips.child(div().text_color(rgb(FAINT)).child("Enter a URL"))
                } else {
                    chips.children(url_segments(&request.url, scope).into_iter().map(|segment| match segment {
                        UrlSegment::Text(text) => div().flex_none().child(text),
                        UrlSegment::Chip { name, resolves } => div()
                            .flex_none()
                            .px_1()
                            .rounded_sm()
                            .bg(rgb(if resolves { CHIP_BG } else { 0x4a3a1a }))
                            .text_color(rgb(if resolves { CHIP_TEXT } else { WARNING }))
                            .child(name),
                    }))
                }
            }
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(36.0))
            .flex_none()
            .rounded_md()
            .border_1()
            .border_color(rgb(if url_focused { ACCENT } else { BORDER }))
            .bg(rgb(FIELD_BG))
            .overflow_hidden()
            .child(
                div()
                    .id("method")
                    .h_full()
                    .flex()
                    .flex_none()
                    .items_center()
                    .px_3()
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(method_color(&request.method)))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(ROW_HOVER)))
                    .child(request.method.clone())
                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.change_request(cx, |request| {
                            edit::cycle_method(request);
                            true
                        });
                    })),
            )
            .child(url_area)
            .child(
                div()
                    .id("send-request")
                    .h_full()
                    .flex()
                    .flex_none()
                    .items_center()
                    .px_4()
                    .bg(rgb(ACCENT))
                    .text_color(rgb(0xffffff))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(ACCENT_HOVER)))
                    .child(if sending { "Sending…" } else { "Send" })
                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.send_selected(cx);
                        cx.notify();
                    })),
            )
    }

    fn input_cell(input: &Entity<TextInput>) -> gpui::Div {
        div().px_2().py_1().rounded_sm().border_1().border_color(rgb(BORDER)).bg(rgb(FIELD_BG)).font_family(MONO).overflow_hidden().child(input.clone())
    }

    /// The headers as editable rows: an on/off box, the name, the value and a remove mark, then a row to add one.
    /// A Secret header's value is masked and cannot be typed, because Secrets are not ported yet (ADR 0092).
    fn headers_editor(&self, request: &ApiRequest, cx: &mut Context<Self>) -> gpui::Div {
        let mut section = div().flex().flex_col().gap_1();
        let inputs = self.inputs.as_ref().map(|inputs| inputs.headers.as_slice()).unwrap_or(&[]);
        for (index, (header, (key_input, value_input))) in request.headers.iter().zip(inputs).enumerate() {
            let value_cell = if header.is_secret {
                div().px_2().py_1().font_family(MONO).text_color(rgb(FAINT)).child("•••••• (a Secret, not editable here)")
            } else {
                Self::input_cell(value_input)
            };
            section = section.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("header-toggle-{index}")))
                            .w(px(18.0))
                            .h(px(18.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(if header.enabled { ACCENT } else { BORDER }))
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .cursor_pointer()
                            .child(if header.enabled { "✓" } else { "" })
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::toggle_header(request, index));
                            })),
                    )
                    .child(Self::input_cell(key_input).w(px(240.0)).flex_none())
                    .child(value_cell.flex_1())
                    .child(
                        div()
                            .id(SharedString::from(format!("header-remove-{index}")))
                            .px_2()
                            .flex_none()
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .hover(|style| style.text_color(rgb(ERROR)))
                            .child("×")
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::remove_header(request, index));
                                this.inputs = None;
                            })),
                    ),
            );
        }
        section.child(
            div().flex().flex_row().child(
                div()
                    .id("header-add")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(TEXT)).bg(rgb(ROW_HOVER)))
                    .child("+ Add header")
                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.change_request(cx, |request| {
                            edit::add_header(request);
                            true
                        });
                        this.inputs = None;
                    })),
            ),
        )
    }

    fn request_tab_button(&self, tab: RequestTab, label: String, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_active = self.request_tab == tab;
        div()
            .id(SharedString::from(format!("request-tab-{label}")))
            .pb_1()
            .cursor_pointer()
            .border_b_2()
            .border_color(rgb(if is_active { ACCENT } else { WINDOW_BG }))
            .text_color(rgb(if is_active { TEXT } else { MUTED }))
            .hover(|style| style.text_color(rgb(TEXT)))
            .child(label)
            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                this.request_tab = tab;
                cx.notify();
            }))
    }

    /// A value that is a Secret: shown masked. Read-only when there is no vault to type it into.
    fn secret_row(label: &'static str, is_set: bool) -> gpui::Div {
        let value = if is_set { "•••••• (kept in the secrets vault)" } else { "Not set" };
        pair_row(label.to_string(), value.to_string(), !is_set)
    }

    /// A Secret's masked field when there is one, else its read-only row.
    fn secret_field(label: &'static str, input: Option<&Entity<TextInput>>, is_set: bool) -> gpui::Div {
        match input {
            Some(input) => div()
                .flex()
                .flex_row()
                .items_center()
                .gap_4()
                .child(div().w(px(220.0)).flex_none().font_family(MONO).text_color(rgb(TEXT)).child(label))
                .child(Self::input_cell(input).flex_1()),
            None => Self::secret_row(label, is_set),
        }
    }

    /// The auth kind, and the fields that kind uses. Secrets are masked; the others can be edited.
    fn auth_editor(&self, request: &ApiRequest, cx: &mut Context<Self>) -> gpui::Div {
        let auth = request.auth.clone().unwrap_or_default();
        let inputs = self.inputs.as_ref();
        let kind_label = match auth.kind.as_str() {
            auth_kinds::NONE => "No auth",
            auth_kinds::BEARER => "Bearer token",
            auth_kinds::BASIC => "Basic",
            auth_kinds::API_KEY => "API key",
            _ => "Other (not editable here)",
        };
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(div().text_xs().text_color(rgb(FAINT)).child("Type"))
                    .child(
                        div()
                            .id("auth-kind")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(ROW_HOVER)))
                            .child(kind_label)
                            .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| {
                                    edit::cycle_auth_kind(request);
                                    true
                                });
                            })),
                    ),
            )
            .child(muted_note(auth_label(request)));

        match (auth.kind.as_str(), inputs) {
            (auth_kinds::BEARER, inputs) => {
                section = section.child(Self::secret_field(
                    "Token",
                    inputs.and_then(|inputs| inputs.auth_token.as_ref()),
                    !auth.token.is_empty(),
                ));
            }
            (auth_kinds::BASIC, Some(inputs)) => {
                section = section
                    .child(pair_row("Username".to_string(), String::new(), false).child(Self::input_cell(&inputs.auth_username).flex_1()))
                    .child(Self::secret_field("Password", inputs.auth_password.as_ref(), !auth.password.is_empty()));
            }
            (auth_kinds::API_KEY, Some(inputs)) => {
                let placement = if auth.key_in_query { "Query string" } else { "Header" };
                section = section
                    .child(div().flex().flex_row().gap_3().child(Self::input_cell(&inputs.auth_key_name).flex_1()))
                    .child(Self::secret_field("Value", inputs.auth_key_value.as_ref(), !auth.key_value.is_empty()))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(div().text_xs().text_color(rgb(FAINT)).child("Sent in"))
                            .child(
                                div()
                                    .id("auth-key-placement")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(ROW_HOVER)))
                                    .child(placement)
                                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                        this.change_request(cx, |request| {
                                            edit::toggle_key_in_query(request);
                                            true
                                        });
                                    })),
                            ),
                    );
            }
            _ => {}
        }
        section
    }

    /// A small button that steps a value or flips a flag, named by its id.
    fn small_button(id: SharedString, label: String) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(rgb(BORDER))
            .text_xs()
            .text_color(rgb(MUTED))
            .cursor_pointer()
            .hover(|style| style.text_color(rgb(TEXT)).bg(rgb(ROW_HOVER)))
            .child(label)
    }

    /// The checks as editable rows: on/off, where the value comes from, the path, the comparison, what it is
    /// compared with, and a remove mark, then a row to add one.
    fn checks_editor(&self, request: &ApiRequest, cx: &mut Context<Self>) -> gpui::Div {
        let mut section = div().flex().flex_col().gap_1();
        let checks = request.assertions.as_deref().unwrap_or(&[]);
        let inputs = self.inputs.as_ref().map(|inputs| inputs.checks.as_slice()).unwrap_or(&[]);
        if checks.is_empty() {
            section = section.child(muted_note("No checks. A check compares part of the response with a value."));
        }
        for (index, check) in checks.iter().enumerate() {
            let inputs = inputs.get(index);
            let path_cell = match inputs {
                Some((path, _)) if matches!(check.source.as_str(), "header" | "json") => {
                    Self::input_cell(path).w(px(180.0)).flex_none()
                }
                _ => div().w(px(180.0)).flex_none().text_xs().text_color(rgb(FAINT)).child("No path for this source"),
            };
            let expected_cell = match inputs {
                Some((_, expected)) => Self::input_cell(expected).flex_1(),
                None => div().flex_1(),
            };
            section = section.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("check-toggle-{index}")))
                            .w(px(18.0))
                            .h(px(18.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(if check.enabled { ACCENT } else { BORDER }))
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .cursor_pointer()
                            .child(if check.enabled { "✓" } else { "" })
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::toggle_check(request, index));
                            })),
                    )
                    .child(
                        Self::small_button(SharedString::from(format!("check-source-{index}")), check.source.clone())
                            .flex_none()
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::cycle_check_source(request, index));
                            })),
                    )
                    .child(path_cell)
                    .child(
                        Self::small_button(SharedString::from(format!("check-op-{index}")), check.op.clone())
                            .flex_none()
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::cycle_check_op(request, index));
                            })),
                    )
                    .child(expected_cell)
                    .child(
                        div()
                            .id(SharedString::from(format!("check-remove-{index}")))
                            .px_2()
                            .flex_none()
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .hover(|style| style.text_color(rgb(ERROR)))
                            .child("×")
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::remove_check(request, index));
                                this.inputs = None;
                            })),
                    ),
            );
        }
        section.child(
            div().flex().flex_row().child(
                div()
                    .id("check-add")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(TEXT)).bg(rgb(ROW_HOVER)))
                    .child("+ Add check")
                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.change_request(cx, |request| {
                            edit::add_check(request);
                            true
                        });
                        this.inputs = None;
                    })),
            ),
        )
    }

    fn request_section(&self, request: &ApiRequest, cx: &mut Context<Self>) -> gpui::Div {
        let section = div().flex().flex_col().gap_1();
        match self.request_tab {
            RequestTab::Params => {
                let params = query_params(&request.url);
                if params.is_empty() {
                    return section.child(muted_note("The URL has no query parameters."));
                }
                section.children(params.into_iter().map(|(key, value)| pair_row(key, value, false)))
            }
            RequestTab::Headers => self.headers_editor(request, cx),
            RequestTab::Auth => self.auth_editor(request, cx),
            RequestTab::Body => match self.inputs.as_ref() {
                Some(inputs) => section.child(
                    div()
                        .id("body-editor")
                        .w_full()
                        .min_h(px(120.0))
                        .max_h(px(260.0))
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(FIELD_BG))
                        .font_family(MONO)
                        .overflow_y_scroll()
                        .child(inputs.body.clone()),
                ),
                None => section.child(muted_note("Pick a request in the sidebar to edit its body.")),
            },
            RequestTab::Checks => self.checks_editor(request, cx),
        }
    }

    fn request_pane(&self, url_focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pane = div().flex_none().flex().flex_col().gap_2().p_3();
        if let Some(error) = &self.model.error {
            return pane.child(div().text_color(rgb(WARNING)).child(error.clone()));
        }
        let Some(request) = self.selected_request() else {
            return pane.child(muted_note("Pick a request in the sidebar to open it."));
        };
        // The scope is found by the sidebar item's id, which is not the Request's own id.
        let scope = self.scope_for(self.state.selected_item().unwrap_or_default());
        let resolved = variables::apply(request, &scope).url;
        let checks = request.assertions.as_ref().map_or(0, Vec::len);
        let enabled_headers = request.headers.iter().filter(|header| header.enabled).count();

        let name_field = match self.inputs.as_ref() {
            Some(inputs) => Self::input_cell(&inputs.name).w_full().text_lg().font_weight(FontWeight::SEMIBOLD),
            None => div().text_lg().child(request.name.clone()),
        };
        let mut pane = pane.child(name_field).child(self.url_bar(request, &scope, url_focused, cx));
        if resolved != request.url {
            pane = pane.child(
                div()
                    .px_1()
                    .text_xs()
                    .font_family(MONO)
                    .text_color(rgb(FAINT))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(format!("→ {resolved}")),
            );
        }
        pane.child(
            div()
                .flex()
                .flex_row()
                .gap_5()
                .pt_1()
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(self.request_tab_button(RequestTab::Params, format!("Params {}", query_params(&request.url).len()), cx))
                .child(self.request_tab_button(RequestTab::Headers, format!("Headers {enabled_headers}"), cx))
                .child(self.request_tab_button(RequestTab::Auth, "Auth".to_string(), cx))
                .child(self.request_tab_button(RequestTab::Body, "Body".to_string(), cx))
                .child(self.request_tab_button(RequestTab::Checks, format!("Checks {checks}"), cx)),
        )
        .child(self.request_section(request, cx).id("request-section").max_h(px(220.0)).overflow_y_scroll().py_1())
    }

    fn response_tab_button(&self, tab: ResponseTab, label: String, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_active = self.response_tab == tab;
        div()
            .id(SharedString::from(format!("response-tab-{label}")))
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor_pointer()
            .text_color(rgb(if is_active { TEXT } else { MUTED }))
            .when(is_active, |style| style.bg(rgb(BORDER)))
            .hover(|style| style.text_color(rgb(TEXT)))
            .child(label)
            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                this.response_tab = tab;
                cx.notify();
            }))
    }

    fn body_lines(sent: &SentResponse) -> Vec<gpui::Div> {
        sent.body
            .lines()
            .take(DISPLAY_LINES)
            .map(|line| {
                if !sent.is_json {
                    return div().child(line.to_string());
                }
                let colored = color_line(line);
                div().flex().flex_row().pl(px(INDENT_STEP * colored.indent as f32)).children(
                    colored.spans.into_iter().map(|span| div().flex_none().text_color(rgb(span_color(span.kind))).child(span.text)),
                )
            })
            .collect()
    }

    fn response_pane(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut pane = div().flex_1().flex().flex_col().gap_2().p_3().border_t_1().border_color(rgb(BORDER)).overflow_hidden();

        let sent = match self.selected_response() {
            None => return pane.child(muted_note("Send the request to see its response here.")),
            Some(ResponseView::Sending) => return pane.child(muted_note("Sending…")),
            Some(ResponseView::Failed(message)) => {
                return pane.child(div().text_color(rgb(ERROR)).child(format!("The request failed: {message}")));
            }
            Some(ResponseView::Done(sent)) => sent,
        };
        let response = &sent.response;
        let copy_text: String = match self.response_tab {
            ResponseTab::Body => sent.body.clone(),
            ResponseTab::Headers => response.headers.iter().map(|header| format!("{}: {}\n", header.key, header.value)).collect(),
        };

        pane = pane.child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .px_2()
                        .rounded_sm()
                        .bg(rgb(BORDER))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(status_color(response.status_code)))
                        .child(format!("{} {}", response.status_code, response.reason_phrase)),
                )
                .child(div().text_xs().text_color(rgb(MUTED)).child(format!("{} ms", response.elapsed.as_millis())))
                .child(div().text_xs().text_color(rgb(MUTED)).child(format!("{} bytes", response.body.len())))
                .child(div().flex_1())
                .child(self.response_tab_button(ResponseTab::Body, "Body".to_string(), cx))
                .child(self.response_tab_button(ResponseTab::Headers, format!("Headers {}", response.headers.len()), cx))
                .child(
                    div()
                        .id("copy-response")
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .cursor_pointer()
                        .hover(|style| style.text_color(rgb(TEXT)).bg(rgb(ROW_HOVER)))
                        .child("Copy")
                        .on_click(cx.listener(move |_: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                        })),
                ),
        );
        if !sent.unresolved.is_empty() {
            let text = format!("Sent with placeholders that had no value: {}", sent.unresolved.join(", "));
            pane = pane.child(div().text_xs().text_color(rgb(WARNING)).child(text));
        }
        if response.truncated {
            pane = pane.child(div().text_xs().text_color(rgb(WARNING)).child("The body was cut at the size limit."));
        }
        if response.certificates_unverified {
            pane = pane.child(div().text_xs().text_color(rgb(WARNING)).child("Certificate checks were off for this send."));
        }

        let content = div().id("response-content").flex_1().flex().flex_col().overflow_y_scroll().font_family(MONO);
        pane.child(match self.response_tab {
            ResponseTab::Body => content.children(Self::body_lines(sent)),
            ResponseTab::Headers => {
                content.gap_1().children(response.headers.iter().map(|header| pair_row(header.key.clone(), header.value.clone(), false)))
            }
        })
    }

    fn status_bar(&self) -> impl IntoElement + use<> {
        let status = &self.model.status;
        let branch = status.branch.clone().unwrap_or_else(|| "no git branch".to_string());
        div()
            .h(px(24.0))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap_5()
            .px_3()
            .bg(rgb(CHROME_BG))
            .border_t_1()
            .border_color(rgb(BORDER))
            .text_xs()
            .text_color(rgb(FAINT))
            .child(format!("Branch: {branch}"))
            .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(status.workspace.clone()))
            .children(self.notice.as_ref().map(|(text, is_problem)| {
                div().flex_none().text_color(rgb(if *is_problem { WARNING } else { SUCCESS })).child(text.clone())
            }))
            .children((self.state.unsaved_count() > 0).then(|| div().flex_none().child(format!("{} unsaved", self.state.unsaved_count()))))
    }
}

/// The Secrets for this run: the vault, the Secrets read from it, and the reason the vault cannot be used, if it cannot.
/// When there is a problem, nothing is saved, so the vault is never written over.
struct VaultState {
    vault: Option<Box<dyn SecretVault>>,
    secrets: BTreeMap<String, SecretValue>,
    problem: Option<String>,
}

/// Opens the vault in the personal folder, and reads the Secrets from it.
fn open_vault() -> VaultState {
    let Some(personal) = default_personal_directory() else {
        return VaultState {
            vault: None,
            secrets: BTreeMap::new(),
            problem: Some("No personal folder is available, so secrets cannot be stored.".to_string()),
        };
    };
    match secrets::default_vault(&personal) {
        None => VaultState { vault: None, secrets: BTreeMap::new(), problem: None },
        Some(vault) => match vault.load() {
            Ok(secrets) => VaultState { vault: Some(vault), secrets, problem: None },
            Err(error) => VaultState { vault: Some(vault), secrets: BTreeMap::new(), problem: Some(error.message().to_string()) },
        },
    }
}

fn read_dot_env(model: &ShellModel) -> Vec<KeyValue> {
    model.folder_path().and_then(|folder| dotenv::read(folder).ok()).map(|result| result.variables).unwrap_or_default()
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_inputs(cx);
        let url_focused = self.inputs.as_ref().is_some_and(|inputs| inputs.url.focus_handle(cx).is_focused(window));
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this: &mut Shell, _: &Save, _: &mut Window, cx: &mut Context<Shell>| this.save_workspace(cx)))
            .on_action(cx.listener(|this: &mut Shell, _: &SendRequest, _: &mut Window, cx: &mut Context<Shell>| {
                this.send_selected(cx);
                cx.notify();
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(WINDOW_BG))
            .text_color(rgb(TEXT))
            .text_sm()
            .child(self.top_bar(cx))
            .child(
                div().flex().flex_row().flex_1().overflow_hidden().child(self.sidebar(cx)).child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(self.tab_strip(cx))
                        .child(self.request_pane(url_focused, cx))
                        .child(self.response_pane(cx)),
                ),
            )
            .child(self.status_bar())
    }
}

fn main() {
    let folder = folder_from(std::env::args().nth(1).as_deref(), std::env::var("RICOCHET_DATA").ok().as_deref());
    let (model, store) = ShellModel::open(folder);
    let vault = open_vault();

    Application::new().run(move |cx: &mut App| {
        text_input::bind_keys(cx);
        text_area::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("secondary-s", Save, None),
            KeyBinding::new("secondary-enter", SendRequest, None),
            KeyBinding::new("enter", SendRequest, Some("TextInput")),
        ]);
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("Ricochet".into()), ..Default::default() }),
                ..Default::default()
            },
            move |window, cx| {
                let shell = cx.new(move |cx| Shell::new(model, store, vault, cx));
                window.focus(&shell.read(cx).focus_handle);
                shell
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
