using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class NameFilterTests
{
    [Fact]
    public void Empty_or_blank_query_matches_everything()
    {
        Assert.True(NameFilter.Matches("Anything", ""));
        Assert.True(NameFilter.Matches("Anything", "   "));
        Assert.True(NameFilter.Matches("Anything", null!));
    }

    [Fact]
    public void Matches_a_substring_ignoring_case_and_surrounding_spaces()
    {
        Assert.True(NameFilter.Matches("Get Users", "users"));
        Assert.True(NameFilter.Matches("Get Users", "  GET "));
        Assert.False(NameFilter.Matches("Get Users", "orders"));
    }

    [Fact]
    public void A_node_is_shown_when_a_child_is_shown_even_if_its_own_name_does_not_match()
    {
        Assert.True(NameFilter.Shows("Shop", "users", [false, true, false]));
        Assert.False(NameFilter.Shows("Shop", "users", [false, false]));
        Assert.False(NameFilter.Shows("Shop", "users", []));
    }

    [Fact]
    public void A_matching_name_is_shown_without_any_child()
    {
        Assert.True(NameFilter.Shows("Users", "users", []));
    }

    [Fact]
    public void Off_filter_shows_every_node()
    {
        Assert.True(NameFilter.Shows("Shop", "", []));
    }
}
