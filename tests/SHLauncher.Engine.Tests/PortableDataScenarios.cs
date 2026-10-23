using SHLauncher.Infrastructure.Runtime;
internal static class PortableDataScenarios
{
    internal static void Run()
    {
        var previous=Environment.GetEnvironmentVariable("SHLAUNCHER_DATA_DIR");
        var marker=Path.Combine(AppContext.BaseDirectory,"portable.flag");var created=false;
        try
        {
            Environment.SetEnvironmentVariable("SHLAUNCHER_DATA_DIR",null);
            if(!File.Exists(marker)){File.WriteAllText(marker,"portable test");created=true;}
            if(ApplicationDataPaths.Resolve()!=Path.Combine(AppContext.BaseDirectory,"data"))throw new Exception("Portable data did not stay next to app");
            var configured=Path.Combine(AppContext.BaseDirectory,"custom-data");Environment.SetEnvironmentVariable("SHLAUNCHER_DATA_DIR",configured);
            if(ApplicationDataPaths.Resolve()!=configured)throw new Exception("Explicit data path lost precedence");
            Environment.SetEnvironmentVariable("SHLAUNCHER_DATA_DIR","relative");
            try{ApplicationDataPaths.Resolve();throw new Exception("Relative data path accepted");}catch(IOException){}
            Console.WriteLine("PASS portable data stays beside app; absolute override wins; relative override rejected.");
        }
        finally
        {
            Environment.SetEnvironmentVariable("SHLAUNCHER_DATA_DIR",previous);
            if(created)File.Move(marker,Path.Combine(AppContext.BaseDirectory,"portable-fixture-"+Guid.NewGuid().ToString("N")+".txt"));
        }
    }
}
