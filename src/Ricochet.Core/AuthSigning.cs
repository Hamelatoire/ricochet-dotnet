using System.Globalization;
using System.Security.Cryptography;
using System.Text;

namespace Ricochet.Core;

/// <summary>
/// Builds the Authorization headers for Digest, OAuth 1.0 and AWS Signature Version 4 (ADR 0024).
/// These are pure functions: the nonce, timestamp and clock are passed in, so the results can be checked against known examples.
/// </summary>
public static class AuthSigning
{
    /// <summary>Percent-encodes as RFC 3986 requires, which OAuth 1.0 and AWS both use: only unreserved characters stay as they are.</summary>
    public static string Encode(string value)
    {
        var sb = new StringBuilder();
        foreach (var b in Encoding.UTF8.GetBytes(value))
        {
            var c = (char)b;
            if (b < 128 && (char.IsAsciiLetterOrDigit(c) || c is '-' or '.' or '_' or '~'))
                sb.Append(c);
            else
                sb.Append('%').Append(b.ToString("X2", CultureInfo.InvariantCulture));
        }

        return sb.ToString();
    }

    /// <summary>Splits a query string into decoded pairs. A pair without "=" has an empty value.</summary>
    public static List<KeyValuePair<string, string>> ParseQuery(string query)
    {
        var pairs = new List<KeyValuePair<string, string>>();
        foreach (var part in query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            var eq = part.IndexOf('=');
            var key = eq < 0 ? part : part[..eq];
            var value = eq < 0 ? "" : part[(eq + 1)..];
            pairs.Add(new(Uri.UnescapeDataString(key.Replace('+', ' ')), Uri.UnescapeDataString(value.Replace('+', ' '))));
        }

        return pairs;
    }

    // Digest (RFC 7616, checked against the MD5 example of RFC 2617)

    /// <summary>
    /// The Authorization value answering a Digest challenge. <paramref name="challenge"/> is the text after the "Digest" scheme
    /// in WWW-Authenticate. Supports MD5 and SHA-256 with qop "auth" or no qop.
    /// </summary>
    /// <exception cref="RequestFailedException">The challenge is incomplete or asks for something not supported.</exception>
    public static string DigestAuthorization(RequestAuth auth, string method, string requestTarget, string challenge, string clientNonce)
    {
        var parameters = ParseDigestChallenge(challenge);
        if (!parameters.TryGetValue("realm", out var realm) || !parameters.TryGetValue("nonce", out var nonce))
            throw new RequestFailedException("The server's Digest challenge has no realm or nonce.");

        var algorithm = parameters.GetValueOrDefault("algorithm", "MD5");
        if (algorithm is not ("MD5" or "SHA-256"))
            throw new RequestFailedException($"The server asks for Digest algorithm {algorithm}, which is not supported. Use MD5 or SHA-256.");

        string? qop = null;
        if (parameters.TryGetValue("qop", out var offered))
        {
            var options = offered.Split(',').Select(o => o.Trim()).ToList();
            if (!options.Contains("auth"))
                throw new RequestFailedException($"The server asks for Digest qop {offered}, which is not supported. Use auth.");

            qop = "auth";
        }

        const string nc = "00000001";
        var ha1 = Hash(algorithm, $"{auth.Username}:{realm}:{auth.Password}");
        var ha2 = Hash(algorithm, $"{method}:{requestTarget}");
        var response = qop is null
            ? Hash(algorithm, $"{ha1}:{nonce}:{ha2}")
            : Hash(algorithm, $"{ha1}:{nonce}:{nc}:{clientNonce}:{qop}:{ha2}");

        var header = new StringBuilder();
        header.Append($"Digest username=\"{Quote(auth.Username)}\", realm=\"{Quote(realm)}\", nonce=\"{Quote(nonce)}\", ");
        header.Append($"uri=\"{Quote(requestTarget)}\", algorithm={algorithm}");
        if (qop is not null)
            header.Append($", qop={qop}, nc={nc}, cnonce=\"{Quote(clientNonce)}\"");
        header.Append($", response=\"{response}\"");
        if (parameters.TryGetValue("opaque", out var opaque))
            header.Append($", opaque=\"{Quote(opaque)}\"");

        return header.ToString();
    }

    /// <summary>Reads name=value pairs from a challenge, where values may be quoted and contain commas.</summary>
    public static Dictionary<string, string> ParseDigestChallenge(string parameters)
    {
        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var i = 0;
        while (i < parameters.Length)
        {
            while (i < parameters.Length && parameters[i] is ',' or ' ' or '\t')
                i++;

            var start = i;
            while (i < parameters.Length && parameters[i] is not ('=' or ','))
                i++;

            var key = parameters[start..i].Trim();
            if (i >= parameters.Length || parameters[i] != '=')
                continue;

            i++; // the "="
            string value;
            if (i < parameters.Length && parameters[i] == '"')
            {
                i++;
                var sb = new StringBuilder();
                while (i < parameters.Length && parameters[i] != '"')
                {
                    if (parameters[i] == '\\' && i + 1 < parameters.Length)
                        i++;

                    sb.Append(parameters[i]);
                    i++;
                }

                i++; // the closing quote
                value = sb.ToString();
            }
            else
            {
                var valueStart = i;
                while (i < parameters.Length && parameters[i] != ',')
                    i++;

                value = parameters[valueStart..i].Trim();
            }

            if (key.Length > 0)
                result[key] = value;
        }

        return result;
    }

    private static string Hash(string algorithm, string text)
    {
        var bytes = Encoding.UTF8.GetBytes(text);
        var hash = algorithm == "SHA-256" ? SHA256.HashData(bytes) : MD5.HashData(bytes);
        return Hex(hash);
    }

    private static string Quote(string value) => value.Replace("\\", "\\\\").Replace("\"", "\\\"");

    // OAuth 1.0 (RFC 5849, HMAC-SHA1)

    /// <summary>
    /// The Authorization header for an OAuth 1.0 request signed with HMAC-SHA1. <paramref name="bodyParameters"/> are the
    /// decoded form fields of a form-encoded body, which take part in the signature. Query parameters are read from the URL.
    /// </summary>
    public static string OAuth1Authorization(RequestAuth auth, string method, Uri uri, IEnumerable<KeyValuePair<string, string>> bodyParameters, string nonce, long timestamp)
    {
        var details = auth.Details ?? new AuthDetails();
        if (details.ConsumerKey.Length == 0)
            throw new RequestFailedException("Enter the OAuth 1.0 consumer key.");

        var oauth = new List<KeyValuePair<string, string>>
        {
            new("oauth_consumer_key", details.ConsumerKey),
            new("oauth_nonce", nonce),
            new("oauth_signature_method", "HMAC-SHA1"),
            new("oauth_timestamp", timestamp.ToString(CultureInfo.InvariantCulture)),
        };
        if (auth.Token.Length > 0)
            oauth.Add(new("oauth_token", auth.Token));
        oauth.Add(new("oauth_version", "1.0"));

        var everything = ParseQuery(uri.Query).Concat(bodyParameters).Concat(oauth)
            .Select(p => (Key: Encode(p.Key), Value: Encode(p.Value)))
            .OrderBy(p => p.Key, StringComparer.Ordinal)
            .ThenBy(p => p.Value, StringComparer.Ordinal)
            .Select(p => $"{p.Key}={p.Value}");

        var baseString = $"{method.ToUpperInvariant()}&{Encode(uri.GetLeftPart(UriPartial.Path))}&{Encode(string.Join('&', everything))}";
        var signingKey = $"{Encode(details.ConsumerSecret)}&{Encode(details.TokenSecret)}";
        var signature = Convert.ToBase64String(HMACSHA1.HashData(Encoding.UTF8.GetBytes(signingKey), Encoding.UTF8.GetBytes(baseString)));

        oauth.Add(new("oauth_signature", signature));
        return "OAuth " + string.Join(", ", oauth.Select(p => $"{p.Key}=\"{Encode(p.Value)}\""));
    }

    // AWS Signature Version 4

    /// <summary>
    /// The headers that sign a request for AWS Signature Version 4: X-Amz-Date, X-Amz-Security-Token when there is a session token,
    /// and Authorization. The signed headers are host, x-amz-date and, when there is one, x-amz-security-token.
    /// </summary>
    public static IReadOnlyList<KeyValue> AwsSigV4Headers(RequestAuth auth, string method, Uri uri, byte[] body, DateTimeOffset now)
    {
        var details = auth.Details ?? new AuthDetails();
        if (details.AwsAccessKeyId.Length == 0 || details.AwsSecretAccessKey.Length == 0)
            throw new RequestFailedException("Enter the AWS access key id and secret access key.");
        if (details.AwsRegion.Length == 0 || details.AwsService.Length == 0)
            throw new RequestFailedException("Enter the AWS region and service.");

        var amzDate = now.UtcDateTime.ToString("yyyyMMdd'T'HHmmss'Z'", CultureInfo.InvariantCulture);
        var date = amzDate[..8];
        var host = uri.IsDefaultPort ? uri.Host : $"{uri.Host}:{uri.Port}";

        var headers = new SortedDictionary<string, string>(StringComparer.Ordinal)
        {
            ["host"] = host,
            ["x-amz-date"] = amzDate,
        };
        if (details.AwsSessionToken.Length > 0)
            headers["x-amz-security-token"] = details.AwsSessionToken;

        var canonicalHeaders = string.Concat(headers.Select(h => $"{h.Key}:{h.Value.Trim()}\n"));
        var signedHeaders = string.Join(';', headers.Keys);

        var canonicalUri = string.Join('/', uri.AbsolutePath.Split('/').Select(s => Encode(Uri.UnescapeDataString(s))));
        if (canonicalUri.Length == 0)
            canonicalUri = "/";

        var canonicalQuery = string.Join('&', ParseQuery(uri.Query)
            .Select(p => (Key: Encode(p.Key), Value: Encode(p.Value)))
            .OrderBy(p => p.Key, StringComparer.Ordinal)
            .ThenBy(p => p.Value, StringComparer.Ordinal)
            .Select(p => $"{p.Key}={p.Value}"));

        var canonicalRequest = $"{method.ToUpperInvariant()}\n{canonicalUri}\n{canonicalQuery}\n{canonicalHeaders}\n{signedHeaders}\n{Hex(SHA256.HashData(body))}";
        var scope = $"{date}/{details.AwsRegion}/{details.AwsService}/aws4_request";
        var stringToSign = $"AWS4-HMAC-SHA256\n{amzDate}\n{scope}\n{Hex(SHA256.HashData(Encoding.UTF8.GetBytes(canonicalRequest)))}";

        var kDate = HMACSHA256.HashData(Encoding.UTF8.GetBytes("AWS4" + details.AwsSecretAccessKey), Encoding.UTF8.GetBytes(date));
        var kRegion = HMACSHA256.HashData(kDate, Encoding.UTF8.GetBytes(details.AwsRegion));
        var kService = HMACSHA256.HashData(kRegion, Encoding.UTF8.GetBytes(details.AwsService));
        var kSigning = HMACSHA256.HashData(kService, Encoding.UTF8.GetBytes("aws4_request"));
        var signature = Hex(HMACSHA256.HashData(kSigning, Encoding.UTF8.GetBytes(stringToSign)));

        var result = new List<KeyValue> { new("X-Amz-Date", amzDate) };
        if (details.AwsSessionToken.Length > 0)
            result.Add(new("X-Amz-Security-Token", details.AwsSessionToken));

        result.Add(new("Authorization",
            $"AWS4-HMAC-SHA256 Credential={details.AwsAccessKeyId}/{scope}, SignedHeaders={signedHeaders}, Signature={signature}"));
        return result;
    }

    private static string Hex(byte[] bytes) => Convert.ToHexString(bytes).ToLowerInvariant();
}
