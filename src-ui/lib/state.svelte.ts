import type { AppView, Operation, OperationRequest, Release, UpdateState } from './api';
import type { ShortcutsIndex } from './shortcuts';
import type { CatalogData } from './catalog';
import type { ShellTab } from './tabs.svelte';

export type Theme = 'system' | 'light' | 'dark';
export type ConfirmationKind = 'remove-data' | 'background' | 'reopen-mode' | 'app-closed' | null;

export interface ConfirmationState {
  title: string;
  copy: string;
  action: string;
  destructive: boolean;
  kind: ConfirmationKind;
  folders: string[];
  removeData: boolean;
  background: boolean;
  mode: 'foreground' | 'background';
  appClosed: boolean;
}

export interface ConfirmationResult {
  confirmed: boolean;
  removeData: boolean;
  background: boolean;
  mode: 'foreground' | 'background';
  appClosed: boolean;
}

export const ui = $state({
  apps: [] as AppView[],
  /** False until the first app list arrives: empty states wait for it. */
  loaded: false,
  toolsApp: null as AppView | null,
  notificationsEnabled: true,
  learnTarget: 'arcade.ecosystem',
  releases: new Map<string, Release>(),
  updates: null as UpdateState | null,
  updateChecking: false,
  updateChecksEnabled: true,
  autoUpdate: {} as Record<string, boolean>,
  selected: null as string | null,
  busy: false,
  refreshing: false,
  refreshPending: false,
  testing: false,
  message: 'Loading your apps…',
  messageError: false,
  dialog: null as ConfirmationState | null,
  phase: '',
  progress: null as number | null,
  cancellable: true,
  linkEnabled: true,
  diagnostics: '',
  catalogData: null as CatalogData | null,
  catalogLoading: false,
  releaseNotesHtml: {} as Record<string, string>,
  theme: 'system' as Theme,
  tab: 'apps' as ShellTab,
  /** The user picked a page; startup must not move them elsewhere. */
  tabChosen: false,
  /** Set by the launcher's Uninstall action: Settings scrolls to the review. */
  reviewUninstall: false,
  shortcutsIndex: null as ShortcutsIndex | null,
  shortcutLabels: {} as Record<string, string>,
  shortcutsRevision: 0,
});

let confirmationResolver: ((result: ConfirmationResult) => void) | null = null;

export function askForConfirmation(
  confirmation: Pick<ConfirmationState, 'title' | 'copy' | 'action' | 'destructive' | 'kind' | 'folders'>,
): Promise<ConfirmationResult> {
  if (confirmationResolver) finishConfirmation(false);
  return new Promise((resolve) => {
    confirmationResolver = resolve;
    ui.dialog = {
      ...confirmation,
      removeData: false,
      background: false,
      mode: 'foreground',
      appClosed: false,
    };
  });
}

export function finishConfirmation(confirmed: boolean): void {
  const current = ui.dialog;
  if (!current) return;
  const result: ConfirmationResult = {
    confirmed,
    removeData: current.removeData,
    background: current.background,
    mode: current.mode,
    appClosed: current.appClosed,
  };
  ui.dialog = null;
  const resolve = confirmationResolver;
  confirmationResolver = null;
  resolve?.(result);
}

export function rememberRelease(release: Release): void {
  ui.releases = new Map(ui.releases).set(release.id, release);
}

export function forgetRelease(id: string): void {
  const next = new Map(ui.releases);
  next.delete(id);
  ui.releases = next;
}

export function operationRequest(id: string, operation: Operation): OperationRequest {
  return { id, operation, mode: null, removeData: false, appClosed: false };
}
