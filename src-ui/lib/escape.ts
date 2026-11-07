// ARCADE_APP.md UI-2: Escape goes back one step. An open dialog handles its own Escape (it is cancelled); a search
// or text field with text is cleared; otherwise the page's sub-view (details, guide reader) returns to its parent.
// The top level does nothing: the window closes only from its title bar or the tray.

/** Window capture phase, before any page sees the key: clear a filled-in text field. */
export function clearFieldOnEscape(event: KeyboardEvent): void {
  if (event.key !== 'Escape' || event.defaultPrevented || openDialog()) return;
  const field = event.target;
  if (field instanceof HTMLInputElement && (field.type === 'search' || field.type === 'text') && field.value) {
    event.preventDefault();
    field.value = '';
    field.dispatchEvent(new Event('input', { bubbles: true }));
  }
}

/** True when this Escape is the page's to use for going back one step. */
export function escapeGoesBack(event: KeyboardEvent): boolean {
  return event.key === 'Escape' && !event.defaultPrevented && !event.repeat && !openDialog();
}

function openDialog(): boolean {
  return document.querySelector('dialog[open]') !== null;
}
