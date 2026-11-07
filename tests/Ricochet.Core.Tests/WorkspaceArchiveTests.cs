using System.IO.Compression;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class WorkspaceArchiveTests : IDisposable
{
    private readonly string _work = Directory.CreateTempSubdirectory("ricochet-archive").FullName;
    private string Source => Path.Combine(_work, "source");
    private string Zip => Path.Combine(_work, "workspace.zip");
    private string Target => Path.Combine(_work, "imported");

    public void Dispose() => Directory.Delete(_work, recursive: true);

    private static ApiRequest Request(string name, string id) => new()
    {
        Id = id,
        Name = name,
        Url = $"http://x.test/{name}",
    };

    private static Workspace Sample() => new()
    {
        Collections =
        [
            new Collection
            {
                Id = "c1aaaaaaaa",
                Name = "Shop API",
                Variables = [new("base", "http://x.test")],
                Items =
                [
                    new CollectionItem { Id = "i1aaaaaaaa", Name = "Zebra", Request = Request("Zebra", "r1aaaaaaaa") },
                    new CollectionItem
                    {
                        Id = "f1aaaaaaaa",
                        Name = "Orders",
                        Children = [new CollectionItem { Id = "i2aaaaaaaa", Name = "Create", Request = Request("Create", "r2aaaaaaaa") }],
                    },
                ],
            },
        ],
        Environments = [new ApiEnvironment { Id = "e1aaaaaaaa", Name = "Staging", Variables = [new("a", "1")] }],
        Globals = [new("g", "1")],
    };

    private void SaveSample()
    {
        var folder = new WorkspaceFolder(Source);
        folder.Load();
        folder.Save(Sample());
    }

    /// <summary>Writes a ZIP with exactly these entries. Names are written as given, so unsafe ones can be tested.</summary>
    private static void MakeZip(string path, params (string Name, string Content)[] entries)
    {
        using var stream = File.Create(path);
        using var zip = new ZipArchive(stream, ZipArchiveMode.Create);
        foreach (var (name, content) in entries)
        {
            using var writer = new StreamWriter(zip.CreateEntry(name).Open(), new UTF8Encoding(false));
            writer.Write(content);
        }
    }

    private static Dictionary<string, string> ReadZip(string path)
    {
        using var zip = ZipFile.OpenRead(path);
        return zip.Entries.ToDictionary(e => e.FullName, e =>
        {
            using var reader = new StreamReader(e.Open(), Encoding.UTF8);
            return reader.ReadToEnd();
        });
    }

    private const string ValidMarker = "{\"id\":\"w1aaaaaaaa\"}";

    [Fact]
    public void Export_then_import_gives_back_the_same_workspace()
    {
        SaveSample();

        var exported = WorkspaceArchive.Export(Source, Zip);
        var imported = WorkspaceArchive.Import(Zip, Target);

        Assert.True(exported.Succeeded, string.Join("; ", exported.Problems));
        Assert.True(imported.Succeeded, string.Join("; ", imported.Problems));
        var (loaded, issues) = new WorkspaceFolder(Target).Load();
        Assert.Empty(issues);
        var collection = Assert.Single(loaded.Collections);
        Assert.Equal("Shop API", collection.Name);
        Assert.Equal(["Zebra", "Orders"], collection.Items.Select(i => i.Name));
        Assert.Equal("Create", collection.Items[1].Children[0].Name);
        Assert.Equal("Staging", Assert.Single(loaded.Environments).Name);
        Assert.Equal("g", Assert.Single(loaded.Globals).Key);
        Assert.True(File.Exists(Path.Combine(Target, ".gitignore")));
    }

    [Fact]
    public void Export_leaves_out_personal_files_history_secrets_and_ignored_patterns()
    {
        SaveSample();
        File.WriteAllText(Path.Combine(Source, "state.json"), "{\"theme\":\"personal-marker\"}");
        File.WriteAllText(Path.Combine(Source, "history.json"), "[\"personal-marker\"]");
        File.WriteAllText(Path.Combine(Source, "collections", "history.json"), "personal-marker");
        File.WriteAllText(Path.Combine(Source, "secrets.dat"), "personal-marker");
        File.WriteAllText(Path.Combine(Source, ".env"), "TOKEN=personal-marker");
        File.WriteAllText(Path.Combine(Source, "notes.secret"), "personal-marker");
        File.WriteAllText(Path.Combine(Source, "scratch.tmp"), "personal-marker");

        var result = WorkspaceArchive.Export(Source, Zip);

        Assert.True(result.Succeeded, string.Join("; ", result.Problems));
        var entries = ReadZip(Zip);
        Assert.DoesNotContain(entries.Keys, name => name.Contains("state", StringComparison.OrdinalIgnoreCase)
            || name.Contains("history", StringComparison.OrdinalIgnoreCase)
            || name.Contains("secret", StringComparison.OrdinalIgnoreCase)
            || name.Contains(".env", StringComparison.OrdinalIgnoreCase)
            || name.EndsWith(".tmp", StringComparison.OrdinalIgnoreCase)
            || name.EndsWith(".gitignore", StringComparison.OrdinalIgnoreCase));
        Assert.DoesNotContain(entries.Values, text => text.Contains("personal-marker", StringComparison.Ordinal));
        Assert.Contains("ricochet.json", entries.Keys);
        Assert.Contains("globals.json", entries.Keys);
    }

    [Fact]
    public void Export_leaves_out_secret_values_that_were_split_out_before_saving()
    {
        var workspace = Sample() with
        {
            Globals = [new KeyValue("token", "hunter2-secret", IsSecret: true)],
        };
        var (clean, _) = WorkspaceSecrets.Split(workspace);
        var folder = new WorkspaceFolder(Source);
        folder.Load();
        folder.Save(clean);

        var result = WorkspaceArchive.Export(Source, Zip);

        Assert.True(result.Succeeded, string.Join("; ", result.Problems));
        Assert.DoesNotContain(ReadZip(Zip).Values, text => text.Contains("hunter2-secret", StringComparison.Ordinal));
    }

    [Fact]
    public void Export_refuses_a_shared_file_it_cannot_read_and_writes_no_archive()
    {
        SaveSample();
        File.WriteAllText(Path.Combine(Source, "collections", "shop-api-c1aaaaaa", "broken-r9.json"), "{ not json");

        var result = WorkspaceArchive.Export(Source, Zip);

        Assert.False(result.Succeeded);
        Assert.Contains(result.Problems, p => p.Contains("broken-r9.json", StringComparison.Ordinal));
        Assert.False(File.Exists(Zip));
    }

    [Theory]
    [InlineData("../escape.json")]
    [InlineData("collections/../../escape.json")]
    [InlineData("/escape.json")]
    [InlineData("C:/escape.json")]
    [InlineData("collections\\escape.json")]
    [InlineData("collections//escape.json")]
    public void Import_refuses_entries_that_could_point_outside_the_folder(string name)
    {
        MakeZip(Zip, ("ricochet.json", ValidMarker), (name, "{\"id\":\"x\"}"));

        var result = WorkspaceArchive.Import(Zip, Target);

        Assert.False(result.Succeeded);
        Assert.Contains(result.Problems, p => p.StartsWith(name, StringComparison.Ordinal));
        Assert.False(Directory.Exists(Target));
        Assert.False(File.Exists(Path.Combine(_work, "escape.json")));
    }

    [Fact]
    public void Import_refuses_a_folder_that_already_holds_a_workspace_and_leaves_it_alone()
    {
        SaveSample();
        var existing = Path.Combine(Source, "ricochet.json");
        var before = File.ReadAllText(existing);
        MakeZip(Zip, ("ricochet.json", ValidMarker));

        var result = WorkspaceArchive.Import(Zip, Source);

        Assert.False(result.Succeeded);
        Assert.Equal(before, File.ReadAllText(existing));
        Assert.Single(new WorkspaceFolder(Source).Load().Workspace.Collections);
    }

    [Fact]
    public void Import_refuses_a_non_empty_folder_that_is_not_a_workspace()
    {
        Directory.CreateDirectory(Target);
        File.WriteAllText(Path.Combine(Target, "notes.txt"), "keep me");
        MakeZip(Zip, ("ricochet.json", ValidMarker));

        var result = WorkspaceArchive.Import(Zip, Target);

        Assert.False(result.Succeeded);
        Assert.Equal("keep me", File.ReadAllText(Path.Combine(Target, "notes.txt")));
        Assert.False(File.Exists(Path.Combine(Target, "ricochet.json")));
    }

    [Theory]
    [InlineData("collections/shop/collection.json", "not json at all")]
    [InlineData("collections/shop/collection.json", "{\"name\":\"no id\"}")]
    [InlineData("collections/shop/req-r1.json", "{\"id\":\"i1\",\"name\":\"no request\"}")]
    [InlineData("environments/staging.json", "[1, 2, 3]")]
    [InlineData("globals.json", "{\"not\":\"a list\"}")]
    public void Import_refuses_files_that_are_not_readable_workspace_json(string name, string content)
    {
        MakeZip(Zip, ("ricochet.json", ValidMarker), (name, content));

        var result = WorkspaceArchive.Import(Zip, Target);

        Assert.False(result.Succeeded);
        Assert.Contains(result.Problems, p => p.StartsWith(name, StringComparison.Ordinal));
        Assert.False(Directory.Exists(Target));
    }

    [Theory]
    [InlineData("state.json")]
    [InlineData("history.json")]
    [InlineData("secrets.dat")]
    [InlineData(".env")]
    [InlineData("random-notes.json")]
    public void Import_refuses_files_outside_the_workspace_layout(string name)
    {
        MakeZip(Zip, ("ricochet.json", ValidMarker), (name, "{\"id\":\"x\"}"));

        var result = WorkspaceArchive.Import(Zip, Target);

        Assert.False(result.Succeeded);
        Assert.Contains(result.Problems, p => p.StartsWith(name, StringComparison.Ordinal));
        Assert.False(Directory.Exists(Target));
    }

    [Fact]
    public void Import_refuses_an_archive_without_a_marker_file()
    {
        MakeZip(Zip, ("globals.json", "[]"));

        var result = WorkspaceArchive.Import(Zip, Target);

        Assert.False(result.Succeeded);
        Assert.Contains(result.Problems, p => p.Contains("ricochet.json", StringComparison.Ordinal));
        Assert.False(Directory.Exists(Target));
    }
}
