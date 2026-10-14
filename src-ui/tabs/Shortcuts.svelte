<script lang="ts">
  import { api } from '../lib/api';
  import ShortcutRebind from '../components/ShortcutRebind.svelte';
  import TraySettings from '../components/TraySettings.svelte';
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

<div id="panel-shortcuts" class="tab-panel shortcuts-panel" role="tabpanel" aria-labelledby="tab-shortcuts" tabindex="0" hidden={!active}>
  <header class="page-heading shortcuts-heading">
    <div>
      <p class="eyebrow">ARCADE TOOLS / SHORTCUTS</p>
      <h1>Find a shortcut.</h1>
      <p class="intro">Search Arcade, window-manager and app shortcuts in one place.</p>
    </div>
    {#if index}<span class="app-count">Updated {new Date(index.builtAt).toLocaleString()}</span>{/if}
  </header>

  {#if !index}
    <section class="shortcut-empty" aria-labelledby="shortcuts-empty-title">
      <span class="shortcut-empty-mark" aria-hidden="true">⌘</span>
      <h2 id="shortcuts-empty-title">Your shortcut index is not ready yet</h2>
      <p>Open the shortcut cheat sheet once (Super+/) or restart Arcade Tools to build the shortcut index.</p>
    </section>
  {:else}
    <label class="shortcut-search">
      <span class="visually-hidden">Search all shortcuts</span>
      <svg aria-hidden="true" viewBox="0 0 20 20" fill="none"><circle cx="8.8" cy="8.8" r="5.7" /><path d="m13 13 4 4" /></svg>
      <input type="search" value={query} oninput={changeSearch} placeholder="Search all shortcuts, apps and keys" aria-label="Search all shortcuts" />
      <kbd class="search-hint">/</kbd>
    </label>

    <div class="shortcut-view-tabs" role="tablist" aria-label="Shortcut views">
      {#each views as item (item.id)}
        <button id="shortcut-view-{item.id}" type="button" role="tab" aria-selected={view === item.id}
          aria-controls="shortcut-view-panel" tabindex={view === item.id ? 0 : -1}
          class:active={view === item.id} onclick={() => chooseView(item.id)} onkeydown={(event) => moveView(event, item.id)}>
          {item.label}
          {#if item.id === 'conflicts' && index.conflicts.length}<span class="view-count">{index.conflicts.length}</span>{/if}
        </button>
      {/each}
    </div>

    {#if search}
      <div id="shortcut-view-panel" class="shortcut-results" role="tabpanel" aria-labelledby="shortcut-view-{view}">
        {#if !hasGlobalMatches}
          <p class="shortcut-no-results" role="status">No shortcuts match “{query}”.</p>
        {:else}
          {#if globalArcade.length}
            <section class="shortcut-section" aria-labelledby="search-arcade-heading">
              <div class="section-heading"><h2 id="search-arcade-heading">Arcade</h2><span>{globalArcade.length}</span></div>
              <ul class="shortcut-list">
                {#each globalArcade as shortcut (`${shortcut.app}-${shortcut.id}`)}
                  <li class="shortcut-row">
                    <div class="shortcut-copy"><h3>{shortcut.title}</h3><p>{shortcut.name}</p></div>
                    <div class="shortcut-key-area"><KeyChips keys={shortcut.display} os={index.os} /><ShortcutRebind {shortcut} {index} /></div>
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
          {#if globalWm.length}
            <section class="shortcut-section" aria-labelledby="search-wm-heading">
              <div class="section-heading"><h2 id="search-wm-heading">Window manager</h2><span>{globalWm.length}</span></div>
              <ul class="shortcut-list">
                {#each globalWm as match, itemIndex (`${match.shortcut.submap}-${match.shortcut.chord}-${itemIndex}`)}
                  <li class="shortcut-row">
                    <div class="shortcut-copy"><h3>{labels[labelKey(match.shortcut)] ?? match.shortcut.label}</h3><p>{match.group.title} · {match.shortcut.origin.file}:{match.shortcut.origin.line}</p></div>
                    <KeyChips keys={[match.shortcut.display]} os={index.os} />
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
          {#if globalApps.length}
            <section class="shortcut-section" aria-labelledby="search-apps-heading">
              <div class="section-heading"><h2 id="search-apps-heading">Apps</h2><span>{globalApps.length}</span></div>
              <ul class="shortcut-list">
                {#each globalApps as match, itemIndex (`${match.app.id}-${match.shortcut.id}-${itemIndex}`)}
                  <li class="shortcut-row">
                    <div class="shortcut-copy"><h3>{match.shortcut.title}</h3><p>{match.app.name} · {match.group.title}</p></div>
                    <KeyChips keys={match.shortcut.display} os={index.os} />
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
          {#if globalConflicts.length}
            <section class="shortcut-section" aria-labelledby="search-conflicts-heading">
              <div class="section-heading"><h2 id="search-conflicts-heading">Conflicts</h2><span>{globalConflicts.length}</span></div>
              {#each globalConflicts as conflict, itemIndex (`${conflict.chord}-${itemIndex}`)}
                <div class="conflict-card"><KeyChips keys={[conflict.chord]} os={index.os} />
                  <p><strong>{conflict.a.label}</strong> ({ownerName(conflict.a)}) and <strong>{conflict.b.label}</strong> ({ownerName(conflict.b)}) use the same chord. Review the two bindings before changing either one.</p>
                </div>
              {/each}
            </section>
          {/if}
        {/if}
      </div>
    {:else}
      <div id="shortcut-view-panel" class="shortcut-view-panel" role="tabpanel" aria-labelledby="shortcut-view-{view}">
        {#if view === 'this-app'}
          <section class="shortcut-section" aria-labelledby="current-app-heading">
            <h2 id="current-app-heading">{currentApp?.name ?? 'Choose the app you are using'}</h2>
            <label class="app-picker">App <select bind:value={currentAppId}><option value="">Choose an app</option>{#each index.apps as app (app.id)}<option value={app.id}>{app.name}</option>{/each}</select></label>
            {#if currentApp}{#each appGroups(currentApp) as group (group.title)}<section class="shortcut-group"><h3>{group.title}</h3><ul class="shortcut-list">{#each group.shortcuts as shortcut (shortcut.id)}<AppShortcutRow {shortcut} os={index.os} />{/each}</ul></section>{/each}{:else}<p class="setting-description">Automatic focus detection is available on supported desktops. Choose an app here, or open the overlay while that app is focused.</p>{/if}
          </section>
        {:else if view === 'arcade'}
          <section class="shortcut-section" aria-labelledby="arcade-shortcuts-heading">
            <div class="section-heading"><div><h2 id="arcade-shortcuts-heading">Arcade shortcuts</h2><p>Global shortcuts provided by Arcade apps.</p></div><span>{visibleArcade.length}</span></div>
            {#if visibleArcade.length}
              <ul class="shortcut-list">
                {#each visibleArcade as shortcut (`${shortcut.app}-${shortcut.id}`)}
                  <li class="shortcut-row">
                    <div class="shortcut-copy"><h3>{shortcut.title}</h3><p>{shortcut.name}</p></div>
                    <div class="shortcut-key-area"><KeyChips keys={shortcut.display} os={index.os} /><ShortcutRebind {shortcut} {index} /></div>
                  </li>
                {/each}
              </ul>
            {:else}<p class="shortcut-no-results" role="status">No Arcade global shortcuts were found.</p>{/if}
          </section>
        {:else if view === 'wm'}
          <section class="shortcut-section" aria-labelledby="wm-shortcuts-heading">
            <div class="section-heading"><div><h2 id="wm-shortcuts-heading">{index.wm.name}</h2><p>Window-manager bindings from configuration and the running session.</p></div><span>{visibleWmGroups.reduce((total, group) => total + group.shortcuts.length, 0)}</span></div>
            {#if index.wm.warnings.length}<ul class="shortcut-warnings" aria-label="Window manager warnings">{#each index.wm.warnings as warning}<li>{warning}</li>{/each}</ul>{/if}
            {#if visibleWmGroups.length}
              {#each visibleWmGroups as group (group.title)}
                <section class="shortcut-group" aria-labelledby={`wm-group-${group.title}`}>
                  <h3 id={`wm-group-${group.title}`}>{group.title}</h3>
                  <ul class="shortcut-list">
                    {#each group.shortcuts as shortcut, itemIndex (`${shortcut.submap}-${shortcut.chord}-${itemIndex}`)}
                      {@const key = labelKey(shortcut)}
                      {@const shownLabel = currentLabel(key, shortcut.label)}
                      <li class="wm-shortcut-row">
                        <div class="shortcut-copy">
                          <div class="shortcut-title-line"><h4>{shownLabel}</h4><span class="state-badge" class:state-active={shortcut.state === 'active'} class:state-configured={shortcut.state === 'configuredOnly'} class:state-runtime={shortcut.state === 'runtimeOnly'}>{stateLabel(shortcut.state)}</span></div>
                          <p>{shortcut.detail}</p>
                          <p class="shortcut-origin">{shortcut.origin.file}:{shortcut.origin.line}</p>
                        </div>
                        <KeyChips keys={[shortcut.display]} os={index.os} />
                        <div class="label-editor">
                          <label for={`shortcut-label-${itemIndex}-${group.title}`}>Label</label>
                          <input id={`shortcut-label-${itemIndex}-${group.title}`} value={shownLabel} oninput={(event) => changeLabel(key, event)} aria-label={`Label for ${shortcut.display}`} />
                          <button type="button" class="small-button" aria-label={`Save label for ${shownLabel}`} onclick={() => void saveLabel(key, shortcut.label)}>Save label</button>
                          {#if savedLabelKey === key}<span class="save-status" role="status">Saved</span>{/if}
                        </div>
                      </li>
                    {/each}
                  </ul>
                </section>
              {/each}
            {:else}<p class="shortcut-no-results" role="status">No window-manager shortcuts match your search.</p>{/if}
            {#if labelError}<p class="shortcut-error" role="alert">{labelError}</p>{/if}
          </section>
        {:else if view === 'apps'}
          <section class="shortcut-section" aria-labelledby="app-shortcuts-heading">
            <div class="section-heading"><div><h2 id="app-shortcuts-heading">App shortcuts</h2><p>Shortcut sheets grouped by app and context.</p></div><span>{index.apps.length}</span></div>
            <label class="app-picker">Choose an app
              <select aria-label="Choose an app" value={selectedAppId} onchange={changeApp}>
                {#each index.apps as app (app.id)}<option value={app.id}>{app.name} · {app.source === 'catalog' ? 'Catalog' : 'Arcade'}</option>{/each}
              </select>
            </label>
            {#if selectedApp}
              <div class="app-sheet-heading"><div><h3>{selectedApp.name}</h3><p>{selectedApp.source === 'catalog' ? 'Catalog shortcut sheet' : 'Arcade app shortcut sheet'}</p></div><span class="source-badge">{selectedApp.source}</span></div>
              {#if visibleAppGroups.length}
                {#each visibleAppGroups as group (group.title)}
                  <section class="shortcut-group" aria-labelledby={`app-group-${group.title}`}>
                    <h3 id={`app-group-${group.title}`}>{group.title}{#if group.context}<span>{group.context}</span>{/if}</h3>
                    <ul class="shortcut-list">
                      {#each group.shortcuts as shortcut (shortcut.id)}<AppShortcutRow {shortcut} os={index.os} />{/each}
                    </ul>
                  </section>
                {/each}
              {:else}<p class="shortcut-no-results" role="status">No shortcuts match your search for this app.</p>{/if}
            {:else}<p class="shortcut-no-results" role="status">No app shortcut sheets are available.</p>{/if}
          </section>
        {:else}
          <section class="shortcut-section" aria-labelledby="conflict-heading">
            <div class="section-heading"><div><h2 id="conflict-heading">Shortcut conflicts</h2><p>Bindings that use the same normalized chord.</p></div><span>{visibleConflicts.length}</span></div>
            {#if visibleConflicts.length}
              {#each visibleConflicts as conflict, itemIndex (`${conflict.chord}-${itemIndex}`)}
                <div class="conflict-card"><KeyChips keys={[conflict.chord]} os={index.os} />
                  <div class="conflict-owners"><p><strong>{conflict.a.label}</strong><span>{ownerName(conflict.a)} · {conflict.a.kind}</span></p><span class="conflict-vs" aria-hidden="true">↔</span><p><strong>{conflict.b.label}</strong><span>{ownerName(conflict.b)} · {conflict.b.kind}</span></p></div>
                  {#each index.arcade.filter((shortcut) => shortcut.rebindable && shortcut.keys.includes(conflict.chord)) as shortcut (`${shortcut.app}-${shortcut.id}`)}<ShortcutRebind {shortcut} {index} />{/each}
                  <p class="conflict-explanation">Both shortcuts claim {conflict.chord}. Review each owner’s settings to decide which binding should change.</p>
                </div>
              {/each}
            {:else}<p class="shortcut-no-results" role="status">No shortcut conflicts were found.</p>{/if}
          </section>
        {/if}
      </div>
    {/if}
  {/if}
<button type="button" onclick={() => void refreshSheets()}>Refresh catalog shortcut sheets</button>
  {#if sheetMessage}<p role="status">{sheetMessage}</p>{/if}
  <TraySettings {active} busy={false} />
</div>
