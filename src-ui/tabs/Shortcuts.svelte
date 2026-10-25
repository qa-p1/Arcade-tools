<script lang="ts">
  import { api } from '../lib/api';
  import ShortcutRebind from '../components/ShortcutRebind.svelte';
  import Icon from '../components/Icon.svelte';
  import AppShortcutRow from '../components/AppShortcutRow.svelte';
  import KeyChips from '../components/KeyChips.svelte';
  import type {
    ArcadeShortcut,
    AppShortcut,
    ShortcutConflict,
    ShortcutGroup,
    ShortcutSheet,
    ShortcutsIndex,
    WindowManagerGroup,
    WindowManagerShortcut,
  } from '../lib/shortcuts';

  type View = 'this-app' | 'arcade' | 'wm' | 'apps' | 'conflicts';
  type Props = {
    active: boolean;
    index: ShortcutsIndex | null;
    labels: Record<string, string>;
    onLabel: (key: string, label: string | null) => Promise<void>;
    onChange: () => void;
  };
  let { active, index, labels, onLabel, onChange }: Props = $props();

  const views: { id: View; label: string }[] = [
    { id: 'this-app', label: 'This app' },
    { id: 'arcade', label: 'Arcade' },
    { id: 'wm', label: 'Window manager' },
    { id: 'apps', label: 'Apps' },
    { id: 'conflicts', label: 'Conflicts' },
  ];
  let view = $state<View>('arcade');
  let query = $state('');
  let selectedAppId = $state('');
  let currentAppId = $state('');
  let focusChecked = $state(false);
  let labelDrafts = $state<Record<string, string>>({});
  let savedLabelKey = $state('');
  let sheetMessage = $state('');
  async function refreshSheets(): Promise<void> { try { sheetMessage = `${await api.refreshShortcutSheets()} catalog sheets refreshed.`; } catch (error) { sheetMessage = (error as { message?: string }).message ?? String(error); } }
  let labelError = $state('');
  let editingLabel = $state('');
  const wmName = $derived(index?.wm.name && index.wm.name !== 'none' ? index.wm.name.charAt(0).toUpperCase() + index.wm.name.slice(1) : '');
  let sheetsBusy = $state(false);
  async function refreshAllSheets(): Promise<void> { sheetsBusy = true; try { await refreshSheets(); } finally { sheetsBusy = false; } }

  const search = $derived(query.trim().toLocaleLowerCase());
  const selectedApp = $derived(index?.apps.find((app) => app.id === selectedAppId) ?? null);
  const currentApp = $derived(index?.apps.find((app) => app.id === currentAppId) ?? null);
  $effect(() => {
    if (!active) { focusChecked = false; return; }
    if (!focusChecked) { focusChecked = true; void api.currentShortcutApp().then((id) => currentAppId = id ?? '').catch(() => {}); }
  });

  $effect(() => {
    const apps = index?.apps ?? [];
    if (apps.length > 0 && !apps.some((app) => app.id === selectedAppId)) {
      selectedAppId = apps[0].id;
    }
  });

  function matches(needle: string, ...fields: (string | null | undefined)[]): boolean {
    return needle.length === 0 || fields.some((field) => field?.toLocaleLowerCase().includes(needle));
  }

  function arcadeMatches(shortcut: ArcadeShortcut, needle = search): boolean {
    return matches(needle, shortcut.app, shortcut.name, shortcut.id, shortcut.title, ...shortcut.keys, ...shortcut.display);
  }

  function wmMatches(group: WindowManagerGroup, shortcut: WindowManagerShortcut, needle = search): boolean {
    return matches(needle, group.title, shortcut.chord, shortcut.display, shortcut.label, shortcut.detail,
      labels[`${shortcut.submap ?? ''}|${shortcut.chord}`], shortcut.submap, shortcut.origin.file,
      String(shortcut.origin.line));
  }

  function appMatches(app: ShortcutSheet, group: ShortcutGroup, shortcut: AppShortcut, needle = search): boolean {
    return matches(needle, app.name, app.id, app.source, group.title, group.context, shortcut.id,
      shortcut.title, shortcut.description, ...shortcut.keys, ...shortcut.display);
  }

  function appGroups(app: ShortcutSheet): ShortcutGroup[] {
    return app.groups
      .map((group) => ({
        ...group,
        shortcuts: group.shortcuts.filter((shortcut) => appMatches(app, group, shortcut)),
      }))
      .filter((group) => group.shortcuts.length > 0);
  }

  function wmGroups(): WindowManagerGroup[] {
    return (index?.wm.groups ?? [])
      .map((group) => ({ ...group, shortcuts: group.shortcuts.filter((shortcut) => wmMatches(group, shortcut)) }))
      .filter((group) => group.shortcuts.length > 0);
  }

  const visibleArcade = $derived((index?.arcade ?? []).filter((shortcut) => arcadeMatches(shortcut)));
  const visibleWmGroups = $derived(wmGroups());
  const wmCount = $derived(visibleWmGroups.reduce((total, group) => total + group.shortcuts.length, 0));
  const visibleAppGroups = $derived(selectedApp ? appGroups(selectedApp) : []);
  const visibleConflicts = $derived((index?.conflicts ?? []).filter((conflict) =>
    matches(search, conflict.chord, conflict.a.kind, conflict.a.owner, conflict.a.label,
      conflict.b.kind, conflict.b.owner, conflict.b.label)));
  const globalArcade = $derived((index?.arcade ?? []).filter((shortcut) => arcadeMatches(shortcut, search)));
  const globalWm = $derived((index?.wm.groups ?? []).flatMap((group) => group.shortcuts
    .filter((shortcut) => wmMatches(group, shortcut, search))
    .map((shortcut) => ({ group, shortcut }))));
  const globalApps = $derived((index?.apps ?? []).flatMap((app) => app.groups.flatMap((group) =>
    group.shortcuts.filter((shortcut) => appMatches(app, group, shortcut, search))
      .map((shortcut) => ({ app, group, shortcut })))));
  const globalConflicts = $derived((index?.conflicts ?? []).filter((conflict) =>
    matches(search, conflict.chord, conflict.a.kind, conflict.a.owner, conflict.a.label,
      conflict.b.kind, conflict.b.owner, conflict.b.label)));
  const hasGlobalMatches = $derived(globalArcade.length + globalWm.length + globalApps.length + globalConflicts.length > 0);

  function chooseView(next: View): void {
    view = next;
    onChange();
  }

  function moveView(event: KeyboardEvent, current: View): void {
    const direction = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1
      : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
    if (direction === 0) return;
    event.preventDefault();
    const index = views.findIndex((item) => item.id === current);
    const next = views[(index + direction + views.length) % views.length];
    chooseView(next.id);
    document.getElementById(`shortcut-view-${next.id}`)?.focus();
  }

  function changeSearch(event: Event): void {
    query = (event.currentTarget as HTMLInputElement).value;
    onChange();
  }

  function changeApp(event: Event): void {
    selectedAppId = (event.currentTarget as HTMLSelectElement).value;
    onChange();
  }

  function labelKey(shortcut: WindowManagerShortcut): string {
    return `${shortcut.submap ?? ''}|${shortcut.chord}`;
  }

  function currentLabel(key: string, fallback: string): string {
    return labelDrafts[key] ?? labels[key] ?? fallback;
  }

  function changeLabel(key: string, event: Event): void {
    labelDrafts[key] = (event.currentTarget as HTMLInputElement).value;
    savedLabelKey = '';
    labelError = '';
    onChange();
  }

  async function saveLabel(key: string, fallback: string): Promise<void> {
    labelError = '';
    try {
      const value = labelDrafts[key] ?? labels[key] ?? fallback;
      await onLabel(key, value.trim() || null);
      const next = { ...labelDrafts };
      delete next[key];
      labelDrafts = next;
      savedLabelKey = key;
      editingLabel = '';
      onChange();
    } catch (error) {
      labelError = error instanceof Error ? error.message : String(error);
      onChange();
    }
  }

  function stateLabel(state: WindowManagerShortcut['state']): string {
    if (state === 'configuredOnly') return 'Configured, not active';
    if (state === 'runtimeOnly') return 'Added at runtime';
    return 'Active';
  }

  function ownerName(owner: ShortcutConflict['a']): string {
    return owner.owner;
  }
</script>

{#snippet arcadeRow(shortcut: ArcadeShortcut, os: string)}
  <li class="row shortcut-row arcade-row">
    <div class="row-main"><div class="row-title">{shortcut.title}</div><div class="row-sub">{shortcut.name}</div></div>
    <KeyChips keys={shortcut.display} {os} />
    {#if index}<div class="rebind"><ShortcutRebind {shortcut} {index} /></div>{/if}
  </li>
{/snippet}

<div id="panel-shortcuts" class="tab-panel shortcuts-panel" role="tabpanel" aria-labelledby="tab-shortcuts" tabindex="0" hidden={!active}>
  <header class="page-head">
    <div>
      <h1>Shortcuts</h1>
      <p class="sub">Every shortcut on this computer in one place.{#if index}&nbsp;Updated {new Date(index.builtAt).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })}.{/if}</p>
    </div>
    <div class="page-actions">
      <button type="button" class="btn ghost" disabled={sheetsBusy} onclick={() => void refreshAllSheets()}><Icon name="refresh" size={14} />{sheetsBusy ? 'Refreshing…' : 'Refresh catalog shortcut sheets'}</button>
    </div>
  </header>
  {#if sheetMessage}<p class="section-note sheet-message" role="status">{sheetMessage}</p>{/if}

  {#if !index}
    <div class="empty-state">
      <Icon name="keyboard" size={24} />
      <h2 id="shortcuts-empty-title">The shortcut list isn't ready yet</h2>
      <p>Open the shortcut overlay once (Super + /) or restart Arcade Tools to build it.</p>
    </div>
  {:else}
    <div class="toolbar">
      <label class="search-field">
        <Icon name="search" />
        <span class="visually-hidden">Search all shortcuts</span>
        <input type="search" value={query} oninput={changeSearch} placeholder="Search shortcuts, apps or keys" aria-label="Search all shortcuts" />
      </label>
      {#if !search}
        <div class="segmented" role="tablist" aria-label="Shortcut views">
          {#each views as item (item.id)}
            <button id="shortcut-view-{item.id}" type="button" role="tab" aria-selected={view === item.id}
              aria-controls="shortcut-view-panel" tabindex={view === item.id ? 0 : -1}
              class:active={view === item.id} onclick={() => chooseView(item.id)} onkeydown={(event) => moveView(event, item.id)}>
              {item.id === 'wm' && wmName ? wmName : item.label}
              {#if item.id === 'conflicts' && index.conflicts.length}<span class="view-count">{index.conflicts.length}</span>{/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    {#if search}
      <div id="shortcut-view-panel" class="shortcut-results" role="tabpanel" aria-label="Search results">
        {#if !hasGlobalMatches}
          <p class="no-results" role="status">No shortcuts match “{query}”.</p>
        {:else}
          {#if globalArcade.length}
            <section class="group" aria-labelledby="search-arcade-heading">
              <h3 class="group-title" id="search-arcade-heading">Arcade <span>{globalArcade.length}</span></h3>
              <ul class="list plain">{#each globalArcade as shortcut (`${shortcut.app}-${shortcut.id}`)}{@render arcadeRow(shortcut, index.os)}{/each}</ul>
            </section>
          {/if}
          {#if globalWm.length}
            <section class="group" aria-labelledby="search-wm-heading">
              <h3 class="group-title" id="search-wm-heading">{wmName || 'Window manager'} <span>{globalWm.length}</span></h3>
              <ul class="list plain">
                {#each globalWm as match, itemIndex (`${match.shortcut.submap}-${match.shortcut.chord}-${itemIndex}`)}
                  <li class="row shortcut-row"><div class="row-main"><div class="row-title">{labels[labelKey(match.shortcut)] ?? match.shortcut.label}</div><div class="row-sub">{match.group.title}</div></div><KeyChips keys={[match.shortcut.display]} os={index.os} /></li>
                {/each}
              </ul>
            </section>
          {/if}
          {#if globalApps.length}
            <section class="group" aria-labelledby="search-apps-heading">
              <h3 class="group-title" id="search-apps-heading">Apps <span>{globalApps.length}</span></h3>
              <ul class="list plain">
                {#each globalApps as match, itemIndex (`${match.app.id}-${match.shortcut.id}-${itemIndex}`)}
                  <li class="row shortcut-row"><div class="row-main"><div class="row-title">{match.shortcut.title}</div><div class="row-sub">{match.app.name} · {match.group.title}</div></div><KeyChips keys={match.shortcut.display} os={index.os} /></li>
                {/each}
              </ul>
            </section>
          {/if}
          {#if globalConflicts.length}
            <section class="group" aria-labelledby="search-conflicts-heading">
              <h3 class="group-title" id="search-conflicts-heading">Conflicts <span>{globalConflicts.length}</span></h3>
              <ul class="list plain">
                {#each globalConflicts as conflict, itemIndex (`${conflict.chord}-${itemIndex}`)}
                  <li class="row conflict-card"><span class="dot warn"></span><div class="row-main"><div class="row-title">{conflict.a.label} <span class="faint">and</span> {conflict.b.label}</div><div class="row-sub">{ownerName(conflict.a)} · {ownerName(conflict.b)}</div></div><KeyChips keys={[conflict.chord]} os={index.os} /></li>
                {/each}
              </ul>
            </section>
          {/if}
        {/if}
      </div>
    {:else}
      <div id="shortcut-view-panel" class="shortcut-view-panel" role="tabpanel" aria-labelledby="shortcut-view-{view}">
        {#if view === 'this-app'}
          <div class="view-head">
            <p class="muted">{currentApp ? `Shortcuts for ${currentApp.name}.` : 'Pick the app you are using. The overlay detects it automatically on supported desktops.'}</p>
            <select class="select" aria-label="App" bind:value={currentAppId}><option value="">Choose an app</option>{#each index.apps as app (app.id)}<option value={app.id}>{app.name}</option>{/each}</select>
          </div>
          {#if currentApp}
            {#each appGroups(currentApp) as group (group.title)}
              <section class="group"><h3 class="group-title">{group.title}</h3><ul class="list plain">{#each group.shortcuts as shortcut (shortcut.id)}<AppShortcutRow {shortcut} os={index.os} />{/each}</ul></section>
            {/each}
          {/if}
        {:else if view === 'arcade'}
          <p class="view-note muted" id="arcade-shortcuts-heading">Shortcuts Arcade apps add to your whole desktop.</p>
          {#if visibleArcade.length}
            <ul class="list plain">{#each visibleArcade as shortcut (`${shortcut.app}-${shortcut.id}`)}{@render arcadeRow(shortcut, index.os)}{/each}</ul>
          {:else}<p class="no-results" role="status">No Arcade global shortcuts were found.</p>{/if}
        {:else if view === 'wm'}
          <p class="view-note muted" id="wm-shortcuts-heading">{wmCount} bindings from your {wmName || 'window manager'} configuration. Rename any of them to make the list easier to read.</p>
          {#if index.wm.warnings.length}<div class="notice warn wm-warnings"><Icon name="alert" /><ul class="row-main" aria-label="Window manager warnings">{#each index.wm.warnings as warning}<li>{warning}</li>{/each}</ul></div>{/if}
          {#if visibleWmGroups.length}
            {#each visibleWmGroups as group (group.title)}
              <section class="group" aria-labelledby={`wm-group-${group.title}`}>
                <h3 class="group-title" id={`wm-group-${group.title}`}>{group.title} <span>{group.shortcuts.length}</span></h3>
                <ul class="list plain">
                  {#each group.shortcuts as shortcut, itemIndex (`${shortcut.submap}-${shortcut.chord}-${itemIndex}`)}
                    {@const key = labelKey(shortcut)}
                    {@const shownLabel = currentLabel(key, shortcut.label)}
                    <li class="row wm-shortcut-row">
                      <div class="row-main">
                        {#if editingLabel === key}
                          <form class="label-editor" onsubmit={(event) => { event.preventDefault(); void saveLabel(key, shortcut.label); }}>
                            <input id={`shortcut-label-${itemIndex}-${group.title}`} class="input" value={shownLabel} oninput={(event) => changeLabel(key, event)} aria-label={`Label for ${shortcut.display}`} />
                            <button type="submit" class="btn sm primary" aria-label={`Save label for ${shownLabel}`}>Save</button>
                            <button type="button" class="btn sm ghost" onclick={() => editingLabel = ''}>Cancel</button>
                          </form>
                        {:else}
                          <div class="row-title">{shownLabel}{#if shortcut.state !== 'active'}<span class="badge" class:warn={shortcut.state === 'configuredOnly'}>{stateLabel(shortcut.state)}</span>{/if}{#if savedLabelKey === key}<span class="faint save-status" role="status">Saved</span>{/if}</div>
                          {#if shortcut.detail && shortcut.detail !== shownLabel}<div class="row-sub" title="{shortcut.origin.file}:{shortcut.origin.line}">{shortcut.detail}</div>{/if}
                        {/if}
                      </div>
                      <KeyChips keys={[shortcut.display]} os={index.os} />
                      {#if editingLabel !== key}<button type="button" class="btn ghost icon-only sm edit" title="Rename" aria-label={`Rename ${shownLabel}`} onclick={() => { editingLabel = key; savedLabelKey = ''; }}><Icon name="edit" size={14} /></button>{/if}
                    </li>
                  {/each}
                </ul>
              </section>
            {/each}
          {:else}<p class="no-results" role="status">No window-manager shortcuts were found.</p>{/if}
          {#if labelError}<p class="error-text" role="alert">{labelError}</p>{/if}
        {:else if view === 'apps'}
          <div class="view-head">
            <p class="muted" id="app-shortcuts-heading">Shortcut sheets for {index.apps.length} apps.</p>
            <select class="select" aria-label="Choose an app" value={selectedAppId} onchange={changeApp}>
              {#each index.apps as app (app.id)}<option value={app.id}>{app.name}</option>{/each}
            </select>
          </div>
          {#if selectedApp}
            {#if visibleAppGroups.length}
              {#each visibleAppGroups as group (group.title)}
                <section class="group" aria-labelledby={`app-group-${group.title}`}>
                  <h3 class="group-title" id={`app-group-${group.title}`}>{group.title}{#if group.context}<span>{group.context}</span>{/if}</h3>
                  <ul class="list plain">{#each group.shortcuts as shortcut (shortcut.id)}<AppShortcutRow {shortcut} os={index.os} />{/each}</ul>
                </section>
              {/each}
            {:else}<p class="no-results" role="status">This app has no shortcuts listed.</p>{/if}
          {:else}<p class="no-results" role="status">No app shortcut sheets are available.</p>{/if}
        {:else}
          <p class="view-note muted" id="conflict-heading">Two bindings on the same keys: only one of them works.</p>
          {#if visibleConflicts.length}
            <ul class="list plain">
              {#each visibleConflicts as conflict, itemIndex (`${conflict.chord}-${itemIndex}`)}
                {@const fixable = index.arcade.filter((shortcut) => shortcut.rebindable && shortcut.keys.includes(conflict.chord))}
                <li class="row conflict-card">
                  <span class="dot warn"></span>
                  <div class="row-main">
                    <div class="row-title">{conflict.a.label} <span class="faint">and</span> {conflict.b.label}</div>
                    <div class="row-sub">{ownerName(conflict.a)} · {ownerName(conflict.b)}</div>
                  </div>
                  <KeyChips keys={[conflict.chord]} os={index.os} />
                  {#if fixable.length}<div class="rebind">{#each fixable as shortcut (`${shortcut.app}-${shortcut.id}`)}<ShortcutRebind {shortcut} {index} />{/each}</div>{/if}
                </li>
              {/each}
            </ul>
          {:else}<div class="empty-state"><Icon name="check" size={24} /><h2>No conflicts</h2><p>Every shortcut has its own keys.</p></div>{/if}
        {/if}
      </div>
    {/if}
    <p class="section-note overlay-note">Press <KeyChips keys={[index.os === 'macos' ? 'Ctrl+Option+/' : index.os === 'windows' ? 'Ctrl+Alt+/' : 'Super+/']} os={index.os} /> in any app to see its shortcuts. Change the overlay shortcut in Settings.</p>
  {/if}
</div>

<style>
  .sheet-message { margin: -12px 0 16px; }
  .toolbar { display: grid; gap: 12px; margin-bottom: 20px; }
  .toolbar .segmented { justify-self: start; }
  .view-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .view-note { margin-bottom: 14px; }
  .group + .group { margin-top: 22px; }
  .group-title { display: flex; align-items: baseline; gap: 8px; margin: 0 0 8px 2px; color: var(--faint); font-size: 11px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; }
  .group-title span { font-weight: 500; letter-spacing: 0; text-transform: none; }
  .plain { margin: 0; padding: 0; list-style: none; }
  .plain > :global(.row) { min-height: 46px; padding: 8px 14px; }
  .arcade-row, .conflict-card { flex-wrap: wrap; }
  .rebind { display: flex; justify-content: flex-end; }
  .rebind:has(:global(.rebind-editing)) { flex-basis: 100%; justify-content: flex-start; }
  .row-title :global(.badge) { margin-left: 4px; }
  .save-status { font-size: 12px; font-weight: 400; }
  .edit { opacity: 0; }
  .wm-shortcut-row:hover .edit, .edit:focus-visible { opacity: 1; }
  .label-editor { display: flex; gap: 6px; }
  .label-editor .input { flex: 1; min-width: 0; height: 28px; }
  .wm-warnings ul { margin: 0; padding-left: 16px; }
  .wm-warnings { margin-bottom: 14px; }
  .no-results { padding: 20px; border: 1px dashed var(--line-strong); border-radius: var(--radius); color: var(--muted); text-align: center; }
  .overlay-note { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 28px; }
</style>
