using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class CommandPaletteTests
{
    private static readonly string[] Names =
    [
        "Send request",
        "Save request",
        "New request",
        "Get hello",
        "Import Postman files",
    ];

    [Fact]
    public void An_empty_query_keeps_every_entry_in_order()
    {
        var result = CommandPalette.Filter(Names, "   ", n => n);

        Assert.Equal(Names, result);
    }

    [Fact]
    public void Matching_ignores_case_and_needs_every_word()
    {
        var result = CommandPalette.Filter(Names, "REQUEST hello", n => n);

        Assert.Empty(result);
        Assert.Equal(["Get hello"], CommandPalette.Filter(Names, "get HELLO", n => n));
    }

    [Fact]
    public void Names_that_start_with_the_first_word_come_first()
    {
        var result = CommandPalette.Filter(Names, "request", n => n);

        Assert.Equal(["Send request", "Save request", "New request"], result);
    }

    [Fact]
    public void A_name_that_starts_with_the_first_word_ranks_above_one_that_only_contains_it()
    {
        var result = CommandPalette.Filter(["Open a request", "Request list"], "request", n => n);

        Assert.Equal(["Request list", "Open a request"], result);
    }

    [Fact]
    public void A_query_with_no_match_gives_nothing()
    {
        Assert.Empty(CommandPalette.Filter(Names, "delete", n => n));
    }
}
