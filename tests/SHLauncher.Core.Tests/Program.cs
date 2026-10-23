using SHLauncher.Core.Launching;

ArchitectureChecks.CheckProject("SHLauncher.Core");
ArchitectureChecks.CheckProject("SHLauncher.Contracts", "SHLauncher.Core");
var permitted = new[] { "Idle>Preparing", "Preparing>Launching", "Preparing>Cancelling", "Preparing>Failed", "Launching>Running", "Launching>Cancelling", "Launching>Failed", "Running>Idle", "Running>Stopping", "Running>Failed", "Stopping>Idle", "Cancelling>Idle", "Failed>Idle", "Failed>Preparing" };
var checkedPairs = 0;
foreach (var from in Enum.GetValues<LauncherTaskState>())
foreach (var to in Enum.GetValues<LauncherTaskState>())
{
    var expected = permitted.Contains($"{from}>{to}");
    if (LauncherTransitions.CanMove(from, to) != expected) throw new Exception($"Transition policy mismatch: {from}->{to}");
    if (!expected)
    {
        try { LauncherTransitions.Ensure(from, to); throw new Exception("Illegal transition accepted"); }
        catch (InvalidOperationException) { }
    }
    checkedPairs++;
}
Console.WriteLine($"PASS: Core architecture and {checkedPairs} state pairs");
