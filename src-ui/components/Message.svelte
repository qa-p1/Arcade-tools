<script lang="ts">
  type Props = { text: string; error: boolean };
  let { text, error }: Props = $props();
</script>

<div id="message" class:error role="status" aria-live="polite">{text}</div>
