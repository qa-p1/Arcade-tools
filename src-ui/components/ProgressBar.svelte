<script lang="ts">
  type Props = {
    visible: boolean;
    phase: string;
    progress: number | null;
    cancellable: boolean;
    onCancel: () => void;
  };
  let { visible, phase, progress, cancellable, onCancel }: Props = $props();
</script>

<div id="operation" class="operation-bar" hidden={!visible} aria-live="polite">
  <span id="operation-text" class="operation-text">{phase || 'Working…'}</span>
  <progress id="progress" class="progress" max="1" value={progress ?? undefined} aria-label="Operation progress"></progress>
  <button id="cancel" class="btn sm" type="button" disabled={!cancellable} onclick={onCancel}>Cancel</button>
</div>
