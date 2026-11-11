//! The application state the window reads and changes: the Requests open as tabs (ADR 0041), the tab that is showing,
//! the item selected in the sidebar and the active Environment. Plain data with no GPUI types, so tests reach it directly.
//!
//! A tab is only a pointer to a Request in the Collections. It holds an id, never a copy of the Request, so the Workspace
//! stays the one place Requests live. A Request can be open in one tab at most.

use std::collections::BTreeSet;

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
    /// Ids of the Collections and Folders folded shut in the sidebar. Everything else is open.
    collapsed: BTreeSet<String>,
    /// Ids of the sidebar items whose Requests have changes that are not saved yet.
    unsaved: BTreeSet<String>,
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

    /// Records that a Request has a change that is not on disk yet.
    pub fn mark_unsaved(&mut self, item_id: &str) {
        self.unsaved.insert(item_id.to_string());
    }

    pub fn is_unsaved(&self, item_id: &str) -> bool {
        self.unsaved.contains(item_id)
    }

    pub fn has_unsaved(&self) -> bool {
        !self.unsaved.is_empty()
    }

    pub fn unsaved_count(&self) -> usize {
        self.unsaved.len()
    }

    /// Forgets every unsaved mark, after a save wrote the whole Workspace.
    pub fn clear_unsaved(&mut self) {
        self.unsaved.clear();
    }

    pub fn is_collapsed(&self, item_id: &str) -> bool {
        self.collapsed.contains(item_id)
    }

    pub fn collapsed(&self) -> &BTreeSet<String> {
        &self.collapsed
    }

    /// Folds a Collection or Folder shut in the sidebar, or opens it again.
    pub fn toggle_collapsed(&mut self, item_id: &str) {
        if !self.collapsed.remove(item_id) {
            self.collapsed.insert(item_id.to_string());
        }
    }

    /// Makes the Environment with this id the active one, or none. Changes the window only, not any file.
    pub fn set_active_environment(&mut self, environment_id: Option<&str>) {
        self.active_environment_id = environment_id.map(str::to_string);
    }

    /// Moves to the next Environment in the Workspace's order: none, then each in turn, then none again.
    /// An active Environment that no longer exists counts as none. This changes the window only, not any file.
    pub fn cycle_environment(&mut self, workspace: &Workspace) {
        let ids: Vec<&str> = workspace.environments.iter().map(|environment| environment.id.as_str()).collect();
        let current = self.active_environment_id.as_deref().and_then(|id| ids.iter().position(|other| *other == id));
        let next = match current {
            None => ids.first(),
            Some(index) => ids.get(index + 1),
        };
        self.active_environment_id = next.map(|id| id.to_string());
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

impl AppState {
    /// Opens the Requests that were open last time, in their order, and shows the one that was showing, or the first
    /// when that one is gone (ADR 0041). Ids of Requests no longer in the Workspace, and repeats, are skipped, as
    /// `RestoreTabs` does in the C# app. Tabs already open stay open.
    pub fn restore_tabs(&mut self, workspace: &Workspace, open_tabs: &[String], active_tab: Option<&str>) {
        for id in open_tabs {
            if !self.is_open(id) && crate::tree::find_request(workspace, id).is_some() {
                self.open_tabs.push(id.clone());
            }
        }
        let active = active_tab.filter(|id| self.is_open(id)).map(str::to_string).or_else(|| self.open_tabs.first().cloned());
        if let Some(active) = active {
            self.activate_tab(&active);
        }
    }

    /// Closes every open tab. Nothing is selected afterwards.
    pub fn close_all_tabs(&mut self) {
        self.open_tabs.clear();
        self.active_tab = None;
        self.selected_item = None;
    }

    /// Closes every tab but the one showing. Does nothing when no tab is showing.
    pub fn close_other_tabs(&mut self) {
        let Some(active) = self.active_tab.clone() else {
            return;
        };
        self.open_tabs.retain(|id| *id == active);
    }

    /// Makes the tab after (`step` 1) or before (`step` -1) the showing one active, wrapping round. Returns false when
    /// no tab is open.
    pub fn step_tab(&mut self, step: isize) -> bool {
        if self.open_tabs.is_empty() {
            return false;
        }
        let len = self.open_tabs.len() as isize;
        let current = self.active_tab.as_ref().and_then(|active| self.open_tabs.iter().position(|id| id == active)).unwrap_or(0) as isize;
        let next = self.open_tabs[(current + step).rem_euclid(len) as usize].clone();
        self.activate_tab(&next)
    }

    /// Closes the tab that is showing. Returns false when no tab is showing.
    pub fn close_active_tab(&mut self) -> bool {
        match self.active_tab.clone() {
            Some(active) => self.close_tab(&active),
            None => false,
        }
    }
}
