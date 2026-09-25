import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';

// UI fixtures are confined to this test; the application still reads chain APIs.
const tokenId = '115792089237316195423570985008687907853269984665640564039457584007913129639935';
const data = {
  address: '0x1111111111111111111111111111111111111111', fingerprint: {},
  asset_flows: [{ asset_id: 'eip155:1/erc1155:0x2222222222222222222222222222222222222222',
    token_id: tokenId, transfer_count: 2, inbound_amount: '5', outbound_amount: '1' }],
  top_counterparties: [], semantic_events: [], entities: [], clusters: [], exposure_paths: [],
  risk_engine: {}, data_coverage: {}, neo4j_projection: { projected: false },
  graph: {truncated:false}, exposure_coverage: {truncated:false}, activity: [], limitations: [],
  holdings: {status:'partial',persisted:true,block_number:42,observed_at_unix_ms:1700000000000,assets:[
    {asset_id:'eip155:1/native:eth',symbol:'ETH',decimals:18,token_id:'',amount:'1000000000000000000',status:'available'},
    {asset_id:'eip155:1/erc1155:0x2222222222222222222222222222222222222222',token_id:tokenId,decimals:0,amount:'2',status:'available'},
    {asset_id:'eip155:1/erc20:0x3333333333333333333333333333333333333333',token_id:'',decimals:18,amount:null,status:'unavailable'},
    {asset_id:'eip155:1/erc20:0x4444444444444444444444444444444444444444',token_id:'',decimals:18,amount:'0',status:'available'},
    {asset_id:'eip155:1/erc20:0x5555555555555555555555555555555555555555',token_id:'',decimals:0,amount:tokenId,status:'available'},
  ]},
};
const browser = await chromium.launch({ ...(process.platform === 'win32' ? {channel:'msedge'} : {}), headless: true });
try {
  const errors = [];
  await mkdir(new URL('../../test-results/', import.meta.url), {recursive:true});
  for (const network of ['ethereum', 'bsc']) {
   const html = (await readFile(new URL(`../../dockerizd_${network}/web/index.html`, import.meta.url), 'utf8'))
     .replace(/<script[^>]*src=[^>]*><\/script>/g, '');
   for (const width of [1280, 390]) {
    const page = await browser.newPage();
    page.on('pageerror', e => errors.push(e.message));
    await page.route('**/*', route => route.abort());
    await page.setViewportSize({width, height:900});
    await page.setContent(html);
    const payload = structuredClone(data);
    if (network === 'bsc') {
      for (const asset of [...payload.asset_flows, ...payload.holdings.assets]) {
        asset.asset_id = asset.asset_id.replace('eip155:1/', 'eip155:56/').replace('/native:eth','/native:bnb');
      }
      payload.holdings.assets[0].symbol = 'BNB';
    }
    await page.evaluate(payload => renderInvestigation(payload), payload);
    const row = page.locator('.holding-row').filter({hasText:'Token ID'});
    assert.equal(await row.count(), 1);
    assert.ok((await row.innerText()).includes(tokenId));
    assert.equal(await row.evaluate(el => el.scrollWidth <= el.clientWidth + 1), true);
    assert.deepEqual(await page.locator('.holding-amount').allTextContents(), ['1','2','Unavailable','0',tokenId]);
    assert.equal(await page.locator('.holding-row').evaluateAll(rows => rows.every(el => el.scrollWidth <= el.clientWidth + 1)), true);
    await row.scrollIntoViewIfNeeded();
    await page.screenshot({path:fileURLToPath(new URL(`../../test-results/${network}-evidence-${width}.png`, import.meta.url))});
    await page.close();
  }
  }
  assert.deepEqual(errors, []);
  console.log('ETH/BSC evidence UI: exact units, zero vs unavailable, full NFT IDs, desktop/mobile overflow checks passed.');
} finally { await browser.close(); }
