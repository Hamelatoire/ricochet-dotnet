using System.Collections;
using System.IO.Compression;
using Ricochet.Core;

namespace Ricochet.Cli;

/// <summary>Runs a Workspace's requests and assertions, for CI. It only reads the Workspace.</summary>
internal static class Program
{
    private const int ExitPassed = 0;
    private const int ExitTestsFailed = 1;
    private const int ExitRefused = 1;
    private const int ExitCannotRun = 2;
    private const int ExitCancelled = 130;
    private const string VariablePrefix = "RICOCHET_VAR_";

    private const string Usage = """
        Ricochet: run a workspace's requests and check their assertions.

        Usage:
          ricochet run [folder] [options]
          ricochet export <collection> --postman <file> | --openapi <file>   (see ricochet export --help)
          ricochet env export <environment> <file.json> | env import <file.json>   (see ricochet env --help)
          ricochet docs <collection> <file.html> [--force]   (see ricochet docs --help)
          ricochet zip export <workspace> <file.zip> | zip import <file.zip> <new-folder>   (see ricochet zip --help)
          ricochet clone <git-url> <target-folder>   (see ricochet clone --help)

        Arguments:
          folder                 The workspace folder. Defaults to the current folder.

        Options:
          --collection <name>    Run only this collection.
          --folder <name>        Run only requests inside folders with this name.
          --tag <name>           Run only requests with this tag, given by their folders. Repeatable; any match counts.
          --exclude-tag <name>   Skip requests with this tag. Repeatable; wins over --tag.
          --env <name>           Use this environment.
          --var <name=value>     Set a variable, overriding every scope. Repeatable.
          --data <file>          Run the collection once per row of a .csv or .json data file. Each row's columns
                                 are variables for that run, and each run's results are printed under its row number.
          --xunit <file>         Also write an xUnit XML report.
          --timeout <seconds>    Time limit for each request. Default: 120.
          --bail                 Stop at the first failure.
          --cookies              Keep cookies between the requests of each run, as a browser does. Cookies are
                                 kept in memory only and are never written to a file. Each run starts with none.
          --insecure             Skip TLS certificate checks. Only for servers you trust, such as a local test server.
          --help                 Show this help.

        Variables can also come from a .env file in the workspace folder, or from environment
        variables named RICOCHET_VAR_<name>. Environment variables win over .env, and --var wins over both.
        Prefer those for secrets in CI, because --var values can show up in process lists.
        Values set either way are masked in the output.

        Exit codes:
          0  Everything passed.
          1  A request or assertion failed, in any data row.
          2  The run could not happen, for example because a workspace file or the data file is unreadable.
        """;

    private sealed class Arguments
    {
        public string Folder { get; set; } = ".";
        public string? Collection { get; set; }
        public string? FolderName { get; set; }
        public List<string> Tags { get; } = [];
        public List<string> ExcludeTags { get; } = [];
        public string? Environment { get; set; }
        public string? Xunit { get; set; }
        public string? DataFile { get; set; }
        public int TimeoutSeconds { get; set; } = 120;
        public bool Bail { get; set; }
        public bool Insecure { get; set; }
        public bool Cookies { get; set; }
        public List<KeyValue> Variables { get; } = [];
    }

    private static async Task<int> Main(string[] args)
    {
        if (args.Length == 0 || args[0] is "--help" or "-h" or "help")
        {
            Console.WriteLine(Usage);
            return args.Length == 0 ? ExitCannotRun : ExitPassed;
        }

        if (args[0] == "export")
            return ExportCommand(args.Skip(1).ToList());

        if (args[0] == "env")
            return EnvCommand(args.Skip(1).ToList());

        if (args[0] == "docs")
            return DocsCommand(args.Skip(1).ToList());

        if (args[0] == "zip")
            return ZipCommand(args.Skip(1).ToList());

        if (args[0] == "clone")
            return await CloneCommand(args.Skip(1).ToList());

        if (args[0] != "run")
            return CannotRun($"Unknown command \"{args[0]}\". Try: ricochet --help");

        if (!TryParse(args.Skip(1).ToList(), out var options, out var parseError))
            return parseError is null ? ExitPassed : CannotRun(parseError);

        using var cancel = new CancellationTokenSource();
        Console.CancelKeyPress += (_, e) =>
        {
            e.Cancel = true;
            cancel.Cancel();
        };

        try
        {
            return await RunAsync(options!, cancel.Token);
        }
        catch (OperationCanceledException)
        {
            Console.Error.WriteLine("Cancelled.");
            return ExitCancelled;
        }
        catch (RunSetupException ex)
        {
            return CannotRun(ex.Message);
        }
    }

    private static async Task<int> RunAsync(Arguments options, CancellationToken cancellationToken)
    {
        if (!WorkspaceFolder.LooksLikeWorkspace(options.Folder))
            return CannotRun($"\"{Path.GetFullPath(options.Folder)}\" is not a Ricochet workspace folder.");

        // Read before the Workspace, so a bad data file fails fast. Problems throw RunSetupException, which exits 2.
        var dataRows = options.DataFile is null ? null : ReadDataRows(options.DataFile);

        var folder = new WorkspaceFolder(Path.GetFullPath(options.Folder));
        var (workspace, issues) = folder.Load();

        // An unreadable file could be hiding tests, so a green result wouldn't mean anything.
        if (issues.Count > 0)
        {
            foreach (var issue in issues)
                Console.Error.WriteLine($"{issue.Path}: {issue.Problem}");
            return CannotRun("Some workspace files could not be used. Fix them and run again.");
        }

        workspace = WorkspaceSecrets.Merge(workspace, WorkspaceSecrets.ForWorkspace(LoadSecrets(), folder.Id));

        // Lowest priority first: .env, then RICOCHET_VAR_ variables, then --var. Later entries win.
        var dotEnv = DotEnv.Read(folder.Root);
        foreach (var line in dotEnv.SkippedLines)
            Console.Error.WriteLine($"Warning: {DotEnv.FileName} line {line} was skipped, because it is not KEY=VALUE.");

        var colors = new Colors(!Console.IsOutputRedirected && Environment.GetEnvironmentVariable("NO_COLOR") is null);
        string? currentCollection = null;
        var startedAt = DateTimeOffset.Now;

        // Certificate checks are a CI setting of their own. The app's Settings page does not reach the CLI (ADR 0007).
        if (options.Insecure)
            Console.Error.WriteLine("Warning: TLS certificate checks are off (--insecure). Requests are not verified.");

        var sender = new RequestSender(
            RequestSender.CreateDefaultClient(verifyCertificates: !options.Insecure),
            TimeSpan.FromSeconds(options.TimeoutSeconds));
        var runOptions = new RunOptions
        {
            CollectionName = options.Collection,
            FolderName = options.FolderName,
            IncludeTags = FolderTags.Normalize(options.Tags) ?? [],
            ExcludeTags = FolderTags.Normalize(options.ExcludeTags) ?? [],
            EnvironmentName = options.Environment,
            Overrides = [.. dotEnv.Variables, .. VariablesFromEnvironment(), .. options.Variables],
            StopOnFirstFailure = options.Bail,
            KeepCookies = options.Cookies,
        };

        RunReport report;
        if (dataRows is null)
        {
            report = await new CollectionRunner(sender).RunAsync(
                workspace,
                runOptions,
                result =>
                {
                    if (result.Collection != currentCollection)
                    {
                        currentCollection = result.Collection;
                        Console.WriteLine(result.Collection);
                    }

                    PrintResult(result, colors);
                },
                cancellationToken);
        }
        else
        {
            // Each row is its own run, printed under its row number. Row values are overrides, so they are masked like --var values.
            var rowReports = await new DataRowRunner(sender).RunAsync(
                workspace,
                runOptions,
                dataRows,
                row => PrintRow(row, dataRows.Count, colors),
                cancellationToken);

            var stoppedEarly = options.Bail && rowReports.Count < dataRows.Count;
            report = DataRowReports.Combine(rowReports, stoppedEarly);

            Console.WriteLine();
            Console.WriteLine($"Data rows: {rowReports.Count} run, {rowReports.Count(r => r.Report.Passed)} passed, {rowReports.Count(r => !r.Report.Passed)} failed.");
        }

        Console.WriteLine();
        Console.WriteLine(RunReports.Summary(report));
        foreach (var caution in RunReports.Cautions(report))
            Console.WriteLine($"Note: {caution}");

        if (options.Xunit is not null)
        {
            try
            {
                using var stream = File.Create(options.Xunit);
                RunReports.WriteXunit(report, stream, startedAt);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                return CannotRun($"Could not write the xUnit report: {ex.Message}");
            }
        }

        return report.Passed ? ExitPassed : ExitTestsFailed;
    }

    private static void PrintRow(DataRowReport row, int rowCount, Colors colors)
    {
        Console.WriteLine();
        Console.WriteLine($"Row {row.Row} of {rowCount}  {colors.Verdict(row.Report.Passed)}");

        string? currentCollection = null;
        foreach (var result in row.Report.Results)
        {
            if (result.Collection != currentCollection)
            {
                currentCollection = result.Collection;
                Console.WriteLine(result.Collection);
            }

            PrintResult(result, colors);
        }

        Console.WriteLine($"  {RunReports.Summary(row.Report)}");
    }

    // Data file values are read from a file the user chose, so they are never printed here. Only the row number is shown.
    private static IReadOnlyList<IReadOnlyList<KeyValue>> ReadDataRows(string path)
    {
        var format = DataFileParser.FormatFor(path);

        string text;
        try
        {
            text = File.ReadAllText(path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            throw new RunSetupException($"Could not read the data file \"{path}\": {ex.Message}");
        }

        var rows = DataFileParser.Parse(text, format);
        if (rows.Count == 0)
            throw new RunSetupException($"The data file \"{path}\" has no rows to run.");

        return rows;
    }

    private static void PrintResult(RequestRunResult result, Colors colors)
    {
        var status = result.StatusCode is { } code ? $"  {code}  {result.ElapsedMs} ms" : "";
        Console.WriteLine($"  {colors.Verdict(result.Passed)}  {result.Method} {result.Path}{status}");

        if (result.Error is not null)
            Console.WriteLine($"        {result.Error}");

        foreach (var assertion in result.Assertions)
        {
            var detail = assertion.Passed ? "" : $": {assertion.Message}";
            Console.WriteLine($"        {colors.Verdict(assertion.Passed)}  {assertion.Description}{detail}");
        }

        foreach (var name in result.Captured)
            Console.WriteLine($"        SET   {name}");
    }

    // Secrets are only available on a machine that has them. On CI, this is usually empty.
    private static Dictionary<string, string> LoadSecrets()
    {
        try
        {
            return SecretVaults.CreateDefault(WorkspaceStore.DefaultSecretsPath()).Load();
        }
        catch (SecretVaultException)
        {
            return [];
        }
    }

    private static IEnumerable<KeyValue> VariablesFromEnvironment()
    {
        foreach (DictionaryEntry entry in Environment.GetEnvironmentVariables())
        {
            var name = (string)entry.Key;
            if (name.StartsWith(VariablePrefix, StringComparison.OrdinalIgnoreCase) && name.Length > VariablePrefix.Length)
                yield return new KeyValue(name[VariablePrefix.Length..], (string?)entry.Value ?? "");
        }
    }

    /// <returns>False to stop. A null error means help was shown, which is not a failure.</returns>
    private static bool TryParse(List<string> args, out Arguments? result, out string? error)
    {
        var options = new Arguments();
        result = options;
        error = null;
        var positional = 0;

        for (var i = 0; i < args.Count; i++)
        {
            var arg = args[i];

            string? Value()
            {
                i++;
                return i < args.Count ? args[i] : null;
            }

            string? Missing() => $"{arg} needs a value.";

            switch (arg)
            {
                case "--help" or "-h":
                    Console.WriteLine(Usage);
                    return false;
                case "--bail":
                    options.Bail = true;
                    break;
                case "--insecure":
                    options.Insecure = true;
                    break;
                case "--cookies":
                    options.Cookies = true;
                    break;
                case "--collection":
                    options.Collection = Value();
                    if (options.Collection is null) { error = Missing(); return false; }
                    break;
                case "--folder":
                    options.FolderName = Value();
                    if (options.FolderName is null) { error = Missing(); return false; }
                    break;
                case "--tag":
                case "--exclude-tag":
                    var tag = Value();
                    if (string.IsNullOrWhiteSpace(tag)) { error = Missing(); return false; }
                    (arg == "--tag" ? options.Tags : options.ExcludeTags).Add(tag);
                    break;
                case "--env":
                    options.Environment = Value();
                    if (options.Environment is null) { error = Missing(); return false; }
                    break;
                case "--xunit":
                    options.Xunit = Value();
                    if (options.Xunit is null) { error = Missing(); return false; }
                    break;
                case "--data":
                    options.DataFile = Value();
                    if (options.DataFile is null) { error = Missing(); return false; }
                    break;
                case "--timeout":
                    if (!int.TryParse(Value(), out var seconds) || seconds <= 0)
                    {
                        error = "--timeout needs a positive number of seconds.";
                        return false;
                    }
                    options.TimeoutSeconds = seconds;
                    break;
                case "--var":
                    var pair = Value();
                    var equals = pair?.IndexOf('=') ?? -1;
                    if (equals <= 0)
                    {
                        error = "--var needs name=value.";
                        return false;
                    }
                    options.Variables.Add(new KeyValue(pair![..equals], pair[(equals + 1)..]));
                    break;
                default:
                    if (arg.StartsWith('-'))
                    {
                        error = $"Unknown option \"{arg}\". Try: ricochet --help";
                        return false;
                    }

                    if (positional++ > 0)
                    {
                        error = $"Unexpected argument \"{arg}\". Try: ricochet --help";
                        return false;
                    }

                    options.Folder = arg;
                    break;
            }
        }

        return true;
    }

    private const string CloneUsage = """
        Ricochet: clone a git repository as a workspace (ADR 0034).

        Usage:
          ricochet clone <git-url> <target-folder>

        The target folder must already exist and be empty. Ricochet runs git clone only: it never
        commits, pushes or stores credentials. Authentication uses your existing git setup, and a
        password in the address is refused.

        Exit codes:
          0  The clone succeeded and the folder is a Ricochet workspace.
          1  The clone succeeded, but the folder is not a Ricochet workspace. The files stay on disk.
          2  The clone could not run, for example because the target is not empty, git is not
             installed, or the clone failed.
        """;

    private static async Task<int> CloneCommand(List<string> args)
    {
        if (args.Count == 0 || args[0] is "--help" or "-h")
        {
            Console.WriteLine(CloneUsage);
            return args.Count == 0 ? ExitCannotRun : ExitPassed;
        }

        if (args.Count != 2 || args.Any(a => a.StartsWith('-')))
            return CannotRun("Usage: ricochet clone <git-url> <target-folder>. Try: ricochet clone --help");

        using var cancel = new CancellationTokenSource();
        Console.CancelKeyPress += (_, e) =>
        {
            e.Cancel = true;
            cancel.Cancel();
        };

        var result = await GitCloner.CloneAsync(args[0], args[1], cancel.Token);
        if (!result.Succeeded)
            return CannotRun(result.Message);

        if (!result.IsWorkspace)
        {
            Console.Error.WriteLine(result.Message);
            return ExitTestsFailed;
        }

        Console.WriteLine(result.Message);
        return ExitPassed;
    }

    private static int CannotRun(string message)
    {
        Console.Error.WriteLine(message);
        return ExitCannotRun;
    }

    private const string ExportUsage = """
        Ricochet: export a collection to a Postman v2.1 file, an OpenAPI 3 file, or both.

        Usage:
          ricochet export <collection> [options]

        Arguments:
          collection             The name of the collection to export.

        Options:
          --postman <file>       Write a Postman v2.1 collection to this file.
          --openapi <file>       Write an OpenAPI 3 document to this file.
          --workspace <folder>   The workspace folder. Defaults to the current folder.
          --force                Overwrite a file that already exists.
          --help                 Show this help.

        Secret values are never written. Secret Variables are left out, and a secret's value is
        replaced with {{name}}. Everything else that cannot be exported is listed after the export.
        Files are never written inside the workspace folder.

        Exit codes:
          0  The file was written.
          2  The export could not happen, for example because the collection does not exist,
             a file already exists without --force, or the workspace is unreadable.
        """;

    private sealed class ExportArguments
    {
        public string? Collection { get; set; }
        public string Workspace { get; set; } = ".";
        public string? Postman { get; set; }
        public string? OpenApi { get; set; }
        public bool Force { get; set; }
    }

    private static int ExportCommand(List<string> args)
    {
        if (!TryParseExport(args, out var options, out var parseError))
            return parseError is null ? ExitPassed : CannotRun(parseError);

        if (!WorkspaceFolder.LooksLikeWorkspace(options!.Workspace))
            return CannotRun($"\"{Path.GetFullPath(options.Workspace)}\" is not a Ricochet workspace folder.");

        var folder = new WorkspaceFolder(Path.GetFullPath(options.Workspace));
        var (workspace, issues) = folder.Load();

        // Same rule as run: an unreadable file could hide part of the collection, so the export would be incomplete.
        if (issues.Count > 0)
        {
            foreach (var issue in issues)
                Console.Error.WriteLine($"{issue.Path}: {issue.Problem}");
            return CannotRun("Some workspace files could not be used. Fix them and export again.");
        }

        // Secret values come from this machine's vault, so they can be replaced with their placeholders.
        workspace = WorkspaceSecrets.Merge(workspace, WorkspaceSecrets.ForWorkspace(LoadSecrets(), folder.Id));

        Collection collection;
        try
        {
            collection = ExportFiles.FindCollection(workspace.Collections, options.Collection!);
        }
        catch (ExportFileException ex)
        {
            return CannotRun(ex.Message);
        }

        // Check every target before writing any, so a refused second file never leaves the first one behind.
        var targets = new List<(string Label, string Path, Func<ExportResult> Export)>();
        try
        {
            if (options.Postman is not null)
                targets.Add(("Postman", ExportFiles.Check(options.Postman, folder.Root, options.Force), () => PostmanExporter.Export(collection)));
            if (options.OpenApi is not null)
                targets.Add(("OpenAPI", ExportFiles.Check(options.OpenApi, folder.Root, options.Force), () => OpenApiExporter.Export(collection)));
        }
        catch (ExportFileException ex)
        {
            return CannotRun(ex.Message);
        }

        if (targets.Count == 2 && string.Equals(targets[0].Path, targets[1].Path, StringComparison.OrdinalIgnoreCase))
            return CannotRun("--postman and --openapi need different files.");

        foreach (var target in targets)
        {
            var result = target.Export();
            try
            {
                ExportFiles.Write(target.Path, result.Json, folder.Root, options.Force);
            }
            catch (ExportFileException ex)
            {
                return CannotRun(ex.Message);
            }

            Console.WriteLine($"Wrote {target.Label} export to {target.Path}");
            Console.WriteLine(result.Notes.Count == 0
                ? "Nothing was left out."
                : result.ToReportText());
            Console.WriteLine();
        }

        return ExitPassed;
    }

    /// <returns>False to stop. A null error means help was shown, which is not a failure.</returns>
    private static bool TryParseExport(List<string> args, out ExportArguments? result, out string? error)
    {
        var options = new ExportArguments();
        result = options;
        error = null;
        var positional = 0;

        for (var i = 0; i < args.Count; i++)
        {
            var arg = args[i];

            string? Value()
            {
                i++;
                return i < args.Count ? args[i] : null;
            }

            switch (arg)
            {
                case "--help" or "-h":
                    Console.WriteLine(ExportUsage);
                    return false;
                case "--force":
                    options.Force = true;
                    break;
                case "--workspace":
                    options.Workspace = Value() ?? "";
                    if (options.Workspace.Length == 0) { error = "--workspace needs a folder."; return false; }
                    break;
                case "--postman":
                    options.Postman = Value();
                    if (string.IsNullOrWhiteSpace(options.Postman)) { error = "--postman needs a file."; return false; }
                    break;
                case "--openapi":
                    options.OpenApi = Value();
                    if (string.IsNullOrWhiteSpace(options.OpenApi)) { error = "--openapi needs a file."; return false; }
                    break;
                default:
                    if (arg.StartsWith('-'))
                    {
                        error = $"Unknown option \"{arg}\". Try: ricochet export --help";
                        return false;
                    }

                    if (positional++ > 0)
                    {
                        error = $"Unexpected argument \"{arg}\". Try: ricochet export --help";
                        return false;
                    }

                    options.Collection = arg;
                    break;
            }
        }

        if (options.Collection is null)
        {
            error = "Name the collection to export. Try: ricochet export --help";
            return false;
        }

        if (options.Postman is null && options.OpenApi is null)
        {
            error = "Choose at least one format: --postman <file> or --openapi <file>.";
            return false;
        }

        return true;
    }

    private const string DocsUsage = """
        Ricochet: write one collection as a self-contained HTML page for people who do not use Ricochet (ADR 0028).

        Usage:
          ricochet docs <collection> <file.html> [options]

        Arguments:
          collection             The name of the collection to document.
          file.html              The page to write. It must be outside the workspace folder.

        Options:
          --workspace <folder>   The workspace folder. Defaults to the current folder.
          --force                Overwrite a file that already exists.
          --help                 Show this help.

        Secret values are shown as ***. The page has no scripts and makes no network calls.

        Exit codes:
          0  The page was written.
          2  The page could not be written, for example because the collection does not exist,
             a file already exists without --force, or the workspace is unreadable.
        """;

    private static int DocsCommand(List<string> args)
    {
        if (args.Any(a => a is "--help" or "-h"))
        {
            Console.WriteLine(DocsUsage);
            return ExitPassed;
        }

        var force = args.Remove("--force");
        var workspace = ".";
        var positional = new List<string>();
        for (var i = 0; i < args.Count; i++)
        {
            if (args[i] == "--workspace")
            {
                if (i + 1 >= args.Count) return CannotRun("--workspace needs a folder.");
                workspace = args[++i];
            }
            else if (args[i].StartsWith('-'))
            {
                return CannotRun($"Unknown option \"{args[i]}\". Try: ricochet docs --help");
            }
            else
            {
                positional.Add(args[i]);
            }
        }

        if (positional.Count != 2)
            return CannotRun("ricochet docs needs a collection and a file. Try: ricochet docs --help");

        if (!WorkspaceFolder.LooksLikeWorkspace(workspace))
            return CannotRun($"\"{Path.GetFullPath(workspace)}\" is not a Ricochet workspace folder.");

        var folder = new WorkspaceFolder(Path.GetFullPath(workspace));
        var (loaded, issues) = folder.Load();
        if (issues.Count > 0)
        {
            foreach (var issue in issues)
                Console.Error.WriteLine($"{issue.Path}: {issue.Problem}");
            return CannotRun("Some workspace files could not be used. Fix them and try again.");
        }

        try
        {
            var collection = ExportFiles.FindCollection(loaded.Collections, positional[0]);
            var target = ExportFiles.Check(positional[1], folder.Root, force);
            ExportFiles.Write(target, CollectionDocumentation.Render(collection), folder.Root, force);
            Console.WriteLine($"Wrote documentation for {collection.Name} to {target}");
            return ExitPassed;
        }
        catch (ExportFileException ex)
        {
            return CannotRun(ex.Message);
        }
    }

    private const string ZipUsage = """
        Ricochet: export a workspace to one ZIP file, or import such a ZIP into a new folder (ADR 0035).

        Usage:
          ricochet zip export <workspace> <file.zip> [--force]
          ricochet zip import <file.zip> <new-folder>

        Export includes only the shared workspace files. Personal state, History, Secrets and .env are
        never included. The ZIP must be outside the workspace folder.

        Import writes into a new or empty folder only, and never over an existing workspace. Every entry
        is checked first, so a refused entry means nothing was written.

        Options:
          --force                Overwrite a ZIP file that already exists (export only).
          --help                 Show this help.

        Exit codes:
          0  The ZIP was written, or the workspace was imported.
          1  An entry was refused. Nothing was written.
          2  The command could not run, for example because a file is missing, the target folder is not
             empty, or a folder cannot be written.
        """;

    private static int ZipCommand(List<string> args)
    {
        if (args.Count == 0)
        {
            Console.WriteLine(ZipUsage);
            return ExitCannotRun;
        }

        if (args.Any(a => a is "--help" or "-h"))
        {
            Console.WriteLine(ZipUsage);
            return ExitPassed;
        }

        var verb = args[0];
        var rest = args.Skip(1).ToList();
        var force = rest.Remove("--force");

        if (rest.Any(a => a.StartsWith('-')))
            return CannotRun($"Unknown option \"{rest.First(a => a.StartsWith('-'))}\". Try: ricochet zip --help");

        if (verb == "export" && rest.Count == 2)
            return ZipExport(rest[0], rest[1], force);

        if (verb == "import" && rest.Count == 2 && !force)
            return ZipImport(rest[0], rest[1]);

        return CannotRun(verb is "export" or "import"
            ? $"ricochet zip {verb} needs {(verb == "export" ? "a workspace and a ZIP file" : "a ZIP file and a new folder")}. Try: ricochet zip --help"
            : $"Unknown command \"zip {verb}\". Try: ricochet zip --help");
    }

    private static int ZipExport(string workspace, string zipPath, bool force)
    {
        if (!WorkspaceFolder.LooksLikeWorkspace(workspace))
            return CannotRun($"\"{Path.GetFullPath(workspace)}\" is not a Ricochet workspace folder.");

        string target;
        try
        {
            target = ExportFiles.Check(zipPath, workspace, force);
        }
        catch (ExportFileException ex)
        {
            return CannotRun(ex.Message);
        }

        var parent = Path.GetDirectoryName(target);
        if (parent is null || !CanWriteTo(parent))
            return CannotRun($"The ZIP cannot be written: \"{parent}\" is not a writable folder.");

        // The Core export reads only the shared files and never loads Secrets, so none can end up in the ZIP.
        var result = WorkspaceArchive.Export(Path.GetFullPath(workspace), target);
        if (!result.Succeeded)
            return ZipRefused(result.Problems);

        Console.WriteLine($"Wrote {result.Files} workspace files to {target}");
        Console.WriteLine("Personal state, History, Secrets and .env were left out.");
        return ExitPassed;
    }

    private static int ZipImport(string zipPath, string folder)
    {
        if (!File.Exists(zipPath))
            return CannotRun($"There is no file at \"{Path.GetFullPath(zipPath)}\".");

        if (!ArchiveIsWorkspace(zipPath, out var archiveProblem))
            return CannotRun(archiveProblem);

        var target = Path.GetFullPath(folder);
        if (File.Exists(target))
            return CannotRun($"\"{target}\" is a file. Choose a new, empty folder.");

        if (Directory.Exists(target) && Directory.EnumerateFileSystemEntries(target).Any())
            return CannotRun($"\"{target}\" is not empty. Choose a new, empty folder.");

        // Probe the nearest folder that already exists, without creating anything, so a refused import leaves nothing behind.
        var existing = target;
        while (existing is not null && !Directory.Exists(existing) && !File.Exists(existing))
            existing = Path.GetDirectoryName(existing);
        if (existing is null || File.Exists(existing) || !CanWriteTo(existing))
            return CannotRun($"The workspace cannot be written to \"{target}\".");

        var result = WorkspaceArchive.Import(zipPath, target);
        if (!result.Succeeded)
            return ZipRefused(result.Problems);

        Console.WriteLine($"Imported {result.Files} workspace files into {target}");
        return ExitPassed;
    }

    private static bool ArchiveIsWorkspace(string zipPath, out string problem)
    {
        try
        {
            using var zip = ZipFile.OpenRead(zipPath);
            problem = "";
            if (zip.GetEntry("ricochet.json") is not null)
                return true;

            problem = $"\"{Path.GetFullPath(zipPath)}\" has no ricochet.json, so it is not a workspace.";
            return false;
        }
        catch (Exception ex) when (ex is InvalidDataException or IOException or UnauthorizedAccessException)
        {
            problem = $"\"{Path.GetFullPath(zipPath)}\" is not a readable ZIP file: {ex.Message}";
            return false;
        }
    }

    // Creates and deletes a probe file, so an unwritable folder is reported before anything is written.
    private static bool CanWriteTo(string folder)
    {
        try
        {
            var probe = Path.Combine(folder, $".ricochet-write-test-{Guid.NewGuid():N}");
            using (File.Create(probe))
            {
            }

            File.Delete(probe);
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }

    // Core reports each refused entry as "name: reason" and writes nothing when there is any, so exit 1 means nothing was written.
    private static int ZipRefused(IEnumerable<string> problems)
    {
        foreach (var problem in problems)
            Console.Error.WriteLine(problem);

        Console.Error.WriteLine("Nothing was written.");
        return ExitTestsFailed;
    }

    private const string EnvUsage = """
        Ricochet: move one environment in or out of a file, without its secret values (ADR 0036).

        Usage:
          ricochet env export <environment> <file.json> [--workspace <folder>] [--force]
          ricochet env import <file.json> [--workspace <folder>] [--replace]

        Arguments:
          environment            The name of the environment to export.
          file.json              The file to write (export) or read (import).

        Options:
          --workspace <folder>   The workspace folder. Defaults to the current folder.
          --force                Export: overwrite a file that already exists.
          --replace              Import: overwrite an environment with the same name. Without it, a name clash is refused.
          --help                 Show this help.

        Secret values are never exported, and import never writes one. Secret variables come back without
        a value, so set them again. A Secret this machine already has keeps its value when you replace.
        Export files are never written inside the workspace folder.

        Exit codes:
          0  The environment was exported or imported.
          1  The import was refused, because an environment with that name already exists and --replace was not given.
          2  The command could not run, for example because the environment does not exist, the file cannot be
             read or written, or the workspace is unreadable.
        """;

    private sealed class EnvArguments
    {
        public List<string> Positional { get; } = [];
        public string Workspace { get; set; } = ".";
        public bool Force { get; set; }
        public bool Replace { get; set; }
    }

    private static int EnvCommand(List<string> args)
    {
        if (args.Count == 0 || args[0] is "--help" or "-h")
        {
            Console.WriteLine(EnvUsage);
            return args.Count == 0 ? ExitCannotRun : ExitPassed;
        }

        if (!TryParseEnv(args.Skip(1).ToList(), out var options, out var parseError))
            return parseError is null ? ExitPassed : CannotRun(parseError);

        return args[0] switch
        {
            "export" => EnvExport(options!),
            "import" => EnvImport(options!),
            _ => CannotRun($"Unknown command \"env {args[0]}\". Try: ricochet env --help"),
        };
    }

    private static int EnvExport(EnvArguments options)
    {
        if (options.Positional.Count != 2)
            return CannotRun("Usage: ricochet env export <environment> <file.json>. Try: ricochet env --help");
        if (options.Replace)
            return CannotRun("--replace is for env import. Try: ricochet env --help");

        var name = options.Positional[0];
        var file = options.Positional[1];
        if (!WorkspaceFolder.LooksLikeWorkspace(options.Workspace))
            return CannotRun($"\"{Path.GetFullPath(options.Workspace)}\" is not a Ricochet workspace folder.");

        var folder = new WorkspaceFolder(Path.GetFullPath(options.Workspace));
        var (workspace, issues) = folder.Load();
        if (issues.Count > 0)
        {
            foreach (var issue in issues)
                Console.Error.WriteLine($"{issue.Path}: {issue.Problem}");
            return CannotRun("Some workspace files could not be used. Fix them and export again.");
        }

        // Merged only so the report can count the Secrets; the export itself never writes their values.
        workspace = WorkspaceSecrets.Merge(workspace, WorkspaceSecrets.ForWorkspace(LoadSecrets(), folder.Id));
        var environment = EnvironmentTransfer.FindEnvironment(workspace.Environments, name);
        if (environment is null)
        {
            var names = workspace.Environments.Select(e => e.Name).Where(n => n.Length > 0).ToList();
            var known = names.Count == 0 ? "The workspace has no environments." : "Environments: " + string.Join(", ", names) + ".";
            return CannotRun($"No environment is named \"{name}\", or more than one has that name. {known}");
        }

        try
        {
            ExportFiles.Write(file, EnvironmentTransfer.Export(environment), folder.Root, options.Force);
        }
        catch (ExportFileException ex)
        {
            return CannotRun(ex.Message);
        }

        var left = EnvironmentTransfer.SecretCount(environment);
        Console.WriteLine($"Exported environment \"{environment.Name}\" to {Path.GetFullPath(file)}");
        Console.WriteLine(left == 0
            ? "No Secret values were left out."
            : $"Left out {left} Secret {(left == 1 ? "value" : "values")}. Secret values are never exported; set them again on the machine that imports this file.");
        return ExitPassed;
    }

    private static int EnvImport(EnvArguments options)
    {
        if (options.Positional.Count != 1)
            return CannotRun("Usage: ricochet env import <file.json>. Try: ricochet env --help");
        if (options.Force)
            return CannotRun("--force is for env export. Use --replace to overwrite an environment on import.");

        var file = options.Positional[0];
        string json;
        try
        {
            json = File.ReadAllText(file);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return CannotRun($"Could not read \"{Path.GetFullPath(file)}\": {ex.Message}");
        }

        EnvironmentImportResult imported;
        try
        {
            imported = EnvironmentTransfer.Import(json, Path.GetFileName(file));
        }
        catch (EnvironmentImportException ex)
        {
            return CannotRun(ex.Message);
        }

        if (!WorkspaceFolder.LooksLikeWorkspace(options.Workspace))
            return CannotRun($"\"{Path.GetFullPath(options.Workspace)}\" is not a Ricochet workspace folder.");

        var folder = new WorkspaceFolder(Path.GetFullPath(options.Workspace));
        var (workspace, issues) = folder.Load();
        if (issues.Count > 0)
        {
            foreach (var issue in issues)
                Console.Error.WriteLine($"{issue.Path}: {issue.Problem}");
            return CannotRun("Some workspace files could not be used. Fix them and import again.");
        }

        var incoming = imported.Environment;
        var clashed = workspace.Environments.Any(e => e.Name.Equals(incoming.Name, StringComparison.OrdinalIgnoreCase));
        var placement = EnvironmentTransfer.Place(workspace.Environments, incoming, options.Replace ? EnvironmentClash.Replace : EnvironmentClash.Skip);
        if (placement.Placed is null)
        {
            Console.Error.WriteLine($"Environment \"{incoming.Name}\" already exists in this workspace, so nothing was imported. Pass --replace to overwrite it.");
            return ExitRefused;
        }

        // Split moves every Secret value out of the Workspace, and its values are dropped here rather than stored.
        // The vault is never written, so this machine's Secret values stay as they were.
        var (clean, _) = WorkspaceSecrets.Split(workspace with { Environments = placement.Environments });
        try
        {
            folder.Save(clean);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return CannotRun($"Could not save the workspace: {ex.Message}");
        }

        Console.WriteLine(clashed ? $"Replaced environment \"{incoming.Name}\" in {folder.Root}" : $"Imported environment \"{incoming.Name}\" into {folder.Root}");
        Console.WriteLine(imported.Summary);
        foreach (var issue in imported.Issues)
            Console.WriteLine($"Note: {issue.Category}: {issue.Detail}");

        var secrets = EnvironmentTransfer.SecretCount(incoming);
        if (secrets > 0)
            Console.WriteLine($"Note: Secret values are never imported ({secrets} Secret {(secrets == 1 ? "variable" : "variables")}). A value this machine already has is kept; set any others in the app.");

        return ExitPassed;
    }

    /// <returns>False to stop. A null error means help was shown, which is not a failure.</returns>
    private static bool TryParseEnv(List<string> args, out EnvArguments? result, out string? error)
    {
        var options = new EnvArguments();
        result = options;
        error = null;

        for (var i = 0; i < args.Count; i++)
        {
            var arg = args[i];
            switch (arg)
            {
                case "--help" or "-h":
                    Console.WriteLine(EnvUsage);
                    return false;
                case "--force":
                    options.Force = true;
                    break;
                case "--replace":
                    options.Replace = true;
                    break;
                case "--workspace":
                    i++;
                    if (i >= args.Count || string.IsNullOrWhiteSpace(args[i])) { error = "--workspace needs a folder."; return false; }
                    options.Workspace = args[i];
                    break;
                default:
                    if (arg.StartsWith('-'))
                    {
                        error = $"Unknown option \"{arg}\". Try: ricochet env --help";
                        return false;
                    }

                    options.Positional.Add(arg);
                    break;
            }
        }

        return true;
    }


    private sealed class Colors(bool enabled)
    {
        public string Verdict(bool passed)
        {
            var word = passed ? "PASS" : "FAIL";
            if (!enabled)
                return word;
            return passed ? $"\u001b[32m{word}\u001b[0m" : $"\u001b[31m{word}\u001b[0m";
        }
    }
}
