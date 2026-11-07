using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class ExportFilesTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "ricochet-export-" + Guid.NewGuid().ToString("N"));

    public ExportFilesTests() => Directory.CreateDirectory(_root);

    public void Dispose()
    {
        if (Directory.Exists(_root))
            Directory.Delete(_root, recursive: true);
    }

    private string Workspace()
    {
        var workspace = Path.Combine(_root, "workspace");
        Directory.CreateDirectory(workspace);
        return workspace;
    }

    private static Collection Named(string name) => new() { Name = name };

    [Fact]
    public void FindCollection_matches_the_name_ignoring_case()
    {
        var found = ExportFiles.FindCollection([Named("Shop"), Named("Billing")], "shop");

        Assert.Equal("Shop", found.Name);
    }

    [Fact]
    public void FindCollection_names_the_collections_it_has_when_none_matches()
    {
        var error = Assert.Throws<ExportFileException>(() => ExportFiles.FindCollection([Named("Shop")], "Nope"));

        Assert.Contains("No collection is named \"Nope\"", error.Message);
        Assert.Contains("Shop", error.Message);
    }

    [Fact]
    public void FindCollection_refuses_two_collections_with_the_same_name()
    {
        var error = Assert.Throws<ExportFileException>(() => ExportFiles.FindCollection([Named("Shop"), Named("shop")], "Shop"));

        Assert.Contains("More than one", error.Message);
    }

    [Fact]
    public void Write_creates_a_new_file_as_utf8_without_a_byte_order_mark()
    {
        var target = Path.Combine(_root, "out.json");

        ExportFiles.Write(target, "{\"name\":\"café\"}", Workspace(), force: false);

        var bytes = File.ReadAllBytes(target);
        Assert.False(bytes.Length >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF);
        Assert.Equal("{\"name\":\"café\"}", Encoding.UTF8.GetString(bytes));
    }

    [Fact]
    public void Write_refuses_an_existing_file_without_force_and_leaves_it_unchanged()
    {
        var target = Path.Combine(_root, "out.json");
        File.WriteAllText(target, "original");

        var error = Assert.Throws<ExportFileException>(() => ExportFiles.Write(target, "new", Workspace(), force: false));

        Assert.Contains("--force", error.Message);
        Assert.Equal("original", File.ReadAllText(target));
    }

    [Fact]
    public void Write_overwrites_an_existing_file_with_force()
    {
        var target = Path.Combine(_root, "out.json");
        File.WriteAllText(target, "original");

        ExportFiles.Write(target, "new", Workspace(), force: true);

        Assert.Equal("new", File.ReadAllText(target));
    }

    [Fact]
    public void Write_refuses_a_path_inside_the_workspace_even_with_force()
    {
        var workspace = Workspace();

        Assert.Throws<ExportFileException>(() => ExportFiles.Write(Path.Combine(workspace, "export.json"), "x", workspace, force: true));
        Assert.Throws<ExportFileException>(() => ExportFiles.Write(Path.Combine(workspace, "sub", "export.json"), "x", workspace, force: true));
        Assert.False(File.Exists(Path.Combine(workspace, "export.json")));
    }

    [Fact]
    public void Write_allows_a_sibling_folder_whose_name_starts_with_the_workspace_name()
    {
        var workspace = Workspace();
        var sibling = Path.Combine(_root, "workspace-exports");
        Directory.CreateDirectory(sibling);

        ExportFiles.Write(Path.Combine(sibling, "export.json"), "x", workspace, force: false);

        Assert.True(File.Exists(Path.Combine(sibling, "export.json")));
    }

    [Fact]
    public void Write_refuses_a_folder_as_the_target()
    {
        var folder = Path.Combine(_root, "exports");
        Directory.CreateDirectory(folder);

        Assert.Throws<ExportFileException>(() => ExportFiles.Write(folder, "x", Workspace(), force: true));
    }

    [Fact]
    public void Write_reports_a_missing_folder_as_an_export_error()
    {
        var target = Path.Combine(_root, "missing", "out.json");

        Assert.Throws<ExportFileException>(() => ExportFiles.Write(target, "x", Workspace(), force: false));
    }
}
