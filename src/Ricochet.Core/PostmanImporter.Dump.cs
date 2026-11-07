using System.IO.Compression;
using System.Text;

namespace Ricochet.Core;

/// <summary>One export inside a Postman data dump, already read, so choosing it needs no second read.</summary>
/// <param name="FileName">The path of the entry inside the ZIP. Unique within a dump, so it identifies the entry in a selection.</param>
public sealed record DumpEntry(string FileName, string Name, string Kind, ImportResult Result);

/// <summary>Reads a Postman data dump, a ZIP of collection, environment and globals exports (ADR 0038).</summary>
public static partial class PostmanImporter
{
    /// <summary>The most unpacked data a dump may hold, so a small ZIP cannot fill memory.</summary>
    public const long MaxDumpBytes = 100L * 1024 * 1024;

    /// <summary>Reads every export in a data dump. Nothing is imported until the caller chooses from this list.</summary>
    /// <exception cref="PostmanImportException">The file is not a ZIP, holds something other than Postman exports, or is too large.</exception>
    public static IReadOnlyList<DumpEntry> ReadDump(Stream dump)
    {
        ZipArchive archive;
        try
        {
            archive = new ZipArchive(dump, ZipArchiveMode.Read, leaveOpen: true);
        }
        catch (InvalidDataException)
        {
            throw new PostmanImportException("This is not a Postman data dump, because it is not a ZIP file.");
        }

        using (archive)
        {
            var files = archive.Entries.Where(e => !e.FullName.EndsWith('/')).ToList();
            if (files.Count == 0)
                throw new PostmanImportException("This data dump holds no files.");
            if (files.Sum(e => e.Length) > MaxDumpBytes)
                throw new PostmanImportException("This data dump is too large to read. Export the collections you need in smaller files.");

            var seen = new HashSet<string>(StringComparer.Ordinal);
            var entries = new List<DumpEntry>();
            foreach (var file in files)
            {
                if (!seen.Add(file.FullName))
                    throw new PostmanImportException($"{file.FullName} is listed more than once in this data dump.");
                if (!file.FullName.EndsWith(".json", StringComparison.OrdinalIgnoreCase))
                    throw new PostmanImportException($"{file.FullName} is not a Postman export. A data dump may hold only .json exports.");

                string json;
                try
                {
                    using var reader = new StreamReader(file.Open(), Encoding.UTF8);
                    json = reader.ReadToEnd();
                }
                catch (Exception ex) when (ex is IOException or InvalidDataException)
                {
                    throw new PostmanImportException($"{file.FullName} could not be read: {ex.Message}");
                }

                ImportResult result;
                try
                {
                    result = Import(json, Path.GetFileName(file.FullName));
                }
                catch (PostmanImportException ex)
                {
                    throw new PostmanImportException($"{file.FullName}: {ex.Message}");
                }

                entries.Add(new DumpEntry(file.FullName, NameOf(result), KindOf(result), result));
            }

            return entries;
        }
    }

    /// <summary>The results for the entries whose file names are selected, in dump order. Names not in the listing are ignored.</summary>
    public static IReadOnlyList<ImportResult> Selected(IReadOnlyList<DumpEntry> entries, IReadOnlySet<string> selectedFileNames) =>
        entries.Where(e => selectedFileNames.Contains(e.FileName)).Select(e => e.Result).ToList();

    private static string NameOf(ImportResult result) =>
        result.Collection?.Name ?? result.Environment?.Name ?? "Globals";

    private static string KindOf(ImportResult result) =>
        result.Collection is not null ? "Collection" : result.Environment is not null ? "Environment" : "Globals";
}
