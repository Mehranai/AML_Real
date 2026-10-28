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
    if (data.address) address.value=data.address;
    draw(data.graph || {});
    const holdings=data.holdings || {};
    byId('balance').textContent=holdings.balance_decimal ?? '—';
    byId('balance-scope').textContent=holdings.history_complete ? 'Confirmed snapshot; mempool excluded' : 'Partial history: observed UTXOs only';
    byId('utxos').textContent=holdings.utxo_count ?? '—';
    byId('transactions').textContent=data.fingerprint?.transaction_count ?? '—';
    byId('history').textContent=holdings.history_complete ? 'From genesis' : 'Partial / empty';
    byId('height').textContent=holdings.as_of_block == null ? 'No committed block' : `As of block ${holdings.as_of_block}`;
    const claims=data.intelligence?.claims || data.entities || [];
    byId('labels').innerHTML=claims.length ? claims.map(c=>`<tr><td>${escape(c.entity_name)} / ${escape(c.entity_type)}</td><td>${escape(c.review_status)} / ${data.source_validity?.current_risk_usable === false ? 'historical' : c.active === true ? 'active' : 'inactive'}</td><td>${escape(c.source_id)} · ${escape(c.source_reference)}<br>${(c.evidence_refs || []).map(escape).join('<br>')}</td><td>${escape(c.reviewed_by || 'Awaiting review')}</td></tr>`).join('') : '<tr><td colspan="4">No attribution claims in this snapshot. Missing labels do not mean low risk.</td></tr>';
    byId('labels-scope').textContent=data.source_validity?.current_risk_usable === false ? 'Historical attribution snapshot: rerun the investigation before using current labels or risk.' : 'Only reviewed claims from active sources can support risk. A label is an attribution claim, not proof of laundering.';
    byId('raw').textContent=JSON.stringify(data,null,2);
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
      const suffix=pathMode?`paths/${encodeURIComponent(byId('target').value.trim())}?max_hops=3&max_nodes=200`:'investigation?limit=40';
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
