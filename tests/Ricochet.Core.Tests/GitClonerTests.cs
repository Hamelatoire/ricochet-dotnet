using System.Diagnostics;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>Clones from local bare repositories with the real git. Nothing touches the network (ADR 0034).</summary>
public sealed class GitClonerTests : IDisposable
{
    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-clone").FullName;

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

    /// <summary>A bare repository with one commit that holds the given files.</summary>
    private string MakeRemote(string name, params string[] files)
    {
        var remote = Path.Combine(_root, name);
        var work = Path.Combine(_root, name + "-work");
        Directory.CreateDirectory(remote);
        Git(remote, "init", "-q", "--bare", "-b", "main");

        Directory.CreateDirectory(work);
        Git(work, "init", "-q", "-b", "main");
        foreach (var file in files)
        {
            var path = Path.Combine(work, file);
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, "{}");
        }
        Git(work, "add", "-A");
        Git(work, "commit", "-q", "-m", "initial");
        Git(work, "push", "-q", remote, "main");
        return remote;
    }

    private string EmptyFolder(string name)
    {
        var folder = Path.Combine(_root, name);
        Directory.CreateDirectory(folder);
        return folder;
    }

    [Fact]
    public async Task Cloning_a_workspace_repository_reports_a_workspace()
    {
        var remote = MakeRemote("workspace.git", "ricochet.json", "collections/a/collection.json");
        var target = EmptyFolder("target");

        var result = await GitClonerTests.Clone(remote, target);

        Assert.True(result.Succeeded, result.Message);
        Assert.True(result.IsWorkspace, result.Message);
        Assert.True(File.Exists(Path.Combine(target, "ricochet.json")));
        Assert.True(WorkspaceFolder.LooksLikeWorkspace(target));
    }

    [Fact]
    public async Task Cloning_a_repository_that_is_not_a_workspace_says_so_and_keeps_the_files()
    {
        var remote = MakeRemote("notes.git", "README.md");
        var target = EmptyFolder("target");

        var result = await GitClonerTests.Clone(remote, target);

        Assert.True(result.Succeeded, result.Message);
        Assert.False(result.IsWorkspace);
        Assert.Contains("not a Ricochet workspace", result.Message);
        Assert.True(File.Exists(Path.Combine(target, "README.md")));
    }

    [Fact]
    public async Task A_missing_repository_fails_and_reports_git_s_message()
    {
        var target = EmptyFolder("target");

        var result = await GitClonerTests.Clone(Path.Combine(_root, "does-not-exist.git"), target);

        Assert.False(result.Succeeded);
        Assert.False(result.IsWorkspace);
        Assert.StartsWith("git clone failed", result.Message);
    }

    [Fact]
    public async Task A_non_empty_folder_is_refused_and_left_alone()
    {
        var remote = MakeRemote("workspace2.git", "ricochet.json");
        var target = EmptyFolder("target");
        var existing = Path.Combine(target, "mine.txt");
        File.WriteAllText(existing, "keep me");

        var result = await GitClonerTests.Clone(remote, target);

        Assert.False(result.Succeeded);
        Assert.Contains("not empty", result.Message);
        Assert.Equal("keep me", File.ReadAllText(existing));
        Assert.False(File.Exists(Path.Combine(target, "ricochet.json")));
    }

    [Fact]
    public void A_target_folder_must_exist_and_be_empty()
    {
        var empty = EmptyFolder("empty");
        var missing = Path.Combine(_root, "missing");
        var file = Path.Combine(_root, "afile.txt");
        File.WriteAllText(file, "x");
        var full = EmptyFolder("full");
        File.WriteAllText(Path.Combine(full, "a.txt"), "x");

        Assert.Null(GitCloner.CheckTargetFolder(empty));
        Assert.Contains("does not exist", GitCloner.CheckTargetFolder(missing));
        Assert.Contains("is a file", GitCloner.CheckTargetFolder(file));
        Assert.Contains("not empty", GitCloner.CheckTargetFolder(full));
        Assert.NotNull(GitCloner.CheckTargetFolder(""));
        Assert.NotNull(GitCloner.CheckTargetFolder("   "));
    }

    [Fact]
    public void Blank_urls_and_urls_that_look_like_options_are_refused()
    {
        Assert.NotNull(GitCloner.CheckUrl(""));
        Assert.NotNull(GitCloner.CheckUrl("   "));
        Assert.NotNull(GitCloner.CheckUrl("--upload-pack=touch x"));
    }

    [Fact]
    public void Urls_with_a_password_are_refused()
    {
        var problem = GitCloner.CheckUrl("https://alice:hunter2@example.test/team/api.git");

        Assert.NotNull(problem);
        Assert.Contains("password", problem);
        Assert.DoesNotContain("hunter2", problem);
    }

    [Fact]
    public void Transports_that_run_programs_are_refused()
    {
        Assert.NotNull(GitCloner.CheckUrl("ext::sh -c echo"));
        Assert.NotNull(GitCloner.CheckUrl("ftp://example.test/repo.git"));
    }

    [Theory]
    [InlineData("https://example.test/team/api.git")]
    [InlineData("ssh://git@example.test/team/api.git")]
    [InlineData("git@example.test:team/api.git")]
    [InlineData("C:\\repos\\api.git")]
    public void Ordinary_repository_addresses_are_accepted(string url) =>
        Assert.Null(GitCloner.CheckUrl(url));

    [Fact]
    public async Task Refused_urls_never_reach_git()
    {
        var target = EmptyFolder("target");

        var result = await GitClonerTests.Clone("https://alice:hunter2@example.test/api.git", target);

        Assert.False(result.Succeeded);
        Assert.Empty(Directory.EnumerateFileSystemEntries(target));
    }

    private static Task<GitCloneResult> Clone(string url, string folder) =>
        GitCloner.CloneAsync(url, folder, TestContext.Current.CancellationToken);
}
