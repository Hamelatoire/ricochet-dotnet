using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class DotEnvTests
{
    [Fact]
    public void Reads_plain_quoted_and_exported_values()
    {
        var result = DotEnv.Parse("A=1\nB=\"two words\"\nexport C='three'\n");

        Assert.Equal(["A", "B", "C"], result.Variables.Select(v => v.Key));
        Assert.Equal(["1", "two words", "three"], result.Variables.Select(v => v.Value));
        Assert.Empty(result.SkippedLines);
    }

    [Fact]
    public void Blank_lines_and_comments_are_ignored()
    {
        var result = DotEnv.Parse("# a comment\n\n   \nTOKEN=abc\n");

        Assert.Equal(["TOKEN"], result.Variables.Select(v => v.Key));
        Assert.Empty(result.SkippedLines);
    }

    [Fact]
    public void A_value_may_contain_equals_signs()
    {
        var result = DotEnv.Parse("KEY=abc=def==\n");

        Assert.Equal("abc=def==", result.Variables.Single().Value);
    }

    [Fact]
    public void Lines_that_are_not_assignments_are_reported_by_number_without_their_text()
    {
        var result = DotEnv.Parse("OK=1\nnot an assignment\n1BAD=x\n=nokey\nFINE=2\n");

        Assert.Equal(["OK", "FINE"], result.Variables.Select(v => v.Key));
        Assert.Equal([2, 3, 4], result.SkippedLines);
    }

    [Fact]
    public void Windows_line_endings_are_read()
    {
        var result = DotEnv.Parse("A=1\r\nB=2\r\n");

        Assert.Equal(["1", "2"], result.Variables.Select(v => v.Value));
    }

    [Fact]
    public void A_missing_file_gives_no_variables_and_no_error()
    {
        var folder = Path.Combine(Path.GetTempPath(), "ricochet-dotenv-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder);
        try
        {
            var result = DotEnv.Read(folder);

            Assert.Empty(result.Variables);
            Assert.Empty(result.SkippedLines);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void The_file_is_read_from_the_workspace_folder()
    {
        var folder = Path.Combine(Path.GetTempPath(), "ricochet-dotenv-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder);
        try
        {
            File.WriteAllText(Path.Combine(folder, DotEnv.FileName), "TOKEN=from-file\n");

            Assert.Equal("from-file", DotEnv.Read(folder).Variables.Single().Value);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void Values_are_Secrets_so_the_app_hides_them_but_an_empty_value_stays_plain()
    {
        var result = DotEnv.Parse("TOKEN=abc\nEMPTY=\n");

        Assert.Equal([true, false], result.Variables.Select(v => v.IsSecret));
        Assert.Equal(["abc", ""], result.Variables.Select(v => v.Value));
    }

    [Fact]
    public void A_dotenv_value_is_masked_when_it_is_echoed_in_an_example()
    {
        var sent = new ApiRequest { Method = "GET", Url = "https://example.test/{{TOKEN}}" };
        var variables = VariableResolver.Layer(DotEnv.Parse("TOKEN=abcd1234\n").Variables);
        var response = new ApiResponse(200, "OK", [], "{\"token\":\"abcd1234\"}"u8.ToArray(), "application/json", null, TimeSpan.Zero, false);

        var example = ResponseExamples.Create("Echo", sent, variables, [], response);

        Assert.DoesNotContain("abcd1234", example.Body);
        Assert.Contains(SecretRules.HiddenValue, example.Body);
    }
}
