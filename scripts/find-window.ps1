# Shared by the development scripts: finds the main window of a process by name, also
# when the window is off the screen (where Process.MainWindowHandle can be empty).
# Dot-source it, then call Find-AppWindow "whatsapp-gpui".
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class AppWindows {
    public delegate bool Callback(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Callback cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    // The largest visible top-level window of the process.
    public static IntPtr Largest(uint pid) {
        IntPtr best = IntPtr.Zero; long bestArea = 0;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && IsWindowVisible(h)) {
                RECT r; GetWindowRect(h, out r);
                long area = (long)(r.R - r.L) * (r.B - r.T);
                if (area > bestArea) { bestArea = area; best = h; }
            }
            return true;
        }, IntPtr.Zero);
        return best;
    }
}
"@
function Find-AppWindow([string]$ProcessName) {
    foreach ($p in (Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)) {
        $h = [AppWindows]::Largest([uint32]$p.Id)
        if ($h -ne [IntPtr]::Zero) { return $h }
    }
    return [IntPtr]::Zero
}
