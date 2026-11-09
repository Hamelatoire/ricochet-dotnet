use pretty_assertions::assert_eq;
use ricochet_core::sidebar_menu::{SidebarAction, SidebarItemKind, hide_label, offers};

#[test]
fn a_request_offers_hide_and_delete_but_not_rename_tags_or_new_folder() {
    assert!(!offers(SidebarAction::Rename, SidebarItemKind::Request, false, 0));
    assert!(!offers(SidebarAction::Tags, SidebarItemKind::Request, false, 0));
    assert!(!offers(SidebarAction::NewFolder, SidebarItemKind::Request, false, 0));
    assert!(offers(SidebarAction::Hide, SidebarItemKind::Request, false, 0));
    assert!(offers(SidebarAction::Delete, SidebarItemKind::Request, false, 0));
}

#[test]
fn a_folder_offers_rename_tags_new_folder_hide_and_delete() {
    for action in [
        SidebarAction::Rename,
        SidebarAction::Tags,
        SidebarAction::NewFolder,
        SidebarAction::Hide,
        SidebarAction::Delete,
    ] {
        assert!(offers(action, SidebarItemKind::Folder, false, 0), "{action:?}");
    }
    assert!(!offers(SidebarAction::ShowHidden, SidebarItemKind::Folder, false, 3));
}

#[test]
fn a_collection_never_offers_hide_or_tags() {
    assert!(!offers(SidebarAction::Hide, SidebarItemKind::Collection, false, 0));
    assert!(!offers(SidebarAction::Tags, SidebarItemKind::Collection, false, 0));
    assert!(offers(SidebarAction::Rename, SidebarItemKind::Collection, false, 0));
}

#[test]
fn show_hidden_appears_on_a_collection_only_when_something_is_hidden() {
    assert!(!offers(SidebarAction::ShowHidden, SidebarItemKind::Collection, false, 0));
    assert!(offers(SidebarAction::ShowHidden, SidebarItemKind::Collection, false, 2));
}

#[test]
fn hide_label_says_show_for_a_hidden_item() {
    assert_eq!(hide_label(false), "Hide");
    assert_eq!(hide_label(true), "Show");
}
