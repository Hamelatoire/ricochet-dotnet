using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>
/// Edge cases of placeholder resolution (ADR 0002 for precedence, ADR 0008 for built-in functions).
/// These pin current behaviour; they change no code.
/// </summary>
public class VariableResolutionEdgeCasesTests
{
    private static ApiRequest Request(string url = "http://x.test/") => new() { Url = url };

    // ----- A variable whose value contains another placeholder -----

    [Fact]
    public void A_variable_whose_value_names_another_variable_is_resolved_through_it()
    {
        var resolution = VariableResolver.Resolve(
            Request("http://x.test/{{outer}}"),
            [new KeyValue("outer", "{{inner}}/tail"), new KeyValue("inner", "abc")]);

        Assert.Equal("http://x.test/abc/tail", resolution.Sent.Url);
        Assert.Empty(VariableResolver.FindPlaceholders(resolution.Sent));
    }

    [Fact]
    public void Chained_variables_resolve_to_the_end_of_the_chain()
    {
        var resolved = VariableResolver.Apply(
            Request("http://x.test/{{a}}"),
            [new KeyValue("a", "{{b}}"), new KeyValue("b", "{{c}}"), new KeyValue("c", "end")]);

        Assert.Equal("http://x.test/end", resolved.Url);
    }

    [Fact]
    public void A_nested_placeholder_that_names_nothing_is_reported_as_missing()
    {
        var resolved = VariableResolver.Apply(
            Request("http://x.test/{{outer}}"),
            [new KeyValue("outer", "{{nowhere}}")]);

        Assert.Equal(["nowhere"], VariableResolver.FindPlaceholders(resolved));
    }

    // ----- A disabled variable never overrides a lower scope -----

    [Fact]
    public void A_disabled_variable_in_a_higher_scope_does_not_override_a_lower_scope()
    {
        var collection = new[] { new KeyValue("host", "collection.test") };
        var environment = new[] { new KeyValue("host", "staging.test", Enabled: false) };

        var variables = VariableResolver.Layer(collection, environment);
        var resolution = VariableResolver.Resolve(Request("http://{{host}}/"), variables);

        Assert.Equal("http://collection.test/", resolution.Sent.Url);
    }

    [Fact]
    public void A_disabled_variable_listed_before_an_enabled_one_does_not_hide_it()
    {
        var resolved = VariableResolver.Apply(
            Request("http://{{host}}/"),
            [new KeyValue("host", "off.test", Enabled: false), new KeyValue("host", "on.test")]);

        Assert.Equal("http://on.test/", resolved.Url);
    }

    [Fact]
    public void A_disabled_variable_with_no_lower_scope_value_is_reported_as_missing()
    {
        var resolved = VariableResolver.Apply(
            Request("http://{{host}}/"),
            [new KeyValue("host", "off.test", Enabled: false)]);

        Assert.Equal(["host"], VariableResolver.FindPlaceholders(resolved));
    }

    // ----- Placeholders that stay unresolved and are reported -----

    [Fact]
    public void A_name_with_inner_spaces_stays_unresolved_and_is_reported_with_the_spaces()
    {
        var resolved = VariableResolver.Apply(Request("http://x.test/{{first name}}"), []);

        Assert.Equal("http://x.test/{{first name}}", resolved.Url);
        Assert.Equal(["first name"], VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void Spaces_just_inside_the_braces_are_trimmed_and_the_variable_resolves()
    {
        var resolved = VariableResolver.Apply(Request("http://x.test/{{  user  }}"), [new KeyValue("user", "ada")]);

        Assert.Equal("http://x.test/ada", resolved.Url);
    }

    // Bug: an unclosed placeholder is left as written but is not reported, so the app and CLI do not warn about it.
    // Expected behaviour is that it is reported, as an unresolved placeholder with a missing closing brace.
    [Fact(Skip = "Known bug: an unclosed placeholder stays as written but FindPlaceholders does not report it. See the PR description.")]
    public void An_unclosed_placeholder_stays_unresolved_and_is_reported()
    {
        var resolved = VariableResolver.Apply(Request("http://x.test/{{user"), [new KeyValue("user", "ada")]);

        Assert.Equal("http://x.test/{{user", resolved.Url);
        Assert.NotEmpty(VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void An_unclosed_placeholder_is_left_exactly_as_written()
    {
        var resolved = VariableResolver.Apply(Request("http://x.test/{{user"), [new KeyValue("user", "ada")]);

        Assert.Equal("http://x.test/{{user", resolved.Url);
    }

    // ----- Built-in functions in headers and bodies -----

    [Fact]
    public void A_built_in_function_in_a_header_value_is_replaced_with_a_fresh_guid()
    {
        var resolved = VariableResolver.Apply(
            Request() with { Headers = [new("X-Request-Id", "{{uuid.v4()}}")] },
            []);

        var value = resolved.Headers.Single().Value;
        Assert.True(Guid.TryParse(value, out _));
        Assert.Empty(VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void A_built_in_function_in_a_body_is_replaced()
    {
        var resolved = VariableResolver.Apply(
            Request() with { Body = """{"id":"{{uuid.v4()}}"}""" },
            []);

        var id = System.Text.Json.JsonDocument.Parse(resolved.Body).RootElement.GetProperty("id").GetString();
        Assert.True(Guid.TryParse(id, out _));
    }

    [Fact]
    public void A_built_in_function_outside_a_JSON_string_in_a_body_is_replaced()
    {
        var resolved = VariableResolver.Apply(
            Request() with { Body = """{"count":{{random.int(5, 5)}}}""" },
            []);

        Assert.Equal("""{"count":5}""", resolved.Body);
    }

    [Fact]
    public void Two_uses_of_a_variable_holding_a_function_in_one_request_resolve_to_the_same_value()
    {
        var resolution = VariableResolver.Resolve(
            Request("http://x.test/{{rid}}") with
            {
                Method = "POST",
                Headers = [new("X-Request-Id", "{{rid}}")],
                Body = """{"id":"{{rid}}"}""",
            },
            [new KeyValue("rid", "{{uuid.v4()}}")]);

        var fromUrl = resolution.Sent.Url.Replace("http://x.test/", "");
        var fromHeader = resolution.Sent.Headers.Single().Value;
        var fromBody = System.Text.Json.JsonDocument.Parse(resolution.Sent.Body).RootElement.GetProperty("id").GetString();

        Assert.True(Guid.TryParse(fromUrl, out _));
        Assert.Equal(fromUrl, fromHeader);
        Assert.Equal(fromUrl, fromBody);
    }

    [Fact]
    public void Two_direct_uses_of_a_function_in_one_request_are_each_fresh()
    {
        var resolved = VariableResolver.Apply(
            Request("http://x.test/{{uuid.v4()}}") with { Headers = [new("X-Id", "{{uuid.v4()}}")] },
            []);

        var fromUrl = resolved.Url.Replace("http://x.test/", "");
        var fromHeader = resolved.Headers.Single().Value;
        Assert.NotEqual(fromUrl, fromHeader);
    }
}
