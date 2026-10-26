<script lang="ts">
  import Icon from './Icon.svelte';
  type Props = { text: string; error: boolean; raised?: boolean };
  let { text, error, raised = false }: Props = $props();
  // A notice fades after a few seconds; an error stays until dismissed.
  let dismissed = $state('');
  const key = $derived(`${error}:${text}`);
  $effect(() => {
    const current = key;
    if (!text || error) return;
    const timer = setTimeout(() => { dismissed = current; }, 6000);
    return () => clearTimeout(timer);
  });
</script>

<div id="message" class:error class:raised role="status" aria-live="polite" hidden={!text || dismissed === key}>
  <Icon name={error ? 'alert' : 'info'} />
  <span class="message-text">{text}</span>
  <button class="btn ghost icon-only sm" type="button" aria-label="Dismiss" onclick={() => dismissed = key}><Icon name="x" size={14} /></button>
</div>
