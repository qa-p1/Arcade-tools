<script lang="ts">
  import { tabs, type ShellTab } from '../lib/tabs';
  type Props = { active: ShellTab; onSelect: (id: ShellTab) => void };

  let { active, onSelect }: Props = $props();
  const visibleTabs = tabs.filter((tab) => tab.enabled);

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
  <a class="brand" href="#apps" aria-label="Arcade Tools home" onclick={(event) => { event.preventDefault(); onSelect('apps'); }}>
    <span class="brand-mark" aria-hidden="true">A</span>
    <span><strong>Arcade</strong><small>TOOLS</small></span>
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
        onclick={() => onSelect(tab.id)}
        onkeydown={(event) => moveTab(event, tab.id)}
      >{tab.label}</button>
    {/each}
  </div>
  <p class="sidebar-caption">Your Arcade apps</p>
</aside>
