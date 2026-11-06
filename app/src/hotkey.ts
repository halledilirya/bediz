const NAMED: Record<number, string> = {
  44: "Print Screen",
  19: "Pause",
  145: "Scroll Lock",
  45: "Insert",
  46: "Delete",
  36: "Home",
  35: "End",
  33: "Page Up",
  34: "Page Down",
  38: "Up",
  40: "Down",
  37: "Left",
  39: "Right",
  32: "Space",
};

const ALONE = new Set(["Print Screen", "Pause", "Scroll Lock"]);
const MODIFIERS = new Set([16, 17, 18, 91, 92]);
const F1 = 112;
const F24 = 135;

export interface Held {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
}

export interface Pressed extends Held {
  virtualKey: number;
  meta: boolean;
}

export type Picked =
  | { kind: "hotkey"; hotkey: string }
  | { kind: "modifiers" }
  | { kind: "unsupported" }
  | { kind: "needs-modifier"; key: string };

export function keyName(virtualKey: number): string | null {
  const letter = virtualKey >= 65 && virtualKey <= 90;
  const digit = virtualKey >= 48 && virtualKey <= 57;
  if (letter || digit) {
    return String.fromCharCode(virtualKey);
  }
  if (virtualKey >= F1 && virtualKey <= F24) {
    return `F${String(virtualKey - F1 + 1)}`;
  }
  return NAMED[virtualKey] ?? null;
}

export function held(keys: Held): string[] {
  const names: string[] = [];
  if (keys.ctrl) {
    names.push("Ctrl");
  }
  if (keys.alt) {
    names.push("Alt");
  }
  if (keys.shift) {
    names.push("Shift");
  }
  return names;
}

export function pick(pressed: Pressed): Picked {
  if (MODIFIERS.has(pressed.virtualKey)) {
    return { kind: "modifiers" };
  }
  const key = keyName(pressed.virtualKey);
  if (key === null || pressed.meta) {
    return { kind: "unsupported" };
  }
  const functionKey = pressed.virtualKey >= F1 && pressed.virtualKey <= F24;
  if (!pressed.ctrl && !pressed.alt && !functionKey && !ALONE.has(key)) {
    return { kind: "needs-modifier", key: [...held(pressed), key].join("+") };
  }
  return { kind: "hotkey", hotkey: [...held(pressed), key].join("+") };
}

export function keysOf(hotkey: string): string[] {
  return hotkey === "" ? [] : hotkey.split("+");
}
