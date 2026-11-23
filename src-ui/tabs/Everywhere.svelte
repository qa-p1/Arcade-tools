<script lang="ts">
  import { asFailure } from '../lib/controller';
  import { glyph } from '../lib/glyphs';
  import Icon from '../components/Icon.svelte';
  import { api, type EverywhereChange, type EverywherePage } from '../lib/api';

  type Props = { active: boolean; run: (action: () => Promise<void>, label?: string) => void };
  let { active, run }: Props = $props();
  let page = $state<EverywherePage | null>(null);
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);

  const helper = $derived(page?.helper);
  const pinned = $derived((page?.pins ?? []).filter((pin) => pin.pinned));
  const unpinned = $derived((page?.pins ?? []).filter((pin) => !pin.pinned));
  const full = $derived(!!page && pinned.length >= page.maxPins);
  const short = (name: string) => name.replace(/^Arcade /, '');

  async function refresh(): Promise<void> {
    loading = true;
    error = null;
    try { page = await api.everywherePage(); loaded = true; }
    catch (problem) { error = asFailure(problem).message; }
    finally { loading = false; }
  }
  async function change(next: EverywhereChange): Promise<void> { page = await api.everywhereChange(next); }
  async function start(): Promise<void> { page = await api.everywhereStart(); }
  function setPins(references: string[]): Promise<void> { return change({ kind: 'pins', references }); }
  function move(index: number, by: number): Promise<void> {
    const order = pinned.map((pin) => pin.reference);
    const [item] = order.splice(index, 1);
    order.splice(index + by, 0, item);
    return setPins(order);
  }
  async function chooseFolder(): Promise<void> {
    if (!page) return;
    const folder = await api.everywhereChooseFolder();
    if (folder) await change({ kind: 'output', folder, askWhere: page.askWhere });
  }

  $effect(() => { if (active && !loaded && !loading && !error) void refresh(); });
</script>

<div id="panel-everywhere" class="tab-panel" role="tabpanel" aria-labelledby="tab-everywhere" tabindex="0" hidden={!active}>
  <header class="page-head">
    <div>
      <h1>Everywhere</h1>
      <p class="sub">Use your Arcade apps from your file manager's right-click menu, without opening them.</p>
    </div>
    <div class="page-actions">
      <button type="button" class="btn" class:busy={loading} disabled={loading} onclick={() => run(refresh)}><Icon name="refresh" size={14} />Refresh</button>
    </div>
  </header>

  {#if error}
    <div class="notice warn"><Icon name="alert" /><span class="row-main">{error}</span></div>
  {/if}

  {#if page && helper}
    <section class="section" aria-labelledby="everywhere-helper-heading">
      <div class="section-head"><h2 id="everywhere-helper-heading">Arcade helper</h2></div>
      <div class="row">
        <span class="glyph-tile"><Icon name="globe" size={18} /></span>
        <span class="row-main">
          <span class="row-title">{helper.running ? `Running · ${helper.running}` : 'Not running'}</span>
          <span class="row-sub">{helper.running
            ? 'It adds Arcade to your menus, runs the actions and shows their progress.'
            : helper.shipped ? 'Arcade Tools starts it at login. Start it now to add Arcade to your menus.' : 'This copy of Arcade Tools has no helper.'}</span>
        </span>
        {#if !helper.running && helper.shipped}
          <button type="button" class="btn sm" onclick={() => run(start)}><Icon name="play" size={14} />Start</button>
        {/if}
      </div>
      <div class="row">
        <span class="glyph-tile"><Icon name="folder" size={18} /></span>
        <span class="row-main">
          <span class="row-title">File managers</span>
          <span class="row-sub">{helper.fileManagers.length ? helper.fileManagers.join(', ') : 'None yet. The helper adds itself when it starts.'}</span>
        </span>
      </div>
      <div class="row">
        <span class="glyph-tile"><Icon name="link" size={18} /></span>
        <span class="row-main">
          <span class="row-title">Browsers</span>
          <span class="row-sub">{helper.browsers.length ? helper.browsers.join(', ') : 'None yet. Browsers need the Arcade extension, which comes in a later update.'}</span>
        </span>
      </div>
    </section>

    <section class="section" aria-labelledby="everywhere-pins-heading">
      <div class="section-head"><h2 id="everywhere-pins-heading">In the menu</h2></div>
      <p class="sub">Pinned actions show at the top level of the right-click menu, in this order (up to {page.maxPins}). Everything else is under “Arcade…”.</p>
      {#if page.pins.length === 0}
        <div class="notice"><Icon name="info" /><span class="row-main">No installed app offers file actions yet.</span></div>
      {/if}
      <div class="list">
        {#each pinned as pin, index (pin.reference)}
          <div class="row">
            <span class="glyph-tile"><img src={glyph(pin.app)} alt="" width="18" height="18" /></span>
            <span class="row-main"><span class="row-title">{pin.title}</span><span class="row-sub">{short(pin.app)}</span></span>
            <button type="button" class="btn sm icon-only" aria-label={`Move ${pin.title} up`} disabled={index === 0} onclick={() => run(() => move(index, -1))}><span class="flip"><Icon name="chevronDown" size={14} /></span></button>
            <button type="button" class="btn sm icon-only" aria-label={`Move ${pin.title} down`} disabled={index === pinned.length - 1} onclick={() => run(() => move(index, 1))}><Icon name="chevronDown" size={14} /></button>
            <input class="switch" type="checkbox" aria-label={`Pin ${pin.title}`} checked onchange={() => run(() => setPins(pinned.filter((other) => other.reference !== pin.reference).map((other) => other.reference)))} />
          </div>
        {/each}
        {#each unpinned as pin (pin.reference)}
          <div class="row">
            <span class="glyph-tile"><img src={glyph(pin.app)} alt="" width="18" height="18" /></span>
            <span class="row-main"><span class="row-title">{pin.title}</span><span class="row-sub">{short(pin.app)}</span></span>
            <input class="switch" type="checkbox" aria-label={`Pin ${pin.title}`} checked={false} disabled={full} title={full ? `Up to ${page.maxPins} pins` : undefined} onchange={() => run(() => setPins([...pinned.map((other) => other.reference), pin.reference]))} />
          </div>
        {/each}
      </div>
    </section>

    <section class="section" aria-labelledby="everywhere-output-heading">
      <div class="section-head"><h2 id="everywhere-output-heading">Results</h2></div>
      <div class="row">
        <span class="row-main"><span class="row-title">Save results to</span><span class="row-sub mono">{page.outputFolder}</span></span>
        <button type="button" class="btn sm" disabled={page.askWhere} onclick={() => run(chooseFolder)}><Icon name="folder" size={14} />Choose…</button>
      </div>
      <label class="row">
        <span class="row-main"><span class="row-title">Ask every time</span><span class="row-sub">The progress window asks where to save each result.</span></span>
        <input class="switch" type="checkbox" aria-label="Ask where to save every time" checked={page.askWhere} onchange={(event) => { const askWhere = event.currentTarget.checked; run(() => change({ kind: 'output', folder: page!.outputFolder, askWhere })); }} />
      </label>
    </section>

    {#if page.jobs.length}
      <section class="section" aria-labelledby="everywhere-jobs-heading">
        <div class="section-head"><h2 id="everywhere-jobs-heading">When two apps do the same thing</h2></div>
        <p class="sub">The menu shows both until you pick one here (or choose “Always use this” in the menu).</p>
        <div class="list">
          {#each page.jobs as job (job.job)}
            <label class="row">
              <span class="row-main"><span class="row-title">{job.choices[0]?.title ?? job.job}</span><span class="row-sub">{job.choices.map((choice) => short(choice.app)).join(' or ')}</span></span>
              <select class="select sm" aria-label={`Default for ${job.job}`} value={job.chosen ?? ''} onchange={(event) => { const reference = event.currentTarget.value || null; run(() => change({ kind: 'jobDefault', job: job.job, reference })); }}>
                <option value="">Show all</option>
                {#each job.choices as choice (choice.reference)}<option value={choice.reference}>{short(choice.app)}</option>{/each}
              </select>
            </label>
          {/each}
        </div>
      </section>
    {/if}
  {:else if loading}
    <p class="sub">Loading…</p>
  {/if}
</div>

<style>
  .flip { display: inline-flex; transform: rotate(180deg); }
  .mono { font-family: var(--font-mono, ui-monospace, monospace); word-break: break-all; }
  .section > .sub { margin: 0 0 8px; }
</style>
