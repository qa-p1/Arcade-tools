<script lang="ts">
  import Icon from './Icon.svelte';
  type Props = { text: string; error: boolean; raised?: boolean; serial?: number; duration?: number };
  let { text, error, raised = false, serial = 0, duration = 5000 }: Props = $props();
  // A notice fades after 5 s ("Copied" after 1.5 s, STANDARD §5). An error
  // fades after 12 s: until 1.0.2 it stayed until dismissed and covered the
  // page (D41); a failed operation stays in Recent activity. Either waits
  // while the pointer or focus is on it, so it can be read and copied.
  const ERROR_DURATION = 12000;
  let dismissed = $state('');
  let hovered = $state(false);
  let focused = $state(false);
  const key = $derived(`${serial}:${error}:${text}`);
  const held = $derived(hovered || focused);
  $effect(() => {
    const current = key;
    if (!text || held) return;
    const timer = setTimeout(() => { dismissed = current; }, error ? Math.max(duration, ERROR_DURATION) : duration);
    return () => clearTimeout(timer);
  });
</script>

<div id="message" class="toast" class:error class:success={!error && text.startsWith('Copied')} class:raised role="status" aria-live="polite" hidden={!text || dismissed === key}
  onpointerenter={() => hovered = true} onpointerleave={() => hovered = false}
  onfocusin={() => focused = true} onfocusout={() => focused = false}>
  <Icon name={error ? 'alert' : 'info'} />
  <span class="message-text">{text}</span>
  <button class="btn ghost icon-only sm" type="button" aria-label="Dismiss" onclick={() => dismissed = key}><Icon name="x" size={14} /></button>
</div>
