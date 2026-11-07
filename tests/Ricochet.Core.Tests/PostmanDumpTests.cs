using System.IO.Compression;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class PostmanDumpTests
{
    private const string V21 = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

    private static string Collection(string name) =>
        $$"""{ "info": { "name": "{{name}}", "schema": "{{V21}}" }, "item": [ { "name": "Ping", "request": { "url": "https://x.test/ping" } } ] }""";

    private static string Environment(string name) =>
        $$"""{ "name": "{{name}}", "values": [ { "key": "host", "value": "x.test", "enabled": true } ] }""";

    private const string Globals = """{ "_postman_variable_scope": "globals", "values": [ { "key": "token", "value": "t", "enabled": true } ] }""";

    /// <summary>Builds a dump ZIP in memory from entry name and text pairs. A name ending in '/' makes a folder entry.</summary>
    private static MemoryStream Zip(params (string Name, string Text)[] entries)
    {
        var stream = new MemoryStream();
        using (var archive = new ZipArchive(stream, ZipArchiveMode.Create, leaveOpen: true))
        {
            foreach (var (name, text) in entries)
            {
                var entry = archive.CreateEntry(name);
                if (text.Length == 0)
                    continue;
                using var writer = new StreamWriter(entry.Open(), new UTF8Encoding(false));
                writer.Write(text);
            }
        }

        stream.Position = 0;
        return stream;
    }

    [Fact]
    public void Lists_every_collection_environment_and_globals_in_the_dump()
    {
        using var dump = Zip(
            ("shop.json", Collection("Shop API")),
            ("staging.json", Environment("Staging")),
            ("globals.json", Globals),
            ("billing.json", Collection("Billing")));

        var entries = PostmanImporter.ReadDump(dump);

        Assert.Equal(["Shop API", "Staging", "Globals", "Billing"], entries.Select(e => e.Name));
        Assert.Equal(["Collection", "Environment", "Globals", "Collection"], entries.Select(e => e.Kind));
        Assert.Equal(["shop.json", "staging.json", "globals.json", "billing.json"], entries.Select(e => e.FileName));
        Assert.Equal("Ping", entries[0].Result.Collection!.Items.OfType<CollectionItem>().Single().Name);
    }

    [Fact]
    public void Selected_returns_only_the_chosen_entries_in_dump_order()
    {
        using var dump = Zip(
            ("shop.json", Collection("Shop API")),
            ("staging.json", Environment("Staging")),
            ("billing.json", Collection("Billing")));
        var entries = PostmanImporter.ReadDump(dump);

        // Selection is by file name, so the order the user ticked in does not matter.
        var chosen = PostmanImporter.Selected(entries, new HashSet<string> { "billing.json", "shop.json", "not-in-dump.json" });

        Assert.Equal(["Shop API", "Billing"], chosen.Select(r => r.Collection!.Name));
    }

    [Fact]
    public void Selecting_nothing_imports_nothing()
    {
        using var dump = Zip(("shop.json", Collection("Shop API")));
        var entries = PostmanImporter.ReadDump(dump);

        Assert.Empty(PostmanImporter.Selected(entries, new HashSet<string>()));
    }

    [Fact]
    public void Entries_in_folders_are_listed_by_their_path_and_folder_entries_are_ignored()
    {
        using var dump = Zip(
            ("collections/", ""),
            ("collections/shop.json", Collection("Shop API")));

        var entries = PostmanImporter.ReadDump(dump);

        var entry = Assert.Single(entries);
        Assert.Equal("collections/shop.json", entry.FileName);
        Assert.Equal("Shop API", entry.Name);
    }

    [Fact]
    public void Refuses_a_file_that_is_not_a_zip()
    {
        using var notZip = new MemoryStream(Encoding.UTF8.GetBytes(Collection("Shop API")));

        var ex = Assert.Throws<PostmanImportException>(() => PostmanImporter.ReadDump(notZip));
        Assert.Contains("not a ZIP", ex.Message);
    }

    [Fact]
    public void Refuses_an_empty_zip()
    {
        using var dump = Zip();

        var ex = Assert.Throws<PostmanImportException>(() => PostmanImporter.ReadDump(dump));
        Assert.Contains("no files", ex.Message);
    }

    [Fact]
    public void Refuses_a_dump_with_a_file_that_is_not_json()
    {
        using var dump = Zip(("shop.json", Collection("Shop API")), ("notes.txt", "hello"));

        var ex = Assert.Throws<PostmanImportException>(() => PostmanImporter.ReadDump(dump));
        Assert.Contains("notes.txt", ex.Message);
    }

    [Fact]
    public void Refuses_a_dump_with_an_entry_that_is_not_a_postman_export_and_names_it()
    {
        using var dump = Zip(("shop.json", Collection("Shop API")), ("manifest.json", """{ "version": 3 }"""));

        var ex = Assert.Throws<PostmanImportException>(() => PostmanImporter.ReadDump(dump));
        Assert.Contains("manifest.json", ex.Message);
        Assert.Contains("not a Postman collection", ex.Message);
    }

    [Fact]
    public void Refuses_a_dump_with_invalid_json_and_names_the_entry()
    {
        using var dump = Zip(("broken.json", "{ not json"));

        var ex = Assert.Throws<PostmanImportException>(() => PostmanImporter.ReadDump(dump));
        Assert.StartsWith("broken.json:", ex.Message);
    }

    [Fact]
    public void Refuses_a_dump_in_the_older_v1_format()
    {
        using var dump = Zip(("old.json", """{ "version": 1, "collections": [] }"""));

        var ex = Assert.Throws<PostmanImportException>(() => PostmanImporter.ReadDump(dump));
        Assert.Contains("v1", ex.Message);
    }

    [Fact]
    public void Refuses_a_dump_that_lists_the_same_file_twice()
    {
        using var dump = Zip(("shop.json", Collection("Shop API")), ("shop.json", Collection("Shop API 2")));

        var ex = Assert.Throws<PostmanImportException>(() => PostmanImporter.ReadDump(dump));
        Assert.Contains("more than once", ex.Message);
    }
}
