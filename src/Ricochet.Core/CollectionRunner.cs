using System.Globalization;
using System.Text;
using System.Xml;

namespace Ricochet.Core;

/// <summary>What was asked for can't be run at all. The message is suitable for showing to the user.</summary>
public sealed class RunSetupException(string message) : Exception(message);

public sealed record RunOptions
{
    public string? CollectionName { get; init; }
    public string? FolderName { get; init; }
    public string? EnvironmentName { get; init; }

    /// <summary>Values given for this run. They override every scope, and are treated as Secrets in reports.</summary>
    public IReadOnlyList<KeyValue> Overrides { get; init; } = [];

    public bool StopOnFirstFailure { get; init; }
}

public sealed record RequestRunResult(
    string Collection,
    string Path,
    string Method,
    int? StatusCode,
    long ElapsedMs,
    IReadOnlyList<AssertionResult> Assertions,
    string? Error,
    bool HasScripts)
{
    public bool Passed => Error is null && Assertions.All(a => a.Passed);

    /// <summary>Names of the Variables this Request set. Values are deliberately left out.</summary>
    public IReadOnlyList<string> Captured { get; init; } = [];
}

public sealed record RunReport(IReadOnlyList<RequestRunResult> Results, bool Stopped)
{
    public bool Passed => Results.All(r => r.Passed);
    public int FailedRequests => Results.Count(r => !r.Passed);
    public int TotalAssertions => Results.Sum(r => r.Assertions.Count);
    public int FailedAssertions => Results.Sum(r => r.Assertions.Count(a => !a.Passed));
    public int RequestsWithScripts => Results.Count(r => r.HasScripts);
    public int RequestsWithoutAssertions => Results.Count(r => r.Error is null && r.Assertions.Count == 0 && r.Captured.Count == 0);
}

/// <summary>Runs Requests in order and checks their assertions. Reads the Workspace and never changes it.</summary>
public sealed class CollectionRunner(RequestSender sender)
{
    private const string Mask = "***";

    // Masking very short values would scramble unrelated output, such as a status code.
    private const int MinMaskedLength = 4;

    private sealed record Planned(Collection Collection, string Path, ApiRequest Request, bool HasScripts);

    /// <exception cref="RunSetupException">Nothing could be selected to run.</exception>
    public async Task<RunReport> RunAsync(
        Workspace workspace,
        RunOptions options,
        Action<RequestRunResult>? onResult = null,
        CancellationToken cancellationToken = default)
    {
        var environment = FindEnvironment(workspace, options.EnvironmentName);
        var planned = Plan(workspace, options);

        var results = new List<RequestRunResult>();
        var stopped = false;
        var captured = new List<KeyValue>();
        Collection? currentCollection = null;
        foreach (var item in planned)
        {
            // Like other Variables, captured values belong to one Collection.
            if (!ReferenceEquals(item.Collection, currentCollection))
            {
                captured.Clear();
                currentCollection = item.Collection;
            }

            var result = await RunOneAsync(item, workspace, environment, captured, options, cancellationToken);
            results.Add(result);
            onResult?.Invoke(result);

            if (!result.Passed && options.StopOnFirstFailure)
            {
                stopped = results.Count < planned.Count;
                break;
            }
        }

        return new RunReport(results, stopped);
    }

    private static ApiEnvironment? FindEnvironment(Workspace workspace, string? name)
    {
        if (string.IsNullOrEmpty(name))
            return null;

        return workspace.Environments.FirstOrDefault(e => e.Name.Equals(name, StringComparison.OrdinalIgnoreCase))
            ?? throw new RunSetupException($"No environment is named \"{name}\". Available: {NamesOf(workspace.Environments.Select(e => e.Name))}");
    }

    private static string NamesOf(IEnumerable<string> names)
    {
        var list = names.ToList();
        return list.Count > 0 ? string.Join(", ", list) : "none";
    }

    private static List<Planned> Plan(Workspace workspace, RunOptions options)
    {
        var collections = workspace.Collections.AsEnumerable();
        if (!string.IsNullOrEmpty(options.CollectionName))
        {
            collections = workspace.Collections.Where(c => c.Name.Equals(options.CollectionName, StringComparison.OrdinalIgnoreCase)).ToList();
            if (!collections.Any())
                throw new RunSetupException($"No collection is named \"{options.CollectionName}\". Available: {NamesOf(workspace.Collections.Select(c => c.Name))}");
        }

        var planned = new List<Planned>();
        foreach (var collection in collections)
        {
            var folderMatched = string.IsNullOrEmpty(options.FolderName);
            foreach (var item in collection.Items)
                Walk(collection, item, "", folderMatched, collection.Scripts.Count > 0, options, planned);
        }

        // A run that tests nothing must not look like a success.
        if (planned.Count == 0)
        {
            throw new RunSetupException(string.IsNullOrEmpty(options.FolderName)
                ? "There are no requests to run."
                : $"No requests were found in a folder named \"{options.FolderName}\".");
        }

        return planned;
    }

    private static void Walk(Collection collection, CollectionItem item, string parentPath, bool folderMatched, bool parentHasScripts, RunOptions options, List<Planned> planned)
    {
        var path = parentPath.Length > 0 ? $"{parentPath} / {item.Name}" : item.Name;
        var hasScripts = parentHasScripts || item.Scripts.Count > 0;

        if (item.Request is { } request)
        {
            if (folderMatched)
                planned.Add(new Planned(collection, path, request, hasScripts));
            return;
        }

        var matched = folderMatched || item.Name.Equals(options.FolderName, StringComparison.OrdinalIgnoreCase);
        foreach (var child in item.Children)
            Walk(collection, child, path, matched, hasScripts, options, planned);
    }

    private async Task<RequestRunResult> RunOneAsync(Planned item, Workspace workspace, ApiEnvironment? environment, List<KeyValue> captured, RunOptions options, CancellationToken cancellationToken)
    {
        var variables = VariableResolver.Layer(workspace.Globals, item.Collection.Variables, environment?.Variables, captured, options.Overrides);
        var resolved = VariableResolver.Apply(item.Request, variables);

        RequestRunResult Failed(string error, int? status = null, long elapsed = 0) =>
            new(item.Collection.Name, item.Path, item.Request.Method, status, elapsed, [], MaskIn(error, SecretsToMask(variables, options.Overrides, captured, resolved)), item.HasScripts);

        // Sending a literal {{token}} would fail confusingly, and could reach the wrong place.
        var missing = VariableResolver.FindPlaceholders(resolved);
        if (missing.Count > 0)
            return Failed($"Not sent, because there is no value for: {string.Join(", ", missing)}");

        try
        {
            var response = await sender.SendAsync(resolved, cancellationToken);
            var capture = AssertionEvaluator.EvaluateCaptures(resolved.Captures, response);
            foreach (var value in capture.Values)
            {
                captured.RemoveAll(c => c.Key == value.Key);
                captured.Add(value);
            }

            // Worked out after capturing, so a new value is masked in this response's own results too.
            var mask = SecretsToMask(variables, options.Overrides, captured, resolved);
            var assertions = AssertionEvaluator.Evaluate(resolved.Assertions, response)
                .Concat(capture.Failures)
                .Select(a => a with { Description = MaskIn(a.Description, mask), Message = MaskIn(a.Message, mask) })
                .ToList();

            return new RequestRunResult(item.Collection.Name, item.Path, item.Request.Method, response.StatusCode,
                (long)response.Elapsed.TotalMilliseconds, assertions, null, item.HasScripts)
            {
                Captured = capture.Values.Select(v => v.Key).ToList(),
            };
        }
        catch (RequestFailedException ex)
        {
            return Failed(ex.Message);
        }
    }

    private static List<string> SecretsToMask(IEnumerable<KeyValue> variables, IEnumerable<KeyValue> overrides, IEnumerable<KeyValue> captured, ApiRequest resolved)
    {
        // Captures are often tokens, and nothing marks which ones, so all of them are masked.
        var values = variables.Where(v => v.IsSecret).Concat(overrides).Concat(captured).Select(v => v.Value).ToList();
        if (resolved.Auth is { } auth)
            values.AddRange([auth.Token, auth.Password, auth.KeyValue]);

        // Longest first, so that a value containing another is masked whole.
        return values.Where(v => v.Length >= MinMaskedLength).Distinct().OrderByDescending(v => v.Length).ToList();
    }

    private static string MaskIn(string text, List<string> secrets) =>
        secrets.Aggregate(text, (current, secret) => current.Replace(secret, Mask, StringComparison.Ordinal));
}

public static class RunReports
{
    public static string Summary(RunReport report)
    {
        var text = new StringBuilder();
        text.Append($"{Count(report.Results.Count, "request")}: {report.Results.Count - report.FailedRequests} passed, {report.FailedRequests} failed. ");
        text.Append($"{Count(report.TotalAssertions, "assertion")}: {report.TotalAssertions - report.FailedAssertions} passed, {report.FailedAssertions} failed.");
        return text.ToString();
    }

    /// <summary>Things that could make a passing run less meaningful than it looks.</summary>
    public static IReadOnlyList<string> Cautions(RunReport report)
    {
        var cautions = new List<string>();
        if (report.Stopped)
            cautions.Add("Stopped at the first failure, so later requests did not run.");
        if (report.RequestsWithScripts > 0)
            cautions.Add($"{Count(report.RequestsWithScripts, "request")} had Postman scripts that were not run, so any tests in them did not count.");
        if (report.RequestsWithoutAssertions > 0)
            cautions.Add($"{Count(report.RequestsWithoutAssertions, "request")} had no assertions, so only getting a response was checked.");
        return cautions;
    }

    private static string Count(int count, string noun) => count == 1 ? $"1 {noun}" : $"{count} {noun}s";

    /// <summary>Writes xUnit's v2 XML format, with one assembly per Collection and one test per Request.</summary>
    public static void WriteXunit(RunReport report, Stream output, DateTimeOffset startedAt)
    {
        using var xml = XmlWriter.Create(output, new XmlWriterSettings { Indent = true, Encoding = new UTF8Encoding(false) });
        xml.WriteStartElement("assemblies");
        xml.WriteAttributeString("timestamp", startedAt.ToString("MM/dd/yyyy HH:mm:ss", CultureInfo.InvariantCulture));

        foreach (var suite in report.Results.GroupBy(r => r.Collection))
        {
            var name = Clean(suite.Key);
            var time = Seconds(suite.Sum(r => r.ElapsedMs));
            var total = Number(suite.Count());
            var passed = Number(suite.Count(r => r.Passed));
            var failed = Number(suite.Count(r => !r.Passed));

            xml.WriteStartElement("assembly");
            xml.WriteAttributeString("name", name);
            xml.WriteAttributeString("test-framework", "Ricochet");
            xml.WriteAttributeString("run-date", startedAt.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture));
            xml.WriteAttributeString("run-time", startedAt.ToString("HH:mm:ss", CultureInfo.InvariantCulture));
            xml.WriteAttributeString("time", time);
            xml.WriteAttributeString("total", total);
            xml.WriteAttributeString("passed", passed);
            xml.WriteAttributeString("failed", failed);
            xml.WriteAttributeString("skipped", "0");
            xml.WriteAttributeString("errors", "0");
            xml.WriteElementString("errors", "");

            xml.WriteStartElement("collection");
            xml.WriteAttributeString("name", name);
            xml.WriteAttributeString("time", time);
            xml.WriteAttributeString("total", total);
            xml.WriteAttributeString("passed", passed);
            xml.WriteAttributeString("failed", failed);
            xml.WriteAttributeString("skipped", "0");

            foreach (var result in suite)
            {
                var testName = Clean($"{result.Method} {result.Path}");
                xml.WriteStartElement("test");
                xml.WriteAttributeString("name", testName);
                xml.WriteAttributeString("type", name);
                xml.WriteAttributeString("method", testName);
                xml.WriteAttributeString("time", Seconds(result.ElapsedMs));
                xml.WriteAttributeString("result", result.Passed ? "Pass" : "Fail");

                if (!result.Passed)
                {
                    IEnumerable<string> problems = result.Error is not null
                        ? [result.Error]
                        : result.Assertions.Where(a => !a.Passed).Select(a => $"{a.Description}: {a.Message}");
                    xml.WriteStartElement("failure");
                    xml.WriteAttributeString("exception-type", result.Error is not null ? "RequestFailed" : "AssertionFailed");
                    xml.WriteStartElement("message");
                    xml.WriteCData(Clean(string.Join('\n', problems)).Replace("]]>", "]]&gt;", StringComparison.Ordinal));
                    xml.WriteEndElement();
                    xml.WriteEndElement();
                }

                xml.WriteEndElement();
            }

            xml.WriteEndElement();
            xml.WriteEndElement();
        }

        xml.WriteEndElement();
    }

    private static string Number(int value) => value.ToString(CultureInfo.InvariantCulture);

    private static string Seconds(long milliseconds) => (milliseconds / 1000d).ToString("0.###", CultureInfo.InvariantCulture);

    // Response text can hold control characters that XML can't represent, even escaped.
    private static string Clean(string text)
    {
        var clean = new StringBuilder(text.Length);
        for (var i = 0; i < text.Length; i++)
        {
            if (i + 1 < text.Length && char.IsSurrogatePair(text[i], text[i + 1]))
            {
                clean.Append(text[i]).Append(text[i + 1]);
                i++;
            }
            else
            {
                clean.Append(XmlConvert.IsXmlChar(text[i]) ? text[i] : '�');
            }
        }

        return clean.ToString();
    }
}
