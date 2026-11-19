# Run after cargo build --release. Only send messages to the test instance.
param([string]$Executable, [string]$ScreenshotDirectory)
$ErrorActionPreference = 'Stop'
if (!$Executable) { $Executable = Join-Path $PSScriptRoot '../target/release/glidequill.exe' }
$exe = (Resolve-Path $Executable).Path
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class GestureSmoke {
    [DllImport("user32.dll")] public static extern IntPtr GetWindowDpiAwarenessContext(IntPtr h);
    [DllImport("user32.dll")] public static extern bool AreDpiAwarenessContextsEqual(IntPtr a,IntPtr b);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Bounds r);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h,uint attr,out Bounds r,int size);
    public struct Bounds {public int Left,Top,Right,Bottom;}
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int max);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
"@
function Send($handle, $message, $wp=0, $lp=0) {
    [GestureSmoke]::SendMessage($handle,$message,[IntPtr]$wp,[IntPtr]$lp)
}
function Get-Label($window, $id) {
    $text = [Text.StringBuilder]::new(256)
    [void][GestureSmoke]::GetWindowText([GestureSmoke]::GetDlgItem($window,$id),$text,256)
    $text.ToString()
}
function Save-Screenshot($w,$name) {
    if (!$ScreenshotDirectory) { return }
    Start-Sleep -Milliseconds 250
    Add-Type -AssemblyName System.Drawing
    $r = [GestureSmoke+Bounds]::new()
    $previous = [GestureSmoke]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try { Require ([GestureSmoke]::GetWindowRect($w,[ref]$r)) 'Missing physical bounds' }
    finally { [void][GestureSmoke]::SetThreadDpiAwarenessContext($previous) }
    $bmp = [Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $dc = $g.GetHdc()
    try { Require ([GestureSmoke]::PrintWindow($w,$dc,2)) 'Window capture failed' }
    finally { $g.ReleaseHdc($dc); $g.Dispose() }
    # The Win32 window rect includes an invisible resize margin outside the DWM frame.
    $frame = [GestureSmoke+Bounds]::new()
    $visible = [Drawing.Rectangle]::new(0,0,$bmp.Width,$bmp.Height)
    if ([GestureSmoke]::DwmGetWindowAttribute($w,9,[ref]$frame,16) -eq 0) {
        $visible = [Drawing.Rectangle]::Intersect($visible,[Drawing.Rectangle]::FromLTRB(
            $frame.Left-$r.Left,$frame.Top-$r.Top,$frame.Right-$r.Left,$frame.Bottom-$r.Top))
    }
    try {
        $cropped = $bmp.Clone($visible,$bmp.PixelFormat)
        try { $cropped.Save((Join-Path $ScreenshotDirectory $name),[Drawing.Imaging.ImageFormat]::Png) }
        finally { $cropped.Dispose() }
    }
    finally { $bmp.Dispose() }
}
function Require($condition,$message) { if (!$condition) { throw $message } }

$root = Join-Path ([IO.Path]::GetTempPath()) ('glidequill-test-' + [guid]::NewGuid())
$start = [Diagnostics.ProcessStartInfo]::new($exe)
$start.UseShellExecute = $false
$start.EnvironmentVariables['GLIDEQUILL_UI_TEST_ID'] = [guid]::NewGuid().ToString()
$start.EnvironmentVariables['LOCALAPPDATA'] = $root
function Start-TestApp {
    $p = [Diagnostics.Process]::Start($start)
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    while (!$p.HasExited -and $deadline.ElapsedMilliseconds -lt 5000) {
        $p.Refresh()
        if ($p.MainWindowHandle -ne 0) { return $p }
        Start-Sleep -Milliseconds 50
    }
    if (!$p.HasExited) { $p.Kill(); $p.WaitForExit() }
    throw 'Missing native window'
}
$legacyDir = Join-Path $root 'GlideQuill'
[void][IO.Directory]::CreateDirectory($legacyDir)
$legacyFile = Join-Path $legacyDir 'rules.json'
$legacy = '{"global":["Close","Minimize","Paste","Copy"],"browser":["Home","End","Off","Off"],"excluded":["blocked.exe"]}'
[IO.File]::WriteAllText($legacyFile,$legacy)
$p = Start-TestApp
try {
    $w=$p.MainWindowHandle
    Require ((Get-Label $w 121) -eq '启用全局手势') 'Unexpected startup hook state'
    Require ([GestureSmoke]::AreDpiAwarenessContextsEqual([GestureSmoke]::GetWindowDpiAwarenessContext($w), [IntPtr](-4))) 'Window must render at per-monitor DPI v2'
    $expected = @('向左划 · 后退','向右划 · 前进','向上划 · 页面顶部','向下划 · 页面底部')
    foreach ($i in 0..3) { Require ((Get-Label $w (140+$i)) -eq $expected[$i]) 'Wrong preset' }
    foreach ($id in @(110,120,125,126)) {
        Require ([GestureSmoke]::GetDlgItem($w,$id) -eq [IntPtr]::Zero) 'Obsolete editor control'
    }
    Write-Output ("Window DPI: {0}" -f [GestureSmoke]::GetDpiForWindow($w))
    Save-Screenshot $w 'windows-light.png'
    # The isolated instance installs the real hook but always forwards mouse events.
    foreach ($round in 1..3) {
        [void](Send $w 0x111 121)
        Require ((Get-Label $w 121) -eq '暂停全局手势') 'Listener did not start'
        [void](Send $w 0x111 121)
        Require ((Get-Label $w 121) -eq '启用全局手势') 'Listener did not stop'
    }
    [void](Send $w 0x111 121)
    [void](Send $w 0x111 127)
    Require ((Get-Label $w 127) -eq '浅色') 'Theme failed'
    Save-Screenshot $w 'windows-dark.png'
    [void](Send $w 0x10)
    Require (![GestureSmoke]::IsWindowVisible($w)) 'Tray hide failed'
    Require ((Get-Label $w 121) -eq '暂停全局手势') 'Hiding the window stopped the listener'
    $p.Refresh()
    Write-Output ("Hidden-window process snapshot: working set {0:N1} MiB, private bytes {1:N1} MiB." -f ($p.WorkingSet64/1MB),($p.PrivateMemorySize64/1MB))
    $second=[Diagnostics.Process]::Start($start)
    Require ($second.WaitForExit(5000)) 'Duplicate instance'
    Require ([GestureSmoke]::IsWindowVisible($w)) 'Reopen failed'
    [void](Send $w 0x111 124)
    Require ($p.WaitForExit(5000)) 'Tray exit failed'
    Require ((Get-Content -Raw -LiteralPath $legacyFile) -ceq $legacy) 'Legacy rules changed'
    Write-Output 'PASS: four read-only rules, preview switch, theme, tray, single-instance and legacy config preservation.'
} finally {
    if (!$p.HasExited) { $p.Kill(); $p.WaitForExit() }
    $tempRoot=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ((Split-Path $root -Parent) -eq $tempRoot -and (Split-Path $root -Leaf) -like 'glidequill-test-*' -and (Test-Path $root)) {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
