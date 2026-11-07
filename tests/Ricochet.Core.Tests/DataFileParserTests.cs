using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class DataFileParserTests
{
    private static IReadOnlyList<IReadOnlyList<KeyValue>> Csv(string text) => DataFileParser.Parse(text, DataFileFormat.Csv);

    private static IReadOnlyList<IReadOnlyList<KeyValue>> Json(string text) => DataFileParser.Parse(text, DataFileFormat.Json);

    private static string[] Pairs(IEnumerable<KeyValue> row) => row.Select(kv => $"{kv.Key}={kv.Value}").ToArray();

    [Fact]
    public void Csv_header_names_the_variables_and_each_line_is_a_row()
    {
        var rows = Csv("name,region\nann,eu\nbob,us\n");

        Assert.Equal(2, rows.Count);
        Assert.Equal(["name=ann", "region=eu"], Pairs(rows[0]));
        Assert.Equal(["name=bob", "region=us"], Pairs(rows[1]));
    }

    [Fact]
    public void Csv_quoted_fields_keep_commas_doubled_quotes_and_line_breaks()
    {
        var rows = Csv("name,note\r\n\"Ann, Jr.\",\"say \"\"hi\"\"\nnext\"\r\n");

        Assert.Equal(["name=Ann, Jr.", "note=say \"hi\"\nnext"], Pairs(rows[0]));
    }

    [Fact]
    public void Csv_blank_lines_and_comma_only_lines_are_skipped()
    {
        var rows = Csv("name,region\n\nann,eu\n  \n,\nbob,us\n\n");

        Assert.Equal(2, rows.Count);
        Assert.Equal("ann", rows[0][0].Value);
        Assert.Equal("bob", rows[1][0].Value);
    }

    [Fact]
    public void Csv_byte_order_mark_and_spaces_around_header_names_are_ignored()
    {
        var rows = Csv("﻿ name , region \nann,eu");

        Assert.Equal(["name=ann", "region=eu"], Pairs(rows[0]));
    }

    [Fact]
    public void Csv_a_line_with_the_wrong_number_of_values_names_the_line()
    {
        var error = Assert.Throws<RunSetupException>(() => Csv("name,region\nann,eu\nbob\n"));

        Assert.Contains("Line 3", error.Message);
    }

    [Fact]
    public void Csv_with_an_unclosed_quote_names_the_line_it_started_on()
    {
        var error = Assert.Throws<RunSetupException>(() => Csv("name\n\"ann\n"));

        Assert.Contains("line 2", error.Message);
    }

    [Fact]
    public void Csv_with_a_repeated_header_name_is_refused()
    {
        var error = Assert.Throws<RunSetupException>(() => Csv("name,name\na,b\n"));

        Assert.Contains("name", error.Message);
    }

    [Fact]
    public void Csv_with_no_header_is_refused()
    {
        Assert.Throws<RunSetupException>(() => Csv(""));
        Assert.Throws<RunSetupException>(() => Csv("\n\n"));
    }

    [Fact]
    public void Csv_header_with_no_name_is_refused()
    {
        Assert.Throws<RunSetupException>(() => Csv("name,\na,b\n"));
    }

    [Fact]
    public void Csv_with_only_a_header_has_no_rows()
    {
        Assert.Empty(Csv("name,region\n"));
    }

    [Fact]
    public void Json_array_of_objects_gives_one_row_per_object_with_values_as_text()
    {
        var rows = Json("""[{"name":"ann","age":32,"active":true,"score":1.50,"note":null,"tags":{"a":1}}]""");

        Assert.Equal(["name=ann", "age=32", "active=true", "score=1.50", "note=", "tags={\"a\":1}"], Pairs(rows[0]));
    }

    [Fact]
    public void Json_empty_objects_are_skipped()
    {
        var rows = Json("""[{"name":"ann"}, {}, {"name":"bob"}]""");

        Assert.Equal(2, rows.Count);
        Assert.Equal("bob", rows[1][0].Value);
    }

    [Fact]
    public void Json_that_is_not_valid_is_refused_with_the_reason()
    {
        var error = Assert.Throws<RunSetupException>(() => Json("""[{"name": "ann" """));

        Assert.Contains("not valid JSON", error.Message);
    }

    [Fact]
    public void Json_that_is_not_an_array_is_refused()
    {
        Assert.Throws<RunSetupException>(() => Json("""{"name":"ann"}"""));
    }

    [Fact]
    public void Json_row_that_is_not_an_object_names_the_row()
    {
        var error = Assert.Throws<RunSetupException>(() => Json("""[{"name":"ann"}, "bob"]"""));

        Assert.Contains("Row 2", error.Message);
    }

    [Theory]
    [InlineData("users.csv", DataFileFormat.Csv)]
    [InlineData("USERS.JSON", DataFileFormat.Json)]
    public void Format_is_taken_from_the_file_extension(string fileName, DataFileFormat expected)
    {
        Assert.Equal(expected, DataFileParser.FormatFor(fileName));
    }

    [Fact]
    public void Other_file_extensions_are_refused()
    {
        Assert.Throws<RunSetupException>(() => DataFileParser.FormatFor("users.xlsx"));
    }
}
