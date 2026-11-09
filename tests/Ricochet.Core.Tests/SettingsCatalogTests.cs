using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class SettingsCatalogTests
{
    [Fact]
    public void Every_setting_belongs_to_a_listed_section()
    {
        foreach (var item in SettingsCatalog.Items)
        {
            Assert.Contains(item.Section, SettingsCatalog.Sections);
        }
    }

    [Fact]
    public void Every_section_has_at_least_one_setting()
    {
        foreach (var section in SettingsCatalog.Sections)
        {
            Assert.Contains(SettingsCatalog.Items, item => item.Section == section);
        }
    }

    [Fact]
    public void Setting_keys_are_unique()
    {
        var keys = SettingsCatalog.Items.Select(item => item.Key).ToList();
        Assert.Equal(keys.Count, keys.Distinct().Count());
    }

    [Fact]
    public void A_blank_query_returns_every_setting()
    {
        Assert.Equal(SettingsCatalog.Items, SettingsCatalog.Search("   "));
        Assert.Equal(SettingsCatalog.Items, SettingsCatalog.Search(null));
    }

    [Fact]
    public void Search_finds_a_setting_by_help_text_in_another_section()
    {
        // "certificate" is in the name of one Network setting and in the help text of Requests' TLS setting.
        var keys = SettingsCatalog.Search("certificate").Select(item => item.Key);
        Assert.Contains("client-certificate-file", keys);
        Assert.Contains("verify-tls", keys);
    }

    [Fact]
    public void Search_ignores_case()
    {
        Assert.Contains(SettingsCatalog.Search("PROXY"), item => item.Key == "proxy-address");
    }

    [Fact]
    public void Every_word_must_match()
    {
        var keys = SettingsCatalog.Search("proxy timeout").Select(item => item.Key);
        Assert.Empty(keys);
    }

    [Fact]
    public void Search_with_no_match_returns_nothing()
    {
        Assert.Empty(SettingsCatalog.Search("kubernetes"));
    }
}
