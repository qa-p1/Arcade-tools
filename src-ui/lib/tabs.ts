export const tabs = [
  { id: 'home', label: 'Home', enabled: false },
  { id: 'apps', label: 'Apps', enabled: true },
  { id: 'updates', label: 'Updates', enabled: false },
  { id: 'shortcuts', label: 'Shortcuts', enabled: false },
  { id: 'learn', label: 'Learn', enabled: false },
  { id: 'connections', label: 'Connections', enabled: false },
  { id: 'engines', label: 'Engines', enabled: false },
  { id: 'settings', label: 'Settings', enabled: true },
] as const;

export type ShellTab = (typeof tabs)[number]['id'];
