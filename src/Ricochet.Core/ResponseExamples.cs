namespace Ricochet.Core;

/// <summary>Turns a response into a saved example, with credentials masked (ADR 0023).</summary>
public static class ResponseExamples
{
    // Masking very short values would scramble unrelated output, as in CollectionRunner.
    private const int MinMaskedLength = 4;

    /// <summary>
    /// Builds the example for a response. <paramref name="sent"/> is the resolved Request, so its literal
    /// credentials are known and can be found wherever they were echoed back. Values that count as secrets
    /// in <paramref name="variables"/>, and captured values, are masked too, as in a run's output.
    /// </summary>
    /// <exception cref="InvalidOperationException">The response cannot be kept in full, so it is not saved.</exception>
    public static ResponseExample Create(string name, ApiRequest sent, IEnumerable<KeyValue> variables, IEnumerable<KeyValue> captured, ApiResponse response)
    {
        if (response.Truncated)
            throw new InvalidOperationException("The response is too large to save in full, so it was not saved as an example.");

        var formatted = ResponseFormatter.Format(response);
        if (formatted.IsBinary)
            throw new InvalidOperationException("Binary responses cannot be saved as examples.");

        // Sensitive headers sent are masked wherever they are echoed, as well as the values CollectionRunner masks.
        var headerSecrets = sent.Headers.Where(h => h.Enabled && SecretRules.IsSensitiveHeader(h.Key) && SecretRules.IsLiteral(h.Value)).Select(h => h.Value);
        var secrets = CollectionRunner.SecretsToMask(variables, [], captured, sent)
            .Concat(headerSecrets.Where(v => v.Length >= MinMaskedLength))
            .Distinct()
            .OrderByDescending(v => v.Length)
            .ToList();

        return new ResponseExample
        {
            Name = name.Trim(),
            StatusCode = response.StatusCode,
            ReasonPhrase = response.ReasonPhrase,
            Headers = response.Headers.Select(h => MaskHeader(h, secrets)).ToList(),
            MediaType = response.MediaType,
            Body = Mask(formatted.Text, secrets),
        };
    }

    /// <summary>Sensitive headers are hidden outright, as they are in History. Other values have any secret masked.</summary>
    private static KeyValue MaskHeader(KeyValue header, List<string> secrets)
    {
        if (SecretRules.IsSensitiveHeader(header.Key) && SecretRules.IsLiteral(header.Value))
            return header with { Value = SecretRules.HiddenValue };

        return header with { Value = Mask(header.Value, secrets) };
    }

    private static string Mask(string text, List<string> secrets) =>
        secrets.Aggregate(text, (current, secret) => current.Replace(secret, SecretRules.HiddenValue, StringComparison.Ordinal));
}
