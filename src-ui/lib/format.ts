import { formatRelative } from '../design/format';

/** formatRelative for use inside a sentence: "checked just now", "yesterday 14:05". */
export function relativeInSentence(timeMs: number): string {
  const text = formatRelative(timeMs);
  return text.startsWith('Just') || text.startsWith('Yesterday') ? text[0].toLowerCase() + text.slice(1) : text;
}
