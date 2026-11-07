using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>Prompt Variables (ADR 0019): which ones a send asks for, and how answers are used.</summary>
public class PromptVariablesTests
{
    private static ApiRequest Request(string url = "https://x.test/{{path}}", string body = "") =>
        new() { Url = url, Body = body };

    [Fact]
    public void A_prompt_variable_used_by_the_request_is_found_with_its_stored_value()
    {
        var found = VariableResolver.FindPromptVariables(Request(), [new("path", "users", IsPrompt: true)]);

        Assert.Equal([new PromptVariable("path", "users")], found);
    }

    [Fact]
    public void Variables_that_are_not_marked_prompt_are_not_found()
    {
        var found = VariableResolver.FindPromptVariables(Request(), [new("path", "users")]);

        Assert.Empty(found);
    }

    [Fact]
    public void A_prompt_variable_the_request_does_not_use_is_not_found()
    {
        var found = VariableResolver.FindPromptVariables(Request(), [new("otp", "", IsPrompt: true)]);

        Assert.Empty(found);
    }

    [Fact]
    public void A_prompt_variable_used_through_another_variable_is_found()
    {
        var variables = new[]
        {
            new KeyValue("path", "users/{{id}}"),
            new KeyValue("id", "", IsPrompt: true),
        };

        var found = VariableResolver.FindPromptVariables(Request(), variables);

        Assert.Equal([new PromptVariable("id", "")], found);
    }

    [Fact]
    public void A_higher_scope_variable_of_the_same_name_replaces_a_prompt_one()
    {
        // The Environment's plain value wins over the Collection's prompt, so nothing is asked.
        KeyValue[] variables = [new("path", "", IsPrompt: true), new("path", "orders")];

        Assert.Empty(VariableResolver.FindPromptVariables(Request(), variables));
    }

    [Fact]
    public void A_disabled_prompt_variable_is_not_found()
    {
        var found = VariableResolver.FindPromptVariables(Request(), [new("path", "users", Enabled: false, IsPrompt: true)]);

        Assert.Empty(found);
    }

    [Fact]
    public void Without_an_answer_a_prompt_variable_with_no_value_stays_unresolved()
    {
        // This is the case a CI run hits: nothing can answer, so the name is reported and the request isn't sent.
        var resolved = VariableResolver.Resolve(Request(), [new("path", "", IsPrompt: true)]).Sent;

        Assert.Equal(["path"], VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void Without_an_answer_a_prompt_variable_with_a_stored_value_uses_it()
    {
        var resolved = VariableResolver.Apply(Request(), [new("path", "users", IsPrompt: true)]);

        Assert.Equal("https://x.test/users", resolved.Url);
    }

    [Fact]
    public void An_answer_is_sent_in_place_of_the_stored_value()
    {
        var answers = new Dictionary<string, string> { ["path"] = "orders" };

        var resolved = VariableResolver.Resolve(Request(), [new("path", "users", IsPrompt: true)], answers);

        Assert.Equal("https://x.test/orders", resolved.Sent.Url);
    }

    [Fact]
    public void An_answer_stays_out_of_History()
    {
        var answers = new Dictionary<string, string> { ["otp"] = "483920" };
        var request = Request(url: "https://x.test/verify", body: """{"code":"{{otp}}"}""");

        var resolution = VariableResolver.Resolve(request, [new("otp", "", IsPrompt: true)], answers);

        Assert.Contains("483920", resolution.Sent.Body);
        Assert.DoesNotContain("483920", resolution.History.Body);
        Assert.Equal("""{"code":"{{otp}}"}""", resolution.History.Body);
    }

    [Fact]
    public void An_empty_answer_leaves_the_variable_unresolved_rather_than_sending_blank()
    {
        var answers = new Dictionary<string, string> { ["path"] = "" };

        var resolved = VariableResolver.Resolve(Request(), [new("path", "users", IsPrompt: true)], answers).Sent;

        Assert.Equal(["path"], VariableResolver.FindPlaceholders(resolved));
    }

    [Fact]
    public void Answers_do_not_change_the_stored_variable()
    {
        var variables = new[] { new KeyValue("path", "users", IsPrompt: true) };

        VariableResolver.Resolve(Request(), variables, new Dictionary<string, string> { ["path"] = "orders" });

        Assert.Equal("users", variables[0].Value);
    }
}
