//! The Settings page's logic (ADR 0007, ADR 0069), kept out of the window so tests reach it: reading what was typed
//! into each field, and which sections and settings a search shows. The window applies each change at once, saves it
//! in `state.json`, and sends with it from then on, as the C# settings page does.

use ricochet_core::personal_state::PersonalSettings;
use ricochet_core::settings_catalog::{self, SECTIONS, SettingItem, SettingSection};

/// The longest request timeout, in seconds, as the catalog's help text gives it.
pub const MAX_TIMEOUT_SECONDS: i32 = 3600;

/// A field that takes typed text. Toggles change on a click and need no parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextSetting {
    Timeout,
    MaxResponse,
    Proxy,
    ClientCertificate,
}

/// Applies what was typed into one field. Returns the problem to show beside the field when the text cannot be
/// used, in which case the settings are left as they were.
///
/// # Errors
/// A message for the user, for a number that is not a whole number in range.
pub fn apply_text(settings: &mut PersonalSettings, field: TextSetting, text: &str) -> Result<(), String> {
    match field {
        TextSetting::Timeout => {
            settings.request_timeout_seconds = parse_number(text, 1, MAX_TIMEOUT_SECONDS, "The timeout")?;
        }
        TextSetting::MaxResponse => {
            settings.max_response_megabytes = parse_number(text, 1, i32::MAX / 1024, "The largest response")?;
        }
        TextSetting::Proxy => settings.proxy_url = text.trim().to_string(),
        TextSetting::ClientCertificate => settings.client_certificate_path = text.trim().to_string(),
    }
    Ok(())
}

/// The text a field starts with.
pub fn text_of(settings: &PersonalSettings, field: TextSetting) -> String {
    match field {
        TextSetting::Timeout => settings.request_timeout_seconds.to_string(),
        TextSetting::MaxResponse => settings.max_response_megabytes.to_string(),
        TextSetting::Proxy => settings.proxy_url.clone(),
        TextSetting::ClientCertificate => settings.client_certificate_path.clone(),
    }
}

fn parse_number(text: &str, min: i32, max: i32, what: &str) -> Result<i32, String> {
    match text.trim().parse::<i32>() {
        Ok(value) if (min..=max).contains(&value) => Ok(value),
        _ => Err(format!("{what} must be a whole number between {min} and {max}.")),
    }
}

/// The sections a search shows, each with its matching settings, in page order. An empty search shows everything.
pub fn visible_sections(query: &str) -> Vec<(SettingSection, Vec<&'static SettingItem>)> {
    let query = query.trim();
    let matches = settings_catalog::search((!query.is_empty()).then_some(query));
    SECTIONS
        .iter()
        .filter_map(|section| {
            let items: Vec<&'static SettingItem> = matches.iter().copied().filter(|item| item.section == *section).collect();
            (!items.is_empty()).then_some((*section, items))
        })
        .collect()
}
