using System.IO;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json.Nodes;
using Avalonia.Controls;
using Avalonia.Headless.XUnit;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Avalonia.VisualTree;

namespace CrystalStudio.Tests;

public sealed class AiTests
{
    private sealed class Handler(Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> send) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token) => send(request, token);
    }
    private static HttpResponseMessage Reply(string json) => new(HttpStatusCode.OK) { Content = new StringContent(json, Encoding.UTF8, "application/json") };
    private static AiEndpointSettings Endpoint(string provider, AiRole role) => AiProviders.Create(provider, role) with { ApiKey = "role-key" };

    [Fact]
    public void SettingsKeepThreeIndependentEndpointsAndReadLegacyFiles()
    {
        string root = Path.Combine(Path.GetTempPath(), "CrystalAi-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            var store = new SettingsStore(Path.Combine(root, "settings.json"));
            File.WriteAllText(store.FilePath, "{\"Language\":\"zh-CN\"}");
            Assert.Equal("openai", (store.Load().Ai ?? new()).For(AiRole.Vision).Provider);
            var ai = new AiSettings(Endpoint("gemini", AiRole.Vision), Endpoint("sdwebui", AiRole.Image), Endpoint("ark", AiRole.Chat));
            store.Save(store.Load() with { Ai = ai });
            Assert.Equal(ai, store.Load().Ai);
            store.Save(store.Load() with { Language = "en-US" });
            Assert.Equal(ai, store.Load().Ai);
        }
        finally { Directory.Delete(root, true); }
    }

    [Theory]
    [InlineData("openai")]
    [InlineData("ark")]
    [InlineData("zhipu")]
    [InlineData("siliconflow")]
    public async Task CompatibleVisionUsesItsOwnKeyModelAndImages(string provider)
    {
        var cfg = Endpoint(provider, AiRole.Vision);
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            Assert.Equal("Bearer role-key", request.Headers.Authorization!.ToString());
            Assert.EndsWith("/chat/completions", request.RequestUri!.AbsolutePath);
            var body = JsonNode.Parse(await request.Content!.ReadAsStringAsync(token))!;
            Assert.Equal(cfg.Model, (string?)body["model"]);
            Assert.Equal("data:image/png;base64,AQ==", (string?)body["messages"]![1]!["content"]![1]!["image_url"]!["url"]);
            return Reply("{\"choices\":[{\"message\":{\"content\":\"ok\"}}]}");
        }));
        Assert.Equal("ok", await new AiClient(http).TextAsync(cfg, "system", "user", ["data:image/png;base64,AQ=="], default));
    }

    [Fact]
    public async Task GeminiUsesNativeImagesAndQueryAuthentication()
    {
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            Assert.Contains(":generateContent?key=role-key", request.RequestUri!.ToString());
            Assert.Null(request.Headers.Authorization);
            var body = JsonNode.Parse(await request.Content!.ReadAsStringAsync(token))!;
            Assert.Equal("AQ==", (string?)body["contents"]![0]!["parts"]![1]!["inline_data"]!["data"]);
            return Reply("{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"native\"}]}}]}");
        }));
        Assert.Equal("native", await new AiClient(http).TextAsync(Endpoint("gemini", AiRole.Vision), "s", "u", ["data:image/png;base64,AQ=="], default));
    }

    [Fact]
    public async Task DashscopeVisionUsesNativeMultimodalEndpoint()
    {
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            Assert.EndsWith("/services/aigc/multimodal-generation/generation", request.RequestUri!.AbsolutePath);
            var body = JsonNode.Parse(await request.Content!.ReadAsStringAsync(token))!;
            Assert.Equal("data:image/png;base64,AQ==", (string?)body["input"]!["messages"]![1]!["content"]![1]!["image"]);
            return Reply("{\"output\":{\"choices\":[{\"message\":{\"content\":[{\"text\":\"native\"}]}}]}}");
        }));
        Assert.Equal("native", await new AiClient(http).TextAsync(Endpoint("dashscope", AiRole.Vision), "s", "u", ["data:image/png;base64,AQ=="], default));
    }

    [Theory]
    [InlineData("openai", "/images/generations", "{\"data\":[{\"b64_json\":\"AQ==\"}]}")]
    [InlineData("sdwebui", "/sdapi/v1/txt2img", "{\"images\":[\"AQ==\"]}")]
    [InlineData("gemini", ":generateContent", "{\"candidates\":[{\"content\":{\"parts\":[{\"inlineData\":{\"mimeType\":\"image/png\",\"data\":\"AQ==\"}}]}}]}")]
    public async Task ImageAdaptersReturnImageBytes(string provider, string path, string response)
    {
        using var http = new HttpClient(new Handler((request, _) =>
        {
            Assert.EndsWith(path, request.RequestUri!.AbsolutePath);
            return Task.FromResult(Reply(response));
        }));
        Assert.Equal(new byte[] { 1 }, await new AiClient(http).GenerateImageAsync(Endpoint(provider, AiRole.Image), "tree", 512, 512, default));
    }

    [Fact]
    public async Task CustomTemplateEscapesPromptAndUsesResponsePath()
    {
        var cfg = Endpoint("custom", AiRole.Chat) with { BaseUrl = "https://example.invalid/v1", Path = "/run", ExtraBody = "{\"question\":\"{{prompt}}\",\"model\":\"{{model}}\"}", ResponsePath = "reply.text", Auth = "x-api-key" };
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            Assert.Equal("role-key", request.Headers.GetValues("x-api-key").Single());
            var body = JsonNode.Parse(await request.Content!.ReadAsStringAsync(token))!;
            Assert.Equal("quote \" slash \\ newline\n", (string?)body["question"]);
            return Reply("{\"reply\":{\"text\":\"custom\"}}");
        }));
        Assert.Equal("custom", await new AiClient(http).TextAsync(cfg, "", "quote \" slash \\ newline\n", [], default));
    }

    [Fact]
    public async Task HttpErrorsRedactKeys()
    {
        using var http = new HttpClient(new Handler((_, _) => Task.FromResult(new HttpResponseMessage(HttpStatusCode.Unauthorized) { Content = new StringContent("invalid role-key") })));
        var error = await Assert.ThrowsAsync<InvalidDataException>(() => new AiClient(http).TextAsync(Endpoint("openai", AiRole.Chat), "", "test", [], default));
        Assert.Contains("401", error.Message);
        Assert.DoesNotContain("role-key", error.Message);
    }

    [Fact]
    public async Task ImageDownloadsDoNotReceiveProviderCredentials()
    {
        using var http = new HttpClient(new Handler((request, _) =>
        {
            if (request.RequestUri!.Host == "images.invalid")
            {
                Assert.Null(request.Headers.Authorization); Assert.False(request.Headers.Contains("x-api-key"));
                Assert.DoesNotContain("role-key", request.RequestUri.ToString());
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent([1, 2]) });
            }
            Assert.Equal("Bearer role-key", request.Headers.Authorization!.ToString());
            return Task.FromResult(Reply("{\"data\":[{\"url\":\"https://images.invalid/result\"}]}"));
        }));
        Assert.Equal(new byte[] { 1, 2 }, await new AiClient(http).GenerateImageAsync(Endpoint("openai", AiRole.Image), "tree", 512, 512, default));
    }

    [Fact]
    public async Task StabilityUsesMultipartImageRequest()
    {
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            Assert.EndsWith("/v2beta/stable-image/generate/core", request.RequestUri!.AbsolutePath);
            Assert.Equal("multipart/form-data", request.Content!.Headers.ContentType!.MediaType);
            string body = await request.Content.ReadAsStringAsync(token);
            Assert.Contains("tree", body); Assert.Contains("aspect_ratio", body);
            Assert.Equal("Bearer role-key", request.Headers.Authorization!.ToString());
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent([1]) };
        }));
        Assert.Equal(new byte[] { 1 }, await new AiClient(http).GenerateImageAsync(Endpoint("stability", AiRole.Image), "tree", 512, 512, default));
    }

    [Fact]
    public async Task DashscopePollsNativeImageTask()
    {
        int calls = 0;
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            calls++;
            Assert.Equal("Bearer role-key", request.Headers.Authorization!.ToString());
            if (calls == 1)
            {
                Assert.Equal("enable", request.Headers.GetValues("X-DashScope-Async").Single());
                var body = JsonNode.Parse(await request.Content!.ReadAsStringAsync(token))!;
                Assert.Equal("512*512", (string?)body["parameters"]!["size"]);
                return Reply("{\"output\":{\"task_id\":\"task-1\"}}");
            }
            Assert.Equal(HttpMethod.Get, request.Method); Assert.EndsWith("/tasks/task-1", request.RequestUri!.AbsolutePath);
            return Reply("{\"output\":{\"task_status\":\"SUCCEEDED\",\"results\":[{\"url\":\"data:image/png;base64,AQ==\"}]}}");
        }));
        Assert.Equal(new byte[] { 1 }, await new AiClient(http).GenerateImageAsync(Endpoint("dashscope", AiRole.Image), "tree", 512, 512, default));
        Assert.Equal(2, calls);
    }

    [Fact]
    public async Task DashscopeChatUsesTextProtocol()
    {
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            Assert.EndsWith("/services/aigc/text-generation/generation", request.RequestUri!.AbsolutePath);
            var body = JsonNode.Parse(await request.Content!.ReadAsStringAsync(token))!;
            Assert.Equal("hello", (string?)body["input"]!["messages"]![1]!["content"]);
            return Reply("{\"output\":{\"choices\":[{\"message\":{\"content\":\"world\"}}]}}");
        }));
        Assert.Equal("world", await new AiClient(http).TextAsync(Endpoint("dashscope", AiRole.Chat), "", "hello", [], default));
    }

    [Theory]
    [InlineData("raw")]
    [InlineData("none")]
    [InlineData("query")]
    public async Task AuthenticationStylesUseOnlyConfiguredCredentialLocation(string auth)
    {
        using var http = new HttpClient(new Handler((request, _) =>
        {
            Assert.Equal(auth == "raw" ? "role-key" : null, request.Headers.Authorization?.ToString());
            Assert.Equal(auth == "query", request.RequestUri!.Query.Contains("key=role-key", StringComparison.Ordinal));
            return Task.FromResult(Reply("{\"choices\":[{\"message\":{\"content\":\"ok\"}}]}"));
        }));
        Assert.Equal("ok", await new AiClient(http).TextAsync(Endpoint("openai", AiRole.Chat) with { Auth = auth }, "", "test", [], default));
    }

    [Fact]
    public async Task TimeoutAndUserCancellationAreDistinct()
    {
        using var http = new HttpClient(new Handler(async (_, token) =>
        {
            await Task.Delay(Timeout.Infinite, token); return Reply("{}");
        }));
        var client = new AiClient(http); var cfg = Endpoint("openai", AiRole.Chat) with { TimeoutSeconds = 1 };
        var error = await Assert.ThrowsAsync<InvalidDataException>(() => client.TextAsync(cfg, "", "test", [], default));
        Assert.Contains("timed out", error.Message);
        using var stop = new CancellationTokenSource(); stop.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.TextAsync(cfg, "", "test", [], stop.Token));
    }

    [Fact]
    public async Task InvalidJsonAndTextOnlyImageRepliesAreRejected()
    {
        using var http = new HttpClient(new Handler((_, _) => Task.FromResult(Reply("not json"))));
        await Assert.ThrowsAsync<InvalidDataException>(() => new AiClient(http).TextAsync(Endpoint("openai", AiRole.Chat), "", "test", [], default));
        using var textHttp = new HttpClient(new Handler((_, _) => Task.FromResult(Reply("{\"choices\":[{\"message\":{\"content\":\"no image\"}}]}"))));
        await Assert.ThrowsAsync<InvalidDataException>(() => new AiClient(textHttp).GenerateImageAsync(Endpoint("openai", AiRole.Image), "tree", 512, 512, default));
    }

    [Fact]
    public void RecognitionParsesFencedJsonAndRejectsUnknownCategories()
    {
        var result = ExtractionAi.Parse("```json\n{\"name\":\"松树\",\"category\":\"tree\",\"completeness\":\"complete\",\"reason\":\"针叶树冠\"}\n```", ["Tree", "Building", "Unclassified"]);
        Assert.Equal("Tree", result.Category);
        Assert.Equal("松树", result.Name);
        Assert.Equal(AiObjectCompleteness.Complete, result.Completeness);
        Assert.Throws<InvalidDataException>(() => ExtractionAi.Parse("{\"name\":\"松树\",\"category\":\"invented\"}", ["Tree"]));
        Assert.Throws<InvalidDataException>(() => ExtractionAi.Parse("{\"name\":\"\",\"category\":\"Tree\"}", ["Tree"]));
    }

    [Fact]
    public void RecognitionIsOneUndoableChangeAndPreservesArtwork()
    {
        var candidate = new ExtractionCandidate(Guid.NewGuid(), "Object", "Unclassified", "source", ExtractionKind.Discovered,
            [new(0, 0, new() { Foreground = new(2, 1, 0, 0), BackImage = 0x20000000 })], [], [new("a.map", 5, 5, false)], [], Reviewed: true);
        var review = new ExtractionReview([candidate]);
        review.ApplyAi(new Dictionary<Guid, AiObjectRecognition> { [candidate.Id] = new("松树", "Tree", "树冠", AiObjectCompleteness.Complete) });
        var updated = Assert.Single(review.Candidates);
        Assert.Equal("松树", updated.Name); Assert.Equal("Tree", updated.Category); Assert.False(updated.Reviewed);
        Assert.Equal(candidate.Fingerprint, updated.Fingerprint); Assert.Same(candidate.Occurrences, updated.Occurrences);
        review.Undo(); Assert.Equal(candidate, Assert.Single(review.Candidates));
        review.Redo(); Assert.Equal("松树", Assert.Single(review.Candidates).Name);
    }

    [Theory]
    [InlineData("complete", "Complete")]
    [InlineData("incomplete", "Incomplete")]
    [InlineData("uncertain", "Uncertain")]
    public void RecognitionRequiresAnExplicitCompletenessDecision(string value, string expected)
    {
        var result = ExtractionAi.Parse(new JsonObject { ["name"] = "屋顶", ["category"] = "Building", ["reason"] = "轮廓证据", ["completeness"] = value }.ToJsonString(), ["Building"]);
        Assert.Equal(expected, result.Completeness.ToString());
    }

    [Theory]
    [InlineData("")]
    [InlineData(",\"completeness\":null")]
    [InlineData(",\"completeness\":false")]
    [InlineData(",\"completeness\":\"yes\"")]
    [InlineData(",\"completeness\":\"\"")]
    public void MissingOrMalformedCompletenessCannotSilentlyKeepOrDeleteObjects(string field)
    {
        Assert.Throws<InvalidDataException>(() => ExtractionAi.Parse("{\"name\":\"屋顶\",\"category\":\"Building\",\"reason\":\"属于建筑的一部分\"" + field + "}", ["Building"]));
    }

    [Fact]
    public void SixRoofSlicesAreExcludedWhileCompleteSlenderObjectsAndUncertainResultsRemainUndoableAndExportable()
    {
        string root = Path.Combine(Path.GetTempPath(), "CrystalAiCompleteness-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(root, "catalogue")); Directory.CreateDirectory(Path.Combine(root, "Objects"));
        string index = Path.Combine(root, "catalogue", "palette.json");
        File.WriteAllText(index, "{\"schemaVersion\":1,\"objectRoot\":\"Objects\",\"previewRoot\":\"catalogue\",\"objects\":[]}");
        try
        {
            string[] names = ["传统建筑屋顶", "传统建筑屋顶", "传统建筑", "传统建筑屋顶", "瓦屋顶", "传统建筑屋顶", "旗杆", "柱子", "完整墙段", "待审核物件"];
            var originals = names.Select((name, i) => new ExtractionCandidate(Guid.NewGuid(), name, "Building", "source", ExtractionKind.Discovered,
                [new(0, 0, new() { Foreground = new(2, (short)(i + 1), 0, 0), BackImage = 0x20000000 })], [], [new("whitevillage.map", 5 + i, 5, false)], [], Reviewed: true)).ToArray();
            var results = originals.Select((c, i) => (c.Id, Recognition: new AiObjectRecognition(c.Name, "Building", i < 6 ? "仅屋顶的一条瓦片，缺少主体轮廓" : "可见结构证据",
                i < 6 ? AiObjectCompleteness.Incomplete : i == 9 ? AiObjectCompleteness.Uncertain : AiObjectCompleteness.Complete))).ToDictionary(p => p.Id, p => p.Recognition);
            var review = new ExtractionReview(originals);
            review.ApplyAi(results);
            Assert.Equal(originals.Skip(6).Select(c => c.Id), review.Candidates.Select(c => c.Id));
            Assert.All(review.Candidates, c => Assert.False(c.Reviewed));
            Assert.Equal(originals.Skip(6).Select(c => c.Fingerprint), review.Candidates.Select(c => c.Fingerprint));
            Assert.All(review.Candidates, c => Assert.Same(originals.Single(o => o.Id == c.Id).Occurrences, c.Occurrences));
            Assert.Contains("Completeness uncertain", review.Candidates[^1].Reason);
            review.Undo(); Assert.Equal(originals, review.Candidates);
            review.Redo(); Assert.Equal(4, review.Candidates.Count);
            review.Update(review.Candidates.Select(c => c.Id), reviewed: true);
            var exported = ObjectExtractionExport.Save(index, new("ai-completeness", originals.ToList(), ["whitevillage.map"], []), review.Candidates, default);
            Assert.Equal(4, exported.Objects.Count);
            Assert.Equal(originals.Skip(6).Select(c => c.Fingerprint).Order(), exported.Objects.Select(c => c.Fingerprint).Order());
            Assert.Equal(4, StampCategories.Read(index).Objects.Count);
            Assert.All(originals.Take(6), c => Assert.DoesNotContain(exported.Objects, output => output.Fingerprint == c.Fingerprint));
        }
        finally { Directory.Delete(root, true); }
    }

    [AvaloniaFact]
    public async Task SettingsRestoreAndSaveIndependentRoles()
    {
        AiSettings? saved = null;
        var ai = new AiSettings(Endpoint("gemini", AiRole.Vision), Endpoint("sdwebui", AiRole.Image), Endpoint("zhipu", AiRole.Chat));
        var window = new SettingsWindow(new LibrarySettings(Ai: ai), (_, _, value, _) => { saved = value; return Task.CompletedTask; });
        window.Show(); await Task.Delay(50); Dispatcher.UIThread.RunJobs();
        try
        {
            window.GetVisualDescendants().OfType<Expander>().Single(e => e.Name == "AiSettings").IsExpanded = true;
            await Task.Delay(50); Dispatcher.UIThread.RunJobs();
            var tabs = window.GetVisualDescendants().OfType<TabControl>().Single(t => t.Name == "AiRoleTabs");
            Assert.Equal(3, tabs.Items.Count);
            foreach (var role in Enum.GetValues<AiRole>())
            {
                tabs.SelectedIndex = (int)role; await Task.Delay(25); Dispatcher.UIThread.RunJobs();
                var panel = window.GetVisualDescendants().Single(c => c.Name == "Ai" + role + "Settings");
                var model = panel.GetVisualDescendants().OfType<TextBox>().Single(c => c.Name == "Ai" + role + "Model");
                Assert.Equal(ai.For(role).Model, model.Text);
                model.Text = "custom-" + role;
                var providers = panel.GetVisualDescendants().OfType<ComboBox>().Single(c => c.Name == "Ai" + role + "Provider");
                Assert.Equal(role == AiRole.Image ? 9 : 7, providers.Items.Count);
                int originalIndex = providers.SelectedIndex;
                providers.SelectedIndex = 0;
                Assert.Equal(AiProviders.Create("openai", role).Model, model.Text);
                providers.SelectedIndex = originalIndex;
                Assert.Equal("custom-" + role, model.Text);
            }
            var buttons = ((StackPanel)((ScrollViewer)window.Content!).Content!).Children.OfType<StackPanel>().Last();
            buttons.Children.OfType<Button>().Last().RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
            Assert.NotNull(saved);
            foreach (var role in Enum.GetValues<AiRole>())
            {
                Assert.Equal(ai.For(role).BaseUrl, saved.For(role).BaseUrl);
                Assert.Equal("custom-" + role, saved.For(role).Model);
            }
        }
        finally { window.Close(); }
    }

    [AvaloniaTheory]
    [InlineData("success")]
    [InlineData("cancel")]
    [InlineData("failure")]
    [InlineData("incomplete")]
    [InlineData("exclude-all")]
    [InlineData("invalid-completeness")]
    public async Task ExtractionRecognitionUsesVisionAndCanBeUndone(string mode)
    {
        string root = Path.Combine(Path.GetTempPath(), "CrystalAiUi-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(root, "catalogue")); Directory.CreateDirectory(Path.Combine(root, "Objects"));
        string index = Path.Combine(root, "catalogue", "palette.json");
        File.WriteAllText(index, "{\"schemaVersion\":1,\"objectRoot\":\"Objects\",\"previewRoot\":\"catalogue\",\"objects\":[]}");
        var map = new MapDocument(32, 32); map.Set(5, 5, new() { Foreground = new(2, 1, 0, 0) });
        map.Set(20, 20, new() { Foreground = new(1, 1, 0, 0) });
        ObjectExtractionWindow? window = null;
        int requests = 0;
        using var http = new HttpClient(new Handler(async (request, token) =>
        {
            requests++; Assert.Equal("Bearer vision-key", request.Headers.Authorization!.ToString());
            if (requests == 2 && mode == "cancel")
            {
                window!.GetVisualDescendants().OfType<Button>().Single(b => b.Name == "ExtractionCancel").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
                token.ThrowIfCancellationRequested();
            }
            if (requests == 2 && mode == "failure") return Reply("{}");
            var body = JsonNode.Parse(await request.Content!.ReadAsStringAsync(token))!;
            Assert.Equal("vision-model", (string?)body["model"]);
            var content = body["messages"]![1]!["content"]!.AsArray(); Assert.Equal(3, content.Count);
            string system = (string)body["messages"]![0]!["content"]!;
            Assert.Contains("屋面窄条", system); Assert.Contains("不能把第二张图", system);
            Assert.Contains("完整的柱子、旗杆", system);
            string completeness = mode == "exclude-all" || (mode == "incomplete" && requests == 1) ? "incomplete" : "complete";
            var recognition = new JsonObject { ["name"] = "松树", ["category"] = "Unclassified", ["reason"] = "可见轮廓证据", ["completeness"] = completeness };
            if (mode == "invalid-completeness" && requests == 2) recognition.Remove("completeness");
            string text = recognition.ToJsonString();
            return Reply(new JsonObject { ["choices"] = new JsonArray(new JsonObject { ["message"] = new JsonObject { ["content"] = text } }) }.ToJsonString());
        }));
        var cfg = new AiSettings(Endpoint("openai", AiRole.Vision) with { ApiKey = "vision-key", Model = "vision-model" }, Endpoint("sdwebui", AiRole.Image), Endpoint("gemini", AiRole.Chat));
        window = new ObjectExtractionWindow(() => new("source.map", map), TestApplication.Root, index, () => { }, aiSettings: () => cfg, aiClient: new AiClient(http));
        window.Show(); Dispatcher.UIThread.RunJobs();
        try
        {
            await window.ScanAsync(); var originals = window.Review!.Candidates.ToArray(); Assert.Equal(2, originals.Length);
            long revision = map.PickupRevision;
            await window.RecognizeAsync(true);
            Assert.Equal(2, requests);
            if (mode is "success" or "incomplete" or "exclude-all")
            {
                int expected = mode == "success" ? 2 : mode == "incomplete" ? 1 : 0;
                Assert.Equal(expected, window.Review.Candidates.Count);
                Assert.All(window.Review.Candidates, c => { Assert.Equal("松树", c.Name); Assert.False(c.Reviewed); });
                var retained = mode == "success" ? originals : mode == "incomplete" ? originals.Skip(1).ToArray() : [];
                Assert.Equal(retained.Select(c => c.Fingerprint), window.Review.Candidates.Select(c => c.Fingerprint));
                Assert.Equal(expected, window.GetVisualDescendants().OfType<ListBox>().Single(l => l.Name == "ExtractionCandidates").Items.Count);
                Assert.False(window.GetVisualDescendants().OfType<Button>().Single(b => b.Name == "ExtractionSave").IsEnabled);
                window.Review.Undo();
            }
            else Assert.False(window.Review.CanUndo);
            Assert.Equal(revision, map.PickupRevision);
            Assert.Equal(originals, window.Review.Candidates);
        }
        finally { window.Close(); await Task.Delay(50); Dispatcher.UIThread.RunJobs(); Directory.Delete(root, true); }
    }
}
