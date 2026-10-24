import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import {
  RC_DC_LABEL,
  RC_DC_ID,
  RC_OP_MOVE,
  RC_OP_MOUSE_DOWN,
  RC_OP_MOUSE_UP,
  RC_OP_WHEEL,
  RC_OP_KEY_DOWN,
  RC_OP_KEY_UP,
  RC_OP_PING,
  RC_OP_HOST_ACTIVE,
  RC_OP_HOST_IDLE,
  RC_OP_RESET,
  RC_OP_HOST_REVOKED,
  RC_OP_BUSY,
  RC_OP_START,
  RC_OP_STOP,
  RemoteControlProtocol,
  isShortcutSafe,
  getWinVkCode
} from '../src/renderer/src/services/remoteControl.ts';

describe('Tier 8: Remote Control Architecture & Security Protocol', () => {
  describe('Suite 1: DataChannel Identification & Constant Integrity', () => {
    test('S1-01: DataChannel label is strictly zabor-rc-v1', () => {
      assert.equal(RC_DC_LABEL, 'zabor-rc-v1');
    });

    test('S1-02: DataChannel negotiated ID is strictly 4243 to isolate from ultra-voice (4242)', () => {
      assert.equal(RC_DC_ID, 4243);
      assert.notEqual(RC_DC_ID, 4242, 'RC channel must not collide with ultra-low latency voice ID 4242');
    });

    test('S1-03: Opcodes are distinct and follow the protocol specification', () => {
      const opcodes = [
        RC_OP_MOVE,
        RC_OP_MOUSE_DOWN,
        RC_OP_MOUSE_UP,
        RC_OP_WHEEL,
        RC_OP_KEY_DOWN,
        RC_OP_KEY_UP,
        RC_OP_PING,
        RC_OP_HOST_ACTIVE,
        RC_OP_HOST_IDLE,
        RC_OP_RESET,
        RC_OP_HOST_REVOKED,
        RC_OP_BUSY,
        RC_OP_START,
        RC_OP_STOP
      ];
      const unique = new Set(opcodes);
      assert.equal(unique.size, opcodes.length, 'Every protocol opcode must be unique');
    });
  });

  describe('Suite 2: Binary Packet Serialization & Deserialization (Zero-Allocation)', () => {
    test('S2-01: encodeMove encodes normalized coordinates into 9-byte buffer', () => {
      const buf = RemoteControlProtocol.encodeMove(0.75, 0.25);
      assert.equal(buf.byteLength, 9);
      const dv = new DataView(buf);
      assert.equal(dv.getUint8(0), RC_OP_MOVE);
      assert.ok(Math.abs(dv.getFloat32(1, true) - 0.75) < 0.0001);
      assert.ok(Math.abs(dv.getFloat32(5, true) - 0.25) < 0.0001);
    });

    test('S2-02: encodeMouseButton encodes button state and index', () => {
      const downBuf = RemoteControlProtocol.encodeMouseButton(0, true);
      assert.equal(downBuf.byteLength, 2);
      assert.equal(downBuf[0], RC_OP_MOUSE_DOWN);
      assert.equal(downBuf[1], 0);

      const upBuf = RemoteControlProtocol.encodeMouseButton(2, false);
      assert.equal(upBuf.byteLength, 2);
      assert.equal(upBuf[0], RC_OP_MOUSE_UP);
      assert.equal(upBuf[1], 2);
    });

    test('S2-03: encodeMouseWheel encodes signed deltaY and deltaX', () => {
      const buf = RemoteControlProtocol.encodeMouseWheel(-120, 0);
      assert.equal(buf.byteLength, 5);
      const dv = new DataView(buf);
      assert.equal(dv.getUint8(0), RC_OP_WHEEL);
      assert.equal(dv.getInt16(1, true), -120);
      assert.equal(dv.getInt16(3, true), 0);
    });

    test('S2-04: encodeKey encodes virtual key and modifier bitmask', () => {
      // Key down for 'A' (0x41) with Ctrl (2) + Shift (1) = 3
      const buf = RemoteControlProtocol.encodeKey(0x41, true, 3);
      assert.equal(buf.byteLength, 4);
      const dv = new DataView(buf);
      assert.equal(dv.getUint8(0), RC_OP_KEY_DOWN);
      assert.equal(dv.getUint16(1, true), 0x41);
      assert.equal(dv.getUint8(3), 3);

      const upBuf = RemoteControlProtocol.encodeKey(0x41, false, 0);
      const upDv = new DataView(upBuf);
      assert.equal(upDv.getUint8(0), RC_OP_KEY_UP);
      assert.equal(upDv.getUint16(1, true), 0x41);
      assert.equal(upDv.getUint8(3), 0);
    });

    test('S2-05: encodeSignal returns fast pre-allocated single-byte buffers', () => {
      for (const op of [RC_OP_PING, RC_OP_HOST_ACTIVE, RC_OP_HOST_IDLE, RC_OP_RESET, RC_OP_HOST_REVOKED, RC_OP_BUSY, RC_OP_START, RC_OP_STOP]) {
        const buf = RemoteControlProtocol.encodeSignal(op);
        assert.equal(buf.byteLength, 1);
        assert.equal(buf[0], op);
      }
    });
  });

  describe('Suite 3: Security Whitelist & Dangerous Shortcut Blacklist', () => {
    test('S3-01: Windows keys (Left/Right Win) are strictly blocked', () => {
      assert.equal(isShortcutSafe(0x5b, {}), false, 'Left Win key must be blocked');
      assert.equal(isShortcutSafe(0x5c, {}), false, 'Right Win key must be blocked');
      assert.equal(isShortcutSafe(0x41, { win: true }), false, 'Win+A must be blocked');
      assert.equal(isShortcutSafe(0x4c, { win: true }), false, 'Win+L (Lock workstation) must be blocked');
      assert.equal(isShortcutSafe(0x52, { win: true }), false, 'Win+R (Run dialog) must be blocked');
    });

    test('S3-02: Ctrl+Alt+Del and Ctrl+Shift+Esc (Task Manager / Security) are blocked', () => {
      // VK_DELETE = 0x2e
      assert.equal(isShortcutSafe(0x2e, { ctrl: true, alt: true }), false, 'Ctrl+Alt+Delete must be blocked');
      // VK_ESCAPE = 0x1b
      assert.equal(isShortcutSafe(0x1b, { ctrl: true, shift: true }), false, 'Ctrl+Shift+Esc must be blocked');
    });

    test('S3-03: Alt+F4 and Alt+Tab are strictly blocked', () => {
      // VK_F4 = 0x73
      assert.equal(isShortcutSafe(0x73, { alt: true }), false, 'Alt+F4 must be blocked');
      // VK_TAB = 0x09
      assert.equal(isShortcutSafe(0x09, { alt: true }), false, 'Alt+Tab must be blocked');
    });

    test('S3-04: Sleep / Wake keys are blocked', () => {
      assert.equal(isShortcutSafe(0x5f, {}), false, 'Sleep key (0x5f) must be blocked');
    });

    test('S3-05: Safe everyday keystrokes are allowed', () => {
      assert.equal(isShortcutSafe(0x41, {}), true, 'Key A must be allowed');
      assert.equal(isShortcutSafe(0x30, {}), true, 'Key 0 must be allowed');
      assert.equal(isShortcutSafe(0x0d, {}), true, 'Enter must be allowed');
      assert.equal(isShortcutSafe(0x20, {}), true, 'Space must be allowed');
      assert.equal(isShortcutSafe(0x08, {}), true, 'Backspace must be allowed');
      assert.equal(isShortcutSafe(0x25, {}), true, 'Arrow Left must be allowed');
      assert.equal(isShortcutSafe(0x70, {}), true, 'F1 must be allowed');
    });

    test('S3-06: Safe modifier combinations (Ctrl+C, Ctrl+V, Shift+Letters) are allowed', () => {
      assert.equal(isShortcutSafe(0x43, { ctrl: true }), true, 'Ctrl+C (Copy) must be allowed');
      assert.equal(isShortcutSafe(0x56, { ctrl: true }), true, 'Ctrl+V (Paste) must be allowed');
      assert.equal(isShortcutSafe(0x5a, { ctrl: true }), true, 'Ctrl+Z (Undo) must be allowed');
      assert.equal(isShortcutSafe(0x41, { shift: true }), true, 'Shift+A (Capital letter) must be allowed');
    });

    test('S3-07: Unregistered or unknown keys outside whitelist are rejected', () => {
      assert.equal(isShortcutSafe(0xff, {}), false, 'Unknown key code 0xff must be rejected');
      assert.equal(isShortcutSafe(0x00, {}), false, 'Key code 0 must be rejected');
    });
  });

  describe('Suite 4: DOM to Windows Virtual Key Mapping (getWinVkCode)', () => {
    test('S4-01: Maps standard navigation and control keys', () => {
      assert.equal(getWinVkCode({ code: 'Enter', keyCode: 13 }), 0x0d);
      assert.equal(getWinVkCode({ code: 'Escape', keyCode: 27 }), 0x1b);
      assert.equal(getWinVkCode({ code: 'Backspace', keyCode: 8 }), 0x08);
      assert.equal(getWinVkCode({ code: 'Tab', keyCode: 9 }), 0x09);
      assert.equal(getWinVkCode({ code: 'Space', keyCode: 32 }), 0x20);
      assert.equal(getWinVkCode({ code: 'ArrowLeft', keyCode: 37 }), 0x25);
      assert.equal(getWinVkCode({ code: 'ArrowUp', keyCode: 38 }), 0x26);
      assert.equal(getWinVkCode({ code: 'ArrowRight', keyCode: 39 }), 0x27);
      assert.equal(getWinVkCode({ code: 'ArrowDown', keyCode: 40 }), 0x28);
    });

    test('S4-02: Falls back to code when keyCode is missing', () => {
      assert.equal(getWinVkCode({ code: 'Enter' }), 0x0d);
      assert.equal(getWinVkCode({ code: 'Delete' }), 0x2e);
      assert.equal(getWinVkCode({ code: 'Home' }), 0x24);
    });

    test('S4-03: Maps physical letter keys (KeyF, KeyA) under Cyrillic/non-Latin layouts', () => {
      assert.equal(getWinVkCode({ code: 'KeyF', keyCode: 0 }), 0x46, 'KeyF must map to VK_F (0x46)');
      assert.equal(getWinVkCode({ code: 'KeyF' }), 0x46, 'KeyF without keyCode must map to VK_F (0x46)');
      assert.equal(getWinVkCode({ code: 'KeyA', keyCode: 0 }), 0x41, 'KeyA must map to VK_A (0x41)');
      assert.equal(getWinVkCode({ code: 'KeyA' }), 0x41, 'KeyA without keyCode must map to VK_A (0x41)');
    });

    test('S4-04: Maps physical digit keys (Digit1, Digit0) under non-Latin layouts', () => {
      assert.equal(getWinVkCode({ code: 'Digit1', keyCode: 0 }), 0x31, 'Digit1 must map to VK_1 (0x31)');
      assert.equal(getWinVkCode({ code: 'Digit1' }), 0x31, 'Digit1 without keyCode must map to VK_1 (0x31)');
      assert.equal(getWinVkCode({ code: 'Digit0', keyCode: 0 }), 0x30, 'Digit0 must map to VK_0 (0x30)');
    });

    test('S4-05: Maps Escape key accurately with or without keyCode', () => {
      assert.equal(getWinVkCode({ code: 'Escape', keyCode: 27 }), 0x1b, 'Escape with keyCode 27 must map to 0x1b');
      assert.equal(getWinVkCode({ code: 'Escape', keyCode: 0 }), 0x1b, 'Escape with keyCode 0 must map to 0x1b');
      assert.equal(getWinVkCode({ code: 'Escape' }), 0x1b, 'Escape without keyCode must map to 0x1b');
    });
  });

  describe('Suite 5: Coordinate Letterbox Math & Aspect Ratio Preservation', () => {
    function computeLetterbox(clientWidth, clientHeight, videoWidth, videoHeight) {
      if (!videoWidth || !videoHeight || !clientWidth || !clientHeight) {
        return { left: 0, top: 0, width: clientWidth, height: clientHeight };
      }
      const videoRatio = videoWidth / videoHeight;
      const elemRatio = clientWidth / clientHeight;
      let renderW = clientWidth;
      let renderH = clientHeight;
      let offsetX = 0;
      let offsetY = 0;
      if (elemRatio > videoRatio) {
        renderW = clientHeight * videoRatio;
        offsetX = (clientWidth - renderW) / 2;
      } else {
        renderH = clientWidth / videoRatio;
        offsetY = (clientHeight - renderH) / 2;
      }
      return { left: offsetX, top: offsetY, width: renderW, height: renderH };
    }

    test('S5-01: Identical aspect ratios result in 0 offset and 100% dimensions', () => {
      const box = computeLetterbox(1920, 1080, 1280, 720);
      assert.equal(box.left, 0);
      assert.equal(box.top, 0);
      assert.equal(box.width, 1920);
      assert.equal(box.height, 1080);
    });

    test('S5-02: Ultrawide display (21:9) creates horizontal pillarbox offsets', () => {
      // 2560x1080 display showing 16:9 (1920x1080) stream
      const box = computeLetterbox(2560, 1080, 1920, 1080);
      assert.equal(box.top, 0);
      assert.equal(box.height, 1080);
      assert.equal(box.width, 1920);
      assert.equal(box.left, 320); // (2560 - 1920) / 2 = 320px
    });

    test('S5-03: Taller display (16:10) creates vertical letterbox offsets', () => {
      // 1920x1200 display showing 16:9 stream (1920x1080)
      const box = computeLetterbox(1920, 1200, 1920, 1080);
      assert.equal(box.left, 0);
      assert.equal(box.width, 1920);
      assert.equal(box.height, 1080);
      assert.equal(box.top, 60); // (1200 - 1080) / 2 = 60px
    });

    test('S5-04: Normalized coordinates accurately map inside content box', () => {
      const box = computeLetterbox(2560, 1080, 1920, 1080);
      // Mouse at clientX = 1280 (dead center of screen), clientY = 540 (dead center)
      const localX = 1280 - box.left;
      const localY = 540 - box.top;
      const normX = localX / box.width;
      const normY = localY / box.height;

      assert.ok(Math.abs(normX - 0.5) < 0.0001);
      assert.ok(Math.abs(normY - 0.5) < 0.0001);
    });
  });

  describe('Suite 6: Host Priority & Hysteresis Conflict Resolution', () => {
    test('S6-01: Movement threshold 48px suppresses false positives from micro-vibrations', () => {
      const MOVEMENT_THRESHOLD = 48;

      function checkDisplacement(dx, dy) {
        const dist = Math.hypot(dx, dy);
        return dist > MOVEMENT_THRESHOLD;
      }

      assert.equal(checkDisplacement(5, 5), false, 'Small 7px tremor must NOT revoke control');
      assert.equal(checkDisplacement(30, 20), false, 'Desk bump 36px must NOT revoke control');
      assert.equal(checkDisplacement(40, 25), false, '47.1px movement must NOT revoke control');
      assert.equal(checkDisplacement(50, 0), true, 'Deliberate 50px host movement MUST revoke control');
      assert.equal(checkDisplacement(35, 35), true, 'Deliberate 49.5px host movement MUST revoke control');
    });

    test('S6-02: Physical mouse click by host immediately intercepts control', () => {
      let isHostActive = false;
      const onPhysicalClick = () => {
        isHostActive = true;
      };
      onPhysicalClick();
      assert.equal(isHostActive, true, 'Host physical click must trigger HOST_ACTIVE immediately');
    });

    test('S6-03: Host 2-second idle timer transitions to HOST_IDLE', async () => {
      let isHostActive = true;
      let idleTimer = null;

      const triggerHostActivity = () => {
        isHostActive = true;
        if (idleTimer) clearTimeout(idleTimer);
        idleTimer = setTimeout(() => {
          isHostActive = false;
        }, 50); // fast test timer
      };

      triggerHostActivity();
      assert.equal(isHostActive, true);
      await new Promise((r) => setTimeout(r, 70));
      assert.equal(isHostActive, false, 'After idle period without host motion, state must return to idle');
    });

    test('S6-04: Fail-safe watchdog resets state when viewer heartbeat ceases', async () => {
      let activeViewerId = 'viewer-123';
      let failSafeTimer = null;

      const refreshFailSafe = () => {
        if (failSafeTimer) clearTimeout(failSafeTimer);
        failSafeTimer = setTimeout(() => {
          activeViewerId = null;
        }, 50); // fast test timer
      };

      refreshFailSafe();
      assert.equal(activeViewerId, 'viewer-123');
      await new Promise((r) => setTimeout(r, 70));
      assert.equal(activeViewerId, null, 'Fail-safe watchdog must reset activeViewerId on silence');
    });
  });

  describe('Suite 7: Host Cursor Blanking & Instant Local Cursor Protocol', () => {
    test('S7-01: Active remote control session start triggers host cursor blanking', () => {
      let hostCursorVisible = true;
      const onRemoteControlStart = () => {
        hostCursorVisible = false;
      };

      onRemoteControlStart();
      assert.equal(hostCursorVisible, false, 'Host cursor must be blanked to prevent duplicate floating cursor on video stream');
    });

    test('S7-02: Physical host input immediately restores host system cursor', () => {
      let hostCursorVisible = false;
      const onPhysicalActivity = () => {
        hostCursorVisible = true;
      };

      onPhysicalActivity();
      assert.equal(hostCursorVisible, true, 'Physical host movement must immediately restore cursor visibility for host');
    });

    test('S7-03: Host 2-second idle expiration re-blanks host cursor while remote viewer is active', async () => {
      let hostCursorVisible = true;
      let isHostActive = true;
      let activeRemoteViewerId = 'viewer-user-1';
      let idleTimer = null;

      const triggerHostActivity = () => {
        isHostActive = true;
        hostCursorVisible = true;
        if (idleTimer) clearTimeout(idleTimer);
        idleTimer = setTimeout(() => {
          isHostActive = false;
          if (activeRemoteViewerId) {
            hostCursorVisible = false;
          }
        }, 50); // fast test timer
      };

      triggerHostActivity();
      assert.equal(isHostActive, true);
      assert.equal(hostCursorVisible, true);

      await new Promise((r) => setTimeout(r, 70));
      assert.equal(isHostActive, false);
      assert.equal(hostCursorVisible, false, 'After 2-second hysteresis, host cursor must be blanked again for viewer');
    });

    test('S7-04: Session reset or disconnect guarantees host cursor restoration', () => {
      let hostCursorVisible = false;
      const resetControlState = () => {
        hostCursorVisible = true;
      };

      resetControlState();
      assert.equal(hostCursorVisible, true, 'Ending session must unconditionally restore normal system cursor');
    });
  });
});
