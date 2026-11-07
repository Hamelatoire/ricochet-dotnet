using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>The request to send, and the copy History keeps: built-in function values filled in, Variables left as written.</summary>
public sealed record ResolvedRequests(ApiRequest Sent, ApiRequest History);

/// <summary>A prompt Variable the request uses (ADR 0019). Default is its stored value, which the prompt starts from.</summary>
public sealed record PromptVariable(string Name, string Default);

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
    /// The prompt Variables this request uses, directly or through other Variables, with the value each one
    /// would otherwise have. A prompt Variable's own stored value is not searched further, because the answer replaces it (ADR 0019).
    /// </summary>
    public static IReadOnlyList<PromptVariable> FindPromptVariables(ApiRequest request, IEnumerable<KeyValue> variables)
    {
        var winners = new Dictionary<string, KeyValue>();
        foreach (var variable in variables)
        {
            if (variable.Enabled && variable.Key.Trim().Length > 0 && !(variable.IsSecret && variable.Value.Length == 0))
                winners[variable.Key.Trim()] = variable;
        }

        var found = new List<PromptVariable>();
        var seen = new HashSet<string>();
        var pending = new Queue<string>(FindPlaceholders(request));
        while (pending.Count > 0)
        {
            var name = pending.Dequeue();
            if (!seen.Add(name) || !winners.TryGetValue(name, out var variable))
                continue;

            if (variable.IsPrompt)
            {
                found.Add(new PromptVariable(name, variable.Value));
                continue;
            }

            foreach (Match match in Placeholder().Matches(variable.Value))
                pending.Enqueue(match.Groups[1].Value.Trim());
        }

        return found;
    }

    /// <summary>
    /// Resolves once, so the request sent and the History copy hold the same generated values. The History copy keeps
    /// Variables as written, so a Secret's value never reaches History through it.
    /// <paramref name="answers"/> holds the values typed into the prompt for this send only (ADR 0019). An empty answer
    /// leaves its Variable unset, so it is reported rather than sent blank.
    /// </summary>
    public static ResolvedRequests Resolve(ApiRequest request, IEnumerable<KeyValue> variables, IReadOnlyDictionary<string, string>? answers = null)
    {
        var lookup = new Dictionary<string, string>();
        foreach (var variable in variables)
        {
            // A Secret or prompt Variable with no value here counts as unset, so it gets reported rather than sent blank.
            if (variable.Enabled && variable.Key.Length > 0 && !((variable.IsSecret || variable.IsPrompt) && variable.Value.Length == 0))
                lookup[variable.Key.Trim()] = variable.Value;
        }

        foreach (var (name, answer) in answers ?? new Dictionary<string, string>())
        {
            if (answer.Length == 0)
                lookup.Remove(name);
            else
                lookup[name] = answer;
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
                Details = request.Auth.Details is not { } details ? null : new
                {
                    ConsumerKey = Expand(details.ConsumerKey, true),
                    ConsumerSecret = Expand(details.ConsumerSecret, true),
                    TokenSecret = Expand(details.TokenSecret, true),
                    AwsAccessKeyId = Expand(details.AwsAccessKeyId, true),
                    AwsSecretAccessKey = Expand(details.AwsSecretAccessKey, true),
                    AwsSessionToken = Expand(details.AwsSessionToken, true),
                    AwsRegion = Expand(details.AwsRegion, true),
                    AwsService = Expand(details.AwsService, true),
                },
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
                Details = auth.Details is null ? null : new AuthDetails
                {
                    ConsumerKey = auth.Details.ConsumerKey.Sent,
                    ConsumerSecret = auth.Details.ConsumerSecret.Sent,
                    TokenSecret = auth.Details.TokenSecret.Sent,
                    AwsAccessKeyId = auth.Details.AwsAccessKeyId.Sent,
                    AwsSecretAccessKey = auth.Details.AwsSecretAccessKey.Sent,
                    AwsSessionToken = auth.Details.AwsSessionToken.Sent,
                    AwsRegion = auth.Details.AwsRegion.Sent,
                    AwsService = auth.Details.AwsService.Sent,
                },
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
                Details = auth.Details is null ? null : new AuthDetails
                {
                    ConsumerKey = auth.Details.ConsumerKey.Shown,
                    ConsumerSecret = auth.Details.ConsumerSecret.Shown,
                    TokenSecret = auth.Details.TokenSecret.Shown,
                    AwsAccessKeyId = auth.Details.AwsAccessKeyId.Shown,
                    AwsSecretAccessKey = auth.Details.AwsSecretAccessKey.Shown,
                    AwsSessionToken = auth.Details.AwsSessionToken.Shown,
                    AwsRegion = auth.Details.AwsRegion.Shown,
                    AwsService = auth.Details.AwsService.Shown,
                },
            },
        };

        return new ResolvedRequests(sent, history);
    }

    /// <summary>
    /// Reads the external Secret behind each secret reference (ADR 0025) that the request reaches, and returns the variables
    /// with those values added as Secrets, so they win over the references. Call it just before sending, and pass the result
    /// to <see cref="Resolve"/> for the send only: the request and its History copy keep the reference as written.
    /// A reference that cannot be read fails the request before anything is sent. The message names the Variable, never a value.
    /// </summary>
    /// <exception cref="RequestFailedException">A reference could not be read.</exception>
    public static async Task<IReadOnlyList<KeyValue>> ResolveReferencesAsync(
        ApiRequest request,
        IEnumerable<KeyValue> variables,
        ExternalSecretResolver secrets,
        CancellationToken cancellationToken = default)
    {
        var all = variables.ToList();

        // The same winner rule Resolve uses, so the reference read here is the one the request would send.
        var winners = new Dictionary<string, KeyValue>();
        foreach (var variable in all)
        {
            if (variable.Enabled && variable.Key.Length > 0 && !((variable.IsSecret || variable.IsPrompt) && variable.Value.Length == 0))
                winners[variable.Key.Trim()] = variable;
        }

        var added = new List<KeyValue>();
        var seen = new HashSet<string>();
        var pending = new Queue<string>(FindPlaceholders(request));
        while (pending.Count > 0)
        {
            var name = pending.Dequeue();
            if (!seen.Add(name) || !winners.TryGetValue(name, out var variable))
                continue;

            // A prompt Variable is sent with its answer, so its stored value is never read.
            if (variable.IsPrompt)
                continue;

            if (!ExternalSecretReference.IsReference(variable.Value))
            {
                foreach (Match match in Placeholder().Matches(variable.Value))
                    pending.Enqueue(match.Groups[1].Value.Trim());
                continue;
            }

            string value;
            try
            {
                value = await secrets.ResolveAsync(variable.Value, cancellationToken);
            }
            catch (ExternalSecretException ex)
            {
                throw new RequestFailedException($"The secret reference for {name} could not be read. {ex.Message}", ex);
            }

            // Marked as a Secret, so the value is masked wherever this send's output is shown.
            added.Add(variable with { Key = name, Value = value, Enabled = true, IsSecret = true });
        }

        return added.Count == 0 ? all : [.. all, .. added];
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
            { Kind: AuthKinds.Basic or AuthKinds.Digest } auth => [auth.Username, auth.Password],
            { Kind: AuthKinds.ApiKey } auth => [auth.KeyName, auth.KeyValue],
            { Kind: AuthKinds.OAuth1, Details: { } d } auth => [d.ConsumerKey, d.ConsumerSecret, auth.Token, d.TokenSecret],
            { Kind: AuthKinds.AwsSigV4, Details: { } d } => [d.AwsAccessKeyId, d.AwsSecretAccessKey, d.AwsSessionToken, d.AwsRegion, d.AwsService],
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
