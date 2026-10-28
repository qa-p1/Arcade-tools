<script lang="ts">
  import { glyph } from '../lib/glyphs';
  import { api, type AppView, type BackupReport, type DiagnosticsReport, type RestorePlan, type RestoreResult, type UninstallPlan } from '../lib/api';
  import { askForConfirmation, ui, type Theme } from '../lib/state.svelte';
  import { checkAllUpdates, copyText, openLearn } from '../lib/controller';
  import type { ShellTab } from '../lib/tabs.svelte';
  import { tick } from 'svelte';
  import TraySettings from '../components/TraySettings.svelte';
  import Icon from '../components/Icon.svelte';

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
    onSelectTab: (tab: ShellTab) => void;
    onTheme: (theme: Theme) => void;
    onLink: (enabled: boolean) => Promise<void>;
    run: (action: () => Promise<void>) => void;
  };
  let {
    active, theme, linkEnabled, diagnostics, busy, toolsApp, apps, updateChecksEnabled, notificationsEnabled,
    onUpdateChecks, onOnboarding, onSelectTab, onTheme, onLink, run,
  }: Props = $props();
  const themes: { id: Theme; label: string }[] = [{ id: 'system', label: 'System' }, { id: 'light', label: 'Light' }, { id: 'dark', label: 'Dark' }];
  const channelName = $derived(toolsApp?.managed ? (toolsApp.channel === 'nightly' ? 'Nightly' : 'Stable') : 'Development build');

  async function copyDiagnostics(): Promise<void> {
    await copyText(await api.diagnosticsText());
  }

  let supportApp = $state('arcade.tools');
  let report = $state<DiagnosticsReport | null>(null);
  let backup = $state<BackupReport | null>(null);
  let restorePath = $state('');
  // The plan remembers the file it reviewed: the field may change meanwhile.
  let restorePlan = $state<(RestorePlan & { path: string }) | null>(null);
  let restoreGeneration = 0;
  let restored = $state<RestoreResult[]>([]);
  let removeData = $state(false);
  let uninstallResults = $state<{ app: string; removed: boolean; reason: string }[]>([]);
  const installedApps = $derived(apps.filter((app) => app.installed));
  const appName = (id: string) => id === 'arcade.tools' ? 'Arcade Tools' : apps.find((app) => app.id === id)?.name ?? id;
  // The shared preference, so a restore or another window's change shows here.
  async function setNotifications(enabled: boolean): Promise<void> {
    await api.setNotifications(enabled);
    ui.notificationsEnabled = enabled;
  }

  async function createDiagnostics(): Promise<void> {
    report = await api.createDiagnostics(supportApp);
  }

  async function exportBackup(): Promise<void> {
    backup = await api.exportBackup();
  }

  async function planRestore(): Promise<void> {
    restored = [];
    restorePlan = null;
    const path = restorePath.trim();
    const generation = ++restoreGeneration;
    const plan = await api.planRestore(path);
    if (generation === restoreGeneration) restorePlan = { ...plan, path };
  }

  async function restore(): Promise<void> {
    const plan = restorePlan;
    if (!plan) return;
    const included = plan.apps.filter((entry) => entry.backup.settings.length);
    const names = included.map((entry) => appName(entry.backup.id)).join(', ') || 'no apps';
    const missing = included.filter((entry) => !entry.installed).map((entry) => appName(entry.backup.id));
    const choice = await askForConfirmation({ title: 'Restore settings from this backup?', copy: `${plan.path}\nSettings for ${names} will be replaced with the backed-up copies.${missing.length ? `\n${missing.join(', ')} will be installed on the saved channel first.` : ''}\nApps may ask to restart.`, action: 'Restore', destructive: false, kind: null, folders: [] });
    if (!choice.confirmed) return;
    restored = await api.restoreBackup(plan.path, plan.sha256);
    restorePlan = null;
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
    // The dialog describes the plan that will run, whatever the checkbox
    // says by the time the plan arrives.
    const removable = plan.apps.filter((entry) => entry.canUninstall);
    const kept = plan.apps.filter((entry) => !entry.canUninstall);
    const lines = removable.map((entry) => `• ${appName(entry.id)}${entry.version ? ` ${entry.version}` : ''}`);
    if (kept.length) lines.push('', 'Kept: ' + kept.map((entry) => `${appName(entry.id)} (${entry.reason ?? 'not managed'})`).join(', '));
    const choice = await askForConfirmation({
      title: `Uninstall ${removable.length} Arcade ${removable.length === 1 ? 'app' : 'apps'}?`,
      copy: lines.join('\n') + (plan.removeData ? '\nSettings and data are deleted too.' : '\nSettings and data are kept.'),
      action: 'Uninstall', destructive: true, kind: null,
      folders: plan.removeData ? removable.flatMap((entry) => entry.dataFolders) : [],
    });
    if (!choice.confirmed) return;
    uninstallResults = (await api.uninstallAll(plan)).results;
  }
</script>

<div id="panel-settings" class="tab-panel settings-panel" role="tabpanel" aria-labelledby="tab-settings" tabindex="0" hidden={!active}>
  <header class="page-head">
    <div>
      <h1>Settings</h1>
      <p class="sub">How Arcade Tools looks and looks after your apps.</p>
    </div>
  </header>

  <section class="section" aria-labelledby="general-heading">
    <div class="section-head"><h2 id="general-heading">General</h2></div>
    <div class="list">
      <div class="row">
        <div class="row-main"><div class="row-title" id="appearance-heading">Appearance</div><div class="row-sub">Follow your system or pick a theme.</div></div>
        <div class="segmented" role="radiogroup" aria-labelledby="appearance-heading">
          {#each themes as option (option.id)}
            <button type="button" role="radio" aria-checked={theme === option.id} class:active={theme === option.id} onclick={() => onTheme(option.id)}>{option.label}</button>
          {/each}
        </div>
      </div>
      <TraySettings {active} {busy} parts={['login']} />
      <label class="row">
        <span class="row-main"><span class="row-title">Check for updates automatically</span><span class="row-sub">A few times a day. Nothing installs without asking unless you turn on automatic updates for an app.</span></span>
        <input class="switch" type="checkbox" aria-label="Check for updates automatically" checked={updateChecksEnabled} disabled={busy} onchange={(event) => run(() => onUpdateChecks(event.currentTarget.checked))} />
      </label>
      <label class="row">
        <span class="row-main"><span class="row-title">Show notifications</span><span class="row-sub">When an update is ready or something needs you.</span></span>
        <input class="switch" type="checkbox" aria-label="Show notifications" checked={notificationsEnabled} disabled={busy} onchange={(event) => run(() => setNotifications(event.currentTarget.checked))} />
      </label>
      <div class="row">
        <div class="row-main"><div class="row-title">Set up apps again</div><div class="row-sub">Choose apps, sort out shortcuts and the tray, step by step.</div></div>
        <button type="button" class="btn sm" onclick={onOnboarding}>Set up apps again</button>
      </div>
    </div>
  </section>

  <section class="section" aria-labelledby="shortcuts-settings-heading">
    <div class="section-head"><h2 id="shortcuts-settings-heading">Shortcuts</h2></div>
    <div class="list">
      <TraySettings {active} {busy} parts={['overlay']} />
      <div class="row">
        <div class="row-main"><div class="row-title">App shortcuts</div><div class="row-sub">Every Arcade app's shortcuts, with clashes, in one list.</div></div>
        <button type="button" class="btn sm" onclick={() => onSelectTab('shortcuts')}>Open Shortcuts</button>
      </div>
    </div>
  </section>

  <section class="section" aria-labelledby="tray-heading">
    <div class="section-head"><h2 id="tray-heading">Tray</h2></div>
    <div class="list"><TraySettings {active} {busy} parts={['tray']} /></div>
  </section>

  <section class="section" aria-labelledby="backup-heading">
    <div class="section-head"><h2 id="backup-heading">Backup and restore</h2></div>
    <div class="list">
      <div class="row">
        <div class="row-main"><div class="row-title">Back up settings</div><div class="row-sub">{backup ? `Saved to ${backup.file}` : 'Save the settings of every installed app in one file.'}</div></div>
        {#if backup}<button type="button" class="btn sm ghost" onclick={() => run(() => api.revealArchive(backup!.file))}>Show file</button>{/if}
        <button type="button" class="btn sm" disabled={busy} onclick={() => run(exportBackup)}>Back up</button>
      </div>
      {#if backup?.apps.some((app) => app.notIncluded)}
        <div class="row-extra flush"><ul class="plain-list">{#each backup.apps.filter((app) => app.notIncluded) as app (app.id)}<li><strong>{appName(app.id)}</strong> <span class="muted">· not included: {app.notIncluded}</span></li>{/each}</ul></div>
      {/if}
      <div class="row">
        <div class="row-main">
          <div class="row-title">Restore from a backup</div>
          <form class="restore-form" onsubmit={(event) => { event.preventDefault(); run(planRestore); }}>
            <label class="visually-hidden" for="restore-path">Backup file</label>
            <input id="restore-path" class="input" type="text" placeholder="Path to an Arcade backup (.zip)" bind:value={restorePath} />
            <button class="btn sm" type="submit" disabled={busy || !restorePath.trim()}>Review restore</button>
          </form>
        </div>
      </div>
      {#if restorePlan}
        <div class="row-extra flush">
          <ul class="plain-list">{#each restorePlan.apps as entry (entry.backup.id)}<li><strong>{appName(entry.backup.id)}</strong> <span class="muted">v{entry.backup.version}{!entry.backup.settings.length ? ` · no settings in this backup${entry.backup.notIncluded ? ` (${entry.backup.notIncluded})` : ''}` : entry.installed ? '' : ' · will be installed first'}</span></li>{/each}</ul>
          <button type="button" class="btn sm primary primary-action" disabled={busy} onclick={() => run(restore)}>Restore</button>
        </div>
      {/if}
      {#if restored.length}
        <div class="row-extra flush"><ul class="plain-list">{#each restored as result (result.app)}<li><strong>{appName(result.app)}</strong> <span class="muted">{result.imported ? 'restored' : 'not restored'}{result.restartRequired ? ' · restart required' : ''} · {result.message}</span></li>{/each}</ul></div>
      {/if}
    </div>
  </section>

  <section class="section" aria-labelledby="integration-heading">
    <div class="section-head"><h2 id="integration-heading">Connected apps</h2></div>
    <div class="list">
      <label class="row">
        <span class="row-main"><span class="row-title">Connect with other Arcade apps</span><span class="row-sub">Arcade apps can pass files and text to each other, and ask Tools to install another app after you confirm.</span></span>
        <input id="settings-link-toggle" class="switch" type="checkbox" aria-label="Connect with other Arcade apps" checked={linkEnabled} disabled={busy} onchange={(event) => run(() => onLink(event.currentTarget.checked))} />
      </label>
      <div class="row">
        <div class="row-main"><div class="row-title">Connected apps</div><div class="row-sub">Which apps are running and what they do together.</div></div>
        <button type="button" class="btn sm" onclick={() => onSelectTab('connections')}>Open Connections</button>
      </div>
    </div>
  </section>

  <section class="section" aria-labelledby="about-heading">
    <div class="section-head"><h2 id="about-heading">About</h2></div>
    <div class="list">
      <div class="row">
        <span class="glyph-tile"><img src={glyph('arcade.tools')} alt="" width="18" height="18" /></span>
        <div class="row-main">
          <div class="row-title about-version selectable">Version {toolsApp?.version ?? __TOOLS_VERSION__}</div>
          <div class="row-sub">{channelName} · The optional home for the Arcade apps.</div>
          {#if toolsApp?.reason}<div class="row-sub">{toolsApp.reason}</div>{/if}
        </div>
        {#if toolsApp?.installed}
          <button type="button" class="btn sm" disabled={busy} onclick={() => { onSelectTab('updates'); run(checkAllUpdates); }}>Check for updates</button>
        {:else}
          <button type="button" class="btn sm primary" disabled={busy} onclick={() => run(installTools)}>Install Arcade Tools</button>
        {/if}
      </div>
      <div class="row">
        <div class="row-main"><div class="row-title">User guide</div><div class="row-sub">How Tools installs, updates and connects your apps.</div></div>
        <button type="button" class="btn sm" onclick={() => run(() => openLearn('arcade.tools'))}>Open user guide</button>
      </div>
      <div class="row">
        <div class="row-main"><div class="row-title">Logs</div><div class="row-sub">Warnings and errors from Tools.</div></div>
        <button type="button" class="btn sm" onclick={() => run(api.openLogsFolder)}>Open logs folder</button>
      </div>
      <div class="row">
        <div class="row-main"><div class="row-title">Diagnostics</div><div class="row-sub">Version, system and connection details for a bug report. No personal files or secrets.</div></div>
        <button type="button" class="btn sm" onclick={() => run(copyDiagnostics)}>Copy diagnostics</button>
      </div>
      <div class="row" id="support-heading">
        <div class="row-main"><div class="row-title">Report a problem</div><div class="row-sub">{report ? `Saved to ${report.file}${report.omitted.length ? ` · left out: ${report.omitted.join(', ')}` : ''}` : 'Collects the logs and settings of one app in a file. Personal paths and secrets are removed; nothing is sent automatically.'}</div></div>
        <select class="select sm" aria-label="App to diagnose" bind:value={supportApp}>
          <option value="arcade.tools">Arcade Tools</option>
          {#each installedApps as app (app.id)}<option value={app.id}>{app.name}</option>{/each}
        </select>
        <button type="button" class="btn sm" disabled={busy} onclick={() => run(createDiagnostics)}>Create file</button>
      </div>
      {#if report}
        <div class="row-extra flush btn-group">
          <button type="button" class="btn sm" onclick={() => run(() => api.revealArchive(report!.file))}>Show in folder</button>
          <button type="button" class="btn sm primary" onclick={() => run(() => api.openIssue(supportApp, report!.issueUrl))}>Open an issue</button>
        </div>
      {/if}
      <div class="row">
        <div class="row-main"><div class="row-title">Licences</div><div class="row-sub">Arcade Tools is MIT licensed. It includes Inter (SIL Open Font License) and other open-source parts.</div></div>
        <a class="btn sm" href="https://github.com/qa-p1/Arcade-tools/blob/main/VENDORED" target="_blank" rel="noreferrer">Show licences</a>
      </div>
      <details class="tech" aria-labelledby="diagnostics-heading">
        <summary class="row"><span class="row-main row-title" id="diagnostics-heading">Technical details</span><Icon name="chevronDown" size={15} /></summary>
        <pre id="diagnostics" class="selectable">{diagnostics}</pre>
      </details>
    </div>
  </section>

  <section class="section" aria-labelledby="uninstall-heading" bind:this={uninstallSection}>
    <div class="section-head"><h2 id="uninstall-heading">Uninstall</h2></div>
    <div class="list">
      <div class="row">
        <div class="row-main"><div class="row-title">Remove Tools, or Tools and every app it manages</div><div class="row-sub">You see the full list before anything is removed.</div></div>
      </div>
      <div class="row-extra flush uninstall">
        <label class="check-row"><input type="checkbox" bind:checked={removeData} /> Also delete settings and data</label>
        <span class="spacer"></span>
        <button type="button" class="btn sm danger" disabled={busy || !toolsApp?.installed} onclick={() => run(uninstallToolsOnly)}>Uninstall Tools only…</button>
        <button type="button" class="btn sm danger" disabled={busy} onclick={() => run(uninstallEverything)}>Uninstall Tools and apps…</button>
      </div>
      {#if uninstallResults.length}
        <div class="row-extra flush"><ul class="plain-list">{#each uninstallResults as result (result.app)}<li><strong>{appName(result.app)}</strong> <span class="muted">{result.removed ? 'removed' : 'kept'} · {result.reason}</span></li>{/each}</ul></div>
      {/if}
    </div>
  </section>
</div>

<style>
  label.row .row-title, label.row .row-sub { display: block; }
  .row-extra.flush { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 0 14px 14px; }
  .uninstall { padding-top: 2px; }
  .spacer { flex: 1; }
  .plain-list { display: grid; flex: 1 1 100%; gap: 4px; margin: 0; padding: 0; list-style: none; font-size: var(--fs-control); }
  .restore-form { display: flex; gap: 8px; margin-top: 8px; }
  .restore-form .input { flex: 1; min-width: 0; }
  .tech { border-top: 1px solid var(--line); }
  .tech summary { min-height: 44px; cursor: pointer; list-style: none; color: var(--muted); }
  .tech summary::-webkit-details-marker { display: none; }
  .tech[open] summary :global(.icon) { transform: rotate(180deg); }
  #diagnostics { margin: 0; padding: 0 14px 14px; color: var(--muted); font-size: var(--fs-caption); line-height: 1.6; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
