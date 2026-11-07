using System.Net;
using System.Net.Sockets;
using System.Text;
using Avalonia;
using Avalonia.Headless;
using Ricochet.App;
using Ricochet.App.Services;
using Ricochet.Core;

// Tests run one at a time, because each one sets RICOCHET_DATA, which is shared by the whole process.
[assembly: Xunit.v3.ParallelizationAttribute(Mode = Xunit.Sdk.ParallelMode.None)]
// The headless host starts the app from this builder (see AvaloniaTestHost).
[assembly: AvaloniaTestApplication(typeof(Ricochet.App.Tests.TestAppBuilder))]

namespace Ricochet.App.Tests;

/// <summary>The app as the headless host starts it. Only the test assembly uses it, through the attribute below.</summary>
public static class TestAppBuilder
{
    public static AppBuilder BuildAvaloniaApp() => AppBuilder.Configure<App>().UseHeadless(new AvaloniaHeadlessPlatformOptions());
}

/// <summary>
/// Starts Avalonia without a display. The app's watcher posts to the UI dispatcher when files change, so the dispatcher
/// must exist even though no window is shown. Started on first use: the app's startup code reads RICOCHET_DATA, so the
/// temporary folder must be set first (see <see cref="TempWorkspace"/>).
/// </summary>
internal static class AvaloniaTestHost
{
    internal static HeadlessUnitTestSession Session() =>
        HeadlessUnitTestSession.GetOrStartForAssembly(typeof(AvaloniaTestHost).Assembly);
}

/// <summary>A Workspace folder and personal data folder in a new temporary directory, deleted after the test.</summary>
public sealed class TempWorkspace : IDisposable
{
    public const string DataVariable = "RICOCHET_DATA";

    private readonly string? _previousData = Environment.GetEnvironmentVariable(DataVariable);

    public TempWorkspace()
    {
        Root = Directory.CreateTempSubdirectory("ricochet-app-tests").FullName;
        PersonalDirectory = Path.Combine(Root, "personal");
        Directory.CreateDirectory(PersonalDirectory);
        Environment.SetEnvironmentVariable(DataVariable, PersonalDirectory);
        AvaloniaTestHost.Session();
    }

    public string Root { get; }
    public string PersonalDirectory { get; }

    /// <summary>A new store on the same personal folder, as if the app were started again.</summary>
    public WorkspaceStore NewStore() => new(PersonalDirectory);

    public void Dispose()
    {
        Environment.SetEnvironmentVariable(DataVariable, _previousData);
        try
        {
            Directory.Delete(Root, recursive: true);
        }
        catch (IOException)
        {
            // The Workspace folder watcher may still hold the folder open. The OS removes the temporary folder later.
        }
        catch (UnauthorizedAccessException)
        {
        }
    }
}

/// <summary>
/// A server on this machine's loopback address that answers every request with 201 Created and records the request line.
/// The real sender talks to it, so no request leaves the machine. Loopback needs no URL reservation or administrator rights.
/// </summary>
public sealed class StubServer : IDisposable
{
    private readonly TcpListener _listener = new(IPAddress.Loopback, 0);
    private readonly Lock _gate = new();
    private readonly List<string> _requestLines = [];

    public StubServer()
    {
        _listener.Start();
        Port = ((IPEndPoint)_listener.LocalEndpoint).Port;
        _ = AcceptAsync();
    }

    public int Port { get; }

    /// <summary>The first line of each request received, such as "POST /orders HTTP/1.1".</summary>
    public IReadOnlyList<string> RequestLines
    {
        get
        {
            lock (_gate)
                return _requestLines.ToList();
        }
    }

    public string Url(string path) => $"http://127.0.0.1:{Port}{path}";

    private async Task AcceptAsync()
    {
        try
        {
            while (true)
            {
                var client = await _listener.AcceptTcpClientAsync();
                _ = ServeAsync(client);
            }
        }
        catch (ObjectDisposedException)
        {
            // Disposed: the test is over.
        }
        catch (SocketException)
        {
            // Stopped listening.
        }
    }

    private async Task ServeAsync(TcpClient client)
    {
        using (client)
        {
            var stream = client.GetStream();
            using var reader = new StreamReader(stream, Encoding.ASCII, leaveOpen: true);
            var requestLine = await reader.ReadLineAsync() ?? "";
            string? header;
            do
            {
                header = await reader.ReadLineAsync();
            }
            while (!string.IsNullOrEmpty(header));

            lock (_gate)
                _requestLines.Add(requestLine);

            const string body = "{\"ok\":true}";
            var response = "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: "
                + Encoding.ASCII.GetByteCount(body) + "\r\nConnection: close\r\n\r\n" + body;
            await stream.WriteAsync(Encoding.ASCII.GetBytes(response));
        }
    }

    public void Dispose() => _listener.Stop();
}

public sealed class FakeDialogs : IDialogs
{
    public Task<IReadOnlyList<string>> PickPostmanFilesAsync() => Task.FromResult<IReadOnlyList<string>>([]);
    public Task<string?> PickOpenApiFileAsync() => Task.FromResult<string?>(null);
    public Task<string?> PickIdsFileAsync() => Task.FromResult<string?>(null);
    public Task<string?> PickWorkspaceFolderAsync() => Task.FromResult<string?>(null);
    public Task<string?> PickClientCertificateAsync() => Task.FromResult<string?>(null);
    public Task<string?> PickResponseSavePathAsync(string suggestedFileName) => Task.FromResult<string?>(null);
}

public sealed class FakeClipboard : IClipboardService
{
    public string? Text { get; private set; }

    public Task SetTextAsync(string text)
    {
        Text = text;
        return Task.CompletedTask;
    }
}
