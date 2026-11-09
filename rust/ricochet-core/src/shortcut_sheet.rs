//! Port of `ShortcutSheet.cs` (ADR 0067). The table of shortcuts behind the cheat sheet, grouped by the area where
//! each one works. The keys are written for the operating system: Cmd on macOS, Ctrl elsewhere (ADR 0040).
//! The sheet's UI is not ported. The C# has no search, so none is added here.

/// The part of the app where a shortcut works, used to group the cheat sheet. Declaration order is the order the
/// sheet shows the areas in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShortcutArea {
    General,
    Request,
    Palette,
}

/// One shortcut as the cheat sheet shows it: the keys, what they do, and where they work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutEntry {
    pub area: ShortcutArea,
    pub keys: String,
    pub action: String,
}

impl ShortcutEntry {
    fn new(area: ShortcutArea, keys: impl Into<String>, action: &str) -> Self {
        Self {
            area,
            keys: keys.into(),
            action: action.to_string(),
        }
    }
}

/// Every shortcut the app has, in the order the sheet shows them within each area.
pub fn entries(mac_os: bool) -> Vec<ShortcutEntry> {
    let modifier = if mac_os { "Cmd" } else { "Ctrl" };
    let key = |suffix: &str| format!("{modifier}+{suffix}");

    vec![
        ShortcutEntry::new(ShortcutArea::General, "F1", "Show this list of shortcuts"),
        ShortcutEntry::new(ShortcutArea::General, key("/"), "Show this list of shortcuts"),
        ShortcutEntry::new(ShortcutArea::General, key("K"), "Open the command palette"),
        ShortcutEntry::new(ShortcutArea::General, key("P"), "Find a request"),
        ShortcutEntry::new(ShortcutArea::General, key("N"), "New request"),
        ShortcutEntry::new(ShortcutArea::General, key("S"), "Save"),
        ShortcutEntry::new(ShortcutArea::General, key("W"), "Close the active tab"),
        ShortcutEntry::new(ShortcutArea::Request, key("Enter"), "Send the request"),
        ShortcutEntry::new(ShortcutArea::Request, key("L"), "Focus the URL"),
        ShortcutEntry::new(ShortcutArea::Request, key("E"), "Switch environment"),
        ShortcutEntry::new(ShortcutArea::Palette, "Up and Down", "Choose an entry"),
        ShortcutEntry::new(ShortcutArea::Palette, "Enter", "Run the chosen entry"),
        ShortcutEntry::new(ShortcutArea::Palette, "Escape", "Close the palette or this list"),
    ]
}

/// The entries grouped by area, in the order the areas are declared, with empty areas left out. Each group keeps
/// the entries in their table order.
pub fn groups(mac_os: bool) -> Vec<(ShortcutArea, Vec<ShortcutEntry>)> {
    let mut groups: Vec<(ShortcutArea, Vec<ShortcutEntry>)> = Vec::new();
    let mut entries = entries(mac_os);
    entries.sort_by_key(|entry| entry.area);

    for entry in entries {
        match groups.last_mut() {
            Some((area, group)) if *area == entry.area => group.push(entry),
            _ => groups.push((entry.area, vec![entry])),
        }
    }
    groups
}

/// The heading shown above each area.
pub fn title(area: ShortcutArea) -> &'static str {
    match area {
        ShortcutArea::General => "Everywhere",
        ShortcutArea::Request => "In a request",
        ShortcutArea::Palette => "In the palette and this list",
    }
}
