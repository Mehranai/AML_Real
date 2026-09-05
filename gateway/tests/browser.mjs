import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const run = promisify(execFile);
const root = fileURLToPath(new URL('../../', import.meta.url));
const output = new URL('../../test-results/', import.meta.url);
const tron = 'TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ';
const tronTarget = 'TMeWat4Y7Sx8bfskXt1R5nDV3ZuiTDxr2N';
const eth = '0x0000000000000000000000000000000000000001';
const ethTarget = '0x0000000000000000000000000000000000000002';
const serviceKey = 'browser-test-service-key-000000000000000000000000';
const observed = [];
const ready = { status: 'ready', dependencies: { clickhouse: 'ready', neo4j: 'ready' } };
const tronGraph = {
  address: tron, nodes: [tron, tronTarget].map(id => ({ id, node_type: 'wallet' })),
  edges: [{ id: 'test-transfer', from: tron, to: tronTarget, amount: '10', operation_type: 'transfer', transfer_type: 'TRX' }],
  exchange_interactions: [], neo4j: { imported_wallet_nodes: 2, imported_transfer_edges: 1 },
};
const ethGraph = {
  nodes: [eth, ethTarget].map(address => ({ id: address, address, inbound_edges: 1, outbound_edges: 1 })),
  edges: [{ id: 'test-transfer', from_address: eth, to_address: ethTarget, amount: '10', transfer_type: 'native', asset_id: 'native:ETH', block_number: 1, tx_hash: 'test-hash' }],
};
const ethInvestigation = {
  address: eth, graph: ethGraph, fingerprint: { transaction_count: 1, transfer_count: 1, unique_counterparties: 1 },
  asset_flows: [], top_counterparties: [], semantic_events: [], entities: [], clusters: [], exposure_paths: [],
  risk_engine: { enabled: false, signals: [] }, data_coverage: {}, neo4j_projection: { projected: false, node_count: 0, edge_count: 0 },
};

async function mock(network) {
  const server = createServer(async (request, response) => {
    let body = '';
    for await (const chunk of request) body += chunk;
    const url = new URL(request.url, 'http://test');
    observed.push({ network, method: request.method, path: request.url, body, serviceKey: request.headers['x-aml-service-key'] });
    let result;
    if (url.pathname === '/ready') result = ready;
    else if (url.pathname.endsWith('/neo4j/import')) result = tronGraph;
    else if (url.pathname.includes('/paths/')) result = network === 'tron'
      ? { ...tronGraph, source_address: tron, target_address: tronTarget, max_depth: 10, path_count: 1, searched_node_count: 2, paths: [{ node_ids: [tron, tronTarget], edge_ids: ['test-transfer'], hop_count: 1 }] }
      : { ...ethGraph, source: eth, target: ethTarget, max_hops: 10, expanded_addresses: 2, paths: [{ addresses: [eth, ethTarget], hop_count: 1 }], neo4j_projection: { edge_count: 1 } };
    else if (url.pathname.endsWith('/investigation')) result = network === 'tron'
      ? { graph: tronGraph, fingerprint: { flows: { total_transfers: 1 } }, holdings: { total_asset_count: 1, assets: [], native_balance: { balance_decimal: '10' } }, activity: { summary: { outgoing_transfers: 1 } } }
      : ethInvestigation;
    else result = { network, method: request.method, url: request.url, body };
    response.writeHead(url.pathname.endsWith('/test-error') ? 422 : 200, { 'content-type': 'application/json' });
    response.end(JSON.stringify(result));
  });
  server.listen(0, '0.0.0.0');
  await once(server, 'listening');
  return server;
}

const tronServer = await mock('tron');
const ethServer = await mock('ethereum');
const reservation = await mock('unused');
const port = reservation.address().port;
await new Promise(resolve => reservation.close(resolve));
const env = {
  ...process.env, AML_PORT: String(port), AML_BIND_ADDRESS: '127.0.0.1',
  AML_TRON_UPSTREAM: `http://host.docker.internal:${tronServer.address().port}`,
  AML_ETHEREUM_UPSTREAM: `http://host.docker.internal:${ethServer.address().port}`,
  AML_SERVICE_KEY: serviceKey,
};
const project = `aml-whole-test-${process.pid}`;
const compose = (...args) => run('docker', ['compose', '-p', project, '-f', 'compose.yaml', ...args], { cwd: root, env, timeout: 120000 });
const base = `http://127.0.0.1:${port}`;
let browser;
let failure;
try {
  await mkdir(output, { recursive: true });
  await compose('up', '-d', '--no-build', 'gateway');
  for (let attempt = 0; attempt < 40; attempt++) {
    try { if ((await fetch(`${base}/health`)).ok) break; } catch { /* Wait for container startup. */ }
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  assert.equal((await fetch(`${base}/health`)).status, 200);
  for (const network of ['tron', 'ethereum']) {
    assert.deepEqual(await (await fetch(`${base}/networks/${network}/ready`)).json(), ready);
    const response = await fetch(`${base}/api/${network}/test-error?limit=20&encoded=a%2Bb`);
    assert.equal(response.status, 422);
    const echo = await response.json();
    assert.equal(echo.network, network);
    assert.equal(echo.url, `/api/${network}/test-error?limit=20&encoded=a%2Bb`);
    assert.equal(response.headers.get('cache-control'), 'no-store');
  }
  const snapshot = await (await fetch(`${base}/api/analysis/tron/wallet/${tron}`)).json();
  assert.equal(snapshot.network, 'tron');
  const post = await (await fetch(`${base}/api/tron/test-post?limit=23`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"test":true}' })).json();
  assert.equal(post.method, 'POST');
  assert.equal(post.body, '{"test":true}');
  assert.ok(observed.filter(item => item.path.startsWith('/api/')).every(item => item.serviceKey === serviceKey));
  assert.equal((await fetch(`${base}/api/solana/wallet/x`)).status, 404);
  assert.equal((await fetch(`${base}/.env`)).status, 404);
  console.log('PASS proxy isolation, queries, POST body, status codes, snapshot routing and unknown routes');

  try { browser = await chromium.launch({ headless: true }); }
  catch (error) {
    if (!String(error).includes('Executable')) throw error;
    browser = await chromium.launch({ headless: true, channel: 'msedge' });
  }
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(base);
  await page.waitForFunction(() => document.getElementById('network-list').getAttribute('aria-busy') === 'false');
  assert.equal(await page.locator('.state[data-state=ready]').count(), 6);
  assert.equal(await page.locator('img').evaluateAll(images => images.every(image => image.complete && image.naturalWidth > 0)), true);
  await page.screenshot({ path: fileURLToPath(new URL('home-desktop.png', output)), fullPage: true });
  await page.locator('#address').fill(eth);
  await page.getByRole('button', { name: 'Investigate', exact: true }).click();
  assert.match(await page.locator('#address-error').innerText(), /TRON/);
  await page.locator('#address').fill(tron);
  await page.getByRole('button', { name: 'Investigate', exact: true }).click();
  await page.waitForFunction(() => document.getElementById('metricEdges')?.textContent === '1');
  assert.match(page.url(), /\/networks\/tron\/\?address=/);
  assert.equal(await page.locator('#flowSvg .edge').count(), 1);
  assert.match(await page.locator('#nativeBalance').innerText(), /10.*TRX/);
  assert.equal(observed.filter(item => item.network === 'tron' && item.path.includes('/investigation?')).length, 1);
  await page.screenshot({ path: fileURLToPath(new URL('tron-desktop.png', output)), fullPage: true });
  await page.locator('[data-inspector-tab="paths"]').click();
  await page.locator('#pathTargetInput').fill(tronTarget);
  await page.locator('#pathDepthInput').selectOption('10');
  await page.locator('#pathButton').click();
  await page.waitForFunction(() => document.getElementById('pathSummary').textContent.includes('1 path(s)'));
  assert.ok(observed.some(item => item.network === 'tron' && item.path.includes('/paths/') && item.path.includes('max_depth=10')));
  await page.locator('#projectButton').click();
  await page.waitForFunction(() => !document.getElementById('projectButton').disabled);
  assert.ok(observed.some(item => item.network === 'tron' && item.method === 'POST' && item.path.includes('/neo4j/import')));
  await page.getByRole('link', { name: 'Networks', exact: true }).click();

  await page.locator('#network').selectOption('ethereum');
  await page.locator('#address').fill(eth);
  await page.getByRole('button', { name: 'Investigate', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('#evidence-content')?.textContent.includes('Behavioral fingerprint'));
  assert.match(page.url(), /\/networks\/ethereum\/\?address=/);
  assert.equal(await page.locator('#path-form').isVisible(), false);
  assert.equal(await page.locator('#graph-placeholder').isVisible(), false);
  assert.equal(observed.filter(item => item.network === 'ethereum' && item.path.includes('/investigation?')).length, 1);
  await page.waitForTimeout(300);
  const colors = await page.locator('canvas').evaluate(canvas => {
    const pixels = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
    const colors = new Set();
    for (let i = 0; i < pixels.length; i += 4) if (pixels[i + 3]) colors.add(`${pixels[i]},${pixels[i + 1]},${pixels[i + 2]}`);
    return colors.size;
  });
  assert.ok(colors > 20, `Graph canvas should be nonblank; got ${colors} colors`);
  await page.screenshot({ path: fileURLToPath(new URL('ethereum-desktop.png', output)), fullPage: true });
  await page.locator('#path-tab').click();
  assert.equal(await page.locator('#wallet-form').isVisible(), false);
  await page.locator('#target-address').fill(ethTarget);
  await page.locator('#max-hops').fill('10');
  await page.getByRole('button', { name: 'Find paths', exact: true }).click();
  await page.waitForFunction(() => document.getElementById('evidence-content').textContent.includes('Path 1'));
  assert.ok(observed.some(item => item.network === 'ethereum' && item.path.includes('/paths/') && item.path.includes('max_hops=10')));
  console.log('PASS automatic wallet search exactly once, graph/holdings rendering and 10-hop path controls for both networks');

  for (const width of [390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`${base}/?network=ethereum`);
    assert.equal(await page.locator('#network').inputValue(), 'ethereum');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    await page.screenshot({ path: fileURLToPath(new URL(`home-${width}.png`, output)), fullPage: true });
    for (const [network, address, selector] of [['tron', tron, '#metricEdges'], ['ethereum', eth, '#evidence-content']]) {
      await page.goto(`${base}/networks/${network}/?address=${address}`);
      await page.waitForFunction(({ selector, network }) => network === 'tron' ? document.querySelector(selector)?.textContent === '1' : document.querySelector(selector)?.textContent.includes('Behavioral fingerprint'), { selector, network });
      assert.equal(await page.getByRole('link', { name: 'Networks', exact: true }).isVisible(), true);
      if (network === 'ethereum') {
        assert.equal(await page.locator('#path-form').isVisible(), false);
        assert.equal(await page.locator('#graph-placeholder').isVisible(), false);
      }
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${network} overflows at ${width}px`);
      await page.screenshot({ path: fileURLToPath(new URL(`${network}-${width}.png`, output)), fullPage: true });
    }
  }
  assert.deepEqual(errors, []);
  console.log('PASS responsive layouts and screenshots at 390 / 768 / 1440, no JavaScript errors');

  await new Promise(resolve => tronServer.close(resolve));
  await page.goto(base);
  await page.waitForFunction(() => document.getElementById('network-list').getAttribute('aria-busy') === 'false');
  assert.equal(await page.locator('#tron-api').innerText(), 'Unavailable');
  assert.equal(await page.locator('#tron-clickhouse').innerText(), 'Unknown');
  assert.equal(await page.locator('#ethereum-api').innerText(), 'Ready');
  assert.equal((await fetch(`${base}/api/tron/test`, { signal: AbortSignal.timeout(12000) })).status, 503);
  console.log('PASS offline chain isolation and honest unknown dependency status');
} catch (error) {
  failure = error;
} finally {
  if (browser) await browser.close();
  try { await compose('down'); } catch (error) { failure ??= error; }
  tronServer.closeAllConnections();
  ethServer.closeAllConnections();
  tronServer.close();
  ethServer.close();
}
if (failure) throw failure;
