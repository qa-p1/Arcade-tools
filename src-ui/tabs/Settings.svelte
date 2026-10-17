<script lang="ts">
  import { api, type AppView, type BackupReport, type DiagnosticsReport, type RestorePlan, type RestoreResult, type UninstallPlan } from '../lib/api';
  import { askForConfirmation, ui, type Theme } from '../lib/state.svelte';
  import { tick } from 'svelte';
  import TraySettings from '../components/TraySettings.svelte';

  type Props = {
    active: boolean;
    theme: Theme;
    linkEnabled: boolean;
    diagnostics: string;
    busy: boolean;
    toolsApp: AppView | null;
    apps: AppView[];
    updateChecksEnabled: boolean;
    notificationsEnabled: boolean;
    onUpdateChecks: (enabled: boolean) => Promise<void>;
    onOnboarding: () => void;
    onTheme: (theme: Theme) => void;
    onLink: (enabled: boolean) => Promise<void>;
    run: (action: () => Promise<void>) => void;
  };
  let {
    active, theme, linkEnabled, diagnostics, busy, toolsApp, apps, updateChecksEnabled, notificationsEnabled,
    onUpdateChecks, onOnboarding, onTheme, onLink, run,
  }: Props = $props();

  let notifications = $state<boolean | null>(null);
  let supportApp = $state('arcade.tools');
  let report = $state<DiagnosticsReport | null>(null);
  let backup = $state<BackupReport | null>(null);
  let restorePath = $state('');
  let restorePlan = $state<RestorePlan | null>(null);
  let restored = $state<RestoreResult[]>([]);
  let removeData = $state(false);
  let uninstallResults = $state<{ app: string; removed: boolean; reason: string }[]>([]);
  const installedApps = $derived(apps.filter((app) => app.installed));
  const notificationsOn = $derived(notifications ?? notificationsEnabled);

  async function setNotifications(enabled: boolean): Promise<void> {
    await api.setNotifications(enabled);
    notifications = enabled;
  }

  async function createDiagnostics(): Promise<void> {
    report = await api.createDiagnostics(supportApp);
  }

  async function exportBackup(): Promise<void> {
    backup = await api.exportBackup();
  }

  async function planRestore(): Promise<void> {
    restored = [];
    restorePlan = await api.planRestore(restorePath.trim());
  }

  async function restore(): Promise<void> {
    if (!restorePlan) return;
    const names = restorePlan.apps.map((entry) => entry.backup.id).join(', ');
    const choice = await askForConfirmation({ title: 'Restore settings from this backup?', copy: `Settings for ${names} will be replaced with the backed-up copies. Missing apps will be installed on their saved channels. Apps may ask to restart.`, action: 'Restore', destructive: false, kind: null, folders: [] });
    if (!choice.confirmed) return;
    restored = await api.restoreBackup(restorePath.trim(), restorePlan.sha256);
    restorePlan = null;
  }

  async function updateTools(): Promise<void> {
    const choice = await askForConfirmation({ title: 'Update Arcade Tools?', copy: 'Tools downloads the latest release, closes, and reopens itself. Your apps keep running.' + (toolsApp?.reason ? `\n${toolsApp.reason}` : ''), action: 'Update', destructive: false, kind: null, folders: [] });
    if (choice.confirmed) await api.selfUpdate();
  }

  async function installTools(): Promise<void> {
    await api.installTools();
  }

  let uninstallSection = $state<HTMLElement | null>(null);
  $effect(() => {
    if (!active || !ui.reviewUninstall) return;
    ui.reviewUninstall = false;
    void tick().then(() => {
      uninstallSection?.scrollIntoView({ block: 'center' });
      uninstallSection?.querySelector<HTMLButtonElement>('button')?.focus();
    });
  });

  async function uninstallToolsOnly(): Promise<void> {
    const choice = await askForConfirmation({
      title: 'Uninstall Arcade Tools?',
      copy: 'Tools closes and is removed. Your Arcade apps stay installed and keep updating themselves.'
        + (removeData ? '\nTools settings and data are deleted too.' : '\nTools settings and data are kept.'),
      action: 'Uninstall Tools', destructive: true, kind: null, folders: [],
    });
    if (choice.confirmed) await api.uninstallToolsOnly(removeData);
  }

  async function uninstallEverything(): Promise<void> {
    const plan: UninstallPlan = await api.planUninstallAll(removeData);
    const removable = plan.apps.filter((entry) => entry.canUninstall);
    const kept = plan.apps.filter((entry) => !entry.canUninstall);
    const lines = removable.map((entry) => `• ${entry.id}${entry.version ? ` ${entry.version}` : ''}`);
    if (kept.length) lines.push('', 'Kept: ' + kept.map((entry) => `${entry.id} (${entry.reason ?? 'not managed'})`).join(', '));
    const choice = await askForConfirmation({
      title: `Uninstall ${removable.length} Arcade ${removable.length === 1 ? 'app' : 'apps'}?`,
      copy: lines.join('\n') + (removeData ? '\nSettings and data are deleted too.' : '\nSettings and data are kept.'),
      action: 'Uninstall', destructive: true, kind: null,
      folders: removeData ? removable.flatMap((entry) => entry.dataFolders) : [],
    });
    if (!choice.confirmed) return;
    uninstallResults = (await api.uninstallAll(plan)).results;
  }
</script>

<div id="panel-settings" class="tab-panel settings-panel" role="tabpanel" aria-labelledby="tab-settings" tabindex="0" hidden={!active}>
  <header class="page-heading">
    <div>
      <p class="eyebrow">ARCADE TOOLS / PREFERENCES</p>
      <h1>Settings</h1>
      <p class="intro">Choose how Arcade Tools looks, connects, and looks after your apps.</p>
    </div>
  </header>

  <section class="settings-card" aria-labelledby="appearance-heading">
    <div>
      <h2 id="appearance-heading">Appearance</h2>
      <p class="setting-description">Follow your system or choose a theme.</p>
    </div>
    <label class="setting-control">Theme
      <select aria-label="Theme" value={theme} onchange={(event) => onTheme(event.currentTarget.value as Theme)}>
        <option value="system">System</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
  </section>

  <section class="settings-card" aria-labelledby="updates-settings-heading">
    <div>
      <h2 id="updates-settings-heading">Updates and notifications</h2>
      <p class="setting-description">Tools checks for updates a few times a day and tells you when one is ready. Nothing installs without your confirmation unless you turn on automatic updates for an app.</p>
    </div>
    <div class="settings-toggles">
      <label><input type="checkbox" checked={updateChecksEnabled} disabled={busy} onchange={(event) => run(() => onUpdateChecks(event.currentTarget.checked))} /> Check for updates automatically</label>
      <label><input type="checkbox" checked={notificationsOn} disabled={busy} onchange={(event) => run(() => setNotifications(event.currentTarget.checked))} /> Show notifications</label>
    </div>
  </section>

  <section class="settings-card" aria-labelledby="integration-heading">
    <div>
      <h2 id="integration-heading">Connections</h2>
      <p class="setting-description">Allow other Arcade apps to ask Tools to install an app after your confirmation.</p>
    </div>
    <label class="settings-link-toggle"><input id="settings-link-toggle" type="checkbox" checked={linkEnabled} disabled={busy} onchange={(event) => run(() => onLink(event.currentTarget.checked))} /> Connect with other Arcade apps</label>
  </section>

  <section class="settings-card" aria-labelledby="support-heading">
    <div>
      <h2 id="support-heading">Report a problem</h2>
      <p class="setting-description">Create a diagnostics file to attach to an issue. Personal paths and secrets are removed; nothing is sent automatically.</p>
    </div>
    <div class="settings-stack">
      <div class="actions-inline">
        <select aria-label="App to diagnose" bind:value={supportApp}>
          <option value="arcade.tools">Arcade Tools</option>
          {#each installedApps as app (app.id)}<option value={app.id}>{app.name}</option>{/each}
        </select>
        <button type="button" disabled={busy} onclick={() => run(createDiagnostics)}>Create diagnostics file</button>
      </div>
      {#if report}
        <p class="setting-description">Saved {report.file}{report.omitted.length ? ` · left out: ${report.omitted.join(', ')}` : ''}</p>
        <div class="actions-inline">
          <button type="button" onclick={() => run(() => api.revealArchive(report!.file))}>Show file</button>
          <button type="button" class="primary-action" onclick={() => run(() => api.openIssue(supportApp, report!.issueUrl))}>Open an issue</button>
        </div>
      {/if}
    </div>
  </section>

  <section class="settings-card" aria-labelledby="backup-heading">
    <div>
      <h2 id="backup-heading">Backup and restore</h2>
      <p class="setting-description">Save the settings of every installed Arcade app in one file, or bring them back on this or another computer.</p>
    </div>
    <div class="settings-stack">
      <div class="actions-inline"><button type="button" disabled={busy} onclick={() => run(exportBackup)}>Back up settings</button></div>
      {#if backup}
        <p class="setting-description">Saved {backup.file}</p>
        <ul class="settings-list">{#each backup.apps as app (app.id)}<li><strong>{app.id}</strong> v{app.version}{app.notIncluded ? ` · not included: ${app.notIncluded}` : ''}</li>{/each}</ul>
        <div class="actions-inline"><button type="button" onclick={() => run(() => api.revealArchive(backup!.file))}>Show file</button></div>
      {/if}
      <form class="actions-inline" onsubmit={(event) => { event.preventDefault(); run(planRestore); }}>
        <label class="visually-hidden" for="restore-path">Backup file</label>
        <input id="restore-path" type="text" placeholder="Path to an Arcade backup (.zip)" bind:value={restorePath} />
        <button type="submit" disabled={busy || !restorePath.trim()}>Review restore</button>
      </form>
      {#if restorePlan}
        <ul class="settings-list">{#each restorePlan.apps as entry (entry.backup.id)}<li><strong>{entry.backup.id}</strong> v{entry.backup.version}{entry.installed ? '' : ' · will be installed after confirmation'}</li>{/each}</ul>
        <div class="actions-inline"><button type="button" class="primary-action" disabled={busy} onclick={() => run(restore)}>Restore</button></div>
      {/if}
      {#if restored.length}
        <ul class="settings-list">{#each restored as result (result.app)}<li><strong>{result.app}</strong> {result.imported ? 'restored' : 'not restored'}{result.restartRequired ? ' · restart required' : ''} · {result.message}</li>{/each}</ul>
      {/if}
    </div>
  </section>
  <TraySettings {active} {busy} />

  <section class="settings-card diagnostics-card" aria-labelledby="diagnostics-heading">
    <div>
      <h2 id="diagnostics-heading">Diagnostics</h2>
      <p class="setting-description">Arcade Link status and local integration details.</p>
    </div>
    <pre id="diagnostics">{diagnostics}</pre>
  </section>

  <section class="settings-card about-card" aria-labelledby="about-heading">
    <div>
      <h2 id="about-heading">About</h2>
      <p class="setting-description">Arcade Tools keeps the Arcade app collection installed and up to date. Every app also works on its own.</p>
    </div>
    <div class="settings-stack">
      <p class="about-version">Arcade Tools <strong>{toolsApp?.version ?? '0.2.0'}</strong>{toolsApp?.managed ? ` · ${toolsApp.channel}` : ''}</p>
      <p class="setting-description">Arcade Link 0.3.0 · Tools is MIT licensed. The overlay includes code from Arcade Find (MIT or Apache-2.0), and catalog guides and sheets use CC BY 4.0.</p>
      <div class="actions-inline"><a href="https://github.com/qa-p1/Arcade-tools" target="_blank" rel="noreferrer">Repository</a><a href="https://github.com/qa-p1/Arcade-tools/blob/main/VENDORED" target="_blank" rel="noreferrer">Sources and licenses</a></div>
      <div class="actions-inline">
        {#if toolsApp?.installed}
          <button type="button" disabled={busy} onclick={() => run(updateTools)}>Update Arcade Tools</button>
        {:else}
          <button type="button" class="primary-action" disabled={busy} onclick={() => run(installTools)}>Install Arcade Tools for this user</button>
        {/if}
        <button type="button" onclick={onOnboarding}>Run setup again</button>
      </div>
      {#if toolsApp?.reason}<p class="setting-description">{toolsApp.reason}</p>{/if}
    </div>
  </section>

  <section class="settings-card danger-card" aria-labelledby="uninstall-heading" bind:this={uninstallSection}>
    <div>
      <h2 id="uninstall-heading">Uninstall</h2>
      <p class="setting-description">Remove only Tools and keep your apps, or remove every Arcade app Tools manages and then Tools. You see the full list before anything is removed.</p>
    </div>
    <div class="settings-stack">
      <label><input type="checkbox" bind:checked={removeData} /> Also delete settings and data</label>
      <div class="actions-inline">
        <button type="button" class="danger-button" disabled={busy || !toolsApp?.installed} onclick={() => run(uninstallToolsOnly)}>Uninstall Tools only…</button>
        <button type="button" class="danger-button" disabled={busy} onclick={() => run(uninstallEverything)}>Uninstall Tools and apps…</button>
      </div>
      {#if uninstallResults.length}
        <ul class="settings-list">{#each uninstallResults as result (result.app)}<li><strong>{result.app}</strong> {result.removed ? 'removed' : 'kept'} · {result.reason}</li>{/each}</ul>
      {/if}
    </div>
  </section>
</div>
