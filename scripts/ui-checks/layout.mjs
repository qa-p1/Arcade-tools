const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export default {
  async 'Onboarding bundle text does not overlap'(page) {
    await page.evaluate(() => window.__mock.on('list_bundles', () => [
      { id: 'b1', title: 'Everyday desktop helpers', description: 'Find files fast, capture text from the screen and keep a clipboard history for the whole week.', apps: ['arcade.find', 'arcade.lens', 'arcade.clipboard'] },
      { id: 'b2', title: 'Everything', description: 'All seven Arcade apps.', apps: '*' }]));
    await page.click('#tab-onboarding');
    await page.click('text=Choose apps');
    await page.waitForSelector('.bundle-list .bundle-card');
    await page.setViewportSize({ width: 760, height: 900 });
    await sleep(200);
    const overlaps = await page.$$eval('.bundle-list .bundle-card', (cards) => cards.flatMap((card) => {
      const boxes = [...card.children].map((el) => el.getBoundingClientRect());
      const out = [];
      for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
        const [a, b] = [boxes[i], boxes[j]];
        if (a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom) out.push(`${card.textContent.slice(0, 20)}: ${i}/${j}`);
      }
      for (let i = 1; i < boxes.length; i++) if (boxes[i].top < boxes[i - 1].bottom - 1) out.push(`${card.textContent.slice(0, 20)}: part ${i} beside part ${i - 1}`);
      const r = card.getBoundingClientRect();
      boxes.forEach((b, i) => { if (b.right > r.right + 1) out.push(`${i} overflows card`); });
      return out;
    }));
    if (overlaps.length) throw new Error(overlaps.join('; '));
  },
};
