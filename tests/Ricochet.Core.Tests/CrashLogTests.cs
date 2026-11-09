using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class CrashLogTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-crash").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    private static Exception Thrown(string message)
    {
        try
        {
            throw new InvalidOperationException(message);
        }
        catch (Exception ex)
        {
            return ex;
        }
    }

    [Fact]
    public void Write_creates_a_log_under_the_personal_folder()
    {
        var path = CrashLog.Write(_dir, Thrown("boom"), "1.2.3");

        Assert.NotNull(path);
        Assert.StartsWith(CrashLog.DirectoryFor(_dir), path);
        Assert.True(File.Exists(path));
    }

    [Fact]
    public void Log_holds_type_app_version_and_OS_but_not_the_exception_message()
    {
        var secret = "https://internal.example/api?token=abc123";
        var path = CrashLog.Write(_dir, Thrown(secret), "9.8.7");

        var text = File.ReadAllText(path!);

        Assert.Contains("System.InvalidOperationException", text);
        Assert.Contains("App version: 9.8.7", text);
        Assert.Contains("Operating system:", text);
        Assert.DoesNotContain(secret, text);
        Assert.DoesNotContain("abc123", text);
    }

    [Fact]
    public void Log_states_that_nothing_is_sent()
    {
        var text = CrashLog.Format(Thrown("x"), "1.0.0", DateTime.UtcNow, "Test OS", true);

        Assert.Contains("Nothing is sent anywhere.", text);
    }

    [Fact]
    public void Inner_exceptions_are_written_with_their_types_but_not_their_messages()
    {
        var inner = Thrown("inner secret value");
        var outer = new ApplicationException("outer secret value", inner);

        var text = CrashLog.Format(outer, "1.0.0", DateTime.UtcNow, "Test OS", true);

        Assert.Contains("Inner exception:", text);
        Assert.Contains("System.InvalidOperationException", text);
        Assert.DoesNotContain("secret value", text);
    }

    [Fact]
    public void Profile_folder_is_shortened_so_the_user_name_is_not_written()
    {
        var profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var trace = $"   at Foo() in {Path.Combine(profile, "src", "Foo.cs")}:line 3";

        var redacted = CrashLog.RedactProfile(trace);

        Assert.DoesNotContain(profile, redacted);
        Assert.Contains("~", redacted);
    }

    [Fact]
    public void Old_logs_are_pruned_past_the_limit_and_newer_ones_are_kept()
    {
        var start = new DateTimeOffset(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);
        string? newest = null;
        for (var i = 0; i < CrashLog.MaxLogs + 5; i++)
            newest = CrashLog.Write(_dir, Thrown($"crash {i}"), "1.0.0", start.AddMinutes(i));

        var kept = Directory.GetFiles(CrashLog.DirectoryFor(_dir), "crash-*.log");

        Assert.Equal(CrashLog.MaxLogs, kept.Length);
        Assert.Contains(newest!, kept);
        Assert.DoesNotContain(kept, file => Path.GetFileName(file).StartsWith("crash-20260101-000000"));
    }

    [Fact]
    public void Prune_leaves_files_it_did_not_write_alone()
    {
        var directory = CrashLog.DirectoryFor(_dir);
        Directory.CreateDirectory(directory);
        var notes = Path.Combine(directory, "notes.txt");
        File.WriteAllText(notes, "mine");

        for (var i = 0; i < CrashLog.MaxLogs + 2; i++)
            CrashLog.Write(_dir, Thrown("x"), "1.0.0", new DateTimeOffset(2026, 1, 1, 0, 0, i, TimeSpan.Zero));

        Assert.True(File.Exists(notes));
    }

    [Fact]
    public void Write_returns_null_instead_of_throwing_when_the_folder_cannot_be_written()
    {
        var blocker = Path.Combine(_dir, "not-a-folder");
        File.WriteAllText(blocker, "file in the way");

        var path = CrashLog.Write(blocker, Thrown("x"), "1.0.0");

        Assert.Null(path);
    }
}
