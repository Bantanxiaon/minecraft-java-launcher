using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;
using SHLauncher.Contracts.Downloads;
using SHLauncher.Downloads;
if (args.Length > 0 && args[0] == "child-download")
{
    var childRoot = args[2]; Directory.CreateDirectory(childRoot);
    using var childScheduler = new DownloadScheduler(1,1);
    using var childManager = new DownloadManager(childScheduler, Path.Combine(childRoot, "cache"));
    var childResult = await childManager.DownloadAsync(new(Guid.NewGuid(), "Cross process download", childRoot,
        [new(Path.Combine(childRoot, "target.bin"), long.Parse(args[5]), args[3], args[4], [new("Local process test", new Uri(args[1]))])]), null, default);
    await System.IO.File.WriteAllTextAsync(Path.Combine(childRoot, "result.json"), JsonSerializer.Serialize(childResult));
    if (!childResult.Succeeded) throw new Exception("Child download failed.");
    return;
}
var root = new DirectoryInfo(AppContext.BaseDirectory);
while (root is not null && !File.Exists(Path.Combine(root.FullName, "SHLauncherNext.sln"))) root = root.Parent;
var work = Path.Combine(root!.FullName, ".devdata", "download-tests", Guid.NewGuid().ToString("N")); Directory.CreateDirectory(work);
await using var server = new LocalHttpServer();
using var scheduler = new DownloadScheduler(4, 2);
var options = new DownloadOptions(SegmentThreshold: 64 * 1024, ReadTimeout: TimeSpan.FromMilliseconds(400));
using var manager = new DownloadManager(scheduler, Path.Combine(work, "cache"), options);
var hash = Convert.ToHexString(SHA256.HashData(server.Payload)); var sha1 = Convert.ToHexString(SHA1.HashData(server.Payload));
var checks = new List<string>(); var snapshots = new List<DownloadTaskSnapshot>(); var snapshotGate = new object();
var report = new InlineProgress(snapshot => { lock (snapshotGate) snapshots.Add(snapshot); });
DownloadFile FilePlan(string destination, params string[] sources) => new(Path.Combine(work, destination), server.Payload.Length, sha1, hash,
    sources.Select((s, i) => new DownloadSource(s, new Uri(server.BaseUri, s), i == 0)).ToArray());
DownloadRequest Request(params DownloadFile[] files) => new(Guid.NewGuid(), "Local HTTP reproducible test", work, files);
void Check(bool condition, string message) { if (!condition) throw new Exception(message); checks.Add(message); }
async Task<DownloadResult> Run(DownloadManager engine, DownloadFile file, CancellationToken token = default) => await engine.DownloadAsync(Request(file), report, token);
// Isolated cache folders keep transfer tests independent from cache reuse tests.
DownloadManager Fresh(string name, DownloadOptions? config = null) => new(scheduler, Path.Combine(work, name + "-cache"), config ?? options);
using (var engine = Fresh("range"))
{
    var result = await Run(engine, FilePlan("range.bin", "range"));
    Check(result.Succeeded && server.Calls("range") == 3, "strict 206 Range probe plus two segments");
    Check((await System.IO.File.ReadAllBytesAsync(Path.Combine(work, "range.bin"))).SequenceEqual(server.Payload), "segmented bytes match complete payload");
}
foreach (var endpoint in new[] { "no-range", "range-invalid", "range-416", "range-broken" })
{
    using var engine = Fresh(endpoint); var result = await Run(engine, FilePlan(endpoint + ".bin", endpoint));
    Check(result.Succeeded && (await System.IO.File.ReadAllBytesAsync(Path.Combine(work, endpoint + ".bin"))).SequenceEqual(server.Payload), endpoint + " falls back without corrupt output");
}
using (var engine = Fresh("failover"))
{
    var result = await Run(engine, FilePlan("failover.bin", "missing", "range"));
    Check(result.Succeeded && result.Files.Single().SourceId == "range" && server.Calls("missing") == 1, "404 disables source A and switches to source B");
    Check(engine.Events().Any(e => e.SourceId == "missing" && e.Kind == "disabled"), "failover reason is traceable");
}
using (var engine = Fresh("forbidden"))
{
    var result = await Run(engine, FilePlan("forbidden.bin", "forbidden", "range"));
    Check(result.Succeeded && server.Calls("forbidden") == 1, "403 not blindly retried");
}
using (var engine = Fresh("retry"))
{
    var result = await Run(engine, FilePlan("retry.bin", "reset")); Check(result.Succeeded && server.Calls("reset") >= 2, "502 transient retry");
}
{
    const string marker="synthetic-private-value";
    var journal=Path.Combine(work,"redaction-journal");
    using var sink=new JsonDownloadEventSink(journal);
    using var engine=new DownloadManager(scheduler,Path.Combine(work,"redaction-cache"),options,eventSink:sink,
        authorize:_=>throw new IOException("fixture failure "+marker),redact:text=>text.Replace(marker,"[REDACTED]",StringComparison.Ordinal));
    var result=await Run(engine,FilePlan("redaction.bin","range"));sink.Dispose();
    Check(!result.Succeeded&&result.Failures.All(f=>!f.Message.Contains(marker,StringComparison.Ordinal))&&engine.Events().All(e=>!e.Message.Contains(marker,StringComparison.Ordinal))&&
        Directory.GetFiles(journal,"*.jsonl").All(p=>!System.IO.File.ReadAllText(p).Contains(marker,StringComparison.Ordinal)),
        "private values redacted from download failures, memory events and durable journal before recording");
}
foreach (var endpoint in new[] { "unauthorized", "forbidden" })
{
    using var engine = Fresh("terminal-" + endpoint);
    var before = server.Calls(endpoint);
    var result = await Run(engine, FilePlan("terminal-" + endpoint + ".bin", endpoint));
    Check(!result.Succeeded && result.Failures.Single().Code == (endpoint == "unauthorized" ? "HTTP401" : "HTTP403") && server.Calls(endpoint) == before + 1,
        endpoint + " preserves structured HTTP status without retrying credentials");
}
using (var engine = Fresh("busy"))
{
    var clock = Stopwatch.StartNew(); var result = await Run(engine, FilePlan("busy.bin", "busy"));
    Check(result.Succeeded && clock.Elapsed >= TimeSpan.FromSeconds(1), "429 Retry-After minimum respected");
}
using (var engine = Fresh("hash"))
{
    var destination = Path.Combine(work, "keep-existing.bin"); await System.IO.File.WriteAllTextAsync(destination, "original");
    var result = await Run(engine, FilePlan("keep-existing.bin", "corrupt"));
    Check(!result.Succeeded && await System.IO.File.ReadAllTextAsync(destination) == "original", "hash failure never overwrites existing target");
    Check(Directory.GetFiles(work, "*.part").Length > 0, "failed partial files retained with .part extension");
}
using (var engine = Fresh("cache"))
{
    var one = await Run(engine, FilePlan("cache1.bin", "range")); var before = server.Calls("range");
    var two = await Run(engine, FilePlan("cache2.bin", "range"));
    var three = await Run(engine, FilePlan("cache2.bin", "range"));
    Check(one.Succeeded && two.Files.Single().Reused && three.Files.Single().Reused && before == server.Calls("range"), "cache copy and warm reinstall require no network");
}
using (var engine = Fresh("cancel"))
{
    using var cancel = new CancellationTokenSource(140); var result = await Run(engine, FilePlan("cancel.bin", "slow"), cancel.Token);
    Check(result.Cancelled && !System.IO.File.Exists(Path.Combine(work, "cancel.bin")), "cancel awaits workers and leaves no fake complete target");
    var parts = Directory.GetFiles(work, "cancel.bin.*.part").Select(p => (p, new FileInfo(p).Length)).ToArray();
    await Task.Delay(180); Check(parts.All(p => new FileInfo(p.p).Length == p.Length), "no background writes after cancellation returns");
    var retry = await Run(engine, FilePlan("cancel.bin", "range")); Check(retry.Succeeded, "interrupted part safely restarts with fresh temp file");
}
using (var engine = Fresh("timeout", options with { AttemptsPerSource = 1 }))
{
    var result = await Run(engine, FilePlan("timeout.bin", "timeout", "range")); Check(result.Succeeded, "header timeout switches source");
}
using (var engine = new DownloadManager(scheduler, Path.Combine(work, "disk-cache"), options, _ => 0))
{
    var before = server.Calls("range"); var result = await Run(engine, FilePlan("disk.bin", "range"));
    Check(!result.Succeeded && result.Failures.Single().Code == "DiskSpace" && server.Calls("range") == before, "disk capacity safely simulated before network");
}
using (var engine = Fresh("global"))
{
    var files = Enumerable.Range(0, 8).Select(i => FilePlan("parallel" + i + ".bin", "range")).ToArray();
    var result = await engine.DownloadAsync(Request(files), report, default);
    Console.WriteLine(JsonSerializer.Serialize(new { Batch = result, Peak = scheduler.PeakConnections, Active = scheduler.ActiveConnections, Events = engine.Events().TakeLast(30) }, new JsonSerializerOptions { WriteIndented = true }));
    Check(result.Succeeded && scheduler.PeakConnections <= 2 && scheduler.ActiveConnections == 0, "global connection lease count never exceeds two and returns to zero");
}
using (var engine = Fresh("metadata"))
{
    var invalidJson = FilePlan("invalid.json", "range") with { Sha1 = null, Sha256 = null, ContentKind = DownloadContentKind.JsonMetadata };
    var result = await Run(engine, invalidJson);
    Check(!result.Succeeded && !System.IO.File.Exists(invalidJson.Destination), "invalid JSON metadata is not finalized even with HTTP 200");
}
using (var engine = Fresh("checksum"))
{
    DownloadFile Checksum(string endpoint) => new(Path.Combine(work, endpoint + ".sha1"), null, null, null,
        [new("Maven test", new Uri(server.BaseUri, endpoint))], DownloadContentKind.Sha1Metadata);
    var valid = await Run(engine, Checksum("sha1"));
    Check(valid.Succeeded && (await System.IO.File.ReadAllTextAsync(Checksum("sha1").Destination)).Trim() == sha1, "strict Maven SHA1 sidecar accepted");
    var invalid = await Run(engine, Checksum("sha1-invalid"));
    Check(!invalid.Succeeded && !System.IO.File.Exists(Checksum("sha1-invalid").Destination), "HTML masquerading as checksum rejected before finalize");
    var oversized = await Run(engine, Checksum("range"));
    Check(!oversized.Succeeded && !System.IO.File.Exists(Checksum("range").Destination), "checksum metadata limited to 1024 bytes");
}
using (var engine = Fresh("xml"))
{
    DownloadFile Xml(string endpoint) => new(Path.Combine(work, endpoint + ".xml"), null, null, null,
        [new("Maven XML", new Uri(server.BaseUri, endpoint))], DownloadContentKind.XmlMetadata);
    Check((await Run(engine, Xml("xml"))).Succeeded, "well formed Maven XML metadata accepted");
    Check(!(await Run(engine, Xml("xml-dtd"))).Succeeded && !System.IO.File.Exists(Xml("xml-dtd").Destination), "external XML entities and DTD rejected");
    Check(!(await Run(engine, Xml("range"))).Succeeded && !System.IO.File.Exists(Xml("range").Destination), "non XML response not finalized as metadata");
}
var unknownKind = false;
try { await manager.DownloadAsync(Request(FilePlan("unknown.bin", "range") with { Sha1 = null, Sha256 = null, ContentKind = (DownloadContentKind)999 }), report, default); } catch (ArgumentException) { unknownKind = true; }
Check(unknownKind, "unknown metadata kind cannot bypass binary hash requirement");
var missingHash = false;
try { await manager.DownloadAsync(Request(FilePlan("unhashed.bin", "range") with { Sha1 = null, Sha256 = null }), report, default); } catch (ArgumentException) { missingHash = true; }
Check(missingHash, "binary without expected hash rejected");
var invalidRoot = false;
try { await manager.DownloadAsync(Request(FilePlan("../escape.bin", "range")), report, default); } catch (ArgumentException) { invalidRoot = true; }
Check(invalidRoot, "destination traversal rejected");
Check(snapshots.Any(s => s.State == DownloadTaskState.Verifying) && snapshots.Any(s => s.State == DownloadTaskState.Finalizing), "unified task publishes verification and finalize stages");
await CrossProcessScenarios.Run(server, work);
checks.Add("real process interruption and restart with changed source/hash never resumes stale part");

using (var sink = new JsonDownloadEventSink(Path.Combine(work, "journal")))
using (var engine = new DownloadManager(scheduler, Path.Combine(work, "journal-cache"), options, eventSink: sink))
{
    var before = server.Calls("range");
    var result = await Run(engine, FilePlan("prefer-no-range.bin", "no-range", "range"));
    Check(result.Succeeded && result.Files.Single().SourceId == "no-range" && server.Calls("range") == before, "valid source A without Range stays preferred over B");
}
using (var sink = new JsonDownloadEventSink(Path.Combine(work, "journal")))
using (var engine = new DownloadManager(scheduler, Path.Combine(work, "journal-failover-cache"), options, eventSink: sink))
{
    var result = await Run(engine, FilePlan("no-range-failover.bin", "no-range-corrupt", "range"));
    Check(result.Succeeded && result.Files.Single().SourceId == "range", "corrupt source A without Range fails hash and switches to Range B");
    sink.Dispose();
    Check(sink.ReadTail(result.Id).Any(e => e.Kind == "complete"), "journal tail can be read after closing and reopening without original memory events");
    var events = Directory.GetFiles(Path.Combine(work, "journal"), "*.jsonl").SelectMany(System.IO.File.ReadAllLines).Select(l => JsonSerializer.Deserialize<DownloadEvent>(l)!).ToArray();
    Check(events.Any(e => e.Kind == "complete" && e.Message.Contains("target=")) && events.Any(e => e.Message.Contains("Response 206")), "durable journal records actual response and finalized target");
}

foreach (var mode in Enum.GetValues<DownloadSourceMode>())
{
    using var engine = new DownloadManager(scheduler, Path.Combine(work, "policy-" + mode), options,
        sourceSelector: new DownloadSourceSelector(()=>mode));
    var target = Path.Combine(work,"policy-" + mode + ".bin");
    var file = new DownloadFile(target,server.Payload.Length,sha1,hash,[
        new("official",new Uri(server.BaseUri,mode == DownloadSourceMode.MirrorPreferred ? "range" : "missing"),true),
        new("mirror",new Uri(server.BaseUri,mode == DownloadSourceMode.MirrorPreferred ? "corrupt" : "range"),false)]);
    var result = await Run(engine,file);
    Check(result.Succeeded && result.Files.Single().SourceId == (mode == DownloadSourceMode.MirrorPreferred ? "official" : "mirror"),
        mode + " exhausts failed preferred source and verifies exact artifact from fallback");
    Check(engine.Events().Any(e=>e.Kind=="source-policy") && engine.Events().Any(e=>e.Kind=="integrity" && e.Message.Contains("obtained=")), "source policy and expected/obtained hash diagnostics " + mode);
}
using(var engine=Fresh("all-sources-fail"))
{
    var result=await Run(engine,FilePlan("all-sources-fail.bin","missing","forbidden"));
    Check(!result.Succeeded&&!System.IO.File.Exists(Path.Combine(work,"all-sources-fail.bin"))&&engine.Events().Count(e=>e.Kind=="disabled")==2,
        "all eligible sources exhausted before truthful failure, no complete-looking target");
}
var mirrorSelector = new DownloadSourceSelector(()=>DownloadSourceMode.MirrorPreferred,true);
var officialAsset = new DownloadFile(Path.Combine(work,"inert-asset"),1,sha1,null,[new("Mojang",new Uri("https://resources.download.minecraft.net/ab/hash"))]);
Check(mirrorSelector.Select(officialAsset).First().Id == "BMCLAPI" && mirrorSelector.Select(officialAsset with { Sources=[new("Provider",new Uri("https://cdn.modrinth.com/inert"))] }).Count == 1,
    "mirror mapping is scoped to documented hosts and never substitutes another provider");
var rotating=Path.Combine(work,"rotating-journal");var rotatingId=Guid.NewGuid();
using(var sink=new JsonDownloadEventSink(rotating,1024))
{
    for(var i=0;i<80;i++)sink.Write(new(rotatingId,DateTimeOffset.UtcNow,"test","rotation",i+new string('x',150)));
}
using(var reopened=new JsonDownloadEventSink(rotating,1024))
{
    reopened.Write(new(rotatingId,DateTimeOffset.UtcNow,"test","last","kept"));
    Check(Directory.GetFiles(rotating,"*.jsonl").Length>1 && reopened.ReadTail(rotatingId).Last().Message=="kept",
        "source journal rotates without dropping later artifacts and reopens latest part");
}
var outcome = new { Passed = true, Checks = checks, ActualServerPeak = server.Peak, ActualConnectionLeasePeak = scheduler.PeakConnections, FileParallelism = 4, GlobalConnectionLimit = 2, SegmentsPerFile = 2,
    Notes = "Local TCP HTTP server; no public servers attacked. Restart partial from zero, no unverified cross-session resume.", Work = work };
Console.WriteLine(JsonSerializer.Serialize(outcome, new JsonSerializerOptions { WriteIndented = true }));
await System.IO.File.WriteAllTextAsync(Path.Combine(work, "result.json"), JsonSerializer.Serialize(outcome));
sealed class InlineProgress(Action<DownloadTaskSnapshot> action) : IProgress<DownloadTaskSnapshot> { public void Report(DownloadTaskSnapshot value) => action(value); }
