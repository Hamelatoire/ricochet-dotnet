//! Where the person left off (ADR 0043, ADR 0068): the window's placement and the panel sizes, as the C# app's
//! `WindowMemory` and `PanelMemory` keep them. Plain numbers with no GPUI types, so tests reach them directly. The
//! values are personal and saved in `state.json`, never in the Workspace.
//!
//! The Rust window works in logical pixels, so the saved position is logical too. The C# app saves its position in
//! device pixels, so with display scaling above 100% a window saved by one app opens a little off in the other.

use ricochet_core::personal_state::{PanelLayout, WindowPlacement};

/// The sidebar width when nothing is saved.
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 280.0;
/// Room kept for the other panels, so a size can never hide the request or response entirely. As in `PanelMemory.cs`.
pub const MIN_MAIN_WIDTH: f32 = 480.0;
pub const MIN_SIDEBAR_WIDTH: f32 = 200.0;
pub const MIN_REQUEST_HEIGHT: f32 = 160.0;
pub const MIN_RESPONSE_HEIGHT: f32 = 200.0;
/// The height of the window's bars above and below the request pane, allowed for when a saved height is restored
/// before the panes are laid out.
const CHROME_HEIGHT: f32 = 120.0;

/// A point on the title bar, near its left edge: if it is on a screen, the window can be reached and grabbed.
const GRAB_X: f64 = 40.0;
const GRAB_Y: f64 = 16.0;

/// A screen's area, in the same logical pixels as the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenArea {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// The placement to save for a window with these normal (not maximised) bounds.
pub fn placement(x: f64, y: f64, width: f64, height: f64, is_maximized: bool) -> WindowPlacement {
    WindowPlacement { x: x.round() as i32, y: y.round() as i32, width: width.round(), height: height.round(), is_maximized }
}

/// The index of the screen a saved window would open on, or None when the placement is not one a window could have
/// (`is_reasonable`), or its title bar would be on no screen, such as a monitor that is no longer there. None keeps
/// the default size and position.
pub fn screen_for(placement: &WindowPlacement, screens: &[ScreenArea]) -> Option<usize> {
    if !placement.is_reasonable() {
        return None;
    }
    let grab_x = f64::from(placement.x) + GRAB_X;
    let grab_y = f64::from(placement.y) + GRAB_Y;
    screens.iter().position(|screen| screen.contains(grab_x, grab_y))
}

/// The sidebar width to use in a window this wide: as wide as asked, but leaving the request and response room.
pub fn clamp_sidebar_width(width: f32, window_width: f32) -> f32 {
    let widest = MIN_SIDEBAR_WIDTH.max(window_width - MIN_MAIN_WIDTH);
    width.clamp(MIN_SIDEBAR_WIDTH, widest)
}

/// The request pane height to use when the request and response share this much height, leaving the response room.
pub fn clamp_request_height(height: f32, shared_height: f32) -> f32 {
    let tallest = MIN_REQUEST_HEIGHT.max(shared_height - MIN_RESPONSE_HEIGHT);
    height.clamp(MIN_REQUEST_HEIGHT, tallest)
}

/// The panel sizes to start with: the saved ones, clamped to the window as it opens, or None for each that was not
/// saved, so it keeps its default.
pub fn restored_panels(saved: Option<&PanelLayout>, window_width: f32, window_height: f32) -> (Option<f32>, Option<f32>) {
    let Some(saved) = saved.map(PanelLayout::sanitized) else {
        return (None, None);
    };
    let sidebar = saved.sidebar_width.map(|width| clamp_sidebar_width(width as f32, window_width));
    let request = saved.request_height.map(|height| clamp_request_height(height as f32, window_height - CHROME_HEIGHT));
    (sidebar, request)
}
