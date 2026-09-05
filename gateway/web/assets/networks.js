export const NETWORKS = Object.freeze([
  Object.freeze({ id: 'tron', name: 'TRON', chainId: 'tron:mainnet', placeholder: 'T...', pattern: /^T[1-9A-HJ-NP-Za-km-z]{33}$/, image: 'tron.png' }),
  Object.freeze({ id: 'ethereum', name: 'Ethereum', chainId: 'eip155:1', placeholder: '0x...', pattern: /^0x[0-9a-fA-F]{40}$/, image: 'ethereum.png' }),
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
    throw new Error(networkId === 'tron'
      ? 'Enter a TRON Base58 address: 34 characters, starting with T.'
      : 'Enter an Ethereum address: 0x followed by 40 hexadecimal characters.');
  }
  return address;
}

export function investigationUrl(networkId, raw) {
  const address = validateAddress(networkId, raw);
  return `/networks/${networkId}/?${new URLSearchParams({ address })}`;
}
