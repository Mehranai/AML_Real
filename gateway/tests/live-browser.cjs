// Actual panel, Gateway and services only; no route interception or response fixtures.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require('playwright');

async function main() {
  const [configPath, baseUrl, outputPath, executablePath] = process.argv.slice(2);
  assert(configPath && baseUrl && outputPath, 'Usage: node live-browser.cjs CONFIG GATEWAY_URL OUTPUT_DIR [CHROMIUM_PATH]');
  const config = JSON.parse(await fs.readFile(configPath, 'utf8'));
  const networks = ['bitcoin', 'ethereum', 'bsc', 'tron'];
  assert.deepEqual(Object.keys(config.networks).sort(), [...networks].sort());
  await fs.mkdir(outputPath, { recursive: true });
  const browser = await chromium.launch({ headless: true, ...(executablePath ? { executablePath } : {}) });
  const results = [];
  try {
    for (const network of networks) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 1100 } });
      try {
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        const investigationResponse = page.waitForResponse(r => new URL(r.url()).pathname.endsWith('/investigation') && r.status() === 200, { timeout: 180_000 });
        const route = new URL(`/networks/${network}/`, baseUrl);
        route.searchParams.set('address', config.networks[network].address);
        await page.goto(route.href);
        const response = await investigationResponse;
        assert.equal((await response.request().allHeaders())['x-aml-service-key'], undefined, 'service key leaked into a browser request');
        const created = await response.json();
        assert.equal(created.response_contract, 'chain_response_v1');
        assert.equal(created.source_validity.status, 'current');
        assert(created.graph.nodes.length && created.graph.edges.length);
        await page.locator('#snapshot-export:enabled').waitFor({ timeout: 180_000 });
        const exporting = page.waitForResponse(r => new URL(r.url()).pathname.endsWith('/export') && r.request().method() === 'POST' && r.status() === 200);
        await page.locator('#snapshot-export').click();
        const saved = await (await exporting).json();
        assert.equal(saved.investigation.state, 'saved');
        await page.waitForFunction(() => document.querySelector('#snapshot-status')?.textContent.includes('Saved permanently'));
        const id = created.investigation.id;
        await page.locator(`#snapshot-saved option[value="${id}"]`).waitFor({ state: 'attached' });
        const reopening = page.waitForResponse(r => new URL(r.url()).pathname === `/api/investigations/${id}` && r.status() === 200);
        await page.locator('#snapshot-saved').selectOption(id);
        const reopened = await (await reopening).json();
        assert.deepEqual(reopened.graph, created.graph);
        assert.deepEqual(reopened.evidence, created.evidence);
        assert.equal(reopened.source_validity.status, 'current');
        assert(!errors.length, `${network}: ${errors.join('; ')}`);
        await page.screenshot({ path: path.join(outputPath, `${network}.png`), fullPage: true });
        results.push({ network, status: 'pass', id, edges: created.graph.edges.length });
        console.log(`PASS ${network}: real browser investigation, Export and reopen`);
      } finally {
        await context.close();
      }
    }
  } finally {
    await browser.close();
  }
  await fs.writeFile(path.join(outputPath, 'result.json'), JSON.stringify({ status: 'pass', results }, null, 2));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
