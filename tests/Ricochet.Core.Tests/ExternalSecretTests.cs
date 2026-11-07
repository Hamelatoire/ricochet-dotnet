using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class ExternalSecretReferenceTests
{
    [Fact]
    public void Environment_reference_round_trips()
    {
        var source = ExternalSecretReference.Parse(ExternalSecretReference.ForEnvironmentVariable("API_TOKEN"));

        Assert.Equal(new ExternalSecretSource(ExternalSecretKind.EnvironmentVariable, "API_TOKEN"), source);
    }

    [Fact]
    public void Command_keeps_colons_and_spaces_in_the_line()
    {
        var line = "vault kv get -field=token secret/app:prod";

        var source = ExternalSecretReference.Parse(ExternalSecretReference.ForCommand(line));

        Assert.Equal(new ExternalSecretSource(ExternalSecretKind.Command, line), source);
    }

    [Theory]
    [InlineData("secret-ref:env:")]
    [InlineData("secret-ref:env:has space")]
    [InlineData("secret-ref:env:A=B")]
    [InlineData("secret-ref:command:   ")]
    [InlineData("secret-ref:file:/etc/passwd")]
    [InlineData("secret-ref:")]
    public void Invalid_references_are_rejected(string value)
    {
        Assert.Throws<ExternalSecretException>(() => ExternalSecretReference.Parse(value));
    }

    [Theory]
    [InlineData("plain value", false)]
    [InlineData("{{token}}", false)]
    [InlineData("secret-ref:env:X", true)]
    public void Only_values_with_the_prefix_are_references(string value, bool expected)
    {
        Assert.Equal(expected, ExternalSecretReference.IsReference(value));
    }
}

public class ExternalSecretResolverTests
{
    private sealed class FakeRunner(CommandOutput output) : ICommandRunner
    {
        public string? LastCommand { get; private set; }

        public Task<CommandOutput> RunAsync(string commandLine, CancellationToken cancellationToken)
        {
            LastCommand = commandLine;
            return Task.FromResult(output);
        }
    }

    private static Func<string, string?> Environment(params (string Name, string Value)[] values)
    {
        var map = values.ToDictionary(v => v.Name, v => v.Value);
        return name => map.TryGetValue(name, out var value) ? value : null;
    }

    [Fact]
    public async Task Reads_an_environment_variable()
    {
        var resolver = new ExternalSecretResolver(Environment(("API_TOKEN", "t-123")), new FakeRunner(new CommandOutput(0, "")));

        var value = await resolver.ResolveAsync(ExternalSecretReference.ForEnvironmentVariable("API_TOKEN"));

        Assert.Equal("t-123", value);
    }

    [Fact]
    public async Task Missing_environment_variable_names_the_variable_not_a_value()
    {
        var resolver = new ExternalSecretResolver(Environment(), new FakeRunner(new CommandOutput(0, "")));

        var error = await Assert.ThrowsAsync<ExternalSecretException>(() =>
            resolver.ResolveAsync(ExternalSecretReference.ForEnvironmentVariable("API_TOKEN")));

        Assert.Contains("API_TOKEN", error.Message);
    }

    [Fact]
    public async Task Empty_environment_variable_counts_as_no_value()
    {
        var resolver = new ExternalSecretResolver(Environment(("API_TOKEN", "")), new FakeRunner(new CommandOutput(0, "")));

        await Assert.ThrowsAsync<ExternalSecretException>(() =>
            resolver.ResolveAsync(ExternalSecretReference.ForEnvironmentVariable("API_TOKEN")));
    }

    [Fact]
    public async Task Command_output_is_used_without_its_final_line_break()
    {
        var runner = new FakeRunner(new CommandOutput(0, "s3cret\r\n"));
        var resolver = new ExternalSecretResolver(Environment(), runner);

        var value = await resolver.ResolveAsync(ExternalSecretReference.ForCommand("vault read token"));

        Assert.Equal("s3cret", value);
        Assert.Equal("vault read token", runner.LastCommand);
    }

    [Fact]
    public async Task Failed_command_throws_without_its_output()
    {
        var resolver = new ExternalSecretResolver(Environment(), new FakeRunner(new CommandOutput(3, "s3cret")));

        var error = await Assert.ThrowsAsync<ExternalSecretException>(() =>
            resolver.ResolveAsync(ExternalSecretReference.ForCommand("vault read token")));

        Assert.Contains("3", error.Message);
        Assert.DoesNotContain("s3cret", error.Message);
    }

    [Fact]
    public async Task Command_printing_nothing_throws()
    {
        var resolver = new ExternalSecretResolver(Environment(), new FakeRunner(new CommandOutput(0, "\n")));

        await Assert.ThrowsAsync<ExternalSecretException>(() =>
            resolver.ResolveAsync(ExternalSecretReference.ForCommand("vault read token")));
    }

    [Fact]
    public async Task Invalid_reference_is_rejected_before_anything_runs()
    {
        var runner = new FakeRunner(new CommandOutput(0, "x"));
        var resolver = new ExternalSecretResolver(Environment(), runner);

        await Assert.ThrowsAsync<ExternalSecretException>(() => resolver.ResolveAsync("secret-ref:bogus:x"));

        Assert.Null(runner.LastCommand);
    }
}

public class ExternalSecretWorkspaceTests
{
    [Fact]
    public void A_reference_variable_is_saved_as_written_and_no_value_enters_the_vault()
    {
        var reference = ExternalSecretReference.ForEnvironmentVariable("API_TOKEN");
        var workspace = new Workspace
        {
            Globals = [new KeyValue("token", reference)],
        };

        var (clean, secrets) = WorkspaceSecrets.Split(workspace);

        Assert.Equal(reference, clean.Globals[0].Value);
        Assert.Empty(secrets);
    }
}
