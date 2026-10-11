export type ShellTab = 'home' | 'apps' | 'updates' | 'shortcuts' | 'learn' | 'connections' | 'engines' | 'settings';
export interface ShellTabModel {
  id: ShellTab;
  label: string;
  enabled: boolean;
}

export const tabs: ShellTabModel[] = $state([
  { id: 'home', label: 'Home', enabled: false },
  { id: 'apps', label: 'Apps', enabled: true },
  { id: 'updates', label: 'Updates', enabled: false },
  { id: 'shortcuts', label: 'Shortcuts', enabled: false },
  { id: 'learn', label: 'Learn', enabled: false },
  { id: 'connections', label: 'Connections', enabled: false },
  { id: 'engines', label: 'Engines', enabled: false },
  { id: 'settings', label: 'Settings', enabled: true },
]);

export function setShortcutsEnabled(enabled: boolean): void {
  const shortcuts = tabs.find((tab) => tab.id === 'shortcuts');
  if (shortcuts) shortcuts.enabled = enabled;
}
