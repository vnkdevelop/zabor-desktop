using System;
using System.Diagnostics;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Shapes;
using System.Windows.Threading;

namespace ZaborUpdater {
    public class Program {
        [STAThread]
        public static void Main(string[] args) {
            int parentPid = 0;
            if (args != null && args.Length > 0) {
                int.TryParse(args[0], out parentPid);
            }

            var app = new Application();
            var window = new Window {
                Title = "ZaborUpdater",
                WindowStyle = WindowStyle.None,
                AllowsTransparency = true,
                Background = Brushes.Transparent,
                ShowInTaskbar = false,
                Topmost = true,
                WindowStartupLocation = WindowStartupLocation.CenterScreen,
                Width = 380,
                Height = 140
            };

            window.MouseLeftButtonDown += (s, e) => {
                try { window.DragMove(); } catch {}
            };
            window.KeyDown += (s, e) => {
                if (e.Key == System.Windows.Input.Key.Escape) {
                    window.Close();
                }
            };

            var card = new Border {
                Width = 240,
                Height = 110,
                CornerRadius = new CornerRadius(16),
                Background = new SolidColorBrush(Color.FromArgb(0xD9, 0x0A, 0x0A, 0x0E)),
                BorderBrush = new SolidColorBrush(Color.FromArgb(0x24, 0xFF, 0xFF, 0xFF)),
                BorderThickness = new Thickness(1)
            };

            var stack = new StackPanel {
                VerticalAlignment = VerticalAlignment.Center,
                HorizontalAlignment = HorizontalAlignment.Center
            };

            var text = new TextBlock {
                Text = "zabor",
                FontSize = 26,
                FontWeight = FontWeights.Black,
                Foreground = Brushes.White,
                HorizontalAlignment = HorizontalAlignment.Center,
                Margin = new Thickness(0, 0, 0, 12)
            };
            stack.Children.Add(text);

            var barGrid = new Grid {
                Height = 4,
                Width = 160,
                HorizontalAlignment = HorizontalAlignment.Center
            };

            var track = new Border {
                CornerRadius = new CornerRadius(2),
                Background = new SolidColorBrush(Color.FromArgb(0x2B, 0xFF, 0xFF, 0xFF))
            };
            barGrid.Children.Add(track);

            var canvas = new Canvas {
                ClipToBounds = true
            };

            var indicator = new Border {
                CornerRadius = new CornerRadius(2),
                Height = 4,
                Width = 50,
                Background = Brushes.White
            };

            var anim = new DoubleAnimation {
                From = -50,
                To = 160,
                Duration = TimeSpan.FromSeconds(1.1),
                RepeatBehavior = RepeatBehavior.Forever
            };
            indicator.BeginAnimation(Canvas.LeftProperty, anim);

            canvas.Children.Add(indicator);
            barGrid.Children.Add(canvas);
            stack.Children.Add(barGrid);
            card.Child = stack;
            window.Content = card;

            var stopwatch = Stopwatch.StartNew();
            var timer = new DispatcherTimer {
                Interval = TimeSpan.FromMilliseconds(500)
            };

            timer.Tick += (s, e) => {
                // Hard safety timeout: 10 minutes
                if (stopwatch.Elapsed.TotalSeconds > 600) {
                    window.Close();
                    return;
                }

                // Primary closing mechanism: the installer calls taskkill when installation finishes.
                // Fallback check: only consider closing if at least 6 seconds passed and setup is no longer running.
                if (stopwatch.Elapsed.TotalSeconds > 6) {
                    try {
                        bool setupRunning = false;
                        var allProcs = Process.GetProcesses();
                        foreach (var p in allProcs) {
                            try {
                                string name = p.ProcessName;
                                if (name.IndexOf("ZABOR-Setup", StringComparison.OrdinalIgnoreCase) >= 0 ||
                                    name.IndexOf("Au_", StringComparison.OrdinalIgnoreCase) >= 0) {
                                    setupRunning = true;
                                    break;
                                }
                            } catch {}
                        }

                        // NEVER close while the setup installer is actively running!
                        if (setupRunning) {
                            return;
                        }

                        // If setup has finished and at least 10s elapsed:
                        if (stopwatch.Elapsed.TotalSeconds > 10) {
                            var zaborProcs = Process.GetProcessesByName("ZABOR");
                            if (zaborProcs != null && zaborProcs.Length > 0) {
                                bool foundRunningApp = false;
                                foreach (var zp in zaborProcs) {
                                    if (parentPid <= 0 || zp.Id != parentPid) {
                                        foundRunningApp = true;
                                        break;
                                    }
                                }
                                if (foundRunningApp) {
                                    window.Close();
                                    return;
                                }
                            }

                            if (stopwatch.Elapsed.TotalSeconds > 15) {
                                window.Close();
                                return;
                            }
                        }
                    } catch {}
                }
            };

            timer.Start();
            window.Show();
            app.Run(window);
        }
    }
}
