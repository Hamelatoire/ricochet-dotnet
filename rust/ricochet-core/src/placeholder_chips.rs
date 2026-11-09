//! Port of `PlaceholderChips.cs` (ADR 0009). Finds the `{{...}}` placeholders the editor shows as chips, what each
//! would resolve to, and what to suggest after `{{`. The text is never changed here: a chip exists only on screen,
//! so the file and the sent Request keep the plain `{{name}}` text.
//!
//! Resolving follows `variables::resolve`: a Variable wins over a built-in function of the same name.
//!
//! Offsets (`Chip::start`, `Chip::end`) are byte offsets into the UTF-8 text, not UTF-16 units as in C#. The UI
//! that will use them is not ported yet, and it must convert if its text control counts UTF-16.
//! Not ported: the UI (the chip drawing, key handling and tooltip display).

use std::sync::OnceLock;

use regex::Regex;

use crate::builtins;
use crate::models::KeyValue;

/// A `{{...}}` placeholder in a field's text, which the app draws as a chip (ADR 0009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// Offset of the opening braces in the field's text (a byte offset).
    pub start: usize,
    /// Length of the whole placeholder in bytes, braces included.
    pub length: usize,
    /// The name between the braces, trimmed.
    pub name: String,
    /// True for a built-in function (ADR 0008), false for a Variable.
    pub is_function: bool,
    /// True when the name has a value to send: a Variable in scope, or a function that runs.
    pub resolves: bool,
}

impl Chip {
    /// The offset just after the closing braces.
    pub fn end(&self) -> usize {
        self.start + self.length
    }
}

/// A name offered after typing `{{` (ADR 0009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipSuggestion {
    pub name: String,
    pub is_function: bool,
}

impl ChipSuggestion {
    /// The word the suggestion list shows beside the name.
    pub fn kind(&self) -> &'static str {
        if self.is_function { "function" } else { "Variable" }
    }
}

/// The placeholder pattern, matching C# `\{\{([^{}\r\n]+)\}\}`: a placeholder never spans a line break.
fn pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\{\{([^{}\r\n]+)\}\}").expect("placeholder pattern is valid"))
}

/// The chips in a field's text, in order. `variables` is every scope layered lowest first.
pub fn find(text: &str, variables: &[KeyValue]) -> Vec<Chip> {
    let names = resolvable_names(variables);
    pattern()
        .captures_iter(text)
        .map(|caps| {
            let whole = caps.get(0).expect("group 0 always exists");
            let name = caps[1].trim().to_string();
            let is_variable = names.contains(&name);
            let is_function = !is_variable && is_function(&name);
            let resolves = is_variable || (is_function && builtins::try_evaluate(&name).is_some());
            Chip { start: whole.start(), length: whole.len(), name, is_function, resolves }
        })
        .collect()
}

/// What to offer after `{{` for the text typed so far: Variables in scope first, then the built-in functions.
/// Disabled Variables, and Secrets with no value, are left out because they would not resolve.
pub fn suggest(variables: &[KeyValue], typed: &str) -> Vec<ChipSuggestion> {
    let prefix = typed.trim().to_lowercase();
    let mut suggestions = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for variable in variables.iter().filter(|v| is_resolvable(v)) {
        let name = variable.key.trim();
        if name.to_lowercase().starts_with(&prefix) && seen.insert(name.to_string()) {
            suggestions.push(ChipSuggestion { name: name.to_string(), is_function: false });
        }
    }

    suggestions.extend(
        builtins::EXAMPLES
            .iter()
            .filter(|example| example.to_lowercase().starts_with(&prefix))
            .map(|example| ChipSuggestion { name: (*example).to_string(), is_function: true }),
    );
    suggestions
}

/// The text shown when the pointer rests on a chip: the value, or only that a Secret is hidden. A Secret's value is
/// never returned (ADR 0009).
pub fn tip(chip: &Chip, variables: &[KeyValue]) -> String {
    let braces = format!("{{{{{}}}}}", chip.name);
    if chip.is_function {
        return if chip.resolves {
            format!("{braces}: built-in function, a new value on each send")
        } else {
            format!("{braces}: unknown function")
        };
    }

    // The same Variable the send uses: the last resolvable entry with this name.
    let found = variables
        .iter()
        .filter(|v| is_resolvable(v))
        .filter(|v| v.key.trim() == chip.name)
        .next_back();
    match found {
        None => format!("{braces}: no value"),
        Some(variable) if variable.is_secret => format!("{braces}: Secret, hidden"),
        Some(variable) => format!("{braces} = {}", variable.value),
    }
}

/// The chip the offset falls strictly inside, if any.
pub fn covering(chips: &[Chip], offset: usize) -> Option<&Chip> {
    chips.iter().find(|chip| chip.start < offset && offset < chip.end())
}

/// The chip that ends exactly at the offset, if any.
pub fn ending_at(chips: &[Chip], offset: usize) -> Option<&Chip> {
    chips.iter().find(|chip| chip.end() == offset)
}

/// The chip that starts exactly at the offset, if any.
pub fn starting_at(chips: &[Chip], offset: usize) -> Option<&Chip> {
    chips.iter().find(|chip| chip.start == offset)
}

fn is_function(name: &str) -> bool {
    name.contains('(') || builtins::is_postman_alias(name)
}

/// The same rule `variables::resolve` applies: enabled, and a Secret with no value counts as unset.
fn is_resolvable(variable: &KeyValue) -> bool {
    variable.enabled && !variable.key.is_empty() && !(variable.is_secret && variable.value.is_empty())
}

fn resolvable_names(variables: &[KeyValue]) -> std::collections::HashSet<String> {
    variables.iter().filter(|v| is_resolvable(v)).map(|v| v.key.trim().to_string()).collect()
}
