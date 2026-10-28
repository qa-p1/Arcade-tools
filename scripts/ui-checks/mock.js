// Mock Tauri backend for the Tools UI. Injected before the page loads.
// window.__mock.on(cmd, fn) overrides a command; fn may return a promise
// (use window.__mock.later(ms, value) to control response order).
(() => {
  const CATALOG = __CATALOG__;
  const ENGINES = __ENGINES__;
  const callbacks = {};
  const listeners = {};
  let next = 1;
  const app = (id, extra = {}) => {
    const entry = CATALOG.apps.find((a) => a.id === id);
    return {
      id, name: entry.name, pitch: entry.pitch, tagline: entry.tagline ?? entry.pitch, releasesUrl: `https://github.com/${entry.repo}/releases`,
      version: '1.0.0', managed: true, installed: true, running: false, channel: 'stable', healthy: true,
      startAtLogin: false, loginSupported: true, executable: `/home/u/Applications/Arcade/${id}.AppImage`,
      dataFolders: [`/home/u/.config/${id}`], reason: null, description: entry.description, category: entry.category,
      icon: entry.icon, platforms: entry.platforms, minTools: entry.minTools, installReason: null,
      installSupported: true, installationClass: 'managed', rollbackVersion: null, ...extra,
    };
  };
  const state = {
    apps: CATALOG.apps.map((a) => app(a.id)),
    preferences: {
      linkEnabled: true, channels: {}, updates: { enabled: true, autoUpdate: {} },
      notificationsEnabled: true, theme: 'light', onboardingComplete: true,
    },
    updates: { schema: 1, lastCheck: null, lastSuccess: null, lastFailure: null, apps: {} },
  };
  const defaults = {
    initialize: () => ({ apps: state.apps, preferences: state.preferences, updates: state.updates, selected: null, diagnostics: [], testing: false }),
    list_apps: () => state.apps,
    update_state: () => state.updates,
    preferences: () => state.preferences,
    tools_app: () => ({ ...app('arcade.box'), id: 'arcade.tools', name: 'Arcade Tools', version: '0.2.1' }),
    self_install_proposal: () => null,
    shortcuts_index: () => null,
    rebuild_shortcut_index: () => ({ error: null }),
    set_app_shortcut: () => ({ applied: true, via: 'hyprland' }),
    shortcut_labels: () => ({}),
    catalog_data: () => ({ catalog: CATALOG, engines: ENGINES }),
    tray_ui_intent: () => null,
    tray_get_settings: () => ({ preferences: { hostIcons: true, excluded: [], showUpdateBadge: true, startAtLogin: true, shortcutsEnabled: true, shortcut: 'Super+Slash' }, apps: [], loginAvailable: true, defaultShortcut: 'Super+/', shortcutStatus: { accelerator: 'Super+Slash', available: true, via: null, hint: null, error: null } }),
    update_history: () => [],
    list_connections: () => ({ linkEnabled: true, peers: [] }),
    dashboard: () => ({ apps: [], totalMemoryBytes: 0, totalCpuPercent: 0 }),
    health: () => [],
    list_learn_apps: () => CATALOG.apps.map((a) => ({ id: a.id, name: a.name, installedVersion: '1.0.0', docsVersion: '1.0.0', available: true, source: 'bundled', needsFetch: false, docsUrl: '' })),
    docs_pages: ({ id }) => [{ path: `${id}/index.md`, title: `${id} intro` }],
    read_doc: ({ id, path }) => ({ app: id, version: '1.0.0', path, title: path, html: `<p>BODY:${id}</p>`, source: 'bundled', needsFetch: false }),
    search_docs: ({ query }) => [{ app: 'arcade.find', version: '1.0.0', path: 'x.md', title: `HIT:${query}`, snippet: '', score: 1 }],
    release_history: ({ id }) => [{ version: `${id}-9.0`, publishedAt: '2026-10-01', prerelease: false, html: '' }],
    render_markdown: ({ markdown }) => `<p>${markdown}</p>`,
    list_engines: () => [],
    list_bundles: () => [],
    ui_rendered: () => null,
    check_release: ({ id }) => ({ id, version: '2.0.0', channel: 'stable', file: 'x.AppImage', notes: 'notes', updateAvailable: true }),
  };
  const mock = {
    state, calls: [], handlers: {},
    on(cmd, fn) { this.handlers[cmd] = fn; },
    later(ms, value) { return new Promise((resolve) => setTimeout(() => resolve(typeof value === 'function' ? value() : value), ms)); },
    emit(event, payload) { (listeners[event] || []).forEach((id) => callbacks[id] && callbacks[id]({ event, payload, id: 0 })); },
    app,
  };
  window.__mock = mock;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    transformCallback(cb) { const id = next++; callbacks[id] = cb; return id; },
    unregisterCallback(id) { delete callbacks[id]; },
    convertFileSrc: (path) => path,
    async invoke(cmd, args = {}) {
      mock.calls.push([cmd, JSON.parse(JSON.stringify(args ?? {}))]);
      if (cmd === 'plugin:event|listen') { (listeners[args.event] ||= []).push(args.handler); return args.handler; }
      if (cmd.startsWith('plugin:')) return null;
      const handler = mock.handlers[cmd] ?? defaults[cmd];
      if (!handler) return null;
      return handler(args);
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
})();
