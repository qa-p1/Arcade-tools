// Tray, shortcut overlay and start-at-login preferences, shared by the rows
// in Settings (General, Shortcuts, Tray) and the onboarding tray step.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ui } from './state.svelte';

export type TrayPreferences = {
  hostIcons: boolean;
  excluded: string[];
  showUpdateBadge: boolean;
  startAtLogin: boolean;
  shortcutsEnabled: boolean;
  shortcut: string;
};
export type TrayView = {
  preferences: TrayPreferences;
  apps: { id: string; name: string; excluded: boolean }[];
  loginAvailable: boolean;
  shortcutStatus: { accelerator: string; available: boolean; via: string | null; hint: string | null; error: string | null };
};

export const tray = $state<{ view: TrayView | null; saving: boolean; error: string; failed: boolean }>({
  view: null, saving: false, error: '', failed: false,
});

function message(failure: unknown): string {
  return (failure as { message?: string }).message ?? String(failure);
}

export async function loadTray(): Promise<void> {
  try {
    tray.view = await invoke<TrayView>('tray_get_settings');
    if (ui.testing) ui.shortcutsRevision += 1;
    tray.error = '';
    tray.failed = false;
  } catch (failure) {
    tray.error = message(failure);
    tray.failed = tray.view === null;
  }
}

export async function saveTray(change: Partial<TrayPreferences>): Promise<void> {
  const view = tray.view;
  if (!view || tray.saving) return;
  tray.saving = true;
  tray.error = '';
  try {
    const preferences = await invoke<TrayPreferences>('tray_set_settings', {
      preferences: { ...view.preferences, ...change },
    });
    tray.view = { ...view, preferences };
    if (ui.testing) ui.shortcutsRevision += 1;
  } catch (failure) {
    tray.error = message(failure);
  } finally {
    tray.saving = false;
  }
}

export function keepOwnIcon(id: string, own: boolean): void {
  if (!tray.view) return;
  const excluded = tray.view.preferences.excluded.filter((app) => app !== id);
  if (own) excluded.push(id);
  void saveTray({ excluded });
}

let subscribed = false;
/** Reloads when the tray, overlay or installed apps change elsewhere. */
export function watchTray(isActive: () => boolean): void {
  if (subscribed) return;
  subscribed = true;
  const reload = () => { if (isActive()) void loadTray(); };
  void Promise.all([
    listen('tray-settings-changed', reload),
    listen('overlay-shortcut-changed', reload),
    listen('registry-changed', reload),
  ]).catch(() => { subscribed = false; /* Startup can precede the resident service. */ });
}
