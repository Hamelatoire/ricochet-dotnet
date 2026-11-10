// The Ricochet window (ADR 0091): a top bar with Save and the Environment, a sidebar of Collections and recent sends, a request
// pane with a URL bar and section tabs, a response pane and a status bar. Reads a Workspace folder from the first
// argument or RICOCHET_DATA. The URL, method and headers can be edited and saved; the body and auth are read-only.

use std::collections::HashMap;

use gpui::{
    App, Application, Bounds, ClickEvent, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight, KeyBinding, Render, SharedString, Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions,
    actions, div, prelude::*, px, rgb, size,
};
use ricochet_app::edit::{self, Field, set_secret};
use ricochet_core::secrets::{self, SecretValue, SecretVault, auth_key};
use std::collections::BTreeMap;
use ricochet_app::json_color::{SpanKind, color_line};
use ricochet_app::request_view::{UrlSegment, query_params, url_segments};
use ricochet_core::history;
use ricochet_core::models::HistoryEntry;
use ricochet_core::personal_state::{self, default_personal_directory};
use chrono::Local;
use std::path::PathBuf;
use ricochet_app::scope::{self, variables_in_scope};
use ricochet_app::shell::{ShellModel, folder_from};
use ricochet_app::state::AppState;
use ricochet_app::status::NO_ENVIRONMENT;
use ricochet_app::tree::{RowKind, TreeRow, build_tree, build_tree_folded, filter_rows, find_request};
use ricochet_app::tree_edit;
use ricochet_app::palette::{self, PaletteAction};
use ricochet_core::shortcut_sheet;
use ricochet_core::models::{ApiRequest, KeyValue, auth_kinds};
use ricochet_core::response_status::{ResponseStatusClass, classify};
use ricochet_core::send::{ApiResponse, RequestSender};
use ricochet_core::response_compare::{self, DiffKind, ResponseInput};
use ricochet_core::response_examples;
use ricochet_core::curl;
use ricochet_core::postman;
use ricochet_core::insomnia;
use ricochet_core::store::WorkspaceFolder;
use ricochet_core::{dotenv, response_format, variables};
use ricochet_app::text_area::{self, TextArea};
use ricochet_app::text_input::{self, Changed, TextInput};

actions!(ricochet, [Save, SendRequest, NewRequest, OpenPalette, ClosePalette, OpenShortcutSheet, CloseTab, NextTab, PreviousTab]);

/// How much of a response body the pane draws. The rest is cut, so a large body cannot stall the window.
const DISPLAY_CHARS: usize = 20_000;
const DISPLAY_LINES: usize = 1_500;
/// How many History entries the sidebar lists. The file keeps up to MAX_HISTORY.
const HISTORY_SHOWN: usize = 40;
/// The confirm marker for clearing the History, kept apart from item ids.
const HISTORY_CONFIRM: &str = "__clear_history__";
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
    /// The Request as it was sent, with its Variables and Secrets filled in, and the Variables it was sent with. A saved
    /// example masks their values.
    sent: ApiRequest,
    scope: Vec<KeyValue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResponseTab {
    Body,
    Headers,
    /// Compares the response with the one sent before it, for the same Request.
    Compare,
    /// The body as it arrived, without pretty-printing.
    Raw,
}

/// The section of the Request on show under the URL bar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestTab {
    Params,
    Headers,
    Auth,
    Body,
    Checks,
    Captures,
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

/// The response as the comparison reads it.
fn input_of(response: &ApiResponse) -> ResponseInput {
    ResponseInput {
        status: response.status_code,
        headers: response.headers.clone(),
        body: response.body.clone(),
        media_type: response.media_type.clone(),
    }
}

/// The side-by-side comparison of the response with the one before it: the status, the headers that changed, and the
/// body rows, with removed, added and changed rows tinted. Sensitive headers show only that they are hidden.
fn compare_view(previous: Option<&ResponseInput>, current: &ResponseInput) -> gpui::Div {
    let Some(previous) = previous else {
        return div().flex().flex_col().child(muted_note("Send this request again to compare its response with the one before."));
    };
    let comparison = response_compare::compare(previous, current, &[]);
    let mut view = div().flex().flex_col().gap_2().font_family(MONO).text_sm();
    if previous.status != current.status {
        view = view.child(div().text_color(rgb(WARNING)).child(format!("Status {} → {}", previous.status, current.status)));
    }
    let changed: Vec<_> = comparison.headers.iter().filter(|header| header.changed()).collect();
    if changed.is_empty() {
        view = view.child(muted_note("No header values changed."));
    }
    for header in changed {
        let before = header.previous.clone().unwrap_or_else(|| "none".to_string());
        let after = header.current.clone().unwrap_or_else(|| "none".to_string());
        view = view.child(div().text_color(rgb(MUTED)).child(format!("{}: {} → {}", header.name, before, after)));
    }
    if let Some(note) = comparison.body_note.clone() {
        return view.child(muted_note(note));
    }
    for line in comparison.body {
        let (before_bg, after_bg) = match line.kind {
            DiffKind::Same => (WINDOW_BG, WINDOW_BG),
            DiffKind::Removed => (0x3d1f24, WINDOW_BG),
            DiffKind::Added => (WINDOW_BG, 0x1f3d28),
            DiffKind::Changed => (0x3d3520, 0x3d3520),
        };
        let number = |value: Option<usize>| value.map(|n| n.to_string()).unwrap_or_default();
        view = view.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .bg(rgb(before_bg))
                        .child(div().w(px(36.0)).flex_none().text_color(rgb(FAINT)).child(number(line.previous_number)))
                        .child(div().flex_1().overflow_hidden().child(line.previous.unwrap_or_default())),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .bg(rgb(after_bg))
                        .child(div().w(px(36.0)).flex_none().text_color(rgb(FAINT)).child(number(line.current_number)))
                        .child(div().flex_1().overflow_hidden().child(line.current.unwrap_or_default())),
                ),
        );
    }
    view
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
    /// One (name, value) pair per header, in the Request's order. A Secret header's value is a masked field, or None
    /// when there is no vault to hold it.
    headers: Vec<(Entity<TextInput>, Option<Entity<TextInput>>)>,
    /// One (path, expected) pair per check, in the Request's order.
    checks: Vec<(Entity<TextInput>, Entity<TextInput>)>,
    /// One (variable, path) pair per capture, in the Request's order.
    captures: Vec<(Entity<TextInput>, Entity<TextInput>)>,
    _subscriptions: Vec<Subscription>,
}

/// The name and value fields of a selected Collection's or Folder's Variables, one pair per variable in order.
struct ContainerInputs {
    item_id: String,
    /// The Collection whose variables an Environment overrides. None when the editor shows no overrides.
    source: Option<String>,
    /// The name field, for an Environment. None for a Collection or Folder.
    name: Option<Entity<TextInput>>,
    /// The tags field, for a Folder: comma separated. None for a Collection, an Environment or a Request.
    tags: Option<Entity<TextInput>>,
    rows: Vec<(Entity<TextInput>, Entity<TextInput>)>,
    /// One (name, default, value field) per Collection variable, for an Environment.
    overrides: Vec<(String, String, Entity<TextInput>)>,
    _subscriptions: Vec<Subscription>,
}

/// The prompt Variables a send needs, each with the field its value is typed in. Open until the send or cancel.
struct PromptDialog {
    request_id: String,
    rows: Vec<(String, Entity<TextInput>)>,
}

/// The command palette while it is open: its filter field and the text in it.
struct PaletteState {
    input: Entity<TextInput>,
    query: String,
    _subscription: Subscription,
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
    /// Whether the Environment menu under the top bar is open.
    environment_menu_open: bool,
    /// Scrolls the body editor, which keeps its caret in view.
    body_scroll: gpui::ScrollHandle,
    /// The sidebar item whose Delete button was clicked once. A second click deletes it.
    confirm_delete: Option<String>,
    /// The sidebar row clicked last, which the sidebar buttons act on. It can be a Folder or Collection, which never
    /// becomes the selected Request, so the Request on show stays put.
    tree_target: Option<String>,
    /// The fields of the selected Collection's or Folder's Variables. None when a Request, or nothing, is selected.
    container_inputs: Option<ContainerInputs>,
    /// The Environment open in the editor, or None. Set from the Environment menu, and cleared when a Request is picked.
    editing_environment: Option<String>,
    /// The prompt Variables waiting for values before a send, or None.
    prompt: Option<PromptDialog>,
    /// The command palette, or None while it is closed.
    palette: Option<PaletteState>,
    /// Whether the keyboard shortcut sheet is open (F1 or Ctrl+/).
    shortcut_sheet_open: bool,
    /// The Collection or Folder being renamed: its id and the field that holds the name. Set by the Rename button.
    renaming: Option<(String, Entity<TextInput>, Subscription)>,
    /// Whether hidden items show in the sidebar, dimmed (ADR 0032).
    show_hidden: bool,
    focus_handle: FocusHandle,
    state: AppState,
    /// The response sent before the latest one, for each Request, for the Compare tab. Kept in memory only.
    previous_responses: HashMap<String, ResponseInput>,
    /// The latest response of each Request sent in this window, by Request id. Kept in memory only.
    responses: HashMap<String, ResponseView>,
    /// Every send, newest first. Kept in the personal folder as history.json (see ricochet_core::history).
    history: Vec<HistoryEntry>,
    /// False when history.json could not be read at start, so it is not written over.
    history_readable: bool,
    /// The personal folder, where history.json is kept. None when there is none.
    personal_dir: Option<PathBuf>,
    request_tab: RequestTab,
    response_tab: ResponseTab,
    /// The Workspace's `.env` Variables, read at start and again before each send.
    dot_env: Vec<KeyValue>,
}

impl Shell {
    fn new(model: ShellModel, store: Option<WorkspaceFolder>, vault: VaultState, cx: &mut Context<Self>) -> Self {
        let mut state = AppState::for_workspace(&model.workspace);
        let dot_env = read_dot_env(&model);
        let personal_dir = default_personal_directory();
        // The Environment chosen last time, if it still exists in the Workspace.
        if let Some(id) = personal_dir.as_deref().and_then(|dir| personal_state::load(dir).state.active_environment_id) {
            if model.workspace.environments.iter().any(|environment| environment.id == id) {
                state.set_active_environment(Some(&id));
            }
        }
        let (history, history_readable) = match personal_dir.as_deref().map(history::load) {
            Some(Ok(entries)) => (entries, true),
            Some(Err(_)) => (Vec::new(), false),
            None => (Vec::new(), true),
        };
        let search = cx.new(|cx| TextInput::new("", "Search requests", cx));
        let search_subscription = cx.subscribe(&search, |this: &mut Shell, input, _: &Changed, cx| {
            this.search_text = input.read(cx).text().to_string();
            cx.notify();
        });
        Self {
            model,
            store,
            inputs: None,
            notice: (!history_readable)
                .then(|| ("Your history could not be read, so it will not be saved this time.".to_string(), true)),
            vault,
            search,
            search_text: String::new(),
            _search_subscription: search_subscription,
            environment_menu_open: false,
            body_scroll: gpui::ScrollHandle::new(),
            confirm_delete: None,
            tree_target: None,
            container_inputs: None,
            editing_environment: None,
            prompt: None,
            palette: None,
            shortcut_sheet_open: false,
            renaming: None,
            show_hidden: false,
            focus_handle: cx.focus_handle(),
            state,
            responses: HashMap::new(),
            previous_responses: HashMap::new(),
            history,
            history_readable,
            personal_dir,
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
        let mut headers = Vec::new();
        for (index, header) in request.headers.iter().enumerate() {
            let key_input = cx.new(|cx| TextInput::new(header.key.clone(), "Name", cx));
            subscriptions.push(cx.subscribe(&key_input, move |this: &mut Shell, input, _: &Changed, cx| {
                let text = input.read(cx).text().to_string();
                this.rename_header(cx, index, &text);
            }));
            let value_input = if header.is_secret {
                self.make_secret_header_input(&request.id, index, &header.key, &mut subscriptions, cx)
            } else {
                Some(Self::make_input(Field::HeaderValue(index), header.value.clone(), "Value", &mut subscriptions, cx))
            };
            headers.push((key_input, value_input));
        }
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
        let captures = request
            .captures
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(index, capture)| {
                (
                    Self::make_input(Field::CaptureName(index), capture.variable.clone(), "Variable", &mut subscriptions, cx),
                    Self::make_input(Field::CapturePath(index), capture.path.clone(), "Path", &mut subscriptions, cx),
                )
            })
            .collect();
        let auth_token = self.make_secret_input(auth_key(&request.id, "token"), "Token", &mut subscriptions, cx);
        let auth_password = self.make_secret_input(auth_key(&request.id, "password"), "Password", &mut subscriptions, cx);
        let auth_key_value = self.make_secret_input(auth_key(&request.id, "keyValue"), "Value", &mut subscriptions, cx);
        let scroll = self.body_scroll.clone();
        let body = cx.new(|cx| TextArea::new(request.body.clone(), "Request body", cx).with_scroll(scroll));
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
            captures,
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
        self.edit_request(cx, |request, _| change(request));
    }

    /// Like [`Self::change_request`], and the change may also touch the Secrets vault. Both are unsaved until the
    /// next save.
    fn edit_request(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ApiRequest, &mut BTreeMap<String, SecretValue>) -> bool,
    ) {
        let Some(item_id) = self.state.selected_item().map(str::to_string) else {
            return;
        };
        let Some(request) = edit::find_request_mut(&mut self.model.workspace, &item_id) else {
            return;
        };
        if change(request, &mut self.vault.secrets) {
            self.state.mark_unsaved(&item_id);
            self.notice = None;
            cx.notify();
        }
    }

    /// Renames a header on show. A Secret header's value moves with it.
    fn rename_header(&mut self, cx: &mut Context<Self>, index: usize, name: &str) {
        self.edit_request(cx, |request, secrets| edit::set_header_name(request, secrets, index, name));
    }

    /// A masked field for a Secret header's value, holding the value stored under the header's name. None when no
    /// vault can hold it.
    fn make_secret_header_input(
        &self,
        request_id: &str,
        index: usize,
        name: &str,
        subscriptions: &mut Vec<Subscription>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TextInput>> {
        if self.vault.vault.is_none() || self.vault.problem.is_some() {
            return None;
        }
        let current = self
            .vault
            .secrets
            .get(&secrets::header_key(request_id, name))
            .map(|value| value.expose().to_string())
            .unwrap_or_default();
        let input = cx.new(|cx| TextInput::new_masked(current, "Value", cx));
        subscriptions.push(cx.subscribe(&input, move |this: &mut Shell, input, _: &Changed, cx| {
            let text = input.read(cx).text().to_string();
            this.edit_request(cx, |request, secrets| edit::set_secret_header(request, secrets, index, &text));
        }));
        Some(input)
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

    /// A copy of the Request with its Secrets filled in, for sending: the Secret headers and the auth Secrets. Returns
    /// whether a Secret the Request needs is not stored. `stored` is the Request as the window holds it, before
    /// Variables were applied to `request`. The stored Request is never changed.
    fn with_secrets(&self, mut request: ApiRequest, stored: &ApiRequest) -> (ApiRequest, bool) {
        let id = request.id.clone();
        let lookup = |key: &str| self.vault.secrets.get(key).map(|value| value.expose().to_string());
        let mut missing = edit::fill_secret_headers(&mut request, stored, lookup);
        if let Some(auth) = request.auth.as_mut() {
            let field_and_target = match auth.kind.as_str() {
                ricochet_core::models::auth_kinds::BEARER => Some(("token", &mut auth.token)),
                ricochet_core::models::auth_kinds::BASIC => Some(("password", &mut auth.password)),
                ricochet_core::models::auth_kinds::API_KEY => Some(("keyValue", &mut auth.key_value)),
                _ => None,
            };
            if let Some((field, target)) = field_and_target {
                match lookup(&auth_key(&id, field)) {
                    Some(value) => *target = value,
                    None => missing = true,
                }
            }
        }
        (request, missing)
    }

    /// Sends the selected Request off the UI thread, with its Variables resolved the way the C# app layers them.
    /// A placeholder with no value is sent as written, and the response pane names it.
    fn send_selected(&mut self, cx: &mut Context<Self>) {
        // While the palette is open, sending runs its first entry.
        if self.palette.is_some() {
            self.run_palette(0, cx);
            return;
        }
        // While the prompt dialog is open, sending means confirming it.
        if self.prompt.is_some() {
            self.confirm_prompt(cx);
            return;
        }
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
        let prompts = variables::find_prompt_variables(request, &self.scope_for(&id));
        if !prompts.is_empty() {
            self.open_prompt(cx, &id, &prompts);
            return;
        }
        self.send_with(cx, &id, &[]);
    }

    /// Sends the Request with the answers to its prompt Variables. The answers come after the scope, so they win.
    fn send_with(&mut self, cx: &mut Context<Self>, id: &str, answers: &[KeyValue]) {
        let id = id.to_string();
        let Some(request) = find_request(&self.model.workspace, &id) else {
            return;
        };
        let mut scope = self.scope_for(&id);
        scope.extend(answers.iter().cloned());
        let history_copy = history::redacted(request);
        let collection_id = scope::collection_of(&self.model.workspace, &id).map(|collection| collection.id.clone());
        let (sent, missing_secret) = self.with_secrets(variables::apply(request, &scope), request);
        if self.vault.problem.is_some() || missing_secret {
            self.notice = Some((
                "A secret this request needs is not stored, so it was sent without it.".to_string(),
                true,
            ));
        }
        let unresolved = variables::find_placeholders(&sent);

        // The response being replaced becomes the one the Compare tab compares with.
        if let Some(ResponseView::Done(old)) = self.responses.get(&id) {
            let snapshot = input_of(&old.response);
            self.previous_responses.insert(id.clone(), snapshot);
        }
        self.responses.insert(id.clone(), ResponseView::Sending);
        let for_example = (sent.clone(), scope.clone());
        let work = cx.background_executor().spawn(async move {
            RequestSender::new().send(&sent).map_err(|error| error.message().to_string()).map(move |response| {
                let formatted = response_format::format(&response);
                let mut body: String = formatted.text.chars().take(DISPLAY_CHARS).collect();
                if body.len() < formatted.text.len() {
                    body.push_str("\n… (body cut for display)");
                }
                Box::new(SentResponse { response, body, is_json: formatted.is_json, unresolved, sent: for_example.0, scope: for_example.1 })
            })
        });
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            this.update(cx, |shell, cx| {
                let entry = HistoryEntry {
                    request: history_copy.clone(),
                    status_code: match &outcome {
                        Ok(sent) => Some(i32::from(sent.response.status_code)),
                        Err(_) => None,
                    },
                    elapsed_ms: match &outcome {
                        Ok(sent) => i64::try_from(sent.response.elapsed.as_millis()).unwrap_or(i64::MAX),
                        Err(_) => 0,
                    },
                    at: Local::now().fixed_offset(),
                    collection_id: collection_id.clone(),
                    certificates_unverified: match &outcome {
                        Ok(sent) if sent.response.certificates_unverified => Some(true),
                        _ => None,
                    },
                };
                history::record(&mut shell.history, entry);
                shell.save_history();
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
                        if !this.model.workspace.environments.is_empty() {
                            this.environment_menu_open = !this.environment_menu_open;
                        }
                        cx.notify();
                    })),
            )
    }

    fn tree_row(&self, row: &TreeRow, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_selected = self.tree_target.as_deref().or(self.state.selected_item()) == Some(row.id.as_str());
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
                this.tree_target = Some(id.clone());
                this.editing_environment = None;
                this.renaming = None;
                this.confirm_delete = None;
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

    fn history_row(&self, index: usize, entry: &HistoryEntry, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = entry.request.id.clone();
        let (status, color) = match entry.status_code {
            Some(code) => (code.to_string(), status_color(u16::try_from(code).unwrap_or(0))),
            None => ("ERR".to_string(), ERROR),
        };
        let when = entry.at.format("%H:%M:%S").to_string();
        div()
            .id(SharedString::from(format!("history-{index}")))
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
                    this.editing_environment = None;
                    this.tree_target = None;
                    cx.notify();
                }
            }))
            .child(div().w(px(28.0)).flex_none().font_weight(FontWeight::SEMIBOLD).text_color(rgb(color)).child(status))
            .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_color(rgb(MUTED)).child(entry.request.name.clone()))
            .child(div().flex_none().text_color(rgb(FAINT)).child(when))
            .child(div().flex_none().text_color(rgb(FAINT)).child(format!("{} ms", entry.elapsed_ms)))
    }

    /// Saves the active Environment in state.json, keeping the file's other settings.
    fn save_environment(&mut self) {
        let Some(dir) = self.personal_dir.clone() else {
            return;
        };
        if let Err(error) = personal_state::save_active_environment(&dir, self.state.active_environment_id()) {
            self.notice = Some((format!("The environment could not be saved: {error}"), true));
        }
    }

    /// Opens the command palette with its filter field focused.
    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("", "Type a command or a request name", cx));
        let subscription = cx.subscribe(&input, |this: &mut Shell, input, _: &Changed, cx| {
            let text = input.read(cx).text().to_string();
            if let Some(palette) = this.palette.as_mut() {
                palette.query = text;
            }
            cx.notify();
        });
        input.focus_handle(cx).focus(window);
        self.palette = Some(PaletteState { input, query: String::new(), _subscription: subscription });
        cx.notify();
    }

    /// Closes whichever overlay is open: the palette, then the shortcut sheet.
    fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette = None;
        self.shortcut_sheet_open = false;
        cx.notify();
    }

    /// The keyboard shortcut sheet: every shortcut, grouped by the area where it works. None while it is closed.
    fn shortcut_sheet_overlay(&self) -> Option<gpui::Stateful<gpui::Div>> {
        if !self.shortcut_sheet_open {
            return None;
        }
        let mac_os = cfg!(target_os = "macos");
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CHROME_BG))
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Keyboard shortcuts"))
            .child(muted_note("Press Escape to close."));
        for (area, entries) in shortcut_sheet::groups(mac_os) {
            panel = panel.child(Self::section_label(shortcut_sheet::title(area)).pt_2());
            for entry in entries {
                panel = panel.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .text_sm()
                        .child(div().w(px(180.0)).flex_none().font_family(MONO).text_color(rgb(TEXT)).child(entry.keys))
                        .child(div().flex_1().text_color(rgb(MUTED)).child(entry.action)),
                );
            }
        }
        Some(
            div()
                .id("shortcut-sheet")
                .absolute()
                .top(px(60.0))
                .left(px(160.0))
                .w(px(640.0))
                .max_h(px(560.0))
                .overflow_y_scroll()
                .child(panel),
        )
    }

    /// Closes the palette and runs the entry at this index of the filtered list. An index with no entry only closes it.
    fn run_palette(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(query) = self.palette.as_ref().map(|palette| palette.query.clone()) else {
            return;
        };
        let shown = palette::filter(&palette::entries(&self.model.workspace), &query);
        self.palette = None;
        let Some(entry) = shown.into_iter().nth(index) else {
            cx.notify();
            return;
        };
        match entry.action {
            PaletteAction::Send => self.send_selected(cx),
            PaletteAction::Save => self.save_workspace(cx),
            PaletteAction::NewRequest => self.add_request(cx),
            PaletteAction::OpenRequest(id) => {
                self.state.pick_request(&id);
                self.editing_environment = None;
                self.tree_target = None;
            }
            PaletteAction::UseEnvironment(id) => {
                self.state.set_active_environment(id.as_deref());
                self.save_environment();
            }
            PaletteAction::CloseAllTabs => {
                self.state.close_all_tabs();
                self.editing_environment = None;
                self.tree_target = None;
            }
            PaletteAction::CloseOtherTabs => self.state.close_other_tabs(),
        }
        cx.notify();
    }

    /// The palette: its filter field over the first ten entries it matches. None while it is closed.
    fn palette_overlay(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let palette = self.palette.as_ref()?;
        let shown = palette::filter(&palette::entries(&self.model.workspace), &palette.query);
        let rows: Vec<_> = shown
            .iter()
            .take(10)
            .enumerate()
            .map(|(index, entry)| {
                div()
                    .id(SharedString::from(format!("palette-{index}")))
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(rgb(TEXT))
                    .hover(|style| style.bg(rgb(ROW_HOVER)))
                    .child(entry.label.clone())
                    .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.run_palette(index, cx)
                    }))
            })
            .collect();
        Some(
            div()
                .id("palette")
                .absolute()
                .top(px(90.0))
                .left(px(360.0))
                .w(px(520.0))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CHROME_BG))
                .child(Self::input_cell(&palette.input))
                .children(rows)
                .children(shown.is_empty().then(|| muted_note("Nothing matches."))),
        )
    }

    /// Saves the latest response of the selected Request as an example in the Request (ADR 0023), with the secrets it
    /// was sent with masked. The Request is saved with the rest of the Workspace.
    fn save_example(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.state.selected_item().map(str::to_string) else {
            return;
        };
        let Some(ResponseView::Done(sent)) = self.responses.get(&id) else {
            return;
        };
        let name = find_request(&self.model.workspace, &id).map(|request| request.name.clone()).unwrap_or_default();
        match response_examples::create(&name, &sent.sent, &sent.scope, &[], &sent.response) {
            Ok(example) => {
                if let Some(request) = edit::find_request_mut(&mut self.model.workspace, &id) {
                    request.examples.get_or_insert_with(Vec::new).push(example);
                    self.state.mark_unsaved(&id);
                    self.notice = Some(("Example added to the request. Save to write it to the file.".to_string(), false));
                }
            }
            Err(error) => self.notice = Some((format!("The example was not saved: {error}"), true)),
        }
        cx.notify();
    }

    /// Writes the selected response's body to a file the user picks, as the bytes arrived. The save dialog is the
    /// platform's own, so the file goes wherever the user chooses.
    fn save_response_file(&mut self, cx: &mut Context<Self>) {
        let (bytes, suggested) = match self.selected_response() {
            Some(ResponseView::Done(sent)) => {
                let json = sent.response.media_type.as_deref().is_some_and(|media| media.contains("json"));
                (sent.response.body.clone(), if json { "response.json" } else { "response.txt" })
            }
            _ => return,
        };
        let directory = std::env::current_dir().unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&directory, Some(suggested));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let outcome = std::fs::write(&path, &bytes);
            this.update(cx, |shell, cx| {
                shell.notice = Some(match outcome {
                    Ok(()) => (format!("Saved the response to {}", path.display()), false),
                    Err(error) => (format!("The response could not be saved: {error}"), true),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Copies the selected Request as a curl command, with its Variables applied. Secret values from the vault are
    /// masked in the command.
    fn copy_as_curl(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.state.selected_item().map(str::to_string) else {
            return;
        };
        let Some(request) = find_request(&self.model.workspace, &id) else {
            return;
        };
        let resolved = variables::apply(request, &self.scope_for(&id));
        let secrets: Vec<String> = self.vault.secrets.values().map(|value| value.expose().to_string()).collect();
        let secret_refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        let command = curl::generate(&resolved, &secret_refs, false);
        cx.write_to_clipboard(ClipboardItem::new_string(command));
        self.notice = Some(("Copied as curl. Secret values are masked.".to_string(), false));
        cx.notify();
    }

    /// Asks for a Postman export and imports it: its Collection, Environment or globals are added to the Workspace,
    /// and the Workspace is saved with the next save. Nothing is written by the import itself.
    fn import_postman(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a Postman export".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            this.update(cx, |shell, cx| match std::fs::read_to_string(&path) {
                Ok(text) => shell.finish_import(&text, &file_name, cx),
                Err(error) => {
                    shell.notice = Some((format!("The file could not be read: {error}"), true));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Imports a Postman export, or else an Insomnia export, into the Workspace.
    fn finish_import(&mut self, text: &str, file_name: &str, cx: &mut Context<Self>) {
        let postman_error = match postman::import(text, file_name) {
            Ok(result) => {
                if let Some(collection) = result.collection {
                    let id = collection.id.clone();
                    self.model.workspace.collections.push(collection);
                    self.state.mark_unsaved(&id);
                }
                if let Some(environment) = result.environment {
                    let id = environment.id.clone();
                    self.model.workspace.environments.push(environment);
                    self.state.mark_unsaved(&id);
                }
                if let Some(globals) = result.globals {
                    self.model.workspace.globals.extend(globals);
                }
                let problems = result.issues.len();
                let text = if problems == 0 {
                    format!("{} Save to keep it.", result.summary)
                } else {
                    format!("{} {problems} item(s) could not be carried over. Save to keep the import.", result.summary)
                };
                self.notice = Some((text, problems > 0));
                cx.notify();
                return;
            }
            Err(error) => error,
        };
        match insomnia::import(text, file_name) {
            Ok(result) => {
                for workspace in result.workspaces {
                    let id = workspace.collection.id.clone();
                    self.model.workspace.collections.push(workspace.collection);
                    self.state.mark_unsaved(&id);
                    for environment in workspace.environments {
                        let id = environment.id.clone();
                        self.model.workspace.environments.push(environment);
                        self.state.mark_unsaved(&id);
                    }
                }
                let problems = result.issues.len();
                let text = if problems == 0 {
                    format!("{} Save to keep it.", result.summary)
                } else {
                    format!("{} {problems} item(s) could not be carried over. Save to keep the import.", result.summary)
                };
                self.notice = Some((text, problems > 0));
            }
            Err(_) => {
                self.notice = Some((format!("The file is not a Postman or Insomnia export. {postman_error}"), true));
            }
        }
        cx.notify();
    }

    /// Opens the dialog for the prompt Variables a send needs, each with its default filled in.
    fn open_prompt(&mut self, cx: &mut Context<Self>, id: &str, prompts: &[variables::PromptVariable]) {
        let rows = prompts
            .iter()
            .map(|prompt| {
                let input = cx.new(|cx| TextInput::new(prompt.default.clone(), "Value", cx));
                (prompt.name.clone(), input)
            })
            .collect();
        self.prompt = Some(PromptDialog { request_id: id.to_string(), rows });
        self.notice = None;
        cx.notify();
    }

    /// Sends the waiting Request with the typed values. Nothing is saved: the values are for this send only.
    fn confirm_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.prompt.take() else {
            return;
        };
        let answers: Vec<KeyValue> =
            dialog.rows.iter().map(|(name, input)| KeyValue::new(name.clone(), input.read(cx).text().to_string())).collect();
        self.send_with(cx, &dialog.request_id, &answers);
    }

    fn cancel_prompt(&mut self, cx: &mut Context<Self>) {
        self.prompt = None;
        cx.notify();
    }

    /// The dialog that asks for the prompt Variables before a send. None when no send is waiting on it.
    fn prompt_dialog(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let dialog = self.prompt.as_ref()?;
        let rows: Vec<_> = dialog
            .rows
            .iter()
            .map(|(name, input)| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.0)).flex_none().font_family(MONO).overflow_hidden().whitespace_nowrap().child(name.clone()))
                    .child(div().flex_1().child(Self::input_cell(input)))
            })
            .collect();
        Some(
            div()
                .id("prompt-dialog")
                .absolute()
                .top(px(120.0))
                .left(px(440.0))
                .w(px(480.0))
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .rounded_md()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CHROME_BG))
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Values for this send"))
                .child(muted_note("These variables have no stored value. The values are used for this send only and are not saved."))
                .children(rows)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(Self::small_button(SharedString::from("prompt-cancel"), "Cancel".to_string()).on_click(cx.listener(
                            |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.cancel_prompt(cx),
                        )))
                        .child(
                            Self::small_button(SharedString::from("prompt-send"), "Send".to_string())
                                .text_color(rgb(ACCENT))
                                .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                    this.confirm_prompt(cx)
                                })),
                        ),
                ),
        )
    }

    /// Clears every entry from the History, on the second click: the first only asks. history.json is written after.
    fn clear_history(&mut self, cx: &mut Context<Self>) {
        if self.confirm_delete.as_deref() != Some(HISTORY_CONFIRM) {
            self.confirm_delete = Some(HISTORY_CONFIRM.to_string());
            cx.notify();
            return;
        }
        self.confirm_delete = None;
        self.history.clear();
        self.save_history();
        self.notice = Some(("History cleared.".to_string(), false));
        cx.notify();
    }

    fn clear_history_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let armed = self.confirm_delete.as_deref() == Some(HISTORY_CONFIRM);
        Self::small_button(SharedString::from("history-clear"), if armed { "Clear history?" } else { "Clear" }.to_string())
            .when(armed, |button| button.text_color(rgb(ERROR)).border_color(rgb(ERROR)))
            .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.clear_history(cx)))
    }

    /// Writes history.json. Nothing is written when the file could not be read at start, or there is no folder.
    fn save_history(&mut self) {
        if !self.history_readable {
            return;
        }
        let Some(dir) = self.personal_dir.clone() else {
            return;
        };
        if let Err(error) = history::save(&dir, &self.history) {
            self.notice = Some((format!("The history could not be saved: {error}"), true));
        }
    }

    /// What the selected sidebar item is, when one is selected and still exists.
    fn selected_kind(&self) -> Option<(String, RowKind)> {
        let id = self.tree_target.as_deref().or(self.state.selected_item())?;
        build_tree(&self.model.workspace, true).into_iter().find(|row| row.id == id).map(|row| (row.id, row.kind))
    }

    /// Opens a Collection or Folder in the sidebar, so a new item inside it shows.
    fn expand(&mut self, item_id: &str) {
        if self.state.is_collapsed(item_id) {
            self.state.toggle_collapsed(item_id);
        }
    }

    /// Adds an empty Request next to or inside the selected item, then opens it. The change is saved with the rest.
    fn add_request(&mut self, cx: &mut Context<Self>) {
        let near = self.selected_kind().map(|(id, _)| id);
        if let Some(near) = near.as_deref() {
            self.expand(near);
        }
        let id = tree_edit::new_request(&mut self.model.workspace, near.as_deref());
        if let Some(collection) = self.model.workspace.collections.first().map(|collection| collection.id.clone()) {
            if near.is_none() {
                self.expand(&collection);
            }
        }
        self.state.mark_unsaved(&id);
        self.state.pick_request(&id);
        self.editing_environment = None;
        self.tree_target = Some(id);
        self.confirm_delete = None;
        self.notice = None;
        cx.notify();
    }

    /// Adds an empty Folder inside the selected Collection or Folder.
    fn add_folder(&mut self, parent: &str, cx: &mut Context<Self>) {
        if let Some(id) = tree_edit::new_folder(&mut self.model.workspace, parent) {
            self.expand(parent);
            self.state.mark_unsaved(&id);
            self.tree_target = Some(id);
            self.confirm_delete = None;
            self.notice = None;
        }
        cx.notify();
    }

    /// Copies the selected Request or Folder straight after it (ADR 0059), and selects the copy.
    fn duplicate(&mut self, item_id: &str, kind: RowKind, cx: &mut Context<Self>) {
        if let Some(copy) = tree_edit::duplicate_item(&mut self.model.workspace, item_id) {
            self.state.mark_unsaved(&copy);
            if kind == RowKind::Request {
                self.state.pick_request(&copy);
            }
            self.tree_target = Some(copy);
            self.confirm_delete = None;
            self.notice = None;
        }
        cx.notify();
    }

    /// Opens a name field for the Collection or Folder, holding its current name. Each change renames it.
    fn start_rename(&mut self, item_id: &str, cx: &mut Context<Self>) {
        let name = build_tree(&self.model.workspace, true).into_iter().find(|row| row.id == item_id).map(|row| row.label);
        let Some(name) = name else {
            return;
        };
        let input = cx.new(|cx| TextInput::new(name, "Name", cx));
        let id = item_id.to_string();
        let subscription = cx.subscribe(&input, move |this: &mut Shell, input, _: &Changed, cx| {
            let text = input.read(cx).text().to_string();
            if tree_edit::rename_container(&mut this.model.workspace, &id, &text) {
                this.state.mark_unsaved(&id);
                this.notice = None;
                cx.notify();
            }
        });
        self.renaming = Some((item_id.to_string(), input, subscription));
        cx.notify();
    }

    /// Hides the item, or shows it again when it is hidden. A hidden item that is not shown in the sidebar loses
    /// the selection, so the buttons never act on something out of sight.
    fn toggle_hidden(&mut self, item_id: &str, cx: &mut Context<Self>) {
        let hide = !tree_edit::is_hidden(&self.model.workspace, item_id);
        if tree_edit::set_hidden(&mut self.model.workspace, item_id, hide) {
            self.state.mark_unsaved(item_id);
            if hide && !self.show_hidden {
                self.tree_target = None;
            }
            self.notice = None;
        }
        cx.notify();
    }

    /// Deletes the item on the second click: the first only asks. Tabs and responses of everything inside go too.
    fn delete(&mut self, item_id: &str, cx: &mut Context<Self>) {
        cx.notify();
        if self.confirm_delete.as_deref() != Some(item_id) {
            self.confirm_delete = Some(item_id.to_string());
            return;
        }
        self.confirm_delete = None;
        let removed = tree_edit::delete_item(&mut self.model.workspace, item_id);
        self.tree_target = None;
        for id in &removed {
            self.state.close_tab(id);
            self.responses.remove(id);
        }
        if let Some(first) = removed.first() {
            self.state.mark_unsaved(first);
            self.notice = Some((format!("Deleted {} item(s). Save to remove them from the folder.", removed.len()), false));
        }
    }

    /// Buttons for the selected sidebar item: add a Request or Folder, copy it, delete it. A button that does not
    /// apply to what is selected is shown dimmed and does nothing.
    fn tree_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let selected = self.selected_kind();
        let kind = selected.as_ref().map(|(_, kind)| *kind);
        let is_container = matches!(kind, Some(RowKind::Collection | RowKind::Folder));
        let can_copy = matches!(kind, Some(RowKind::Folder | RowKind::Request));
        let confirming = selected.as_ref().is_some_and(|(id, _)| self.confirm_delete.as_deref() == Some(id.as_str()));

        let button = |id: &'static str, label: &'static str, enabled: bool| {
            Self::small_button(SharedString::from(id), label.to_string())
                .when(!enabled, |button| button.opacity(0.4).cursor_default())
        };
        let folder_target = selected.clone();
        let copy_target = selected.clone();
        let delete_target = selected.clone();
        div()
            .flex()
            .flex_row()
            .flex_none()
            .gap_1()
            .px_1()
            .child(button("tree-new-request", "+ Request", true).on_click(cx.listener(
                |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.add_request(cx),
            )))
            .child(button("tree-new-folder", "+ Folder", is_container).on_click(cx.listener(
                move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    if let Some((id, RowKind::Collection | RowKind::Folder)) = folder_target.clone() {
                        this.add_folder(&id, cx);
                    }
                },
            )))
            .child(button("tree-copy", "Copy", can_copy).on_click(cx.listener(
                move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    if let Some((id, kind @ (RowKind::Folder | RowKind::Request))) = copy_target.clone() {
                        this.duplicate(&id, kind, cx);
                    }
                },
            )))
            .child(
                button("tree-delete", if confirming { "Delete?" } else { "Delete" }, selected.is_some())
                    .when(confirming, |button| button.text_color(rgb(ERROR)).border_color(rgb(ERROR)))
                    .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        if let Some((id, _)) = delete_target.clone() {
                            this.delete(&id, cx);
                        }
                    })),
            )
    }

    /// Rename and Hide for the selected item, the name field while renaming, and the show-hidden switch.
    fn tree_more_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let selected = self.selected_kind();
        let kind = selected.as_ref().map(|(_, kind)| *kind);
        let can_rename = matches!(kind, Some(RowKind::Collection | RowKind::Folder));
        let can_hide = matches!(kind, Some(RowKind::Folder | RowKind::Request));
        let is_hidden = selected.as_ref().is_some_and(|(id, _)| tree_edit::is_hidden(&self.model.workspace, id));
        let hidden_count = tree_edit::hidden_count(&self.model.workspace);
        let button = |id: &'static str, label: String, enabled: bool| {
            Self::small_button(SharedString::from(id), label).when(!enabled, |button| button.opacity(0.4).cursor_default())
        };
        let rename_target = selected.clone();
        let hide_target = selected.clone();
        let hide_label = ricochet_core::sidebar_menu::hide_label(is_hidden).to_string();
        let show_label = if self.show_hidden { "Hide hidden".to_string() } else { format!("Show hidden ({hidden_count})") };
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .flex_none()
            .gap_1()
            .px_1()
            .child(button("tree-rename", "Rename".to_string(), can_rename).on_click(cx.listener(
                move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    if let Some((id, RowKind::Collection | RowKind::Folder)) = rename_target.clone() {
                        this.start_rename(&id, cx);
                    }
                },
            )))
            .child(button("tree-hide", hide_label, can_hide).on_click(cx.listener(
                move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    if let Some((id, RowKind::Folder | RowKind::Request)) = hide_target.clone() {
                        this.toggle_hidden(&id, cx);
                    }
                },
            )))
            .child(button("tree-show-hidden", show_label, hidden_count > 0 || self.show_hidden).on_click(cx.listener(
                |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    this.show_hidden = !this.show_hidden;
                    cx.notify();
                },
            )));
        row = row.child(Self::small_button(SharedString::from("tree-import"), "Import".to_string()).on_click(cx.listener(
            |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.import_postman(cx),
        )));
        if let Some((_, input, _)) = self.renaming.as_ref() {
            row = div().flex().flex_col().gap_1().child(row).child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .px_1()
                    .child(Self::input_cell(input).flex_1())
                    .child(Self::small_button(SharedString::from("tree-rename-done"), "Done".to_string()).on_click(cx.listener(
                        |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                            this.renaming = None;
                            cx.notify();
                        },
                    ))),
            );
        }
        row
    }

    fn section_label(text: &'static str) -> gpui::Div {
        div().px_2().text_xs().text_color(rgb(FAINT)).child(text)
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = &self.model;
        let tree = build_tree_folded(&model.workspace, self.show_hidden, self.state.collapsed());
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
            .child(self.tree_actions(cx))
            .child(self.tree_more_actions(cx))
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

        let shown: Vec<&HistoryEntry> = self.history.iter().take(HISTORY_SHOWN).collect();
        if !shown.is_empty() {
            let rows: Vec<_> = shown.iter().enumerate().map(|(index, entry)| self.history_row(index, entry, cx)).collect();
            column = column
                .child(div().h(px(1.0)).flex_none().bg(rgb(BORDER)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(Self::section_label("History").pt_1())
                        .child(self.clear_history_button(cx)),
                )
                .child(div().id("history").max_h(px(200.0)).flex().flex_col().overflow_y_scroll().children(rows));
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
                        this.editing_environment = None;
                        this.tree_target = None;
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
    /// A Secret header's value is a masked field kept in the secrets vault. With no vault it is read-only.
    fn headers_editor(&self, request: &ApiRequest, cx: &mut Context<Self>) -> gpui::Div {
        let mut section = div().flex().flex_col().gap_1();
        let inputs = self.inputs.as_ref().map(|inputs| inputs.headers.as_slice()).unwrap_or(&[]);
        for (index, (header, (key_input, value_input))) in request.headers.iter().zip(inputs).enumerate() {
            let value_cell = match value_input.as_ref() {
                Some(value_input) => Self::input_cell(value_input),
                None => div().px_2().py_1().font_family(MONO).text_color(rgb(FAINT)).child("•••••• (a Secret, not editable here)"),
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
                                this.edit_request(cx, |request, secrets| edit::remove_header_and_secret(request, secrets, index));
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

    /// The captures as editable rows: on/off, where the value comes from, the path (when the source takes one), the
    /// Variable it is saved to and a remove mark, then a row to add one.
    fn captures_editor(&self, request: &ApiRequest, cx: &mut Context<Self>) -> gpui::Div {
        let mut section = div().flex().flex_col().gap_1();
        let captures = request.captures.as_deref().unwrap_or(&[]);
        let inputs = self.inputs.as_ref().map(|inputs| inputs.captures.as_slice()).unwrap_or(&[]);
        if captures.is_empty() {
            section = section.child(muted_note("No saved values. A saved value keeps part of a response for later Requests to use."));
        }
        for (index, capture) in captures.iter().enumerate() {
            let inputs = inputs.get(index);
            let path_cell = match inputs {
                Some((_, path)) if edit::capture_needs_path(&capture.source) => Self::input_cell(path).w(px(180.0)).flex_none(),
                _ => div().w(px(180.0)).flex_none().text_xs().text_color(rgb(FAINT)).child("No path for this source"),
            };
            let name_cell = match inputs {
                Some((name, _)) => Self::input_cell(name).flex_1(),
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
                            .id(SharedString::from(format!("capture-toggle-{index}")))
                            .w(px(18.0))
                            .h(px(18.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(if capture.enabled { ACCENT } else { BORDER }))
                            .text_xs()
                            .text_color(rgb(ACCENT))
                            .cursor_pointer()
                            .child(if capture.enabled { "✓" } else { "" })
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::toggle_capture(request, index));
                            })),
                    )
                    .child(
                        Self::small_button(SharedString::from(format!("capture-source-{index}")), capture.source.clone())
                            .flex_none()
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::cycle_capture_source(request, index));
                            })),
                    )
                    .child(path_cell)
                    .child(name_cell)
                    .child(
                        div()
                            .id(SharedString::from(format!("capture-remove-{index}")))
                            .px_2()
                            .flex_none()
                            .text_color(rgb(FAINT))
                            .cursor_pointer()
                            .hover(|style| style.text_color(rgb(ERROR)))
                            .child("×")
                            .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                                this.change_request(cx, |request| edit::remove_capture(request, index));
                                this.inputs = None;
                            })),
                    ),
            );
        }
        section.child(
            div().flex().flex_row().child(
                div()
                    .id("capture-add")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(TEXT)).bg(rgb(ROW_HOVER)))
                    .child("+ Add saved value")
                    .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.change_request(cx, |request| {
                            edit::add_capture(request);
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
                        .track_scroll(&self.body_scroll)
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
            RequestTab::Captures => self.captures_editor(request, cx),
        }
    }

    /// Rebuilds the variable fields when another Collection or Folder is selected, and clears them for a Request.
    fn ensure_container_inputs(&mut self, cx: &mut Context<Self>) {
        let Some(item_id) = self.container_target() else {
            self.container_inputs = None;
            return;
        };
        let is_environment = self.editing_environment.as_deref() == Some(item_id.as_str());
        let source = if is_environment { self.override_source() } else { None };
        if self.container_inputs.as_ref().is_some_and(|inputs| inputs.item_id == item_id && inputs.source == source) {
            return;
        }
        let variables: Vec<KeyValue> = edit::variables_of(&self.model.workspace, &item_id).map(<[KeyValue]>::to_vec).unwrap_or_default();
        let mut subscriptions = Vec::new();
        let name = is_environment.then(|| {
            let current = self.model.workspace.environments.iter().find(|environment| environment.id == item_id).map(|environment| environment.name.clone());
            let input = cx.new(|cx| TextInput::new(current.unwrap_or_default(), "Environment name", cx));
            subscriptions.push(cx.subscribe(&input, |this: &mut Shell, input, _: &Changed, cx| {
                let text = input.read(cx).text().to_string();
                let Some(id) = this.editing_environment.clone() else {
                    return;
                };
                if edit::rename_environment(&mut this.model.workspace, &id, &text) {
                    this.state.mark_unsaved(&id);
                    this.notice = None;
                    cx.notify();
                }
            }));
            input
        });
        let is_folder = !is_environment
            && self.selected_kind().is_some_and(|(id, kind)| id == item_id && kind == RowKind::Folder);
        let tags = is_folder.then(|| {
            let text = edit::folder_tags_of(&self.model.workspace, &item_id).join(", ");
            let input = cx.new(|cx| TextInput::new(text, "Tags, separated by commas", cx));
            let folder = item_id.clone();
            subscriptions.push(cx.subscribe(&input, move |this: &mut Shell, input, _: &Changed, cx| {
                let text = input.read(cx).text().to_string();
                if edit::set_folder_tags(&mut this.model.workspace, &folder, &text) {
                    this.state.mark_unsaved(&folder);
                    this.notice = None;
                    cx.notify();
                }
            }));
            input
        });
        let mut rows = Vec::new();
        for (index, variable) in variables.iter().enumerate() {
            let name = cx.new(|cx| TextInput::new(variable.key.clone(), "Name", cx));
            subscriptions.push(cx.subscribe(&name, move |this: &mut Shell, input, _: &Changed, cx| {
                let text = input.read(cx).text().to_string();
                this.change_variable(cx, index, edit::VariableField::Name, &text);
            }));
            let value = cx.new(|cx| TextInput::new(variable.value.clone(), "Value", cx));
            subscriptions.push(cx.subscribe(&value, move |this: &mut Shell, input, _: &Changed, cx| {
                let text = input.read(cx).text().to_string();
                this.change_variable(cx, index, edit::VariableField::Value, &text);
            }));
            rows.push((name, value));
        }
        let mut overrides = Vec::new();
        if let (true, Some(collection)) = (is_environment, source.as_deref()) {
            for default in edit::collection_variables(&self.model.workspace, collection) {
                let current = self
                    .model
                    .workspace
                    .environments
                    .iter()
                    .find(|environment| environment.id == item_id)
                    .and_then(|environment| environment.variables.iter().find(|variable| variable.key == default.key))
                    .map(|variable| variable.value.clone())
                    .unwrap_or_default();
                let key = default.key.clone();
                let input = cx.new(|cx| TextInput::new(current, "Value for this environment", cx));
                let watched = key.clone();
                subscriptions.push(cx.subscribe(&input, move |this: &mut Shell, input, _: &Changed, cx| {
                    let text = input.read(cx).text().to_string();
                    let Some(environment) = this.editing_environment.clone() else {
                        return;
                    };
                    if edit::set_override(&mut this.model.workspace, &environment, &watched, &text) {
                        this.state.mark_unsaved(&environment);
                        this.notice = None;
                        cx.notify();
                    }
                }));
                overrides.push((key, default.value.clone(), input));
            }
        }
        self.container_inputs = Some(ContainerInputs { item_id, source, name, tags, rows, overrides, _subscriptions: subscriptions });
    }

    /// The Collection, Folder or Environment the editor shows: the Environment open in the editor, else the sidebar's
    /// Collection or Folder. None when a Request is selected.
    fn container_target(&self) -> Option<String> {
        if let Some(id) = self.editing_environment.as_ref() {
            if edit::variables_of(&self.model.workspace, id).is_some() {
                return Some(id.clone());
            }
        }
        self.selected_kind().filter(|(_, kind)| matches!(kind, RowKind::Collection | RowKind::Folder)).map(|(id, _)| id)
    }

    /// The Collection that the sidebar's selection is in, which an Environment's overrides are read from.
    fn override_source(&self) -> Option<String> {
        let item = self.tree_target.as_deref().or(self.state.selected_item())?;
        edit::collection_id_of(&self.model.workspace, item)
    }

    /// Deletes the Environment on the second click: the first only asks.
    fn delete_environment_click(&mut self, cx: &mut Context<Self>, id: &str) {
        if self.confirm_delete.as_deref() != Some(id) {
            self.confirm_delete = Some(id.to_string());
            cx.notify();
            return;
        }
        self.confirm_delete = None;
        if edit::delete_environment(&mut self.model.workspace, id) {
            self.state.mark_unsaved(id);
            if self.state.active_environment_id() == Some(id) {
                self.state.set_active_environment(None);
            }
            self.editing_environment = None;
            self.container_inputs = None;
            self.notice = None;
        }
        cx.notify();
    }

    /// Changes the name or value of one variable as the user types.
    fn change_variable(&mut self, cx: &mut Context<Self>, index: usize, field: edit::VariableField, text: &str) {
        let Some(item_id) = self.container_inputs.as_ref().map(|inputs| inputs.item_id.clone()) else {
            return;
        };
        let Some(variables) = edit::variables_mut(&mut self.model.workspace, &item_id) else {
            return;
        };
        if edit::set_variable_text(variables, index, field, text) {
            self.state.mark_unsaved(&item_id);
            self.notice = None;
            cx.notify();
        }
    }

    /// Adds, turns on or off, or removes variables. The fields are rebuilt afterwards so their rows match the list.
    fn edit_variables(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Vec<KeyValue>) -> bool) {
        let Some(item_id) = self.container_inputs.as_ref().map(|inputs| inputs.item_id.clone()) else {
            return;
        };
        let Some(variables) = edit::variables_mut(&mut self.model.workspace, &item_id) else {
            return;
        };
        if change(variables) {
            self.state.mark_unsaved(&item_id);
            self.notice = None;
            self.container_inputs = None;
        }
        cx.notify();
    }

    /// The pane for a selected Collection or Folder: its name, and its Variables with an on and off switch and a
    /// remove mark on each row, then a row to add one. None when a Request or nothing is selected.
    fn container_pane(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let item_id = self.container_target()?;
        let inputs = self.container_inputs.as_ref().filter(|inputs| inputs.item_id == item_id)?;
        let variables: &[KeyValue] = edit::variables_of(&self.model.workspace, &item_id).unwrap_or_default();
        let title = build_tree(&self.model.workspace, true).into_iter().find(|row| row.id == item_id).map(|row| row.label).unwrap_or_default();
        let heading = match inputs.name.as_ref() {
            Some(name) => Self::input_cell(name).text_lg().font_weight(FontWeight::SEMIBOLD),
            None => div().text_lg().font_weight(FontWeight::SEMIBOLD).child(title),
        };
        let delete_environment = inputs.name.is_some().then(|| {
            let armed = self.confirm_delete.as_deref() == Some(item_id.as_str());
            let id = item_id.clone();
            Self::small_button(SharedString::from("env-delete"), if armed { "Delete environment?" } else { "Delete environment" }.to_string())
                .when(armed, |button| button.text_color(rgb(ERROR)).border_color(rgb(ERROR)))
                .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    this.delete_environment_click(cx, &id);
                }))
        });

        let mut list = div().flex().flex_col().gap_1();
        for (index, (variable, (name_input, value_input))) in variables.iter().zip(&inputs.rows).enumerate() {
            let label = if variable.enabled { "On" } else { "Off" };
            let switch = Self::small_button(SharedString::from(format!("var-switch-{index}")), label.to_string())
                .text_color(rgb(if variable.enabled { SUCCESS } else { FAINT }))
                .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    this.edit_variables(cx, |vars| edit::toggle_variable(vars, index));
                }));
            let remove = Self::small_button(SharedString::from(format!("var-remove-{index}")), "×".to_string())
                .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    this.edit_variables(cx, |vars| edit::remove_variable(vars, index));
                }));
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(switch)
                    .child(div().w(px(220.0)).flex_none().child(Self::input_cell(name_input)))
                    .child(div().flex_1().child(Self::input_cell(value_input)))
                    .child(remove),
            );
        }
        let add = Self::small_button(SharedString::from("var-add"), "+ Variable".to_string())
            .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                this.edit_variables(cx, |vars| {
                    edit::add_variable(vars);
                    true
                });
            }));
        Some(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(heading)
                .children(inputs.tags.as_ref().map(|tags| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(60.0)).flex_none().text_color(rgb(MUTED)).child("Tags"))
                        .child(div().flex_1().child(Self::input_cell(tags)))
                }))
                .child(muted_note("These Variables apply to the Requests inside this item. A Variable that is off is not used."))
                .child(list.id("container-variables").max_h(px(360.0)).overflow_y_scroll())
                .child(add)
                .children((!inputs.overrides.is_empty()).then(|| {
                    let mut section = div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .pt_2()
                        .child(muted_note("From the collection. A value typed here overrides the default, and an empty value uses the default."));
                    for (key, default, input) in &inputs.overrides {
                        let default_text = if default.is_empty() { "No default".to_string() } else { format!("Default: {default}") };
                        section = section.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(div().w(px(220.0)).flex_none().font_family(MONO).overflow_hidden().whitespace_nowrap().child(key.clone()))
                                .child(div().w(px(200.0)).flex_none().text_xs().text_color(rgb(FAINT)).overflow_hidden().whitespace_nowrap().child(default_text))
                                .child(div().flex_1().child(Self::input_cell(input))),
                        );
                    }
                    section
                }))
                .children(delete_environment),
        )
    }

    fn request_pane(&self, url_focused: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pane = div().flex_none().flex().flex_col().gap_2().p_3();
        if let Some(error) = &self.model.error {
            return pane.child(div().text_color(rgb(WARNING)).child(error.clone()));
        }
        if let Some(container) = self.container_pane(cx) {
            return container;
        }
        let Some(request) = self.selected_request() else {
            return pane.child(muted_note("Pick a request in the sidebar to open it."));
        };
        // The scope is found by the sidebar item's id, which is not the Request's own id.
        let scope = self.scope_for(self.state.selected_item().unwrap_or_default());
        let resolved = variables::apply(request, &scope).url;
        let checks = request.assertions.as_ref().map_or(0, Vec::len);
        let captures = request.captures.as_ref().map_or(0, Vec::len);
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
                .child(self.request_tab_button(RequestTab::Checks, format!("Checks {checks}"), cx))
                .child(div().flex_1())
                .child(Self::small_button(SharedString::from("copy-curl"), "Copy as curl".to_string()).on_click(cx.listener(
                    |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.copy_as_curl(cx),
                )))
                .child(self.request_tab_button(RequestTab::Captures, format!("Captures {captures}"), cx)),
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
            ResponseTab::Compare => String::new(),
            ResponseTab::Raw => String::from_utf8_lossy(&response.body).into_owned(),
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
                .child(Self::small_button(SharedString::from("save-example"), "Save example".to_string()).on_click(cx.listener(
                    |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.save_example(cx),
                )))
                .child(Self::small_button(SharedString::from("save-file"), "Save file".to_string()).on_click(cx.listener(
                    |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| this.save_response_file(cx),
                )))
                .child(self.response_tab_button(ResponseTab::Body, "Body".to_string(), cx))
                .child(self.response_tab_button(ResponseTab::Headers, format!("Headers {}", response.headers.len()), cx))
                .child(self.response_tab_button(ResponseTab::Compare, "Compare".to_string(), cx))
                .child(self.response_tab_button(ResponseTab::Raw, "Raw".to_string(), cx))
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
            ResponseTab::Raw => {
                let text = String::from_utf8_lossy(&response.body).into_owned();
                content.children(text.lines().map(|line| div().flex_none().child(line.to_string())).collect::<Vec<_>>())
            }
            ResponseTab::Compare => {
                let previous = self.state.selected_item().and_then(|id| self.previous_responses.get(id));
                content.child(compare_view(previous, &input_of(response)))
            }
        })
    }

    /// The menu under the top bar: no environment, then each Environment, with the active one marked.
    fn environment_menu(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        if !self.environment_menu_open {
            return None;
        }
        let active = self.state.active_environment_id().map(str::to_string);
        let mut options = vec![(None, "No environment".to_string())];
        for environment in &self.model.workspace.environments {
            options.push((Some(environment.id.clone()), environment.name.trim().to_string()));
        }
        let rows = options.into_iter().enumerate().map(|(index, (id, label))| {
            let is_active = id == active;
            let chosen = id.clone();
            div()
                .id(SharedString::from(format!("env-option-{index}")))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .text_color(rgb(if is_active { SUCCESS } else { TEXT }))
                .hover(|style| style.bg(rgb(ROW_HOVER)))
                .child(div().w(px(12.0)).text_color(rgb(SUCCESS)).child(if is_active { "✓" } else { "" }))
                .child(label)
                .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                    this.state.set_active_environment(chosen.as_deref());
                    this.save_environment();
                    this.environment_menu_open = false;
                    cx.notify();
                }))
        });
        Some(
            div()
                .id("environment-menu")
                .absolute()
                .top(px(44.0))
                .right(px(12.0))
                .w(px(240.0))
                .flex()
                .flex_col()
                .p_1()
                .gap_px()
                .rounded_md()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CHROME_BG))
                .children(rows)
                .child(
                    div()
                        .id("env-new")
                        .px_3()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(rgb(ACCENT))
                        .hover(|style| style.bg(rgb(ROW_HOVER)))
                        .child("+ New environment")
                        .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                            let id = edit::new_environment(&mut this.model.workspace, "New environment");
                            this.state.mark_unsaved(&id);
                            this.editing_environment = Some(id);
                            this.container_inputs = None;
                            this.environment_menu_open = false;
                            this.notice = None;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("env-edit")
                        .px_3()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(rgb(ACCENT))
                        .hover(|style| style.bg(rgb(ROW_HOVER)))
                        .child("Edit environments")
                        .on_click(cx.listener(|this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                            let first = this.model.workspace.environments.first().map(|environment| environment.id.clone());
                            this.editing_environment = this.state.active_environment_id().map(str::to_string).or(first);
                            this.container_inputs = None;
                            this.environment_menu_open = false;
                            cx.notify();
                        })),
                ),
        )
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
        self.ensure_container_inputs(cx);
        let url_focused = self.inputs.as_ref().is_some_and(|inputs| inputs.url.focus_handle(cx).is_focused(window));
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this: &mut Shell, _: &Save, _: &mut Window, cx: &mut Context<Shell>| this.save_workspace(cx)))
            .on_action(cx.listener(|this: &mut Shell, _: &NewRequest, _: &mut Window, cx: &mut Context<Shell>| this.add_request(cx)))
            .on_action(cx.listener(|this: &mut Shell, _: &OpenPalette, window: &mut Window, cx: &mut Context<Shell>| {
                this.open_palette(window, cx)
            }))
            .on_action(cx.listener(|this: &mut Shell, _: &ClosePalette, _: &mut Window, cx: &mut Context<Shell>| this.close_palette(cx)))
            .on_action(cx.listener(|this: &mut Shell, _: &CloseTab, _: &mut Window, cx: &mut Context<Shell>| {
                this.state.close_active_tab();
                this.editing_environment = None;
                this.tree_target = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this: &mut Shell, _: &NextTab, _: &mut Window, cx: &mut Context<Shell>| {
                this.state.step_tab(1);
                this.editing_environment = None;
                this.tree_target = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this: &mut Shell, _: &PreviousTab, _: &mut Window, cx: &mut Context<Shell>| {
                this.state.step_tab(-1);
                this.editing_environment = None;
                this.tree_target = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this: &mut Shell, _: &OpenShortcutSheet, _: &mut Window, cx: &mut Context<Shell>| {
                this.shortcut_sheet_open = true;
                cx.notify();
            }))
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
            .children(self.environment_menu(cx))
            .children(self.prompt_dialog(cx))
            .children(self.palette_overlay(cx))
            .children(self.shortcut_sheet_overlay())
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
            KeyBinding::new("secondary-n", NewRequest, None),
            KeyBinding::new("secondary-k", OpenPalette, None),
            KeyBinding::new("escape", ClosePalette, None),
            KeyBinding::new("f1", OpenShortcutSheet, None),
            KeyBinding::new("secondary-w", CloseTab, None),
            KeyBinding::new("secondary-tab", NextTab, None),
            KeyBinding::new("secondary-shift-tab", PreviousTab, None),
            KeyBinding::new("secondary-/", OpenShortcutSheet, None),
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
