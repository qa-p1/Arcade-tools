// Key chords as Tools stores them ("Ctrl+Alt+S", "Super+Slash"): read from a
// key press, compared, checked and shown with platform names (STANDARD §5.1).

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

const MODIFIERS = ['Ctrl', 'Alt', 'Shift', 'Super'] as const;
const MAC_SYMBOLS: Record<string, string> = { Ctrl: '⌃', Alt: '⌥', Option: '⌥', Shift: '⇧', Super: '⌘', Cmd: '⌘' };
const KEY_SYMBOLS: Record<string, string> = {
  Slash: '/', Backslash: '\\', Period: '.', Comma: ',', Minus: '-', Equal: '=', Semicolon: ';', Quote: "'",
  Backquote: '`', BracketLeft: '[', BracketRight: ']',
};
const ALIASES: Record<string, string> = {
  control: 'ctrl', meta: 'super', win: 'super', mod4: 'super', logo: 'super', cmd: 'super', mod1: 'alt', option: 'alt',
  // "Super+/" and "Super+Slash" are the same chord.
  ...Object.fromEntries(Object.entries(KEY_SYMBOLS).map(([name, symbol]) => [symbol, name.toLowerCase()])),
};

/** The chord a key press makes, or null while only modifiers are down. */
export function chordFromEvent(event: KeyboardEvent): string | null {
  if (['Control', 'Alt', 'Shift', 'Meta', 'OS', 'AltGraph'].includes(event.key)) return null;
  const key = event.code.replace(/^Key/, '').replace(/^Digit/, '').replace(/^Arrow/, '') || event.key;
  const held = [event.ctrlKey, event.altKey, event.shiftKey, event.metaKey];
  return [...MODIFIERS.filter((_, i) => held[i]), key].join('+');
}

/** Comparable form of a chord: lower case, modifiers sorted, aliases folded. */
export function normalizeChord(chord: string): string {
  const parts = chord.toLowerCase().replace(/\s+/g, '').split('+').filter(Boolean).map((part) => ALIASES[part] ?? part);
  const key = parts.pop() ?? '';
  return [...new Set(parts)].sort().concat(key).join('+');
}

/**
 * A global shortcut takes its key away from every app, so a plain or
 * Shift-only key (Shift+S would steal capital S) needs Ctrl, Alt or Super.
 * F1–F24 and media keys may stand alone.
 */
export function needsModifier(chord: string): boolean {
  const parts = normalizeChord(chord).split('+');
  const key = parts.at(-1) ?? '';
  if (!chord.trim() || /^(f([1-9]|1[0-9]|2[0-4])|print|printscreen|audio.*|media.*|volume.*)$/.test(key)) return false;
  return !parts.slice(0, -1).some((part) => part === 'ctrl' || part === 'alt' || part === 'super');
}

/** The keys of a chord as the platform names them: "Ctrl Alt S", "⌃ ⌥ S". */
export function chordParts(chord: string, mac = isMac): string[] {
  return chord.split(/\s*\+\s*(?=.)/).filter(Boolean).map((part) => {
    if (mac && MAC_SYMBOLS[part]) return MAC_SYMBOLS[part];
    return KEY_SYMBOLS[part] ?? part;
  });
}
