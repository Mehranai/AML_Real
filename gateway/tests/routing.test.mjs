import test from 'node:test';
import assert from 'node:assert/strict';
import { getNetwork, investigationUrl, validateAddress } from '../web/assets/networks.js';

const tron = 'TB16q6kpSEW2WqvTJ9ua7HAoP9ugQ2HdHZ';
const ethereum = '0x0000000000000000000000000000000000000001';
const bitcoin = '1BoatSLRHtKNngkdXEeobR76b53LETtpyT';

test('Bitcoin has an explicit mainnet route without altering Base58 case', () => {
  assert.equal(investigationUrl('bitcoin', bitcoin), `/networks/bitcoin/?address=${bitcoin}`);
  assert.equal(validateAddress('bitcoin', ` ${bitcoin} `), bitcoin);
  assert.equal(getNetwork('bitcoin').chainId, 'bip122:000000000019d6689c085ae165831e93');
  const taproot = 'bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr';
  assert.equal(validateAddress('bitcoin', taproot), taproot);
  assert.equal(validateAddress('bitcoin', taproot.toUpperCase()), taproot.toUpperCase());
  for (const invalid of [ethereum, tron, 'tb1qtest', 'bc1Qmixedcase', '../ready', bitcoin + '?x=y']) {
    assert.throws(() => validateAddress('bitcoin', invalid), /Bitcoin/);
  }
});

test('each wallet routes only to its explicit network', () => {
  assert.equal(investigationUrl('tron', tron), `/networks/tron/?address=${tron}`);
  assert.equal(investigationUrl('ethereum', ethereum), `/networks/ethereum/?address=${ethereum}`);
  assert.equal(investigationUrl('bsc', ethereum), `/networks/bsc/?address=${ethereum}`);
  assert.equal(getNetwork('bsc').chainId, 'eip155:56');
  assert.throws(() => investigationUrl('bsc', tron), /BNB Smart Chain/);
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
