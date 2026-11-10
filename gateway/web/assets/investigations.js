(() => {
  'use strict';
  if (!location.pathname.startsWith('/networks/')) return;
  const chain = location.pathname.split('/')[2];
  if (!['tron', 'ethereum', 'bsc', 'bitcoin'].includes(chain)) return;
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
    .bridge-tools form {display:flex;gap:8px;flex-wrap:wrap;margin:8px 0}
    .bridge-tools input {min-width:0;max-width:100%;padding:5px}
  `;
  document.head.append(style);
  const bar = document.createElement('div');
  bar.className = 'snapshot-tools';
  bar.innerHTML = '<button type="button" id="snapshot-export" disabled title="Permanently save this investigation in central Neo4j">Export</button><select id="snapshot-saved" aria-label="Open a saved investigation"><option value="">Saved investigations</option></select><button type="button" id="snapshot-refresh" title="Refresh saved investigations" aria-label="Refresh saved investigations"><img src="/assets/refresh-cw.svg" width="16" height="16" alt=""></button><span role="status" id="snapshot-status">No snapshot</span>';
  host.append(bar);
  const evidencePanel = document.createElement('section');
  evidencePanel.className = 'central-risk';
  evidencePanel.setAttribute('aria-label', 'Common chain evidence');
  bar.after(evidencePanel);
  const bridgePanel = document.createElement('section');
  bridgePanel.className = 'central-risk bridge-tools';
  evidencePanel.after(bridgePanel);
  let evidenceData = null;
  const save = bar.querySelector('#snapshot-export');
  const select = bar.querySelector('#snapshot-saved');
  const status = bar.querySelector('#snapshot-status');
  let current = null;
  let sourceValidity = null;
  const freshnessNote = () => sourceValidity && sourceValidity.status !== 'current' ? ' | Historical snapshot: source changed or unverified; run a new investigation.' : '';
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
    const previous = current?.id;
    evidenceData = data;
    current = data?.investigation || null;
    sourceValidity = data?.source_validity || null;
    save.disabled = busy || !current || current.state === 'saved';
    save.textContent = current?.state === 'saved' ? 'Saved' : 'Export';
    status.textContent = current
      ? `${current.network_id} | ${current.state === 'saved' ? 'Saved permanently' : 'Temporary until ' + new Date(current.expires_at_unix_ms).toLocaleString()}`
      : 'No central snapshot';
    status.textContent += freshnessNote();
    const risk = document.getElementById('central-risk-panel');
    if (risk) risk.innerHTML = riskMarkup(data?.risk_engine);
    evidencePanel.innerHTML = evidenceMarkup(evidenceData, sourceValidity);
    if (previous !== current?.id) showBridges();
  }
  function showBridges() {
    bridgePanel.replaceChildren();
    if (!current) return;
    const bitcoin = chain === 'bitcoin';
    const url = `/api/investigations/${encodeURIComponent(current.id)}/bridges`;
    const coverage = {
      ethereum: 'cBridge USDC and USDT between Ethereum and BSC.',
      bsc: 'cBridge USDC and USDT with Ethereum, and Allbridge Core with TRON for USDT destinations.',
      tron: 'Allbridge Core with BSC for USDT destinations.',
      bitcoin: 'Threshold tBTC v2 standard deposits, confirmed sweeps, and optimistic mints.'
    };
    bridgePanel.innerHTML = `<details><summary>Cross-chain bridge evidence</summary>
      <p>Trace a transaction already shown in this investigation. Coverage includes ${coverage[chain]}</p>
      <form><label>Transaction hash <input name="hash" required maxlength="66" size="28"></label>
      <label>${bitcoin ? 'Output index (vout)' : 'Log index'} <input name="index" type="number" min="0" max="4294967295" step="1" placeholder="Optional"></label>
      <button type="submit" disabled>${bitcoin ? 'Match deposit reveal' : 'Match bridge operation'}</button>
      ${bitcoin ? '<button type="submit" value="sweep" disabled>Verify deposit sweep</button><button type="submit" value="mint" disabled>Verify optimistic mint</button>' : ''}
      </form><p role="status"></p><div></div></details>`;
    const form = bridgePanel.querySelector('form');
    const buttons = [...form.querySelectorAll('button')];
    const setDisabled = disabled => buttons.forEach(button => { button.disabled = disabled; });
    const message = bridgePanel.querySelector('[role=status]');
    const results = bridgePanel.querySelector('div');
    const traces = new Map();
    const labels = {
      matched: bitcoin ? 'Bitcoin output matched the Ethereum deposit reveal.' : 'Both receipts matched.',
      anchor_not_found: bitcoin ? 'No eligible Bitcoin output for this transaction and output index.' : 'No supported bridge log for this transaction or log index.',
      ambiguous_anchor: bitcoin ? 'Several candidates were found. Enter the exact Bitcoin output index.' : 'More than one possible source log. Try its exact log index when available.',
      ambiguous_peer: 'Several receipts claim this operation on the other network.',
      unsupported_route: 'This bridge route or messenger is outside the reviewed coverage.',
      unsupported_asset: 'This token is outside the reviewed bridge asset pairs.',
      peer_not_found_in_indexed_window: bitcoin ? 'No reveal in indexed data within 48 hours on either side of the Bitcoin block time.' : 'No peer receipt in the indexed 48-hour window.',
      inconsistent_pair: bitcoin ? 'The reveal and Bitcoin output did not meet the standard tBTC deposit checks.' : 'The receipts disagree with the matching policy.',
      sweep_spend_not_found: 'No spend of this output within the next 4,320 indexed Bitcoin blocks.',
      sweep_proof_not_found: 'No matching sweep proof in the indexed Ethereum time window.',
      ambiguous_sweep_spend: 'Several indexed inputs claim to spend this output.',
      ambiguous_sweep_proof: 'Several Ethereum logs claim this sweep.',
      inconsistent_sweep: 'The Ethereum proof does not match the Bitcoin spend and deposit wallet.',
      unsupported_mint_vault: 'The deposit does not target the supported tBTC vault.',
      mint_not_found: 'No optimistic mint finalization in the indexed seven-day window.',
      ambiguous_mint: 'Several Ethereum logs claim to finalize minting for this deposit.',
      inconsistent_mint: 'The finalization and token issuance logs could not be matched.',
      source_changed: 'Canonical evidence changed during matching. Retry shortly.'
    };
    const render = () => {
      const ref = leg => {
        const {tx_hash, output_index, input_index, log_index} = leg.receipt;
        const index = input_index != null ? `vin ${escape(input_index)}`
          : output_index != null ? `vout ${escape(output_index)}` : `log ${escape(log_index)}`;
        return `${escape(leg.network_id)} · ${escape(tx_hash)} · ${index}`;
      };
      const operation = {allbridge_core: 'Allbridge Core operation', threshold_tbtc_v2: 'Threshold tBTC v2 deposit reveal'};
      const labelsByClaim = {deposit_sweep: 'Threshold tBTC v2 confirmed deposit sweep', optimistic_mint: 'Threshold tBTC v2 optimistic mint'};
      const label = trace => labelsByClaim[trace.claim] || operation[trace.protocol] || 'cBridge operation';
      const limit = trace => {
        if (trace.claim === 'optimistic_mint') return 'The issuance record is scoped to the depositor. Sweep and settlement are evaluated separately.';
        if (trace.claim === 'deposit_sweep') return 'Token mint and settlement are unverified.';
        if (trace.claim === 'deposit_reveal') return 'Sweep and token mint are unverified.';
        return 'Settlement and continuity of funds are unverified.';
      };
      const issued = trace => {
        if (trace.claim !== 'optimistic_mint') return '';
        const raw = trace.relay.amount_raw.padStart(19, '0');
        const amount = `${raw.slice(0, -18)}.${raw.slice(-18)}`.replace(/\.?0+$/, '');
        return `<p>Issued ${escape(amount)} tBTC to ${escape(trace.relay.recipient)}</p>`;
      };
      results.innerHTML = Array.from(traces.values(), trace => `<details><summary>${escape(trace.asset)} ${label(trace)} · ${escape(trace.confidence)} confidence</summary>
        <p>${ref(trace.send)} &rarr; ${ref(trace.relay)}</p>
        ${issued(trace)}
        <p>Historical evidence captured at ${escape(new Date(trace.captured_at_unix_ms).toLocaleString())}. ${limit(trace)}</p>
        <pre>${escape(JSON.stringify(trace, null, 2))}</pre></details>`).join('');
    };
    json(url).then(({traces: stored}) => {
      for (const trace of stored) traces.set(trace.id, trace);
      render();
    }).catch(error => { message.textContent = error.message; }).finally(() => setDisabled(false));
    form.addEventListener('submit', async event => {
      event.preventDefault();
      if (buttons[0].disabled) return;
      const action = bitcoin ? event.submitter?.value : '';
      setDisabled(true);
      message.textContent = 'Looking up evidence on both networks…';
      const input = new FormData(form);
      const body = {tx_hash: input.get('hash').trim()};
      if (input.get('index') !== '') body[bitcoin ? 'output_index' : 'log_index'] = Number(input.get('index'));
      if (action === 'sweep') body.verify_sweep = true;
      if (action === 'mint') body.verify_mint = true;
      try {
        const result = await json(url, {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(body)});
        const matches = {
          sweep: 'Bitcoin spend matched the Ethereum sweep proof.',
          mint: 'tBTC issuance matched the exact Bitcoin deposit.'
        };
        message.textContent = (result.status === 'matched' && matches[action])
          || labels[result.status] || 'Bridge evidence is unavailable.';
        if (result.trace) { traces.set(result.trace.id, result.trace); render(); }
      } catch (error) { message.textContent = error.message; }
      finally { setDisabled(false); }
    });
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
        sourceValidity = result.source_validity || sourceValidity;
        const risk = document.getElementById('central-risk-panel');
        if (risk && result.source_validity) risk.innerHTML = riskMarkup(result.risk_engine);
        evidencePanel.innerHTML = evidenceMarkup(evidenceData, sourceValidity);
        save.textContent = 'Saved';
        status.textContent = `${current.network_id} | Saved permanently${freshnessNote()}`;
      }
      await refresh();
    } catch (error) { status.textContent = error.message; }
    finally { save.disabled = busy || !current || current.state === 'saved'; }
  }
  function riskMarkup(risk) {
    if (!risk) return '<div class="central-risk">Wallet risk is unavailable for this view.</div>';
    const suppressed = risk.status === 'source_snapshot_invalid' || risk.status === 'policy_unverified';
    const heading = risk.risk_score == null
      ? (suppressed ? 'Current assessment unavailable' : 'Insufficient evidence')
      : escape(risk.risk_score) + ' / 100';
    return `<div class="central-risk"><h3>${heading} | ${escape(risk.risk_level)}</h3>
      <p>Evidence policy: ${escape(risk.policy_version || 'unrecorded')}</p>
      ${(risk.signals || []).map(signal => `<details><summary>+${Number(signal.contribution).toFixed(1)} ${escape(signal.signal_type)}</summary><p>${escape(signal.summary)}</p><pre>${escape(JSON.stringify(signal.evidence, null, 2))}</pre></details>`).join('')}
      <ul>${(risk.limitations || []).map(item=>`<li>${escape(item)}</li>`).join('')}</ul></div>`;
  }
  function evidenceMarkup(data, validity) {
    const view = data?.evidence;
    if (view?.contract !== 'chain_response_v1') return '';
    const source = data.source_snapshot || {};
    const holdings = view.holdings || {};
    const assets = holdings.assets || [];
    const model = holdings.model === 'indexed_netflow' ? 'indexed netflow (not an RPC balance)' : holdings.model;
    const count = section => `${escape(view[section]?.status || 'unavailable')} | ${(view[section]?.items || []).length} returned`;
    const analysis = data.analysis_evidence;
    const clustering = analysis?.clustering;
    const exposure = analysis?.exposure;
    const analysisMarkup = analysis?.contract === 'chain_analysis_evidence_v1' && view.mode === 'wallet'
      && Array.isArray(clustering?.items) && exposure
      ? `<p>Clustering: ${escape(clustering.status)} | ${clustering.items.filter(item => item.kind === 'investigative_lead').length} investigative leads | ${clustering.items.filter(item => item.kind === 'reviewed_association').length} reviewed associations<br>
          Exposure: ${escape(exposure.status)} | ${escape(exposure.direct_count)} direct | ${escape(exposure.indirect_count)} indirect (returned evidence)</p>`
      : '';
    const semantics = data.semantic_evidence;
    const semanticItems = Array.isArray(semantics?.items) ? semantics.items : [];
    const semanticMarkup = semantics?.contract === 'chain_semantic_evidence_v1' && view.mode === 'wallet'
      ? `<details><summary>Protocol and NFT evidence</summary>
          <p>${semanticItems.filter(item => item.grade === 'decoded_receipt').length} decoded receipts |
          ${semanticItems.filter(item => item.grade === 'observed_transfer').length} observed NFT transfers |
          ${semanticItems.filter(item => ['candidate','behavioral_candidate'].includes(item.grade)).length} candidates</p>
          <p>Receipt coverage: ${escape(semantics.coverage?.protocol_receipts)}.
            Mixer coverage: ${escape(semantics.coverage?.mixer_receipts)}. NFT coverage: ${escape(semantics.coverage?.nft_transfers)}.</p>
          ${semantics.coverage?.semantic_events_truncated || semantics.coverage?.nft_transfers_truncated
            ? '<p>Some events are outside the returned evidence window.</p>' : ''}
          <ul>${semanticItems.slice(0,12).map(item => `<li>${escape(item.event_type)} | ${escape(item.grade)}
            | tx ${escape(item.tx_hash)}${item.receipt ? ` | log ${escape(item.receipt.log_index)}` : ''}
            ${item.token_id != null ? ` | token ${escape(item.token_id)} | raw amount ${escape(item.amount_raw)}` : ''}
            | identity ${escape(item.protocol_identity?.status)}
            ${item.protocol_identity?.protocol ? ` (${escape(item.protocol_identity.protocol)})` : ''}</li>`).join('')}</ul>
          ${semanticItems.length > 12 ? `<p>Showing 12 of ${semanticItems.length} returned events.</p>` : ''}
          </details>`
      : '';
    const market = data.nft_marketplace_evidence;
    const marketMarkup = market?.contract === 'nft_marketplace_evidence_v1' && view.mode === 'wallet'
      ? `<details><summary>NFT marketplace · Seaport 1.6</summary>
          <p>${escape(market.status)} · ${escape(market.searched_transactions)} NFT transactions checked.</p>
          <p>Order receipts show on-chain order terms and related NFT transfers. Sale price and payment settlement are unverified.</p>
          ${market.items.map(item => `<details><summary>Order ${escape(item.order.order_hash)}</summary>
            <p>Transaction ${escape(item.receipt.tx_hash)} · log ${escape(item.receipt.log_index)}</p>
            <pre>${escape(JSON.stringify({order:item.order, related_nft_transfers:item.related_nft_transfers,
              transfer_refs_truncated:item.transfer_refs_truncated, receipt:item.receipt}, null, 2))}</pre></details>`).join('')}
          </details>`
      : '';
    return `<details><summary>Evidence | ${escape(data.network_id)} | ${escape(validity?.status || 'not_checked')}</summary>
      <p>Block: ${escape(source.as_of_block ?? 'waiting')} | History: ${source.history_complete === true ? 'from genesis to this block' : 'partial or unknown'} | Model: ${escape(view.graph?.model)}</p>
      <p>Activity: ${count('activity')}<br>Paths: ${count('paths')} (bounded connectivity)<br>Attribution: ${count('attribution')} (claims, not ownership)</p>
      ${analysisMarkup}
      ${semanticMarkup}
      ${marketMarkup}
      <p>Holdings: ${escape(holdings.status)} | ${escape(model)}. Unavailable amounts are not zero.</p>
      <ul>${assets.slice(0,20).map(asset => `<li>${escape(asset.asset_id)} | raw amount: ${escape(asset.amount_raw ?? 'unavailable')} | ${escape(asset.status)}</li>`).join('')}</ul>
      ${assets.length > 20 ? `<p>Showing 20 of ${assets.length} returned assets.</p>` : ''}
      <p>Snapshot: ${escape(source.snapshot_id)}. No continuity of particular funds or ownership is asserted.</p></details>`;
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
  window.AmlSnapshots = {query, show, exportCurrent, riskMarkup, evidenceMarkup, onReopen(handler) {reopenHandler=handler;}};
})();
