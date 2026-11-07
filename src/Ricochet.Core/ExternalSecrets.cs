using System.ComponentModel;
using System.Diagnostics;

namespace Ricochet.Core;

/// <summary>An external Secret could not be read. The message never contains the value, so it is safe to show.</summary>
public sealed class ExternalSecretException(string message, Exception? inner = null) : Exception(message, inner);

public enum ExternalSecretKind
{
    EnvironmentVariable,
    Command,
}

/// <summary>Where an external Secret's value comes from. See docs/adr/0025-read-secrets-from-external-secret-managers.md.</summary>
public sealed record ExternalSecretSource(ExternalSecretKind Kind, string Target);

/// <summary>
/// A Variable's value can be a reference instead of the Secret itself. A reference holds no secret, so it can be
/// saved in a Workspace file: <c>secret-ref:env:NAME</c> reads an environment variable, and
/// <c>secret-ref:command:LINE</c> runs a command and uses what it prints.
/// Commands must not contain secrets themselves, because the line is saved in the Workspace.
/// </summary>
public static class ExternalSecretReference
{
    public const string Prefix = "secret-ref:";
    private const string EnvironmentKind = "env:";
    private const string CommandKind = "command:";

    public static bool IsReference(string value) => value.StartsWith(Prefix, StringComparison.Ordinal);

    /// <exception cref="ExternalSecretException">The value is not a valid reference.</exception>
    public static ExternalSecretSource Parse(string value)
    {
        if (!IsReference(value))
            throw new ExternalSecretException("This is not a secret reference.");

        var body = value[Prefix.Length..];
        if (body.StartsWith(EnvironmentKind, StringComparison.Ordinal))
        {
            var name = body[EnvironmentKind.Length..].Trim();
            if (name.Length == 0 || name.Contains('=') || name.Any(c => char.IsWhiteSpace(c)))
                throw new ExternalSecretException("The environment variable name in a secret reference is not valid.");
            return new ExternalSecretSource(ExternalSecretKind.EnvironmentVariable, name);
        }

        if (body.StartsWith(CommandKind, StringComparison.Ordinal))
        {
            var line = body[CommandKind.Length..].Trim();
            if (line.Length == 0)
                throw new ExternalSecretException("The command in a secret reference is empty.");
            return new ExternalSecretSource(ExternalSecretKind.Command, line);
        }

        throw new ExternalSecretException($"A secret reference must start with {Prefix}{EnvironmentKind} or {Prefix}{CommandKind}.");
    }

    public static string ForEnvironmentVariable(string name) => Prefix + EnvironmentKind + name;

    public static string ForCommand(string commandLine) => Prefix + CommandKind + commandLine;
}

public sealed record CommandOutput(int ExitCode, string StandardOutput);

/// <summary>Runs a command line for a secret reference. Replaced in tests, so no real secret store is needed.</summary>
public interface ICommandRunner
{
    Task<CommandOutput> RunAsync(string commandLine, CancellationToken cancellationToken);
}

/// <summary>Runs the command through cmd.exe on Windows and /bin/sh elsewhere, with no input and a time limit.</summary>
public sealed class ProcessCommandRunner(TimeSpan? timeout = null) : ICommandRunner
{
    public static readonly TimeSpan DefaultTimeout = TimeSpan.FromSeconds(30);

    private readonly TimeSpan _timeout = timeout ?? DefaultTimeout;

    public async Task<CommandOutput> RunAsync(string commandLine, CancellationToken cancellationToken)
    {
        var info = new ProcessStartInfo
        {
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true,
        };
        if (OperatingSystem.IsWindows())
        {
            info.FileName = "cmd.exe";
            info.Arguments = $"/d /s /c \"{commandLine}\"";
        }
        else
        {
            info.FileName = "/bin/sh";
            info.ArgumentList.Add("-c");
            info.ArgumentList.Add(commandLine);
        }

        Process process;
        try
        {
            process = Process.Start(info) ?? throw new ExternalSecretException("The command for a secret reference could not be started.");
        }
        catch (Win32Exception ex)
        {
            throw new ExternalSecretException("The command for a secret reference could not be started.", ex);
        }

        using (process)
        {
            process.StandardInput.Close();
            var stdout = process.StandardOutput.ReadToEndAsync(cancellationToken);
            // Error output is drained so the process cannot block, and is never shown: it may echo a value.
            var stderr = process.StandardError.ReadToEndAsync(cancellationToken);

            using var limit = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            limit.CancelAfter(_timeout);
            try
            {
                await process.WaitForExitAsync(limit.Token);
            }
            catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
            {
                try { process.Kill(entireProcessTree: true); }
                catch (InvalidOperationException) { }
                throw new ExternalSecretException("The command for a secret reference did not finish in time.");
            }

            var output = await stdout;
            _ = await stderr;
            return new CommandOutput(process.ExitCode, output);
        }
    }
}

/// <summary>
/// Reads a Secret's value from an external source, on demand. It only reads: nothing is saved, and the caller
/// must mask the returned value wherever it is shown, as the CLI does for stored Secrets.
/// </summary>
public sealed class ExternalSecretResolver(Func<string, string?>? environment = null, ICommandRunner? commands = null)
{
    private readonly Func<string, string?> _environment = environment ?? Environment.GetEnvironmentVariable;
    private readonly ICommandRunner _commands = commands ?? new ProcessCommandRunner();

    /// <exception cref="ExternalSecretException">The reference is invalid, or the source gave no value.</exception>
    public async Task<string> ResolveAsync(string reference, CancellationToken cancellationToken = default)
    {
        var source = ExternalSecretReference.Parse(reference);
        return source.Kind switch
        {
            ExternalSecretKind.EnvironmentVariable => FromEnvironment(source.Target),
            ExternalSecretKind.Command => await FromCommandAsync(source.Target, cancellationToken),
            _ => throw new ExternalSecretException("Unknown secret source."),
        };
    }

    private string FromEnvironment(string name)
    {
        var value = _environment(name);
        if (string.IsNullOrEmpty(value))
            throw new ExternalSecretException($"Environment variable {name} is not set, so its Secret has no value on this machine.");
        return value;
    }

    private async Task<string> FromCommandAsync(string commandLine, CancellationToken cancellationToken)
    {
        var result = await _commands.RunAsync(commandLine, cancellationToken);
        if (result.ExitCode != 0)
            throw new ExternalSecretException($"The command for a secret reference exited with code {result.ExitCode}. Run it yourself to see its error.");

        var value = result.StandardOutput.TrimEnd('\r', '\n');
        if (value.Length == 0)
            throw new ExternalSecretException("The command for a secret reference printed no value.");
        return value;
    }
}
