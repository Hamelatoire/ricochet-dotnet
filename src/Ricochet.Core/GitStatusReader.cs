using System.Diagnostics;
using System.Globalization;

namespace Ricochet.Core;

/// <summary>
/// The branch of the Workspace's git repository, and how far it is from its upstream (ADR 0011).
/// Counts compare with what git last fetched, so they can be out of date.
/// </summary>
/// <param name="Branch">The branch name, or the short commit hash when HEAD is detached.</param>
/// <param name="Ahead">Commits on this branch that the upstream doesn't have. Null when there is no upstream.</param>
/// <param name="Behind">Commits on the upstream that this branch doesn't have. Null when there is no upstream.</param>
public sealed record GitStatus(string Branch, int? Ahead, int? Behind);

/// <summary>
/// Reads the branch with the git already installed, and never changes anything (ADR 0011). It runs only commands
/// that read refs and commit counts, so a repository's own settings can't run a program. It never fetches.
/// </summary>
public static class GitStatusReader
{
    private static readonly TimeSpan Limit = TimeSpan.FromSeconds(3);

    /// <summary>Returns null when git is not installed, the folder is not a repository, or git doesn't answer in time.</summary>
    public static async Task<GitStatus?> ReadAsync(string folder, CancellationToken cancellationToken = default)
    {
        var git = FindGit(folder);
        if (git is null)
            return null;

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(Limit);

        var branch = await RunAsync(git, folder, timeout.Token, "rev-parse", "--abbrev-ref", "HEAD");
        if (branch is null)
            return null;

        if (branch == "HEAD")
        {
            var hash = await RunAsync(git, folder, timeout.Token, "rev-parse", "--short", "HEAD");
            return hash is null ? null : new GitStatus(hash, null, null);
        }

        // "behind<TAB>ahead": the left side is the upstream's commits that this branch lacks.
        var counts = await RunAsync(git, folder, timeout.Token, "rev-list", "--left-right", "--count", "@{upstream}...HEAD");
        var parts = counts?.Split('\t', StringSplitOptions.RemoveEmptyEntries);
        if (parts is not { Length: 2 }
            || !int.TryParse(parts[0], NumberStyles.None, CultureInfo.InvariantCulture, out var behind)
            || !int.TryParse(parts[1], NumberStyles.None, CultureInfo.InvariantCulture, out var ahead))
            return new GitStatus(branch, null, null);

        return new GitStatus(branch, ahead, behind);
    }

    /// <summary>
    /// Finds git on PATH, and never in the Workspace folder, so a git program shipped inside a shared folder is never run.
    /// </summary>
    internal static string? FindGit(string folder)
    {
        var names = OperatingSystem.IsWindows() ? new[] { "git.exe" } : new[] { "git" };
        var workspace = Path.GetFullPath(folder).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        var path = Environment.GetEnvironmentVariable("PATH") ?? "";

        foreach (var directory in path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
        {
            if (!Path.IsPathFullyQualified(directory))
                continue;

            var full = Path.GetFullPath(directory).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (full.StartsWith(workspace, StringComparison.OrdinalIgnoreCase))
                continue;

            foreach (var name in names)
            {
                var candidate = Path.Combine(full, name);
                if (File.Exists(candidate))
                    return candidate;
            }
        }

        return null;
    }

    private static async Task<string?> RunAsync(string git, string folder, CancellationToken cancellationToken, params string[] arguments)
    {
        var start = new ProcessStartInfo(git)
        {
            WorkingDirectory = folder,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
        };
        foreach (var argument in arguments)
            start.ArgumentList.Add(argument);

        // Never prompt for a password, and never take a lock that could get in the user's way.
        start.Environment["GIT_TERMINAL_PROMPT"] = "0";
        start.Environment["GIT_OPTIONAL_LOCKS"] = "0";

        using var process = Process.Start(start);
        if (process is null)
            return null;

        try
        {
            var output = await process.StandardOutput.ReadToEndAsync(cancellationToken);
            await process.WaitForExitAsync(cancellationToken);
            return process.ExitCode == 0 ? output.Trim() : null;
        }
        catch (OperationCanceledException)
        {
            try { process.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            return null;
        }
    }
}
