# UI checks

The Tools UI in Chromium against a mock backend (`mock.js`) whose responses
can be delayed, to check races between overlapping requests and other
behaviour that needs no real apps.

```sh
npm run build
PLAYWRIGHT_MODULES=/path/to/node_modules node scripts/ui-checks/harness.mjs dist scripts/ui-checks/races.mjs
node scripts/ui-checks/harness.mjs dist scripts/ui-checks/ux.mjs
node scripts/ui-checks/harness.mjs dist scripts/ui-checks/layout.mjs
```

Set `CHROMIUM` to use a Chromium other than Playwright's own. Each scenario
asserts the correct behaviour and prints PASS or FAIL.
