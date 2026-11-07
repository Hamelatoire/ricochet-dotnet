using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>ADR 0025: a Variable holding a secret reference is read just before sending, and only the send carries the value.</summary>
public class SecretReferenceSendingTests
{
    private const string Token = "s3cr3t-value-123";

    /// <summary>Records what was sent and answers 200, without any network.</summary>
    private sealed class FakeHandler : HttpMessageHandler
    {
        public List<HttpRequestMessage> Received { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Received.Add(request);
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("ok") });
        }
    }

    private sealed class FakeCommands(CommandOutput output) : ICommandRunner
    {
        public int Calls { get; private set; }

        public Task<CommandOutput> RunAsync(string commandLine, CancellationToken cancellationToken)
        {
            Calls++;
            return Task.FromResult(output);
        }
    }

    private static ExternalSecretResolver Environment(params (string Name, string Value)[] values) =>
        new(name => values.FirstOrDefault(v => v.Name == name).Value, new FakeCommands(new CommandOutput(0, "")));

    private static ApiRequest RequestUsing(string token) => new()
    {
        Name = "Me",
        Url = "http://api.test/me",
        Headers = [new KeyValue("Authorization", $"Bearer {{{{{token}}}}}")],
    };

    private static Workspace One(ApiRequest request, params KeyValue[] variables) => new()
    {
        Collections = [new Collection { Name = "Shop", Variables = variables, Items = [new CollectionItem { Name = request.Name, Request = request }] }],
    };

    private static RunOptions NoOverrides() => new();

    private static (CollectionRunner Runner, FakeHandler Handler) RunnerWith(ExternalSecretResolver secrets)
    {
        var handler = new FakeHandler();
        var sender = new RequestSender(new HttpClient(handler) { Timeout = Timeout.InfiniteTimeSpan });
        return (new CollectionRunner(sender, secrets), handler);
    }

    [Fact]
    public async Task The_request_sends_the_environment_value_of_a_reference()
    {
        var secrets = Environment(("API_TOKEN", Token));
        var (runner, handler) = RunnerWith(secrets);
        var request = RequestUsing("token");

        var report = await runner.RunAsync(One(request, new KeyValue("token", ExternalSecretReference.ForEnvironmentVariable("API_TOKEN"))),
            NoOverrides(), cancellationToken: TestContext.Current.CancellationToken);

        Assert.True(report.Passed);
        var sent = Assert.Single(handler.Received);
        Assert.Equal($"Bearer {Token}", sent.Headers.GetValues("Authorization").Single());
    }

    [Fact]
    public async Task The_request_sends_the_output_of_a_command_reference()
    {
        var commands = new FakeCommands(new CommandOutput(0, $"{Token}\r\n"));
        var handler = new FakeHandler();
        var runner = new CollectionRunner(new RequestSender(new HttpClient(handler) { Timeout = Timeout.InfiniteTimeSpan }), new ExternalSecretResolver(_ => null, commands));

        await runner.RunAsync(One(RequestUsing("token"), new KeyValue("token", ExternalSecretReference.ForCommand("vault read token"))),
            NoOverrides(), cancellationToken: TestContext.Current.CancellationToken);

        Assert.Equal(1, commands.Calls);
        Assert.Equal($"Bearer {Token}", Assert.Single(handler.Received).Headers.GetValues("Authorization").Single());
    }

    [Fact]
    public async Task The_saved_request_and_the_variables_keep_the_reference()
    {
        var reference = ExternalSecretReference.ForEnvironmentVariable("API_TOKEN");
        var request = RequestUsing("token");
        var variables = new[] { new KeyValue("token", reference) };

        var sendVariables = await VariableResolver.ResolveReferencesAsync(request, variables, Environment(("API_TOKEN", Token)),
            TestContext.Current.CancellationToken);
        var sent = VariableResolver.Apply(request, sendVariables);

        Assert.Equal($"Bearer {Token}", sent.Headers.Single().Value);
        Assert.Equal("Bearer {{token}}", request.Headers.Single().Value);
        Assert.Equal(reference, variables.Single().Value);
        // The value is marked as a Secret, so every place that masks Secrets also masks it.
        Assert.Contains(sendVariables, v => v.Key == "token" && v.IsSecret && v.Value == Token);
    }

    [Fact]
    public async Task A_missing_environment_variable_fails_before_anything_is_sent()
    {
        var (runner, handler) = RunnerWith(Environment());

        var report = await runner.RunAsync(One(RequestUsing("token"), new KeyValue("token", ExternalSecretReference.ForEnvironmentVariable("API_TOKEN"))),
            NoOverrides(), cancellationToken: TestContext.Current.CancellationToken);

        Assert.Empty(handler.Received);
        Assert.False(report.Passed);
        var error = Assert.Single(report.Results).Error!;
        Assert.Contains("token", error);
        Assert.Contains("API_TOKEN", error);
    }

    [Fact]
    public async Task A_failed_command_fails_before_anything_is_sent_and_never_shows_its_output()
    {
        var commands = new FakeCommands(new CommandOutput(3, Token));
        var handler = new FakeHandler();
        var runner = new CollectionRunner(new RequestSender(new HttpClient(handler) { Timeout = Timeout.InfiniteTimeSpan }), new ExternalSecretResolver(_ => null, commands));

        var report = await runner.RunAsync(One(RequestUsing("token"), new KeyValue("token", ExternalSecretReference.ForCommand("vault read token"))),
            NoOverrides(), cancellationToken: TestContext.Current.CancellationToken);

        Assert.Empty(handler.Received);
        var error = Assert.Single(report.Results).Error!;
        Assert.Contains("token", error);
        Assert.DoesNotContain(Token, error);
    }

    [Fact]
    public async Task A_reference_the_request_does_not_use_is_not_read()
    {
        var commands = new FakeCommands(new CommandOutput(1, ""));
        var handler = new FakeHandler();
        var runner = new CollectionRunner(new RequestSender(new HttpClient(handler) { Timeout = Timeout.InfiniteTimeSpan }), new ExternalSecretResolver(_ => null, commands));
        var request = new ApiRequest { Name = "Open", Url = "http://api.test/open" };

        var report = await runner.RunAsync(One(request, new KeyValue("unused", ExternalSecretReference.ForCommand("vault read unused"))),
            NoOverrides(), cancellationToken: TestContext.Current.CancellationToken);

        Assert.True(report.Passed);
        Assert.Equal(0, commands.Calls);
        Assert.Single(handler.Received);
    }

    [Fact]
    public async Task A_reference_reached_through_another_variable_is_read()
    {
        var request = RequestUsing("outer");
        var variables = new[]
        {
            new KeyValue("outer", "{{inner}}"),
            new KeyValue("inner", ExternalSecretReference.ForEnvironmentVariable("API_TOKEN")),
        };

        var sendVariables = await VariableResolver.ResolveReferencesAsync(request, variables, Environment(("API_TOKEN", Token)),
            TestContext.Current.CancellationToken);

        Assert.Equal($"Bearer {Token}", VariableResolver.Apply(request, sendVariables).Headers.Single().Value);
    }

    [Fact]
    public async Task A_disabled_reference_is_not_read_and_does_not_fail_the_request()
    {
        var commands = new FakeCommands(new CommandOutput(1, ""));
        var resolver = new ExternalSecretResolver(_ => null, commands);
        var request = RequestUsing("token");
        var variables = new[] { new KeyValue("token", ExternalSecretReference.ForCommand("vault read token"), Enabled: false) };

        var sendVariables = await VariableResolver.ResolveReferencesAsync(request, variables, resolver, TestContext.Current.CancellationToken);

        // A disabled Variable never wins, so its reference is neither read nor reported: the placeholder stays visible instead.
        Assert.Equal(0, commands.Calls);
        Assert.Equal(new[] { "token" }, VariableResolver.FindPlaceholders(VariableResolver.Apply(request, sendVariables)));
    }
}
