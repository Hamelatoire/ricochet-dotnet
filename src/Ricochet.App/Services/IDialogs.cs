namespace Ricochet.App.Services;

/// <summary>
/// The file and folder pickers. The window implements it, because the native pickers need a window handle (ADR 0039).
/// A null or empty result means the user cancelled. Every picker only reads or names a path; none writes to one itself.
/// </summary>
public interface IDialogs
{
    Task<IReadOnlyList<string>> PickPostmanFilesAsync();

    Task<string?> PickOpenApiFileAsync();

    Task<string?> PickIdsFileAsync();

    Task<string?> PickWorkspaceFolderAsync();

    Task<string?> PickClientCertificateAsync();

    /// <summary>Asks where to save a response body. The caller writes the file.</summary>
    Task<string?> PickResponseSavePathAsync(string suggestedFileName);
}

/// <summary>Puts text on the system clipboard.</summary>
public interface IClipboardService
{
    Task SetTextAsync(string text);
}
