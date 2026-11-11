//! The theme choice and palettes (ADR 0013), as `ThemeSettings.cs` defines the choice.

use ricochet_app::theme::{self, DARK, LIGHT, ThemeChoice, theme_setting_matches};

#[test]
fn the_stored_value_is_read_as_in_the_csharp_app() {
    assert_eq!(ThemeChoice::from_stored(Some("light")), ThemeChoice::Light);
    assert_eq!(ThemeChoice::from_stored(Some("dark")), ThemeChoice::Dark);
    assert_eq!(ThemeChoice::from_stored(None), ThemeChoice::System);
    // Anything else follows the system, as the C# switch does.
    assert_eq!(ThemeChoice::from_stored(Some("Dark")), ThemeChoice::System);
    assert_eq!(ThemeChoice::from_stored(Some("")), ThemeChoice::System);
}

#[test]
fn each_choice_is_stored_and_read_back_unchanged() {
    for choice in ThemeChoice::ALL {
        assert_eq!(ThemeChoice::from_stored(choice.stored()), choice);
    }
    assert_eq!(ThemeChoice::System.stored(), None);
    assert_eq!(ThemeChoice::Light.stored(), Some("light"));
    assert_eq!(ThemeChoice::Dark.stored(), Some("dark"));
}

#[test]
fn the_choices_are_offered_in_the_csharp_order_and_names() {
    let names: Vec<&str> = ThemeChoice::ALL.iter().map(|choice| choice.name()).collect();
    assert_eq!(names, ["System", "Light", "Dark"]);
    assert_eq!(ThemeChoice::default(), ThemeChoice::System);
}

#[test]
fn system_follows_the_operating_system_and_the_others_override_it() {
    assert!(ThemeChoice::System.is_dark(true));
    assert!(!ThemeChoice::System.is_dark(false));
    assert!(ThemeChoice::Dark.is_dark(false));
    assert!(!ThemeChoice::Light.is_dark(true));
}

#[test]
fn the_active_palette_follows_set_dark() {
    theme::set_dark(false);
    assert_eq!(*theme::active(), LIGHT);
    theme::set_dark(true);
    assert_eq!(*theme::active(), DARK);
}

#[test]
fn the_light_palette_uses_the_csharp_light_colours() {
    assert_eq!(LIGHT.window_bg, 0xf3f4f7);
    assert_eq!(LIGHT.text, 0x15171c);
    assert_eq!(LIGHT.muted, 0x5d6470);
    assert_eq!(LIGHT.method_get, 0x15803d);
    assert_eq!(LIGHT.method_delete, 0xb91c1c);
    assert_ne!(LIGHT.text, DARK.text);
}

#[test]
fn the_theme_setting_matches_search_words_in_its_name_or_help() {
    assert!(theme_setting_matches(""));
    assert!(theme_setting_matches("theme"));
    assert!(theme_setting_matches("DARK system"));
    assert!(theme_setting_matches("appearance"));
    assert!(!theme_setting_matches("proxy"));
    assert!(!theme_setting_matches("theme proxy"));
}
