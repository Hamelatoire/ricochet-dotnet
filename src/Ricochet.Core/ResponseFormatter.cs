using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ricochet.Core;

public sealed record FormattedBody(string Text, bool IsJson, bool IsBinary);

public static class ResponseFormatter
{
    private const int MaxPrettyPrintChars = 32 * 1024 * 1024;
    private const int BinarySniffBytes = 8192;
    private const int MaxJsonDepth = 512;

    public static FormattedBody Format(ApiResponse response)
    {
        var encoding = ResolveEncoding(response.CharSet);
        if (encoding is null && LooksBinary(response.Body))
            return new FormattedBody($"[Binary response, {response.Body.Length:N0} bytes]", false, true);

        var text = (encoding ?? Encoding.UTF8).GetString(response.Body).TrimStart('﻿');
        if (response.Truncated || text.Length > MaxPrettyPrintChars || !MightBeJson(response.MediaType, text))
            return new FormattedBody(text, false, false);

        return TryPrettyPrintJson(text, out var pretty)
            ? new FormattedBody(pretty, true, false)
            : new FormattedBody(text, false, false);
    }

    private static Encoding? ResolveEncoding(string? charSet)
    {
        if (string.IsNullOrWhiteSpace(charSet))
            return null;

        try
        {
            return Encoding.GetEncoding(charSet.Trim().Trim('"'));
        }
        catch (ArgumentException)
        {
            return null;
        }
    }

    private static bool LooksBinary(byte[] body) =>
        body.AsSpan(0, Math.Min(body.Length, BinarySniffBytes)).Contains((byte)0);

    private static bool MightBeJson(string? mediaType, string text)
    {
        if (mediaType?.Contains("json", StringComparison.OrdinalIgnoreCase) == true)
            return true;

        var start = text.AsSpan().TrimStart();
        return start.Length > 0 && start[0] is '{' or '[';
    }

    private static bool TryPrettyPrintJson(string text, out string pretty)
    {
        try
        {
            // JsonDocument keeps numbers as written, so large or precise values are not altered.
            using var document = JsonDocument.Parse(text, new JsonDocumentOptions { MaxDepth = MaxJsonDepth });
            using var output = new MemoryStream();
            using (var writer = new Utf8JsonWriter(output, new JsonWriterOptions
            {
                Indented = true,
                Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                MaxDepth = MaxJsonDepth,
            }))
            {
                document.WriteTo(writer);
            }

            pretty = Encoding.UTF8.GetString(output.GetBuffer(), 0, (int)output.Length);
            return true;
        }
        catch (JsonException)
        {
            pretty = "";
            return false;
        }
    }
}
