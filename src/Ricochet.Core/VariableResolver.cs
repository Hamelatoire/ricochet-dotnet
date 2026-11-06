using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

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

    public static ApiRequest Apply(ApiRequest request, IEnumerable<KeyValue> variables)
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
        string Expand(string text, bool functions) =>
            Placeholder().Replace(text, m => Replacement(m.Groups[1].Value.Trim(), m.Value, functions));

        string Replacement(string name, string original, bool functions)
        {
            if (lookup.TryGetValue(name, out var value))
                return VariableText(name, value, functions);

            return functions ? BuiltInFunctions.TryEvaluate(name) ?? original : original;
        }

        string VariableText(string name, string value, bool functions)
        {
            if (expanded.TryGetValue((name, functions), out var done))
                return done;

            // A variable that refers back to itself is left as written, so the cycle shows up as a missing value.
            if (!inProgress.Add(name))
                return "{{" + name + "}}";

            var text = Expand(value, functions);
            inProgress.Remove(name);
            expanded[(name, functions)] = text;
            return text;
        }

        // In a JSON body, a value inside a string is escaped, so a quote in it can't end the string early.
        // A value outside a string is inserted as written, because it may be a whole JSON object.
        string ResolveBody(string text)
        {
            if (!text.TrimStart().StartsWith('{') && !text.TrimStart().StartsWith('['))
                return Expand(text, functions: true);

            var result = new StringBuilder();
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

                result.Append(literal);
            }

            var last = 0;
            foreach (Match match in Placeholder().Matches(text))
            {
                AppendLiteral(text[last..match.Index]);
                last = match.Index + match.Length;

                var replacement = Replacement(match.Groups[1].Value.Trim(), match.Value, functions: true);
                if (replacement == match.Value)
                {
                    result.Append(match.Value);
                    continue;
                }

                result.Append(inString ? JsonEncodedText.Encode(replacement, JavaScriptEncoder.UnsafeRelaxedJsonEscaping).ToString() : replacement);
            }

            AppendLiteral(text[last..]);
            return result.ToString();
        }

        return request with
        {
            Url = Expand(request.Url, functions: true),
            Body = ResolveBody(request.Body),
            Headers = request.Headers.Select(h => h with { Key = Expand(h.Key, functions: true), Value = Expand(h.Value, functions: true) }).ToList(),
            Auth = request.Auth is { } auth
                ? auth with
                {
                    Token = Expand(auth.Token, functions: true),
                    Username = Expand(auth.Username, functions: true),
                    Password = Expand(auth.Password, functions: true),
                    KeyName = Expand(auth.KeyName, functions: true),
                    KeyValue = Expand(auth.KeyValue, functions: true),
                }
                : null,
            Assertions = request.Assertions?.Select(a => a with { Path = Expand(a.Path, functions: false), Expected = Expand(a.Expected, functions: false) }).ToList(),
            Captures = request.Captures?.Select(c => c with { Path = Expand(c.Path, functions: false) }).ToList(),
        };
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
