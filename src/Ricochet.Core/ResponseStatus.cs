namespace Ricochet.Core;

/// <summary>How a response's status code is shown: the colour family of its status badge.</summary>
public enum ResponseStatusClass
{
    /// <summary>1xx, or any code outside 100–599: shown without a colour.</summary>
    Neutral,
    Success,
    Redirect,
    ClientError,
    ServerError,
}

public static class ResponseStatus
{
    public static ResponseStatusClass Classify(int statusCode) => statusCode switch
    {
        >= 200 and < 300 => ResponseStatusClass.Success,
        >= 300 and < 400 => ResponseStatusClass.Redirect,
        >= 400 and < 500 => ResponseStatusClass.ClientError,
        >= 500 and < 600 => ResponseStatusClass.ServerError,
        _ => ResponseStatusClass.Neutral,
    };

    /// <summary>
    /// A file name for saving the body, chosen from its media type so that the file opens in the right
    /// application. Unknown or missing types fall back to <c>.bin</c>.
    /// </summary>
    public static string SuggestedBodyFileName(string? mediaType)
    {
        var type = (mediaType ?? "").Split(';')[0].Trim().ToLowerInvariant();
        var extension = type switch
        {
            "image/svg+xml" => "svg",
            _ when type.EndsWith("json", StringComparison.Ordinal) => "json",
            _ when type.EndsWith("xml", StringComparison.Ordinal) => "xml",
            "text/html" => "html",
            "text/css" => "css",
            "text/javascript" or "application/javascript" => "js",
            "text/csv" => "csv",
            "text/plain" => "txt",
            "application/pdf" => "pdf",
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "application/zip" => "zip",
            _ => "bin",
        };
        return $"response.{extension}";
    }
}
