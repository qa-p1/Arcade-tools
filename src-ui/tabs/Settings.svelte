<script lang="ts">
  import type { Theme } from '../lib/state.svelte';

  type Props = {
    active: boolean;
    theme: Theme;
    linkEnabled: boolean;
    diagnostics: string;
    busy: boolean;
    onTheme: (theme: Theme) => void;
    onLink: (enabled: boolean) => Promise<void>;
    run: (action: () => Promise<void>) => void;
  };
  let { active, theme, linkEnabled, diagnostics, busy, onTheme, onLink, run }: Props = $props();
</script>

<div id="panel-settings" class="tab-panel settings-panel" role="tabpanel" aria-labelledby="tab-settings" tabindex="0" hidden={!active}>
  <header class="page-heading">
    <div>
      <p class="eyebrow">ARCADE TOOLS / PREFERENCES</p>
      <h1>Settings</h1>
      <p class="intro">Choose how Arcade Tools looks and connects.</p>
    </div>
  </header>

  <section class="settings-card" aria-labelledby="appearance-heading">
    <div>
      <h2 id="appearance-heading">Appearance</h2>
      <p class="setting-description">Follow your system or choose a theme.</p>
    </div>
    <label class="setting-control">Theme
      <select aria-label="Theme" value={theme} onchange={(event) => onTheme(event.currentTarget.value as Theme)}>
        <option value="system">System</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
  </section>

  <section class="settings-card" aria-labelledby="integration-heading">
    <div>
      <h2 id="integration-heading">Connections</h2>
      <p class="setting-description">Allow other Arcade apps to ask Tools to install an app after your confirmation.</p>
    </div>
    <label class="settings-link-toggle"><input id="settings-link-toggle" type="checkbox" checked={linkEnabled} disabled={busy} onchange={(event) => run(() => onLink(event.currentTarget.checked))} /> Connect with other Arcade apps</label>
  </section>

  <section class="settings-card diagnostics-card" aria-labelledby="diagnostics-heading">
    <div>
      <h2 id="diagnostics-heading">Diagnostics</h2>
      <p class="setting-description">Arcade Link status and local integration details.</p>
    </div>
    <pre id="diagnostics">{diagnostics}</pre>
  </section>

  <section class="settings-card about-card" aria-labelledby="about-heading">
    <div>
      <h2 id="about-heading">About</h2>
      <p class="setting-description">Arcade Tools keeps the Arcade app collection installed and up to date.</p>
    </div>
    <p class="about-version">Arcade Tools <strong>0.1.0</strong></p>
  </section>
</div>
