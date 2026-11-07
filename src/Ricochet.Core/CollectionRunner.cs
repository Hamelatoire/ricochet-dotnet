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

    /// <summary>Run only Requests with at least one of these tags, which their Folders give them (ADR 0030). Empty means no filter.</summary>
    public IReadOnlyList<string> IncludeTags { get; init; } = [];

    /// <summary>Skip Requests with any of these tags. Skipping wins over including.</summary>
    public IReadOnlyList<string> ExcludeTags { get; init; } = [];

    /// <summary>
    /// Keep cookies between this run's Requests, as a browser does (ADR 0012). They start empty, are never written to a file,
    /// and are dropped when the run ends, so one run never sees another run's cookies. Off by default.
    /// </summary>
    public bool KeepCookies { get; init; }
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

/// <summary>A Request that was selected for the run but left out because it, or a Folder above it, is hidden (ADR 0032).</summary>
public sealed record SkippedRequest(string Collection, string Path);

public sealed record RunReport(IReadOnlyList<RequestRunResult> Results, bool Stopped)
{
    /// <summary>Hidden Requests that would otherwise have run. They are named, so a skipped test can't go unnoticed.</summary>
    public IReadOnlyList<SkippedRequest> SkippedHidden { get; init; } = [];

    public bool Passed => Results.All(r => r.Passed);
    public int FailedRequests => Results.Count(r => !r.Passed);
    public int TotalAssertions => Results.Sum(r => r.Assertions.Count);
    public int FailedAssertions => Results.Sum(r => r.Assertions.Count(a => !a.Passed));
    public int RequestsWithScripts => Results.Count(r => r.HasScripts);
    public int RequestsWithoutAssertions => Results.Count(r => r.Error is null && r.Assertions.Count == 0 && r.Captured.Count == 0);
}

/// <summary>Runs Requests in order and checks their assertions. Reads the Workspace and never changes it.</summary>
public sealed class CollectionRunner(RequestSender sender, ExternalSecretResolver? secrets = null)
{
    private const string Mask = "***";

    private readonly ExternalSecretResolver _secrets = secrets ?? new ExternalSecretResolver();

    // Masking very short values would scramble unrelated output, such as a status code.
    private const int MinMaskedLength = 4;

    private sealed record Planned(Collection Collection, string ItemId, string Path, ApiRequest Request, bool HasScripts);

    /// <exception cref="RunSetupException">Nothing could be selected to run.</exception>
    public async Task<RunReport> RunAsync(
        Workspace workspace,
        RunOptions options,
        Action<RequestRunResult>? onResult = null,
        CancellationToken cancellationToken = default)
    {
        var environment = FindEnvironment(workspace, options.EnvironmentName);
        var (planned, hidden) = Plan(workspace, options);

        var results = new List<RequestRunResult>();
        var stopped = false;
        var captured = new List<KeyValue>();
        Collection? currentCollection = null;
        // Null unless the run keeps cookies. Disposing it at the end drops them (ADR 0012).
        using var runCookies = options.KeepCookies ? sender.BeginRunCookies() : null;
        foreach (var item in planned)
        {
            // Like other Variables, captured values belong to one Collection.
            if (!ReferenceEquals(item.Collection, currentCollection))
            {
                captured.Clear();
                currentCollection = item.Collection;
            }

            var result = await RunOneAsync(item, workspace, environment, captured, options, runCookies, cancellationToken);
            results.Add(result);
            onResult?.Invoke(result);

            if (!result.Passed && options.StopOnFirstFailure)
            {
                stopped = results.Count < planned.Count;
                break;
            }
        }

        return new RunReport(results, stopped) { SkippedHidden = hidden };
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

    private static (List<Planned> Planned, List<SkippedRequest> Hidden) Plan(Workspace workspace, RunOptions options)
    {
        var collections = workspace.Collections.AsEnumerable();
        if (!string.IsNullOrEmpty(options.CollectionName))
        {
            collections = workspace.Collections.Where(c => c.Name.Equals(options.CollectionName, StringComparison.OrdinalIgnoreCase)).ToList();
            if (!collections.Any())
                throw new RunSetupException($"No collection is named \"{options.CollectionName}\". Available: {NamesOf(workspace.Collections.Select(c => c.Name))}");
        }

        var planned = new List<Planned>();
        var hidden = new List<SkippedRequest>();
        foreach (var collection in collections)
        {
            var folderMatched = string.IsNullOrEmpty(options.FolderName);
            foreach (var item in collection.Items)
                Walk(collection, item, "", folderMatched, collection.Scripts.Count > 0, [], false, options, planned, hidden);
        }

        // A run that tests nothing must not look like a success.
        if (planned.Count == 0)
        {
            if (hidden.Count > 0)
                throw new RunSetupException(hidden.Count == 1
                    ? "The only request that would run is hidden. Show it in the app to run it."
                    : $"All {hidden.Count} requests that would run are hidden. Show them in the app to run them.");
            if (!string.IsNullOrEmpty(options.FolderName))
                throw new RunSetupException($"No requests were found in a folder named \"{options.FolderName}\".");
            if (options.IncludeTags.Count > 0 || options.ExcludeTags.Count > 0)
                throw new RunSetupException("No requests match the tags given.");
            throw new RunSetupException("There are no requests to run.");
        }

        return (planned, hidden);
    }

    private static void Walk(Collection collection, CollectionItem item, string parentPath, bool folderMatched, bool parentHasScripts,
        IReadOnlyList<string> inheritedTags, bool parentHidden, RunOptions options, List<Planned> planned, List<SkippedRequest> hidden)
    {
        // A hidden folder, and everything in it, is not run (ADR 0032). Hidden Requests that were selected are named instead.
        var isHidden = parentHidden || collection.HiddenItemIds?.Contains(item.Id) == true;

        var path = parentPath.Length > 0 ? $"{parentPath} / {item.Name}" : item.Name;
        var hasScripts = parentHasScripts || item.Scripts.Count > 0;

        // Requests inherit the tags of every Folder above them (ADR 0030).
        var tags = inheritedTags.Concat(item.Tags ?? []).ToList();

        if (item.Request is { } request)
        {
            if (folderMatched && FolderTags.Selects(inheritedTags, options.IncludeTags, options.ExcludeTags))
            {
                if (isHidden)
                    hidden.Add(new SkippedRequest(collection.Name, path));
                else
                    planned.Add(new Planned(collection, item.Id, path, request, hasScripts));
            }
            return;
        }

        var matched = folderMatched || item.Name.Equals(options.FolderName, StringComparison.OrdinalIgnoreCase);
        foreach (var child in item.Children)
            Walk(collection, child, path, matched, hasScripts, tags, isHidden, options, planned, hidden);
    }

    private async Task<RequestRunResult> RunOneAsync(Planned item, Workspace workspace, ApiEnvironment? environment, List<KeyValue> captured, RunOptions options, RequestSender.RunCookies? runCookies, CancellationToken cancellationToken)
    {
        var layered = VariableResolver.Layer(workspace.Globals, item.Collection.Variables, VariableResolver.FolderScope(item.Collection, item.ItemId),
            environment?.Variables, captured, options.Overrides);
        var variables = (IReadOnlyList<KeyValue>)layered;
        var resolved = VariableResolver.Apply(item.Request, variables);

        RequestRunResult Failed(string error, int? status = null, long elapsed = 0) =>
            new(item.Collection.Name, item.Path, item.Request.Method, status, elapsed, [], MaskIn(error, SecretsToMask(variables, options.Overrides, captured, resolved)), item.HasScripts);

        // Secret references are read just before sending, and only the send uses the values (ADR 0025).
        try
        {
            variables = await VariableResolver.ResolveReferencesAsync(item.Request, layered, _secrets, cancellationToken);
            resolved = VariableResolver.Apply(item.Request, variables);
        }
        catch (RequestFailedException ex)
        {
            return Failed(ex.Message);
        }

        // Sending a literal {{token}} would fail confusingly, and could reach the wrong place.
        var missing = VariableResolver.FindPlaceholders(resolved);
        if (missing.Count > 0)
            return Failed($"Not sent, because there is no value for: {string.Join(", ", missing)}");

        try
        {
            var response = runCookies is null
                ? await sender.SendAsync(resolved, cancellationToken)
                : await sender.SendAsync(resolved, runCookies, cancellationToken);
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

    internal static List<string> SecretsToMask(IEnumerable<KeyValue> variables, IEnumerable<KeyValue> overrides, IEnumerable<KeyValue> captured, ApiRequest resolved)
    {
        // Captures are often tokens, and nothing marks which ones, so all of them are masked.
        var values = variables.Where(v => v.IsSecret).Concat(overrides).Concat(captured).Select(v => v.Value).ToList();
        if (resolved.Auth is { } auth)
            values.AddRange(WorkspaceSecrets.AuthSecretValues(auth));

        // Longest first, so that a value containing another is masked whole.
        return values.Where(v => v.Length >= MinMaskedLength).Distinct().OrderByDescending(v => v.Length).ToList();
    }

    internal static string MaskIn(string text, List<string> secrets) =>
        secrets.Aggregate(text, (current, secret) => current.Replace(secret, Mask, StringComparison.Ordinal));
}

public static class RunReports
{
    public static string Summary(RunReport report)
    {
        var text = new StringBuilder();
        text.Append($"{Count(report.Results.Count, "request")}: {report.Results.Count - report.FailedRequests} passed, {report.FailedRequests} failed. ");
        text.Append($"{Count(report.TotalAssertions, "assertion")}: {report.TotalAssertions - report.FailedAssertions} passed, {report.FailedAssertions} failed.");
        if (report.SkippedHidden.Count > 0)
            text.Append($" {Count(report.SkippedHidden.Count, "request")} skipped because hidden.");
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
        if (report.SkippedHidden.Count > 0)
            cautions.Add($"Skipped hidden requests, which did not run: {string.Join(", ", report.SkippedHidden.Select(s => $"{s.Collection} / {s.Path}"))}.");
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
