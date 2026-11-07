using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class EnvironmentTransferTests
{
    private static ApiEnvironment Staging() => new()
    {
        Id = "e1aaaaaaaa",
        Name = "Staging",
        Variables =
        [
            new("baseUrl", "https://staging.example.test"),
            new("token", "s3cret-value", IsSecret: true),
            new("off", "1", Enabled: false),
        ],
    };

    [Fact]
    public void Export_leaves_secret_values_out_but_keeps_their_names()
    {
        var json = EnvironmentTransfer.Export(Staging());

        Assert.DoesNotContain("s3cret-value", json);
        var back = EnvironmentTransfer.Import(json, "staging.json").Environment;
        Assert.Equal("Staging", back.Name);
        var token = back.Variables.Single(v => v.Key == "token");
        Assert.True(token.IsSecret);
        Assert.Equal("", token.Value);
    }

    [Fact]
    public void Export_keeps_plain_values_and_enabled_flags()
    {
        var back = EnvironmentTransfer.Import(EnvironmentTransfer.Export(Staging()), "staging.json").Environment;

        Assert.Equal("https://staging.example.test", back.Variables.Single(v => v.Key == "baseUrl").Value);
        Assert.False(back.Variables.Single(v => v.Key == "off").Enabled);
        Assert.False(back.Variables.Single(v => v.Key == "baseUrl").IsSecret);
    }

    [Fact]
    public void Import_of_an_export_reports_each_secret_as_needing_a_value()
    {
        var result = EnvironmentTransfer.Import(EnvironmentTransfer.Export(Staging()), "staging.json");

        var issue = Assert.Single(result.Issues);
        Assert.Equal(ImportCategories.Secrets, issue.Category);
        Assert.Contains("token", issue.Detail);
    }

    [Fact]
    public void Imported_secrets_are_kept_out_of_workspace_files()
    {
        var root = Directory.CreateTempSubdirectory("ricochet-env").FullName;
        try
        {
            var postman = """
                { "name": "Prod", "values": [
                  { "key": "apiKey", "value": "postman-secret-99", "type": "secret", "enabled": true },
                  { "key": "host", "value": "api.example.test", "enabled": true }
                ] }
                """;
            var imported = EnvironmentTransfer.Import(postman, "prod.json").Environment;
            var workspace = new Workspace { Environments = EnvironmentTransfer.Place([], imported, EnvironmentClash.Copy).Environments };

            var folder = new WorkspaceFolder(root);
            folder.Load();
            folder.Save(WorkspaceSecrets.Split(workspace).Clean);

            var written = string.Concat(Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories).Select(File.ReadAllText));
            Assert.DoesNotContain("postman-secret-99", written);
            Assert.Contains("api.example.test", written);
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void Postman_variable_names_are_fitted_and_reported()
    {
        var postman = """
            { "name": "Dev", "values": [ { "key": "my token!", "value": "1", "enabled": true }, { "key": "n", "value": 42, "enabled": true } ] }
            """;

        var result = EnvironmentTransfer.Import(postman, "dev.json");

        Assert.Equal(["my_token_", "n"], result.Environment.Variables.Select(v => v.Key));
        Assert.Equal("42", result.Environment.Variables[1].Value);
        var issue = Assert.Single(result.Issues);
        Assert.Equal(EnvironmentTransfer.RenamedVariables, issue.Category);
        Assert.Contains("my_token_", issue.Detail);
    }

    [Fact]
    public void Postman_secret_with_value_is_imported_with_its_value_for_the_vault()
    {
        var postman = """
            { "name": "Dev", "values": [ { "key": "pw", "value": "hunter2", "type": "secret", "enabled": true } ] }
            """;

        var variable = EnvironmentTransfer.Import(postman, "dev.json").Environment.Variables.Single();

        Assert.True(variable.IsSecret);
        Assert.Equal("hunter2", variable.Value);
    }

    [Fact]
    public void Collections_and_globals_are_not_environments()
    {
        var collection = """{ "info": { "name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" }, "item": [] }""";
        var globals = """{ "_postman_variable_scope": "globals", "values": [ { "key": "g", "value": "1", "enabled": true } ] }""";

        Assert.Throws<EnvironmentImportException>(() => EnvironmentTransfer.Import(collection, "shop.json"));
        Assert.Throws<EnvironmentImportException>(() => EnvironmentTransfer.Import(globals, "globals.json"));
    }

    [Fact]
    public void Newer_format_versions_are_refused()
    {
        var json = """{ "format": "ricochet-environment", "version": 99, "name": "X", "variables": [] }""";

        Assert.Throws<EnvironmentImportException>(() => EnvironmentTransfer.Import(json, "x.json"));
    }

    [Fact]
    public void Invalid_json_is_refused()
    {
        Assert.Throws<EnvironmentImportException>(() => EnvironmentTransfer.Import("{ nope", "x.json"));
    }

    [Fact]
    public void Without_a_clash_the_environment_is_added()
    {
        var incoming = new ApiEnvironment { Name = "Prod" };

        var placed = EnvironmentTransfer.Place([Staging()], incoming, EnvironmentClash.Skip);

        Assert.Equal(["Staging", "Prod"], placed.Environments.Select(e => e.Name));
        Assert.NotNull(placed.Placed);
    }

    [Fact]
    public void Secret_count_counts_secret_variables_whether_or_not_they_have_a_value()
    {
        var environment = new ApiEnvironment
        {
            Name = "Staging",
            Variables = [new("baseUrl", "x"), new("token", "s3cret-value", IsSecret: true), new("empty", "", IsSecret: true)],
        };

        Assert.Equal(2, EnvironmentTransfer.SecretCount(environment));
    }

    [Fact]
    public void Find_matches_a_name_ignoring_case_and_returns_null_when_absent_or_ambiguous()
    {
        var environments = new[] { Staging(), new ApiEnvironment { Name = "Prod" } };

        Assert.Equal("Staging", EnvironmentTransfer.FindEnvironment(environments, "staging")?.Name);
        Assert.Null(EnvironmentTransfer.FindEnvironment(environments, "Dev"));
        Assert.Null(EnvironmentTransfer.FindEnvironment([Staging(), new ApiEnvironment { Name = "STAGING" }], "staging"));
    }

    [Fact]
    public void Replace_overwrites_and_keeps_the_existing_id()
    {
        var incoming = new ApiEnvironment { Name = "staging", Variables = [new("baseUrl", "https://new.example.test")] };

        var placed = EnvironmentTransfer.Place([Staging()], incoming, EnvironmentClash.Replace);

        var only = Assert.Single(placed.Environments);
        Assert.Equal("e1aaaaaaaa", only.Id);
        Assert.Equal("https://new.example.test", only.Variables.Single().Value);
    }

    [Fact]
    public void Replace_keeps_this_machines_secret_values_when_the_file_has_none()
    {
        var incoming = new ApiEnvironment
        {
            Name = "Staging",
            Variables = [new("token", "", IsSecret: true), new("fresh", "", IsSecret: true)],
        };

        var placed = EnvironmentTransfer.Place([Staging()], incoming, EnvironmentClash.Replace);

        var variables = Assert.Single(placed.Environments).Variables;
        Assert.Equal("s3cret-value", variables.Single(v => v.Key == "token").Value);
        Assert.Equal("", variables.Single(v => v.Key == "fresh").Value);
    }

    [Fact]
    public void Copy_numbers_the_name()
    {
        var incoming = new ApiEnvironment { Name = "Staging" };

        var placed = EnvironmentTransfer.Place([Staging()], incoming, EnvironmentClash.Copy);

        Assert.Equal(["Staging", "Staging (2)"], placed.Environments.Select(e => e.Name));
        Assert.NotEqual("e1aaaaaaaa", placed.Placed!.Id);
    }

    [Fact]
    public void Skip_leaves_the_existing_environment_alone()
    {
        var existing = new[] { Staging() };

        var placed = EnvironmentTransfer.Place(existing, new ApiEnvironment { Name = "Staging" }, EnvironmentClash.Skip);

        Assert.Null(placed.Placed);
        Assert.Equal(existing, placed.Environments);
    }
}
