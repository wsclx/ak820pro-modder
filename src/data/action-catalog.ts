/**
 * Curated catalog of actions you can assign to any AK820 Pro key.
 *
 * Each entry is a JSON-RPC-shaped value that matches the Rust `KeyAction`
 * tagged enum (`{ kind: "keyboard", usage: 41 }`, etc.). The grouping is
 * cosmetic — UI-only — to mirror the AJAZZ online-driver's category tabs
 * (Basiszeichen / Erweitert / Sonder).
 */

export type Action =
  | { kind: "default" }
  | { kind: "keyboard"; usage: number }
  | { kind: "consumer_key"; value: number }
  | { kind: "mouse"; button: number; value: number }
  | { kind: "macro"; macro_id: number; param2: number; param3: number }
  | { kind: "tgl"; value: number };

export interface ActionEntry {
  label: string;
  action: Action;
  /** Optional shorter render in narrow buttons. */
  hint?: string;
}

export interface ActionGroup {
  id: string;
  name: string;
  description?: string;
  entries: ActionEntry[];
}

// HID Keyboard Usage Codes (Usage Page 0x07).
const kb = (usage: number, label: string, hint?: string): ActionEntry => ({
  label,
  hint,
  action: { kind: "keyboard", usage },
});

// HID Consumer Page (0x0C) — media keys etc. Common values from USB-HID Usage Tables.
const cons = (value: number, label: string): ActionEntry => ({
  label,
  action: { kind: "consumer_key", value },
});

const letters: ActionEntry[] = [
  // A-Z are HID 4..29
  "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
  "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
].map((l, i) => kb(4 + i, l));

const digits: ActionEntry[] = [
  // 1-9 are HID 30..38, 0 is HID 39
  ...["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d, i) => kb(30 + i, d)),
  kb(39, "0"),
];

const isoDeRow1: ActionEntry[] = [
  kb(53, "^°"),
  kb(45, "ß ?"),
  kb(46, "´ `"),
];

const isoDeRow2: ActionEntry[] = [
  kb(47, "Ü"),
  kb(48, "+ ~ *"),
];

const isoDeRow3: ActionEntry[] = [
  kb(51, "Ö"),
  kb(52, "Ä"),
  kb(50, "# '"),
];

const isoDeRow4: ActionEntry[] = [
  kb(100, "< | >"),
  kb(54, ", ;"),
  kb(55, ". :"),
  kb(56, "- _"),
];

const editing: ActionEntry[] = [
  kb(41, "Esc"),
  kb(43, "Tab"),
  kb(42, "Backspace"),
  kb(40, "Enter"),
  kb(44, "Space"),
  kb(57, "Caps Lock"),
];

const navigation: ActionEntry[] = [
  kb(74, "Home"),
  kb(77, "End"),
  kb(75, "Page Up"),
  kb(78, "Page Down"),
  kb(73, "Insert"),
  kb(76, "Delete"),
  kb(82, "↑"),
  kb(81, "↓"),
  kb(80, "←"),
  kb(79, "→"),
];

const functionKeys: ActionEntry[] = Array.from({ length: 12 }, (_, i) =>
  kb(58 + i, `F${i + 1}`),
);

const modifiers: ActionEntry[] = [
  kb(224, "L-Ctrl"),
  kb(225, "L-Shift"),
  kb(226, "L-Alt"),
  kb(227, "L-Win"),
  kb(228, "R-Ctrl"),
  kb(229, "R-Shift"),
  kb(230, "AltGr"),
  kb(231, "R-Win"),
];

const media: ActionEntry[] = [
  cons(0x00CD, "Play / Pause"),
  cons(0x00B5, "Next Track"),
  cons(0x00B6, "Prev Track"),
  cons(0x00B7, "Stop"),
  cons(0x00E9, "Volume +"),
  cons(0x00EA, "Volume −"),
  cons(0x00E2, "Mute"),
];

const special: ActionEntry[] = [
  { label: "Factory default", action: { kind: "default" }, hint: "↺" },
];

export const ACTION_GROUPS: ActionGroup[] = [
  {
    id: "basic",
    name: "Letters & digits",
    description: "Every alphanumeric key from the HID keyboard usage page.",
    entries: [...letters, ...digits, ...isoDeRow1, ...isoDeRow2, ...isoDeRow3, ...isoDeRow4],
  },
  {
    id: "editing",
    name: "Editing & navigation",
    entries: [...editing, ...navigation],
  },
  {
    id: "function",
    name: "Function keys",
    entries: functionKeys,
  },
  {
    id: "modifiers",
    name: "Modifiers",
    entries: modifiers,
  },
  {
    id: "media",
    name: "Media (Consumer)",
    entries: media,
  },
  {
    id: "special",
    name: "Special",
    description: "Resets this slot to the keyboard's factory mapping.",
    entries: special,
  },
];
