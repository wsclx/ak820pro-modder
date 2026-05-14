// HID Keyboard Usage Page names — partial table, just the codes that appear
// on the AK820 Pro ISO-DE layout. Subset of USB-HID Usage Tables (Page 0x07).
export const HID_USAGE_NAMES: Record<number, string> = {
  0: "—",
  4: "A", 5: "B", 6: "C", 7: "D", 8: "E", 9: "F", 10: "G",
  11: "H", 12: "I", 13: "J", 14: "K", 15: "L", 16: "M", 17: "N",
  18: "O", 19: "P", 20: "Q", 21: "R", 22: "S", 23: "T", 24: "U",
  25: "V", 26: "W", 27: "X", 28: "Y", 29: "Z",
  30: "1", 31: "2", 32: "3", 33: "4", 34: "5",
  35: "6", 36: "7", 37: "8", 38: "9", 39: "0",
  40: "Enter", 41: "Esc", 42: "Backspace", 43: "Tab", 44: "Space",
  45: "Minus", 46: "Equal", 47: "BracketL", 48: "BracketR", 50: "Hash",
  51: "Semi", 52: "Quote", 53: "Backtick", 54: "Comma", 55: "Period",
  56: "Slash", 57: "Caps",
  58: "F1", 59: "F2", 60: "F3", 61: "F4", 62: "F5", 63: "F6",
  64: "F7", 65: "F8", 66: "F9", 67: "F10", 68: "F11", 69: "F12",
  74: "Home", 75: "PgUp", 76: "Del", 77: "End", 78: "PgDn",
  79: "Right", 80: "Left", 81: "Down", 82: "Up",
  100: "ISO\\",
  175: "Fn",
  224: "Ctrl", 225: "Shift", 226: "Alt", 227: "Win",
  228: "RCtrl", 229: "RShift", 230: "RAlt",
};

export function hidName(usage: number): string {
  return HID_USAGE_NAMES[usage] ?? `0x${usage.toString(16).padStart(2, "0")}`;
}
