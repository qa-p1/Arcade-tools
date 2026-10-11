import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';

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

export interface ChannelUpdate {
  latestVersion: string | null;
  updateAvailable: boolean;
  heldBack: boolean;
  reason: string | null;
  error: string | null;
  cache: { release: { version: string; notes: string; channel: Channel } } | null;
}

export interface AppUpdate {
  installedVersion: string | null;
  channel: Channel;
  latest: Partial<Record<Channel, ChannelUpdate>>;
  updateAvailable: boolean;
  heldBack: boolean;
  reason: string | null;
}

export interface UpdateState {
  schema: number;
  lastCheck: number | null;
  lastSuccess: number | null;
  lastFailure: { time: number; reason: string } | null;
  apps: Record<string, AppUpdate>;
}

export type BundleApps = string | string[];
export interface Bundle {
  id: string;
  title: string;
  description: string;
  apps: BundleApps;
}

export interface LearnApp {
  id: string;
  name: string;
  installedVersion: string | null;
  docsVersion: string | null;
  available: boolean;
  source: 'installed' | 'cached' | 'bundled' | null;
  needsFetch: boolean;
  docsUrl: string;
}

export interface DocsSearchHit {
  app: string;
  version: string;
  path: string;
  title: string;
  snippet: string;
  score: number;
}

export interface EngineView {
  id: string;
  name: string;
  homepage: string;
  binaries: string[];
  versionArgs: string[];
  minimumVersion: string | null;
  usedBy: { app: string; purpose: string }[];
  status: { status: string; path?: string; version?: string | null; minimum?: string; message?: string };
  installHint: string | null;
  linkDownload: boolean;
}

export interface ConnectionPeer {
  id: string;
  name: string;
  version: string;
  linkEnabled: boolean;
  reachable: boolean;
  busy: boolean | null;
  protocols: number[];
  actions: { id: string; title: string; verb: string; available: boolean; reason: string | null }[];
}

export interface ConnectionsView {
  linkEnabled: boolean;
  peers: ConnectionPeer[];
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
  preferences: {
    linkEnabled: boolean;
    channels: Record<string, Channel>;
    updates: { enabled: boolean; autoUpdate: Record<string, boolean> };
  };
  updates: UpdateState;
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
  tab: string;
  visiblePanel: string | null;
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
  updateState: () => invoke<UpdateState>('update_state'),
  checkUpdates: () => invoke<UpdateState>('check_updates'),
  checkUpdatesWindowOpen: () => invoke<UpdateState>('check_updates_window_open'),
  setUpdateChecks: (enabled: boolean) => invoke<void>('set_update_checks', { enabled }),
  setAutoUpdate: (id: string, enabled: boolean) => invoke<void>('set_auto_update', { id, enabled }),
  operate: (request: OperationRequest) => invoke<string>('operate', { request }),
  cancelOperation: () => invoke<void>('cancel_operation'),
  setChannel: (id: string, channel: Channel) => invoke<void>('set_channel', { id, channel }),
  launchApp: (id: string, mode: LaunchMode) => invoke<void>('launch_app', { id, mode }),
  setLogin: (id: string, enabled: boolean) => invoke<void>('set_login', { id, enabled }),
  setLink: (enabled: boolean) => invoke<void>('set_link', { enabled }),
  openReleases: (id: string) => invoke<void>('open_releases', { id }),
  listBundles: () => invoke<Bundle[]>('list_bundles'),
  listEngines: (refresh: boolean) => invoke<EngineView[]>('list_engines', { refresh }),
  listConnections: () => invoke<ConnectionsView>('list_connections'),
  listLearnApps: () => invoke<LearnApp[]>('list_learn_apps'),
  searchDocs: (query: string) => invoke<DocsSearchHit[]>('search_docs', { query }),
  fetchDocs: (id: string, version: string) => invoke<void>('fetch_docs', { id, version }),
  openDocs: (id: string) => invoke<void>('open_docs', { id }),
  renderMarkdown: (markdown: string) => invoke<string>('render_markdown', { markdown }),
  uiRendered: (report: UiRenderReport) => invoke<void>('ui_rendered', { report }),
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
