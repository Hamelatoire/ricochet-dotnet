// Feasibility spike for ADR 0091: one GPUI window, nothing else.
// Not wired to the C# app. Check: it builds and opens on Windows.

use gpui::{
    App, Application, Bounds, Context, Render, SharedString, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

struct Spike {
    text: SharedString,
}

impl Render for Spike {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .justify_center()
            .items_center()
            .text_color(rgb(0xffffff))
            .text_xl()
            .child(format!("Ricochet spike: {}", self.text))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(640.), px(400.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Spike {
                    text: "GPUI 0.2.2 on Windows".into(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
