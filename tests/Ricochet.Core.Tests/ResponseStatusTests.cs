using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class ResponseStatusTests
{
    [Theory]
    [InlineData(100, ResponseStatusClass.Neutral)]
    [InlineData(101, ResponseStatusClass.Neutral)]
    [InlineData(200, ResponseStatusClass.Success)]
    [InlineData(204, ResponseStatusClass.Success)]
    [InlineData(299, ResponseStatusClass.Success)]
    [InlineData(301, ResponseStatusClass.Redirect)]
    [InlineData(304, ResponseStatusClass.Redirect)]
    [InlineData(399, ResponseStatusClass.Redirect)]
    [InlineData(400, ResponseStatusClass.ClientError)]
    [InlineData(404, ResponseStatusClass.ClientError)]
    [InlineData(499, ResponseStatusClass.ClientError)]
    [InlineData(500, ResponseStatusClass.ServerError)]
    [InlineData(503, ResponseStatusClass.ServerError)]
    [InlineData(599, ResponseStatusClass.ServerError)]
    [InlineData(600, ResponseStatusClass.Neutral)]
    [InlineData(0, ResponseStatusClass.Neutral)]
    public void Classifies_status_codes_by_family(int statusCode, ResponseStatusClass expected) =>
        Assert.Equal(expected, ResponseStatus.Classify(statusCode));

    [Theory]
    [InlineData("application/json", "response.json")]
    [InlineData("application/json; charset=utf-8", "response.json")]
    [InlineData("Application/JSON", "response.json")]
    [InlineData("application/problem+json", "response.json")]
    [InlineData("application/xml", "response.xml")]
    [InlineData("text/xml", "response.xml")]
    [InlineData("image/svg+xml", "response.svg")]
    [InlineData("text/html", "response.html")]
    [InlineData("text/plain", "response.txt")]
    [InlineData("text/csv", "response.csv")]
    [InlineData("application/pdf", "response.pdf")]
    [InlineData("image/jpeg", "response.jpg")]
    [InlineData("application/octet-stream", "response.bin")]
    [InlineData("", "response.bin")]
    [InlineData(null, "response.bin")]
    public void Suggests_a_file_name_from_the_media_type(string? mediaType, string expected) =>
        Assert.Equal(expected, ResponseStatus.SuggestedBodyFileName(mediaType));
}
