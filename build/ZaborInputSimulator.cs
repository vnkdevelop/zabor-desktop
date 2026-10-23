using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

namespace ZaborInputSimulator {
    public class Program {
        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool SetCursorPos(int X, int Y);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint nInputs, [In] INPUT[] pInputs, int cbSize);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool ClipCursor(ref RECT lpRect);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool ClipCursor(IntPtr lpRect);

        [DllImport("user32.dll")]
        private static extern bool SetProcessDPIAware();

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

        private const uint INPUT_MOUSE = 0;
        private const uint INPUT_KEYBOARD = 1;

        private const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
        private const uint MOUSEEVENTF_LEFTUP = 0x0004;
        private const uint MOUSEEVENTF_RIGHTDOWN = 0x0008;
        private const uint MOUSEEVENTF_RIGHTUP = 0x0010;
        private const uint MOUSEEVENTF_MIDDLEDOWN = 0x0020;
        private const uint MOUSEEVENTF_MIDDLEUP = 0x0040;
        private const uint MOUSEEVENTF_WHEEL = 0x0800;

        private const uint KEYEVENTF_EXTENDEDKEY = 0x0001;
        private const uint KEYEVENTF_KEYUP = 0x0002;

        private const ushort VK_SHIFT = 0x10;
        private const ushort VK_CONTROL = 0x11;
        private const ushort VK_MENU = 0x12; // Alt

        // Win32 Hook Constants
        private const int WH_MOUSE_LL = 14;

        private const int WM_LBUTTONDOWN = 0x0201;
        private const int WM_RBUTTONDOWN = 0x0204;
        private const int WM_MBUTTONDOWN = 0x0207;
        private const int WM_XBUTTONDOWN = 0x020B;

        private const uint LLMHF_INJECTED = 0x00000001;
        private const uint LLMHF_LOWER_IL_INJECTED = 0x00000002;

        private const uint WM_QUIT = 0x0012;

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
        private static long lastPhysicalClickTicks = 0;

        private static IntPtr MouseHookCallback(int nCode, IntPtr wParam, IntPtr lParam) {
            if (nCode >= 0) {
                try {
                    MSLLHOOKSTRUCT hookStruct = (MSLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(MSLLHOOKSTRUCT));

                    // Filter out ALL injected/simulated inputs
                    bool isInjected = ((hookStruct.flags & LLMHF_INJECTED) != 0) ||
                                      ((hookStruct.flags & LLMHF_LOWER_IL_INJECTED) != 0) ||
                                      (hookStruct.dwExtraInfo == SIMULATED_EXTRA_INFO);

                    if (!isInjected) {
                        int msg = wParam.ToInt32();
                        if (msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN || msg == WM_XBUTTONDOWN) {
                            long now = Environment.TickCount;
                            if (now - lastPhysicalClickTicks > 50) {
                                lastPhysicalClickTicks = now;
                                hasRecentPhysicalClick = true;
                                try {
                                    Console.WriteLine("click");
                                } catch {
                                    // Stdout may be closed by host
                                }
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

                    if (line == "check_click") {
                        bool click = hasRecentPhysicalClick;
                        hasRecentPhysicalClick = false;
                        Console.WriteLine(click ? "1" : "0");
                        continue;
                    }

                    string[] parts = line.Split(' ');
                    string cmd = parts[0];

                    if (cmd == "m" && parts.Length >= 3) {
                        int x, y;
                        if (int.TryParse(parts[1], out x) && int.TryParse(parts[2], out y)) {
                            SetCursorPos(x, y);
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

            var input = new INPUT {
                type = INPUT_KEYBOARD,
                u = new InputUnion {
                    ki = new KEYBDINPUT {
                        wVk = vk,
                        wScan = 0,
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
