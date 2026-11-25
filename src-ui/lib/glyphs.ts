// The app marks from the design kit: the monochrome glyphs (src-ui/design/glyphs/arcade.<app>.svg) and each app's
// coloured icon (src-ui/design/app-icons/arcade.<app>.svg).
const urls = import.meta.glob<string>('../design/glyphs/*.svg', { eager: true, query: '?url', import: 'default' });
const icons = import.meta.glob<string>('../design/app-icons/*.svg', { eager: true, query: '?url', import: 'default' });

/** The URL of an app's glyph; Arcade Tools' mark for an unknown id. */
export function glyph(appId: string): string {
  return urls[`../design/glyphs/${appId}.svg`] ?? urls['../design/glyphs/arcade.tools.svg'];
}

/** The URL of an Arcade app's coloured icon, or null for anything that isn't one. */
export function appIcon(appId: string): string | null {
  return icons[`../design/app-icons/${appId}.svg`] ?? null;
}
