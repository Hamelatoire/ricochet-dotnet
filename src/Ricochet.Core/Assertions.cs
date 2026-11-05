using System.Globalization;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

public static class AssertionSources
{
    public const string Status = "status";
    public const string Time = "time";
    public const string Header = "header";
    public const string Body = "body";
    public const string Json = "json";
}

public static class AssertionOps
{
    public const string Equal = "equals";
    public const string NotEqual = "notEquals";
    public const string Contains = "contains";
    public const string NotContains = "notContains";
    public const string Exists = "exists";
    public const string NotExists = "notExists";
    public const string LessThan = "lessThan";
    public const string GreaterThan = "greaterThan";
    public const string Matches = "matches";
}

/// <summary>One expected condition on a response. See docs/adr/0006-declarative-tests-not-scripts.md.</summary>
public sealed record RequestAssertion
{
    public bool Enabled { get; set; } = true;
    public string Source { get; set; } = AssertionSources.Status;
    public string Path { get; set; } = "";
    public string Op { get; set; } = AssertionOps.Equal;
    public string Expected { get; set; } = "";
}

public sealed record AssertionResult(string Description, bool Passed, string Message);

/// <summary>Saves a value from a response into a Variable, for later Requests to use.</summary>
public sealed record RequestCapture
{
    public bool Enabled { get; set; } = true;
    public string Source { get; set; } = AssertionSources.Json;
    public string Path { get; set; } = "";
    public string Variable { get; set; } = "";
}

public sealed record CaptureOutcome(IReadOnlyList<KeyValue> Values, IReadOnlyList<AssertionResult> Failures);

public static class AssertionEvaluator
{
    private const int MaxShownLength = 200;
    private static readonly TimeSpan RegexLimit = TimeSpan.FromSeconds(1);

    public static List<AssertionResult> Evaluate(IEnumerable<RequestAssertion>? assertions, ApiResponse response)
    {
        var results = new List<AssertionResult>();
        var enabled = assertions?.Where(a => a.Enabled).ToList() ?? [];
        if (enabled.Count == 0)
            return results;

        var body = new Lazy<string>(() => ResponseFormatter.Format(response).Text);
        using var json = new LazyJson(body);

        foreach (var assertion in enabled)
            results.Add(EvaluateOne(assertion, response, body, json));

        return results;
    }

    /// <summary>A capture that finds nothing is a failure, because later Requests depend on it.</summary>
    public static CaptureOutcome EvaluateCaptures(IEnumerable<RequestCapture>? captures, ApiResponse response)
    {
        var enabled = captures?.Where(c => c.Enabled).ToList() ?? [];
        var values = new List<KeyValue>();
        var failures = new List<AssertionResult>();
        if (enabled.Count == 0)
            return new CaptureOutcome(values, failures);

        var body = new Lazy<string>(() => ResponseFormatter.Format(response).Text);
        using var json = new LazyJson(body);

        foreach (var capture in enabled)
        {
            var variable = capture.Variable.Trim();
            var description = $"capture {(variable.Length > 0 ? variable : "(unnamed)")} from {capture.Source} {capture.Path}".TrimEnd();

            if (variable.Length == 0)
            {
                failures.Add(new AssertionResult(description, false, "it has no variable name"));
                continue;
            }

            var problem = TryRead(capture.Source, capture.Path, response, body, json, out var actual);
            if (problem is not null)
                failures.Add(new AssertionResult(description, false, problem));
            else if (actual is null || actual == "null")
                failures.Add(new AssertionResult(description, false, "not found"));
            else
                values.Add(new KeyValue(variable, actual));
        }

        return new CaptureOutcome(values, failures);
    }

    public static string Describe(RequestAssertion assertion)
    {
        var subject = assertion.Source switch
        {
            AssertionSources.Status => "status",
            AssertionSources.Time => "time in ms",
            AssertionSources.Body => "body",
            _ => $"{assertion.Source} {assertion.Path}",
        };

        var op = assertion.Op switch
        {
            AssertionOps.Equal => "equals",
            AssertionOps.NotEqual => "does not equal",
            AssertionOps.Contains => "contains",
            AssertionOps.NotContains => "does not contain",
            AssertionOps.Exists => "exists",
            AssertionOps.NotExists => "does not exist",
            AssertionOps.LessThan => "is less than",
            AssertionOps.GreaterThan => "is greater than",
            AssertionOps.Matches => "matches",
            _ => assertion.Op,
        };

        return assertion.Op is AssertionOps.Exists or AssertionOps.NotExists
            ? $"{subject} {op}"
            : $"{subject} {op} {assertion.Expected}";
    }

    private static AssertionResult EvaluateOne(RequestAssertion assertion, ApiResponse response, Lazy<string> body, LazyJson json)
    {
        var description = Describe(assertion);
        var problem = TryRead(assertion.Source, assertion.Path, response, body, json, out var actual);
        return problem is not null
            ? new AssertionResult(description, false, problem)
            : Compare(assertion, description, actual);
    }

    /// <summary>Finds a value in a response. A null value means not found. Returns a message if it can't look at all.</summary>
    private static string? TryRead(string source, string path, ApiResponse response, Lazy<string> body, LazyJson json, out string? actual)
    {
        actual = null;
        switch (source)
        {
            case AssertionSources.Status:
                actual = response.StatusCode.ToString(CultureInfo.InvariantCulture);
                return null;
            case AssertionSources.Time:
                actual = ((long)response.Elapsed.TotalMilliseconds).ToString(CultureInfo.InvariantCulture);
                return null;
            case AssertionSources.Header:
                actual = response.Headers.FirstOrDefault(h => h.Key.Equals(path.Trim(), StringComparison.OrdinalIgnoreCase))?.Value;
                return null;
            case AssertionSources.Body:
                actual = body.Value;
                return null;
            case AssertionSources.Json:
                if (json.Root is not { } root)
                    return "the response is not valid JSON";
                if (!JsonPath.TryFind(root, path, out var value, out var pathError))
                    return pathError;
                actual = value.ValueKind == JsonValueKind.String ? value.GetString() : value.GetRawText();
                return null;
            default:
                return $"unknown source \"{source}\"";
        }
    }

    private static AssertionResult Compare(RequestAssertion assertion, string description, string? actual)
    {
        var expected = assertion.Expected;

        AssertionResult Result(bool passed) => new(description, passed, passed ? "" : actual is null ? "not found" : $"got {Shorten(actual)}");

        if (assertion.Op == AssertionOps.Exists)
            return Result(actual is not null);
        if (assertion.Op == AssertionOps.NotExists)
            return new AssertionResult(description, actual is null, actual is null ? "" : $"found {Shorten(actual)}");
        if (actual is null)
            return Result(false);

        switch (assertion.Op)
        {
            case AssertionOps.Equal:
                return Result(AreEqual(actual, expected));
            case AssertionOps.NotEqual:
                return Result(!AreEqual(actual, expected));
            case AssertionOps.Contains:
                return Result(actual.Contains(expected, StringComparison.Ordinal));
            case AssertionOps.NotContains:
                return Result(!actual.Contains(expected, StringComparison.Ordinal));
            case AssertionOps.LessThan:
            case AssertionOps.GreaterThan:
                if (!TryNumber(actual, out var left))
                    return new AssertionResult(description, false, $"got {Shorten(actual)}, which is not a number");
                if (!TryNumber(expected, out var right))
                    return new AssertionResult(description, false, $"expected \"{expected}\" is not a number");
                return Result(assertion.Op == AssertionOps.LessThan ? left < right : left > right);
            case AssertionOps.Matches:
                try
                {
                    return Result(Regex.IsMatch(actual, expected, RegexOptions.None, RegexLimit));
                }
                catch (ArgumentException ex)
                {
                    return new AssertionResult(description, false, $"invalid pattern: {ex.Message}");
                }
                catch (RegexMatchTimeoutException)
                {
                    return new AssertionResult(description, false, "the pattern took too long to run");
                }
            default:
                return new AssertionResult(description, false, $"unknown comparison \"{assertion.Op}\"");
        }
    }

    // 1.0 and 1 are the same value in JSON, even though the text differs.
    private static bool AreEqual(string actual, string expected) =>
        actual == expected || (TryNumber(actual, out var a) && TryNumber(expected, out var b) && a == b);

    private static bool TryNumber(string text, out decimal value) =>
        decimal.TryParse(text.Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out value);

    private static string Shorten(string text) =>
        text.Length <= MaxShownLength ? text : text[..MaxShownLength] + "…";

    private sealed class LazyJson(Lazy<string> body) : IDisposable
    {
        private JsonDocument? _document;
        private bool _tried;

        public JsonElement? Root
        {
            get
            {
                if (!_tried)
                {
                    _tried = true;
                    try
                    {
                        _document = JsonDocument.Parse(body.Value);
                    }
                    catch (JsonException)
                    {
                        // Reported to each assertion that needs it.
                    }
                }

                return _document?.RootElement;
            }
        }

        public void Dispose() => _document?.Dispose();
    }
}

/// <summary>Finds a value by a path such as items[0].name or $.a['b.c']. A trailing .length counts an array.</summary>
public static class JsonPath
{
    /// <returns>False when not found. An error is set only if the path itself is malformed.</returns>
    public static bool TryFind(JsonElement root, string path, out JsonElement value, out string? error)
    {
        value = root;
        error = null;
        var text = path.Trim();
        if (text.StartsWith('$'))
            text = text[1..];

        var i = 0;
        while (i < text.Length)
        {
            if (text[i] == '.')
            {
                i++;
                continue;
            }

            if (text[i] == '[')
            {
                var close = text.IndexOf(']', i);
                if (close < 0)
                {
                    error = $"invalid path \"{path}\": missing ]";
                    return false;
                }

                var inner = text[(i + 1)..close].Trim();
                i = close + 1;

                if (inner.Length >= 2 && inner[0] is '\'' or '"' && inner[^1] == inner[0])
                {
                    if (!Step(ref value, inner[1..^1]))
                        return false;
                }
                else if (int.TryParse(inner, NumberStyles.None, CultureInfo.InvariantCulture, out var index))
                {
                    if (value.ValueKind != JsonValueKind.Array || index >= value.GetArrayLength())
                        return false;
                    value = value[index];
                }
                else
                {
                    error = $"invalid path \"{path}\": [{inner}] should be a number or a quoted name";
                    return false;
                }

                continue;
            }

            var end = text.IndexOfAny(['.', '['], i);
            if (end < 0)
                end = text.Length;

            if (!Step(ref value, text[i..end]))
                return false;
            i = end;
        }

        return true;
    }

    private static bool Step(ref JsonElement value, string name)
    {
        if (value.ValueKind == JsonValueKind.Object && value.TryGetProperty(name, out var child))
        {
            value = child;
            return true;
        }

        if (value.ValueKind == JsonValueKind.Array && name == "length")
        {
            using var count = JsonDocument.Parse(value.GetArrayLength().ToString(CultureInfo.InvariantCulture));
            value = count.RootElement.Clone();
            return true;
        }

        return false;
    }
}
