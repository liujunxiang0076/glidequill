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
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls,string caption);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowDpiAwarenessContext(IntPtr h);
    [DllImport("user32.dll")] public static extern bool AreDpiAwarenessContextsEqual(IntPtr a,IntPtr b);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Bounds r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr w,IntPtr after,int x,int y,int width,int height,uint flags);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Bounds r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref Point pt);
    [DllImport("user32.dll")] public static extern bool GetPhysicalCursorPos(out Point pt);
    [DllImport("user32.dll")] public static extern bool SetPhysicalCursorPos(int x,int y);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point pt);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h,uint flags);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool GetMonitorInfo(IntPtr m,ref MonitorInfo info);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode)] public static extern uint ExtractIconEx(string file,int index,IntPtr big,IntPtr small,uint count);
    public struct MonitorInfo {public int Size; public Bounds Monitor,Work; public int Flags;}
    public struct Point {public int X,Y;}
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
$testId = [guid]::NewGuid().ToString()
$start.EnvironmentVariables['GLIDEQUILL_UI_TEST_ID'] = $testId
$testKey = "HKCU:\Software\GlideQuill\Tests\$testId"
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
$prefsFile=Join-Path $legacyDir 'settings.json'
[IO.File]::WriteAllText($prefsFile,'{"enabled":false,"dark":false,"idle_ms":1000,"result_ms":550,"bindings":["back","forward","home","end","paste","unassigned","unassigned","unassigned"]}')
$p = Start-TestApp
try {
    $w=$p.MainWindowHandle
    Require ((Get-Label $w 121) -eq '启用全局手势') 'Unexpected startup hook state'
    Require ([GestureSmoke]::AreDpiAwarenessContextsEqual([GestureSmoke]::GetWindowDpiAwarenessContext($w), [IntPtr](-4))) 'Window must render at per-monitor DPI v2'
    Require ($p.MainWindowTitle -eq 'GlideQuill · 设置') 'Missing product name'
    Require ((Send $w 0x7f 0) -ne 0 -and (Send $w 0x7f 1) -ne 0) 'Missing window icons'
    Require ([GestureSmoke]::ExtractIconEx($exe,-1,[IntPtr]::Zero,[IntPtr]::Zero,0) -gt 0) 'Missing executable icon'
    $info=[GestureSmoke+MonitorInfo]::new()
    $info.Size=40
    $frame=[GestureSmoke+Bounds]::new()
    $prior=[GestureSmoke]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try {
        Require ([GestureSmoke]::GetMonitorInfo([GestureSmoke]::MonitorFromWindow($w,2),[ref]$info)) 'No monitor'
        Require ([GestureSmoke]::GetWindowRect($w,[ref]$frame)) 'No window bounds'
        Require ([Math]::Abs($frame.Left+$frame.Right-$info.Work.Left-$info.Work.Right) -le 1) 'Window not centered horizontally'
        Require ([Math]::Abs($frame.Top+$frame.Bottom-$info.Work.Top-$info.Work.Bottom) -le 1) 'Window not centered vertically'
    } finally { [void][GestureSmoke]::SetThreadDpiAwarenessContext($prior) }
    # A compact client area must keep both rule columns and footer actions accessible.
    $prior=[GestureSmoke]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $scale=[GestureSmoke]::GetDpiForWindow($w)/96.0
    try {
        $sw=$frame.Right-$frame.Left
        $sh=$frame.Bottom-$frame.Top
        Require ([GestureSmoke]::SetWindowPos($w,[IntPtr]::Zero,0,0,[int](630*$scale),[int](395*$scale),0x16)) 'Compact resize failed'
        $client=[GestureSmoke+Bounds]::new()
        [void][GestureSmoke]::GetClientRect($w,[ref]$client)
        $corner=[GestureSmoke+Point]::new()
        [void][GestureSmoke]::ClientToScreen($w,[ref]$corner)
        foreach($id in @(136,137,160,163,164,167)){
            $bounds=[GestureSmoke+Bounds]::new()
            [void][GestureSmoke]::GetWindowRect([GestureSmoke]::GetDlgItem($w,$id),[ref]$bounds)
            Require ($bounds.Left -ge $corner.X -and $bounds.Top -ge $corner.Y -and $bounds.Right -le $corner.X+$client.Right -and $bounds.Bottom -le $corner.Y+$client.Bottom) "Compact layout clipped control $id"
        }
    } finally {
        [void][GestureSmoke]::SetWindowPos($w,[IntPtr]::Zero,0,0,$sw,$sh,0x16)
        [void][GestureSmoke]::SetThreadDpiAwarenessContext($prior)
    }
    $names = @('向左','向右','向上','向下','左上','右上','左下','右下')
    $defaults = @(1,2,3,4,6,0,0,0)
    foreach ($i in 0..7) {
        Require ((Get-Label $w (140+$i)) -eq $names[$i]) 'Wrong direction'
        $field=[GestureSmoke]::GetDlgItem($w,(160+$i))
        Require ((Send $field 0x146).ToInt32() -eq 10) 'Missing choices'
        Require ((Send $field 0x147).ToInt32() -eq $defaults[$i]) 'Wrong default action'
    }
    foreach ($id in @(110,120,125,126)) {
        Require ([GestureSmoke]::GetDlgItem($w,$id) -eq [IntPtr]::Zero) 'Obsolete editor control'
    }
    Write-Output ("Window DPI: {0}" -f [GestureSmoke]::GetDpiForWindow($w))
    $apply=[GestureSmoke]::GetDlgItem($w,137)
    $reset=[GestureSmoke]::GetDlgItem($w,136)
    Require (![GestureSmoke]::IsWindowEnabled($apply)) 'Clean draft enabled Save'
    $before=[IO.File]::ReadAllText($prefsFile)
    $diagonal=[GestureSmoke]::GetDlgItem($w,165)
    [void](Send $diagonal 0x14f 1)
    Require ((Send $diagonal 0x157).ToInt32() -ne 0) 'Dropdown did not open'
    foreach ($step in 1..5) { [void](Send $diagonal 0x100 0x28) }
    [void](Send $diagonal 0x100 0x0d)
    Require ([GestureSmoke]::IsWindowEnabled($apply)) 'Editing did not enable Save'
    Require ([GestureSmoke]::IsWindowEnabled($reset)) 'Edited defaults cannot be reset'
    Require ([IO.File]::ReadAllText($prefsFile) -ceq $before) 'Selection wrote preferences before Save'
    [void](Send $w 0x111 130)
    Require (![GestureSmoke]::IsWindowVisible($apply)) 'Rules button shown in General'
    [void](Send $w 0x111 134)
    Require ((Send $diagonal 0x147).ToInt32() -eq 5) 'Page switch lost draft'
    [void](Send ([GestureSmoke]::GetDlgItem($w,139)) 0xf5)
    $turn=[GestureSmoke]::GetDlgItem($w,173)
    Require ([GestureSmoke]::IsWindowVisible($turn) -and ![GestureSmoke]::IsWindowVisible($diagonal)) 'Wrong gesture group'
    Require (![GestureSmoke]::IsWindowEnabled($reset)) 'Other group affected special reset'
    [void](Send $turn 0x14e 6)
    [void](Send $w 0x111 (0x10000+173) $turn)
    Save-Screenshot $w 'windows-special.png'
    [void](Send ([GestureSmoke]::GetDlgItem($w,138)) 0xf5)
    # Simulate a write failure, then retry without re-entering the draft.
    $lock=[IO.File]::Open($prefsFile,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    try { [void](Send $apply 0xf5) } finally { $lock.Dispose() }
    Require ((Get-Label $w 122) -like '保存失败*' -and [GestureSmoke]::IsWindowEnabled($apply)) 'Failed write discarded draft'
    Require ([IO.File]::ReadAllText($prefsFile) -ceq $before) 'Failed write changed preferences'
    [void](Send $apply 0xf5)
    Require (![GestureSmoke]::IsWindowEnabled($apply)) 'Save left the draft dirty'
    $saved=Get-Content $prefsFile -Raw | ConvertFrom-Json
    Require ($saved.bindings[5] -eq 'copy' -and $saved.bindings[4] -eq 'paste' -and $saved.bindings[13] -eq 'paste' -and $saved.idle_ms -eq 1000) 'Keyboard selection or preference preservation failed'
    # Press the hover clear target; the chevron and keyboard still open the native list.
    $dpi=[GestureSmoke]::GetDpiForWindow($diagonal)
    $rect=[GestureSmoke+Bounds]::new()
    $target=[GestureSmoke+Point]::new()
    $original=[GestureSmoke+Point]::new()
    $prior=[GestureSmoke]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try {
        Require ([GestureSmoke]::GetClientRect($diagonal,[ref]$rect)) 'No picker bounds'
        $target.X=$rect.Right-[int](34*$dpi/96)
        $target.Y=[int]($rect.Bottom/2)
        $point=($target.Y -shl 16) -bor $target.X
        Require ([GestureSmoke]::ClientToScreen($diagonal,[ref]$target)) 'No screen position'
        Require ([GestureSmoke]::GetPhysicalCursorPos([ref]$original)) 'Missing initial cursor'
        try {
            Require ([GestureSmoke]::SetPhysicalCursorPos($target.X,$target.Y)) 'Cannot hover picker'
            Start-Sleep -Milliseconds 120
            Require ([GestureSmoke]::WindowFromPoint($target) -eq $diagonal) 'Picker is covered during hover test'
            [void](Send $diagonal 0x200 0 $point)
            Save-Screenshot $w 'windows-light.png'
            [void](Send $diagonal 0x201 1 $point)
            [void](Send $diagonal 0x202 0 0)
            Require ((Send $diagonal 0x147).ToInt32() -eq 5) 'Releasing outside the clear target cleared a binding'
            [void](Send $diagonal 0x201 1 $point)
            [void](Send $diagonal 0x202 0 $point)
        } finally { [void][GestureSmoke]::SetPhysicalCursorPos($original.X,$original.Y) }
    } finally {
        [void][GestureSmoke]::SetThreadDpiAwarenessContext($prior)
    }
    $choice=(Send $diagonal 0x147).ToInt32()
    $dropped=(Send $diagonal 0x157).ToInt32()
    Require ($choice -eq 0 -and $dropped -eq 0) "Hover clear failed (selection $choice, dropdown $dropped, DPI $dpi, bounds $($rect.Right)x$($rect.Bottom), click $point)"
    Require ((Get-Content $prefsFile -Raw | ConvertFrom-Json).bindings[5] -eq 'copy') 'Hover clear applied without Save'
    [void](Send $apply 0xf5)
    Require ((Get-Content $prefsFile -Raw | ConvertFrom-Json).bindings[5] -eq 'unassigned') 'Cleared binding not committed'
    # Save a custom binding again, then keep a factory-reset draft open across unrelated edits.
    [void](Send $diagonal 0x14e 5)
    [void](Send $w 0x111 (0x10000+165) $diagonal)
    [void](Send $apply 0xf5)
    [void](Send $reset 0xf5)
    Require ((Send $diagonal 0x147).ToInt32() -eq 0 -and [GestureSmoke]::IsWindowEnabled($apply)) 'Reset did not stage defaults'
    Require ((Get-Content $prefsFile -Raw | ConvertFrom-Json).bindings[5] -eq 'copy') 'Reset applied without Save'
    [void](Send $w 0x111 130)
    Require ([GestureSmoke]::IsWindowVisible([GestureSmoke]::GetDlgItem($w,131))) 'Missing general settings'
    Require (![GestureSmoke]::IsWindowVisible([GestureSmoke]::GetDlgItem($w,132))) 'Timing control visible in General'
    Require ((Get-Label $w 131) -eq '启用登录自启') 'Autostart should default off'
    Save-Screenshot $w 'windows-settings.png'
    [void](Send $w 0x111 131)
    Require ((Get-ItemPropertyValue -LiteralPath $testKey -Name GlideQuill) -ceq ('"'+$exe+'" --startup')) 'Bad logon command'
    [void](Send $w 0x111 135)
    Require ([GestureSmoke]::IsWindowVisible([GestureSmoke]::GetDlgItem($w,132))) 'Missing timing control'
    Require (![GestureSmoke]::IsWindowVisible([GestureSmoke]::GetDlgItem($w,140))) 'Rule visible in Feedback'
    [void](Send $w 0x111 203)
    [void](Send $w 0x111 211)
    Require ((Get-Label $w 132) -like '*2 秒') 'Idle setting not applied'
    Require ((Get-Label $w 133) -like '*0.25 秒') 'Result setting not applied'
    [void](Send $w 0x111 130)
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
    [void](Send $w 0x111 134)
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
    $start.Arguments='--startup'
    $p=[Diagnostics.Process]::Start($start)
    $deadline=[Diagnostics.Stopwatch]::StartNew()
    do {
        $w=[GestureSmoke]::FindWindow("GlideQuill.Rules.0.1.test.$testId","GlideQuill · 设置")
        if ($w -ne 0 -and (Get-Label $w 121) -eq '暂停全局手势') {break}
        Start-Sleep -Milliseconds 40
    } while (!$p.HasExited -and $deadline.ElapsedMilliseconds -lt 5000)
    Require ($w -ne 0 -and (Get-Label $w 121) -eq '暂停全局手势') 'Enabled state not restored'
    Require (![GestureSmoke]::IsWindowVisible($w)) 'Logon launched a visible window'
    [void](Send $w 0x111 123)
    [void](Send $w 0x111 130)
    Require ((Send ([GestureSmoke]::GetDlgItem($w,165)) 0x147).ToInt32() -eq 5) 'Diagonal binding not restored'
    Require ((Send ([GestureSmoke]::GetDlgItem($w,173)) 0x147).ToInt32() -eq 6) 'Saved special binding not restored'
    Require ((Get-Label $w 127) -eq '浅色') 'Theme was not restored'
    Require ((Get-Label $w 132) -like '*2 秒') 'Idle setting was not restored'
    Require ((Get-Label $w 133) -like '*0.25 秒') 'Result setting was not restored'
    Require ((Get-Label $w 131) -eq '关闭登录自启') 'Autostart registration not detected'
    [void](Send $w 0x111 131)
    Require ((Get-Item -LiteralPath $testKey).GetValueNames() -notcontains 'GlideQuill') 'Logon entry was not removed'
    [void](Send $w 0x111 121)
    [void](Send $w 0x111 124)
    Require ($p.WaitForExit(5000)) 'Background instance exit failed'
    $start.Arguments=''
    $p=Start-TestApp
    Require ((Get-Label $p.MainWindowHandle 121) -eq '启用全局手势') 'Paused state not restored'
    [void](Send $p.MainWindowHandle 0x111 124)
    Require ($p.WaitForExit(5000)) 'Final exit failed'
    Write-Output 'PASS: centered branded window, embedded icon, basic and two-leg bindings, draft navigation, save failure/retry, hover clear, staged reset, restart isolation, settings, theme, tray and legacy configuration.'
} finally {
    if (!$p.HasExited) { $p.Kill(); $p.WaitForExit() }
    if (Test-Path -LiteralPath $testKey) {Remove-Item -LiteralPath $testKey}
    $tempRoot=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ((Split-Path $root -Parent) -eq $tempRoot -and (Split-Path $root -Leaf) -like 'glidequill-test-*' -and (Test-Path $root)) {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
