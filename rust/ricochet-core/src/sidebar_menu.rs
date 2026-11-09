//! Port of `SidebarMenu.cs` (ADR 0056). Decides which context menu entries apply to a sidebar item, so the menu
//! never offers an action that would be refused. The menu entries run the same commands as the item's buttons.
//! No UI here: the view builds the menu from these rules.

/// What a sidebar item is: a Collection at the top, a Folder inside it, or a Request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarItemKind {
    Collection,
    Folder,
    Request,
}

/// The actions in a sidebar item's context menu (ADR 0056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarAction {
    Rename,
    Tags,
    NewFolder,
    Hide,
    ShowHidden,
    Delete,
}

/// True when the context menu for an item of `kind` should offer `action`.
///
/// `is_hidden` and `hidden_count` describe the item and what is hidden inside it. `is_hidden` is accepted for
/// parity with the C# signature, but it does not change what is offered: a hidden item gets the "Show" label
/// instead (see [`hide_label`]).
pub fn offers(action: SidebarAction, kind: SidebarItemKind, _is_hidden: bool, hidden_count: usize) -> bool {
    match action {
        // Collections and Folders can be renamed and can take a new Folder. A Request keeps its name in the tree.
        SidebarAction::Rename | SidebarAction::NewFolder => kind != SidebarItemKind::Request,

        // Only Folders take tags (ADR 0030).
        SidebarAction::Tags => kind == SidebarItemKind::Folder,

        // A Collection is never hidden, so it has "Show hidden" instead of "Hide" when something inside is hidden.
        SidebarAction::Hide => kind != SidebarItemKind::Collection,
        SidebarAction::ShowHidden => kind == SidebarItemKind::Collection && hidden_count > 0,

        SidebarAction::Delete => true,
    }
}

/// The label for hiding or showing an item: a hidden item offers "Show" (ADR 0032).
pub fn hide_label(is_hidden: bool) -> &'static str {
    if is_hidden { "Show" } else { "Hide" }
}
