using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class PlaceholderChipsTests
{
    [Fact]
    public void Finds_each_placeholder_with_its_position_and_name()
    {
        var chips = PlaceholderChips.Find("https://{{ host }}/users/{{id}}", [new KeyValue("host", "api.test"), new KeyValue("id", "7")]);

        Assert.Equal(2, chips.Count);
        Assert.Equal(new Chip(8, 10, "host", false, true), chips[0]);
        Assert.Equal(new Chip(25, 6, "id", false, true), chips[1]);
    }

    [Fact]
    public void A_name_with_no_value_in_scope_does_not_resolve()
    {
        var chips = PlaceholderChips.Find("{{token}}", []);

        Assert.False(chips.Single().Resolves);
    }

    [Fact]
    public void Disabled_variables_and_secrets_without_a_value_do_not_resolve()
    {
        var variables = new[]
        {
            new KeyValue("off", "x", Enabled: false),
            new KeyValue("key", "", IsSecret: true),
        };

        var chips = PlaceholderChips.Find("{{off}} {{key}}", variables);

        Assert.False(chips[0].Resolves);
        Assert.False(chips[1].Resolves);
    }

    [Fact]
    public void Functions_are_chips_and_resolve_only_when_known()
    {
        var chips = PlaceholderChips.Find("{{uuid.v4()}} {{unknown.call()}} {{$guid}} {{$other}}", []);

        Assert.True(chips[0].IsFunction);
        Assert.True(chips[0].Resolves);
        Assert.True(chips[1].IsFunction);
        Assert.False(chips[1].Resolves);
        Assert.True(chips[2].IsFunction);
        Assert.True(chips[2].Resolves);
        Assert.False(chips[3].IsFunction);
        Assert.False(chips[3].Resolves);
    }

    [Fact]
    public void A_variable_named_like_a_function_takes_priority_as_the_send_does()
    {
        var chips = PlaceholderChips.Find("{{uuid.v4()}}", [new KeyValue("uuid.v4()", "fixed")]);

        Assert.False(chips.Single().IsFunction);
        Assert.True(chips.Single().Resolves);
    }

    [Fact]
    public void Tip_shows_a_value_but_never_a_Secret()
    {
        var variables = new[]
        {
            new KeyValue("host", "old.test"),
            new KeyValue("host", "api.test"),
            new KeyValue("password", "hunter2", IsSecret: true),
        };

        var chips = PlaceholderChips.Find("{{host}} {{password}} {{missing}}", variables);

        Assert.Equal("{{host}} = api.test", PlaceholderChips.Tip(chips[0], variables));
        Assert.Equal("{{password}}: Secret, hidden", PlaceholderChips.Tip(chips[1], variables));
        Assert.Equal("{{missing}}: no value", PlaceholderChips.Tip(chips[2], variables));
    }

    [Fact]
    public void Suggest_offers_variables_in_scope_then_functions_matching_what_is_typed()
    {
        var variables = new[]
        {
            new KeyValue("base_url", "http://x"),
            new KeyValue("Base_token", "t"),
            new KeyValue("off", "x", Enabled: false),
        };

        var all = PlaceholderChips.Suggest(variables, "");
        var bas = PlaceholderChips.Suggest(variables, "BA");
        var time = PlaceholderChips.Suggest(variables, "timestamp.");

        Assert.Equal(["base_url", "Base_token"], all.Where(s => !s.IsFunction).Select(s => s.Name));
        Assert.Contains(all, s => s.IsFunction && s.Name == "uuid.v4()");
        Assert.Equal(["base_url", "Base_token"], bas.Select(s => s.Name));
        Assert.All(time, s => Assert.StartsWith("timestamp.", s.Name));
        Assert.Equal(4, time.Count);
    }

    [Fact]
    public void Caret_helpers_find_the_chip_on_each_side_of_an_offset()
    {
        var chips = PlaceholderChips.Find("a{{b}}c", [new KeyValue("b", "1")]);

        Assert.Equal("b", PlaceholderChips.Covering(chips, 3)?.Name);
        Assert.Null(PlaceholderChips.Covering(chips, 1));
        Assert.Equal("b", PlaceholderChips.StartingAt(chips, 1)?.Name);
        Assert.Equal("b", PlaceholderChips.EndingAt(chips, 6)?.Name);
        Assert.Null(PlaceholderChips.EndingAt(chips, 5));
    }
}
