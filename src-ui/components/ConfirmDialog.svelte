<script lang="ts">
  import { tick } from 'svelte';
  import { finishConfirmation, ui } from '../lib/state.svelte';

  let dialogElement: HTMLDialogElement;

  $effect(() => {
    const active = ui.dialog;
    if (!dialogElement) return;
    void tick().then(() => {
      if (active && !dialogElement.open) dialogElement.showModal();
      else if (!active && dialogElement.open) dialogElement.close();
    });
  });

  function closed(): void {
    finishConfirmation(dialogElement.returnValue === 'confirm');
  }
</script>

<dialog id="confirm" bind:this={dialogElement} onclose={closed} aria-labelledby="confirm-title">
  <form method="dialog">
    <h2 id="confirm-title">{ui.dialog?.title ?? ''}</h2>
    <p id="confirm-copy">{ui.dialog?.copy ?? ''}</p>
    <div id="confirm-extra">      {#if ui.dialog?.kind === 'remove-data'}
        <label class="dialog-option"><input type="checkbox" aria-label="Remove settings and data" bind:checked={ui.dialog.removeData} /> Remove settings and data</label>
        <pre class="folders">{ui.dialog.folders.join('\n')}</pre>
      {:else if ui.dialog?.kind === 'background'}
        <label class="dialog-option"><input type="checkbox" aria-label="Open in background" bind:checked={ui.dialog.background} /> Open in background</label>
      {:else if ui.dialog?.kind === 'reopen-mode'}
        <label class="dialog-mode">Reopen the app
          <select class="select sm" aria-label="Reopen mode" bind:value={ui.dialog.mode}>
            <option value="foreground">In foreground</option>
            <option value="background">In background</option>
          </select>
        </label>
      {:else if ui.dialog?.kind === 'app-closed'}
        <label class="dialog-option"><input type="checkbox" aria-label="I have closed the app" bind:checked={ui.dialog.appClosed} /> I have closed the app</label>
      {/if}
    </div>
    <div class="dialog-actions">
      <button class="btn" type="submit" value="cancel">Cancel</button>
      <button id="confirm-action" class="btn" class:destructive={ui.dialog?.destructive} class:primary={!ui.dialog?.destructive} type="submit" value="confirm">{ui.dialog?.action ?? 'Continue'}</button>
    </div>
  </form>
</dialog>
