// The Ricochet window shell (ADR 0091): a sidebar tree of Collections and Requests, a request area, a response
// area and a status bar. Reads a Workspace folder from the first argument or RICOCHET_DATA. Nothing is sent or
// edited yet; later slices fill in the panes.

use gpui::{
    App, Application, Bounds, ClickEvent, Context, FontWeight, Render, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use ricochet_app::shell::{ShellModel, folder_from};
use ricochet_app::state::AppState;
use ricochet_app::tree::{RowKind, TreeRow, find_request};
use ricochet_core::models::{ApiRequest, auth_kinds};

const WINDOW_BG: u32 = 0x1b1d22;
const SIDEBAR_BG: u32 = 0x202328;
const PANEL_BG: u32 = 0x24282e;
const BORDER: u32 = 0x2e323a;
const ROW_HOVER: u32 = 0x2b3038;
const ROW_SELECTED: u32 = 0x2c3d5c;
const TEXT: u32 = 0xe6e8eb;
const MUTED: u32 = 0x9aa1ab;
const WARNING: u32 = 0xf2c94c;
const SIDEBAR_WIDTH: f32 = 310.0;

fn method_color(method: &str) -> u32 {
    match method {
        "GET" => 0x6fcf97,
        "POST" => 0xf2c94c,
        "PUT" => 0x56b6f2,
        "PATCH" => 0xbb87f5,
        "DELETE" => 0xeb5757,
        _ => MUTED,
    }
}

struct Shell {
    model: ShellModel,
    state: AppState,
}

impl Shell {
    fn new(model: ShellModel) -> Self {
        let state = AppState::for_workspace(&model.workspace);
        Self { model, state }
    }

    fn request_name(&self, id: &str) -> String {
        find_request(&self.model.workspace, id).map(|request| request.name.clone()).unwrap_or_else(|| id.to_string())
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tabs: Vec<_> = self.state.open_tabs().iter().map(|id| self.tab(id, cx)).collect();
        let count = self.state.open_tabs().len();
        let mut strip = div().flex().flex_row().items_center().gap_2().h(px(30.0));
        if count == 0 {
            strip = strip.child(div().text_xs().text_color(rgb(MUTED)).child("No requests open"));
        } else {
            strip = strip
                .child(
                    div()
                        .id("tab-strip")
                        .flex_1()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .overflow_x_scroll()
                        .children(tabs),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(if count == 1 { "1 open".to_string() } else { format!("{count} open") }),
                );
        }
        strip
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
            .items_center()
            .gap_2()
            .h(px(26.0))
            .px_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(if is_active { PANEL_BG } else { SIDEBAR_BG }))
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
                    .text_color(rgb(MUTED))
                    .hover(|style| style.text_color(rgb(TEXT)))
                    .child("×")
                    .on_click(cx.listener(move |this: &mut Shell, _: &ClickEvent, _: &mut Window, cx: &mut Context<Shell>| {
                        this.state.close_tab(&close_id);
                        cx.notify();
                    })),
            )
    }

    fn tree_row(&self, row: &TreeRow, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let is_selected = self.state.selected_item() == Some(row.id.as_str());
        let is_request = row.kind == RowKind::Request;
        let id = row.id.clone();
        let indent = px(10.0 + 14.0 * row.depth as f32);
        let label_weight = match row.kind {
            RowKind::Request => FontWeight::NORMAL,
            RowKind::Folder => FontWeight::MEDIUM,
            RowKind::Collection => FontWeight::BOLD,
        };
        let text_color = if row.hidden { MUTED } else { TEXT };

        let mut line = div()
            .id(SharedString::from(format!("row-{}", row.id)))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(24.0))
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
                    this.state.select_item(&id);
                }
                cx.notify();
            }));

        if let Some(method) = &row.method {
            line = line.child(
                div()
                    .w(px(46.0))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(method_color(method)))
                    .child(method.clone()),
            );
        }
        line.child(
            div()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(text_color))
                .font_weight(label_weight)
                .child(row.label.clone()),
        )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = &self.model;
        let rows: Vec<_> = model.rows.iter().map(|row| self.tree_row(row, cx)).collect();

        let mut column = div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(SIDEBAR_BG))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child("COLLECTIONS"),
            )
            .child(
                div()
                    .id("collection-tree")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(rows),
            );

        if model.rows.is_empty() && model.error.is_none() {
            column = column.child(div().text_color(rgb(MUTED)).child("This workspace has no requests yet."));
        }
        if !model.issues.is_empty() {
            let text = format!(
                "{} file(s) could not be read and were left alone.",
                model.issues.len()
            );
            column = column.child(div().text_xs().text_color(rgb(WARNING)).child(text));
        }
        column
    }

    fn request_panel(&self) -> impl IntoElement + use<> {
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_md()
            .bg(rgb(PANEL_BG))
            .border_1()
            .border_color(rgb(BORDER));

        if let Some(error) = &self.model.error {
            return panel.child(div().text_color(rgb(WARNING)).child(error.clone()));
        }

        let request: Option<&ApiRequest> = self
            .state
            .selected_item()
            .and_then(|id| find_request(&self.model.workspace, id));

        match request {
            Some(request) => {
                panel = panel
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(request.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .items_center()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgb(BORDER))
                                    .text_color(rgb(method_color(&request.method)))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(request.method.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(request.url.clone()),
                            ),
                    )
                    .child(header_rows(request))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(format!("Auth: {}", auth_label(request))),
                    );
                if !request.body.is_empty() {
                    panel = panel.child(
                        div()
                            .font_family("Consolas")
                            .text_sm()
                            .p_3()
                            .rounded_sm()
                            .bg(rgb(WINDOW_BG))
                            .child(request.body.clone()),
                    );
                }
            }
            None => {
                panel = panel.child(
                    div()
                        .text_color(rgb(MUTED))
                        .child("Select a request in the sidebar to see it here."),
                );
            }
        }
        panel.child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("Editing and sending come in a later change."),
        )
    }

    fn response_panel(&self) -> impl IntoElement + use<> {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_md()
            .bg(rgb(PANEL_BG))
            .border_1()
            .border_color(rgb(BORDER))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Response"))
            .child(div().text_color(rgb(MUTED)).child("No response yet."))
    }

    fn status_bar(&self) -> impl IntoElement + use<> {
        let status = &self.model.status;
        let branch = status.branch.clone().unwrap_or_else(|| "no git branch".to_string());
        div()
            .h(px(26.0))
            .flex()
            .flex_row()
            .items_center()
            .gap_5()
            .px_3()
            .bg(rgb(SIDEBAR_BG))
            .border_t_1()
            .border_color(rgb(BORDER))
            .text_xs()
            .text_color(rgb(MUTED))
            .child(format!("Environment: {}", status.environment))
            .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(format!("Workspace: {}", status.workspace)))
            .child(format!("Branch: {branch}"))
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.sidebar(cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(WINDOW_BG))
            .text_color(rgb(TEXT))
            .text_sm()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .overflow_hidden()
                    .child(sidebar)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_3()
                            .overflow_hidden()
                            .child(self.tab_strip(cx))
                            .child(self.request_panel())
                            .child(self.response_panel()),
                    ),
            )
            .child(self.status_bar())
    }
}

/// The Request's headers, one per line. Disabled headers are dimmed, and Secret values are masked, as in the C# app.
fn header_rows(request: &ApiRequest) -> impl IntoElement + use<> {
    let mut column = div().flex().flex_col().gap_1().child(
        div()
            .text_xs()
            .text_color(rgb(MUTED))
            .child(format!("{} header(s)", request.headers.len())),
    );
    for header in &request.headers {
        let value = if header.is_secret { "••••••".to_string() } else { header.value.clone() };
        let color = if header.enabled { TEXT } else { MUTED };
        column = column.child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .text_color(rgb(color))
                .child(div().w(px(180.0)).overflow_hidden().whitespace_nowrap().child(header.key.clone()))
                .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(value)),
        );
    }
    column
}

/// The auth kind in words. Credentials are never shown, only the kind.
fn auth_label(request: &ApiRequest) -> &'static str {
    match request.auth.as_ref().map(|auth| auth.kind.as_str()) {
        None | Some(auth_kinds::NONE) => "none",
        Some(auth_kinds::BEARER) => "bearer token (hidden)",
        Some(auth_kinds::BASIC) => "basic (password hidden)",
        Some(auth_kinds::API_KEY) => "API key (value hidden)",
        Some(auth_kinds::DIGEST) => "digest (secret hidden)",
        Some(auth_kinds::OAUTH1) => "OAuth 1.0 (secrets hidden)",
        Some(auth_kinds::AWS_SIG_V4) => "AWS SigV4 (secrets hidden)",
        Some(_) => "unknown kind",
    }
}

fn main() {
    let folder = folder_from(
        std::env::args().nth(1).as_deref(),
        std::env::var("RICOCHET_DATA").ok().as_deref(),
    );
    let model = ShellModel::load(folder);

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Ricochet".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| cx.new(move |_| Shell::new(model)),
        )
        .unwrap();
        cx.activate(true);
    });
}
