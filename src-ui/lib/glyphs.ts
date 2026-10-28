// The app marks from the design kit (src-ui/design/glyphs/arcade.<app>.svg).
const urls = import.meta.glob<string>('../design/glyphs/*.svg', { eager: true, query: '?url', import: 'default' });

/** The URL of an app's glyph; Arcade Tools' mark for an unknown id. */
export function glyph(appId: string): string {
  return urls[`../design/glyphs/${appId}.svg`] ?? urls['../design/glyphs/arcade.tools.svg'];
}
