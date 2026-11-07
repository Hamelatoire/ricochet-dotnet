using System.Diagnostics;

namespace Ricochet.Core;

/// <summary>What a clone produced: whether git succeeded, whether the result is a Ricochet workspace, and a message to show.</summary>
public sealed record GitCloneResult(bool Succeeded, bool IsWorkspace, string Message);

/// <summary>
/// Clones a git repository into an empty folder the user chose, with the git already installed (ADR 0034). It runs only
/// the clone. It never commits, pushes or stores credentials, and it never prompts for a password. Authentication uses
/// the user's existing git setup, so a URL that carries a password is refused.
/// </summary>
public static class GitCloner
{
    // Only transports a person expects. Without this list git's ext:: transport could run an arbitrary program.
    private const string AllowedProtocols = "file:http:https:ssh:git";

    /// <summary>Null when the folder can take a clone, otherwise the reason it can't. It must exist and be empty.</summary>
    public static string? CheckTargetFolder(string folder)
    {
        if (string.IsNullOrWhiteSpace(folder))
            return "Choose a folder for the clone.";

        var full = Path.GetFullPath(folder);
        if (File.Exists(full))
            return "That path is a file. Choose an empty folder.";
        if (!Directory.Exists(full))
            return "That folder does not exist. Create it, then choose it.";
        if (Directory.EnumerateFileSystemEntries(full).Any())
            return "That folder is not empty. Choose an empty folder so the clone doesn't mix with existing files.";

        return null;
    }

    /// <summary>Null when the URL can be cloned, otherwise the reason it can't.</summary>
    public static string? CheckUrl(string url)
    {
        var trimmed = url?.Trim() ?? "";
        if (trimmed.Length == 0)
            return "Enter the address of a git repository.";
        if (trimmed.StartsWith('-'))
            return "The address can't start with a dash.";

        if (Uri.TryCreate(trimmed, UriKind.Absolute, out var uri) && uri.Scheme.Length > 1)
        {
            if (uri.UserInfo.Contains(':'))
                return "The address contains a password. Keep passwords in your git setup, not in the address.";

            var scheme = uri.Scheme.ToLowerInvariant();
            if (!AllowedProtocols.Split(':').Contains(scheme))
                return $"Ricochet clones over {AllowedProtocols.Replace(":", ", ")} only.";
        }

        return null;
    }

    /// <summary>
    /// Clones <paramref name="url"/> into <paramref name="folder"/> and reports whether the result is a Ricochet workspace.
    /// A result that is not a workspace stays on disk, untouched by Ricochet, so the user can look at it.
    /// </summary>
    public static async Task<GitCloneResult> CloneAsync(string url, string folder, CancellationToken cancellationToken = default)
    {
        if (CheckUrl(url) is { } urlProblem)
            return new GitCloneResult(false, false, urlProblem);
        if (CheckTargetFolder(folder) is { } folderProblem)
            return new GitCloneResult(false, false, folderProblem);

        var git = GitStatusReader.FindGit(folder);
        if (git is null)
            return new GitCloneResult(false, false, "git was not found on PATH. Install git to clone a repository.");

        var target = Path.GetFullPath(folder);
        var (exitCode, stderr) = await RunCloneAsync(git, target, url.Trim(), cancellationToken);
        if (exitCode != 0)
        {
            var lastLine = stderr.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).LastOrDefault();
            return new GitCloneResult(false, false, lastLine is null ? "git clone failed." : $"git clone failed: {lastLine}");
        }

        if (WorkspaceFolder.LooksLikeWorkspace(target))
            return new GitCloneResult(true, true, "Cloned. The folder is a Ricochet workspace.");

        return new GitCloneResult(true, false,
            "Cloned, but the folder is not a Ricochet workspace: it has no ricochet.json and no collections folder.");
    }

    private static async Task<(int ExitCode, string Stderr)> RunCloneAsync(string git, string target, string url, CancellationToken cancellationToken)
    {
        var start = new ProcessStartInfo(git)
        {
            WorkingDirectory = target,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
        };
        // "--" stops a URL that looks like an option from being read as one.
        foreach (var argument in new[] { "clone", "--quiet", "--", url, target })
            start.ArgumentList.Add(argument);

        // Never prompt in a terminal, and allow only the transports in AllowedProtocols.
        start.Environment["GIT_TERMINAL_PROMPT"] = "0";
        start.Environment["GIT_ALLOW_PROTOCOL"] = AllowedProtocols;

        using var process = Process.Start(start);
        if (process is null)
            return (-1, "git could not be started.");

        try
        {
            var stdoutTask = process.StandardOutput.ReadToEndAsync(cancellationToken);
            var stderrTask = process.StandardError.ReadToEndAsync(cancellationToken);
            await process.WaitForExitAsync(cancellationToken);
            await stdoutTask;
            return (process.ExitCode, await stderrTask);
        }
        catch (OperationCanceledException)
        {
            try { process.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            return (-1, "The clone was cancelled.");
        }
    }
}
