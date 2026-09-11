(() => {
  'use strict';
  if (!location.pathname.startsWith('/networks/')) return;
  const chain = location.pathname.split('/')[2];
  if (!['tron', 'ethereum', 'bsc'].includes(chain)) return;
  const host = document.querySelector(chain === 'tron' ? '.graph-title' : '.querybar');
  if (!host) return;
  const style = document.createElement('style');
  style.textContent = `
    .snapshot-tools {display:flex;align-items:center;flex-wrap:wrap;gap:8px;max-width:100%;padding:8px 0;font-size:12px}
    .snapshot-tools button,.snapshot-tools select {min-height:36px;max-width:100%;border:1px solid #b8c7bf;border-radius:4px;background:#fff;color:#20392c;padding:6px 10px}
    .snapshot-tools select {width:190px;min-width:0}
    .snapshot-tools [role=status] {overflow-wrap:anywhere;flex:1 1 150px}
    .snapshot-tools button:disabled {opacity:.5;cursor:default}
    .central-risk {overflow-wrap:anywhere;font-size:13px;line-height:1.6}
    .central-risk h3 {font-size:16px;margin:8px 0}
    .central-risk summary {cursor:pointer;padding:6px 0}
    .central-risk pre {white-space:pre-wrap;max-height:240px;overflow:auto;font-size:11px}
    .central-risk ul {padding-left:20px}
  `;
  document.head.append(style);
  const bar = document.createElement('div');
  bar.className = 'snapshot-tools';
  bar.innerHTML = '<button type="button" id="snapshot-export" disabled title="Permanently save this investigation in central Neo4j">Export</button><select id="snapshot-saved" aria-label="Open a saved investigation"><option value="">Saved investigations</option></select><button type="button" id="snapshot-refresh" title="Refresh saved investigations" aria-label="Refresh saved investigations"><img src="/assets/refresh-cw.svg" width="16" height="16" alt=""></button><span role="status" id="snapshot-status">No snapshot</span>';
  host.append(bar);
  const save = bar.querySelector('#snapshot-export');
  const select = bar.querySelector('#snapshot-saved');
  const status = bar.querySelector('#snapshot-status');
  let current = null;
  let sequence = 0;
  let busy = false;
  let reopenHandler;
  const escape = value => String(value ?? '').replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  async function json(url, options) {
    const response = await fetch(url, options);
    const body = await response.json();
    if (!response.ok) throw new Error(body.error || 'Investigation request failed');
    return body;
  }
  async function refresh() {
    const {investigations} = await json('/api/investigations');
    select.replaceChildren(new Option('Saved investigations', ''));
    for (const item of investigations.filter(item => item.network === chain)) {
      select.add(new Option(`${item.network_id} | ${item.address} | ${new Date(item.created_at_unix_ms).toLocaleString()}`, item.id));
    }
  }
  // Initialize the anonymous session before parallel wallet/path requests.
  const ready = refresh().catch(error => { status.textContent = error.message; });
  function show(data) {
    current = data?.investigation || null;
    save.disabled = busy || !current || current.state === 'saved';
    save.textContent = current?.state === 'saved' ? 'Saved' : 'Export';
    status.textContent = current
      ? `${current.network_id} | ${current.state === 'saved' ? 'Saved permanently' : 'Temporary until ' + new Date(current.expires_at_unix_ms).toLocaleString()}`
      : 'No central snapshot';
    const risk = document.getElementById('central-risk-panel');
    if (risk) risk.innerHTML = riskMarkup(data?.risk_engine);
  }
  async function query(url) {
    const version = ++sequence;
    busy = true;
    save.disabled = true;
    try {
      await ready;
      const data = await json(url);
      if (version !== sequence) throw new Error('A newer investigation request replaced this result.');
      busy = false;
      show(data);
      return data;
    } catch (error) {
      if (version === sequence) {
        busy = false;
        save.disabled = !current || current.state === 'saved';
        status.textContent = error.message;
      }
      throw error;
    }
  }
  async function exportCurrent() {
    if (busy || !current || current.state === 'saved') return;
    const id = current.id;
    save.disabled = true;
    try {
      const result = await json(`/api/investigations/${encodeURIComponent(id)}/export`, {
        method:'POST', headers:{'Content-Type':'application/json'}, body:'{}'
      });
      if (current?.id === id) {
        current = result.investigation;
        save.textContent = 'Saved';
        status.textContent = `${current.network_id} | Saved permanently`;
      }
      await refresh();
    } catch (error) { status.textContent = error.message; }
    finally { save.disabled = busy || !current || current.state === 'saved'; }
  }
  function riskMarkup(risk) {
    if (!risk) return '<div class="central-risk">Wallet risk is unavailable for this view.</div>';
    return `<div class="central-risk"><h3>${risk.risk_score == null ? 'Insufficient evidence' : escape(risk.risk_score) + ' / 100'} | ${escape(risk.risk_level)}</h3>
      <p>Evidence policy: ${escape(risk.policy_version)}</p>
      ${(risk.signals || []).map(signal => `<details><summary>+${Number(signal.contribution).toFixed(1)} ${escape(signal.signal_type)}</summary><p>${escape(signal.summary)}</p><pre>${escape(JSON.stringify(signal.evidence, null, 2))}</pre></details>`).join('')}
      <ul>${(risk.limitations || []).map(item=>`<li>${escape(item)}</li>`).join('')}</ul></div>`;
  }
  save.addEventListener('click', exportCurrent);
  bar.querySelector('#snapshot-refresh').addEventListener('click', () => refresh().catch(error=>{status.textContent=error.message;}));
  select.addEventListener('change', async () => {
    if (!select.value || !reopenHandler) return;
    try {
      const data = await query(`/api/investigations/${encodeURIComponent(select.value)}`);
      reopenHandler(data);
    } catch (error) { status.textContent = error.message; }
  });
  window.AmlSnapshots = {query, show, exportCurrent, riskMarkup, onReopen(handler) {reopenHandler=handler;}};
})();
