// SPDX-License-Identifier: Apache-2.0
// Drives the statistics and configuration page in a real browser against a running
// focus-headless, and checks that config.toml and the running app follow what the page does.
// Run it through run.sh, which seeds the data and starts the app.
//
// usage: node check.js <data folder> <app log file> [folder for screenshots]
const { chromium } = require('playwright');
const fs = require('fs');

const home = process.argv[2];
const configPath = home + '/config.toml';
const logPath = process.argv[3];
const shots = process.argv[4];
const url = 'http://127.0.0.1:' + (process.env.FOCUS_UI_PORT || '47821') + '/ui/';
let failures = 0;

function check(name, ok, detail) {
  if (!ok) failures += 1;
  console.log((ok ? 'PASS  ' : 'FAIL  ') + name + (ok || !detail ? '' : '\n        ' + detail));
}

const configText = () => fs.readFileSync(configPath, 'utf8');
const log = () => fs.readFileSync(logPath, 'utf8');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const browser = await chromium.launch();
  const problems = [];
  const ctx = await browser.newContext({ viewport: { width: 1100, height: 900 }, deviceScaleFactor: 2 });
  const p = await ctx.newPage();
  p.on('console', (m) => {
    // A refused save is an expected 400; the browser logs it. Anything else is a problem.
    if (m.text().includes('status of 400')) return;
    if (m.type() === 'error' || m.type() === 'warning') problems.push('[' + m.type() + '] ' + m.text());
  });
  p.on('pageerror', (e) => problems.push('[pageerror] ' + e.message));
  const state = async () => (await p.textContent('#save-state')).trim();
  const saved = async () => {
    await p.waitForFunction(() => {
      const t = document.getElementById('save-state').textContent;
      return t.startsWith('Saved') || t.startsWith('Not saved');
    });
    return state();
  };
  const clearState = () => p.evaluate(() => { document.getElementById('save-state').textContent = ''; });

  // Without the key: a clear message, and no data.
  await p.goto(url);
  await p.waitForSelector('#notice:not([hidden])');
  check('without the key the page says how to open it', (await p.textContent('#notice')).includes('focusctl ui'));
  check('without the key no numbers are shown', (await p.locator('.tile').count()) === 0);

  // With the key.
  await p.goto(url + '#t=dev-token');
  await p.waitForSelector('.tiles');
  check('the key is taken out of the address bar', p.url() === url);
  if (shots) await p.screenshot({ path: shots + '/statistics.png', fullPage: true });
  check('statistics render', (await p.locator('.tile').count()) === 5 && (await p.locator('.col').count()) === 7);

  // Tabs by keyboard.
  await p.focus('#tab-stats');
  await p.keyboard.press('ArrowRight');
  await p.waitForSelector('#panel-config .setting');
  check('arrow keys move between the tabs', (await p.getAttribute('#tab-config', 'aria-selected')) === 'true');

  // 1. A switch: LLM recap.
  const before = configText();
  await p.click('[data-key="recap.mode"]');
  check('turning on the LLM recap saves', (await saved()) === 'Saved.');
  check('config.toml now says mode = "llm"', configText().includes('mode = "llm"'));
  check('the comment in config.toml survived', configText().startsWith('# my settings'));
  check('the token is still in the file, untouched', configText().includes('token = "dev-token"') && before.includes('token = "dev-token"'));
  check('the timeout field appears once LLM recap is on', (await p.locator('[data-key="recap.llm_timeout_seconds"]').count()) === 1);
  await sleep(300);
  check('the running app applied it', log().includes('settings changed: applied'));

  // 2. A number.
  await clearState();
  await p.fill('[data-key="ui.recap_seconds"]', '25');
  await p.press('[data-key="ui.recap_seconds"]', 'Tab');
  check('changing a number saves', (await saved()) === 'Saved.');
  check('config.toml has recap_seconds = 25', /recap_seconds = 25/.test(configText()));

  // 3. A number out of range is refused in the page and never sent.
  await clearState();
  await p.fill('[data-key="countdown.seconds"]', '0');
  await p.press('[data-key="countdown.seconds"]', 'Tab');
  await sleep(200);
  check('a countdown of 0 is refused with a reason', (await state()).startsWith('Not saved') && (await state()).includes('1 to 60'));
  check('the field snaps back to the real value', (await p.inputValue('[data-key="countdown.seconds"]')) === '3');
  check('config.toml has no countdown of 0', !/seconds = 0/.test(configText()));

  // 4. Away app from the list of running apps.
  await clearState();
  await p.selectOption('[data-key="away.apps#pick"]', 'com.apple.Notes');
  check('picking a running app as the away app saves', (await saved()) === 'Saved.');
  check('config.toml lists the away app', configText().includes('apps = ["com.apple.Notes"]'));
  check('the away app shows as a chip by name', (await p.locator('.chip').first().textContent()).includes('Notes'));
  check('the "hard to leave" choice appears for it', (await p.locator('[data-key="away.immersive#com.apple.Notes"]').count()) === 1);

  // 5. A home app can't be an away app: the server refuses, the page explains, nothing is written.
  await clearState();
  const fileBefore = configText();
  await p.fill('[data-key="away.apps#text"]', 'com.apple.Terminal');
  await p.press('[data-key="away.apps#text"]', 'Enter');
  const refusal = await saved();
  check('a home app is refused as an away app, in words', refusal.startsWith('Not saved') && refusal.includes('is a home app'), refusal);
  check('a refused change leaves config.toml byte for byte', configText() === fileBefore);

  // 6. Mark immersive, then remove the away app: both go together.
  await clearState();
  await p.check('[data-key="away.immersive#com.apple.Notes"]');
  check('marking an app hard to leave saves', (await saved()) === 'Saved.');
  await clearState();
  await p.click('.chip button');
  check('removing the away app saves (and clears its hard-to-leave mark)', (await saved()) === 'Saved.');
  check('config.toml has no away app left', /apps = \[\]/.test(configText()) && /immersive = \[\]/.test(configText()));

  // 7. Hotkey: valid one saves with a restart note; invalid one is refused.
  await clearState();
  await p.fill('[data-key="hotkeys.pause"]', 'Control+Option+P');
  await p.press('[data-key="hotkeys.pause"]', 'Tab');
  check('a hotkey change saves and says a restart is needed', (await saved()).includes('Restart focusd'));
  await clearState();
  await p.fill('[data-key="hotkeys.pause"]', 'P');
  await p.press('[data-key="hotkeys.pause"]', 'Tab');
  const hk = await saved();
  check('a hotkey with no modifier is refused', hk.startsWith('Not saved') && hk.includes('modifier'), hk);
  check('the hotkey field snaps back', (await p.inputValue('[data-key="hotkeys.pause"]')) === 'Control+Option+P');

  // 8. Advanced section and a list.
  await p.click('summary');
  await clearState();
  await p.fill('[data-key="ignore.cwd_globs#text"]', '/Users/me/scratch/**');
  await p.press('[data-key="ignore.cwd_globs#text"]', 'Enter');
  check('adding an ignored folder saves', (await saved()) === 'Saved.');
  check('the Advanced section stays open after a save', await p.evaluate(() => document.querySelector('details').open));
  if (shots) {
    // Back to the top first, so the sticky header is drawn once, where it belongs.
    await p.evaluate(() => window.scrollTo(0, 0));
    await p.screenshot({ path: shots + '/configuration.png', fullPage: true });
  }

  // 9. Editing the file by hand is picked up by the running app, and the page shows it.
  fs.writeFileSync(configPath, configText().replace('recap_seconds = 25', 'recap_seconds = 40'));
  await sleep(2600);
  check('a hand edit of config.toml is applied by the running app', log().includes('config.toml changed: applied'));
  await p.click('#tab-stats');
  await p.click('#tab-config');
  // The tab shows what it had, then refreshes from the app: wait for the refresh.
  const shown = await p
    .waitForFunction(() => document.querySelector('[data-key="ui.recap_seconds"]').value === '40', null, { timeout: 5000 })
    .then(() => true, () => false);
  check('the page shows the hand-edited value', shown);

  // 10. A broken hand edit is not applied and the app keeps running on the last good config.
  const good = configText();
  fs.writeFileSync(configPath, good + '\n[switching]\naway_after_secs = 5\n');
  await sleep(2600);
  check('a typo in config.toml is reported, not applied', log().includes('was not applied'));
  fs.writeFileSync(configPath, good);
  await sleep(2600);

  // 11. Statistics: range switch, table view, clear.
  await p.click('#tab-stats');
  await p.waitForSelector('.tiles');
  const todayPrompts = await p.locator('.tile .value').first().textContent();
  await p.click('.segmented button:has-text("Last 7 days")');
  const weekPrompts = await p.locator('.tile .value').first().textContent();
  check('the range switch changes the tiles', Number(weekPrompts) > Number(todayPrompts), todayPrompts + ' vs ' + weekPrompts);
  await p.click('text=Show as table');
  check('a chart has a table view', (await p.locator('table.data').count()) === 1);
  await p.focus('.app-row');
  check('keyboard focus shows the same tooltip as hover', !(await p.evaluate(() => document.getElementById('tooltip').hidden)));
  await p.click('text=Clear statistics');
  await p.click('text=Yes, erase all of it');
  await p.waitForFunction(() => document.querySelector('.insights li').textContent.startsWith('Nothing recorded yet'));
  check('clearing statistics empties the page', (await p.locator('.tile .value').first().textContent()) === '0');
  check('the history file is gone', !fs.existsSync(home + '/runs.jsonl') && !fs.existsSync(home + '/usage.json'));
  await sleep(300);
  check('the running app was told the history was cleared', log().includes('statistics and run history cleared'));

  check('no console errors or policy violations', problems.length === 0, problems.join('\n        '));
  await browser.close();
  console.log(failures === 0 ? '\nALL PASSED' : '\n' + failures + ' FAILED');
  process.exit(failures === 0 ? 0 : 1);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
