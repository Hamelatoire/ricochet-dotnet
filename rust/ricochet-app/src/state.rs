//! The application state the window reads and changes: the Requests open as tabs (ADR 0041), the tab that is showing,
//! the item selected in the sidebar and the active Environment. Plain data with no GPUI types, so tests reach it directly.
//!
//! A tab is only a pointer to a Request in the Collections. It holds an id, never a copy of the Request, so the Workspace
//! stays the one place Requests live. A Request can be open in one tab at most.

use ricochet_core::models::Workspace;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppState {
    /// Ids of the Requests open as tabs, in the order the tabs sit in the strip.
    open_tabs: Vec<String>,
    /// The tab that is showing. Always one of `open_tabs`, or None when no tab is open.
    active_tab: Option<String>,
    /// The item picked in the sidebar: a Request, a Folder or a Collection. None when nothing is picked.
    selected_item: Option<String>,
    /// The Environment in use, if any. Set from the Workspace when the state is made.
    active_environment_id: Option<String>,
}

impl AppState {
    /// A state with no tabs open and nothing selected, using the Workspace's active Environment.
    pub fn for_workspace(workspace: &Workspace) -> Self {
        Self { active_environment_id: workspace.active_environment_id.clone(), ..Self::default() }
    }

    /// The Request ids of the open tabs, in strip order.
    pub fn open_tabs(&self) -> &[String] {
        &self.open_tabs
    }

    pub fn active_tab(&self) -> Option<&str> {
        self.active_tab.as_deref()
    }

    pub fn selected_item(&self) -> Option<&str> {
        self.selected_item.as_deref()
    }

    pub fn active_environment_id(&self) -> Option<&str> {
        self.active_environment_id.as_deref()
    }

    pub fn is_open(&self, request_id: &str) -> bool {
        self.open_tabs.iter().any(|id| id == request_id)
    }

    /// Marks an item in the sidebar as selected. Nothing else changes: no tab opens and none is activated.
    pub fn select_item(&mut self, item_id: &str) {
        self.selected_item = Some(item_id.to_string());
    }

    /// Shows a Request in a tab. A tab that is already open is activated rather than repeated. The Request is selected too,
    /// so the sidebar and the tab always agree on what is showing.
    pub fn open_tab(&mut self, request_id: &str) {
        if !self.is_open(request_id) {
            self.open_tabs.push(request_id.to_string());
        }
        self.active_tab = Some(request_id.to_string());
        self.selected_item = Some(request_id.to_string());
    }

    /// Makes an open tab the one showing, and selects its Request. Returns false, and changes nothing, when no tab
    /// holds that Request.
    pub fn activate_tab(&mut self, request_id: &str) -> bool {
        if !self.is_open(request_id) {
            return false;
        }
        self.active_tab = Some(request_id.to_string());
        self.selected_item = Some(request_id.to_string());
        true
    }

    /// Closes the tab that holds a Request. Closing the tab that is showing moves to the tab that now sits in its place,
    /// or to the one before it when it was last, as the C# app does. Returns false when no tab holds that Request.
    pub fn close_tab(&mut self, request_id: &str) -> bool {
        let Some(index) = self.open_tabs.iter().position(|id| id == request_id) else {
            return false;
        };
        self.open_tabs.remove(index);

        if self.active_tab.as_deref() == Some(request_id) {
            let next = self.open_tabs.get(index.min(self.open_tabs.len().saturating_sub(1))).cloned();
            self.active_tab = next.clone();
            self.selected_item = next;
        }
        true
    }

    /// Picks a Request in the sidebar: it is selected and its tab is opened or activated. This is what clicking a
    /// Request row does. Folders and Collections use [`AppState::select_item`] instead.
    pub fn pick_request(&mut self, request_id: &str) {
        self.open_tab(request_id);
    }
}
