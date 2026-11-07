using System.Net;
using System.Text;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>
/// Renders a Collection as one self-contained HTML page for people who do not use Ricochet (ADR 0028).
/// The page has no scripts, no external styles and no network calls. Secret values and Secret Variables
/// are written as <see cref="Masked"/>, and every piece of text is HTML-escaped.
/// Hidden folders and Requests (ADR 0032) are left out.
/// </summary>
public static partial class CollectionDocumentation
{
    /// <summary>What a Secret looks like in the page. It stands in for the value and never reveals its length.</summary>
    public const string Masked = "***";

    private const int FirstHeadingLevel = 2;
    private const int LastHeadingLevel = 6;

    [GeneratedRegex(@"[?&](api[_-]?key|access[_-]?token|auth[_-]?token|token|password|passwd|secret)=([^&#]*)", RegexOptions.IgnoreCase)]
    private static partial Regex SensitiveQueryParameter();

    private const string Css = """
        :root { color-scheme: light dark; --fg: #1b1f24; --muted: #59636e; --line: #d0d7de; --panel: #f6f8fa; }
        @media (prefers-color-scheme: dark) { :root { --fg: #e6edf3; --muted: #9198a1; --line: #3d444d; --panel: #161b22; } }
        body { margin: 0; background: transparent; color: var(--fg); font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }
        main { max-width: 960px; margin: 0 auto; padding: 24px 16px 64px; }
        h1 { font-size: 28px; margin: 0 0 16px; }
        h2, h3, h4, h5, h6 { margin: 24px 0 8px; }
        section.folder { border-left: 2px solid var(--line); padding-left: 16px; margin-top: 16px; }
        article.request { border: 1px solid var(--line); border-radius: 6px; padding: 12px 16px; margin: 12px 0; }
        .method { display: inline-block; min-width: 4em; font-weight: 600; font-family: ui-monospace, Consolas, monospace; }
        .url { font-family: ui-monospace, Consolas, monospace; word-break: break-all; margin: 4px 0 8px; }
        .label { color: var(--muted); font-size: 13px; text-transform: uppercase; letter-spacing: .04em; margin: 12px 0 4px; }
        table { border-collapse: collapse; width: 100%; font-size: 14px; }
        th, td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--line); vertical-align: top; word-break: break-all; }
        th { color: var(--muted); font-weight: 600; }
        pre { background: var(--panel); border-radius: 6px; padding: 12px; overflow-x: auto; white-space: pre-wrap; word-break: break-word; }
        code { font-family: ui-monospace, Consolas, monospace; }
        dl { display: grid; grid-template-columns: max-content 1fr; gap: 4px 12px; margin: 4px 0; }
        dt { color: var(--muted); }
        dd { margin: 0; word-break: break-all; }
        """;

    /// <summary>Returns the whole page as one string. The output depends only on the Collection, so the same input gives the same page.</summary>
    public static string Render(Collection collection)
    {
        var sb = new StringBuilder();
        sb.Append("<!DOCTYPE html>\n");
        sb.Append("<html lang=\"en\">\n<head>\n");
        sb.Append("<meta charset=\"utf-8\">\n");
        sb.Append("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
        sb.Append("<meta name=\"generator\" content=\"Ricochet\">\n");
        sb.Append("<title>").Append(Encode(collection.Name)).Append("</title>\n");
        sb.Append("<style>\n").Append(Css).Append("\n</style>\n");
        sb.Append("</head>\n<body>\n<main>\n");
        sb.Append("<h1>").Append(Encode(collection.Name)).Append("</h1>\n");

        AppendVariables(sb, collection.Variables);

        var hidden = new HashSet<string>(collection.HiddenItemIds ?? []);
        AppendItems(sb, collection.Items, FirstHeadingLevel, hidden);

        sb.Append("</main>\n</body>\n</html>\n");
        return sb.ToString();
    }

    private static void AppendVariables(StringBuilder sb, IReadOnlyList<KeyValue> variables)
    {
        var enabled = variables.Where(v => v.Enabled && v.Key.Length > 0).ToList();
        if (enabled.Count == 0)
            return;

        sb.Append("<section>\n<h2>Variables</h2>\n");
        AppendTable(sb, ["Name", "Value"], enabled.Select(v => (v.Key, v.IsSecret ? Masked : v.Value)));
        sb.Append("</section>\n");
    }

    private static void AppendItems(StringBuilder sb, IReadOnlyList<CollectionItem> items, int level, HashSet<string> hidden)
    {
        foreach (var item in items)
        {
            if (hidden.Contains(item.Id))
                continue;

            if (item.Request is null)
                AppendFolder(sb, item, level, hidden);
            else
                AppendRequest(sb, item, item.Request, level);
        }
    }

    private static void AppendFolder(StringBuilder sb, CollectionItem folder, int level, HashSet<string> hidden)
    {
        var heading = Heading(level);
        sb.Append("<section class=\"folder\">\n");
        sb.Append('<').Append(heading).Append('>').Append(Encode(folder.Name)).Append("</").Append(heading).Append(">\n");
        AppendItems(sb, folder.Children, level + 1, hidden);
        sb.Append("</section>\n");
    }

    private static void AppendRequest(StringBuilder sb, CollectionItem item, ApiRequest request, int level)
    {
        var heading = Heading(level);
        var title = request.Name.Length > 0 ? request.Name : item.Name.Length > 0 ? item.Name : request.Url;

        sb.Append("<article class=\"request\">\n");
        sb.Append('<').Append(heading).Append("><span class=\"method\">").Append(Encode(request.Method))
            .Append("</span> ").Append(Encode(title)).Append("</").Append(heading).Append(">\n");
        sb.Append("<p class=\"url\"><code>").Append(Encode(MaskUrl(request.Url))).Append("</code></p>\n");

        var headers = request.Headers.Where(h => h.Enabled && h.Key.Length > 0).ToList();
        if (headers.Count > 0)
        {
            sb.Append("<div class=\"label\">Headers</div>\n");
            AppendTable(sb, ["Name", "Value"], headers.Select(h => (h.Key, IsSecretHeader(h) ? Masked : h.Value)));
        }

        AppendAuth(sb, request.Auth);

        if (request.Body.Length > 0)
        {
            sb.Append("<div class=\"label\">Body</div>\n");
            sb.Append("<pre>").Append(Encode(request.Body)).Append("</pre>\n");
        }

        sb.Append("</article>\n");
    }

    private static void AppendAuth(StringBuilder sb, RequestAuth? auth)
    {
        if (auth is null || auth.Kind is AuthKinds.None or "")
            return;

        sb.Append("<div class=\"label\">Authentication</div>\n<dl>\n");
        sb.Append("<dt>Type</dt><dd>").Append(Encode(auth.Kind)).Append("</dd>\n");

        switch (auth.Kind)
        {
            case AuthKinds.Bearer:
                AppendField(sb, "Token", MaskCredential(auth.Token));
                break;
            case AuthKinds.Basic:
                AppendField(sb, "Username", auth.Username);
                AppendField(sb, "Password", MaskCredential(auth.Password));
                break;
            case AuthKinds.ApiKey:
                AppendField(sb, "Key name", auth.KeyName);
                AppendField(sb, "Sent in", auth.KeyInQuery ? "query parameter" : "header");
                AppendField(sb, "Key value", MaskCredential(auth.KeyValue));
                break;
        }

        sb.Append("</dl>\n");
    }

    private static void AppendField(StringBuilder sb, string label, string value)
    {
        if (value.Length == 0)
            return;

        sb.Append("<dt>").Append(Encode(label)).Append("</dt><dd>").Append(Encode(value)).Append("</dd>\n");
    }

    private static void AppendTable(StringBuilder sb, string[] columns, IEnumerable<(string Name, string Value)> rows)
    {
        sb.Append("<table>\n<thead><tr>");
        foreach (var column in columns)
            sb.Append("<th>").Append(Encode(column)).Append("</th>");
        sb.Append("</tr></thead>\n<tbody>\n");
        foreach (var (name, value) in rows)
            sb.Append("<tr><td><code>").Append(Encode(name)).Append("</code></td><td><code>").Append(Encode(value)).Append("</code></td></tr>\n");
        sb.Append("</tbody>\n</table>\n");
    }

    /// <summary>A credential is hidden when it holds a real value. A {{placeholder}} names a variable and is safe to show.</summary>
    private static string MaskCredential(string value) => SecretRules.IsLiteral(value) ? Masked : value;

    private static bool IsSecretHeader(KeyValue header) =>
        header.IsSecret || (SecretRules.IsSensitiveHeader(header.Key) && SecretRules.IsLiteral(header.Value));

    private static string MaskUrl(string url) =>
        SensitiveQueryParameter().Replace(url, m => SecretRules.IsLiteral(m.Groups[2].Value)
            ? $"{m.Value[0]}{m.Groups[1].Value}={Masked}"
            : m.Value);

    private static string Heading(int level) => $"h{Math.Min(level, LastHeadingLevel)}";

    private static string Encode(string text) => WebUtility.HtmlEncode(text);
}
