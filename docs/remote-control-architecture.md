# Remote Control Architecture & Security Specification

## 1. Overview
The Remote Control feature provides lightweight, high-performance P2P remote control of mouse and keyboard inputs during screen sharing in Mode 3 (Fullscreen stream) within the Zabor desktop client.

## 2. P2P DataChannel Architecture
- **Dedicated SCTP Stream**: Uses label `zabor-rc-v1` with negotiated SCTP stream ID `4243`.
- **Audio Isolation**: Completely isolated from ultra-low latency audio stream `zabor-ultra-voice-v1` (ID `4242`).
- **Modes**:
  - `ordered: false`, `maxRetransmits: 0` for mouse move packets (unreliable, ultra-low latency).
  - Pre-allocated zero-allocation binary protocol buffers (`RemoteControlProtocol`).

## 3. Host Priority & Conflict Resolution
- **Movement Threshold Hysteresis**:
  Host cursor movement is filtered against a 48px Euclidean displacement threshold ($\Delta = \sqrt{dx^2 + dy^2} > 48$). Minor sensor jitter, table vibrations, or accidental bumps below 48px do not revoke control.
- **Instant Revocation Triggers**:
  - Deliberate Host mouse movement (> 48px).
  - Host physical mouse click (instantly sets `HOST_ACTIVE`).
  - Global emergency panic hotkey (`Pause/Break` or `Ctrl+Alt+Backspace`).
  - Click on the Host Pill at the top-center of the screen.
- **Host Idle Timer**:
  When the Host stops moving the mouse for 2.0 seconds, a `HOST_IDLE` packet is broadcast, restoring input forwarding capability to the active viewer.
- **Fail-Safe Watchdog**:
  If no heartbeat `PING` or input packet is received from the active viewer for 3.5 seconds, the Host automatically resets control state to idle and detaches input simulation.

## 4. UI Indicators (Strict Minimalism)
- **Host Pill (`RemoteControlPill.tsx`)**:
  - Positioned at `fixed top-3 left-1/2 -translate-x-1/2`.
  - Displays ONLY the avatar and username of the currently controlling party.
  - Neutral glass styling when Host is in control.
  - Neon-magenta (`#FF007F`) active border and background when Viewer has seized control.
  - Interactive panic button: clicking the pill instantly revokes control (`HOST_REVOKED`).
- **Viewer Indicator (`StreamCard.tsx`)**:
  - In Mode 3 (Fullscreen), active control is visualized by a glowing `#FF007F` border around the video container.
  - If Host intercepts control (`HOST_ACTIVE`), the glowing border immediately dims to neutral white/20, visually indicating that inputs are paused.

## 5. Security & Input Simulation
- **Native OS Simulator (`build/ZaborInputSimulator.exe`)**:
  - Standalone Win32 C# utility compiled with `csc.exe` (no node-gyp native compilation dependency).
  - Runs in background child process communicating via binary/line-oriented stdin.
  - Automatically terminates via watchdog monitor if the parent Electron process exits.
- **Strict Whitelist & Blacklist (`isShortcutSafe`)**:
  - Blocks Windows keys (`Win`, `Win+L`, `Win+R`, `Win+X`).
  - Blocks `Ctrl+Alt+Del` and `Ctrl+Shift+Esc` (Task Manager / security screen).
  - Blocks `Alt+F4` and `Alt+Tab`.
  - Blocks system Sleep / Wake keys.
  - Permits standard keys, typing, arrows, and safe editor shortcuts (`Ctrl+C`, `Ctrl+V`, `Ctrl+Z`).
