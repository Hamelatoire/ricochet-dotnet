using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class PostmanTestTranslatorTests
{
    private const string V21 = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

    private static bool Translate(string script, out List<RequestAssertion> assertions) =>
        PostmanTestTranslator.TryTranslate(script, out assertions, out _);

    private static string NotTranslated(string script)
    {
        Assert.False(PostmanTestTranslator.TryTranslate(script, out var assertions, out var reason));
        Assert.Empty(assertions);
        return reason!;
    }

    private static RequestAssertion Single(string script)
    {
        Assert.True(Translate(script, out var assertions));
        return Assert.Single(assertions);
    }

    public static TheoryData<string, string, string, string, string> RecognisedChecks => new()
    {
        { "pm.response.to.have.status(200);", AssertionSources.Status, "", AssertionOps.Equal, "200" },
        { "pm.expect(pm.response.code).to.eql(201);", AssertionSources.Status, "", AssertionOps.Equal, "201" },
        { "pm.expect(pm.response.code).to.equal(204);", AssertionSources.Status, "", AssertionOps.Equal, "204" },
        { "pm.expect(pm.response.code).to.be.equal(404);", AssertionSources.Status, "", AssertionOps.Equal, "404" },
        { "pm.expect(pm.response.code === 200).to.be.true;", AssertionSources.Status, "", AssertionOps.Equal, "200" },
        { "pm.expect(pm.response.responseTime).to.be.below(300);", AssertionSources.Time, "", AssertionOps.LessThan, "300" },
        { "pm.expect(pm.response.responseTime).to.be.lessThan(250);", AssertionSources.Time, "", AssertionOps.LessThan, "250" },
        { "pm.response.to.have.header(\"X-Trace\");", AssertionSources.Header, "X-Trace", AssertionOps.Exists, "" },
        { "pm.response.to.have.header('Content-Type', 'application/json');", AssertionSources.Header, "Content-Type", AssertionOps.Equal, "application/json" },
        { "pm.expect(pm.response.json().name).to.eql(\"Widget\");", AssertionSources.Json, "name", AssertionOps.Equal, "Widget" },
        { "pm.expect(pm.response.json().items[0].id).to.eql(1);", AssertionSources.Json, "items[0].id", AssertionOps.Equal, "1" },
        { "pm.expect(pm.response.json().price).to.equal(19.5);", AssertionSources.Json, "price", AssertionOps.Equal, "19.5" },
        { "pm.expect(pm.response.json().items.length).to.eql(3);", AssertionSources.Json, "items.length", AssertionOps.Equal, "3" },
    };

    [Theory]
    [MemberData(nameof(RecognisedChecks))]
    public void Each_recognised_check_becomes_one_assertion(string check, string source, string path, string op, string expected)
    {
        var assertion = Single(check);
        Assert.Equal(source, assertion.Source);
        Assert.Equal(path, assertion.Path);
        Assert.Equal(op, assertion.Op);
        Assert.Equal(expected, assertion.Expected);
        Assert.True(assertion.Enabled);
    }

    [Fact]
    public void A_check_inside_pm_test_is_translated_for_function_and_arrow_forms()
    {
        var assertions = Single("""
            pm.test("Status is 200", function () {
                pm.response.to.have.status(200);
            });
            """);
        Assert.Equal("200", assertions.Expected);

        var arrow = Single("""
            pm.test('Fast', () => {
                pm.expect(pm.response.responseTime).to.be.below(500);
            });
            """);
        Assert.Equal("500", arrow.Expected);
    }

    [Fact]
    public void Several_checks_in_one_script_become_several_assertions_in_order()
    {
        Assert.True(Translate("""
            pm.test("OK", function () {
                pm.response.to.have.status(200);
                pm.response.to.have.header("X-Trace");
            });
            // a comment is fine
            const data = pm.response.json();
            pm.expect(data.name).to.eql("Widget");
            """, out var assertions));

        Assert.Equal([AssertionSources.Status, AssertionSources.Header, AssertionSources.Json], assertions.Select(a => a.Source));
        Assert.Equal("name", assertions[2].Path);
    }

    [Fact]
    public void A_json_variable_declared_from_pm_response_json_is_followed()
    {
        var assertion = Single("""
            const jsonData = pm.response.json();
            pm.expect(jsonData.items[0].name).to.eql("Widget");
            """);
        Assert.Equal("items[0].name", assertion.Path);
    }

    [Theory]
    [InlineData("pm.response.to.have.status(200);\npm.environment.set(\"token\", pm.response.json().token);")]
    [InlineData("if (pm.response.code === 200) { pm.response.to.have.status(200); }")]
    [InlineData("pm.response.to.have.status(200) && pm.response.to.have.header(\"X\");")]
    [InlineData("pm.expect(pm.response.json().id).to.exist;")]
    [InlineData("pm.expect(pm.response.text()).to.include(\"ok\");")]
    [InlineData("pm.expect(pm.response.code).to.be.oneOf([200, 201]);")]
    [InlineData("pm.response.to.have.status(\"OK\");")]
    [InlineData("pm.expect(pm.response.json().id).to.eql(1, \"id\");")]
    [InlineData("var total = 0;")]
    [InlineData("/* block comment */ pm.response.to.have.status(200);")]
    public void Anything_else_is_not_translated(string script)
    {
        Assert.False(Translate(script, out var assertions));
        Assert.Empty(assertions);
    }

    [Fact]
    public void A_partly_recognised_script_is_not_translated_at_all_and_names_the_line()
    {
        const string script = """
            pm.test("Mixed", function () {
                pm.response.to.have.status(200);
                pm.environment.set("token", pm.response.json().token);
            });
            """;

        Assert.Contains("pm.environment.set", NotTranslated(script));
    }

    [Fact]
    public void A_test_that_is_never_closed_is_not_translated()
    {
        Assert.Contains("closing", NotTranslated("""
            pm.test("Open", function () {
                pm.response.to.have.status(200);
            """));
    }

    [Fact]
    public void A_json_check_on_another_variable_is_not_translated()
    {
        var reason = NotTranslated("""
            const other = pm.response.json();
            const data = pm.response.json();
            pm.expect(other.name).to.eql("x");
            """);
        Assert.Equal("pm.expect(other.name).to.eql(\"x\");", reason);
    }

    [Fact]
    public void A_script_with_no_checks_is_not_translated()
    {
        Assert.Contains("no checks", NotTranslated("// only a comment\n"));
        Assert.Contains("no checks", NotTranslated("pm.test(\"Empty\", function () {\n});"));
    }

    [Fact]
    public void Long_unrecognised_lines_are_shortened_in_the_reason()
    {
        var reason = NotTranslated("pm.expect(pm.response.json().a).to.eql(\"" + new string('x', 300) + "\") || other();");
        Assert.EndsWith("…", reason);
        Assert.True(reason.Length < 150);
    }

    [Fact]
    public void The_translated_status_check_passes_and_fails_as_postman_would()
    {
        var assertions = Single("pm.response.to.have.status(200);");
        var ok = new ApiResponse(200, "OK", [], Encoding.UTF8.GetBytes("{}"), "application/json", null, TimeSpan.Zero, false);
        var missing = ok with { StatusCode = 404 };

        Assert.True(AssertionEvaluator.Evaluate([assertions], ok).Single().Passed);
        Assert.False(AssertionEvaluator.Evaluate([assertions], missing).Single().Passed);
    }

    [Fact]
    public void The_translated_json_check_reads_the_response_body()
    {
        var assertion = Single("pm.expect(pm.response.json().items[0].name).to.eql(\"Widget\");");
        var response = new ApiResponse(200, "OK", [], Encoding.UTF8.GetBytes("""{ "items": [ { "name": "Widget" } ] }"""), "application/json", null, TimeSpan.Zero, false);

        Assert.True(AssertionEvaluator.Evaluate([assertion], response).Single().Passed);
    }

    [Fact]
    public void Importer_attaches_translated_checks_to_the_request_and_keeps_no_script()
    {
        var result = Import(CollectionJson("""
            { "name": "Get", "request": { "url": "u" }, "event": [ { "listen": "test", "script": { "exec": [
                "pm.test(\"OK\", function () {",
                "    pm.response.to.have.status(200);",
                "});"
            ] } } ] }
            """));

        var request = result.Collection!.Items.Single();
        Assert.Equal(AssertionSources.Status, request.Request!.Assertions!.Single().Source);
        Assert.Empty(request.Scripts);
        Assert.DoesNotContain(result.Issues, i => i.Category == ImportCategories.Scripts);
    }

    [Fact]
    public void Importer_reports_an_untranslated_request_test_with_the_first_line_it_could_not_translate()
    {
        var result = Import(CollectionJson("""
            { "name": "Get", "request": { "url": "u" }, "event": [ { "listen": "test", "script": { "exec": [
                "pm.response.to.have.status(200);",
                "var ids = [];"
            ] } } ] }
            """));

        var request = result.Collection!.Items.Single();
        Assert.Null(request.Request!.Assertions);
        Assert.Single(request.Scripts);

        var issue = Assert.Single(result.Issues, i => i.Category == ImportCategories.Scripts);
        Assert.Equal("Shop / Get", issue.Location);
        Assert.Equal("test, not translated: var ids = [];", issue.Detail);
    }

    [Fact]
    public void Importer_never_translates_prerequest_or_folder_scripts()
    {
        var result = Import(CollectionJson("""
            { "name": "Folder", "event": [ { "listen": "test", "script": { "exec": ["pm.response.to.have.status(200);"] } } ], "item": [
                { "name": "Pre", "request": { "url": "u" }, "event": [ { "listen": "prerequest", "script": { "exec": ["pm.response.to.have.status(200);"] } } ] }
            ] }
            """));

        var folder = result.Collection!.Items.Single();
        Assert.Single(folder.Scripts);
        Assert.Null(folder.Children.Single().Request!.Assertions);
        Assert.Single(folder.Children.Single().Scripts);
        Assert.Equal(2, result.Issues.Count(i => i.Category == ImportCategories.Scripts));
    }

    private static ImportResult Import(string json) => PostmanImporter.Import(json, "file.json");

    private static string CollectionJson(string items) =>
        $$"""{ "info": { "name": "Shop", "schema": "{{V21}}" }, "item": [{{items}}] }""";
}
