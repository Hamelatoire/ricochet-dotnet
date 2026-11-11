//! The light and dark theme (ADR 0013). Every colour the window draws is a field of [`Palette`], and each theme
//! defines all of them, so a missing colour is a build error rather than a visual one. Views name a colour with a
//! [`Tone`] and draw it with [`rgb`], which reads the palette that is active now. The choice is kept in `state.json`
//! as `theme`: "light", "dark", or null to follow the operating system, as `ThemeSettings.cs` stores it.

use std::sync::atomic::{AtomicBool, Ordering};

/// The theme choice on the Settings page: follow the system by default, or always use light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    /// The choices in the order the Settings page offers them.
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    /// Reads the value kept in state.json. Anything other than "light" or "dark" follows the system, as in C#.
    pub fn from_stored(stored: Option<&str>) -> Self {
        match stored {
            Some("light") => ThemeChoice::Light,
            Some("dark") => ThemeChoice::Dark,
            _ => ThemeChoice::System,
        }
    }

    /// The value to keep in state.json. None means follow the system.
    pub fn stored(self) -> Option<&'static str> {
        match self {
            ThemeChoice::System => None,
            ThemeChoice::Light => Some("light"),
            ThemeChoice::Dark => Some("dark"),
        }
    }

    /// The name shown on the Settings page.
    pub fn name(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    /// Whether this choice draws the dark palette, given whether the operating system is set to dark.
    pub fn is_dark(self, system_is_dark: bool) -> bool {
        match self {
            ThemeChoice::System => system_is_dark,
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
        }
    }
}

/// The Settings page's Appearance section, which is not in the Core catalog because the C# page draws it apart.
pub const THEME_SETTING_NAME: &str = "Theme";
pub const THEME_SETTING_HELP: &str = "Follow the system, or always use light or dark.";

/// Whether the theme setting matches the Settings search: every word must appear in its name or help text, ignoring
/// case, as the catalog's search does. A blank query matches.
pub fn theme_setting_matches(query: &str) -> bool {
    let haystack = format!("appearance {THEME_SETTING_NAME} {THEME_SETTING_HELP}").to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
}

/// Every colour the window uses, as 0xRRGGBB, except `selection`, which is 0xRRGGBBAA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub window_bg: u32,
    pub chrome_bg: u32,
    pub field_bg: u32,
    pub border: u32,
    pub row_hover: u32,
    pub row_selected: u32,
    pub accent: u32,
    pub accent_hover: u32,
    /// Text on an accent-filled button.
    pub on_accent: u32,
    pub chip_bg: u32,
    pub chip_text: u32,
    /// A placeholder chip whose Variable has no value.
    pub chip_unresolved_bg: u32,
    pub text: u32,
    pub muted: u32,
    pub faint: u32,
    pub success: u32,
    pub warning: u32,
    pub error: u32,
    pub status_redirect: u32,
    pub status_client_error: u32,
    pub method_get: u32,
    pub method_post: u32,
    pub method_put: u32,
    pub method_patch: u32,
    pub method_delete: u32,
    pub method_other: u32,
    pub json_key: u32,
    pub json_string: u32,
    pub json_number: u32,
    pub json_literal: u32,
    pub json_plain: u32,
    pub diff_removed_bg: u32,
    pub diff_added_bg: u32,
    pub diff_changed_bg: u32,
    pub caret: u32,
    pub selection: u32,
    pub placeholder: u32,
}

/// The dark theme, as the Rust app has drawn it so far.
pub const DARK: Palette = Palette {
    window_bg: 0x1a1c20,
    chrome_bg: 0x1f2227,
    field_bg: 0x15171a,
    border: 0x2e323a,
    row_hover: 0x2a2e36,
    row_selected: 0x25354f,
    accent: 0x4c8dff,
    accent_hover: 0x6aa1ff,
    on_accent: 0xffffff,
    chip_bg: 0x22344f,
    chip_text: 0x8ab4ff,
    chip_unresolved_bg: 0x4a3a1a,
    text: 0xe6e8eb,
    muted: 0x9aa1ab,
    faint: 0x6b727d,
    success: 0x6fcf97,
    warning: 0xf2c94c,
    error: 0xeb5757,
    status_redirect: 0x56b6f2,
    status_client_error: 0xf2994a,
    method_get: 0x6fcf97,
    method_post: 0xf2c94c,
    method_put: 0x56b6f2,
    method_patch: 0xbb87f5,
    method_delete: 0xeb5757,
    method_other: 0x9aa1ab,
    json_key: 0x8ab4ff,
    json_string: 0x6fcf97,
    json_number: 0xf2c94c,
    json_literal: 0xbb87f5,
    json_plain: 0xe6e8eb,
    diff_removed_bg: 0x3d1f24,
    diff_added_bg: 0x1f3d28,
    diff_changed_bg: 0x3d3520,
    caret: 0x4c8dff,
    selection: 0x4c8dff55,
    placeholder: 0x6b727d,
};

/// The light theme, with the C# app's light colours (`App.axaml`). The accent stays blue, as in the Rust dark
/// theme, using the C# light Info blue, so the two themes read as one app.
pub const LIGHT: Palette = Palette {
    window_bg: 0xf3f4f7,       // AppBackground
    chrome_bg: 0xffffff,       // SurfaceBackground
    field_bg: 0xeef0f4,        // TextControlBackground
    border: 0xcdd2db,          // StrongLine
    row_hover: 0xe3e6ec,       // TextControlBackgroundPointerOver
    row_selected: 0xe6effd,    // InfoBackground
    accent: 0x1d4ed8,          // Info
    accent_hover: 0x1e40af,
    on_accent: 0xffffff,
    chip_bg: 0xe6effd,         // InfoBackground
    chip_text: 0x1d4ed8,       // Info
    chip_unresolved_bg: 0xfef3dc, // WarningBackground
    text: 0x15171c,            // TextPrimary
    muted: 0x5d6470,           // TextMuted
    faint: 0x646b77,           // TextControlPlaceholderForeground
    success: 0x166534,
    warning: 0xb45309,
    error: 0xb42318,           // Failure
    status_redirect: 0x1d4ed8, // Info
    status_client_error: 0xc2410c,
    method_get: 0x15803d,
    method_post: 0xb45309,
    method_put: 0x1d4ed8,
    method_patch: 0x7e22ce,
    method_delete: 0xb91c1c,
    method_other: 0x5d6470,
    json_key: 0x0369a1,
    json_string: 0x15803d,
    json_number: 0xc2410c,
    json_literal: 0x7c3aed,
    json_plain: 0x27272a,
    diff_removed_bg: 0xfde4e1, // FailureBackground
    diff_added_bg: 0xddf6e6,   // SuccessBackground
    diff_changed_bg: 0xfef3dc, // WarningBackground
    caret: 0x1d4ed8,
    selection: 0x1d4ed833,
    placeholder: 0x646b77,
};

/// A named colour: which field of the palette to draw. Views keep these as constants and pass them to [`rgb`].
pub type Tone = fn(&Palette) -> u32;

static DARK_ACTIVE: AtomicBool = AtomicBool::new(true);

/// Makes the dark or light palette the one drawn from now on. The window must be redrawn to show it.
pub fn set_dark(dark: bool) {
    DARK_ACTIVE.store(dark, Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK_ACTIVE.load(Ordering::Relaxed)
}

/// The palette drawn now.
pub fn active() -> &'static Palette {
    if is_dark() { &DARK } else { &LIGHT }
}

/// A colour of the active palette, for GPUI.
pub fn rgb(tone: Tone) -> gpui::Rgba {
    gpui::rgb(tone(active()))
}
