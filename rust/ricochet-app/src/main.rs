// The Ricochet window (ADR 0091): a top bar with the Environment, a sidebar of Collections and recent sends, a request
// pane with a URL bar and section tabs, a response pane and a status bar. Reads a Workspace folder from the first
// argument or RICOCHET_DATA. Send resolves Variables first; editing comes in a later slice.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    App, Application, Bounds, ClickEvent, ClipboardItem, Context, FontWeight, Image, ImageFormat, Render, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, img, prelude::*, px, rgb, size,
};
use ricochet_app::json_color::{SpanKind, color_line};
use ricochet_app::request_view::{RecentSend, RecentSends, UrlSegment, query_params, url_segments};
use ricochet_app::scope::variables_in_scope;
use ricochet_app::shell::{ShellModel, folder_from};
use ricochet_app::state::AppState;
use ricochet_app::status::NO_ENVIRONMENT;
use ricochet_app::tree::{RowKind, TreeRow, build_tree_folded, find_request};
use ricochet_core::models::{ApiRequest, KeyValue, auth_kinds};
use ricochet_core::response_status::{ResponseStatusClass, classify};
use ricochet_core::send::{ApiResponse, RequestSender};
use ricochet_core::{dotenv, response_format, variables};

/// The app icon, also embedded in the exe by `build.rs`.
const ICON_PNG: &[u8] = include_bytes!("../assets/ricochet-64.png");

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

struct Shell {
    model: ShellModel,
    state: AppState,
    /// The latest response of each Request sent in this window, by Request id. Kept in memory only.
    responses: HashMap<String, ResponseView>,
    recent: RecentSends,
    request_tab: RequestTab,
    response_tab: ResponseTab,
    /// The Workspace's `.env` Variables, read at start and again before each send.
    dot_env: Vec<KeyValue>,
    icon: Arc<Image>,
}

impl Shell {
    fn new(model: ShellModel) -> Self {
        let state = AppState::for_workspace(&model.workspace);
        let dot_env = read_dot_env(&model);
        Self {
            model,
            state,
            responses: HashMap::new(),
            recent: RecentSends::default(),
            request_tab: RequestTab::Headers,
            response_tab: ResponseTab::Body,
            dot_env,
            icon: Arc::new(Image::from_bytes(ImageFormat::Png, ICON_PNG.to_vec())),
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
        let sent = variables::apply(request, &self.scope_for(&id));
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
            .child(img(self.icon.clone()).size(px(20.0)))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Ricochet"))
            .child(div().flex_1())
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
        let rows: Vec<_> = tree.iter().map(|row| self.tree_row(row, cx)).collect();

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
            .child(div().id("collection-tree").flex_1().flex().flex_col().overflow_y_scroll().children(rows));

        if tree.is_empty() && model.error.is_none() {
            column = column.child(muted_note("This workspace has no requests yet.").px_2());
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
                    .child(label)
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

    /// The method, the URL with its placeholders as chips, and the Send button, in one control.
    fn url_bar(&self, request: &ApiRequest, scope: &[KeyValue], cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sending = matches!(self.selected_response(), Some(ResponseView::Sending));
        let segments = url_segments(&request.url, scope).into_iter().map(|segment| match segment {
            UrlSegment::Text(text) => div().flex_none().child(text),
            UrlSegment::Chip { name, resolves } => div()
                .flex_none()
                .px_1()
                .rounded_sm()
                .bg(rgb(if resolves { CHIP_BG } else { 0x4a3a1a }))
                .text_color(rgb(if resolves { CHIP_TEXT } else { WARNING }))
                .child(name),
        });
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(36.0))
            .flex_none()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(FIELD_BG))
            .overflow_hidden()
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_none()
                    .items_center()
                    .px_3()
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(method_color(&request.method)))
                    .child(request.method.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_3()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_family(MONO)
                    .children(segments),
            )
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

    fn request_section(&self, request: &ApiRequest) -> gpui::Div {
        let section = div().flex().flex_col().gap_1();
        match self.request_tab {
            RequestTab::Params => {
                let params = query_params(&request.url);
                if params.is_empty() {
                    return section.child(muted_note("The URL has no query parameters."));
                }
                section.children(params.into_iter().map(|(key, value)| pair_row(key, value, false)))
            }
            RequestTab::Headers => {
                if request.headers.is_empty() {
                    return section.child(muted_note("No headers."));
                }
                section.children(request.headers.iter().map(|header| {
                    let value = if header.is_secret { "••••••".to_string() } else { header.value.clone() };
                    pair_row(header.key.clone(), value, !header.enabled)
                }))
            }
            RequestTab::Auth => section.child(muted_note(auth_label(request))),
            RequestTab::Body => {
                if request.body.is_empty() {
                    return section.child(muted_note("No body."));
                }
                section.children(request.body.lines().map(|line| {
                    let colored = color_line(line);
                    div().flex().flex_row().font_family(MONO).pl(px(INDENT_STEP * colored.indent as f32)).children(
                        colored.spans.into_iter().map(|span| div().flex_none().text_color(rgb(span_color(span.kind))).child(span.text)),
                    )
                }))
            }
            RequestTab::Checks => {
                let checks = request.assertions.as_deref().unwrap_or(&[]);
                if checks.is_empty() {
                    return section.child(muted_note("No checks."));
                }
                section.children(checks.iter().map(|check| {
                    let subject = if check.path.is_empty() { check.source.clone() } else { format!("{} {}", check.source, check.path) };
                    pair_row(subject, format!("{} {}", check.op, check.expected), !check.enabled)
                }))
            }
        }
    }

    fn request_pane(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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

        let mut pane = pane.child(self.url_bar(request, &scope, cx));
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
        .child(self.request_section(request).id("request-section").max_h(px(220.0)).overflow_y_scroll().py_1())
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
    }
}

fn read_dot_env(model: &ShellModel) -> Vec<KeyValue> {
    model.folder_path().and_then(|folder| dotenv::read(folder).ok()).map(|result| result.variables).unwrap_or_default()
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
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
                        .child(self.request_pane(cx))
                        .child(self.response_pane(cx)),
                ),
            )
            .child(self.status_bar())
    }
}

fn main() {
    let folder = folder_from(std::env::args().nth(1).as_deref(), std::env::var("RICOCHET_DATA").ok().as_deref());
    let model = ShellModel::load(folder);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("Ricochet".into()), ..Default::default() }),
                ..Default::default()
            },
            move |_, cx| cx.new(move |_| Shell::new(model)),
        )
        .unwrap();
        cx.activate(true);
    });
}
