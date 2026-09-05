import { NETWORKS, getNetwork, investigationUrl } from './networks.js';

const form = document.getElementById('investigation-form');
const select = document.getElementById('network');
const input = document.getElementById('address');
const error = document.getElementById('address-error');
const list = document.getElementById('network-list');
const refresh = document.getElementById('refresh');

for (const network of NETWORKS) {
  select.add(new Option(network.name, network.id));
  const row = document.createElement('article');
  row.className = 'network-row';
  row.setAttribute('aria-label', network.name);
  // Only compile-time network metadata enters this template, never wallet input.
  row.innerHTML = `<div class="network-name"><img src="/assets/${network.image}" alt=""><div><strong>${network.name}</strong><small>${network.chainId}</small></div></div>
    ${['clickhouse', 'neo4j', 'api'].map((key, index) => `<div><span class="cell-label">${['ClickHouse', 'Neo4j', 'API'][index]}</span><span class="state" id="${network.id}-${key}" data-state="checking">Checking</span></div>`).join('')}
    <a class="row-link" href="/networks/${network.id}/">Open console</a>`;
  list.append(row);
}

const requested = new URLSearchParams(location.search).get('network');
if (getNetwork(requested)) select.value = requested;
function updateNetwork() {
  const network = getNetwork(select.value);
  input.placeholder = network.placeholder;
  input.setAttribute('aria-label', `${network.name} wallet address`);
  error.textContent = '';
  input.removeAttribute('aria-invalid');
}
select.addEventListener('change', updateNetwork);
input.addEventListener('input', () => { error.textContent = ''; input.removeAttribute('aria-invalid'); });
updateNetwork();

form.addEventListener('submit', event => {
  event.preventDefault();
  try {
    const url = investigationUrl(select.value, input.value);
    form.querySelector('button').disabled = true;
    location.assign(url);
  } catch (cause) {
    error.textContent = cause.message;
    input.setAttribute('aria-invalid', 'true');
    input.focus();
  }
});
window.addEventListener('pageshow', () => { form.querySelector('button').disabled = false; });

function setState(id, state) {
  const element = document.getElementById(id);
  element.dataset.state = state;
  element.textContent = { ready: 'Ready', unavailable: 'Unavailable', checking: 'Checking', unknown: 'Unknown' }[state];
}

async function checkNetwork(network) {
  for (const key of ['api', 'clickhouse', 'neo4j']) setState(`${network.id}-${key}`, 'checking');
  try {
    const response = await fetch(`/networks/${network.id}/ready`, { cache: 'no-store', signal: AbortSignal.timeout(10000) });
    const body = await response.json();
    const ready = response.ok && body.status === 'ready';
    setState(`${network.id}-api`, ready ? 'ready' : 'unavailable');
    for (const key of ['clickhouse', 'neo4j']) {
      const state = body.dependencies?.[key];
      setState(`${network.id}-${key}`, state === 'ready' ? 'ready' : state === 'unavailable' ? 'unavailable' : 'unknown');
    }
  } catch {
    setState(`${network.id}-api`, 'unavailable');
    for (const key of ['clickhouse', 'neo4j']) setState(`${network.id}-${key}`, 'unknown');
  }
}

async function checkAll() {
  refresh.disabled = true;
  list.setAttribute('aria-busy', 'true');
  try {
    await Promise.all(NETWORKS.map(checkNetwork));
    document.getElementById('last-checked').textContent = `Checked at ${new Date().toLocaleTimeString()}`;
  } finally {
    list.setAttribute('aria-busy', 'false');
    refresh.disabled = false;
  }
}
refresh.addEventListener('click', checkAll);
checkAll();
