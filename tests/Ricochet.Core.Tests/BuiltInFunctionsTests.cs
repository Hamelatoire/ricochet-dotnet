using System.Globalization;
using System.Text.RegularExpressions;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class BuiltInFunctionsTests
{
    private static ApiRequest Request(string url = "http://x.test/", string body = "") => new() { Url = url, Body = body };

    [Fact]
    public void Uuid_is_a_fresh_guid_each_time()
    {
        var first = BuiltInFunctions.TryEvaluate("uuid.v4()");
        var second = BuiltInFunctions.TryEvaluate("uuid.v4()");

        Assert.True(Guid.TryParse(first, out _));
        Assert.NotEqual(first, second);
    }

    [Fact]
    public void Timestamps_are_current_and_in_the_stated_form()
    {
        var before = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        var unix = long.Parse(BuiltInFunctions.TryEvaluate("timestamp.unix()")!, CultureInfo.InvariantCulture);
        var after = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        Assert.InRange(unix, before, after);

        var millis = long.Parse(BuiltInFunctions.TryEvaluate("timestamp.unixMillis()")!, CultureInfo.InvariantCulture);
        Assert.InRange(millis / 1000, before, after);

        Assert.Matches(new Regex(@"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$"), BuiltInFunctions.TryEvaluate("timestamp.iso8601()")!);
        Assert.Matches(new Regex(@"^\d{4}-\d{2}-\d{2}$"), BuiltInFunctions.TryEvaluate("timestamp.format('yyyy-MM-dd')")!);
    }

    [Fact]
    public void Random_int_is_inclusive_at_both_ends()
    {
        Assert.Equal("7", BuiltInFunctions.TryEvaluate("random.int(7, 7)"));

        for (var i = 0; i < 200; i++)
        {
            var value = int.Parse(BuiltInFunctions.TryEvaluate("random.int(1, 3)")!, CultureInfo.InvariantCulture);
            Assert.InRange(value, 1, 3);
        }
    }

    [Theory]
    [InlineData("nope()")]
    [InlineData("uuid.v4")]
    [InlineData("uuid.v4(1)")]
    [InlineData("random.int(5, 2)")]
    [InlineData("random.int(a, 2)")]
    [InlineData("random.int(1, {{x}})")]
    [InlineData("timestamp.format()")]
    [InlineData("timestamp.format('yyyy")]
    [InlineData("timestamp.format('')")]
    public void Unknown_or_malformed_calls_are_not_evaluated(string name)
    {
        Assert.Null(BuiltInFunctions.TryEvaluate(name));
    }

    [Fact]
    public void Postman_names_act_as_the_matching_functions()
    {
        Assert.True(Guid.TryParse(BuiltInFunctions.TryEvaluate("$guid"), out _));
        Assert.True(BuiltInFunctions.IsPostmanAlias("$randomInt"));
        Assert.False(BuiltInFunctions.IsPostmanAlias("$isbn"));

        var randomInt = int.Parse(BuiltInFunctions.TryEvaluate("$randomInt")!, CultureInfo.InvariantCulture);
        Assert.InRange(randomInt, 0, 1000);
    }

    [Fact]
    public void Each_use_of_a_function_gets_its_own_value()
    {
        var resolved = VariableResolver.Apply(Request(body: "[{{uuid.v4()}}, {{uuid.v4()}}]"), []);

        var ids = resolved.Body.Trim('[', ']').Split(", ");
        Assert.Equal(2, ids.Length);
        Assert.NotEqual(ids[0], ids[1]);
    }

    [Fact]
    public void A_variable_keeps_one_value_for_the_whole_send()
    {
        var resolved = VariableResolver.Apply(
            Request(url: "http://x.test/{{client_id}}/{{client_id}}"),
            [new KeyValue("client_id", "{{uuid.v4()}}")]);

        var parts = resolved.Url.Split('/');
        Assert.Equal(parts[3], parts[4]);
        Assert.True(Guid.TryParse(parts[3], out _));
    }

    [Fact]
    public void A_variable_gets_a_new_value_on_the_next_send()
    {
        var variables = new[] { new KeyValue("client_id", "{{uuid.v4()}}") };
        var first = VariableResolver.Apply(Request(url: "http://x.test/{{client_id}}"), variables).Url;
        var second = VariableResolver.Apply(Request(url: "http://x.test/{{client_id}}"), variables).Url;

        Assert.NotEqual(first, second);
    }

    [Fact]
    public void Assertions_and_captures_never_run_functions()
    {
        var request = Request() with
        {
            Assertions = [new RequestAssertion { Source = AssertionSources.Body, Path = "id", Expected = "{{uuid.v4()}}" }],
        };

        var resolved = VariableResolver.Apply(request, []);

        Assert.Equal("{{uuid.v4()}}", resolved.Assertions![0].Expected);
    }

    [Fact]
    public void An_unknown_function_is_reported_as_a_missing_value()
    {
        var resolved = VariableResolver.Apply(Request(url: "http://x.test/{{nope()}}"), []);

        Assert.Equal(["nope()"], VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void A_variable_that_refers_to_itself_stops_and_is_reported()
    {
        var resolved = VariableResolver.Apply(Request(url: "http://x.test/{{loop}}"), [new KeyValue("loop", "{{loop}}")]);

        Assert.Equal(["loop"], VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void Function_results_inside_a_JSON_string_are_escaped()
    {
        var resolved = VariableResolver.Apply(
            Request(body: """{"when":"{{timestamp.format('yyyy')}}"}"""), []);

        Assert.Matches(new Regex(@"^\{""when"":""\d{4}""\}$"), resolved.Body);
    }
}
