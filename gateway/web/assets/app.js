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
  row.innerHTML = `<div class="network-name">${network.image ? `<img src="/assets/${network.image}" alt="">` : '<span style="display:grid;place-items:center;width:40px;height:40px;background:#f0b90b;color:#181a20;font-size:11px;font-weight:700" aria-hidden="true">BNB</span>'}<div><strong>${network.name}</strong><small>${network.chainId}</small></div></div>
    ${['clickhouse', 'api'].map((key, index) => `<div><span class="cell-label">${['ClickHouse', 'API'][index]}</span><span class="state" id="${network.id}-${key}" data-state="checking">Checking</span></div>`).join('')}
    <div><span class="cell-label">Graph storage</span><span>Main VM</span></div>
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
  for (const key of ['api', 'clickhouse']) setState(`${network.id}-${key}`, 'checking');
  try {
    const response = await fetch(`/networks/${network.id}/ready`, { cache: 'no-store', signal: AbortSignal.timeout(10000) });
    const body = await response.json();
    const ready = response.ok && body.status === 'ready';
    setState(`${network.id}-api`, ready ? 'ready' : 'unavailable');
    for (const key of ['clickhouse']) {
      const state = body.dependencies?.[key];
      setState(`${network.id}-${key}`, state === 'ready' ? 'ready' : state === 'unavailable' ? 'unavailable' : 'unknown');
    }
  } catch {
    setState(`${network.id}-api`, 'unavailable');
    for (const key of ['clickhouse']) setState(`${network.id}-${key}`, 'unknown');
  }
}

async function checkAll() {
  refresh.disabled = true;
  list.setAttribute('aria-busy', 'true');
  try {
    const results = await Promise.all([...NETWORKS.map(checkNetwork),
      fetch('/ready', {signal:AbortSignal.timeout(10000)}).then(r=>r.ok).catch(()=>false)]);
    document.getElementById('last-checked').textContent = `Central Neo4j: ${results.at(-1) ? 'Ready' : 'Unavailable'} | Checked at ${new Date().toLocaleTimeString()}`;
  } finally {
    list.setAttribute('aria-busy', 'false');
    refresh.disabled = false;
  }
}
refresh.addEventListener('click', checkAll);
checkAll();
