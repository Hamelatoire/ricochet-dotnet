using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
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
            var sender = new RequestSender(RequestSender.CreateDefaultClient());
            var store = new WorkspaceStore(WorkspaceStore.DefaultPersonalDirectory(), WorkspaceStore.DefaultOldAppCopy());
            // The window provides the dialogs and the clipboard, so it is created first and then given to the view model.
            var window = new MainWindow();
            var viewModel = new MainViewModel(sender, store, SecretVaults.CreateDefault(store.SecretsPath), window, window);
            RequestedThemeVariant = ThemeSettings.Variant(store.Theme);
            window.DataContext = viewModel;
            WindowMemory.Attach(window, store);
            desktop.MainWindow = window;
        }

        base.OnFrameworkInitializationCompleted();
    }
}
