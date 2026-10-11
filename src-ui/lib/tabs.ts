export const tabs = [
  { id: 'home', label: 'Home', enabled: true },
  { id: 'apps', label: 'Apps', enabled: true },
  { id: 'updates', label: 'Updates', enabled: true },
  { id: 'shortcuts', label: 'Shortcuts', enabled: false },
  { id: 'learn', label: 'Learn', enabled: true },
  { id: 'connections', label: 'Connections', enabled: true },
  { id: 'engines', label: 'Engines', enabled: true },
  { id: 'settings', label: 'Settings', enabled: true },
  { id: 'onboarding', label: 'Onboarding', enabled: true },
] as const;

export type ShellTab = (typeof tabs)[number]['id'];
