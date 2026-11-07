using System.Text;

namespace Ricochet.Core;

/// <summary>
/// Shows a resolved Request as a curl command (ADR 0022). The input must already have its Variables applied, so the
/// command is the one that would be sent. Secret values are masked unless the caller asks to reveal them (ADR 0016).
/// </summary>
public static class CurlGenerator
{
    public const string Mask = "***";

    /// <param name="resolved">The Request with Variables and built-in functions already applied.</param>
    /// <param name="secrets">Values known to be secret, such as Secret Variables. Auth values are always included.</param>
    /// <param name="reveal">True to show the real values.</param>
    public static string Generate(ApiRequest resolved, IEnumerable<string> secrets, bool reveal = false)
    {
        List<string> masked = reveal ? [] : MaskList(resolved, secrets);
        // A plain GET keeps the URL on the curl line. Any other method goes there instead, and the URL moves to the next line.
        var url = Quote(Redact(UrlWithQueryAuth(resolved), masked));
        var isGet = string.Equals(resolved.Method, "GET", StringComparison.OrdinalIgnoreCase);
        var first = isGet ? $"curl {url}" : $"curl -X {resolved.Method.ToUpperInvariant()}";
        var parts = new List<string>();
        if (!isGet)
            parts.Add(url);

        var hasContentType = resolved.Headers.Any(h => h.Enabled && h.Key.Trim().Equals("Content-Type", StringComparison.OrdinalIgnoreCase));
        if (resolved.Body.Length > 0 && !hasContentType)
            parts.Add($"-H {Quote($"Content-Type: {RequestSender.GuessContentType(resolved.Body)}")}");

        foreach (var header in resolved.Headers.Where(h => h.Enabled && h.Key.Trim().Length > 0))
            parts.Add($"-H {Quote(Redact($"{header.Key.Trim()}: {header.Value}", masked))}");

        var auth = resolved.Auth;
        switch (auth?.Kind)
        {
            case AuthKinds.Bearer when auth.Token.Length > 0:
                parts.Add($"-H {Quote(Redact($"Authorization: Bearer {auth.Token}", masked))}");
                break;
            case AuthKinds.Basic:
                parts.Add($"-u {Quote(Redact($"{auth.Username}:{auth.Password}", masked))}");
                break;
            case AuthKinds.ApiKey when !auth.KeyInQuery && auth.KeyName.Length > 0:
                parts.Add($"-H {Quote(Redact($"{auth.KeyName}: {auth.KeyValue}", masked))}");
                break;
        }

        if (resolved.Body.Length > 0)
            parts.Add($"--data-raw {Quote(Redact(resolved.Body, masked))}");

        return parts.Count == 0 ? first : first + string.Concat(parts.Select(part => " \\\n  " + part));
    }

    /// <summary>Secrets that will be masked: the caller's list plus the auth values, longest first.</summary>
    private static List<string> MaskList(ApiRequest resolved, IEnumerable<string> secrets)
    {
        var values = secrets.Append(resolved.Auth?.Token ?? "")
            .Append(resolved.Auth?.Password ?? "")
            .Append(resolved.Auth?.KeyValue ?? "")
            .Where(v => v.Length > 0)
            .Distinct()
            .OrderByDescending(v => v.Length)
            .ToList();
        return values;
    }

    private static string Redact(string text, List<string> secrets) =>
        secrets.Aggregate(text, (current, secret) => current.Replace(secret, Mask, StringComparison.Ordinal));

    /// <summary>An API key sent in the query string is part of the URL, as the sender does.</summary>
    private static string UrlWithQueryAuth(ApiRequest request)
    {
        var url = request.Url.Trim();
        var auth = request.Auth;
        if (auth is not { Kind: AuthKinds.ApiKey, KeyInQuery: true } || auth.KeyName.Length == 0)
            return url;

        var pair = $"{Uri.EscapeDataString(auth.KeyName)}={Uri.EscapeDataString(auth.KeyValue)}";
        return url.Contains('?') ? $"{url}&{pair}" : $"{url}?{pair}";
    }

    /// <summary>Single quotes, with embedded single quotes written as '\'' so the shell reads them back exactly.</summary>
    internal static string Quote(string value)
    {
        var builder = new StringBuilder("'");
        foreach (var c in value)
        {
            if (c == '\'')
                builder.Append("'\\''");
            else
                builder.Append(c);
        }

        return builder.Append('\'').ToString();
    }
}
