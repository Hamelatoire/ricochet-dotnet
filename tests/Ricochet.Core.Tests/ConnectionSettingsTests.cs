using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class ConnectionSettingsTests : IDisposable
{
    private const string Password = "pfx-pass-7d3";

    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-connection").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Proxy_and_certificate_default_to_none()
    {
        var settings = new PersonalSettings();

        Assert.Equal("", settings.ProxyUrl);
        Assert.Equal("", settings.ClientCertificatePath);
    }

    [Fact]
    public void Proxy_and_certificate_path_survive_a_restart_and_stay_out_of_the_workspace()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        store.SaveSettings(new PersonalSettings
        {
            ProxyUrl = "http://proxy.corp.example:8080",
            ClientCertificatePath = @"C:\certs\client.pfx",
        });

        var reloaded = new WorkspaceStore(_dir);
        reloaded.Load();

        Assert.Equal("http://proxy.corp.example:8080", reloaded.Settings.ProxyUrl);
        Assert.Equal(@"C:\certs\client.pfx", reloaded.Settings.ClientCertificatePath);
        var sharedFiles = Directory.GetFiles(reloaded.FolderPath, "*", SearchOption.AllDirectories).Select(File.ReadAllText);
        Assert.All(sharedFiles, text =>
        {
            Assert.DoesNotContain("proxy.corp.example", text);
            Assert.DoesNotContain("client.pfx", text);
        });
    }

    [Fact]
    public void Certificate_password_is_never_written_to_the_personal_settings_file()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        store.SaveSettings(new PersonalSettings { ClientCertificatePath = @"C:\certs\client.pfx" });

        var state = File.ReadAllText(Path.Combine(_dir, "state.json"));

        Assert.DoesNotContain("password", state, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Certificate_password_is_stored_as_a_personal_secret_and_can_be_removed()
    {
        var saved = PersonalSecrets.WithClientCertificatePassword(new Dictionary<string, string>(), Password);

        Assert.Equal(Password, PersonalSecrets.ClientCertificatePassword(saved));

        var cleared = PersonalSecrets.WithClientCertificatePassword(saved, "");
        Assert.Null(PersonalSecrets.ClientCertificatePassword(cleared));
        Assert.Empty(cleared);
    }

    [Fact]
    public void Saving_a_workspace_keeps_the_certificate_password()
    {
        var vault = PersonalSecrets.WithClientCertificatePassword(new Dictionary<string, string>(), Password);

        var afterWorkspaceSave = WorkspaceSecrets.WithWorkspace(vault, "ws-1", new Dictionary<string, string> { ["variable/a/token"] = "t" });

        Assert.Equal(Password, PersonalSecrets.ClientCertificatePassword(afterWorkspaceSave));
    }

    [Fact]
    public void Certificate_password_is_not_part_of_any_workspace()
    {
        var vault = PersonalSecrets.WithClientCertificatePassword(new Dictionary<string, string>(), Password);

        Assert.DoesNotContain(PersonalSecrets.ClientCertificatePasswordKey, WorkspaceSecrets.ForWorkspace(vault, "ws-1").Keys);
    }

    [Fact]
    public void Splitting_a_workspace_never_picks_up_the_certificate_password()
    {
        var workspace = new Workspace
        {
            Collections = [new Collection { Name = "c", Id = "col" }],
        };

        var (_, secrets) = WorkspaceSecrets.Split(workspace);

        Assert.DoesNotContain(PersonalSecrets.ClientCertificatePasswordKey, secrets.Keys);
    }

    [Theory]
    [InlineData("", null)]
    [InlineData("   ", null)]
    [InlineData("proxy.corp:8080", "http://proxy.corp:8080/")]
    [InlineData("http://proxy.corp:8080", "http://proxy.corp:8080/")]
    [InlineData("https://proxy.corp", "https://proxy.corp/")]
    public void Proxy_addresses_are_read_with_http_as_the_default_scheme(string text, string? expected)
    {
        var uri = RequestSender.ParseProxy(text);

        Assert.Equal(expected, uri?.ToString());
    }

    [Theory]
    [InlineData("socks5://proxy.corp:1080")]
    [InlineData("http://user:pass@proxy.corp:8080")]
    [InlineData("http://")]
    public void Proxy_addresses_that_cannot_be_used_are_refused(string text)
    {
        Assert.Throws<RequestFailedException>(() => RequestSender.ParseProxy(text));
    }

    [Fact]
    public void A_certificate_loads_with_its_password_and_a_wrong_password_is_reported_without_it()
    {
        var path = WriteCertificate("client.pfx");

        using var certificate = RequestSender.LoadClientCertificate(path, Password);
        Assert.True(certificate.HasPrivateKey);

        var error = Assert.Throws<RequestFailedException>(() => RequestSender.LoadClientCertificate(path, "wrong-pass"));
        Assert.Contains("client certificate", error.Message);
        Assert.DoesNotContain("wrong-pass", error.Message);
    }

    [Fact]
    public async Task A_missing_certificate_file_stops_requests_with_a_clear_message()
    {
        var sender = new RequestSender(new HttpClient());
        var problem = sender.Configure(TimeSpan.FromSeconds(5), true, true, 1024, clientCertificatePath: Path.Combine(_dir, "missing.pfx"));

        Assert.NotNull(problem);
        var error = await Assert.ThrowsAsync<RequestFailedException>(() => sender.SendAsync(new ApiRequest { Url = "http://127.0.0.1:1/" }));
        Assert.Contains("was not found", error.Message);
    }

    [Fact]
    public async Task An_invalid_proxy_is_reported_when_settings_are_applied_and_stops_requests()
    {
        var sender = new RequestSender(new HttpClient());

        var problem = sender.Configure(TimeSpan.FromSeconds(5), true, true, 1024, proxyUrl: "socks5://proxy.corp:1080");

        Assert.NotNull(problem);
        await Assert.ThrowsAsync<RequestFailedException>(() => sender.SendAsync(new ApiRequest { Url = "http://127.0.0.1:1/" }));
    }

    [Fact]
    public void Applying_valid_settings_reports_no_problem()
    {
        var sender = new RequestSender(new HttpClient());

        var problem = sender.Configure(TimeSpan.FromSeconds(5), true, true, 1024, proxyUrl: "proxy.corp:8080");

        Assert.Null(problem);
    }

    private string WriteCertificate(string fileName)
    {
        using var key = ECDsa.Create(ECCurve.NamedCurves.nistP256);
        var request = new CertificateRequest("CN=ricochet-test", key, HashAlgorithmName.SHA256);
        using var certificate = request.CreateSelfSigned(DateTimeOffset.UtcNow.AddDays(-1), DateTimeOffset.UtcNow.AddDays(1));
        var path = Path.Combine(_dir, fileName);
        File.WriteAllBytes(path, certificate.Export(X509ContentType.Pfx, Password));
        return path;
    }
}
