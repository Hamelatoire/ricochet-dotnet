using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

public sealed class EnvironmentImportException(string message) : Exception(message);

/// <summary>What to do when an imported Environment has the same name as one already in the Workspace.</summary>
public enum EnvironmentClash
{
    /// <summary>Overwrite the existing Environment, keeping its id so local Secret values still apply.</summary>
    Replace,

    /// <summary>Add the Environment under a numbered name, such as "Staging (2)".</summary>
    Copy,

    /// <summary>Leave the existing Environment as it is.</summary>
    Skip,
}

/// <summary>The result of placing an imported Environment into a Workspace. <see cref="Placed"/> is null when skipped.</summary>
public sealed record EnvironmentPlacement(IReadOnlyList<ApiEnvironment> Environments, ApiEnvironment? Placed);

/// <summary>An Environment as written by <see cref="EnvironmentTransfer.Export"/>. Secret values are never written.</summary>
public sealed record EnvironmentFile
{
    public string Format { get; set; } = EnvironmentTransfer.FormatName;
    public int Version { get; set; } = EnvironmentTransfer.FormatVersion;
    public string Name { get; set; } = "";
    public IReadOnlyList<EnvironmentFileVariable> Variables { get; set; } = [];
}

public sealed record EnvironmentFileVariable
{
    public string Key { get; set; } = "";

    /// <summary>Null for a Secret, so the file holds the name but no value.</summary>
    public string? Value { get; set; }

    public bool Enabled { get; set; } = true;
    public bool IsSecret { get; set; }
}

public sealed record EnvironmentImportResult(ApiEnvironment Environment, string Summary, IReadOnlyList<ImportIssue> Issues);

/// <summary>
/// Moves one Environment in and out of a file. Secret values are left out on export. See docs/adr/0036-export-and-import-environments-without-secrets.md.
/// </summary>
public static partial class EnvironmentTransfer
{
    public const string FormatName = "ricochet-environment";
    public const int FormatVersion = 1;

    public const string RenamedVariables = "Variable names changed to fit";

    [GeneratedRegex(@"^[A-Za-z0-9_.\-]+$")]
    private static partial Regex ValidName();

    [GeneratedRegex(@"[^A-Za-z0-9_.\-]")]
    private static partial Regex InvalidNameCharacters();

    public static string Export(ApiEnvironment environment)
    {
        var file = new EnvironmentFile
        {
            Name = environment.Name,
            Variables = environment.Variables.Select(v => new EnvironmentFileVariable
            {
                Key = v.Key,
                Value = v.IsSecret ? null : v.Value,
                Enabled = v.Enabled,
                IsSecret = v.IsSecret,
            }).ToList(),
        };

        return JsonSerializer.Serialize(file, WorkspaceJsonContext.Default.EnvironmentFile);
    }

    /// <summary>How many Secret variables an Environment has. Their values are never exported or imported.</summary>
    public static int SecretCount(ApiEnvironment environment) => environment.Variables.Count(v => v.IsSecret);

    /// <summary>Finds the Environment with this name, ignoring case. Null when there is none, or more than one.</summary>
    public static ApiEnvironment? FindEnvironment(IEnumerable<ApiEnvironment> environments, string name)
    {
        var matches = environments.Where(e => e.Name.Equals(name, StringComparison.OrdinalIgnoreCase)).ToList();
        return matches.Count == 1 ? matches[0] : null;
    }

    /// <summary>Reads a Ricochet environment file, or a Postman environment export.</summary>
    /// <exception cref="EnvironmentImportException">The file is not an environment we can read.</exception>
    public static EnvironmentImportResult Import(string json, string fileName)
    {
        bool isOurs;
        try
        {
            using var document = JsonDocument.Parse(json, new JsonDocumentOptions { AllowTrailingCommas = true, CommentHandling = JsonCommentHandling.Skip });
            isOurs = document.RootElement.ValueKind == JsonValueKind.Object
                && document.RootElement.TryGetProperty("format", out var format)
                && format.ValueKind == JsonValueKind.String
                && format.GetString() == FormatName;
        }
        catch (JsonException ex)
        {
            throw new EnvironmentImportException($"Not valid JSON: {ex.Message}");
        }

        return isOurs ? ReadOwnFile(json, fileName) : ReadPostmanFile(json, fileName);
    }

    /// <summary>
    /// Adds an imported Environment to the existing ones. The existing list is not modified.
    /// A name clash is resolved with <paramref name="clash"/>; without a clash the Environment is simply added.
    /// </summary>
    public static EnvironmentPlacement Place(IReadOnlyList<ApiEnvironment> existing, ApiEnvironment incoming, EnvironmentClash clash)
    {
        var index = existing.ToList().FindIndex(e => e.Name.Equals(incoming.Name, StringComparison.OrdinalIgnoreCase));
        if (index < 0)
        {
            var added = incoming;
            return new EnvironmentPlacement([.. existing, added], added);
        }

        switch (clash)
        {
            case EnvironmentClash.Skip:
                return new EnvironmentPlacement(existing, null);

            case EnvironmentClash.Copy:
                var copy = incoming with
                {
                    Id = Guid.NewGuid().ToString("N"),
                    Name = WorkspaceImport.UniqueName(incoming.Name, existing.Select(e => e.Name)),
                };
                return new EnvironmentPlacement([.. existing, copy], copy);

            default:
                var previous = existing[index];
                var replacement = incoming with
                {
                    Id = previous.Id,
                    Variables = KeepLocalSecrets(incoming.Variables, previous.Variables),
                };
                var list = existing.ToList();
                list[index] = replacement;
                return new EnvironmentPlacement(list, replacement);
        }
    }

    /// <summary>A Secret the file leaves without a value keeps the value this machine already has for it.</summary>
    private static List<KeyValue> KeepLocalSecrets(IEnumerable<KeyValue> incoming, IEnumerable<KeyValue> local)
    {
        var known = local.Where(v => v.IsSecret && v.Value.Length > 0).ToDictionary(v => v.Key, v => v.Value, StringComparer.Ordinal);
        return incoming.Select(v => v.IsSecret && v.Value.Length == 0 && known.TryGetValue(v.Key, out var value)
            ? v with { Value = value }
            : v).ToList();
    }

    private static EnvironmentImportResult ReadOwnFile(string json, string fileName)
    {
        EnvironmentFile? file;
        try
        {
            file = JsonSerializer.Deserialize(json, WorkspaceJsonContext.Default.EnvironmentFile);
        }
        catch (JsonException ex)
        {
            throw new EnvironmentImportException($"Not a readable environment file: {ex.Message}");
        }

        if (file is null)
            throw new EnvironmentImportException("This environment file is empty.");
        if (file.Version > FormatVersion)
            throw new EnvironmentImportException($"This environment file uses format version {file.Version}, which is newer than this app reads ({FormatVersion}).");

        var name = file.Name.Length > 0 ? file.Name : Path.GetFileNameWithoutExtension(fileName);
        var issues = new List<ImportIssue>();
        var variables = new List<KeyValue>();
        foreach (var variable in file.Variables)
        {
            if (variable.Key.Length == 0)
                continue;

            var key = FitName(variable.Key, name, issues);
            var value = variable.Value ?? "";
            if (variable.IsSecret && value.Length == 0)
                issues.Add(new ImportIssue(ImportCategories.Secrets, name, $"{key} (the export has no value, so set it again)"));

            variables.Add(new KeyValue(key, value, variable.Enabled, variable.IsSecret));
        }

        return Build(name, variables, issues);
    }

    private static EnvironmentImportResult ReadPostmanFile(string json, string fileName)
    {
        ImportResult result;
        try
        {
            result = PostmanImporter.Import(json, fileName);
        }
        catch (PostmanImportException ex)
        {
            throw new EnvironmentImportException(ex.Message);
        }

        if (result.Environment is null)
            throw new EnvironmentImportException("This Postman file is not an environment. Use it as a collection or globals instead.");

        var issues = result.Issues.ToList();
        var name = result.Environment.Name;
        var variables = new List<KeyValue>();
        foreach (var variable in result.Environment.Variables)
            variables.Add(variable with { Key = FitName(variable.Key, name, issues) });

        return Build(name, variables, issues);
    }

    private static EnvironmentImportResult Build(string name, List<KeyValue> variables, List<ImportIssue> issues)
    {
        var environment = new ApiEnvironment { Name = name, Variables = variables };
        var summary = $"Imported environment \"{name}\" with {variables.Count} {(variables.Count == 1 ? "variable" : "variables")}.";
        return new EnvironmentImportResult(environment, summary, issues);
    }

    /// <summary>Variable names may only use letters, digits, dash, underscore and dot. Other characters become underscores, and the change is reported.</summary>
    private static string FitName(string key, string environmentName, List<ImportIssue> issues)
    {
        if (ValidName().IsMatch(key))
            return key;

        var fitted = InvalidNameCharacters().Replace(key, "_");
        issues.Add(new ImportIssue(RenamedVariables, environmentName, $"\"{key}\" is now \"{fitted}\""));
        return fitted;
    }
}
