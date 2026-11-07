using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>Recognises values that must not be written to Workspace files. See docs/adr/0004-secrets-in-os-storage.md.</summary>
public static partial class SecretRules
{
    /// <summary>Replaces sensitive values in History. Plain ASCII, because it may end up in a header if replayed.</summary>
    public const string HiddenValue = "[hidden]";

    private static readonly HashSet<string> SensitiveHeaders = new(StringComparer.OrdinalIgnoreCase)
    {
        "Authorization", "Proxy-Authorization", "Cookie", "Set-Cookie",
        "X-Api-Key", "Api-Key", "X-Auth-Token", "X-Access-Token",
    };

    [GeneratedRegex(@"\{\{[^{}]+\}\}")]
    private static partial Regex Placeholder();

    [GeneratedRegex(@"[?&](api[_-]?key|access[_-]?token|auth[_-]?token|token|password|passwd|secret)=([^&#]+)", RegexOptions.IgnoreCase)]
    private static partial Regex SensitiveQueryParameter();

    /// <summary>True when the text holds an actual value, not just {{placeholders}} that are safe to share.</summary>
    public static bool IsLiteral(string value) => Placeholder().Replace(value, "").Trim().Length > 0;

    public static bool IsSensitiveHeader(string name) => SensitiveHeaders.Contains(name.Trim());

    /// <summary>Best-effort list of credentials written where they would be stored in plain text.</summary>
    public static IReadOnlyList<string> FindLikelyLiterals(ApiRequest request)
    {
        var found = new List<string>();
        foreach (var header in request.Headers)
        {
            if (header.Enabled && IsSensitiveHeader(header.Key) && IsLiteral(header.Value) && header.Value != HiddenValue)
                found.Add($"header {header.Key.Trim()}");
        }

        foreach (Match match in SensitiveQueryParameter().Matches(request.Url))
        {
            if (IsLiteral(match.Groups[2].Value))
                found.Add($"URL parameter {match.Groups[1].Value}");
        }

        return found;
    }
}

/// <summary>Separates Secrets from a Workspace before saving, and restores them after loading.</summary>
public static class WorkspaceSecrets
{
    private const string GlobalsScope = "globals";
    private const string WorkspacePrefix = "workspace/";

    /// <summary>The Secrets of one Workspace. Entries from before Workspaces had ids count as belonging to it.</summary>
    public static Dictionary<string, string> ForWorkspace(IReadOnlyDictionary<string, string> vault, string workspaceId)
    {
        var prefix = $"{WorkspacePrefix}{workspaceId}/";
        var result = new Dictionary<string, string>();
        foreach (var (key, value) in vault)
        {
            if (!key.StartsWith(WorkspacePrefix, StringComparison.Ordinal))
                result.TryAdd(key, value);
        }

        foreach (var (key, value) in vault)
        {
            if (key.StartsWith(prefix, StringComparison.Ordinal))
                result[key[prefix.Length..]] = value;
        }

        return result;
    }

    /// <summary>Replaces one Workspace's Secrets, and leaves those of every other Workspace alone.</summary>
    public static Dictionary<string, string> WithWorkspace(IReadOnlyDictionary<string, string> vault, string workspaceId, IReadOnlyDictionary<string, string> secrets)
    {
        var prefix = $"{WorkspacePrefix}{workspaceId}/";
        var result = new Dictionary<string, string>();
        foreach (var (key, value) in vault)
        {
            if (key.StartsWith(WorkspacePrefix, StringComparison.Ordinal) && !key.StartsWith(prefix, StringComparison.Ordinal))
                result[key] = value;
        }

        foreach (var (key, value) in secrets)
            result[prefix + key] = value;

        return result;
    }

    /// <summary>Returns a copy safe to write to disk, plus the values that belong in a vault.</summary>
    public static (Workspace Clean, Dictionary<string, string> Secrets) Split(Workspace workspace)
    {
        var secrets = new Dictionary<string, string>();

        var clean = workspace with
        {
            Collections = workspace.Collections.Select(c => c with
            {
                Variables = SplitVariables(c.Variables, c.Id, secrets),
                Items = c.Items.Select(i => SplitItem(i, secrets)).ToList(),
            }).ToList(),
            Environments = workspace.Environments.Select(e => e with
            {
                Variables = SplitVariables(e.Variables, e.Id, secrets),
            }).ToList(),
            Globals = SplitVariables(workspace.Globals, GlobalsScope, secrets),
            History = workspace.History.Select(SanitizeHistory).ToList(),
        };

        return (clean, secrets);
    }

    /// <summary>Puts vault values back. A value already in the file wins, so it moves to the vault on the next save.</summary>
    public static Workspace Merge(Workspace clean, IReadOnlyDictionary<string, string> secrets) => clean with
    {
        Collections = clean.Collections.Select(c => c with
        {
            Variables = MergeVariables(c.Variables, c.Id, secrets),
            Items = c.Items.Select(i => MergeItem(i, secrets)).ToList(),
        }).ToList(),
        Environments = clean.Environments.Select(e => e with
        {
            Variables = MergeVariables(e.Variables, e.Id, secrets),
        }).ToList(),
        Globals = MergeVariables(clean.Globals, GlobalsScope, secrets),
    };

    /// <summary>True when a file written before Secrets existed still holds values that should move.</summary>
    public static bool NeedsMigration(Workspace loaded)
    {
        var (_, secrets) = Split(loaded);
        return secrets.Count > 0 || loaded.History.Any(h => !ReferenceEquals(SanitizeHistory(h), h));
    }

    /// <summary>History keeps placeholders but no credentials. Returns the same instance when nothing changes.</summary>
    public static HistoryEntry SanitizeHistory(HistoryEntry entry)
    {
        var request = SanitizeForHistory(entry.Request);
        return ReferenceEquals(request, entry.Request) ? entry : entry with { Request = request };
    }

    private static ApiRequest SanitizeForHistory(ApiRequest request)
    {
        var hasSensitiveHeader = request.Headers.Any(IsHeaderToHide);
        var hasLiteralAuth = request.Auth is { } auth && AuthSecretValues(auth).Any(SecretRules.IsLiteral);

        if (!hasSensitiveHeader && !hasLiteralAuth)
            return request;

        return request with
        {
            Headers = request.Headers.Select(h => IsHeaderToHide(h) ? h with { Value = SecretRules.HiddenValue } : h).ToList(),
            Auth = request.Auth is { } a
                ? a with
                {
                    Token = BlankIfLiteral(a.Token),
                    Password = BlankIfLiteral(a.Password),
                    KeyValue = BlankIfLiteral(a.KeyValue),
                    Details = a.Details is { } d
                        ? d with
                        {
                            ConsumerSecret = BlankIfLiteral(d.ConsumerSecret),
                            TokenSecret = BlankIfLiteral(d.TokenSecret),
                            AwsSecretAccessKey = BlankIfLiteral(d.AwsSecretAccessKey),
                            AwsSessionToken = BlankIfLiteral(d.AwsSessionToken),
                        }
                        : null,
                }
                : null,
        };
    }

    /// <summary>Compares what two requests would store, ignoring object identity.</summary>
    public static bool SameContent(ApiRequest a, ApiRequest b) =>
        System.Text.Json.JsonSerializer.Serialize(a, WorkspaceJsonContext.Default.ApiRequest)
        == System.Text.Json.JsonSerializer.Serialize(b, WorkspaceJsonContext.Default.ApiRequest);

    public static bool HasHiddenValues(ApiRequest request) =>
        request.Headers.Any(h => h.Enabled && h.Value == SecretRules.HiddenValue);

    private static bool IsHeaderToHide(KeyValue header) =>
        SecretRules.IsSensitiveHeader(header.Key) && SecretRules.IsLiteral(header.Value) && header.Value != SecretRules.HiddenValue;

    private static string BlankIfLiteral(string value) => SecretRules.IsLiteral(value) ? "" : value;

    /// <summary>Every field of an auth that is a Secret (ADR 0004). Keep in step with <see cref="SplitAuth"/> and <see cref="MergeAuth"/>.</summary>
    internal static IEnumerable<string> AuthSecretValues(RequestAuth auth)
    {
        var details = auth.Details;
        return [auth.Token, auth.Password, auth.KeyValue,
            details?.ConsumerSecret ?? "", details?.TokenSecret ?? "", details?.AwsSecretAccessKey ?? "", details?.AwsSessionToken ?? ""];
    }

    private static string VariableKey(string scope, string name) => $"variable/{scope}/{name}";

    private static string AuthKey(string requestId, string field) => $"auth/{requestId}/{field}";

    private static List<KeyValue> SplitVariables(IEnumerable<KeyValue> variables, string scope, Dictionary<string, string> secrets)
    {
        var result = new List<KeyValue>();
        foreach (var variable in variables)
        {
            if (!variable.IsSecret || variable.Value.Length == 0)
            {
                result.Add(variable);
                continue;
            }

            secrets[VariableKey(scope, variable.Key)] = variable.Value;
            result.Add(variable with { Value = "" });
        }

        return result;
    }

    private static List<KeyValue> MergeVariables(IEnumerable<KeyValue> variables, string scope, IReadOnlyDictionary<string, string> secrets) =>
        variables.Select(v => v.IsSecret && v.Value.Length == 0 && secrets.TryGetValue(VariableKey(scope, v.Key), out var value)
            ? v with { Value = value }
            : v).ToList();

    private static CollectionItem SplitItem(CollectionItem item, Dictionary<string, string> secrets) => item with
    {
        Request = item.Request is { Auth: not null } request
            ? request with { Auth = SplitAuth(request.Auth, request.Id, secrets) }
            : item.Request,
        Children = item.Children.Select(c => SplitItem(c, secrets)).ToList(),
    };

    private static CollectionItem MergeItem(CollectionItem item, IReadOnlyDictionary<string, string> secrets) => item with
    {
        Request = item.Request is { Auth: not null } request
            ? request with { Auth = MergeAuth(request.Auth, request.Id, secrets) }
            : item.Request,
        Children = item.Children.Select(c => MergeItem(c, secrets)).ToList(),
    };

    private static RequestAuth SplitAuth(RequestAuth auth, string requestId, Dictionary<string, string> secrets)
    {
        string Take(string value, string field)
        {
            if (!SecretRules.IsLiteral(value))
                return value; // Placeholders hold no secret, and are safe to share.

            secrets[AuthKey(requestId, field)] = value;
            return "";
        }

        return auth with
        {
            Token = Take(auth.Token, "token"),
            Password = Take(auth.Password, "password"),
            KeyValue = Take(auth.KeyValue, "keyValue"),
            Details = auth.Details is { } d
                ? d with
                {
                    ConsumerSecret = Take(d.ConsumerSecret, "consumerSecret"),
                    TokenSecret = Take(d.TokenSecret, "tokenSecret"),
                    AwsSecretAccessKey = Take(d.AwsSecretAccessKey, "awsSecretAccessKey"),
                    AwsSessionToken = Take(d.AwsSessionToken, "awsSessionToken"),
                }
                : null,
        };
    }

    private static RequestAuth MergeAuth(RequestAuth auth, string requestId, IReadOnlyDictionary<string, string> secrets)
    {
        string Restore(string value, string field) =>
            value.Length == 0 && secrets.TryGetValue(AuthKey(requestId, field), out var stored) ? stored : value;

        return auth with
        {
            Token = Restore(auth.Token, "token"),
            Password = Restore(auth.Password, "password"),
            KeyValue = Restore(auth.KeyValue, "keyValue"),
            Details = auth.Details is { } d
                ? d with
                {
                    ConsumerSecret = Restore(d.ConsumerSecret, "consumerSecret"),
                    TokenSecret = Restore(d.TokenSecret, "tokenSecret"),
                    AwsSecretAccessKey = Restore(d.AwsSecretAccessKey, "awsSecretAccessKey"),
                    AwsSessionToken = Restore(d.AwsSessionToken, "awsSessionToken"),
                }
                : null,
        };
    }
}
