# UI checks

The Tools UI in Chromium against a mock backend (`mock.js`) whose responses
can be delayed, to check races between overlapping requests and other
behaviour that needs no real apps.

```sh
npm run build
npx playwright install chromium   # once
npm run ui-checks                 # races, ux and layout; CI runs the same
node scripts/ui-checks/harness.mjs dist scripts/ui-checks/ux.mjs   # one scenario
SCREENS_DIR=target/ui-screens node scripts/ui-checks/harness.mjs dist scripts/ui-checks/screens.mjs
```

`screens.mjs` takes every tab in light and dark at 1100×760 and 700×600 for
before/after comparisons; it is not part of `ui-checks`.

Set `CHROMIUM` to use a Chromium other than Playwright's own. Each scenario
asserts the correct behaviour and prints PASS or FAIL.
