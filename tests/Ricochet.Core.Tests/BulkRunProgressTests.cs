using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class BulkRunProgressTests
{
    [Theory]
    [InlineData(0, 10, 0)]
    [InlineData(3, 10, 30)]
    [InlineData(9, 10, 90)]
    [InlineData(10, 10, 100)]
    [InlineData(1, 3, 33)]
    [InlineData(0, 0, 0)]
    public void Percent_is_whole_and_never_passes_100(int processed, int total, int expected) =>
        Assert.Equal(expected, BulkRunProgress.Percent(processed, total));

    [Fact]
    public void Percent_stays_at_99_until_the_last_id_is_done()
    {
        Assert.Equal(99, BulkRunProgress.Percent(99, 100));
        Assert.Equal(100, BulkRunProgress.Percent(100, 100));
    }

    [Fact]
    public void Describe_names_the_counts_and_the_percent()
    {
        Assert.Equal("3 of 10 processed (30%)", BulkRunProgress.Describe(3, 10));
        Assert.Equal("", BulkRunProgress.Describe(0, 0));
    }

    [Fact]
    public void Resume_starts_after_the_last_id_processed()
    {
        string[] ids = ["a", "b", "c", "d"];

        Assert.Equal(2, BulkRunner.ResumeIndex(ids, "b"));
        Assert.Equal(4, BulkRunner.ResumeIndex(ids, "d"));
    }

    [Fact]
    public void Resume_starts_from_the_first_id_when_there_is_no_last_id_or_it_is_gone()
    {
        string[] ids = ["a", "b"];

        Assert.Equal(0, BulkRunner.ResumeIndex(ids, null));
        Assert.Equal(0, BulkRunner.ResumeIndex(ids, "removed"));
    }
}
