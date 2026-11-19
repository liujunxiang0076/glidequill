# Run after cargo build --release. Only send messages to the test instance.
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path (Join-Path $PSScriptRoot '../target/release/glidequill.exe')).Path
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class GestureSmoke {
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int max);
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
function Require($condition,$message) { if (!$condition) { throw $message } }
Require (!(Get-Process glidequill -ErrorAction SilentlyContinue)) 'Exit an existing GlideQuill instance before testing.'
$root = Join-Path ([IO.Path]::GetTempPath()) ('glidequill-test-' + [guid]::NewGuid())
$start = [Diagnostics.ProcessStartInfo]::new($exe)
$start.UseShellExecute = $false
$start.EnvironmentVariables['LOCALAPPDATA'] = $root
function Start-TestApp {
    $p = [Diagnostics.Process]::Start($start)
    [void]$p.WaitForInputIdle(5000)
    $p.Refresh()
    Require ($p.MainWindowHandle -ne 0) 'Missing native window'
    $p
}
$p = Start-TestApp
try {
    $w=$p.MainWindowHandle
    Require ((Get-Label $w 121) -eq '启用全局手势') 'Unexpected startup hook state'
    $pad=[GestureSmoke]::GetDlgItem($w,125)
    [void](Send $pad 0x204 2 (30*65536+120))
    [void](Send $pad 0x200 2 (30*65536+55))
    [void](Send $pad 0x205 0 (30*65536+30))
    Require ((Get-Label $w 122) -like '*向左*后退*') 'Practice recognition failed'
    [void](Send $w 0x111 126)
    $up=[GestureSmoke]::GetDlgItem($w,112)
    [void](Send $up 0x14e 6)
    [void](Send $w 0x111 120)
    $cfg=Get-Content (Join-Path $root 'GlideQuill/rules.json') -Encoding utf8 -Raw | ConvertFrom-Json
    Require ($cfg.browser[2] -eq 'Home') 'Save failed'
    [void](Send $w 0x111 127)
    Require ((Get-Label $w 127) -eq '浅色') 'Theme failed'
    [void](Send $w 0x10)
    Require (![GestureSmoke]::IsWindowVisible($w)) 'Tray hide failed'
    $second=[Diagnostics.Process]::Start($start)
    Require ($second.WaitForExit(5000)) 'Duplicate instance'
    Require ([GestureSmoke]::IsWindowVisible($w)) 'Reopen failed'
    [void](Send $w 0x111 124)
    Require ($p.WaitForExit(5000)) 'Tray exit failed'
    $p=Start-TestApp
    $w=$p.MainWindowHandle
    [void](Send $w 0x111 126)
    $up=[GestureSmoke]::GetDlgItem($w,112)
    Require ((Send $up 0x147).ToInt32() -eq 6) 'Reload failed'
    [void](Send $w 0x111 124)
    Require ($p.WaitForExit(5000)) 'Final exit failed'
    Write-Output 'PASS: startup, practice, browser override, save/reload, theme, tray, single-instance reopen and exit.'
} finally {
    if (!$p.HasExited) { $p.Kill(); $p.WaitForExit() }
    $tempRoot=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ((Split-Path $root -Parent) -eq $tempRoot -and (Split-Path $root -Leaf) -like 'glidequill-test-*' -and (Test-Path $root)) {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
