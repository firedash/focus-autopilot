// SPDX-License-Identifier: Apache-2.0
// Takes the README's screenshots of the statistics and configuration page, from the same
// made-up data the checks use. Run through screenshots.sh.
//
// usage: node screenshots.js <folder for the images>
const { chromium } = require('playwright');

const out = process.argv[2];
const url = 'http://127.0.0.1:' + (process.env.FOCUS_UI_PORT || '47898') + '/ui/#t=dev-token';

(async () => {
  const browser = await chromium.launch();

  async function open(opts, tab) {
    const ctx = await browser.newContext({ deviceScaleFactor: 2, ...opts });
    const page = await ctx.newPage();
    await page.goto(url);
    await page.waitForSelector('.tiles');
    if (tab === 'config') {
      await page.click('#tab-config');
      await page.waitForSelector('.setting');
    }
    await page.evaluate(() => window.scrollTo(0, 0));
    return page;
  }

  // The whole Statistics tab: insights, the weekly chart, the totals and the app list.
  let page = await open({ viewport: { width: 1080, height: 900 }, colorScheme: 'light' });
  await page.screenshot({ path: out + '/statistics.png', fullPage: true });

  page = await open({ viewport: { width: 1080, height: 900 }, colorScheme: 'dark' });
  await page.screenshot({ path: out + '/statistics-dark.png', fullPage: true });

  // The Configuration tab down to the end of the Away app card: the first screenful of a long
  // page, cut where a card ends instead of in the middle of a setting.
  page = await open({ viewport: { width: 1080, height: 900 }, colorScheme: 'light' }, 'config');
  const away = await page.locator('#config-root section.card').nth(2).boundingBox();
  await page.screenshot({
    path: out + '/configuration.png',
    fullPage: true,
    clip: { x: 0, y: 0, width: 1080, height: away.y + away.height + 24 },
  });

  page = await open({ viewport: { width: 390, height: 820 }, colorScheme: 'light' });
  await page.screenshot({ path: out + '/statistics-phone.png' });

  await browser.close();
  console.log('wrote screenshots to ' + out);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
