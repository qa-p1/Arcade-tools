import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { CatalogData } from './catalog';
import type { ShortcutsIndex } from './shortcuts';

export type Channel = 'stable' | 'nightly';
export type LaunchMode = 'foreground' | 'background';
export type Operation = 'install' | 'update' | 'repair' | 'uninstall' | 'adopt' | 'rollback';

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
  installationClass: 'managed' | 'legacyManaged' | 'selfInstalled' | 'manual' | 'dev';
  rollbackVersion: string | null;
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
  cache: { body: string; release: { version: string; notes: string; channel: Channel } } | null;
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
  downloaded: boolean;
}
export interface Preferences {
  linkEnabled: boolean;
  channels: Record<string, Channel>;
  updates: { enabled: boolean; autoUpdate: Record<string, boolean> };
  notificationsEnabled: boolean;
  theme: 'system' | 'light' | 'dark';
  onboardingComplete: boolean;
}
export interface Dashboard {
  apps: { id: string; pid: number | null; memoryBytes: number | null; cpuPercent: number | null; uptimeSeconds: number | null; tray: string | null; linkEnabled: boolean; reachable: boolean }[];
  totalMemoryBytes: number;
  totalCpuPercent: number;
}
export interface HealthIssue {
  app: string; kind: string; path: string | null; reason: string; canFix: boolean;
  fixAction: 'repair' | 'adopt' | 'remove_stale_registry' | 'remove_stale_receipt' | 'open_settings' | 'check_updates' | 'open_engines' | 'open_shortcuts';
}
export interface HistoryEntry { time: number; app: string; from: string | null; to: string | null; result: string; reason: string | null }
export interface DiagnosticsReport { file: string; issueUrl: string; omitted: string[] }
export interface BackupApp { id: string; version: string; channel: Channel; settings: string[]; notIncluded: string | null }
export interface BackupReport { file: string; apps: BackupApp[] }
export interface RestorePlan { sha256: string; apps: { backup: BackupApp; installed: boolean }[] }
export interface RestoreResult { app: string; imported: boolean; restartRequired: boolean; message: string }
export interface UninstallPlan { schema: number; fingerprint: string; removeData: boolean; apps: { id: string; version: string | null; canUninstall: boolean; requiresSelfShutdown: boolean; folders: string[]; dataFolders: string[]; reason: string | null }[] }
export interface DocPage { app: string; version: string; path: string; title: string; html: string; source: string; needsFetch: boolean }

export interface ConnectionPeer {
  id: string;
  name: string;
  version: string;
  linkEnabled: boolean;
  reachable: boolean;
  busy: boolean | null;
  protocols: number[];
  actions: { id: string; title: string; verb: string; available: boolean; reason: string | null; accepts: string[]; produces: string[] }[];
  connectedApps: Record<string, unknown>;
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
  preferences: Preferences;
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
  checkUpdates: () => invoke<UpdateState>('check_updates', { online: navigator.onLine }),
  checkUpdatesWindowOpen: () => invoke<UpdateState>('check_updates_window_open', { online: navigator.onLine }),
  setUpdateChecks: (enabled: boolean) => invoke<void>('set_update_checks', { enabled }),
  setAutoUpdate: (id: string, enabled: boolean) => invoke<void>('set_auto_update', { id, enabled }),
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
  installCatalogBundle: (bundleId: string, appIds: string[]) =>
    invoke<string[]>('install_catalog_bundle', { bundleId, appIds }),
  listBundles: () => invoke<Bundle[]>('list_bundles'),
  listEngines: (refresh: boolean) => invoke<EngineView[]>('list_engines', { refresh }),
  listConnections: () => invoke<ConnectionsView>('list_connections'),
  listLearnApps: () => invoke<LearnApp[]>('list_learn_apps'),
  searchDocs: (query: string) => invoke<DocsSearchHit[]>('search_docs', { query }),
  fetchDocs: (id: string, version: string) => invoke<void>('fetch_docs', { id, version }),
  fetchLatestDocs: (id: string) => invoke<string>('fetch_latest_docs', { id }),
  openDocs: (id: string) => invoke<void>('open_docs', { id }),
  preferences: () => invoke<Preferences>('preferences'),
  toolsApp: () => invoke<AppView | null>('tools_app'),
  setNotifications: (enabled: boolean) => invoke<void>('set_notifications', { enabled }),
  setTheme: (theme: string) => invoke<void>('set_theme', { theme }),
  completeOnboarding: () => invoke<void>('complete_onboarding'),
  dashboard: () => invoke<Dashboard>('dashboard'),
  health: () => invoke<HealthIssue[]>('health'),
  fixHealth: (id: string, action: HealthIssue['fixAction']) => invoke<string>('fix_health', { id, action }),
  openAppSettings: (id: string) => invoke<void>('open_app_settings', { id }),
  restartApp: (id: string) => invoke<void>('restart_app', { id }),
  releaseHistory: (id: string) => invoke<{ version: string; publishedAt: string; prerelease: boolean; html: string }[]>('release_history', { id }),
  refreshShortcutSheets: () => invoke<number>('refresh_shortcut_sheets'),
  updateHistory: () => invoke<HistoryEntry[]>('update_history'),
  createDiagnostics: (id: string) => invoke<DiagnosticsReport>('create_diagnostics', { id }),
  exportBackup: () => invoke<BackupReport>('export_backup'),
  planRestore: (path: string) => invoke<RestorePlan>('plan_restore', { path }),
  restoreBackup: (path: string, sha256: string) => invoke<RestoreResult[]>('restore_backup', { path, sha256 }),
  revealArchive: (path: string) => invoke<void>('reveal_archive', { path }),
  openIssue: (id: string, issueUrl?: string) => invoke<void>('open_issue', { id, issueUrl: issueUrl ?? null }),
  docsPages: (id: string, latest = false) => invoke<{ path: string; title: string }[]>('docs_pages', { id, latest }),
  readDoc: (id: string, path: string, latest = false) => invoke<DocPage>('read_doc', { id, path, latest }),
  downloadEngine: (id: string) => invoke<string>('download_engine', { id }),
  selfUpdate: () => invoke<void>('self_update'),
  selfRollback: () => invoke<void>('self_rollback'),
  installTools: () => invoke<void>('install_tools'),
  planUninstallAll: (removeData: boolean) => invoke<UninstallPlan>('plan_uninstall_all', { removeData }),
  uninstallToolsOnly: (removeData: boolean) => invoke<void>('uninstall_tools_only', { removeData }),
  uninstallAll: (plan: UninstallPlan) => invoke<{ results: { app: string; removed: boolean; reason: string }[] }>('uninstall_all', { plan }),
  uiRendered: (report: UiRenderReport) => invoke<void>('ui_rendered', { report }),
  setAppShortcut: (id: string, shortcut: string, accelerator: string) => invoke<{ applied: boolean; via: string; hint?: string }>('set_app_shortcut', { id, shortcut, accelerator }),
  shortcutsIndex: () => invoke<ShortcutsIndex | null>('shortcuts_index'),
  currentShortcutApp: () => invoke<string | null>('current_shortcut_app'),
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
  shortcutsChanged: (handler: () => void): Promise<UnlistenFn> => listen('shortcut-index-built', handler),
  trayNavigation: (handler: (page: string) => void): Promise<UnlistenFn> => listen<string>('tray-navigation', ({ payload }) => handler(payload)),
  updatesChanged: (handler: (state: UpdateState) => void): Promise<UnlistenFn> =>
    listen<UpdateState>('updates-changed', ({ payload }) => handler(payload)),
};
