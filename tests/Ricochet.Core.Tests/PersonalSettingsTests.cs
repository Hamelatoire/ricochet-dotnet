using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class PersonalSettingsTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-settings").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Defaults_apply_when_nothing_is_saved()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();

        Assert.Equal(new PersonalSettings(), store.Settings);
        Assert.Equal(120, store.Settings.RequestTimeoutSeconds);
        Assert.True(store.Settings.VerifyTlsCertificates);
    }

    [Fact]
    public void Defaults_apply_to_state_written_before_settings_existed()
    {
        File.WriteAllText(Path.Combine(_dir, "state.json"), """{ "theme": "dark" }""");

        var store = new WorkspaceStore(_dir);
        store.Load();

        Assert.Equal("dark", store.Theme);
        Assert.Equal(new PersonalSettings(), store.Settings);
    }

    [Fact]
    public void Saved_settings_survive_a_restart_and_stay_out_of_the_workspace()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        store.SaveSettings(new PersonalSettings
        {
            RequestTimeoutSeconds = 30,
            FollowRedirects = false,
            VerifyTlsCertificates = false,
            MaxResponseMegabytes = 5,
            WrapResponse = false,
        });

        var reloaded = new WorkspaceStore(_dir);
        reloaded.Load();

        Assert.Equal(30, reloaded.Settings.RequestTimeoutSeconds);
        Assert.False(reloaded.Settings.FollowRedirects);
        Assert.False(reloaded.Settings.VerifyTlsCertificates);
        Assert.Equal(5, reloaded.Settings.MaxResponseMegabytes);
        Assert.False(reloaded.Settings.WrapResponse);
        var sharedFiles = Directory.GetFiles(reloaded.FolderPath, "*", SearchOption.AllDirectories).Select(File.ReadAllText);
        Assert.All(sharedFiles, text => Assert.DoesNotContain("requestTimeoutSeconds", text));
    }
}
