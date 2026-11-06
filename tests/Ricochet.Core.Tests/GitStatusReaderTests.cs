using System.Diagnostics;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>Runs the real git against temporary repositories (ADR 0011).</summary>
public sealed class GitStatusReaderTests : IDisposable
{
    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-git").FullName;

    public void Dispose()
    {
        // Git marks its object files read-only, which stops Windows from deleting them.
        foreach (var entry in Directory.EnumerateFileSystemEntries(_root, "*", SearchOption.AllDirectories))
            File.SetAttributes(entry, FileAttributes.Normal);
        Directory.Delete(_root, recursive: true);
    }

    private static void Git(string folder, params string[] arguments)
    {
        var start = new ProcessStartInfo("git") { WorkingDirectory = folder, UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true };
        foreach (var argument in arguments)
            start.ArgumentList.Add(argument);
        start.Environment["GIT_AUTHOR_NAME"] = "Test";
        start.Environment["GIT_AUTHOR_EMAIL"] = "test@example.test";
        start.Environment["GIT_COMMITTER_NAME"] = "Test";
        start.Environment["GIT_COMMITTER_EMAIL"] = "test@example.test";

        using var process = Process.Start(start)!;
        process.StandardOutput.ReadToEnd();
        process.StandardError.ReadToEnd();
        process.WaitForExit();
        Assert.Equal(0, process.ExitCode);
    }

    private static void Commit(string folder, string file)
    {
        File.WriteAllText(Path.Combine(folder, file), file);
        Git(folder, "add", file);
        Git(folder, "commit", "-q", "-m", file);
    }

    [Fact]
    public async Task A_folder_outside_any_repository_shows_nothing()
    {
        var plain = Path.Combine(_root, "plain");
        Directory.CreateDirectory(plain);

        Assert.Null(await GitStatusReader.ReadAsync(plain, TestContext.Current.CancellationToken));
    }

    [Fact]
    public async Task A_branch_without_an_upstream_shows_its_name_and_no_counts()
    {
        var repo = Path.Combine(_root, "repo");
        Directory.CreateDirectory(repo);
        Git(repo, "init", "-q", "-b", "feature");
        Commit(repo, "a.txt");

        var status = await GitStatusReader.ReadAsync(repo, TestContext.Current.CancellationToken);

        Assert.Equal(new GitStatus("feature", null, null), status);
    }

    [Fact]
    public async Task Counts_show_commits_ahead_of_and_behind_the_upstream()
    {
        var remote = Path.Combine(_root, "remote.git");
        var work = Path.Combine(_root, "work");
        var other = Path.Combine(_root, "other");
        Directory.CreateDirectory(remote);
        Git(remote, "init", "-q", "--bare", "-b", "main");

        Git(_root, "clone", "-q", remote, work);
        Commit(work, "a.txt");
        Git(work, "push", "-q", "-u", "origin", "HEAD");

        // Someone else pushes one commit, and this copy makes one of its own.
        Git(_root, "clone", "-q", remote, other);
        Commit(other, "b.txt");
        Git(other, "push", "-q", "origin", "HEAD");
        Commit(work, "c.txt");
        Git(work, "fetch", "-q");

        var status = await GitStatusReader.ReadAsync(work, TestContext.Current.CancellationToken);

        Assert.Equal(new GitStatus("main", Ahead: 1, Behind: 1), status);
    }

    [Fact]
    public async Task A_detached_head_shows_the_short_commit_hash()
    {
        var repo = Path.Combine(_root, "repo");
        Directory.CreateDirectory(repo);
        Git(repo, "init", "-q", "-b", "main");
        Commit(repo, "a.txt");
        Git(repo, "checkout", "-q", "--detach");

        var status = await GitStatusReader.ReadAsync(repo, TestContext.Current.CancellationToken);

        Assert.NotNull(status);
        Assert.Matches("^[0-9a-f]{7,}$", status.Branch);
        Assert.Null(status.Ahead);
    }
}
