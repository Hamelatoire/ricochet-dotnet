//! Tests for remembering the window's placement and the panel sizes (ADR 0043, ADR 0068).

use pretty_assertions::assert_eq;
use ricochet_app::memory::{
    MIN_REQUEST_HEIGHT, MIN_SIDEBAR_WIDTH, ScreenArea, clamp_request_height, clamp_sidebar_width, placement, restored_panels, screen_for,
};
use ricochet_core::personal_state::{PanelLayout, WindowPlacement};

const LEFT: ScreenArea = ScreenArea { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 };
const RIGHT: ScreenArea = ScreenArea { x: 1920.0, y: 0.0, width: 2560.0, height: 1440.0 };

fn at(x: i32, y: i32) -> WindowPlacement {
    WindowPlacement { x, y, width: 1200.0, height: 800.0, is_maximized: false }
}

#[test]
fn a_placement_is_rounded_to_whole_pixels() {
    assert_eq!(placement(10.4, -3.6, 1199.7, 800.2, true), WindowPlacement { x: 10, y: -4, width: 1200.0, height: 800.0, is_maximized: true });
}

#[test]
fn a_window_opens_on_the_screen_its_title_bar_is_on() {
    assert_eq!(screen_for(&at(100, 100), &[LEFT, RIGHT]), Some(0));
    assert_eq!(screen_for(&at(2000, 50), &[LEFT, RIGHT]), Some(1));
    // Partly off the left edge, but the title bar's grab point is still on the screen.
    assert_eq!(screen_for(&at(-30, 0), &[LEFT]), Some(0));
}

#[test]
fn a_window_on_a_screen_that_is_gone_keeps_the_default() {
    assert_eq!(screen_for(&at(2000, 50), &[LEFT]), None);
    assert_eq!(screen_for(&at(-100, 100), &[LEFT]), None);
    assert_eq!(screen_for(&at(100, 100), &[]), None);
}

#[test]
fn an_unreasonable_placement_keeps_the_default_even_on_a_screen() {
    let tiny = WindowPlacement { width: 10.0, ..at(100, 100) };
    assert_eq!(screen_for(&tiny, &[LEFT]), None);
}

#[test]
fn the_sidebar_leaves_room_for_the_request_and_response() {
    assert_eq!(clamp_sidebar_width(300.0, 1280.0), 300.0);
    assert_eq!(clamp_sidebar_width(1000.0, 1280.0), 800.0);
    assert_eq!(clamp_sidebar_width(50.0, 1280.0), MIN_SIDEBAR_WIDTH);
    // A window too narrow for both still gets the narrowest sidebar.
    assert_eq!(clamp_sidebar_width(300.0, 500.0), MIN_SIDEBAR_WIDTH);
}

#[test]
fn the_request_pane_leaves_room_for_the_response() {
    assert_eq!(clamp_request_height(250.0, 700.0), 250.0);
    assert_eq!(clamp_request_height(650.0, 700.0), 500.0);
    assert_eq!(clamp_request_height(10.0, 700.0), MIN_REQUEST_HEIGHT);
    assert_eq!(clamp_request_height(300.0, 100.0), MIN_REQUEST_HEIGHT);
}

#[test]
fn saved_panels_are_restored_clamped_to_the_window() {
    let saved = PanelLayout { sidebar_width: Some(1000.0), request_height: Some(900.0) };
    assert_eq!(restored_panels(Some(&saved), 1280.0, 800.0), (Some(800.0), Some(480.0)));
}

#[test]
fn panels_not_saved_or_damaged_keep_their_defaults() {
    assert_eq!(restored_panels(None, 1280.0, 800.0), (None, None));
    let damaged = PanelLayout { sidebar_width: Some(f64::NAN), request_height: Some(240.0) };
    assert_eq!(restored_panels(Some(&damaged), 1280.0, 800.0), (None, Some(240.0)));
}
