namespace Ricochet.Core;

/// <summary>A group on the Settings page, such as Requests or Network (ADR 0069).</summary>
public sealed record SettingSection(string Key, string Title);

/// <summary>
/// One setting on the Settings page. <see cref="Key"/> ties it to its control in the app, and
/// <see cref="Name"/> and <see cref="HelpText"/> are what search matches.
/// </summary>
public sealed record SettingItem(string Key, SettingSection Section, string Name, string HelpText);

/// <summary>
/// The sections and settings of the personal Settings page (ADR 0007, ADR 0069). Search matches
/// the name and help text of each setting, so the page can filter without knowing what the words are.
/// </summary>
public static class SettingsCatalog
{
    public static readonly SettingSection Requests = new("requests", "Requests");
    public static readonly SettingSection Network = new("network", "Network");
    public static readonly SettingSection Responses = new("responses", "Responses");
    public static readonly SettingSection Privacy = new("privacy", "Privacy");
    public static readonly SettingSection Workspace = new("workspace", "Workspace");

    /// <summary>Sections in the order they appear on the page.</summary>
    public static readonly IReadOnlyList<SettingSection> Sections = [Requests, Network, Responses, Privacy, Workspace];

    public static readonly IReadOnlyList<SettingItem> Items =
    [
        new("request-timeout", Requests, "Request timeout (seconds)", "A request that takes longer is cancelled. Between 1 and 3600."),
        new("follow-redirects", Requests, "Follow redirects", "Send the request again to the address a server redirects to."),
        new("verify-tls", Requests, "Verify TLS certificates", "Turning this off accepts any server certificate. Anyone on the network could read or change your requests, including credentials. Keep it on unless you have a reason not to."),
        new("proxy-address", Network, "Proxy address", "Send requests through this HTTP proxy, such as proxy.example.com:8080. Empty uses the operating system's proxy. Proxy logins are not supported yet."),
        new("client-certificate-file", Network, "Client certificate file", "A .pfx or .p12 file sent to servers that ask for one. The file stays where it is; only its path is saved."),
        new("client-certificate-password", Network, "Client certificate password", "Kept in your operating system's secure storage, never in the workspace. Leave empty if the file has no password."),
        new("max-response-size", Responses, "Largest response to keep (MB)", "A longer response is cut off at this size and marked as truncated."),
        new("wrap-response", Responses, "Wrap long lines", "Wrap long lines in the response body. The Wrap button in the response uses this too."),
        new("crash-log-folder", Privacy, "Crash log folder", "After a crash, the app writes a log here. It stays on this machine and is never sent anywhere. Attach it to a bug report yourself if you choose. Exception messages and request data are left out; file paths may include your user name."),
        new("workspace-folder", Workspace, "Workspace folder", "Open a workspace folder, or copy this workspace into a new one, such as a git repository."),
    ];

    /// <summary>
    /// The settings that match <paramref name="query"/>. Every word of the query must appear in the
    /// setting's name or help text, ignoring case. An empty or blank query matches everything.
    /// </summary>
    public static IReadOnlyList<SettingItem> Search(string? query)
    {
        var words = (query ?? string.Empty).Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
        if (words.Length == 0)
        {
            return Items;
        }

        return Items.Where(item => words.All(word =>
            item.Name.Contains(word, StringComparison.OrdinalIgnoreCase)
            || item.HelpText.Contains(word, StringComparison.OrdinalIgnoreCase))).ToList();
    }
}
