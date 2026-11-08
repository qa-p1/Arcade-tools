// Accessibility basics (README Round 5): WCAG 2.1 A/AA rules including contrast (axe-core) on every tab in both
// themes, every control reachable by Tab with a visible focus indicator, no horizontal scrolling at 125% and 150%
// display scaling, and no motion when the system asks for reduced motion.
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';

const AXE = createRequire(import.meta.url).resolve('axe-core/axe.min.js');
const INDEX = JSON.parse(readFileSync(new URL('../../src-ui/fixtures/shortcuts-index.json', import.meta.url)));
const TABS = ['home', 'apps', 'updates', 'shortcuts', 'learn', 'connections', 'engines', 'settings'];
const assert = (ok, message) => { if (!ok) throw new Error(message); };

async function ready(page) {
  await page.waitForSelector('#tab-shortcuts', { timeout: 10000 });
  await page.evaluate((index) => { window.__mock.on('shortcuts_index', () => index); window.__mock.emit('shortcut-index-built', null); }, INDEX);
}

async function show(page, tab) {
  await page.click(`#tab-${tab}`);
  await page.waitForSelector(`#panel-${tab}:not([hidden])`, { timeout: 10000 });
  await page.waitForTimeout(300);
}

export default {
  async 'WCAG A/AA (axe) on every tab, light and dark'(page) {
    await ready(page);
    await page.addScriptTag({ path: AXE });
    const found = [];
    for (const theme of ['light', 'dark']) {
      await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
      for (const tab of TABS) {
        await show(page, tab);
        const violations = await page.evaluate(async () => {
          const result = await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] } });
          return result.violations.map((v) => `${v.id} (${v.impact}): ${v.nodes.length}× e.g. ${v.nodes[0].target.join(' ')} — ${v.nodes[0].failureSummary.split('\n').slice(1, 2).join('').trim()}`);
        });
        for (const v of violations) found.push(`${theme}/${tab}: ${v}`);
      }
    }
    assert(found.length === 0, `axe found: ${[...new Set(found)].join(' | ')}`);
  },

  async 'every control is reachable by Tab and shows where focus is'(page) {
    await ready(page);
    const problems = [];
    for (const tab of TABS) {
      await show(page, tab);
      const controls = await page.evaluate((id) => {
        const scope = [document.querySelector('nav, .sidebar'), document.querySelector(`#panel-${id}`)].filter(Boolean);
        const all = scope.flatMap((root) => [...root.querySelectorAll('button, a[href], input, select, textarea, summary, [tabindex]')]);
        let n = 0;
        return all.filter((el) => {
          const visible = el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';
          const focusable = !el.disabled && el.getAttribute('tabindex') !== '-1' && !el.closest('[inert], [hidden]');
          return visible && focusable;
        }).map((el) => { el.dataset.a11y = String(n++); return el.dataset.a11y; });
      }, tab);
      await page.evaluate(() => { document.activeElement?.blur(); window.scrollTo(0, 0); });
      const reached = new Set();
      for (let i = 0; i < controls.length + 60; i += 1) {
        await page.keyboard.press('Tab');
        const focused = await page.evaluate(() => {
          const el = document.activeElement;
          if (!el || el === document.body) return null;
          // Focus shows when the element or a near ancestor (a search field's frame) changes outline, shadow or
          // border colour between focused and not focused.
          const near = [el, el.parentElement, el.parentElement?.parentElement].filter(Boolean);
          const look = () => near.map((n) => { const s = getComputedStyle(n); return `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor}|${s.boxShadow}|${s.borderColor}`; });
          const own = getComputedStyle(el);
          let ring = own.outlineStyle !== 'none' && parseFloat(own.outlineWidth) > 0;
          if (!ring) {
            const focusedLook = look();
            el.blur();
            const blurredLook = look();
            el.focus();
            ring = focusedLook.some((v, i) => v !== blurredLook[i]);
          }
          return { id: el.dataset.a11y ?? null, ring, label: `${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''} "${(el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 40)}"` };
        });
        if (!focused) continue;
        if (!focused.ring) problems.push(`${tab}: no visible focus on ${focused.label}`);
        if (focused.id !== null) reached.add(focused.id);
      }
      const missed = controls.filter((id) => !reached.has(id));
      if (missed.length) {
        const labels = await page.evaluate((ids) => ids.map((id) => {
          const el = document.querySelector(`[data-a11y="${id}"]`);
          return `${el.tagName.toLowerCase()} "${(el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 40)}"`;
        }), missed);
        problems.push(`${tab}: not reachable by Tab: ${labels.join(', ')}`);
      }
      await page.evaluate(() => document.querySelectorAll('[data-a11y]').forEach((el) => delete el.dataset.a11y));
    }
    assert(problems.length === 0, [...new Set(problems)].join(' | '));
  },

  async 'no horizontal scrolling at 125% and 150% scaling'(page) {
    await ready(page);
    const problems = [];
    // A 1100×760 window at 125% and 150% scaling has this many CSS pixels.
    for (const [scale, width, height] of [['125%', 880, 608], ['150%', 733, 506]]) {
      await page.setViewportSize({ width, height });
      for (const tab of TABS) {
        await show(page, tab);
        const overflow = await page.evaluate((id) => {
          const panel = document.querySelector(`#panel-${id}`);
          const doc = document.documentElement;
          const wide = [doc, panel].filter((el) => el && el.scrollWidth > el.clientWidth + 1);
          return wide.map((el) => `${el.id || el.tagName.toLowerCase()} ${el.scrollWidth}>${el.clientWidth}`);
        }, tab);
        if (overflow.length) problems.push(`${scale} ${tab}: ${overflow.join(', ')}`);
      }
    }
    assert(problems.length === 0, problems.join(' | '));
  },

  async 'reduced motion leaves nothing moving'(page) {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await ready(page);
    const moving = new Set();
    for (const tab of TABS) {
      await show(page, tab);
      const found = await page.evaluate(() => [...document.querySelectorAll('*')].flatMap((el) => {
        const style = getComputedStyle(el);
        const seconds = (value) => Math.max(...value.split(',').map((v) => parseFloat(v) * (v.trim().endsWith('ms') ? 0.001 : 1)));
        const animated = style.animationName !== 'none' && seconds(style.animationDuration) > 0.01;
        const transitioned = seconds(style.transitionDuration) > 0.01;
        return animated || transitioned ? [`${el.tagName.toLowerCase()}.${[...el.classList].join('.')} (${animated ? style.animationName : style.transitionProperty})`] : [];
      }));
      for (const f of found) moving.add(`${tab}: ${f}`);
    }
    assert(moving.size === 0, `still moving with reduced motion: ${[...moving].slice(0, 20).join(' | ')}`);
  },
};
