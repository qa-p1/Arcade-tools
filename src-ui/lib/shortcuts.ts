export type ShortcutSource = 'catalog' | 'arcade';
export type WindowManagerShortcutState = 'active' | 'configuredOnly' | 'runtimeOnly';

export interface ShortcutKeys {
  keys: string[];
  display: string[];
}

export interface AppShortcut extends ShortcutKeys {
  id: string;
  title: string;
  description: string | null;
}

export interface ShortcutGroup {
  title: string;
  context?: string;
  shortcuts: AppShortcut[];
}

export interface ShortcutSheet {
  id: string;
  name: string;
  source: ShortcutSource;
  match?: { class?: string[] };
  groups: ShortcutGroup[];
}

export interface ArcadeShortcut extends ShortcutKeys {
  app: string;
  name: string;
  id: string;
  title: string;
  rebindable: boolean;
}

export interface WindowManagerShortcut {
  chord: string;
  display: string;
  label: string;
  detail: string;
  submap: string | null;
  state: WindowManagerShortcutState;
  origin: { file: string; line: number };
}

export interface WindowManagerGroup {
  title: string;
  shortcuts: WindowManagerShortcut[];
}

export interface ShortcutOwner {
  kind: 'arcade' | 'wm' | string;
  owner: string;
  label: string;
}

export interface ShortcutConflict {
  chord: string;
  a: ShortcutOwner;
  b: ShortcutOwner;
}

export interface ShortcutsIndex {
  schema: number;
  builtAt: string;
  os: 'linux' | 'windows' | 'macos' | string;
  apps: ShortcutSheet[];
  arcade: ArcadeShortcut[];
  wm: {
    name: string;
    configKind: string;
    groups: WindowManagerGroup[];
    warnings: string[];
  };
  conflicts: ShortcutConflict[];
}
