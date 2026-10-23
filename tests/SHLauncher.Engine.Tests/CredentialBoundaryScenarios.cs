using System.Reflection;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using SHLauncher.Bootstrap;
using SHLauncher.Contracts.Install;
using SHLauncher.Infrastructure.Runtime;
internal static class CredentialBoundaryScenarios
{
    internal static async Task Run()
    {
        var workspaceRoot = new DirectoryInfo(AppContext.BaseDirectory);
        while (workspaceRoot is not null && !File.Exists(Path.Combine(workspaceRoot.FullName, "SHLauncherNext.sln"))) workspaceRoot = workspaceRoot.Parent;
        var work = Path.Combine(workspaceRoot!.FullName, ".devdata", "credential-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(work);
        var marker = "synthetic-" + Guid.NewGuid().ToString("N");
        var config = Path.Combine(work, "curseforge.local.json");
        var checks = 0;
        void Check(bool value) { if (!value) throw new Exception("Credential boundary regression."); checks++; }
        void Invalid(CurseForgeAccessConfiguration value)
        {
            try { value.HasAccess(); throw new Exception("Invalid credential accepted."); }
            catch (InstallationPreconditionException error)
            {
                Check(error.Code == "ProviderAccessInvalid" && error.InnerException is null && !error.ToString().Contains(marker, StringComparison.Ordinal));
            }
            Check(value.Headers(new Uri("https://unrelated.invalid/")).Count == 0);
        }
        Check(!new CurseForgeAccessConfiguration(null, null).HasAccess());
        var direct = new CurseForgeAccessConfiguration(marker, null);
        Check(direct.HasAccess());
        Check(!direct.Redact("diagnostic:"+marker).Contains(marker,StringComparison.Ordinal));
        var encoded=new CurseForgeAccessConfiguration("synthetic$+value",null);
        Check(!encoded.Redact("synthetic%24%2Bvalue").Contains("synthetic",StringComparison.Ordinal));
        var privateLog=new SHLauncher.Infrastructure.Logging.GameLogService(direct.Redact);
        privateLog.Write(Guid.NewGuid(),SHLauncher.Contracts.Engine.GameLogLevel.Error,marker,"transport:"+marker);
        Check(privateLog.Snapshot().All(e=>!e.Message.Contains(marker,StringComparison.Ordinal)&&!e.Source.Contains(marker,StringComparison.Ordinal)));
        foreach (var url in new[] { "https://api.curseforge.com/v1/inert", "https://edge.forgecdn.net/files/inert" })
            Check(direct.Headers(new Uri(url)).TryGetValue("x-api-key", out var value) && value == marker);
        foreach (var url in new[] { "https://evil.test/", "https://api.curseforge.com.evil.test/", "http://api.curseforge.com/", "https://api.curseforge.com:444/", "https://user@api.curseforge.com/" })
            Check(direct.Headers(new Uri(url)).Count == 0);
        foreach (var invalid in new[] { marker + "\r\nInjected: yes", marker + " value", marker + "中文", new string('a', 4097) })
            Invalid(new(invalid, null));
        Invalid(new(null, "relative.json"));
        Invalid(new(null, "//unreachable.invalid/share/credential.json"));
        Invalid(new(null, Path.Combine(work, "missing.json")));
        Invalid(new(null, work));
        await File.WriteAllTextAsync(config, JsonSerializer.Serialize(new { apiKey = marker }));
        Check(new CurseForgeAccessConfiguration(null, config).HasAccess());
        await File.WriteAllTextAsync(config, JsonSerializer.Serialize(new { apiKey = marker }), new UTF8Encoding(true));
        Check(new CurseForgeAccessConfiguration(null, config).Headers(new Uri("https://api.curseforge.com/"))["x-api-key"] == marker);
        Invalid(new(marker, config));
        foreach (var invalid in new[] { "{\"apiKey\":\"" + marker, "[]", "{}", "{\"apiKey\":null}", "{\"apiKey\":23}", "{\"apiKey\":\"\"}", "{\"apiKey\":\"a\",\"apiKey\":\"b\"}", "{\"apiKey\":\"a\",\"extra\":true}", new string('x', 16385) })
        {
            await File.WriteAllTextAsync(config, invalid);
            Invalid(new(null, config));
        }
        var original=Environment.GetEnvironmentVariable("SH_CURSEFORGE_API_KEY");
        var originalFile=Environment.GetEnvironmentVariable("SH_CURSEFORGE_CONFIG_FILE");
        Environment.SetEnvironmentVariable("SH_CURSEFORGE_API_KEY","unit-test-placeholder",EnvironmentVariableTarget.Process);
        Environment.SetEnvironmentVariable("SH_CURSEFORGE_CONFIG_FILE",null,EnvironmentVariableTarget.Process);
        var root=new DirectoryInfo(AppContext.BaseDirectory);
        while(root is not null&&!File.Exists(Path.Combine(root.FullName,"SHLauncherNext.sln")))root=root.Parent;
        try
        {
            await using var owner=new LauncherComposition(Path.Combine(root!.FullName,".devdata","credential-tests",Guid.NewGuid().ToString("N")));
            if(Environment.GetEnvironmentVariable("SH_CURSEFORGE_API_KEY") is not null)throw new Exception("Own credential left inheritable by Java child processes.");
            Check(Environment.GetEnvironmentVariable("SH_CURSEFORGE_CONFIG_FILE") is null);
            var method=typeof(LauncherComposition).GetMethod("CurseForgeHeaders",BindingFlags.NonPublic|BindingFlags.Instance)!;
            IReadOnlyDictionary<string,string> Headers(string url)=>(IReadOnlyDictionary<string,string>)method.Invoke(owner,[new Uri(url)])!;
            if(!Headers("https://api.curseforge.com/v1/inert").ContainsKey("x-api-key")||!Headers("https://edge.forgecdn.net/inert").ContainsKey("x-api-key"))
                throw new Exception("Own provider credential not scoped to documented API/CDN.");
            foreach(var uri in new[]{"https://evil.test/","https://api.curseforge.com.evil.test/","http://api.curseforge.com/","https://api.curseforge.com:444/"})
                if(Headers(uri).Count!=0)throw new Exception("Credential would be forwarded outside approved secure origin.");
            var start = new ProcessStartInfo(Environment.ProcessPath!) { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true };
            if (string.Equals(Path.GetFileNameWithoutExtension(Environment.ProcessPath), "dotnet", StringComparison.OrdinalIgnoreCase))
                start.ArgumentList.Add(Assembly.GetExecutingAssembly().Location);
            start.ArgumentList.Add("credential-environment-probe");
            using var child = Process.Start(start)!;
            var output = child.StandardOutput.ReadToEndAsync();
            var errors = child.StandardError.ReadToEndAsync();
            await child.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(15));
            Check(child.ExitCode == 0 && (await output).Trim() == "Credential environment clear" && (await errors).Length == 0);
            foreach(var mode in new[]{"explicit","default","invalid"})
            {
                var probeRoot=Path.Combine(work,"probe-"+mode);
                var file=mode=="default"?Path.Combine(probeRoot,"settings","curseforge.local.json"):config;
                Directory.CreateDirectory(Path.GetDirectoryName(file)!);
                await File.WriteAllTextAsync(file,mode=="invalid"?"{\"apiKey\":\""+marker:JsonSerializer.Serialize(new{apiKey=marker}));
                var probe=new ProcessStartInfo(Environment.ProcessPath!){UseShellExecute=false,RedirectStandardOutput=true,RedirectStandardError=true,CreateNoWindow=true};
                if(string.Equals(Path.GetFileNameWithoutExtension(Environment.ProcessPath),"dotnet",StringComparison.OrdinalIgnoreCase))probe.ArgumentList.Add(Assembly.GetExecutingAssembly().Location);
                probe.ArgumentList.Add("credential-config-probe");probe.ArgumentList.Add(probeRoot);probe.ArgumentList.Add(mode=="invalid"?"invalid":"valid");
                probe.Environment.Remove("SH_CURSEFORGE_API_KEY");
                if(mode=="default")probe.Environment.Remove("SH_CURSEFORGE_CONFIG_FILE");else probe.Environment["SH_CURSEFORGE_CONFIG_FILE"]=file;
                using var process=Process.Start(probe)!;var text=process.StandardOutput.ReadToEndAsync();var stderr=process.StandardError.ReadToEndAsync();
                await process.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(15));
                Check(process.ExitCode==0&&(await text).Trim()=="Local configuration probe passed"&&(await stderr).Length==0);
            }
            Console.WriteLine("PASS: real child process inherits neither provider startup setting; no provider network requests.");
            Console.WriteLine($"PASS: {checks} local credential checks; JSON/BOM/schema/budget, sanitized errors and origin scope. Synthetic credentials only.");
        }
        finally
        {
            Environment.SetEnvironmentVariable("SH_CURSEFORGE_API_KEY",original,EnvironmentVariableTarget.Process);
            Environment.SetEnvironmentVariable("SH_CURSEFORGE_CONFIG_FILE",originalFile,EnvironmentVariableTarget.Process);
        }
    }
    internal static async Task Probe(string dataRoot,string expected)
    {
        await using var owner=new LauncherComposition(dataRoot);
        if(new[]{"SH_CURSEFORGE_API_KEY","SH_CURSEFORGE_CONFIG_FILE"}.Any(k=>Environment.GetEnvironmentVariable(k) is not null))throw new Exception("Provider settings remain inheritable.");
        var method=typeof(LauncherComposition).GetMethod("HasCurseForgeAccess",BindingFlags.NonPublic|BindingFlags.Instance)!;
        var result=false;
        try{result=(bool)method.Invoke(owner,null)!;}
        catch(TargetInvocationException error) when(expected=="invalid"&&error.InnerException is InstallationPreconditionException{Code:"ProviderAccessInvalid"})
        {Console.WriteLine("Local configuration probe passed");return;}
        if(expected!="valid"||!result)throw new Exception("Unexpected local provider configuration result.");
        Console.WriteLine("Local configuration probe passed");
    }
}
