using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Avalonia.Threading;
using Ricochet.App.ViewModels;
using Ricochet.Core;

namespace Ricochet.App;

public partial class App : Application
{
    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var personalDirectory = WorkspaceStore.DefaultPersonalDirectory();
            var version = typeof(App).Assembly.GetName().Version?.ToString() ?? "unknown";
            // Crashes are written to a local log only, never sent anywhere (ADR 0088). Set up first, so that early crashes are caught too.
            AppDomain.CurrentDomain.UnhandledException += (_, e) =>
            {
                if (e.ExceptionObject is Exception exception)
                    CrashLog.Write(personalDirectory, exception, version);
            };
            Dispatcher.UIThread.UnhandledException += (_, e) => CrashLog.Write(personalDirectory, e.Exception, version);

            var sender = new RequestSender(RequestSender.CreateDefaultClient());
            var store = new WorkspaceStore(personalDirectory, WorkspaceStore.DefaultOldAppCopy());
            // The window provides the dialogs and the clipboard, so it is created first and then given to the view model.
            var window = new MainWindow { CrashLogDirectory = CrashLog.DirectoryFor(personalDirectory) };
            var viewModel = new MainViewModel(sender, store, SecretVaults.CreateDefault(store.SecretsPath), window, window);
            RequestedThemeVariant = ThemeSettings.Variant(store.Theme);
            window.DataContext = viewModel;
            WindowMemory.Attach(window, store);
            PanelMemory.Attach(window, store);
            desktop.MainWindow = window;
        }

        base.OnFrameworkInitializationCompleted();
    }
}
