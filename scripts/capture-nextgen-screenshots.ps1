param(
  [string]$OutDir = "docs/evidence/nextgen/ui-after-v2"
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class Win32Shot {
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
  public static List<IntPtr> VisibleWindows(uint pid) {
    var result = new List<IntPtr>();
    EnumWindows((hWnd, lParam) => {
      uint owner;
      GetWindowThreadProcessId(hWnd, out owner);
      if (owner == pid && IsWindowVisible(hWnd)) result.Add(hWnd);
      return true;
    }, IntPtr.Zero);
    return result;
  }
}
"@

function Get-TargetWindow([string]$Kind) {
  $proc = Get-Process -Name "app" -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $proc) { Write-Error "app process not found"; return $null }
  $handles = [Win32Shot]::VisibleWindows([uint32]$proc.Id)
  foreach ($handle in $handles) {
    $rect = New-Object Win32Shot+RECT
    [Win32Shot]::GetWindowRect($handle, [ref]$rect) | Out-Null
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($Kind -eq "main" -and $width -ge 1000) { return @{ Handle = $handle; Rect = $rect } }
    if ($Kind -eq "splash" -and $width -lt 600) { return @{ Handle = $handle; Rect = $rect } }
  }
  return $null
}

function Save-WindowScreenshot([string]$Name, [string]$Kind = "main") {
  $target = Get-TargetWindow $Kind
  if (-not $target) { Write-Error "window not found for $Name ($Kind)"; return $false }
  [Win32Shot]::SetForegroundWindow($target.Handle) | Out-Null
  Start-Sleep -Milliseconds 350
  $rect = $target.Rect
  $width = $rect.Right - $rect.Left
  $height = $rect.Bottom - $rect.Top
  $bmp = New-Object System.Drawing.Bitmap($width, $height)
  $graphics = [System.Drawing.Graphics]::FromImage($bmp)
  $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bmp.Size)
  $path = Join-Path $OutDir "$Name.png"
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $graphics.Dispose(); $bmp.Dispose()
  Write-Output "saved $path ($width x $height)"
  return $true
}

function Invoke-ByName([string]$Name) {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $condition = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::NameProperty, $Name)
  $element = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
  if (-not $element) { Write-Output "UIA element not found: $Name"; return $false }
  try {
    $pattern = $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $pattern.Invoke()
    return $true
  } catch {
    Write-Output "cannot invoke $Name : $_"
    return $false
  }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$exe = "src-tauri/target/release/app.exe"
if (-not (Test-Path $exe)) { Write-Error "missing $exe"; exit 1 }

$existing = Get-Process -Name "app" -ErrorAction SilentlyContinue
if ($existing) { $existing | Stop-Process -Force; Start-Sleep -Seconds 1 }

$proc = Start-Process -FilePath (Resolve-Path $exe) -PassThru
Start-Sleep -Seconds 1
Save-WindowScreenshot "splash" "splash" | Out-Null
Start-Sleep -Seconds 9
Save-WindowScreenshot "home" | Out-Null

$nav = @{
  "library" = "游戏库"
  "discover" = "发现"
  "downloads" = "下载"
  "accounts" = "账户"
  "settings" = "设置"
}
foreach ($entry in $nav.GetEnumerator()) {
  if (Invoke-ByName $entry.Value) {
    Start-Sleep -Milliseconds 900
    Save-WindowScreenshot $entry.Key | Out-Null
  }
}

if (Invoke-ByName "使用教程") {
  Start-Sleep -Milliseconds 700
  Save-WindowScreenshot "home-tutorial" | Out-Null
  Invoke-ByName "我知道了" | Out-Null
  Start-Sleep -Milliseconds 500
}

Write-Output "done: $OutDir"
