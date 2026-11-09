using System.Runtime.InteropServices;
using System.Text;

namespace Ricochet.Core;

/// <summary>
/// Writes a local crash log to the personal folder (ADR 0088). Nothing is sent anywhere: the log stays on this machine,
/// and the person decides whether to attach it to a bug report.
/// The log holds the exception types, the stack traces and the app and OS details. It never holds request data, and
/// exception messages are left out, because they can carry URLs, Variable values or Secrets.
/// </summary>
public static class CrashLog
{
    /// <summary>How many crash logs are kept. Older ones are deleted when a new one is written.</summary>
    public const int MaxLogs = 20;

    private const string FolderName = "crashes";
    private const string FilePrefix = "crash-";
    private const string FileSuffix = ".log";

    /// <summary>The folder crash logs are written to, inside the personal directory.</summary>
    public static string DirectoryFor(string personalDirectory) => Path.Combine(personalDirectory, FolderName);

    /// <summary>
    /// Writes one crash log and removes the oldest ones beyond <see cref="MaxLogs"/>. Returns the path written, or null
    /// if writing failed. It never throws, because it runs while the app is already crashing.
    /// </summary>
    public static string? Write(string personalDirectory, Exception exception, string appVersion, DateTimeOffset? now = null)
    {
        try
        {
            var directory = DirectoryFor(personalDirectory);
            Directory.CreateDirectory(directory);

            var stamp = (now ?? DateTimeOffset.UtcNow).UtcDateTime;
            var path = Path.Combine(directory, $"{FilePrefix}{stamp:yyyyMMdd-HHmmss-fff}{FileSuffix}");

            File.WriteAllText(path, Format(exception, appVersion, stamp, RuntimeInformation.OSDescription, Environment.Is64BitOperatingSystem));
            Prune(directory);
            return path;
        }
        catch
        {
            return null;
        }
    }

    /// <summary>Builds the text of a crash log. Paths under the user's profile are shortened to "~" so the user name isn't written out.</summary>
    public static string Format(Exception exception, string appVersion, DateTime utc, string osDescription, bool is64Bit)
    {
        var builder = new StringBuilder();
        builder.AppendLine("Ricochet crash log");
        builder.AppendLine("Written only on this machine. Nothing is sent anywhere.");
        builder.AppendLine("Exception messages and request data are left out. Stack traces may include file paths.");
        builder.AppendLine();
        builder.AppendLine($"Time (UTC): {utc:yyyy-MM-dd HH:mm:ss}");
        builder.AppendLine($"App version: {appVersion}");
        builder.AppendLine($"Operating system: {osDescription} ({(is64Bit ? "64-bit" : "32-bit")})");
        builder.AppendLine($"Runtime: {Environment.Version}");
        builder.AppendLine();

        var current = exception;
        var depth = 0;
        while (current is not null)
        {
            builder.AppendLine(depth == 0 ? "Exception:" : "Inner exception:");
            builder.AppendLine($"  {current.GetType().FullName}");
            if (current.StackTrace is { Length: > 0 } trace)
                builder.AppendLine(Indent(RedactProfile(trace)));
            current = current.InnerException;
            depth++;
        }

        return builder.ToString();
    }

    /// <summary>Replaces the user's profile folder with "~" in a path or stack trace.</summary>
    public static string RedactProfile(string text)
    {
        var profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (string.IsNullOrEmpty(profile))
            return text;

        return text.Replace(profile, "~", StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>Deletes the oldest crash logs beyond <see cref="MaxLogs"/>. Only files this class wrote are touched.</summary>
    public static void Prune(string directory)
    {
        var logs = Directory.GetFiles(directory, FilePrefix + "*" + FileSuffix)
            .OrderBy(Path.GetFileName, StringComparer.Ordinal)
            .ToList();

        foreach (var old in logs.Take(Math.Max(0, logs.Count - MaxLogs)))
        {
            try
            {
                File.Delete(old);
            }
            catch (IOException)
            {
                // A log that is in use stays. The next write tries again.
            }
        }
    }

    private static string Indent(string text) =>
        string.Join(Environment.NewLine, text.Split(["\r\n", "\n"], StringSplitOptions.None).Select(line => "  " + line));
}
