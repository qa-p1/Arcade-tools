<script lang="ts">
  // TOOLS.md §Crash reports: the full report first; "Copy report" and "Open GitHub issue" only from here, so nothing
  // leaves the machine before the user has read what would. Escape (or Close) closes it.
  import { tick } from 'svelte';
  import { api, type CrashSummary } from '../lib/api';
  import { copyText } from '../lib/controller';

  type Props = { crash: CrashSummary | null; appName: string; onClose: () => void; run: (action: () => Promise<void>, label?: string) => void };
  let { crash, appName, onClose, run }: Props = $props();
  let dialogElement: HTMLDialogElement;
  let text = $state('');
  let error = $state('');

  $effect(() => {
    const current = crash;
    text = '';
    error = '';
    if (!current) {
      if (dialogElement?.open) dialogElement.close();
      return;
    }
    void api.crashReport(current.app, current.file).then((report) => { text = report; }, (e) => { error = e?.message ?? String(e); });
    void tick().then(() => { if (!dialogElement.open) dialogElement.showModal(); });
  });
</script>

<dialog id="crash-report" bind:this={dialogElement} onclose={onClose} aria-labelledby="crash-title">
  <form method="dialog">
    <h2 id="crash-title">{appName} crashed</h2>
    <p class="sub">This report stays on this computer. Read it before you copy it or open an issue with it.</p>
    {#if error}
      <p class="notice danger" role="alert">{error}</p>
    {:else}
      <!-- Read-only text: focusable and scrollable by keyboard, and selectable. -->
      <textarea class="crash-text" readonly rows="14" aria-label="Crash report" value={text || 'Loading…'}></textarea>
    {/if}
    <div class="dialog-actions">
      <button class="btn" type="submit" value="close">Close</button>
      <button class="btn" type="button" disabled={!text} onclick={() => run(() => copyText(text, 'Report copied'))}>Copy report</button>
      <button class="btn primary" type="button" disabled={!text || !crash} onclick={() => crash && run(() => api.openCrashIssue(crash.app, crash.file), appName)}>Open GitHub issue</button>
    </div>
  </form>
</dialog>

<style>
  .crash-text { box-sizing: border-box; width: 100%; max-height: min(50vh, 360px); overflow: auto; resize: none; border: 1px solid var(--line); color: inherit; margin: var(--sp-8) 0; padding: var(--sp-8); border-radius: var(--radius-sm); background: var(--sunken); font-family: var(--font-mono); font-size: var(--fs-mono); line-height: var(--lh-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  dialog { max-width: min(720px, 92vw); }
</style>
