using System.Collections;
using Ricochet.Core;

namespace Ricochet.Cli;

/// <summary>Runs a Workspace's requests and assertions, for CI. It only reads the Workspace.</summary>
internal static class Program
{
    private const int ExitPassed = 0;
    private const int ExitTestsFailed = 1;
    private const int ExitCannotRun = 2;
    private const int ExitCancelled = 130;
    private const string VariablePrefix = "RICOCHET_VAR_";

    private const string Usage = """
        Ricochet: run a workspace's requests and check their assertions.

        Usage:
          ricochet run [folder] [options]

        Arguments:
          folder                 The workspace folder. Defaults to the current folder.

        Options:
          --collection <name>    Run only this collection.
          --folder <name>        Run only requests inside folders with this name.
          --tag <name>           Run only requests with this tag, given by their folders. Repeatable; any match counts.
          --exclude-tag <name>   Skip requests with this tag. Repeatable; wins over --tag.
          --env <name>           Use this environment.
          --var <name=value>     Set a variable, overriding every scope. Repeatable.
          --xunit <file>         Also write an xUnit XML report.
          --timeout <seconds>    Time limit for each request. Default: 120.
          --bail                 Stop at the first failure.
          --insecure             Skip TLS certificate checks. Only for servers you trust, such as a local test server.
          --help                 Show this help.

        Variables can also come from a .env file in the workspace folder, or from environment
        variables named RICOCHET_VAR_<name>. Environment variables win over .env, and --var wins over both.
        Prefer those for secrets in CI, because --var values can show up in process lists.
        Values set either way are masked in the output.

        Exit codes:
          0  Everything passed.
          1  A request or assertion failed.
          2  The run could not happen, for example because a workspace file is unreadable.
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
        public int TimeoutSeconds { get; set; } = 120;
        public bool Bail { get; set; }
        public bool Insecure { get; set; }
        public List<KeyValue> Variables { get; } = [];
    }

    private static async Task<int> Main(string[] args)
    {
        if (args.Length == 0 || args[0] is "--help" or "-h" or "help")
        {
            Console.WriteLine(Usage);
            return args.Length == 0 ? ExitCannotRun : ExitPassed;
        }

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

        var runner = new CollectionRunner(new RequestSender(
            RequestSender.CreateDefaultClient(verifyCertificates: !options.Insecure),
            TimeSpan.FromSeconds(options.TimeoutSeconds)));
        var report = await runner.RunAsync(
            workspace,
            new RunOptions
            {
                CollectionName = options.Collection,
                FolderName = options.FolderName,
                IncludeTags = FolderTags.Normalize(options.Tags) ?? [],
                ExcludeTags = FolderTags.Normalize(options.ExcludeTags) ?? [],
                EnvironmentName = options.Environment,
                Overrides = [.. dotEnv.Variables, .. VariablesFromEnvironment(), .. options.Variables],
                StopOnFirstFailure = options.Bail,
            },
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

    private static int CannotRun(string message)
    {
        Console.Error.WriteLine(message);
        return ExitCannotRun;
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
