using System.Diagnostics;
using SHLauncher.Contracts.Runtime;
namespace SHLauncher.Platform;
public sealed class ProcessMemory:IProcessMemory{public long WorkingSetBytes(){using var process=Process.GetCurrentProcess();process.Refresh();return process.WorkingSet64;}}
