using System.IO.Compression;
using System.Text;
using System.Text.Json;

namespace Ricochet.Core;

/// <summary>What an export or import did. Any problem means nothing was written.</summary>
public sealed record WorkspaceArchiveResult(int Files, IReadOnlyList<string> Problems)
{
    public bool Succeeded => Problems.Count == 0;
}

/// <summary>
/// Exports the shareable files of a workspace folder as one ZIP, and imports such a ZIP into a new, empty folder.
/// Only files in the shareable layout (see <see cref="WorkspaceFolder"/>) are ever included, so personal files,
/// History and Secrets stay out. See docs/adr/0035-export-and-import-a-whole-workspace-as-a-zip.md.
/// </summary>
public static class WorkspaceArchive
{
    private const string MarkerName = "ricochet.json";
    private const string GlobalsName = "globals.json";
    private const string CollectionName = "collection.json";
    private const string FolderName = "folder.json";

    // Personal files that could end up in a shared folder by mistake. They are never exported or imported.
    private static readonly HashSet<string> PersonalFileNames = new(StringComparer.OrdinalIgnoreCase) { "state.json", "history.json" };

    private enum Kind { Marker, Globals, Collection, Folder, Request, Environment }

    public static WorkspaceArchiveResult Export(string workspaceRoot, string zipPath)
    {
        if (!File.Exists(Path.Combine(workspaceRoot, MarkerName)))
            return Failed($"This folder has no {MarkerName}, so it is not a workspace.");

        var problems = new List<string>();
        var files = new List<(string Name, byte[] Bytes)>();
        foreach (var path in SharedCandidates(workspaceRoot))
        {
            var name = Path.GetRelativePath(workspaceRoot, path).Replace('\\', '/');
            var kind = KindOf(name);
            if (kind is null)
                continue;

            try
            {
                var bytes = File.ReadAllBytes(path);
                var problem = Check(kind.Value, bytes);
                if (problem is null)
                    files.Add((name, bytes));
                else
                    problems.Add($"{name}: {problem}");
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                problems.Add($"{name}: {ex.Message}");
            }
        }

        // A file that could not be read would make an archive that import refuses, so nothing is written.
        if (problems.Count > 0)
            return new WorkspaceArchiveResult(0, problems);

        var temp = zipPath + ".tmp";
        try
        {
            using (var stream = File.Create(temp))
            using (var zip = new ZipArchive(stream, ZipArchiveMode.Create))
            {
                foreach (var (name, bytes) in files.OrderBy(f => f.Name, StringComparer.Ordinal))
                {
                    using var entry = zip.CreateEntry(name, CompressionLevel.Optimal).Open();
                    entry.Write(bytes);
                }
            }

            File.Move(temp, zipPath, overwrite: true);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            TryDelete(temp);
            return Failed($"The archive could not be written: {ex.Message}");
        }

        return new WorkspaceArchiveResult(files.Count, []);
    }

    /// <summary>
    /// Creates the folder if needed and writes the archive's workspace files into it. The folder must be empty,
    /// so an existing workspace is never overwritten. Every entry is checked before anything is written.
    /// </summary>
    public static WorkspaceArchiveResult Import(string zipPath, string targetFolder)
    {
        var target = Path.GetFullPath(targetFolder);
        if (File.Exists(target))
            return Failed("A file already has this name, so the archive was not imported. Choose a new, empty folder.");
        if (Directory.Exists(target) && Directory.EnumerateFileSystemEntries(target).Any())
            return Failed("The folder is not empty, so the archive was not imported. Choose a new, empty folder.");

        var problems = new List<string>();
        var files = new List<(string Name, byte[] Bytes)>();
        try
        {
            using var stream = File.OpenRead(zipPath);
            using var zip = new ZipArchive(stream, ZipArchiveMode.Read);
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var entry in zip.Entries)
            {
                var name = entry.FullName;
                if (name.EndsWith('/'))
                {
                    // A folder entry. It creates nothing by itself, but still must not point outside the archive.
                    if (!IsSafeName(name.TrimEnd('/')))
                        problems.Add($"{name}: The path is not allowed.");
                    continue;
                }

                if (!IsSafeName(name))
                {
                    problems.Add($"{name}: The path is not allowed. Entries must be relative, use forward slashes, and stay inside the archive.");
                    continue;
                }

                var kind = KindOf(name);
                if (kind is null)
                {
                    problems.Add($"{name}: This is not a workspace file, so it was refused.");
                    continue;
                }

                if (!seen.Add(name))
                {
                    problems.Add($"{name}: The archive holds this file more than once.");
                    continue;
                }

                using var content = new MemoryStream();
                using (var entryStream = entry.Open())
                    entryStream.CopyTo(content);

                var bytes = content.ToArray();
                var problem = Check(kind.Value, bytes);
                if (problem is null)
                    files.Add((name, bytes));
                else
                    problems.Add($"{name}: {problem}");
            }

            if (!seen.Contains(MarkerName))
                problems.Add($"The archive has no {MarkerName}, so it is not a workspace.");
        }
        catch (Exception ex) when (ex is InvalidDataException or IOException or UnauthorizedAccessException)
        {
            problems.Add($"The archive could not be read: {ex.Message}");
        }

        if (problems.Count > 0)
            return new WorkspaceArchiveResult(0, problems);

        var created = !Directory.Exists(target);
        try
        {
            Directory.CreateDirectory(target);
            foreach (var (name, bytes) in files)
            {
                var path = Path.GetFullPath(Path.Combine(target, name.Replace('/', Path.DirectorySeparatorChar)));
                if (!path.StartsWith(target + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException($"{name} would be written outside the new folder.");

                Directory.CreateDirectory(Path.GetDirectoryName(path)!);
                File.WriteAllBytes(path, bytes);
            }

            // The same .gitignore a new workspace gets, so personal files stay out of git (ADR 0031).
            File.WriteAllText(Path.Combine(target, WorkspaceFolder.GitIgnoreFileName), WorkspaceFolder.GitIgnoreContent);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or InvalidDataException)
        {
            RemoveWhatWasWritten(target, created);
            return Failed($"The archive could not be imported: {ex.Message}");
        }

        return new WorkspaceArchiveResult(files.Count, []);
    }

    /// <summary>The files WorkspaceFolder reads as shared, before checking whether each one is in the right format.</summary>
    private static IEnumerable<string> SharedCandidates(string root)
    {
        foreach (var name in new[] { MarkerName, GlobalsName })
        {
            var path = Path.Combine(root, name);
            if (File.Exists(path))
                yield return path;
        }

        var environments = Path.Combine(root, "environments");
        if (Directory.Exists(environments))
        {
            foreach (var file in Directory.EnumerateFiles(environments, "*.json", SearchOption.AllDirectories))
                yield return file;
        }

        var collections = Path.Combine(root, "collections");
        if (Directory.Exists(collections))
        {
            foreach (var file in Directory.EnumerateFiles(collections, "*.json", SearchOption.AllDirectories))
                yield return file;
        }
    }

    /// <summary>Which shareable file a relative path is, or null when it is not one (including personal files).</summary>
    private static Kind? KindOf(string name)
    {
        var parts = name.Split('/');
        var leaf = parts[^1];
        if (!leaf.EndsWith(".json", StringComparison.OrdinalIgnoreCase) || PersonalFileNames.Contains(leaf))
            return null;

        if (parts.Length == 1)
            return leaf switch
            {
                MarkerName => Kind.Marker,
                GlobalsName => Kind.Globals,
                _ => null,
            };

        if (parts is ["environments", _])
            return Kind.Environment;

        if (parts[0] != "collections" || parts.Length < 3)
            return null;

        return leaf switch
        {
            CollectionName when parts.Length == 3 => Kind.Collection,
            FolderName when parts.Length >= 4 => Kind.Folder,
            CollectionName or FolderName => null,
            _ => Kind.Request,
        };
    }

    /// <summary>Checks that a file is UTF-8 JSON in the shape WorkspaceFolder reads. Returns the problem, or null.</summary>
    private static string? Check(Kind kind, byte[] bytes)
    {
        string text;
        try
        {
            text = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true).GetString(bytes);
        }
        catch (DecoderFallbackException)
        {
            return "It is not UTF-8 text.";
        }

        try
        {
            return kind switch
            {
                Kind.Marker => JsonSerializer.Deserialize(text, WorkspaceJsonContext.Default.WorkspaceMarkerFile) is null ? Empty : null,
                Kind.Globals => JsonSerializer.Deserialize(text, WorkspaceJsonContext.Default.ListKeyValue) is null ? Empty : null,
                Kind.Collection => Identified(JsonSerializer.Deserialize(text, WorkspaceJsonContext.Default.CollectionFile), c => c.Id),
                Kind.Folder => Identified(JsonSerializer.Deserialize(text, WorkspaceJsonContext.Default.FolderFile), f => f.Id),
                Kind.Environment => Identified(JsonSerializer.Deserialize(text, WorkspaceJsonContext.Default.ApiEnvironment), e => e.Id),
                Kind.Request => JsonSerializer.Deserialize(text, WorkspaceJsonContext.Default.RequestFile) is not { } request
                    ? Empty
                    : Identified(request, r => r.Id) ?? (request.Request is null ? "It has no request." : null),
                _ => null,
            };
        }
        catch (JsonException ex)
        {
            return $"It is not readable JSON in the workspace format: {ex.Message}";
        }
    }

    private const string Empty = "The file is empty.";

    private static string? Identified<T>(T? value, Func<T, string?> id) where T : class =>
        value is null ? Empty : string.IsNullOrWhiteSpace(id(value)) ? "It has no id." : null;

    /// <summary>
    /// False for an empty name, an absolute or rooted path, a drive letter, a backslash, or any "." or ".." part,
    /// so an entry can never point outside the folder it is extracted into.
    /// </summary>
    private static bool IsSafeName(string name)
    {
        if (name.Length == 0 || name.StartsWith('/') || name.Contains('\\') || name.Contains(':') || name.Contains('\0'))
            return false;

        return name.Split('/').All(part => part is not ("" or "." or ".."));
    }

    private static void RemoveWhatWasWritten(string target, bool created)
    {
        try
        {
            if (created)
            {
                Directory.Delete(target, recursive: true);
                return;
            }

            // The folder was empty before the import, so removing everything in it restores that.
            foreach (var entry in Directory.EnumerateFileSystemEntries(target))
            {
                if (Directory.Exists(entry))
                    Directory.Delete(entry, recursive: true);
                else
                    File.Delete(entry);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Nothing more can be done here; the reported failure is the one that matters.
        }
    }

    private static void TryDelete(string path)
    {
        try
        {
            File.Delete(path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Left behind only when the write already failed.
        }
    }

    private static WorkspaceArchiveResult Failed(string problem) => new(0, [problem]);
}
