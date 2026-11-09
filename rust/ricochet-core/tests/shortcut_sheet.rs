use pretty_assertions::assert_eq;
use ricochet_core::shortcut_sheet::*;

#[test]
fn every_shortcut_names_an_area_keys_and_an_action() {
    for entry in entries(false) {
        assert!(!entry.keys.trim().is_empty());
        assert!(!entry.action.trim().is_empty());
    }
}

#[test]
fn shortcuts_use_ctrl_on_windows_and_cmd_on_macos() {
    assert!(entries(false).iter().any(|e| e.keys == "Ctrl+S"));
    assert!(entries(true).iter().any(|e| e.keys == "Cmd+S"));
    assert!(!entries(true).iter().any(|e| e.keys.starts_with("Ctrl+")));
}

#[test]
fn groups_follow_the_area_order_and_keep_every_entry() {
    let groups = groups(false);

    let areas: Vec<ShortcutArea> = groups.iter().map(|(area, _)| *area).collect();
    assert_eq!(
        areas,
        vec![ShortcutArea::General, ShortcutArea::Request, ShortcutArea::Palette]
    );
    let total: usize = groups.iter().map(|(_, group)| group.len()).sum();
    assert_eq!(total, entries(false).len());
}

#[test]
fn groups_keep_the_table_order_within_each_area() {
    let groups = groups(false);
    let general: Vec<&str> = groups[0].1.iter().map(|e| e.keys.as_str()).collect();

    assert_eq!(general, vec!["F1", "Ctrl+/", "Ctrl+K", "Ctrl+P", "Ctrl+N", "Ctrl+S", "Ctrl+W"]);
}

#[test]
fn each_area_has_a_heading() {
    for area in [ShortcutArea::General, ShortcutArea::Request, ShortcutArea::Palette] {
        assert!(!title(area).trim().is_empty());
    }
    assert_eq!(title(ShortcutArea::General), "Everywhere");
    assert_eq!(title(ShortcutArea::Request), "In a request");
    assert_eq!(title(ShortcutArea::Palette), "In the palette and this list");
}
