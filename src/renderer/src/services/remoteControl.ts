export const RC_DC_LABEL = 'zabor-rc-v1';
export const RC_DC_ID = 4243;

export const RC_OP_MOVE = 0x01;
export const RC_OP_MOUSE_DOWN = 0x02;
export const RC_OP_MOUSE_UP = 0x03;
export const RC_OP_WHEEL = 0x04;
export const RC_OP_KEY_DOWN = 0x05;
export const RC_OP_KEY_UP = 0x06;

export const RC_OP_PING = 0x10;
export const RC_OP_HOST_ACTIVE = 0x11;
export const RC_OP_HOST_IDLE = 0x12;
export const RC_OP_RESET = 0x13;
export const RC_OP_HOST_REVOKED = 0x14;
export const RC_OP_BUSY = 0x15;
export const RC_OP_START = 0x16;
export const RC_OP_STOP = 0x17;

// Safe Virtual Key Codes Whitelist & Blacklist for Security Filtering
const SAFE_VK_CODES = new Set<number>([
  // Backspace, Tab, Enter, Escape, Space, PageUp, PageDown, End, Home, Left, Up, Right, Down, Insert, Delete
  0x08, 0x09, 0x0d, 0x1b, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2d, 0x2e,
  // 0-9
  0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
  // A-Z
  0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d,
  0x4e, 0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a,
  // Shift, Ctrl, Alt, CapsLock, NumLock, ScrollLock
  0x10, 0x11, 0x12, 0x14, 0x90, 0x91,
  // Numpad 0-9, multiply, add, separator, subtract, decimal, divide
  0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f,
  // F1 - F11 (F12 is omitted as a potential dev/system key)
  0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a,
  // Punctuation and symbols
  0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0, 0xdb, 0xdc, 0xdd, 0xde
]);

const CODE_TO_VK: Record<string, number> = {
  Enter: 0x0d,
  Escape: 0x1b,
  Backspace: 0x08,
  Tab: 0x09,
  Space: 0x20,
  ArrowLeft: 0x25,
  ArrowUp: 0x26,
  ArrowRight: 0x27,
  ArrowDown: 0x28,
  Delete: 0x2e,
  Insert: 0x2d,
  Home: 0x24,
  End: 0x23,
  PageUp: 0x21,
  PageDown: 0x22,
  Minus: 0xbd,
  Equal: 0xbb,
  BracketLeft: 0xdb,
  BracketRight: 0xdd,
  Semicolon: 0xba,
  Quote: 0xde,
  Backquote: 0xc0,
  Backslash: 0xdc,
  Comma: 0xbc,
  Period: 0xbe,
  Slash: 0xbf,
  ShiftLeft: 0x10,
  ShiftRight: 0x10,
  ControlLeft: 0x11,
  ControlRight: 0x11,
  AltLeft: 0x12,
  AltRight: 0x12,
  CapsLock: 0x14,
  NumLock: 0x90,
  ScrollLock: 0x91,
  Numpad0: 0x60,
  Numpad1: 0x61,
  Numpad2: 0x62,
  Numpad3: 0x63,
  Numpad4: 0x64,
  Numpad5: 0x65,
  Numpad6: 0x66,
  Numpad7: 0x67,
  Numpad8: 0x68,
  Numpad9: 0x69,
  NumpadMultiply: 0x6a,
  NumpadAdd: 0x6b,
  NumpadSubtract: 0x6d,
  NumpadDecimal: 0x6e,
  NumpadDivide: 0x6f,
  NumpadEnter: 0x0d
};

export interface KeyModifiers {
  alt?: boolean;
  ctrl?: boolean;
  shift?: boolean;
  win?: boolean;
}

export function isShortcutSafe(vk: number, modifiers: KeyModifiers): boolean {
  // Unconditionally block Windows keys (Start menu, Win+L, Win+R, Win+X)
  if (vk === 0x5b || vk === 0x5c || modifiers.win) return false;

  // Block Sleep/Wake keys
  if (vk === 0x5f) return false;

  // Block Ctrl+Alt+Delete or Ctrl+Shift+Esc (Task Manager / Security Screen)
  if (modifiers.ctrl && modifiers.alt && vk === 0x2e) return false;
  if (modifiers.ctrl && modifiers.shift && vk === 0x1b) return false;

  // Block Alt+F4 (Close window)
  if (modifiers.alt && vk === 0x73) return false;

  // Block Alt+Tab (Switch windows)
  if (modifiers.alt && vk === 0x09) return false;

  // Must be in safe whitelist
  return SAFE_VK_CODES.has(vk);
}

// Convert DOM KeyboardEvent to Windows Virtual Key code
export function getWinVkCode(e: KeyboardEvent | { code?: string; keyCode?: number }): number {
  if (e.code && CODE_TO_VK[e.code]) {
    return CODE_TO_VK[e.code];
  }
  if (e.code && e.code.startsWith('Key') && e.code.length === 4) {
    const charCode = e.code.toUpperCase().charCodeAt(3);
    if (charCode >= 0x41 && charCode <= 0x5a) {
      return charCode;
    }
  }
  if (e.code && e.code.startsWith('Digit') && e.code.length === 6) {
    const charCode = e.code.charCodeAt(5);
    if (charCode >= 0x30 && charCode <= 0x39) {
      return charCode;
    }
  }
  if (e.code && /^F\d{1,2}$/.test(e.code)) {
    const fnNum = parseInt(e.code.slice(1), 10);
    if (fnNum >= 1 && fnNum <= 12) {
      return 0x70 + fnNum - 1;
    }
  }
  if (e.keyCode && SAFE_VK_CODES.has(e.keyCode)) {
    return e.keyCode;
  }
  return e.keyCode || 0;
}

// Zero-allocation pre-allocated reusable binary packets
export class RemoteControlProtocol {
  private static moveBuf = new ArrayBuffer(9);
  private static moveDv = new DataView(RemoteControlProtocol.moveBuf);

  private static btnBuf = new Uint8Array(2);
  private static wheelBuf = new ArrayBuffer(5);
  private static wheelDv = new DataView(RemoteControlProtocol.wheelBuf);

  private static keyBuf = new ArrayBuffer(4);
  private static keyDv = new DataView(RemoteControlProtocol.keyBuf);

  private static singleByteBufs = new Map<number, Uint8Array>([
    [RC_OP_PING, new Uint8Array([RC_OP_PING])],
    [RC_OP_HOST_ACTIVE, new Uint8Array([RC_OP_HOST_ACTIVE])],
    [RC_OP_HOST_IDLE, new Uint8Array([RC_OP_HOST_IDLE])],
    [RC_OP_RESET, new Uint8Array([RC_OP_RESET])],
    [RC_OP_HOST_REVOKED, new Uint8Array([RC_OP_HOST_REVOKED])],
    [RC_OP_BUSY, new Uint8Array([RC_OP_BUSY])],
    [RC_OP_START, new Uint8Array([RC_OP_START])],
    [RC_OP_STOP, new Uint8Array([RC_OP_STOP])]
  ]);

  public static encodeMove(xNorm: number, yNorm: number): ArrayBuffer {
    this.moveDv.setUint8(0, RC_OP_MOVE);
    this.moveDv.setFloat32(1, xNorm, true);
    this.moveDv.setFloat32(5, yNorm, true);
    return this.moveBuf;
  }

  public static encodeMouseButton(button: number, down: boolean): Uint8Array {
    this.btnBuf[0] = down ? RC_OP_MOUSE_DOWN : RC_OP_MOUSE_UP;
    this.btnBuf[1] = button & 0xff;
    return this.btnBuf;
  }

  public static encodeMouseWheel(deltaY: number, deltaX = 0): ArrayBuffer {
    this.wheelDv.setUint8(0, RC_OP_WHEEL);
    this.wheelDv.setInt16(1, deltaY, true);
    this.wheelDv.setInt16(3, deltaX, true);
    return this.wheelBuf;
  }

  public static encodeKey(vk: number, down: boolean, modifiers = 0): ArrayBuffer {
    this.keyDv.setUint8(0, down ? RC_OP_KEY_DOWN : RC_OP_KEY_UP);
    this.keyDv.setUint16(1, vk, true);
    this.keyDv.setUint8(3, modifiers & 0xff);
    return this.keyBuf;
  }

  public static encodeSignal(opcode: number): Uint8Array {
    return this.singleByteBufs.get(opcode) || new Uint8Array([opcode]);
  }
}
