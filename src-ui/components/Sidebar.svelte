<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';
  import { tabs, type ShellTab } from '../lib/tabs.svelte';
  type Props = { active: ShellTab; counts?: Partial<Record<ShellTab, number>>; onSelect: (id: ShellTab) => void; children?: Snippet };

  let { active, counts = {}, onSelect, children }: Props = $props();
  const visibleTabs = $derived(tabs.filter((tab) => tab.enabled));
  const icons: Record<ShellTab, IconName> = {
    home: 'home', apps: 'apps', updates: 'updates', shortcuts: 'keyboard', learn: 'book',
    connections: 'link', engines: 'engine', settings: 'settings', onboarding: 'sparkle',
  };

  function moveTab(event: KeyboardEvent, current: ShellTab): void {
    const direction = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1
      : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
    if (direction === 0) return;
    event.preventDefault();
    const index = visibleTabs.findIndex((tab) => tab.id === current);
    const next = visibleTabs[(index + direction + visibleTabs.length) % visibleTabs.length];
    onSelect(next.id);
    document.getElementById(`tab-${next.id}`)?.focus();
  }
</script>

<aside class="sidebar" aria-label="Arcade Tools">
  <a class="brand" href="#home" aria-label="Arcade Tools home" onclick={(event) => { event.preventDefault(); onSelect('home'); }}>
    <span class="brand-mark" aria-hidden="true"><img src="/glyphs/arcade.tools.svg" alt="" width="16" height="16" /></span>
    <span>Arcade Tools</span>
  </a>
  <div class="tab-list" role="tablist" aria-label="Arcade Tools sections" aria-orientation="vertical">
    {#each visibleTabs as tab (tab.id)}
      <button
        id="tab-{tab.id}"
        class:active={active === tab.id}
        class="tab-button"
        type="button"
        role="tab"
        aria-selected={active === tab.id}
        aria-controls="panel-{tab.id}"
        tabindex={active === tab.id ? 0 : -1}
        title={tab.label}
        onclick={() => onSelect(tab.id)}
        onkeydown={(event) => moveTab(event, tab.id)}
      ><Icon name={icons[tab.id]} /><span class="label">{tab.label}</span>{#if counts[tab.id]}<span class="count" aria-label="{counts[tab.id]} available">{counts[tab.id]}</span>{/if}</button>
    {/each}
  </div>
  <div class="sidebar-foot">{@render children?.()}</div>
</aside>
