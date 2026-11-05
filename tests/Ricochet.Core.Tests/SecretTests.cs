using System.Runtime.Versioning;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class SecretRulesTests
{
    [Theory]
    [InlineData("abc", true)]
    [InlineData("Bearer {{token}}", true)]
    [InlineData("{{token}}", false)]
    [InlineData(" {{a}}{{b}} ", false)]
    [InlineData("", false)]
    public void Only_placeholders_are_safe_to_share(string value, bool expected)
    {
        Assert.Equal(expected, SecretRules.IsLiteral(value));
    }

    [Fact]
    public void Finds_credentials_in_headers_and_urls()
    {
        var found = SecretRules.FindLikelyLiterals(new ApiRequest
        {
            Url = "http://x.test/?api_key=abc&token={{ok}}&page=2",
            Headers =
            [
                new("authorization", "Bearer abc"),
                new("Cookie", "{{ok}}"),
                new("X-Api-Key", "off", Enabled: false),
                new("Accept", "text/plain"),
            ],
        });

        Assert.Equal(["header authorization", "URL parameter api_key"], found);
    }
}

public class WorkspaceSecretsTests
{
    private static ApiRequest RequestWithAuth(RequestAuth auth) => new() { Id = "r1", Name = "R", Url = "u", Auth = auth };

    private static Workspace Sample() => new()
    {
        Collections =
        [
            new Collection
            {
                Id = "c1",
                Variables = [new("csecret", "c-value", IsSecret: true), new("plain", "p")],
                Items =
                [
                    new CollectionItem
                    {
                        Name = "Folder",
                        Children =
                        [
                            new CollectionItem { Name = "R", Request = RequestWithAuth(new RequestAuth { Kind = AuthKinds.Bearer, Token = "literal-token" }) },
                        ],
                    },
                ],
            },
        ],
        Environments = [new ApiEnvironment { Id = "e1", Variables = [new("esecret", "e-value", IsSecret: true)] }],
        Globals = [new("gsecret", "g-value", IsSecret: true)],
    };

    [Fact]
    public void Split_removes_every_secret_and_merge_restores_them()
    {
        var (clean, secrets) = WorkspaceSecrets.Split(Sample());

        var dir = Directory.CreateTempSubdirectory("ricochet-secrets").FullName;
        var folder = new WorkspaceFolder(dir);
        folder.Load();
        folder.Save(clean);
        var written = string.Concat(Directory.EnumerateFiles(dir, "*", SearchOption.AllDirectories).Select(File.ReadAllText));
        foreach (var value in new[] { "c-value", "e-value", "g-value", "literal-token" })
        {
            Assert.DoesNotContain(value, written);
            Assert.Contains(value, secrets.Values);
        }
        Assert.Contains("\"p\"", written);
        Assert.Contains("csecret", written);

        var merged = WorkspaceSecrets.Merge(new WorkspaceFolder(dir).Load().Workspace, secrets);
        Assert.Equal("c-value", merged.Collections[0].Variables[0].Value);
        Assert.True(merged.Collections[0].Variables[0].IsSecret);
        Assert.Equal("literal-token", merged.Collections[0].Items[0].Children[0].Request!.Auth!.Token);
        Assert.Equal("e-value", merged.Environments[0].Variables[0].Value);
        Assert.Equal("g-value", merged.Globals[0].Value);
    }

    [Fact]
    public void Placeholders_in_auth_stay_in_the_file()
    {
        var request = RequestWithAuth(new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" });
        var (clean, secrets) = WorkspaceSecrets.Split(new Workspace
        {
            Collections = [new Collection { Items = [new CollectionItem { Request = request }] }],
        });

        Assert.Empty(secrets);
        Assert.Equal("{{token}}", clean.Collections[0].Items[0].Request!.Auth!.Token);
    }

    [Fact]
    public void Same_variable_name_in_different_scopes_keeps_separate_values()
    {
        var (clean, secrets) = WorkspaceSecrets.Split(new Workspace
        {
            Environments =
            [
                new ApiEnvironment { Id = "staging", Variables = [new("token", "s", IsSecret: true)] },
                new ApiEnvironment { Id = "prod", Variables = [new("token", "p", IsSecret: true)] },
            ],
        });

        var merged = WorkspaceSecrets.Merge(clean, secrets);
        Assert.Equal("s", merged.Environments[0].Variables[0].Value);
        Assert.Equal("p", merged.Environments[1].Variables[0].Value);
    }

    [Fact]
    public void Removed_secret_is_gone_from_the_next_split()
    {
        var (_, secrets) = WorkspaceSecrets.Split(new Workspace { Globals = [new("a", "1", IsSecret: true)] });
        Assert.Single(secrets);

        var (_, after) = WorkspaceSecrets.Split(new Workspace());
        Assert.Empty(after);
    }

    [Fact]
    public void A_value_found_in_the_file_wins_over_the_vault()
    {
        var clean = new Workspace { Globals = [new("a", "from-file", IsSecret: true)] };
        var merged = WorkspaceSecrets.Merge(clean, new Dictionary<string, string> { ["variable/globals/a"] = "from-vault" });
        Assert.Equal("from-file", merged.Globals[0].Value);
    }

    [Fact]
    public void Secret_missing_on_this_machine_stays_empty_and_reports_as_unresolved()
    {
        var merged = WorkspaceSecrets.Merge(new Workspace { Globals = [new("token", "", IsSecret: true)] }, new Dictionary<string, string>());
        Assert.Equal("", merged.Globals[0].Value);

        var resolved = VariableResolver.Apply(new ApiRequest { Url = "{{token}}" }, merged.Globals);
        Assert.Equal(["token"], VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void History_hides_credentials_but_keeps_placeholders()
    {
        var entry = new HistoryEntry(new ApiRequest
        {
            Url = "u",
            Headers = [new("Authorization", "Bearer abc"), new("Cookie", "{{cookie}}"), new("Accept", "x")],
            Auth = new RequestAuth { Kind = AuthKinds.Basic, Username = "me", Password = "pw" },
        }, 200, 1, DateTimeOffset.UnixEpoch);

        var request = WorkspaceSecrets.SanitizeHistory(entry).Request;

        Assert.Equal(SecretRules.HiddenValue, request.Headers[0].Value);
        Assert.Equal("{{cookie}}", request.Headers[1].Value);
        Assert.Equal("x", request.Headers[2].Value);
        Assert.Equal("me", request.Auth!.Username);
        Assert.Equal("", request.Auth.Password);
        Assert.True(WorkspaceSecrets.HasHiddenValues(request));
    }

    [Fact]
    public void Split_also_cleans_history_already_on_disk()
    {
        var workspace = new Workspace
        {
            History = [new HistoryEntry(new ApiRequest { Headers = [new("Authorization", "old-token")] }, 200, 1, DateTimeOffset.UnixEpoch)],
        };

        Assert.True(WorkspaceSecrets.NeedsMigration(workspace));
        var (clean, _) = WorkspaceSecrets.Split(workspace);
        Assert.Equal(SecretRules.HiddenValue, clean.History[0].Request.Headers[0].Value);
        Assert.False(WorkspaceSecrets.NeedsMigration(clean));
    }

    [Fact]
    public void Plain_text_auth_from_an_older_file_needs_migration()
    {
        Assert.True(WorkspaceSecrets.NeedsMigration(Sample()));
        Assert.False(WorkspaceSecrets.NeedsMigration(WorkspaceSecrets.Split(Sample()).Clean));
    }

    [Fact]
    public void Files_from_before_secrets_existed_load_as_not_secret()
    {
        var path = Path.Combine(Directory.CreateTempSubdirectory("ricochet-secrets").FullName, "w.json");
        File.WriteAllText(path, """{ "globals": [ { "key": "a", "value": "1", "enabled": true } ] }""");

        Assert.False(new LegacyWorkspaceFile(path).Load().Globals[0].IsSecret);
    }

    [Fact]
    public void Non_secret_variables_do_not_write_the_flag()
    {
        var path = Path.Combine(Directory.CreateTempSubdirectory("ricochet-secrets").FullName, "w.json");
        var folder = new WorkspaceFolder(Path.GetDirectoryName(path)!);
        folder.Load();
        folder.Save(new Workspace { Globals = [new("a", "1")] });

        Assert.DoesNotContain("isSecret", File.ReadAllText(Path.Combine(Path.GetDirectoryName(path)!, "globals.json")));
    }

    [Fact]
    public void Import_marks_postman_secrets()
    {
        var result = PostmanImporter.Import(
            """{ "name": "E", "values": [ { "key": "s", "value": "x", "type": "secret" }, { "key": "p", "value": "y" } ] }""", "e.json");

        Assert.True(result.Environment!.Variables[0].IsSecret);
        Assert.False(result.Environment.Variables[1].IsSecret);
    }
}

[SupportedOSPlatform("windows")]
public sealed class WindowsSecretVaultTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-vault").FullName;
    private string VaultPath => Path.Combine(_dir, "nested", "secrets.dat");

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Missing_file_is_an_empty_vault()
    {
        Assert.SkipUnless(OperatingSystem.IsWindows(), "DPAPI is Windows only");
        Assert.Empty(new WindowsSecretVault(VaultPath).Load());
    }

    [Fact]
    public void Round_trips_and_is_encrypted_on_disk()
    {
        Assert.SkipUnless(OperatingSystem.IsWindows(), "DPAPI is Windows only");
        var vault = new WindowsSecretVault(VaultPath);

        vault.Save(new Dictionary<string, string> { ["k"] = "super-secret-value-é" });

        Assert.Equal("super-secret-value-é", vault.Load()["k"]);
        var onDisk = File.ReadAllBytes(VaultPath);
        Assert.DoesNotContain("super-secret-value", Encoding.UTF8.GetString(onDisk));
        Assert.DoesNotContain("super-secret-value", Encoding.Unicode.GetString(onDisk));
    }

    [Fact]
    public void Unreadable_file_is_kept_and_reported_never_overwritten()
    {
        Assert.SkipUnless(OperatingSystem.IsWindows(), "DPAPI is Windows only");
        Directory.CreateDirectory(Path.GetDirectoryName(VaultPath)!);
        File.WriteAllText(VaultPath, "not encrypted data");
        var vault = new WindowsSecretVault(VaultPath);

        var ex = Assert.Throws<SecretVaultException>(() => vault.Load());

        var kept = Assert.Single(Directory.GetFiles(Path.GetDirectoryName(VaultPath)!, "*.unreadable-*"));
        Assert.Equal("not encrypted data", File.ReadAllText(kept));
        Assert.Contains(kept, ex.Message);
        Assert.Empty(vault.Load());
    }
}

public class InMemorySecretVaultTests
{
    [Fact]
    public void Round_trips_for_the_session()
    {
        var vault = new InMemorySecretVault();
        Assert.Empty(vault.Load());
        vault.Save(new Dictionary<string, string> { ["k"] = "v" });
        Assert.Equal("v", vault.Load()["k"]);
    }
}
