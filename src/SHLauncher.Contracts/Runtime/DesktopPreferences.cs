namespace SHLauncher.Contracts.Runtime;
public enum DesktopCloseAction{Ask,Exit,Tray}
public sealed record DesktopPreferences(DesktopCloseAction CloseAction=DesktopCloseAction.Ask,bool TrayNoticeSeen=false);
public interface IDesktopPreferences{DesktopPreferences Read();void Save(DesktopPreferences value);}
public interface IProcessMemory{long WorkingSetBytes();}
