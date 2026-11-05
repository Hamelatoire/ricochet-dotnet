using Avalonia.Styling;

namespace Ricochet.App;

/// <summary>The theme choice: follow the system by default, or always use light or dark (ADR 0013).</summary>
public static class ThemeSettings
{
    public const string SystemChoice = "System";
    public const string LightChoice = "Light";
    public const string DarkChoice = "Dark";

    public static readonly IReadOnlyList<string> Choices = [SystemChoice, LightChoice, DarkChoice];

    /// <summary>Turns the stored value ("light", "dark" or null) into the name shown in the menu.</summary>
    public static string ChoiceName(string? stored) => stored switch
    {
        "light" => LightChoice,
        "dark" => DarkChoice,
        _ => SystemChoice,
    };

    /// <summary>Turns a menu name into the value kept in state.json. Null means follow the system.</summary>
    public static string? StoredValue(string choice) => choice switch
    {
        LightChoice => "light",
        DarkChoice => "dark",
        _ => null,
    };

    /// <summary>The variant to force, or null to follow the operating system.</summary>
    public static ThemeVariant? Variant(string? stored) => stored switch
    {
        "light" => ThemeVariant.Light,
        "dark" => ThemeVariant.Dark,
        _ => null,
    };
}
