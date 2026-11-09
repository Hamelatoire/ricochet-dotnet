using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class KeyValueColumnsTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-kv-columns").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Share_inside_the_range_is_kept()
    {
        Assert.Equal(0.4, KeyValueColumns.Clamp(0.4, 600));
    }

    [Fact]
    public void Key_column_cannot_shrink_below_the_minimum_width()
    {
        // 80 of 600 is 13.3%: a smaller share is raised to that.
        Assert.Equal(80.0 / 600, KeyValueColumns.Clamp(0.01, 600), precision: 10);
    }

    [Fact]
    public void Value_column_cannot_shrink_below_the_minimum_width()
    {
        Assert.Equal(1 - 80.0 / 600, KeyValueColumns.Clamp(0.99, 600), precision: 10);
    }

    [Theory]
    [InlineData(double.NaN)]
    [InlineData(double.PositiveInfinity)]
    public void Non_finite_share_gives_the_default(double share)
    {
        Assert.Equal(KeyValueColumns.DefaultKeyShare, KeyValueColumns.Clamp(share, 600));
    }

    [Fact]
    public void Space_too_small_for_both_minimums_gives_the_default()
    {
        Assert.Equal(KeyValueColumns.DefaultKeyShare, KeyValueColumns.Clamp(0.5, 100));
    }

    [Theory]
    [InlineData(0.0, false)]
    [InlineData(1.0, false)]
    [InlineData(-0.2, false)]
    [InlineData(double.NaN, false)]
    [InlineData(0.5, true)]
    public void Only_proportions_between_zero_and_one_are_saveable(double share, bool expected)
    {
        Assert.Equal(expected, KeyValueColumns.IsSaveable(share));
    }

    [Fact]
    public void Saved_split_is_remembered_per_table_kind_after_a_restart()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        store.SaveKeyValueSplit("headers", 0.45);

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();

        Assert.Equal(0.45, reopened.KeyValueSplit("headers"));
        Assert.Null(reopened.KeyValueSplit("variables"));
    }

    [Fact]
    public void Unsaveable_split_is_not_stored()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        store.SaveKeyValueSplit("headers", 1.5);

        Assert.Null(store.KeyValueSplit("headers"));
    }

    [Fact]
    public void Saved_split_outside_the_range_is_ignored()
    {
        File.WriteAllText(Path.Combine(_dir, "state.json"), """{ "keyValueSplits": { "headers": 7 } }""");

        var store = new WorkspaceStore(_dir);
        store.Load();

        Assert.Null(store.KeyValueSplit("headers"));
    }
}
