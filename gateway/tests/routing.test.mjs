import test from 'node:test';
import assert from 'node:assert/strict';
import { getNetwork, investigationUrl, validateAddress } from '../web/assets/networks.js';

const tron = 'TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ';
const ethereum = '0x0000000000000000000000000000000000000001';

test('each wallet routes only to its explicit network', () => {
  assert.equal(investigationUrl('tron', tron), `/networks/tron/?address=${tron}`);
  assert.equal(investigationUrl('ethereum', ethereum), `/networks/ethereum/?address=${ethereum}`);
});
test('whitespace is trimmed without corrupting address case', () => {
  assert.equal(validateAddress('tron', ` ${tron} `), tron);
  const mixedCase = '0xAbCd000000000000000000000000000000000001';
  assert.equal(validateAddress('ethereum', mixedCase), mixedCase);
});
test('wallets from the other network are rejected', () => {
  assert.throws(() => investigationUrl('tron', ethereum), /TRON/);
  assert.throws(() => investigationUrl('ethereum', tron), /Ethereum/);
});
test('unknown networks never become proxy destinations', () => {
  for (const network of ['__proto__', 'constructor', 'https://example.com', '../ethereum', 'solana']) {
    assert.equal(getNetwork(network), undefined);
    assert.throws(() => investigationUrl(network, tron), /supported network/);
  }
});
test('empty, malformed and injection inputs are rejected', () => {
  for (const value of ['', ' ', null, '../ready', '<script>alert(1)</script>', `${tron}?target=x`, 'T' + '0'.repeat(33)]) {
    assert.throws(() => investigationUrl('tron', value));
  }
  assert.throws(() => investigationUrl('ethereum', '0x' + 'g'.repeat(40)));
  assert.throws(() => investigationUrl('ethereum', ethereum + '1'));
});
