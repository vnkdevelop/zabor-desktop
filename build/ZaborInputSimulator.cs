using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

namespace ZaborInputSimulator {
    public class Program {
        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool SetCursorPos(int X, int Y);

        [DllImport("user32.dll")]
        private static extern bool GetCursorPos(out POINT lpPoint);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint nInputs, [In] INPUT[] pInputs, int cbSize);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool ClipCursor(ref RECT lpRect);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool ClipCursor(IntPtr lpRect);

        [DllImport("user32.dll")]
        private static extern uint MapVirtualKey(uint uCode, uint uMapType);

        [DllImport("user32.dll")]
        private static extern bool SetProcessDPIAware();

        [DllImport("user32.dll")]
        private static extern int GetSystemMetrics(int nIndex);

        [DllImport("user32.dll", CharSet = CharSet.Auto, SetLastError = true)]
        private static extern IntPtr SetWindowsHookEx(int idHook, LowLevelMouseProc lpfn, IntPtr hMod, uint dwThreadId);

        [DllImport("user32.dll", CharSet = CharSet.Auto, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool UnhookWindowsHookEx(IntPtr hhk);

        [DllImport("user32.dll", CharSet = CharSet.Auto, SetLastError = true)]
        private static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);

        [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
        private static extern IntPtr GetModuleHandle(string lpModuleName);

        [DllImport("user32.dll")]
        private static extern sbyte GetMessage(out MSG lpMsg, IntPtr hWnd, uint wMsgFilterMin, uint wMsgFilterMax);

        [DllImport("user32.dll")]
        private static extern bool TranslateMessage([In] ref MSG lpMsg);

        [DllImport("user32.dll")]
        private static extern IntPtr DispatchMessage([In] ref MSG lpMsg);

        [DllImport("user32.dll")]
        private static extern bool PostThreadMessage(uint idThread, uint Msg, IntPtr wParam, IntPtr lParam);

        [DllImport("kernel32.dll")]
        private static extern uint GetCurrentThreadId();

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool SetSystemCursor(IntPtr hcur, uint id);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern IntPtr CreateCursor(IntPtr hInst, int xHotSpot, int yHotSpot, int nWidth, int nHeight, byte[] pvANDPlane, byte[] pvXORPlane);

        private const uint INPUT_MOUSE = 0;
        private const uint INPUT_KEYBOARD = 1;

        private const uint MOUSEEVENTF_MOVE = 0x0001;
        private const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
        private const uint MOUSEEVENTF_LEFTUP = 0x0004;
        private const uint MOUSEEVENTF_RIGHTDOWN = 0x0008;
        private const uint MOUSEEVENTF_RIGHTUP = 0x0010;
        private const uint MOUSEEVENTF_MIDDLEDOWN = 0x0020;
        private const uint MOUSEEVENTF_MIDDLEUP = 0x0040;
        private const uint MOUSEEVENTF_WHEEL = 0x0800;
        private const uint MOUSEEVENTF_VIRTUALDESK = 0x4000;
        private const uint MOUSEEVENTF_ABSOLUTE = 0x8000;

        private const uint KEYEVENTF_EXTENDEDKEY = 0x0001;
        private const uint KEYEVENTF_KEYUP = 0x0002;

        private const ushort VK_SHIFT = 0x10;
        private const ushort VK_CONTROL = 0x11;
        private const ushort VK_MENU = 0x12; // Alt

        // Win32 Hook Constants
        private const int WH_MOUSE_LL = 14;

        private const int SM_CXSCREEN = 0;
        private const int SM_CYSCREEN = 1;

        private const int WM_MOUSEMOVE = 0x0200;
        private const int WM_LBUTTONDOWN = 0x0201;
        private const int WM_RBUTTONDOWN = 0x0204;
        private const int WM_MBUTTONDOWN = 0x0207;
        private const int WM_XBUTTONDOWN = 0x020B;

        private const uint LLMHF_INJECTED = 0x00000001;
        private const uint LLMHF_LOWER_IL_INJECTED = 0x00000002;

        private const uint WM_QUIT = 0x0012;

        private const uint SPI_SETCURSORS = 0x0057;

        // Custom signature tag for all inputs injected by Zabor
        private static readonly IntPtr SIMULATED_EXTRA_INFO = new IntPtr(0x5A41424F); // 'ZABO'

        private delegate IntPtr LowLevelMouseProc(int nCode, IntPtr wParam, IntPtr lParam);

        [StructLayout(LayoutKind.Sequential)]
        public struct RECT {
            public int Left;
            public int Top;
            public int Right;
            public int Bottom;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct POINT {
            public int x;
            public int y;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct MSLLHOOKSTRUCT {
            public POINT pt;
            public uint mouseData;
            public uint flags;
            public uint time;
            public IntPtr dwExtraInfo;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct MSG {
            public IntPtr hwnd;
            public uint message;
            public IntPtr wParam;
            public IntPtr lParam;
            public uint time;
            public POINT pt;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct INPUT {
            public uint type;
            public InputUnion u;
        }

        [StructLayout(LayoutKind.Explicit)]
        private struct InputUnion {
            [FieldOffset(0)]
            public MOUSEINPUT mi;
            [FieldOffset(0)]
            public KEYBDINPUT ki;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct MOUSEINPUT {
            public int dx;
            public int dy;
            public uint mouseData;
            public uint dwFlags;
            public uint time;
            public IntPtr dwExtraInfo;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct KEYBDINPUT {
            public ushort wVk;
            public ushort wScan;
            public uint dwFlags;
            public uint time;
            public IntPtr dwExtraInfo;
        }

        private static bool isLeftDown = false;
        private static bool isRightDown = false;
        private static bool isMiddleDown = false;
        private static readonly bool[] pressedKeys = new bool[256];

        // Static references to prevent GC collection of unmanaged callback
        private static LowLevelMouseProc mouseHookDelegate;
        private static IntPtr hookId = IntPtr.Zero;
        private static uint hookThreadId = 0;
        private static volatile bool hasRecentPhysicalClick = false;
        private static long lastPhysicalActivityTicks = 0;
        private static long lastSimulatedClickTicks = 0;
        private static int lastSimulatedX = -1;
        private static int lastSimulatedY = -1;
        private static long lastSimulatedMoveTicks = 0;
        private static readonly AutoResetEvent physicalClickSignal = new AutoResetEvent(false);
        private static int cachedScreenWidth = 0;
        private static int cachedScreenHeight = 0;
        private static long lastScreenMetricsTicks = 0;

        private static readonly int[] simPointsX = new int[64];
        private static readonly int[] simPointsY = new int[64];
        private static readonly long[] simPointsTicks = new long[64];
        private static int simPointIndex = 0;

        private static void RecordSimulatedPoint(int x, int y, long ticks) {
            int idx = (Interlocked.Increment(ref simPointIndex) & 0x7FFFFFFF) % 64;
            simPointsX[idx] = x;
            simPointsY[idx] = y;
            simPointsTicks[idx] = ticks;
        }

        private static bool IsRecentSimulatedPoint(int ptX, int ptY, long now) {
            for (int i = 0; i < 64; i++) {
                long t = simPointsTicks[i];
                if (t == 0) continue;
                long age = Math.Abs(now - t);
                if (age <= 750) {
                    int dx = ptX - simPointsX[i];
                    int dy = ptY - simPointsY[i];
                    if ((dx * dx + dy * dy) <= 144) {
                        return true;
                    }
                }
            }
            return false;
        }

        private static volatile bool isCursorHidden = false;

        private static readonly uint[] OCR_IDS = new uint[] {
            32512, 32513, 32514, 32515, 32516, 32640, 32641, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650
        };

        private static IntPtr CreateBlankCursorHandle() {
            byte[] andMask = new byte[128];
            for (int i = 0; i < andMask.Length; i++) {
                andMask[i] = 0xFF;
            }
            byte[] xorMask = new byte[128];
            return CreateCursor(IntPtr.Zero, 0, 0, 32, 32, andMask, xorMask);
        }

        private static void HideSystemCursor(bool force = false) {
            if (isCursorHidden && !force) return;
            try {
                foreach (uint id in OCR_IDS) {
                    IntPtr hBlank = CreateBlankCursorHandle();
                    if (hBlank != IntPtr.Zero) {
                        SetSystemCursor(hBlank, id);
                    }
                }
                isCursorHidden = true;
            } catch {}

        }

        private static void RestoreSystemCursors(bool force = false) {
            if (!isCursorHidden && !force) return;
            try {
                SystemParametersInfo(SPI_SETCURSORS, 0, IntPtr.Zero, 0);
            } catch {}
            isCursorHidden = false;
        }

        private static void UpdateScreenMetrics() {
            long now = Environment.TickCount;
            if (cachedScreenWidth == 0 || now - lastScreenMetricsTicks > 2000) {
                cachedScreenWidth = GetSystemMetrics(SM_CXSCREEN);
                cachedScreenHeight = GetSystemMetrics(SM_CYSCREEN);
                lastScreenMetricsTicks = now;
            }
        }

        private static void SendMouseMove(int x, int y) {
            UpdateScreenMetrics();
            int w = cachedScreenWidth;
            int h = cachedScreenHeight;
            if (w <= 0 || h <= 0) return;

            long now = Environment.TickCount;
            lastSimulatedX = x;
            lastSimulatedY = y;
            lastSimulatedMoveTicks = now;
            RecordSimulatedPoint(x, y, now);

            int normX = (int)Math.Round((x * 65535.0) / (w - 1));
            int normY = (int)Math.Round((y * 65535.0) / (h - 1));

            var input = new INPUT {
                type = INPUT_MOUSE,
                u = new InputUnion {
                    mi = new MOUSEINPUT {
                        dx = normX,
                        dy = normY,
                        mouseData = 0,
                        dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                        time = 0,
                        dwExtraInfo = SIMULATED_EXTRA_INFO
                    }
                }
            };

            SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT)));
            SetCursorPos(x, y);

        }

        private static IntPtr MouseHookCallback(int nCode, IntPtr wParam, IntPtr lParam) {
            if (nCode >= 0) {
                try {
                    int msg = wParam.ToInt32();
                    if (msg == WM_MOUSEMOVE) {
                        MSLLHOOKSTRUCT hookStruct = (MSLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(MSLLHOOKSTRUCT));
                        long now = Environment.TickCount;

                        bool isInjected = ((hookStruct.flags & LLMHF_INJECTED) != 0) ||
                                          ((hookStruct.flags & LLMHF_LOWER_IL_INJECTED) != 0) ||
                                          (hookStruct.dwExtraInfo == SIMULATED_EXTRA_INFO);

                        if (!isInjected) {
                            if (!IsRecentSimulatedPoint(hookStruct.pt.x, hookStruct.pt.y, now)) {
                                long timeSinceSim = Math.Abs(now - lastSimulatedMoveTicks);
                                int dx = hookStruct.pt.x - lastSimulatedX;
                                int dy = hookStruct.pt.y - lastSimulatedY;
                                int distSq = dx * dx + dy * dy;

                                bool isPhysicalIntervention = (timeSinceSim < 400 && distSq > 10000) ||
                                                              (timeSinceSim >= 400 && distSq > 225);

                                if (isPhysicalIntervention) {
                                    if (Math.Abs(now - lastPhysicalActivityTicks) > 80) {
                                        lastPhysicalActivityTicks = now;
                                        hasRecentPhysicalClick = true;
                                        try {
                                            physicalClickSignal.Set();
                                        } catch {}
                                    }
                                }
                            }
                        }
                    } else if (msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN || msg == WM_XBUTTONDOWN) {
                        MSLLHOOKSTRUCT hookStruct = (MSLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(MSLLHOOKSTRUCT));

                        long now = Environment.TickCount;
                        bool isRecentSimulated = Math.Abs(now - lastSimulatedClickTicks) < 350;

                        // Filter out ALL injected/simulated inputs
                        bool isInjected = isRecentSimulated ||
                                          ((hookStruct.flags & LLMHF_INJECTED) != 0) ||
                                          ((hookStruct.flags & LLMHF_LOWER_IL_INJECTED) != 0) ||
                                          (hookStruct.dwExtraInfo == SIMULATED_EXTRA_INFO);

                        if (!isInjected) {
                            if (Math.Abs(now - lastPhysicalActivityTicks) > 80) {
                                lastPhysicalActivityTicks = now;
                                hasRecentPhysicalClick = true;
                                try {
                                    physicalClickSignal.Set();
                                } catch {}
                            }
                        }
                    }
                } catch {
                    // Suppress any callback exceptions to maintain hook stability
                }
            }
            return CallNextHookEx(hookId, nCode, wParam, lParam);
        }

        public static void Main(string[] args) {
            try {
                SetProcessDPIAware();
            } catch {}

            try {
                AppDomain.CurrentDomain.ProcessExit += (s, e) => {
                    CleanupHook();
                    ResetAll();
                };
            } catch {}

            if (args != null && args.Length > 0 && args[0] == "unclip") {
                ClipCursor(IntPtr.Zero);
                return;
            }

            int parentPid = 0;
            if (args != null && args.Length > 0) {
                int.TryParse(args[0], out parentPid);
            }

            // Watchdog thread to terminate if parent process exits
            if (parentPid > 0) {
                var watchdog = new Thread(() => {
                    try {
                        var parent = Process.GetProcessById(parentPid);
                        parent.WaitForExit();
                        CleanupHook();
                        ResetAll();
                        Environment.Exit(0);
                    } catch {
                        // Do not exit if process handle cannot be obtained. Stdin pipe will handle exit.
                    }
                }) { IsBackground = true };
                watchdog.Start();
            }

            // Autonomous physical click watcher using WH_MOUSE_LL hook with LLMHF_INJECTED filtering
            var hookThread = new Thread(() => {
                try {
                    hookThreadId = GetCurrentThreadId();
                    mouseHookDelegate = MouseHookCallback;

                    using (Process curProcess = Process.GetCurrentProcess())
                    using (ProcessModule curModule = curProcess.MainModule) {
                        IntPtr hMod = GetModuleHandle(curModule.ModuleName);
                        hookId = SetWindowsHookEx(WH_MOUSE_LL, mouseHookDelegate, hMod, 0);
                    }

                    if (hookId != IntPtr.Zero) {
                        MSG msg;
                        while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) {
                            TranslateMessage(ref msg);
                            DispatchMessage(ref msg);
                        }
                    }
                } catch {
                    // Thread safety
                } finally {
                    CleanupHook();
                }
            }) { IsBackground = true };
            hookThread.Start();

            // Background reporter for physical activity to keep hook callback non-blocking
            var clickReporter = new Thread(() => {
                while (true) {
                    try {
                        if (physicalClickSignal.WaitOne()) {
                            if (isCursorHidden) {
                                RestoreSystemCursors(false);
                            }
                            Console.WriteLine("activity");
                        }
                    } catch {
                        break;
                    }
                }
            }) { IsBackground = true };
            clickReporter.Start();

            // Ensure clean start
            ResetAll();

            string line;
            while ((line = Console.ReadLine()) != null) {
                try {
                    line = line.Trim();
                    if (string.IsNullOrEmpty(line)) continue;
                    if (line == "exit" || line == "quit") break;

                    if (line == "reset") {
                        ResetAll();
                        continue;
                    }

                    if (line == "hide_cursor") {
                        HideSystemCursor(true);
                        continue;
                    }

                    if (line == "show_cursor") {
                        RestoreSystemCursors(true);
                        continue;
                    }

                    if (line == "check_click") {
                        bool click = hasRecentPhysicalClick;
                        hasRecentPhysicalClick = false;
                        Console.WriteLine(click ? "1" : "0");
                        continue;
                    }

                    string[] parts = line.Split(' ');
                    string cmd = parts[0];

                    if (cmd == "mn" && parts.Length >= 3) {
                        float xn, yn;
                        if (float.TryParse(parts[1], System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out xn) &&
                            float.TryParse(parts[2], System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out yn)) {
                            UpdateScreenMetrics();
                            int w = cachedScreenWidth;
                            int h = cachedScreenHeight;
                            int x = (int)Math.Round(xn * (w - 1));
                            int y = (int)Math.Round(yn * (h - 1));
                            SendMouseMove(x, y);
                        }
                    } else if (cmd == "m" && parts.Length >= 3) {
                        int x, y;
                        if (int.TryParse(parts[1], out x) && int.TryParse(parts[2], out y)) {
                            SendMouseMove(x, y);
                        }
                    } else if (cmd == "d" && parts.Length >= 2) {
                        int btn;
                        if (int.TryParse(parts[1], out btn)) {
                            SendMouseButton(btn, true);
                        }
                    } else if (cmd == "u" && parts.Length >= 2) {
                        int btn;
                        if (int.TryParse(parts[1], out btn)) {
                            SendMouseButton(btn, false);
                        }
                    } else if (cmd == "w" && parts.Length >= 2) {
                        int delta;
                        if (int.TryParse(parts[1], out delta)) {
                            SendMouseWheel(delta);
                        }
                    } else if (cmd == "kd" && parts.Length >= 2) {
                        ushort vk;
                        if (ushort.TryParse(parts[1], out vk)) {
                            SendKeyboardKey(vk, true);
                        }
                    } else if (cmd == "ku" && parts.Length >= 2) {
                        ushort vk;
                        if (ushort.TryParse(parts[1], out vk)) {
                            SendKeyboardKey(vk, false);
                        }
                    } else if (cmd == "clip" && parts.Length >= 5) {
                        int l, t, r, b;
                        if (int.TryParse(parts[1], out l) && int.TryParse(parts[2], out t) &&
                            int.TryParse(parts[3], out r) && int.TryParse(parts[4], out b)) {
                            RECT rect = new RECT { Left = l, Top = t, Right = r, Bottom = b };
                            ClipCursor(ref rect);
                        }
                    } else if (cmd == "unclip") {
                        ClipCursor(IntPtr.Zero);
                    }
                } catch {
                    // Ignore single malformed line, keep loop running
                }
            }

            CleanupHook();
            ResetAll();
        }

        private static void CleanupHook() {
            try {
                ClipCursor(IntPtr.Zero);
            } catch {}
            try {
                RestoreSystemCursors(true);
            } catch {}
            if (hookId != IntPtr.Zero) {
                UnhookWindowsHookEx(hookId);
                hookId = IntPtr.Zero;
            }
            if (hookThreadId != 0) {
                PostThreadMessage(hookThreadId, WM_QUIT, IntPtr.Zero, IntPtr.Zero);
            }
        }

        private static void SendMouseButton(int button, bool down) {
            uint flag = 0;
            if (button == 0) {
                flag = down ? MOUSEEVENTF_LEFTDOWN : MOUSEEVENTF_LEFTUP;
                isLeftDown = down;
            } else if (button == 1) {
                flag = down ? MOUSEEVENTF_MIDDLEDOWN : MOUSEEVENTF_MIDDLEUP;
                isMiddleDown = down;
            } else if (button == 2) {
                flag = down ? MOUSEEVENTF_RIGHTDOWN : MOUSEEVENTF_RIGHTUP;
                isRightDown = down;
            }

            if (flag == 0) return;

            lastSimulatedClickTicks = Environment.TickCount;

            var input = new INPUT {
                type = INPUT_MOUSE,
                u = new InputUnion {
                    mi = new MOUSEINPUT {
                        dwFlags = flag,
                        mouseData = 0,
                        time = 0,
                        dwExtraInfo = SIMULATED_EXTRA_INFO
                    }
                }
            };

            SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT)));
        }

        private static void SendMouseWheel(int delta) {
            var input = new INPUT {
                type = INPUT_MOUSE,
                u = new InputUnion {
                    mi = new MOUSEINPUT {
                        dwFlags = MOUSEEVENTF_WHEEL,
                        mouseData = (uint)(delta * 120),
                        time = 0,
                        dwExtraInfo = SIMULATED_EXTRA_INFO
                    }
                }
            };

            SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT)));
        }

        private static void SendKeyboardKey(ushort vk, bool down) {
            if (vk == 0 || vk >= 256) return;

            uint flags = 0;
            if (!down) flags |= KEYEVENTF_KEYUP;

            // Extended keys (arrows, nav keys)
            if (vk >= 0x21 && vk <= 0x2E) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }

            ushort scan = 0;
            try {
                scan = (ushort)MapVirtualKey(vk, 0);
            } catch {}

            var input = new INPUT {
                type = INPUT_KEYBOARD,
                u = new InputUnion {
                    ki = new KEYBDINPUT {
                        wVk = vk,
                        wScan = scan,
                        dwFlags = flags,
                        time = 0,
                        dwExtraInfo = SIMULATED_EXTRA_INFO
                    }
                }
            };

            SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT)));
            pressedKeys[vk] = down;
        }

        private static void ResetAll() {
            try {
                ClipCursor(IntPtr.Zero);
            } catch {}
            try {
                RestoreSystemCursors(true);
            } catch {}
            // Release mouse buttons
            if (isLeftDown) {
                SendMouseButton(0, false);
                isLeftDown = false;
            }
            if (isRightDown) {
                SendMouseButton(2, false);
                isRightDown = false;
            }
            if (isMiddleDown) {
                SendMouseButton(1, false);
                isMiddleDown = false;
            }

            // Release all pressed keyboard keys
            for (ushort i = 0; i < 256; i++) {
                if (pressedKeys[i]) {
                    SendKeyboardKey(i, false);
                    pressedKeys[i] = false;
                }
            }

            // Safety explicit release of modifier keys
            SendKeyboardKey(VK_SHIFT, false);
            SendKeyboardKey(VK_CONTROL, false);
            SendKeyboardKey(VK_MENU, false);
        }
    }
}
