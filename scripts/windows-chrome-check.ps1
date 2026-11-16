# One title bar on Windows (#97), checked on GitHub's Windows runner against
# the built Morpheus.exe: no system caption above the app's 32 px bar (the
# client area fills the window's visible frame), the app's minimise,
# maximise and close at the bar's right answering the system's hit test as
# its caption buttons (HTMAXBUTTON brings Windows 11's snap layouts), the bar
# as the caption (drag, Aero snap, double-click maximise), the top edge still
# resizing, and a maximised window exactly on the monitor's work area (no
# overflow by the frame's thickness).
#
#   pwsh -File scripts/windows-chrome-check.ps1 path\to\Morpheus.exe
param([Parameter(Mandatory)] [string] $Exe)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct MONITORINFO { public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; }
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int size);
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO i);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);

    public static IntPtr Find(uint pid, string title) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != pid || !IsWindowVisible(h)) return true;
            var s = new StringBuilder(256); GetWindowText(h, s, 256);
            if (s.ToString() == title) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    // The client area in screen coordinates.
    public static RECT Client(IntPtr h) {
        RECT c; GetClientRect(h, out c);
        var p = new POINT(); ClientToScreen(h, ref p);
        return new RECT { Left = p.X, Top = p.Y, Right = p.X + c.Right, Bottom = p.Y + c.Bottom };
    }
    // The visible frame (the window rect less its invisible resize borders).
    public static RECT Frame(IntPtr h) {
        RECT r; DwmGetWindowAttribute(h, 9 /* DWMWA_EXTENDED_FRAME_BOUNDS */, out r, Marshal.SizeOf(typeof(RECT)));
        return r;
    }
    public static RECT Work(IntPtr h) {
        var i = new MONITORINFO(); i.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
        GetMonitorInfo(MonitorFromWindow(h, 2), ref i);
        return i.rcWork;
    }
    // WM_NCHITTEST at a screen point, the mouse moved there first (GPUI
    // answers from the element under its last mouse position).
    public static int Hit(IntPtr h, int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(40);
        var l = (IntPtr)(((y & 0xffff) << 16) | (x & 0xffff));
        SendMessage(h, 0x0084, IntPtr.Zero, l);
        System.Threading.Thread.Sleep(20);
        return (int)SendMessage(h, 0x0084, IntPtr.Zero, l);
    }
    public static void Click(int x, int y, int times) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(150);
        for (int i = 0; i < times; i++) {
            mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
            mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
            System.Threading.Thread.Sleep(60);
        }
    }
}
'@

$failed = $false
function Fail($m) { Write-Host "FAIL: $m"; $script:failed = $true }
function Ok($m) { Write-Host "ok: $m" }
function Show($name, $r) { "{0}: {1},{2} - {3},{4} ({5}x{6})" -f $name, $r.Left, $r.Top, $r.Right, $r.Bottom, ($r.Right - $r.Left), ($r.Bottom - $r.Top) }
function Near($a, $b, $d) { [Math]::Abs($a - $b) -le $d }

$data = Join-Path ([IO.Path]::GetTempPath()) ("morpheus-chrome-" + [Guid]::NewGuid())
New-Item -ItemType Directory $data | Out-Null
$env:MORPHEUS_DATA_DIR = $data
$err = Join-Path $data 'stderr.txt'
$out = Join-Path $data 'stdout.txt'
$proc = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $err -RedirectStandardOutput $out

try {
    $hwnd = [IntPtr]::Zero
    for ($i = 0; $i -lt 60 -and $hwnd -eq [IntPtr]::Zero; $i++) {
        Start-Sleep -Milliseconds 500
        if ($proc.HasExited) { break }
        $hwnd = [Win]::Find([uint32]$proc.Id, 'Morpheus')
    }
    if ($hwnd -eq [IntPtr]::Zero) {
        Fail "no Morpheus window (exited: $($proc.HasExited))"
        Get-Content $err -ErrorAction SilentlyContinue | Select-Object -Last 30
        exit 1
    }
    Start-Sleep -Seconds 5
    [Win]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, 0, 0, 0x0003) | Out-Null # HWND_TOPMOST, no move/size
    [Win]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Seconds 1
    Write-Host "dpi $([Win]::GetDpiForWindow($hwnd))"

    $chrome = Select-String -Path $err -Pattern 'window chrome:' | Select-Object -First 1
    Write-Host "$chrome"
    if ("$chrome" -match 'buttons app, inset 8,') { Ok 'the app draws its window buttons, bar content 8 px in' }
    else { Fail "the app did not report its own buttons ($chrome)" }

    # No system caption: the client area is the visible frame, at the top too.
    $frame = [Win]::Frame($hwnd); $client = [Win]::Client($hwnd)
    $r = New-Object Win+RECT; [Win]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    Write-Host (Show 'window rect' $r); Write-Host (Show 'visible frame' $frame); Write-Host (Show 'client' $client)
    if ((Near $client.Top $frame.Top 2) -and (Near $client.Left $frame.Left 2) -and (Near $client.Right $frame.Right 2) -and (Near $client.Bottom $frame.Bottom 2)) {
        Ok "the client area fills the window: no system caption (client top $($client.Top - $frame.Top) px below the frame's)"
    } else {
        Fail "a system caption or border is outside the client area (client top $($client.Top - $frame.Top) px below the frame's top)"
    }

    # The bar's row, right to left: close, maximise, minimise, then caption.
    $y = $client.Top + 16
    $seen = @()
    for ($x = $client.Right - 3; $x -gt $client.Right - 160; $x -= 3) {
        $h = [Win]::Hit($hwnd, $x, $y)
        if ($seen.Count -eq 0 -or $seen[-1].Hit -ne $h) { $seen += [pscustomobject]@{ Hit = $h; X = $x } }
    }
    $order = ($seen | ForEach-Object { $_.Hit }) -join ','
    Write-Host "hit test along the bar from the right (HTCLOSE 20, HTMAXBUTTON 9, HTMINBUTTON 8, HTCAPTION 2, HTCLIENT 1): $order"
    $codes = @($seen | ForEach-Object { $_.Hit })
    $iClose = [array]::IndexOf($codes, 20); $iMax = [array]::IndexOf($codes, 9); $iMin = [array]::IndexOf($codes, 8)
    if ($iClose -ge 0 -and $iMax -gt $iClose -and $iMin -gt $iMax) { Ok 'close, maximise and minimise at the right are the system''s caption buttons (snap layouts over maximise)' }
    else { Fail "the window buttons are not the caption's buttons, right to left ($order)" }
    $mid = [int](($client.Left + $client.Right) / 2)
    $hMid = [Win]::Hit($hwnd, $mid, $y)
    if ($hMid -eq 2) { Ok 'the bar is the caption (HTCAPTION): drag, Aero snap, double-click maximise' } else { Fail "the bar's middle is $hMid, not HTCAPTION" }
    $hTop = [Win]::Hit($hwnd, $mid, $client.Top + 1)
    if ($hTop -eq 12) { Ok 'the top edge resizes (HTTOP)' } else { Fail "the bar's top edge is $hTop, not HTTOP" }
    $hBody = [Win]::Hit($hwnd, $mid, $client.Top + 200)
    if ($hBody -eq 1) { Ok 'below the bar is client area' } else { Fail "below the bar is $hBody, not HTCLIENT" }

    function Check-Maximised($how) {
        Start-Sleep -Seconds 2
        if (-not [Win]::IsZoomed($hwnd)) { Fail "$how did not maximise"; return }
        $work = [Win]::Work($hwnd); $c = [Win]::Client($hwnd)
        Write-Host (Show 'work area' $work); Write-Host (Show 'maximised client' $c)
        if ((Near $c.Left $work.Left 1) -and (Near $c.Top $work.Top 1) -and (Near $c.Right $work.Right 1) -and (Near $c.Bottom $work.Bottom 1)) {
            Ok "$how maximises onto the work area exactly (no overflow by the frame)"
        } else {
            Fail "${how}: the maximised client area is not the work area"
        }
    }

    # A double click on the caption maximises; another puts it back.
    [Win]::Click($mid, $y, 2)
    Check-Maximised 'a double click on the bar'
    $c = [Win]::Client($hwnd)
    [Win]::Click([int](($c.Left + $c.Right) / 2), $c.Top + 16, 2)
    Start-Sleep -Seconds 2
    if ([Win]::IsZoomed($hwnd)) { Fail 'a second double click did not restore the window'; [Win]::ShowWindow($hwnd, 9) | Out-Null; Start-Sleep -Seconds 2 }
    else { Ok 'a second double click restores the window' }

    # Our maximise button maximises (the system's button handling).
    $max = $seen | Where-Object { $_.Hit -eq 9 } | Select-Object -First 1
    if ($max) {
        $c = [Win]::Client($hwnd)
        $mx = $max.X - 6; $my = $c.Top + 16
        [Win]::Click($mx, $my, 1)
        Check-Maximised 'the maximise button'
        [Win]::ShowWindow($hwnd, 9) | Out-Null # SW_RESTORE
        Start-Sleep -Seconds 1
    }
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
}
if ($failed) { exit 1 }
