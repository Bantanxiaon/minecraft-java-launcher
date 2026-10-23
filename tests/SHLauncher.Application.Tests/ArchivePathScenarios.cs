using System.IO.Compression;
using SHLauncher.Infrastructure.Install;
internal static class ArchivePathScenarios
{
    internal static async Task Run()
    {
        foreach(var bad in new[]{"../escape","a/../../escape","/absolute","C:/escape","C:relative","\\\\server/share","a/./b","a//b","CON.txt","a/NUL","a/b:ads","a/b. "})
        {
            var rejected=false;try{ArchivePathPolicy.NormalizeRelative(bad);}catch(InvalidDataException){rejected=true;}
            if(!rejected)throw new Exception("Unsafe archive path accepted: "+bad);
        }
        if(ArchivePathPolicy.NormalizeRelative("config/中文 名称.toml")!="config/中文 名称.toml")throw new Exception("Unicode path changed.");
        var root=Path.Combine(ArchitectureChecks.Root(),".devdata","archive-path-tests",Guid.NewGuid().ToString("N"));Directory.CreateDirectory(root);
        var file=Path.Combine(root,"traversal.zip");
        using(var zip=ZipFile.Open(file,ZipArchiveMode.Create))using(var writer=new StreamWriter(zip.CreateEntry("../escape").Open()))writer.Write("bad");
        var invalid=false;
        try{await new CurseForgePackInspector().InspectAsync(file,default);}catch(InvalidDataException){invalid=true;}
        if(!invalid||Directory.GetFiles(root).Length!=1)throw new Exception("Preview must reject unsafe archive without extraction.");
        var target=Path.Combine(root,"owned-target");Directory.CreateDirectory(target);var link=Path.Combine(root,"owned-link");
        try
        {
            Directory.CreateSymbolicLink(link,target);var linkRejected=false;
            try{ArchivePathPolicy.Resolve(root,"owned-link");}catch(InvalidDataException){linkRejected=true;}
            if(!linkRejected)throw new Exception("Directory link target itself accepted.");
            Console.WriteLine("PASS: native directory-link target itself rejected before launch/write.");
        }
        catch(Exception error)when(error is UnauthorizedAccessException||error is IOException&& !Directory.Exists(link))
        {Console.WriteLine("NOT VERIFIED: native symbolic-link creation unavailable in this test environment; no system setting changed.");}
        Console.WriteLine("PASS: archive path traversal/absolute/UNC/ADS/reserved names rejected; Unicode kept; preview creates no files.");
    }
}
