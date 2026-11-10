//! Port of `SettingsCatalog.cs` (ADR 0007, ADR 0069). The sections and settings of the personal Settings page:
//! their keys, names and help text. Search matches the name and help text of each setting, so the page can filter
//! without knowing what the words are. The settings page UI is not ported.
//!
//! The C# catalog carries no defaults, ranges or parsing rules, so none are invented here. Ranges such as the
//! request timeout's "Between 1 and 3600" appear only in the help text, as they do in C#.

/// A group on the Settings page, such as Requests or Network (ADR 0069).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSection {
    pub key: &'static str,
    pub title: &'static str,
}

/// One setting on the Settings page. `key` ties it to its control in the app, and `name` and `help_text` are
/// what search matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingItem {
    pub key: &'static str,
    pub section: SettingSection,
    pub name: &'static str,
    pub help_text: &'static str,
}

pub const REQUESTS: SettingSection = SettingSection { key: "requests", title: "Requests" };
pub const NETWORK: SettingSection = SettingSection { key: "network", title: "Network" };
pub const RESPONSES: SettingSection = SettingSection { key: "responses", title: "Responses" };
pub const PRIVACY: SettingSection = SettingSection { key: "privacy", title: "Privacy" };
pub const WORKSPACE: SettingSection = SettingSection { key: "workspace", title: "Workspace" };

/// Sections in the order they appear on the page.
pub const SECTIONS: &[SettingSection] = &[REQUESTS, NETWORK, RESPONSES, PRIVACY, WORKSPACE];

/// Every setting, in the order it appears on the page.
pub const ITEMS: &[SettingItem] = &[
    SettingItem {
        key: "request-timeout",
        section: REQUESTS,
        name: "Request timeout (seconds)",
        help_text: "A request that takes longer is cancelled. Between 1 and 3600.",
    },
    SettingItem {
        key: "follow-redirects",
        section: REQUESTS,
        name: "Follow redirects",
        help_text: "Send the request again to the address a server redirects to.",
    },
    SettingItem {
        key: "verify-tls",
        section: REQUESTS,
        name: "Verify TLS certificates",
        help_text: "Turning this off accepts any server certificate. Anyone on the network could read or change your requests, including credentials. Keep it on unless you have a reason not to.",
    },
    SettingItem {
        key: "proxy-address",
        section: NETWORK,
        name: "Proxy address",
        help_text: "Send requests through this HTTP proxy, such as proxy.example.com:8080. Empty uses the operating system's proxy. Proxy logins are not supported yet.",
    },
    SettingItem {
        key: "client-certificate-file",
        section: NETWORK,
        name: "Client certificate file",
        help_text: "A .pfx or .p12 file sent to servers that ask for one. The file stays where it is; only its path is saved.",
    },
    SettingItem {
        key: "client-certificate-password",
        section: NETWORK,
        name: "Client certificate password",
        help_text: "Kept in your operating system's secure storage, never in the workspace. Leave empty if the file has no password.",
    },
    SettingItem {
        key: "max-response-size",
        section: RESPONSES,
        name: "Largest response to keep (MB)",
        help_text: "A longer response is cut off at this size and marked as truncated.",
    },
    SettingItem {
        key: "wrap-response",
        section: RESPONSES,
        name: "Wrap long lines",
        help_text: "Wrap long lines in the response body. The Wrap button in the response uses this too.",
    },
    SettingItem {
        key: "crash-log-folder",
        section: PRIVACY,
        name: "Crash log folder",
        help_text: "After a crash, the app writes a log here. It stays on this machine and is never sent anywhere. Attach it to a bug report yourself if you choose. Exception messages and request data are left out; file paths may include your user name.",
    },
    SettingItem {
        key: "workspace-folder",
        section: WORKSPACE,
        name: "Workspace folder",
        help_text: "Open a workspace folder, or copy this workspace into a new one, such as a git repository.",
    },
];

/// The settings that match `query`. Every word of the query must appear in the setting's name or help text,
/// ignoring case. A blank or missing query matches everything.
pub fn search(query: Option<&str>) -> Vec<&'static SettingItem> {
    let words: Vec<String> = query
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();

    if words.is_empty() {
        return ITEMS.iter().collect();
    }

    ITEMS
        .iter()
        .filter(|item| {
            let name = item.name.to_lowercase();
            let help = item.help_text.to_lowercase();
            words.iter().all(|word| name.contains(word.as_str()) || help.contains(word.as_str()))
        })
        .collect()
}
