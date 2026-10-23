using SHLauncher.UI.Presentation;
internal static class AsyncCommandScenarios
{
    internal static async Task Run()
    {
        var finish=new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var started=0;var errors=0;
        var command=new AsyncCommand(async()=>{Interlocked.Increment(ref started);await finish.Task;},()=>true,_=>Interlocked.Increment(ref errors));
        command.Execute(null);var drain=command.DrainAsync();
        if(drain.IsCompleted)throw new Exception("Command drain returned before asynchronous setting write.");
        command.Execute(null);if(started!=2)throw new Exception("Command tracking must not disable game Stop re-entry.");
        finish.SetResult();await command.DrainAsync();await drain;
        if(errors!=0)throw new Exception("Unexpected command error.");
        var failing=new AsyncCommand(()=>throw new IOException("simulated setting failure"),()=>true,_=>Interlocked.Increment(ref errors));
        failing.Execute(null);await failing.DrainAsync();
        if(errors!=1)throw new Exception("Error must be surfaced once before drain completes.");
        Console.WriteLine("PASS: UI command drain waits writes, preserves Stop re-entry and reports failures once.");
    }
}
