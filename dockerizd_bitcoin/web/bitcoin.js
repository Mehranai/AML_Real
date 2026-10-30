(() => {
  'use strict';
  const byId = id => document.getElementById(id);
  const address = byId('address');
  let sequence = 0;
  const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const short = value => value.length > 25 ? value.slice(0, 14) + '…' + value.slice(-8) : value;
  const nodeId = node => node.subject_id ?? node.id ?? node.address;
  const fromId = edge => edge.from_subject_id ?? edge.from ?? edge.from_address;
  const toId = edge => edge.to_subject_id ?? edge.to ?? edge.to_address;
  function btc(value) {
    const n = BigInt(value || '0');
    return `${n / 100000000n}.${String(n % 100000000n).padStart(8, '0')}`;
  }
  function utc(value) {
    if(value == null) return '—';
    const date=new Date(Number(value));return Number.isFinite(date.getTime()) ? date.toISOString() : String(value);
  }
  function activity(data) {
    const f=data.fingerprint || {}, c=data.activity_coverage || {}, scope=data.query_scope?.analytics;
    const filters=scope?.filters;
    if(filters) {
      for(const [id,key] of [['from-block','from_block'],['to-block','to_block']]) byId(id).value=filters[key] ?? '';
      for(const [id,key] of [['from-time','from_unix_ms'],['to-time','to_unix_ms']]) {const formatted=utc(filters[key]);byId(id).value=formatted.endsWith('Z') ? formatted.slice(0,-1) : '';}
      byId('activity-direction').value=filters.direction || 'all';
    }
    const metrics=[['Received BTC',f.received_btc],['Spent BTC',f.spent_btc],['Net flow BTC',f.net_flow_btc],['Transaction fees BTC (not wallet-attributed)',f.fees_of_involved_transactions_btc],['Wallet spent inputs / received outputs',f.spent_input_count == null ? null : `${f.spent_input_count} / ${f.received_output_count}`],['Both-side transactions',f.both_sides_transactions],['First activity (UTC)',f.first_seen_unix_ms == null ? null : utc(f.first_seen_unix_ms)],['Last activity (UTC)',f.last_seen_unix_ms == null ? null : utc(f.last_seen_unix_ms)],['Maximum transaction inputs / outputs',f.maximum_transaction_inputs == null ? null : `${f.maximum_transaction_inputs} / ${f.maximum_transaction_outputs}`],['Unique peer subjects',f.unique_counterparties]];
    byId('activity-metrics').innerHTML=metrics.map(([name,value])=>`<p><small>${escape(name)}</small><strong>${escape(value ?? '—')}</strong></p>`).join('');
    const filterText=filters ? `Blocks ${filters.from_block ?? 'start'} through ${filters.to_block ?? 'tip'}; direction ${filters.direction}; UTC ${utc(filters.from_unix_ms)} through ${utc(filters.to_unix_ms)}.` : '';
    const coverageText=c.status === 'complete_within_scope' ? 'Complete for the selected ingested window' : c.status === 'waiting_for_blocks' ? 'Waiting for committed blocks' : 'Activity not supplied';
    byId('activity-scope').textContent=`${data.source_validity?.current_risk_usable === false ? 'Historical snapshot. ' : ''}${coverageText}. ${filterText} ${c.source_snapshot?.history_complete === false ? 'Partial ingestion history. ' : ''}${scope?.requested_block_bounds_ingested === false ? 'Requested block bounds extend outside the ingested window. ' : ''}Graph, exposure and holdings have their own scopes; risk uses reviewed labels and its exposure scope.`;
    const days=data.activity || [], max=Math.max(1,...days.map(d=>Number(d.transactions)));
    byId('daily-activity').innerHTML=days.length ? days.map(d=>`<tr><td>${escape(utc(d.day_unix_ms).slice(0,10))}</td><td>${escape(d.transactions)} <span class="activity-bar" style="width:${Math.round(Number(d.transactions)/max*80)}px"></span></td><td>${escape(d.received_btc)}</td><td>${escape(d.spent_btc)}</td><td>${escape(d.net_flow_btc)}</td></tr>`).join('') : '<tr><td colspan="5">No activity buckets in this snapshot.</td></tr>';
    byId('days-scope').textContent=`Active UTC days only; zero days are not filled. ${c.activity_truncated ? 'Day display limit reached; summary remains complete within scope.' : ''}`;
    const peers=data.top_counterparties || [];
    byId('counterparties').innerHTML=peers.length ? peers.map(p=>`<tr><td>${p.subject_type === 'address' ? `<a href="/networks/bitcoin/?address=${encodeURIComponent(p.subject_id)}">${escape(p.subject_id)}</a>` : escape(p.subject_id)}<br>${escape(p.subject_type)}</td><td>${escape(p.transaction_count)}</td><td>${escape(p.upstream_transactions)}</td><td>${escape(p.downstream_transactions)}</td></tr>`).join('') : '<tr><td colspan="4">No peer subjects in the selected snapshot; absence does not prove no counterparties.</td></tr>';
    byId('counterparties-scope').textContent=`Peer inputs in receipt transactions and peer outputs in spend transactions are context, not allocated payments or common ownership. ${c.counterparties_truncated ? 'Top-subject display limit reached.' : ''}`;
    byId('recent-transactions').innerHTML=(data.recent_transactions || []).map(t=>`<tr><td>${escape(t.block_number)}<br>${escape(t.tx_hash)}<br>${escape(t.block_hash)}</td><td>${escape(utc(t.block_timestamp_unix_ms))}</td><td>${escape(t.spent_input_count)} / ${escape(t.received_output_count)}</td><td>${escape(t.received_btc)} / ${escape(t.spent_btc)}</td><td>${t.coinbase === 1 ? 'Coinbase: not applicable' : btc(t.transaction_fee_sats)}</td></tr>`).join('') || '<tr><td colspan="5">No transactions in the selected snapshot.</td></tr>';
    byId('recent-scope').textContent=c.transactions_truncated ? 'Recent transaction display limit reached; aggregate statistics still cover the selected scope.' : 'Each transaction occurrence is counted once. This panel does not claim wallet ownership of other inputs or outputs.';
  }
  async function query(url) {
    if (window.AmlSnapshots) return window.AmlSnapshots.query(url);
    const response = await fetch(url);
    const data = await response.json();
    if (!response.ok) throw new Error(data.error || 'Bitcoin API unavailable');
    return data;
  }
  function draw(graph) {
    const nodes = graph.nodes || [], edges = graph.edges || [];
    const shown = nodes.slice(0, 100);
    const incoming = new Set(edges.filter(e=>e.transfer_type === 'utxo_input').map(fromId));
    const columns = [[], [], []];
    for (const node of shown) columns[node.node_type === 'transaction' ? 1 : incoming.has(nodeId(node)) ? 0 : 2].push(node);
    const height = Math.max(240, Math.max(...columns.map(c=>c.length)) * 56 + 60);
    const points = new Map();
    columns.forEach((column, col) => column.forEach((node,i)=>points.set(nodeId(node),{x:col*460+160,y:i*56+40,node})));
    let svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1240" height="${height}" viewBox="0 0 1240 ${height}" role="img" aria-label="Observed Bitcoin address, transaction and script evidence"><defs><marker id="arrow" markerWidth="6" markerHeight="6" refX="6" refY="3" orient="auto"><path d="M0 0L6 3L0 6" fill="#8b9d93"/></marker></defs>`;
    for (const edge of edges.slice(0,500)) {
      const a=points.get(fromId(edge)), b=points.get(toId(edge));
      if (!a || !b) continue;
      const direction = b.x > a.x ? 1 : -1;
      svg += `<path d="M${a.x+direction*130},${a.y} C${(a.x+b.x)/2},${a.y} ${(a.x+b.x)/2},${b.y} ${b.x-direction*130},${b.y}" fill="none" stroke="#8b9d93" marker-end="url(#arrow)"><title>${escape(edge.transfer_type)} · ${btc(edge.amount)} BTC · ${escape(edge.tx_hash)}</title></path>`;
    }
    for (const {x,y,node} of points.values()) svg += `<g><title>${escape(nodeId(node))}</title><rect x="${x-130}" y="${y-17}" width="260" height="34" rx="5" fill="${node.node_type==='transaction'?'#fff0d8':'#e5f2ec'}" stroke="#afc8ba"/><text x="${x}" y="${y+4}" text-anchor="middle">${escape(short(nodeId(node)))}</text></g>`;
    byId('graph').innerHTML=svg+'</svg>';
    byId('graph-scope').textContent = `${nodes.length} nodes, ${edges.length} edges. ${graph.truncated ? 'Query limits reached; evidence is partial. ' : ''}${nodes.length > 100 || edges.length > 500 ? 'Display limited to 100 nodes and 500 edges; inspect raw evidence for all returned data.' : ''}`;
    byId('evidence').innerHTML=edges.slice(0,500).map(e=>`<tr><td>${escape(fromId(e))}</td><td>${escape(toId(e))}</td><td>${btc(e.amount)}</td><td>${escape(e.transfer_type)}</td><td>${escape(e.tx_hash)} / ${escape(e.input_index ?? e.output_index)}</td></tr>`).join('');
  }
  function show(data) {
    const worker=data.data_coverage?.analytics_worker;
    byId('analytics-worker').textContent=worker ? `Captured analytics worker status: ${worker.phase || 'unknown'} (${worker.reason || 'unspecified'}). Heartbeat ${worker.heartbeat_fresh ? 'fresh at capture' : 'not fresh at capture'}; queued jobs: ${Array.isArray(worker.queue) ? worker.queue.length : 0}; completed attempts: ${worker.succeeded ?? 0}; failed jobs: ${worker.failed_jobs ?? 0}. Bounded sweeps do not establish complete or current analysis of every address.` : 'Analytics worker status was not supplied in this snapshot.';
    if (data.address) address.value=data.address;
    draw(data.graph || {});
    const holdings=data.holdings || {};
    byId('balance').textContent=holdings.balance_decimal ?? '—';
    byId('balance-scope').textContent=holdings.history_complete ? 'Confirmed snapshot; mempool excluded' : 'Partial history: observed UTXOs only';
    if(holdings.status === 'waiting_for_blocks') byId('balance-scope').textContent='Waiting for committed blocks; balance is unknown';
    byId('utxos').textContent=holdings.utxo_count ?? '—';
    byId('transactions').textContent=data.fingerprint?.transaction_count ?? '—';
    activity(data);
    const semanticEvents=data.semantic_events || [], semanticCoverage=data.semantic_coverage || {};
    byId('semantic-scope').textContent=`${data.source_validity?.current_risk_usable === false ? 'Historical snapshot. ' : ''}${semanticCoverage.status || 'Semantic evidence not supplied'}. ${semanticCoverage.detector_version || ''} ${semanticCoverage.roots_truncated || semanticCoverage.context_limited || semanticCoverage.event_limit_reached ? 'Query limits reached. ' : ''}${semanticCoverage.canonical_fact_events ?? 0} stored events; ${semanticCoverage.computed_events ?? 0} computed from canonical facts. Roots use recent activity filters; outpoint context can extend outside them. These patterns do not increase the risk score or prove common ownership.`;
    byId('semantic-events').innerHTML=semanticEvents.length ? semanticEvents.map(e=>`<tr><td>${escape(e.event_type)}<br>${escape(e.detector_version)}</td><td>${escape(e.confidence)}<br>Pattern match strength, not crime probability</td><td>${escape(e.storage)}<details><summary>Transaction and outpoint evidence</summary>${(e.evidence?.transactions || []).map(t=>`<p>Block ${escape(t.block_number)} · ${escape(t.tx_hash)}<br>${escape(t.block_hash)}<br>Inputs: ${(t.inputs || []).map(i=>`${escape(i.prev_tx_hash)}:${escape(i.prev_output_index)} (${escape(i.value_sats)} sats)`).join(', ')}<br>Outputs: ${(t.outputs || []).map(o=>`${escape(o.index)} (${escape(o.value_sats)} sats; ${escape(o.subject_type)})`).join(', ')}</p>`).join('')}</details></td><td>Context only; no ownership claim<br>${escape(e.evidence?.match?.identification || '')}</td></tr>`).join('') : '<tr><td colspan="4">No patterns returned in this scope. This does not prove absence of these patterns.</td></tr>';
    byId('history').textContent=holdings.history_complete ? 'From genesis' : 'Partial / empty';
    byId('height').textContent=holdings.as_of_block == null ? 'No committed block' : `As of block ${holdings.as_of_block}`;
    const holdingsStatus={available:'Available for the ingested history',partial:'Observed holdings in partial history',waiting_for_blocks:'Waiting for committed blocks'}[holdings.status] || 'Holdings not supplied';
    byId('holdings-scope').textContent=`${data.source_validity?.current_risk_usable === false ? 'Historical snapshot. ' : ''}${holdingsStatus}. ${holdings.history_complete ? 'Ingested history starts at genesis.' : 'Observed window only; this is not a complete wallet balance.'} Activity filters do not restrict holdings. Mature amounts do not verify keys or script spendability. Mempool is excluded.`;
    const metadata=holdings.metadata || {};
    byId('holdings-meta').textContent=metadata.metadata_version ? `${metadata.name} (${metadata.symbol}), ${metadata.decimals} decimals. ${metadata.verification?.status === 'verified_protocol_constants' ? 'Verified protocol constants (system verification)' : 'Metadata verification unavailable'}. Source: ${metadata.source_reference}. Circulating supply is not estimated.` : 'Native metadata was not supplied in this snapshot.';
    byId('holdings-mature').textContent=holdings.mature_btc ?? '—';
    byId('holdings-immature').textContent=holdings.immature_btc ?? '—';
    byId('holdings-spent').textContent=holdings.spent_output_count ?? '—';
    const outputStatus={spent:'Spent',unspent:'Unspent',unspent_immature_coinbase:'Unspent · immature coinbase'};
    byId('holdings-outputs').innerHTML=(holdings.outputs || []).map(o=>`<tr><td>${escape(o.outpoint)}<br>Block ${escape(o.block_number)} · ${escape(o.script_type)}</td><td>${escape(o.value_btc)}</td><td>${escape(outputStatus[o.status] || o.status)}${o.is_spent === 1 ? `<br>Spent in block ${escape(o.spent_in_block)}<br>${escape(o.spent_in_tx_hash)}:${escape(o.spent_in_input_index)}` : o.maturity ? `<br>${escape(o.maturity.blocks_to_maturity)} blocks to maturity at tip; next-block eligible: ${escape(o.maturity.coinbase_eligible_in_next_block)}` : ''}</td><td>${escape(o.confirmations)}</td></tr>`).join('') || '<tr><td colspan="4">No output states supplied. Check data and history coverage before treating an empty result as zero.</td></tr>';
    byId('holdings-output-scope').textContent=`Latest occurrence per outpoint; superseded BIP30 occurrences remain in activity history. ${holdings.outputs_truncated ? 'Recent output list truncated; totals still cover all ingested output states.' : ''} ERC token standards and EVM contract metadata: ${holdings.capabilities?.erc20 || 'not supplied'}.`;
    const claims=data.intelligence?.claims || data.entities || [];
    byId('labels').innerHTML=claims.length ? claims.map(c=>`<tr><td>${escape(c.entity_name)} / ${escape(c.entity_type)}</td><td>${escape(c.review_status)} / ${data.source_validity?.current_risk_usable === false ? 'historical' : c.active === true ? 'active' : 'inactive'}</td><td>${escape(c.source_id)} · ${escape(c.source_reference)}<br>${(c.evidence_refs || []).map(escape).join('<br>')}</td><td>${escape(c.reviewed_by || 'Awaiting review')}</td></tr>`).join('') : '<tr><td colspan="4">No attribution claims in this snapshot. Missing labels do not mean low risk.</td></tr>';
    byId('labels-scope').textContent=data.source_validity?.current_risk_usable === false ? 'Historical attribution snapshot: rerun the investigation before using current labels or risk.' : 'Only reviewed claims from active sources can support risk. A label is an attribution claim, not proof of laundering.';
    const exposure=data.exposure_paths || [], scope=data.exposure_coverage || {};
    byId('exposure').innerHTML=exposure.length ? exposure.map(p=>`<tr><td>${escape(p.direction)}<br>${escape(p.seed_claim?.entity_name)}<br>${escape(p.seed_address)}</td><td>${escape(p.hop_count)} transaction(s)<details><summary>Transaction and outpoint evidence</summary>${(p.hops || []).map(h=>`<p>${escape(h.tx_hash)}<br>Inputs: ${(h.inputs || []).map(i=>`${escape(i.prev_tx_hash)}:${escape(i.prev_output_index)}`).join(', ')}<br>Output: ${escape(h.tx_hash)}:${escape(h.output?.index)}</p>`).join('')}</details></td><td>${btc(p.allocation?.minimum_sats)} – ${btc(p.allocation?.maximum_sats)}<br>Bounds per path; do not sum.</td><td>${data.source_validity?.current_risk_usable === false ? 'Historical' : p.service_mediated ? 'Service boundary: context only' : p.score_eligible ? 'Positive lower bound' : 'Possible only: not scored'}</td></tr>`).join('') : '<tr><td colspan="4">No exposure paths returned. This does not prove absence of exposure.</td></tr>';
    byId('exposure-scope').textContent=`${scope.status || 'Exposure not supplied'}. ${scope.truncated ? 'Query limits reached; partial candidates are not scored. ' : ''}Only exact spent outpoints connect hops. Values are conservative bounds, not identified satoshis.${scope.run_id ? ' Run: '+scope.run_id : ''}`;
    const clusters=data.clusters || [], clusterScope=data.cluster_coverage || {};
    byId('cluster-scope').textContent=`${data.source_validity?.current_risk_usable === false ? 'Historical snapshot. ' : ''}${clusterScope.status || 'Clustering not supplied'}. ${clusterScope.transactions_examined ?? 0} transactions examined; ${clusterScope.suppressed_transactions?.length ?? 0} suppressed. ${clusterScope.persisted ? 'Versioned leads stored.' : 'No complete publication; refine the activity scope if limits were reached.'} No transitive address merging. Confidence describes a heuristic match, not ownership or crime probability.`;
    const clusterReview={pending:'Awaiting review',approved:'Accepted lead only',rejected:'Rejected lead',revoked:'Review revoked'};
    byId('cluster-leads').innerHTML=clusters.length ? clusters.map(c=>`<tr><td>${escape(c.heuristic)}<br>${escape(c.cluster_id)}<br>Version ${escape(c.cluster_version)}<br>Confidence ${escape(c.confidence)}</td><td>${(c.members || []).map(escape).join('<br>')}<br>Candidate members only</td><td><details><summary>Transaction and input evidence</summary><p>Block ${escape(c.evidence?.block_number)} · ${escape(c.evidence?.tx_hash)}<br>${escape(c.evidence?.block_hash)}</p><p>${(c.evidence?.inputs || []).map(i=>`${escape(i.prev_tx_hash)}:${escape(i.prev_output_index)} (${escape(i.value_sats)} sats)`).join('<br>')}</p><p>${(c.evidence_refs || []).map(escape).join('<br>')}</p><p>${escape(c.evidence?.match?.matching_address_family || '')}${c.evidence?.match?.candidate_output_index == null ? '' : ' · Candidate output '+escape(c.evidence.match.candidate_output_index)}</p></details></td><td>${escape(clusterReview[c.review_status] || c.review_status)}<br>${escape(c.review?.reviewed_by || '')}<br>${escape(c.review?.reason || '')}<br>${(c.review?.evidence_refs || []).map(escape).join('<br>')}</td></tr>`).join('') : '<tr><td colspan="4">No candidates returned in this scope. This does not prove separate ownership or absence of other links.</td></tr>';
    byId('raw').textContent=JSON.stringify(data,null,2);
    const assessment=data.risk_snapshot;
    byId('risk-reference').textContent=assessment?.assessment_id ? `Assessment reference: ${assessment.assessment_id} · ${data.source_validity?.current_risk_usable === false || assessment.current === false ? 'Historical; rerun before using current risk' : 'Verified evidence snapshot'}` : data.risk_persistence?.status === 'waiting_for_blocks' ? 'Assessment is waiting for committed blocks.' : '';
    if (data.paths) byId('path-result').textContent=data.paths.length ? `Found ${data.paths[0].hop_count} transaction hop(s). Fund continuity is not established.` : 'No path in the searched evidence window.';
    else byId('path-result').textContent='';
    if (window.AmlSnapshots) window.AmlSnapshots.show(data);
  }
  async function run(pathMode) {
    const version=++sequence;
    const button=byId(pathMode?'find-path':'search');
    button.disabled=true; byId('message').textContent='';
    try {
      const source=address.value.trim();
      if (!source) throw new Error('Enter a Bitcoin address.');
      const params=new URLSearchParams({limit:'40'});
      if(!pathMode) {
        for(const [id,key] of [['from-block','from_block'],['to-block','to_block']]) {const value=byId(id).value.trim();if(value){if(!/^\d+$/.test(value))throw new Error('Block filters must be unsigned integers.');params.set(key,value);}}
        for(const [id,key] of [['from-time','from_unix_ms'],['to-time','to_unix_ms']]) {const value=byId(id).value;if(value){const ms=Date.parse(value+'Z');if(!Number.isSafeInteger(ms) || ms<0)throw new Error('Enter a valid UTC time.');params.set(key,String(ms));}}
        params.set('direction',byId('activity-direction').value);
      }
      const suffix=pathMode?`paths/${encodeURIComponent(byId('target').value.trim())}?max_hops=3&max_nodes=200`:`investigation?${params}`;
      const data=await query(`/api/bitcoin/wallet/${encodeURIComponent(source)}/${suffix}`);
      if (version===sequence) show(data);
    } catch(error) { if(version===sequence) byId('message').textContent=error.message; }
    finally { button.disabled=false; }
  }
  byId('wallet-form').addEventListener('submit',event=>{event.preventDefault();run(false);});
  byId('path-form').addEventListener('submit',event=>{event.preventDefault();run(true);});
  if(window.AmlSnapshots) window.AmlSnapshots.onReopen(show);
  const initial=new URLSearchParams(location.search).get('address');
  if(initial){address.value=initial;run(false);}
})();
