using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>The request to send, and the copy History keeps: built-in function values filled in, Variables left as written.</summary>
public sealed record ResolvedRequests(ApiRequest Sent, ApiRequest History);

/// <summary>
/// Replaces {{name}} placeholders with variables and built-in functions (ADR 0008).
/// Unknown names are left as written so the problem stays visible.
/// </summary>
public static partial class VariableResolver
{
    [GeneratedRegex(@"\{\{([^{}]+)\}\}")]
    private static partial Regex Placeholder();

    /// <summary>Combines scopes, lowest priority first. A disabled variable never overrides a lower scope.</summary>
    public static IReadOnlyList<KeyValue> Layer(params IEnumerable<KeyValue>?[] lowestPriorityFirst) =>
        lowestPriorityFirst.SelectMany(scope => scope ?? []).ToList();

    /// <summary>The request to send, with every placeholder replaced.</summary>
    public static ApiRequest Apply(ApiRequest request, IEnumerable<KeyValue> variables) => Resolve(request, variables).Sent;

    /// <summary>
    /// Resolves once, so the request sent and the History copy hold the same generated values. The History copy keeps
    /// Variables as written, so a Secret's value never reaches History through it.
    /// </summary>
    public static ResolvedRequests Resolve(ApiRequest request, IEnumerable<KeyValue> variables)
    {
        var lookup = new Dictionary<string, string>();
        foreach (var variable in variables)
        {
            // A Secret with no value on this machine counts as unset, so it gets reported rather than sent blank.
            if (variable.Enabled && variable.Key.Length > 0 && !(variable.IsSecret && variable.Value.Length == 0))
                lookup[variable.Key.Trim()] = variable.Value;
        }

        // A variable is expanded once per send, so every use of it sees the same generated values (ADR 0008).
        var expanded = new Dictionary<(string Name, bool Functions), string>();
        var inProgress = new HashSet<string>();

        // Functions run only where the value is sent. Assertions and Captures never run them (ADR 0008).
        (string Sent, string Shown) Expand(string text, bool functions)
        {
            var sent = new StringBuilder();
            var shown = new StringBuilder();
            var last = 0;
            foreach (Match match in Placeholder().Matches(text))
            {
                sent.Append(text, last, match.Index - last);
                shown.Append(text, last, match.Index - last);
                last = match.Index + match.Length;

                var replacement = Replacement(match.Groups[1].Value.Trim(), match.Value, functions);
                sent.Append(replacement.Sent);
                shown.Append(replacement.Shown);
            }

            sent.Append(text, last, text.Length - last);
            shown.Append(text, last, text.Length - last);
            return (sent.ToString(), shown.ToString());
        }

        // Sent is the value to send. Shown is what History keeps: a variable stays as written, a function shows its value.
        (string Sent, string Shown) Replacement(string name, string original, bool functions)
        {
            if (lookup.ContainsKey(name))
                return (VariableText(name, lookup[name], functions), original);

            if (!functions)
                return (original, original);

            var value = BuiltInFunctions.TryEvaluate(name);
            return value is null ? (original, original) : (value, value);
        }

        string VariableText(string name, string value, bool functions)
        {
            if (expanded.TryGetValue((name, functions), out var done))
                return done;

            // A variable that refers back to itself is left as written, so the cycle shows up as a missing value.
            if (!inProgress.Add(name))
                return "{{" + name + "}}";

            var text = Expand(value, functions).Sent;
            inProgress.Remove(name);
            expanded[(name, functions)] = text;
            return text;
        }

        // In a JSON body, a value inside a string is escaped, so a quote in it can't end the string early.
        // A value outside a string is inserted as written, because it may be a whole JSON object.
        (string Sent, string Shown) ResolveBody(string text)
        {
            if (!text.TrimStart().StartsWith('{') && !text.TrimStart().StartsWith('['))
                return Expand(text, functions: true);

            var sent = new StringBuilder();
            var shown = new StringBuilder();
            var inString = false;
            var escaped = false;

            void AppendLiteral(string literal)
            {
                foreach (var c in literal)
                {
                    if (escaped)
                        escaped = false;
                    else if (inString && c == '\\')
                        escaped = true;
                    else if (c == '"')
                        inString = !inString;
                }

                sent.Append(literal);
                shown.Append(literal);
            }

            var last = 0;
            foreach (Match match in Placeholder().Matches(text))
            {
                AppendLiteral(text[last..match.Index]);
                last = match.Index + match.Length;

                var replacement = Replacement(match.Groups[1].Value.Trim(), match.Value, functions: true);
                if (replacement.Sent == match.Value)
                {
                    sent.Append(match.Value);
                    shown.Append(match.Value);
                    continue;
                }

                sent.Append(inString ? JsonEncodedText.Encode(replacement.Sent, JavaScriptEncoder.UnsafeRelaxedJsonEscaping).ToString() : replacement.Sent);
                shown.Append(replacement.Shown);
            }

            AppendLiteral(text[last..]);
            return (sent.ToString(), shown.ToString());
        }

        var url = Expand(request.Url, functions: true);
        var body = ResolveBody(request.Body);
        var headers = request.Headers.Select(h => (Header: h, Key: Expand(h.Key, true), Value: Expand(h.Value, true))).ToList();
        var auth = request.Auth is null
            ? null
            : new
            {
                Token = Expand(request.Auth.Token, true),
                Username = Expand(request.Auth.Username, true),
                Password = Expand(request.Auth.Password, true),
                KeyName = Expand(request.Auth.KeyName, true),
                KeyValue = Expand(request.Auth.KeyValue, true),
            };

        // Assertions and Captures never run functions, so their text is the same in both copies.
        var assertions = request.Assertions?.Select(x => x with { Path = Expand(x.Path, false).Sent, Expected = Expand(x.Expected, false).Sent }).ToList();
        var captures = request.Captures?.Select(c => c with { Path = Expand(c.Path, false).Sent }).ToList();

        var sent = request with
        {
            Url = url.Sent,
            Body = body.Sent,
            Headers = headers.Select(h => h.Header with { Key = h.Key.Sent, Value = h.Value.Sent }).ToList(),
            Auth = request.Auth is null ? null : request.Auth with
            {
                Token = auth!.Token.Sent,
                Username = auth.Username.Sent,
                Password = auth.Password.Sent,
                KeyName = auth.KeyName.Sent,
                KeyValue = auth.KeyValue.Sent,
            },
            Assertions = assertions,
            Captures = captures,
        };

        var history = request with
        {
            Url = url.Shown,
            Body = body.Shown,
            Headers = headers.Select(h => h.Header with { Key = h.Key.Shown, Value = h.Value.Shown }).ToList(),
            Auth = request.Auth is null ? null : request.Auth with
            {
                Token = auth!.Token.Shown,
                Username = auth.Username.Shown,
                Password = auth.Password.Shown,
                KeyName = auth.KeyName.Shown,
                KeyValue = auth.KeyValue.Shown,
            },
        };

        return new ResolvedRequests(sent, history);
    }

    /// <summary>Names still written as placeholders in the parts of a request that will actually be sent.</summary>
    public static IReadOnlyList<string> FindPlaceholders(ApiRequest request)
    {
        var texts = new List<string> { request.Url, request.Body };
        foreach (var header in request.Headers.Where(h => h.Enabled))
        {
            texts.Add(header.Key);
            texts.Add(header.Value);
        }

        // A request keeps the fields of other auth types, so switching back loses nothing, but only the chosen type is sent.
        texts.AddRange(request.Auth switch
        {
            { Kind: AuthKinds.Bearer } auth => [auth.Token],
            { Kind: AuthKinds.Basic } auth => [auth.Username, auth.Password],
            { Kind: AuthKinds.ApiKey } auth => [auth.KeyName, auth.KeyValue],
            _ => [],
        });

        foreach (var assertion in request.Assertions?.Where(a => a.Enabled) ?? [])
        {
            texts.Add(assertion.Path);
            texts.Add(assertion.Expected);
        }

        texts.AddRange(request.Captures?.Where(c => c.Enabled).Select(c => c.Path) ?? []);

        return texts
            .SelectMany(text => Placeholder().Matches(text))
            .Select(match => match.Groups[1].Value.Trim())
            .Distinct()
            .ToList();
    }
}
