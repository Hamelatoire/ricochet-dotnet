using System.Text;

namespace Ricochet.Core;

/// <summary>An export that cannot be written: no such Collection, a file that already exists, or a path inside the Workspace.</summary>
public sealed class ExportFileException(string message) : Exception(message);

/// <summary>
/// Finds the Collection to export and writes export files (ADR 0029). Export files must never land inside the
/// Workspace folder, because the Workspace is shared through git and a stray export there would be committed.
/// </summary>
public static class ExportFiles
{
    public static Collection FindCollection(IEnumerable<Collection> collections, string name)
    {
        var matches = collections.Where(c => string.Equals(c.Name, name, StringComparison.OrdinalIgnoreCase)).ToList();
        if (matches.Count == 1)
            return matches[0];

        if (matches.Count > 1)
            throw new ExportFileException($"More than one collection is named \"{name}\". Rename one of them and try again.");

        var names = collections.Select(c => c.Name).Where(n => n.Length > 0).ToList();
        var known = names.Count == 0 ? "The workspace has no collections." : "Collections: " + string.Join(", ", names) + ".";
        throw new ExportFileException($"No collection is named \"{name}\". {known}");
    }

    /// <summary>Checks a target path without writing. Returns its full path, or throws when it must not be written.</summary>
    public static string Check(string path, string workspaceRoot, bool force)
    {
        var full = Path.GetFullPath(path);
        if (IsInside(full, Path.GetFullPath(workspaceRoot)))
            throw new ExportFileException($"\"{full}\" is inside the workspace folder. Export to a folder outside it.");

        if (Directory.Exists(full))
            throw new ExportFileException($"\"{full}\" is a folder.");

        if (!force && File.Exists(full))
            throw new ExportFileException($"\"{full}\" already exists. Pass --force to overwrite it.");

        return full;
    }

    /// <summary>Writes UTF-8 without a byte-order mark. Refuses an existing file unless <paramref name="force"/> is set.</summary>
    public static void Write(string path, string contents, string workspaceRoot, bool force)
    {
        var full = Check(path, workspaceRoot, force);
        try
        {
            using var stream = new FileStream(full, force ? FileMode.Create : FileMode.CreateNew, FileAccess.Write, FileShare.None);
            using var writer = new StreamWriter(stream, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
            writer.Write(contents);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            throw new ExportFileException($"Could not write \"{full}\": {ex.Message}");
        }
    }

    /// <summary>True when the path is the folder itself or anything below it.</summary>
    public static bool IsInside(string fullPath, string folder)
    {
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        var root = folder.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
        return fullPath.StartsWith(root, comparison)
            || string.Equals(fullPath.TrimEnd(Path.DirectorySeparatorChar), folder.TrimEnd(Path.DirectorySeparatorChar), comparison);
    }
}
