export const NETWORKS = Object.freeze([
  Object.freeze({ id: 'tron', name: 'TRON', chainId: 'tron:mainnet', placeholder: 'T...', pattern: /^T[1-9A-HJ-NP-Za-km-z]{33}$/, image: 'tron.png' }),
  Object.freeze({ id: 'ethereum', name: 'Ethereum', chainId: 'eip155:1', placeholder: '0x...', pattern: /^0x[0-9a-fA-F]{40}$/, image: 'ethereum.png' }),
  Object.freeze({ id: 'bsc', name: 'BNB Smart Chain', chainId: 'eip155:56', placeholder: '0x...', pattern: /^0x[0-9a-fA-F]{40}$/, image: null }),
  Object.freeze({ id: 'bitcoin', name: 'Bitcoin', chainId: 'bip122:000000000019d6689c085ae165831e93', placeholder: '1…, 3…, bc1q… or bc1p…', pattern: /^(?:[13][1-9A-HJ-NP-Za-km-z]{25,34}|bc1[ac-hj-np-z02-9]{11,87}|BC1[AC-HJ-NP-Z02-9]{11,87})$/, image: null }),
]);

export function getNetwork(id) {
  return NETWORKS.find(network => network.id === id);
}

// Format check only; the chain API remains responsible for canonical validation.
export function validateAddress(networkId, raw) {
  const network = getNetwork(networkId);
  if (!network) throw new Error('Select a supported network.');
  const address = String(raw ?? '').trim();
  if (!address) throw new Error('Enter a wallet address.');
  if (!network.pattern.test(address)) {
    throw new Error(networkId === 'bitcoin'
      ? 'Enter a Bitcoin mainnet address starting with 1, 3, bc1q or bc1p.'
      : networkId === 'tron'
      ? 'Enter a TRON Base58 address: 34 characters, starting with T.'
      : `Enter an ${network.name} address: 0x followed by 40 hexadecimal characters.`);
  }
  return address;
}

export function investigationUrl(networkId, raw) {
  const address = validateAddress(networkId, raw);
  return `/networks/${networkId}/?${new URLSearchParams({ address })}`;
}
