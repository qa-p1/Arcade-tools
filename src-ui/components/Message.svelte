<script lang="ts">
  import Icon from './Icon.svelte';
  type Props = { text: string; error: boolean; raised?: boolean; serial?: number; duration?: number };
  let { text, error, raised = false, serial = 0, duration = 5000 }: Props = $props();
  // A notice fades after 5 s ("Copied" after 1.5 s, STANDARD §5); an error
  // stays until dismissed.
  let dismissed = $state('');
  const key = $derived(`${serial}:${error}:${text}`);
  $effect(() => {
    const current = key;
    if (!text || error) return;
    const timer = setTimeout(() => { dismissed = current; }, duration);
    return () => clearTimeout(timer);
  });
</script>

<div id="message" class="toast" class:error class:success={!error && text.startsWith('Copied')} class:raised role="status" aria-live="polite" hidden={!text || dismissed === key}>
  <Icon name={error ? 'alert' : 'info'} />
  <span class="message-text">{text}</span>
  <button class="btn ghost icon-only sm" type="button" aria-label="Dismiss" onclick={() => dismissed = key}><Icon name="x" size={14} /></button>
</div>
