# One title bar on Windows (#97), checked on GitHub's Windows runner against
# the built Morpheus.exe: no system caption above the app's 32 px bar (the
# client area fills the window's visible frame), the app's minimise,
# maximise and close at the bar's right answering the system's hit test as
# its caption buttons (HTMAXBUTTON brings Windows 11's snap layouts), the bar
# as the caption across its width (drag, Aero snap, double-click maximise),
# a real mouse drag of the bar moving the window, the top edge still
# resizing, a maximised window exactly on the monitor's work area (no
# overflow by the frame's thickness), and closing (the bar's X, Alt+F4)
# hiding the window to the tray with the app still running.
#
# The display is set to 150% first when the runner allows it (the bug
# reports came from a 150% laptop): every coordinate here is in physical
# pixels (this script's thread is per-monitor DPI aware).
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
    [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(POINT p, uint flags);
    [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO i);
    [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr m, int type, out uint x, out uint y);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);

    // Physical pixels for everything this thread asks or sends (per-monitor
    // aware v2), whatever pwsh's own awareness is.
    public static void Aware() { SetThreadDpiAwarenessContext(new IntPtr(-4)); }

    public static IntPtr Find(uint pid, string title) {
        Aware();
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
        Aware();
        RECT c; GetClientRect(h, out c);
        var p = new POINT(); ClientToScreen(h, ref p);
        return new RECT { Left = p.X, Top = p.Y, Right = p.X + c.Right, Bottom = p.Y + c.Bottom };
    }
    // The visible frame (the window rect less its invisible resize borders).
    public static RECT Frame(IntPtr h) {
        Aware();
        RECT r; DwmGetWindowAttribute(h, 9 /* DWMWA_EXTENDED_FRAME_BOUNDS */, out r, Marshal.SizeOf(typeof(RECT)));
        return r;
    }
    public static RECT Work(IntPtr h) {
        Aware();
        var i = new MONITORINFO(); i.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
        GetMonitorInfo(MonitorFromWindow(h, 2), ref i);
        return i.rcWork;
    }
    // WM_NCHITTEST at a screen point, the mouse moved there first (GPUI
    // answers from the element under its last mouse position).
    public static int Hit(IntPtr h, int x, int y) {
        Aware();
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(40);
        var l = (IntPtr)(((y & 0xffff) << 16) | (x & 0xffff));
        SendMessage(h, 0x0084, IntPtr.Zero, l);
        System.Threading.Thread.Sleep(20);
        return (int)SendMessage(h, 0x0084, IntPtr.Zero, l);
    }
    public static void Click(int x, int y, int times) {
        Aware();
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(150);
        for (int i = 0; i < times; i++) {
            mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
            mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
            System.Threading.Thread.Sleep(60);
        }
    }
    // The mouse to a screen point as real input (absolute over the virtual
    // desktop), as a hand on the mouse moves it.
    static void MoveTo(int x, int y) {
        int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
        int nx = (int)Math.Round((x - vx) * 65535.0 / (vw - 1));
        int ny = (int)Math.Round((y - vy) * 65535.0 / (vh - 1));
        mouse_event(0x0001 | 0x8000 | 0x4000, nx, ny, 0, UIntPtr.Zero); // MOVE | ABSOLUTE | VIRTUALDESK
        SetCursorPos(x, y);
    }
    // Press at (x, y), move by (dx, dy) in steps, release.
    public static void Drag(int x, int y, int dx, int dy) {
        Aware();
        MoveTo(x, y);
        System.Threading.Thread.Sleep(200);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(150);
        for (int i = 1; i <= 25; i++) {
            MoveTo(x + dx * i / 25, y + dy * i / 25);
            System.Threading.Thread.Sleep(20);
        }
        System.Threading.Thread.Sleep(200);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(500);
    }
    public static void AltF4() {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x73, 0, 0, UIntPtr.Zero);
        keybd_event(0x73, 0, 2, UIntPtr.Zero);
        keybd_event(0x12, 0, 2, UIntPtr.Zero);
    }

    // The display: a resolution that allows 150%, then 150%.
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
        public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra;
        public int dmFields, dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
        public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
        public short dmLogPixels;
        public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
        public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplaySettings(string dev, int mode, ref DEVMODE dm);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int ChangeDisplaySettings(ref DEVMODE dm, uint flags);
    public static string SetResolution(int w, int h) {
        Aware();
        var dm = new DEVMODE(); dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
        DEVMODE best = dm; bool found = false; long bestArea = 0;
        for (int i = 0; EnumDisplaySettings(null, i, ref dm); i++) {
            long area = (long)dm.dmPelsWidth * dm.dmPelsHeight;
            if (dm.dmPelsWidth == w && dm.dmPelsHeight == h) { best = dm; found = true; break; }
            if (dm.dmPelsWidth <= w && dm.dmPelsHeight <= h && area > bestArea) { best = dm; bestArea = area; }
        }
        if (!found && bestArea == 0) return "no display modes";
        best.dmFields = 0x80000 | 0x100000; // DM_PELSWIDTH | DM_PELSHEIGHT
        int r = ChangeDisplaySettings(ref best, 0);
        return string.Format("resolution {0}x{1}: {2}", best.dmPelsWidth, best.dmPelsHeight, r == 0 ? "set" : "error " + r);
    }

    [StructLayout(LayoutKind.Sequential)] public struct LUID { public uint Low; public int High; }
    [StructLayout(LayoutKind.Sequential)] public struct PATH_SOURCE { public LUID adapterId; public uint id; public uint modeIdx; public uint status; }
    [StructLayout(LayoutKind.Sequential)] public struct PATH_TARGET { public LUID adapterId; public uint id; public uint modeIdx; public int tech; public int rot; public int scaling; public uint rrNum, rrDen; public int scan; public int available; public uint status; }
    [StructLayout(LayoutKind.Sequential)] public struct PATH_INFO { public PATH_SOURCE src; public PATH_TARGET tgt; public uint flags; }
    [StructLayout(LayoutKind.Sequential)] public struct DPI_GET { public int type; public int size; public LUID adapterId; public uint id; public int min, cur, max; }
    [StructLayout(LayoutKind.Sequential)] public struct DPI_SET { public int type; public int size; public LUID adapterId; public uint id; public int scale; }
    [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags, out uint np, out uint nm);
    [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags, ref uint np, [Out] PATH_INFO[] paths, ref uint nm, IntPtr modes, IntPtr topo);
    [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(ref DPI_GET p);
    [DllImport("user32.dll")] static extern int DisplayConfigSetDeviceInfo(ref DPI_SET p);
    static readonly int[] Steps = { 100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500 };
    public static uint PrimaryDpi() {
        Aware();
        uint x, y; GetDpiForMonitor(MonitorFromPoint(new POINT(), 1), 0, out x, out y);
        return x;
    }
    // The primary display's scale (Settings > Display > Scale), through the
    // undocumented DPI-scale device info the Settings app uses: relative
    // steps from the recommended scale.
    public static string SetScale(int percent) {
        Aware();
        uint np, nm;
        if (GetDisplayConfigBufferSizes(2, out np, out nm) != 0 || np == 0) return "no display paths";
        var paths = new PATH_INFO[np];
        IntPtr modes = Marshal.AllocHGlobal((int)nm * 64);
        try {
            int q = QueryDisplayConfig(2, ref np, paths, ref nm, modes, IntPtr.Zero);
            if (q != 0) return "QueryDisplayConfig error " + q;
        } finally { Marshal.FreeHGlobal(modes); }
        var g = new DPI_GET { type = -3, size = Marshal.SizeOf(typeof(DPI_GET)), adapterId = paths[0].src.adapterId, id = paths[0].src.id };
        int e = DisplayConfigGetDeviceInfo(ref g);
        if (e != 0) return "getting the scale: error " + e;
        int now = Array.IndexOf(Steps, (int)Math.Round(PrimaryDpi() * 100.0 / 96 / 25) * 25);
        if (now < 0) return "current scale unknown (dpi " + PrimaryDpi() + ")";
        int recommended = now - g.cur;
        int want = Array.IndexOf(Steps, percent) - recommended;
        string range = string.Format("{0}%..{1}%", Steps[Math.Max(0, recommended + g.min)], Steps[Math.Min(Steps.Length - 1, recommended + g.max)]);
        if (want > g.max || want < g.min) return string.Format("{0}% not offered (range {1})", percent, range);
        var s = new DPI_SET { type = -4, size = Marshal.SizeOf(typeof(DPI_SET)), adapterId = g.adapterId, id = g.id, scale = want };
        e = DisplayConfigSetDeviceInfo(ref s);
        return string.Format("scale {0}% (range {1}): {2}", percent, range, e == 0 ? "set" : "error " + e);
    }
}
'@

$failed = $false
function Fail($m) { Write-Host "FAIL: $m"; $script:failed = $true }
function Ok($m) { Write-Host "ok: $m" }
function Show($name, $r) { "{0}: {1},{2} - {3},{4} ({5}x{6})" -f $name, $r.Left, $r.Top, $r.Right, $r.Bottom, ($r.Right - $r.Left), ($r.Bottom - $r.Top) }
function Near($a, $b, $d) { [Math]::Abs($a - $b) -le $d }

[Win]::Aware()
Write-Host "display before: $([Win]::PrimaryDpi()) dpi"
Write-Host ([Win]::SetResolution(1920, 1080))
Start-Sleep -Seconds 2
Write-Host ([Win]::SetScale(150))
Start-Sleep -Seconds 2
Write-Host "display: $([Win]::PrimaryDpi()) dpi"

$procs = @()
# A Morpheus on its own empty data directory (the sign-in screen), and its
# window once shown, topmost and in front.
function Start-Morpheus {
    $data = Join-Path ([IO.Path]::GetTempPath()) ("morpheus-chrome-" + [Guid]::NewGuid())
    New-Item -ItemType Directory $data | Out-Null
    $env:MORPHEUS_DATA_DIR = $data
    $err = Join-Path $data 'stderr.txt'
    $out = Join-Path $data 'stdout.txt'
    $proc = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $err -RedirectStandardOutput $out
    $script:procs += $proc
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
    [pscustomobject]@{ Proc = $proc; Hwnd = $hwnd; Err = $err }
}

# Closed to the tray: the process runs on, no window of it shows.
function Check-Hidden($app, $how) {
    Start-Sleep -Seconds 3
    $app.Proc.Refresh()
    if ($app.Proc.HasExited) {
        Fail "$how ended the app (exit code $($app.Proc.ExitCode)) instead of hiding it to the tray"
        Get-Content $app.Err -ErrorAction SilentlyContinue | Select-Object -Last 15
        return
    }
    if ([Win]::Find([uint32]$app.Proc.Id, 'Morpheus') -ne [IntPtr]::Zero) { Fail "$how left the window showing"; return }
    Ok "$how hides the window to the tray; the app runs on"
}

try {
    $app = Start-Morpheus
    $hwnd = $app.Hwnd
    $err = $app.Err
    $dpi = [Win]::GetDpiForWindow($hwnd)
    Write-Host "window dpi $dpi ($([int]($dpi * 100 / 96))%)"
    if ($dpi -eq 96) { Write-Host 'note: the runner kept 100%; the checks below ran unscaled' }

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

    # The bar (32 logical px, its middle 16 down) in physical pixels.
    $scale = $dpi / 96.0
    $barMid = [int](16 * $scale)
    $y = $client.Top + $barMid
    # The bar's row, right to left: close, maximise, minimise, then caption.
    # Each change kept as its distance from the client's right edge.
    $seen = @()
    for ($off = 3; $off -lt [int](160 * $scale); $off += 3) {
        $h = [Win]::Hit($hwnd, $client.Right - $off, $y)
        if ($seen.Count -eq 0 -or $seen[-1].Hit -ne $h) { $seen += [pscustomobject]@{ Hit = $h; Off = $off } }
    }
    $order = ($seen | ForEach-Object { "$($_.Hit)@$($_.Off)" }) -join ','
    Write-Host "hit test along the bar from the right, code@px in (HTCLOSE 20, HTMAXBUTTON 9, HTMINBUTTON 8, HTCAPTION 2, HTCLIENT 1): $order"
    $codes = @($seen | ForEach-Object { $_.Hit })
    $iClose = [array]::IndexOf($codes, 20); $iMax = [array]::IndexOf($codes, 9); $iMin = [array]::IndexOf($codes, 8)
    if ($iClose -ge 0 -and $iMax -gt $iClose -and $iMin -gt $iMax) { Ok 'close, maximise and minimise at the right are the system''s caption buttons (snap layouts over maximise)' }
    else { Fail "the window buttons are not the caption's buttons, right to left ($order)" }
    # Three buttons of about 30 logical px: their row's width says the hit
    # test is in the window's own scale.
    if ($iMin -ge 0 -and $iMin + 1 -lt $seen.Count) {
        $buttonsWidth = $seen[$iMin + 1].Off
        if ($buttonsWidth -gt 70 * $scale -and $buttonsWidth -lt 130 * $scale) { Ok "the buttons span $buttonsWidth px at $([int]($scale * 100))%: hit test and drawing agree on the scale" }
        else { Fail "the buttons span $buttonsWidth px at $([int]($scale * 100))% (expected about $([int](96 * $scale)))" }
    }
    # The caption across the bar: left, middle, right of middle, and just
    # left of minimise.
    $width = $client.Right - $client.Left
    $points = @([int]($width * 0.15), [int]($width * 0.5), [int]($width * 0.75))
    if ($iMin -ge 0 -and $iMin + 1 -lt $seen.Count) { $points += $width - $seen[$iMin + 1].Off - [int](12 * $scale) }
    $bad = @()
    foreach ($px in $points) {
        foreach ($dy in @([int](6 * $scale), $barMid, [int](26 * $scale))) {
            $h = [Win]::Hit($hwnd, $client.Left + $px, $client.Top + $dy)
            if ($h -ne 2) { $bad += "$h at ($px,$dy)" }
        }
    }
    if ($bad.Count -eq 0) { Ok "the bar is the caption (HTCAPTION) across its width and height ($($points.Count) x 3 points)" }
    else { Fail "parts of the bar are not HTCAPTION: $($bad -join ', ')" }
    $mid = [int](($client.Left + $client.Right) / 2)
    $hTop = [Win]::Hit($hwnd, $mid, $client.Top + 1)
    if ($hTop -eq 12) { Ok 'the top edge resizes (HTTOP)' } else { Fail "the bar's top edge is $hTop, not HTTOP" }
    $hBody = [Win]::Hit($hwnd, $mid, $client.Top + [int](200 * $scale))
    if ($hBody -eq 1) { Ok 'below the bar is client area' } else { Fail "below the bar is $hBody, not HTCLIENT" }

    # Dragging the bar with the mouse moves the window.
    $before = [Win]::Frame($hwnd)
    $dx = 160; $dy = 90
    [Win]::Drag([int]($client.Left + $width * 0.3), $y, $dx, $dy)
    $after = [Win]::Frame($hwnd)
    Write-Host (Show 'frame before the drag' $before); Write-Host (Show 'frame after the drag' $after)
    if ((Near ($after.Left - $before.Left) $dx 4) -and (Near ($after.Top - $before.Top) $dy 4) -and (Near ($after.Right - $after.Left) ($before.Right - $before.Left) 2)) {
        Ok "dragging the bar moves the window by the mouse's ($dx, $dy)"
    } else {
        Fail "dragging the bar by ($dx, $dy) moved the window by ($($after.Left - $before.Left), $($after.Top - $before.Top))"
    }

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
    $c = [Win]::Client($hwnd)
    [Win]::Click([int](($c.Left + $c.Right) / 2), $c.Top + $barMid, 2)
    Check-Maximised 'a double click on the bar'
    $c = [Win]::Client($hwnd)
    [Win]::Click([int](($c.Left + $c.Right) / 2), $c.Top + $barMid, 2)
    Start-Sleep -Seconds 2
    if ([Win]::IsZoomed($hwnd)) { Fail 'a second double click did not restore the window'; [Win]::ShowWindow($hwnd, 9) | Out-Null; Start-Sleep -Seconds 2 }
    else { Ok 'a second double click restores the window' }

    # Our maximise button maximises (the system's button handling).
    $max = $seen | Where-Object { $_.Hit -eq 9 } | Select-Object -First 1
    if ($max) {
        $c = [Win]::Client($hwnd)
        [Win]::Click($c.Right - $max.Off - 6, $c.Top + $barMid, 1)
        Check-Maximised 'the maximise button'
        [Win]::ShowWindow($hwnd, 9) | Out-Null # SW_RESTORE
        Start-Sleep -Seconds 1
    }

    # The bar's X closes to the tray: the app keeps running.
    $close = $seen | Where-Object { $_.Hit -eq 20 } | Select-Object -First 1
    if ($close) {
        [Win]::SetForegroundWindow($hwnd) | Out-Null
        $c = [Win]::Client($hwnd)
        [Win]::Click($c.Right - $close.Off - 6, $c.Top + $barMid, 1)
        Check-Hidden $app 'the close button'
        if (Select-String -Path $err -Pattern 'window closed, app runs on' -Quiet) { Ok 'the app logged that it runs on' }
    }

    # Alt+F4 (the system's close) does the same, on a second Morpheus.
    $second = Start-Morpheus
    [Win]::AltF4()
    Start-Sleep -Seconds 1
    $second.Proc.Refresh()
    if (-not $second.Proc.HasExited -and [Win]::Find([uint32]$second.Proc.Id, 'Morpheus') -ne [IntPtr]::Zero) {
        # The keys went elsewhere (the runner's focus): the system's close as
        # Alt+F4 sends it.
        Write-Host 'note: Alt+F4 did not reach the window; sending its SC_CLOSE'
        [Win]::SendMessage($second.Hwnd, 0x0112, [IntPtr]0xF060, [IntPtr]::Zero) | Out-Null # WM_SYSCOMMAND SC_CLOSE
    }
    Check-Hidden $second 'Alt+F4 (the system''s close)'
} finally {
    foreach ($p in $procs) { if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
}
if ($failed) { exit 1 }
