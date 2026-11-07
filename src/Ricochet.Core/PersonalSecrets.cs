namespace Ricochet.Core;

/// <summary>
/// Secrets that belong to this person on this machine, not to any Workspace (ADR 0026).
/// They live in the same secure storage as Workspace Secrets, under their own prefix, so saving a Workspace never removes them.
/// </summary>
public static class PersonalSecrets
{
    public const string Prefix = "personal/";
    public const string ClientCertificatePasswordKey = Prefix + "clientCertificatePassword";

    /// <summary>The client certificate's password, or null when none is saved.</summary>
    public static string? ClientCertificatePassword(IReadOnlyDictionary<string, string> vault) =>
        vault.TryGetValue(ClientCertificatePasswordKey, out var password) && password.Length > 0 ? password : null;

    /// <summary>Returns a copy of the vault with the password replaced. An empty password removes it.</summary>
    public static Dictionary<string, string> WithClientCertificatePassword(IReadOnlyDictionary<string, string> vault, string? password)
    {
        var result = new Dictionary<string, string>(vault);
        if (string.IsNullOrEmpty(password))
            result.Remove(ClientCertificatePasswordKey);
        else
            result[ClientCertificatePasswordKey] = password;

        return result;
    }
}
