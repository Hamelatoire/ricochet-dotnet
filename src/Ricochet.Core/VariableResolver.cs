using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>Replaces {{name}} placeholders. Unknown names are left as written so the problem stays visible.</summary>
public static partial class VariableResolver
{
    // A variable's value may itself contain placeholders. The limit stops a cycle from looping forever.
    private const int MaxNesting = 10;

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

        string Resolve(string text)
        {
            for (var depth = 0; depth < MaxNesting; depth++)
            {
                var next = Placeholder().Replace(text, m => lookup.GetValueOrDefault(m.Groups[1].Value.Trim(), m.Value));
                if (next == text)
                    break;
                text = next;
            }

            return text;
        }

        // In a JSON body, a value inside a string is escaped, so a quote in it can't end the string early.
        // A value outside a string is inserted as written, because it may be a whole JSON object.
        string ResolveBody(string text)
        {
            if (!text.TrimStart().StartsWith('{') && !text.TrimStart().StartsWith('['))
                return Resolve(text);

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

                var name = match.Groups[1].Value.Trim();
                if (!lookup.TryGetValue(name, out var value))
                {
                    result.Append(match.Value);
                    continue;
                }

                var resolved = Resolve(value);
                result.Append(inString ? JsonEncodedText.Encode(resolved, JavaScriptEncoder.UnsafeRelaxedJsonEscaping).ToString() : resolved);
            }

            AppendLiteral(text[last..]);
            return result.ToString();
        }

        return request with
        {
            Url = Resolve(request.Url),
            Body = ResolveBody(request.Body),
            Headers = request.Headers.Select(h => h with { Key = Resolve(h.Key), Value = Resolve(h.Value) }).ToList(),
            Auth = request.Auth is { } auth
                ? auth with
                {
                    Token = Resolve(auth.Token),
                    Username = Resolve(auth.Username),
                    Password = Resolve(auth.Password),
                    KeyName = Resolve(auth.KeyName),
                    KeyValue = Resolve(auth.KeyValue),
                }
                : null,
            Assertions = request.Assertions?.Select(a => a with { Path = Resolve(a.Path), Expected = Resolve(a.Expected) }).ToList(),
            Captures = request.Captures?.Select(c => c with { Path = Resolve(c.Path) }).ToList(),
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
