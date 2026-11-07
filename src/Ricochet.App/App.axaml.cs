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
            var viewModel = new MainViewModel(sender, store, SecretVaults.CreateDefault(store.SecretsPath));
            RequestedThemeVariant = ThemeSettings.Variant(store.Theme);
            var window = new MainWindow { DataContext = viewModel };
            WindowMemory.Attach(window, store);
            desktop.MainWindow = window;
        }

        base.OnFrameworkInitializationCompleted();
    }
}
