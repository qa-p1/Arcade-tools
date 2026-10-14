export type ShellTab = 'home' | 'apps' | 'updates' | 'shortcuts' | 'learn' | 'connections' | 'engines' | 'settings' | 'onboarding';
export interface ShellTabModel {
  id: ShellTab;
  label: string;
  enabled: boolean;
}

export const tabs: ShellTabModel[] = $state([
  { id: 'home', label: 'Home', enabled: true },
  { id: 'apps', label: 'Apps', enabled: true },
  { id: 'updates', label: 'Updates', enabled: true },
  { id: 'shortcuts', label: 'Shortcuts', enabled: false },
  { id: 'learn', label: 'Learn', enabled: true },
  { id: 'connections', label: 'Connections', enabled: true },
  { id: 'engines', label: 'Engines', enabled: true },
  { id: 'settings', label: 'Settings', enabled: true },
  { id: 'onboarding', label: 'Onboarding', enabled: true },
]);

export function setShortcutsEnabled(enabled: boolean): void {
  const shortcuts = tabs.find((tab) => tab.id === 'shortcuts');
  if (shortcuts) shortcuts.enabled = enabled;
}
