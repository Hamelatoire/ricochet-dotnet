using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace Ricochet.Core;

/// <summary>Secure storage could not be used. The message is suitable for showing to the user.</summary>
public sealed class SecretVaultException(string message, Exception? inner = null) : Exception(message, inner);

/// <summary>
/// Holds every Secret as one protected item, so each OS only needs "read" and "write",
/// and a Secret that was removed disappears at the next save.
/// </summary>
public interface ISecretVault
{
    /// <exception cref="SecretVaultException">Existing Secrets could not be read. Callers must not overwrite them.</exception>
    Dictionary<string, string> Load();

    /// <exception cref="SecretVaultException">The Secrets could not be stored.</exception>
    void Save(IReadOnlyDictionary<string, string> secrets);
}

/// <summary>Keeps Secrets only until the app closes. Used when the OS offers no secure storage.</summary>
public sealed class InMemorySecretVault : ISecretVault
{
    private Dictionary<string, string> _secrets = [];

    public Dictionary<string, string> Load() => new(_secrets);

    public void Save(IReadOnlyDictionary<string, string> secrets) => _secrets = new Dictionary<string, string>(secrets);
}

public static class SecretVaults
{
    /// <summary>Picks the vault for this OS. On an unknown OS, Secrets last only for the session.</summary>
    public static ISecretVault CreateDefault(string windowsFilePath)
    {
        if (OperatingSystem.IsWindows())
            return new WindowsSecretVault(windowsFilePath);
        if (OperatingSystem.IsMacOS())
            return new MacSecretVault();
        if (OperatingSystem.IsLinux())
            return new LinuxSecretVault();
        return new InMemorySecretVault();
    }

    internal static string Serialize(IReadOnlyDictionary<string, string> secrets) =>
        JsonSerializer.Serialize(new Dictionary<string, string>(secrets), WorkspaceJsonContext.Default.DictionaryStringString);

    internal static Dictionary<string, string> Deserialize(string json) =>
        JsonSerializer.Deserialize(json, WorkspaceJsonContext.Default.DictionaryStringString) ?? [];
}

/// <summary>A file encrypted with DPAPI, which ties it to the current Windows user.</summary>
[SupportedOSPlatform("windows")]
public sealed class WindowsSecretVault(string path) : ISecretVault
{
    public Dictionary<string, string> Load()
    {
        if (!File.Exists(path))
            return [];

        try
        {
            var plain = ProtectedData.Unprotect(File.ReadAllBytes(path), null, DataProtectionScope.CurrentUser);
            return SecretVaults.Deserialize(Encoding.UTF8.GetString(plain));
        }
        catch (Exception ex) when (ex is CryptographicException or JsonException)
        {
            // Unreadable for good, for example after a profile reset. Keep it, and let the next launch start clean.
            var kept = $"{path}.unreadable-{DateTime.UtcNow:yyyyMMddHHmmss}";
            File.Move(path, kept, overwrite: true);
            throw new SecretVaultException($"Your saved secrets could not be decrypted, and were kept at {kept}.", ex);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            throw new SecretVaultException($"Your saved secrets could not be read: {ex.Message}", ex);
        }
    }

    public void Save(IReadOnlyDictionary<string, string> secrets)
    {
        try
        {
            var plain = Encoding.UTF8.GetBytes(SecretVaults.Serialize(secrets));
            var encrypted = ProtectedData.Protect(plain, null, DataProtectionScope.CurrentUser);

            Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
            var temp = path + ".tmp";
            File.WriteAllBytes(temp, encrypted);
            File.Move(temp, path, overwrite: true);
        }
        catch (Exception ex) when (ex is CryptographicException or IOException or UnauthorizedAccessException)
        {
            throw new SecretVaultException($"Your secrets could not be saved: {ex.Message}", ex);
        }
    }
}

/// <summary>One generic password item in the user's login Keychain.</summary>
[SupportedOSPlatform("macos")]
public sealed partial class MacSecretVault : ISecretVault
{
    private const string Security = "/System/Library/Frameworks/Security.framework/Security";
    private const string CoreFoundation = "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation";
    private const int ErrSecItemNotFound = -25300;

    private static readonly byte[] Service = "Ricochet"u8.ToArray();
    private static readonly byte[] Account = "secrets"u8.ToArray();

    [LibraryImport(Security)]
    private static partial int SecKeychainFindGenericPassword(
        IntPtr keychain, uint serviceLength, byte[] service, uint accountLength, byte[] account,
        out uint passwordLength, out IntPtr password, out IntPtr item);

    [LibraryImport(Security)]
    private static partial int SecKeychainAddGenericPassword(
        IntPtr keychain, uint serviceLength, byte[] service, uint accountLength, byte[] account,
        uint passwordLength, byte[] password, IntPtr item);

    [LibraryImport(Security)]
    private static partial int SecKeychainItemModifyAttributesAndData(IntPtr item, IntPtr attributes, uint length, byte[] data);

    [LibraryImport(Security)]
    private static partial int SecKeychainItemFreeContent(IntPtr attributes, IntPtr data);

    [LibraryImport(CoreFoundation)]
    private static partial void CFRelease(IntPtr item);

    public Dictionary<string, string> Load()
    {
        var status = SecKeychainFindGenericPassword(
            IntPtr.Zero, (uint)Service.Length, Service, (uint)Account.Length, Account, out var length, out var data, out var item);

        if (status == ErrSecItemNotFound)
            return [];
        if (status != 0)
            throw new SecretVaultException($"Your saved secrets could not be read from the Keychain (error {status}).");

        try
        {
            return SecretVaults.Deserialize(Marshal.PtrToStringUTF8(data, (int)length));
        }
        catch (JsonException ex)
        {
            throw new SecretVaultException("Your saved secrets in the Keychain could not be understood.", ex);
        }
        finally
        {
            _ = SecKeychainItemFreeContent(IntPtr.Zero, data);
            CFRelease(item);
        }
    }

    public void Save(IReadOnlyDictionary<string, string> secrets)
    {
        var bytes = Encoding.UTF8.GetBytes(SecretVaults.Serialize(secrets));
        var status = SecKeychainFindGenericPassword(
            IntPtr.Zero, (uint)Service.Length, Service, (uint)Account.Length, Account, out _, out var data, out var item);

        if (status == ErrSecItemNotFound)
        {
            status = SecKeychainAddGenericPassword(
                IntPtr.Zero, (uint)Service.Length, Service, (uint)Account.Length, Account, (uint)bytes.Length, bytes, IntPtr.Zero);
        }
        else if (status == 0)
        {
            _ = SecKeychainItemFreeContent(IntPtr.Zero, data);
            status = SecKeychainItemModifyAttributesAndData(item, IntPtr.Zero, (uint)bytes.Length, bytes);
            CFRelease(item);
        }

        if (status != 0)
            throw new SecretVaultException($"Your secrets could not be saved to the Keychain (error {status}).");
    }
}

/// <summary>
/// One item in the Secret Service, through the secret-tool command from libsecret.
/// Values pass over stdin and stdout, so they never appear in a process's arguments.
/// </summary>
[SupportedOSPlatform("linux")]
public sealed class LinuxSecretVault : ISecretVault
{
    private static readonly string[] Attributes = ["service", "ricochet", "account", "secrets"];

    public Dictionary<string, string> Load()
    {
        var (exitCode, output, error) = Run(["lookup", .. Attributes], input: null);

        // secret-tool exits with 1 and no message when the item does not exist yet.
        if (exitCode != 0 && error.Trim().Length == 0)
            return [];
        if (exitCode != 0)
            throw new SecretVaultException($"The Secret Service could not be reached: {error.Trim()}");

        try
        {
            return output.Trim().Length == 0 ? [] : SecretVaults.Deserialize(output);
        }
        catch (JsonException ex)
        {
            throw new SecretVaultException("Your saved secrets in the Secret Service could not be understood.", ex);
        }
    }

    public void Save(IReadOnlyDictionary<string, string> secrets)
    {
        var (exitCode, _, error) = Run(["store", "--label=Ricochet secrets", .. Attributes], SecretVaults.Serialize(secrets));
        if (exitCode != 0)
            throw new SecretVaultException($"Your secrets could not be saved to the Secret Service: {error.Trim()}");
    }

    private static (int ExitCode, string Output, string Error) Run(string[] arguments, string? input)
    {
        var info = new ProcessStartInfo("secret-tool")
        {
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        foreach (var argument in arguments)
            info.ArgumentList.Add(argument);

        try
        {
            using var process = Process.Start(info) ?? throw new SecretVaultException("secret-tool could not be started.");
            if (input is not null)
                process.StandardInput.Write(input);
            process.StandardInput.Close();

            var error = process.StandardError.ReadToEndAsync();
            var output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();
            return (process.ExitCode, output, error.GetAwaiter().GetResult());
        }
        catch (Win32Exception ex)
        {
            throw new SecretVaultException("secret-tool is not installed (it is part of libsecret-tools).", ex);
        }
    }
}
