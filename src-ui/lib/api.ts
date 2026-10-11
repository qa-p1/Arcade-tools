import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { CatalogData } from './catalog';
import type { ShortcutsIndex } from './shortcuts';

export type Channel = 'stable' | 'nightly';
export type LaunchMode = 'foreground' | 'background';
export type Operation = 'install' | 'update' | 'repair' | 'uninstall';

export interface AppView {
  id: string;
  name: string;
  pitch: string;
  releasesUrl: string;
  version: string | null;
  managed: boolean;
  installed: boolean;
  running: boolean;
  channel: Channel;
  healthy: boolean;
  startAtLogin: boolean;
  loginSupported: boolean;
  executable: string | null;
  dataFolders: string[];
  reason: string | null;
  description: string;
  category: string;
  icon: string;
  platforms: Record<string, string>;
  minTools: string;
  installReason: string | null;
  installSupported: boolean;
}

export interface Release {
  id: string;
  version: string;
  channel: Channel;
  file: string;
  notes: string;
  updateAvailable: boolean;
}

export interface OperationProgress {
  id: string;
  phase: string;
  fraction: number | null;
  cancellable: boolean;
}

export interface OperationRequest {
  id: string;
  operation: Operation;
  mode: LaunchMode | null;
  removeData: boolean;
  appClosed: boolean;
}

export interface Bootstrap {
  apps: AppView[];
  preferences: { linkEnabled: boolean; channels: Record<string, Channel> };
  selected: string | null;
  diagnostics: [string, string][];
  testing: boolean;
}

export interface UiControlReport {
  label: string;
  id: string;
  app?: string;
  checked?: boolean;
  disabled: boolean;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface UiRenderReport {
  revision: number;
  apps: AppView[];
  selected: string | null;
  busy: boolean;
  message: string;
  dialog: string | null;
  controls: UiControlReport[];
  phase: string;
}

export interface ManagerFailure {
  code: string;
  message: string;
}

export const api = {
  initialize: () => invoke<Bootstrap>('initialize'),
  listApps: () => invoke<AppView[]>('list_apps'),
  checkRelease: (id: string) => invoke<Release>('check_release', { id }),
  operate: (request: OperationRequest) => invoke<string>('operate', { request }),
  cancelOperation: () => invoke<void>('cancel_operation'),
  setChannel: (id: string, channel: Channel) => invoke<void>('set_channel', { id, channel }),
  launchApp: (id: string, mode: LaunchMode) => invoke<void>('launch_app', { id, mode }),
  setLogin: (id: string, enabled: boolean) => invoke<void>('set_login', { id, enabled }),
  setLink: (enabled: boolean) => invoke<void>('set_link', { enabled }),
  openReleases: (id: string) => invoke<void>('open_releases', { id }),
  catalogData: () => invoke<CatalogData>('catalog_data'),
  refreshCatalog: () => invoke<CatalogData>('refresh_catalog'),
  openCatalogDocs: (id: string) => invoke<void>('open_catalog_docs', { id }),
  renderMarkdown: (markdown: string) => invoke<string>('render_markdown', { markdown }),
  installCatalogApp: (id: string) => invoke<void>('install_catalog_app', { id }),
  updateCatalogApp: (id: string) => invoke<void>('update_catalog_app', { id }),
  uninstallCatalogApp: (id: string) => invoke<void>('uninstall_catalog_app', { id }),
  installCatalogBundle: (bundleId: string, appIds: string[]) =>
    invoke<void>('install_catalog_bundle', { bundleId, appIds }),
  uiRendered: (report: UiRenderReport) => invoke<void>('ui_rendered', { report }),
  shortcutsIndex: () => invoke<ShortcutsIndex | null>('shortcuts_index'),
  shortcutLabels: () => invoke<Record<string, string>>('shortcut_labels'),
  setShortcutLabel: (key: string, label: string | null) =>
    invoke<void>('set_shortcut_label', { key, label }),
};

export const appEvents = {
  operationProgress: (handler: (progress: OperationProgress) => void): Promise<UnlistenFn> =>
    listen<OperationProgress>('operation-progress', ({ payload }) => handler(payload)),
  installRequest: (handler: (id: string) => void): Promise<UnlistenFn> =>
    listen<string>('install-request', ({ payload }) => handler(payload)),
  registryChanged: (handler: () => void): Promise<UnlistenFn> =>
    listen('registry-changed', handler),
  managerMessage: (handler: (message: string) => void): Promise<UnlistenFn> =>
    listen<string>('manager-message', ({ payload }) => handler(payload)),
};
