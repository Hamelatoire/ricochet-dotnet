using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>The mock server dialog (ADR 0027). All routing and listening is in Core: this only holds what the dialog shows.</summary>
public sealed partial class MockServerViewModel : ObservableObject
{
    private const int MaxLogLines = 100;

    private readonly MockServer _server = new();
    private readonly List<string> _logLines = [];
    private MockRouteTable? _routes;

    public MockServerViewModel()
    {
        // The server raises this on a background thread, so the change is handed to the UI thread.
        _server.RequestHandled += entry => Dispatcher.UIThread.Post(() => AddLogLine(entry));
    }

    [ObservableProperty] private bool _isOpen;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasError), nameof(ErrorVisible))]
    private string _error = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanStart), nameof(IsStopped), nameof(StatusText))]
    private bool _isRunning;

    [ObservableProperty] private string _portText = "8080";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanStart), nameof(SourceText))]
    private int _routeCount;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SourceText))]
    private string _sourceName = "";

    [ObservableProperty] private string _logText = "";

    public bool HasError => Error.Length > 0;
    public bool ErrorVisible => HasError;
    public bool IsStopped => !IsRunning;
    public bool CanStart => !IsRunning && RouteCount > 0;

    public string SourceText => RouteCount == 0
        ? "No description loaded yet."
        : $"{SourceName}: {RouteCount} {(RouteCount == 1 ? "route" : "routes")}";

    public string StatusText => IsRunning
        ? $"Running at http://127.0.0.1:{_server.Port}/. Only this computer can reach it. Stop it when you are done."
        : "Not running. Nothing listens on the network until you start it.";

    [RelayCommand]
    private void Open()
    {
        Error = "";
        IsOpen = true;
    }

    /// <summary>Closing the dialog leaves a running server running. Only Stop ends it.</summary>
    [RelayCommand]
    private void Close() => IsOpen = false;

    /// <summary>Reads an OpenAPI description the user picked. The file is only read, and nothing is sent anywhere.</summary>
    public void LoadDescription(string json, string fileName)
    {
        try
        {
            var routes = new MockRouteTable(MockRouteBuilder.FromOpenApi(json));
            _routes = routes;
            SourceName = fileName;
            RouteCount = routes.Count;
            Error = "";
            if (_server.IsRunning)
                _server.UpdateRoutes(routes);
        }
        catch (MockServerException ex)
        {
            Error = ex.Message;
        }
    }

    [RelayCommand]
    private void Start()
    {
        if (_routes is not { } routes)
        {
            Error = "Load an OpenAPI description first.";
            return;
        }

        if (!int.TryParse(PortText.Trim(), out var port))
        {
            Error = $"The port must be a number from {MockServer.MinPort} to {MockServer.MaxPort}.";
            return;
        }

        try
        {
            _server.Start(port, routes);
            Error = "";
        }
        catch (MockServerException ex)
        {
            Error = ex.Message;
        }
        finally
        {
            IsRunning = _server.IsRunning;
        }
    }

    [RelayCommand]
    private void Stop()
    {
        _server.Stop();
        IsRunning = false;
    }

    [RelayCommand]
    private void ClearLog()
    {
        _logLines.Clear();
        LogText = "";
    }

    private void AddLogLine(MockLogEntry entry)
    {
        _logLines.Add($"{entry.At:HH:mm:ss}  {entry.Method,-7} {entry.Path}  {entry.StatusCode}");
        if (_logLines.Count > MaxLogLines)
            _logLines.RemoveRange(0, _logLines.Count - MaxLogLines);
        LogText = string.Join(Environment.NewLine, _logLines);
    }
}
