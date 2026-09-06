export interface KeyDef {
  l?: string;
  /** flex-grow unit (≈ key width) */
  u?: number;
  gap?: boolean;
  space?: boolean;
}

export const KB_ROWS: KeyDef[][] = [
  [
    { l: "esc", u: 1.1 },
    { l: "F1", u: 1 }, { l: "F2", u: 1 }, { l: "F3", u: 1 }, { l: "F4", u: 1 },
    { gap: true, u: 0.4 },
    { l: "F5", u: 1 }, { l: "F6", u: 1 }, { l: "F7", u: 1 }, { l: "F8", u: 1 },
    { gap: true, u: 0.4 },
    { l: "F9", u: 1 }, { l: "F10", u: 1 }, { l: "F11", u: 1 }, { l: "F12", u: 1 },
    { l: "prt", u: 1.1 },
  ],
  [
    { l: "`", u: 1 }, { l: "1", u: 1 }, { l: "2", u: 1 }, { l: "3", u: 1 }, { l: "4", u: 1 },
    { l: "5", u: 1 }, { l: "6", u: 1 }, { l: "7", u: 1 }, { l: "8", u: 1 }, { l: "9", u: 1 },
    { l: "0", u: 1 }, { l: "-", u: 1 }, { l: "=", u: 1 }, { l: "⌫", u: 2 },
  ],
  [
    { l: "tab", u: 1.5 },
    { l: "Q", u: 1 }, { l: "W", u: 1 }, { l: "E", u: 1 }, { l: "R", u: 1 }, { l: "T", u: 1 },
    { l: "Y", u: 1 }, { l: "U", u: 1 }, { l: "I", u: 1 }, { l: "O", u: 1 }, { l: "P", u: 1 },
    { l: "[", u: 1 }, { l: "]", u: 1 }, { l: "\\", u: 1.5 },
  ],
  [
    { l: "caps", u: 1.8 },
    { l: "A", u: 1 }, { l: "S", u: 1 }, { l: "D", u: 1 }, { l: "F", u: 1 }, { l: "G", u: 1 },
    { l: "H", u: 1 }, { l: "J", u: 1 }, { l: "K", u: 1 }, { l: "L", u: 1 },
    { l: "Ñ", u: 1 }, { l: ";", u: 1 }, { l: "'", u: 1 }, { l: "⏎", u: 2.2 },
  ],
  [
    { l: "shift", u: 2.3 },
    { l: "Z", u: 1 }, { l: "X", u: 1 }, { l: "C", u: 1 }, { l: "V", u: 1 }, { l: "B", u: 1 },
    { l: "N", u: 1 }, { l: "M", u: 1 }, { l: ",", u: 1 }, { l: ".", u: 1 }, { l: "/", u: 1 },
    { l: "shift", u: 2.7 },
  ],
  [
    { l: "ctrl", u: 1.25 }, { l: "win", u: 1 }, { l: "alt", u: 1.25 },
    { gap: true, u: 0.5 },
    { l: "", u: 5.5, space: true },
    { gap: true, u: 0.5 },
    { l: "alt", u: 1.25 }, { l: "fn", u: 1 }, { l: "ctrl", u: 1.25 },
  ],
];
