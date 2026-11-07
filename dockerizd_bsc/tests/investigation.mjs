import assert from 'node:assert/strict';
import {execFile,spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {createServer} from 'node:http';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
const run=promisify(execFile);
const directory=fileURLToPath(new URL('../',import.meta.url));
const root=fileURLToPath(new URL('../../',import.meta.url));
const executable=name=>directory+'target/debug/'+name+(process.platform==='win32'?'.exe':'');
const name='bsc-investigation-test-'+process.pid;
const dockerApi=process.argv.includes('--docker-api');
const apiName=name+'-api';
const key='bsc-test-service-key-only-000000000000000000000000';
const password='bsc-isolated-test-password';
const a=n=>'0x'+n.toString(16).padStart(40,'0');
const hash=n=>'0x'+n.toString(16).padStart(64,'0');
const native='eip155:56/native:bnb', token='eip155:56/erc20:'+a(900);
const hex=n=>'0x'+BigInt(n).toString(16);
const word=n=>'0x'+BigInt(n).toString(16).padStart(64,'0');
let api,apiLog='',rpc,created=false,networkCreated=false,apiCreated=false;
let rpcChain='0x38';
const docker=(...args)=>run('docker',args,{cwd:root,timeout:240000});
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function port() {const server=createServer();server.listen(0,'0.0.0.0');await once(server,'listening');const port=server.address().port;await new Promise(r=>server.close(r));return port;}
try {
  await run('cargo',['build','--locked','--offline','--bins'],{cwd:directory,timeout:300000});
  if(dockerApi){await docker('network','create',name);networkCreated=true;}
  await docker('run','--rm','-d','--name',name,...(dockerApi?['--network',name]:[]),'-p','127.0.0.1::8123',
    '-e','CLICKHOUSE_USER=bsc_admin','-e','CLICKHOUSE_PASSWORD='+password,
    '-e','CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT=1',process.env.CLICKHOUSE_IMAGE || 'clickhouse/clickhouse-server:23.8.16.16');
  created=true;
  const chPort=(await docker('port',name,'8123')).stdout.trim().split(':').at(-1);
  const ch='http://127.0.0.1:'+chPort;
  async function sql(query,body) {
    const response=await fetch(ch+'/?'+new URLSearchParams(body===undefined?{}:{query}),{
      method:'POST',headers:{'X-ClickHouse-User':'bsc_admin','X-ClickHouse-Key':password},body:body??query});
    const text=await response.text();assert.ok(response.ok,text);return text;
  }
  for(let i=0;i<60;i++){try{await sql('SELECT 1');break;}catch{await pause(500);}}
  rpc=createServer(async(req,res)=>{
    let text='';for await(const chunk of req)text+=chunk;
    const {id,method,params}=JSON.parse(text);let result;
    if(method==='eth_chainId')result=rpcChain;
    else if(method==='eth_getBlockByNumber'){const height=params[0]==='finalized'?100:Number(BigInt(params[0]));result={number:hex(height),hash:hash(height)};}
    else if(method==='eth_getBalance')result=hex(1234567890123456789n);
    else if(method==='eth_call') {
      const data=params[0].data;
      if(data==='0x313ce567')result=word(18);
      else if(data==='0x06fdde03'||data==='0x95d89b41')result='0x'+Buffer.from('BSC test token').toString('hex').padEnd(64,'0');
      else if(data.startsWith('0x70a08231'))result=word((1n<<200n)+7n);
      else if(data.startsWith('0x6352211e'))result='0x'+a(3).slice(2).padStart(64,'0');
      else if(data.startsWith('0x00fdd58e'))result=word(42);
      else result=null;
    }
    res.setHeader('content-type','application/json');res.end(JSON.stringify({jsonrpc:'2.0',id,result}));
  });
  rpc.listen(0,'0.0.0.0');await once(rpc,'listening');
  const apiPort=await port();
  const env={...process.env,BSC_CLICKHOUSE_URL:ch,BSC_CLICKHOUSE_USER:'bsc_admin',BSC_CLICKHOUSE_PASSWORD:password,
    BSC_CLICKHOUSE_DATABASE:'bsc_aml',BSC_API_ADDR:'0.0.0.0:'+apiPort,AML_SERVICE_KEY:key,
    BSC_MODE:'development',BSC_RPC_URL:'http://127.0.0.1:'+rpc.address().port,BSC_TRACE_MODE:'auto'};
  await run(executable('bsc_schema'),[],{cwd:directory,env,timeout:180000});
  await run(executable('bsc_schema'),['--check'],{cwd:directory,env,timeout:180000});
  // Synthetic UI/path fixtures. Live RPC ingestion and crash/reorg validation run
  // in Rust; this fixture uses the same aml_* schema and publication boundary.
  const insert=(table,rows)=>rows.length?sql('INSERT INTO bsc_aml.'+table+' FORMAT JSONEachRow',rows.map(JSON.stringify).join('\n')):Promise.resolve();
  const timestamp=Date.now()-600000;
  const edge=(id,from,to,block,asset=native)=>({relationship_id:id,block_number:block,
    from_address:from,to_address:to,asset_id:asset,token_id:'',amount:'1000000000000000000',transfer_type:asset===native?'native_external':'erc20'});
  const edges=Array.from({length:10},(_,i)=>edge('edge-'+i,a(i+1),a(i+2),i+1));
  edges.push(edge('different-asset',a(2),a(12),3,token),edge('wrong-time',a(3),a(13),1));
  edges.push(edge('token',a(2),a(3),4,token));
  const nft=edge('nft',a(2),a(3),5,'eip155:56/erc721:'+a(901)+'/7');nft.token_id='7';nft.amount='1';nft.transfer_type='erc721';edges.push(nft);
  const multi=edge('erc1155',a(2),a(3),6,'eip155:56/erc1155:'+a(902)+'/9');multi.token_id='9';multi.amount='42';multi.transfer_type='erc1155';edges.push(multi);
  const facts={aml_transactions:[],aml_address_relationships:[],aml_evm_logs:[],aml_raw_transactions:[],aml_metadata_discoveries:[],aml_semantic_aml_events:[]};
  const blocks=[];
  const topic=address=>word(BigInt(address));
  const transferTopic='0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef';
  for(let block=1;block<=11;block++){
    const head={network_id:'eip155:56',block_number:block,block_hash:hash(block),block_timestamp_unix_ms:timestamp+block*1000,
      revision:1,commit_id:'browser-fixture-'+block,created_at_unix_ms:Date.now()};
    const scoped=edges.filter(e=>e.block_number===block);let logIndex=0;
    for(const [index,e] of scoped.entries()){
      const txHash=hash(1000+block*10+index),isNative=e.asset_id===native;
      const transaction={hash:txHash,blockHash:hash(block),blockNumber:hex(block),transactionIndex:hex(index),
        from:e.from_address,to:isNative?e.to_address:e.asset_id.split(':').at(-1).split('/')[0],
        value:hex(isNative?e.amount:0),input:'0x',nonce:hex(index),gas:'0x5208',gasPrice:'0x3b9aca00'};
      const logs=[];
      if(!isNative){
        const contract=transaction.to;
        let topics=[transferTopic,topic(e.from_address),topic(e.to_address)],data=word(BigInt(e.amount));
        if(e.transfer_type==='erc721'){topics.push(word(BigInt(e.token_id)));data='0x';}
        if(e.transfer_type==='erc1155'){topics=['0xc3d58168c5ae7397731d063d5bbf3d657854427343f4c083240f7aacaa2d0f62',topic(e.from_address),topic(e.from_address),topic(e.to_address)];data=word(BigInt(e.token_id))+word(BigInt(e.amount)).slice(2);}
        logs.push({address:contract,topics,data,blockNumber:hex(block),blockHash:hash(block),transactionHash:txHash,transactionIndex:hex(index),logIndex:hex(logIndex),removed:false});
        facts.aml_evm_logs.push({...head,tx_hash:txHash,transaction_index:index,log_index:logIndex,address:contract,topics,data:data.slice(2),removed:0,raw_evidence_json:JSON.stringify(logs[0])});
      }
      const receipt={transactionHash:txHash,blockHash:hash(block),blockNumber:hex(block),transactionIndex:hex(index),from:e.from_address,to:transaction.to,status:'0x1',logs};
      facts.aml_transactions.push({...head,tx_hash:txHash,transaction_index:index,from_address:e.from_address,to_address:transaction.to,
        amount_raw:isNative?e.amount:'0',fee_raw:'21000000000000',asset_id:'native:BNB',decimals:18,status:'success',coinbase:0,input_count:null,output_count:null,
        raw_evidence_json:JSON.stringify({input_data:'0x',gas_limit:21000,gas_used:21000,transaction_type:0,nonce:index})});
      facts.aml_raw_transactions.push({...head,tx_hash:txHash,transaction_index:index,source_id:'browser_fixture',payload_json:JSON.stringify(transaction),receipt_json:JSON.stringify(receipt),evidence_refs:['tx:'+txHash]});
      const stored={...head,relationship_id:e.relationship_id,tx_hash:txHash,transaction_index:index,event_index:isNative?0:logIndex,event_sub_index:0,trace_address:[],
        from_subject_id:e.from_address,from_subject_type:'address',to_subject_id:e.to_address,to_subject_type:'address',from_address:e.from_address,to_address:e.to_address,
        asset_id:isNative?'native:BNB':e.asset_id.slice('eip155:56/'.length),token_id:e.token_id||null,amount_raw:e.amount,decimals:isNative?18:null,
        transfer_type:e.transfer_type,attribution_method:'observed_transfer',confidence:1,prev_tx_hash:null,prev_output_index:null,output_index:null,evidence_refs:['tx:'+txHash],raw_evidence_json:'{"fixture":true}'};
      facts.aml_address_relationships.push(stored);
      if(e.relationship_id==='token'){
        facts.aml_metadata_discoveries.push({...head,discovery_id:'token-discovery',tx_hash:txHash,transaction_index:index,asset_id:'erc20:'+a(900),standard_hint:'erc20',evidence_id:'browser-log-'+block+'-'+logIndex,source_id:'browser_fixture',evidence_refs:['tx:'+txHash],raw_evidence_json:'{"fixture":true}'});
        const event={protocol_contract:a(900),counterparty_address:a(2),asset_in:'',asset_out:'',remote_receiver:a(40),bridge_message_id:hash(40),bridge_direction:'outgoing',amount_in:'',amount_out:''};
        facts.aml_semantic_aml_events.push({...head,event_id:'bridge-fixture',tx_hash:txHash,subject_id:a(3),event_type:'bridge_transfer',protocol:'fixture-bridge',detector:'fixture',detector_version:'1',confidence:1,
          asset_in:null,asset_out:null,amount_in:null,amount_out:null,remote_network_id:'eip155:1',evidence_refs:['tx:'+txHash],evidence_json:'{"fixture":true}',event_index:logIndex,event_sub_index:0,raw_evidence_json:JSON.stringify(event)});
      }
      if(!isNative)logIndex++;
    }
    const counts=Object.fromEntries(Object.entries(facts).map(([table,rows])=>[table,rows.filter(r=>r.block_number===block).length]));
    blocks.push({...head,parent_hash:hash(block-1),canonical:1,ingestion_status:'complete',transaction_count:scoped.length,
      receipt_coverage:1,trace_coverage:0,prevout_coverage:null,source_id:'browser_fixture',expected_counts_json:JSON.stringify(counts),raw_evidence_json:'{"fixture":true}'});
  }
  for(const [table,rows] of Object.entries(facts))await insert(table,rows);
  await insert('aml_ingested_blocks',blocks);
  await insert('aml_sync_state',[{network_id:'eip155:56',revision:1,initialized:1,start_block:1,last_block_number:11,last_block_hash:hash(11),observed_rpc_height:100,published_at_unix_ms:Date.now()}]);
  const partial=facts.aml_address_relationships.find(e=>e.from_address===a(3));
  await insert('aml_address_relationships',[{...partial,relationship_id:'uncommitted',to_subject_id:a(99),to_address:a(99),commit_id:'partial-browser-fixture',revision:2}]);

  await run(executable('bsc_token_metadata_worker'),[],{cwd:directory,env,timeout:60000});
  if(dockerApi){
    const runtimeEnv={BSC_CLICKHOUSE_URL:'http://'+name+':8123',BSC_CLICKHOUSE_USER:'bsc_admin',
      BSC_CLICKHOUSE_PASSWORD:password,BSC_CLICKHOUSE_DATABASE:'bsc_aml',BSC_API_ADDR:'0.0.0.0:6001',
      AML_SERVICE_KEY:key,BSC_MODE:'development',BSC_TRACE_MODE:'auto',
      BSC_RPC_URL:'http://host.docker.internal:'+rpc.address().port};
    await docker('run','-d','--name',apiName,'--network',name,'--add-host','host.docker.internal:host-gateway',
      '--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true',
      '-p',apiPort+':6001',...Object.entries(runtimeEnv).flatMap(([k,v])=>['-e',k+'='+v]),
      'bsc-aml-service:local','bsc_api');
    apiCreated=true;
  }else{
    api=spawn(executable('bsc_api'),[],{cwd:directory,env,windowsHide:true});
    api.stdout.on('data',chunk=>{apiLog+=chunk;});api.stderr.on('data',chunk=>{apiLog+=chunk;});
  }
  const base='http://127.0.0.1:'+apiPort;
  for(let i=0;i<60;i++){try{if((await fetch(base+'/ready')).ok)break;}catch{}await pause(500);}
  if(dockerApi){
    await docker('exec',apiName,'bsc_api','--healthcheck');
    assert.equal((await docker('exec',apiName,'id','-u')).stdout.trim(),'10001');
    assert.equal(await (await fetch(base+'/')).text(),await readFile(new URL('../web/index.html',import.meta.url),'utf8'));
  }
  async function query(path,expected=200,auth=true){
    const response=await fetch(base+path,{headers:auth?{'X-AML-Service-Key':key}:{}});
    const body=await response.json();assert.equal(response.status,expected,JSON.stringify(body)+'\n'+apiLog);return body;
  }
  await query('/status',401,false);await query('/api/bsc/wallet/not-an-address/investigation',400);
  await query('/api/bsc/wallet/'+a(3)+'/paths/'+a(11)+'?max_hops=11',400);
  let wallet=await query('/api/bsc/wallet/'+a(3)+'/investigation');
  assert.equal(wallet.network_id,'eip155:56');assert.equal(wallet.fingerprint.transfer_count,6);
  assert.equal(wallet.fingerprint.bridge_events,1);
  assert.equal(wallet.semantic_events[0].remote_receiver,a(40));
  assert.equal(wallet.semantic_events[0].bridge_message_id,hash(40));
  assert.equal(wallet.graph.edges.some(e=>e.id==='uncommitted'),false);
  assert.equal(wallet.asset_flows.find(v=>v.asset_id===token).metadata.symbol,'BSC test token');
  assert.equal(wallet.risk_engine.enabled,false);assert.equal(wallet.exposure_paths.length,0);
  const path=await query('/api/bsc/wallet/'+a(1)+'/paths/'+a(11)+'?max_hops=10');
  assert.equal(path.paths[0].hop_count,10);assert.equal(path.edges.length,10);
  assert.equal((await query('/api/bsc/wallet/'+a(1)+'/paths/'+a(12))).paths.length,0);
  assert.equal((await query('/api/bsc/wallet/'+a(1)+'/paths/'+a(13))).paths.length,0);
  assert.equal((await query('/api/bsc/wallet/'+a(11)+'/paths/'+a(1)+'?direction=incoming')).paths[0].hop_count,10);
  const holdings=await query('/api/bsc/wallet/'+a(3)+'/holdings');
  assert.equal(holdings.assets.find(v=>v.asset_id===native).amount,'1234567890123456789');
  assert.equal(holdings.assets.find(v=>v.asset_id===token).amount,((1n<<200n)+7n).toString());
  assert.equal(holdings.assets.find(v=>v.asset_id===multi.asset_id).amount,'42');
  assert.equal(holdings.assets.find(v=>v.asset_id===nft.asset_id).amount,'1');
  rpcChain='0x1';await query('/api/bsc/wallet/'+a(3)+'/holdings',503);rpcChain='0x38';
  const output=fileURLToPath(new URL('../../test-results/',import.meta.url));await mkdir(output,{recursive:true});
  const claim={network_id:'eip155:56',claim_id:'fixture-seed',address:a(1),claim_kind:'label',entity_id:'fixture',
    entity_name:'Test seed only',entity_type:'scam',address_role:'seed',confidence:1,risk_level:100,is_exposure_seed:true,
    seed_category:'scam',source_id:'test',source_reference:'fixture:seed',evidence_refs:['fixture:tx'],
    review_status:'pending',reviewed_by:'',review_note:'isolated test fixture'};
  async function importClaim(c){const file=output+'bsc-claims.jsonl';await writeFile(file,JSON.stringify(c)+'\n');await run(executable('bsc_intelligence'),['import-labels',file],{cwd:directory,env,timeout:30000});}
  await importClaim(claim);
  assert.equal((await query('/api/bsc/wallet/'+a(3)+'/investigation')).exposure_paths.length,0);
  await importClaim({...claim,review_status:'approved',reviewed_by:'test-analyst'});
  wallet=await query('/api/bsc/wallet/'+a(3)+'/investigation');
  assert.ok(wallet.exposure_paths.some(p=>p.seed_address===a(1)&&p.hop_count===2&&p.direction==='RECEIVED_FROM_SEED'));
  await importClaim({...claim,claim_id:'service',address:a(2),entity_id:'exchange',entity_type:'exchange',is_exposure_seed:false,risk_level:0,review_status:'approved',reviewed_by:'test-analyst'});
  assert.equal((await query('/api/bsc/wallet/'+a(3)+'/investigation')).exposure_paths.length,0);
  await importClaim({...claim,claim_id:'service',address:a(2),entity_id:'exchange',entity_type:'exchange',is_exposure_seed:false,risk_level:0,review_status:'rejected',reviewed_by:'test-analyst'});
  const filtered=await query('/api/bsc/wallet/'+a(3)+'/investigation?limit=1');
  assert.equal(filtered.graph.edges.length,1);assert.equal(filtered.graph.truncated,true);
  console.log('PASS real ClickHouse: canonical gates, fingerprint, metadata worker, UInt256 holdings, 10-hop paths, chronology/asset isolation, reviewed labels and service boundaries');
  if(process.argv.includes('--central')) {
    const {verifyCentral}=await import('./central-browser.mjs');
    await verifyCentral({root,apiPort,key,address:a(3),source:a(1),target:a(11)});
  }
  if(process.argv.includes('--ingestion')) {
    await run('cargo',['test','--locked','--offline','--lib','--','--ignored','--test-threads=1'],{cwd:directory,timeout:1200000,
      env:{...env,BSC_TEST_CLICKHOUSE_URL:ch,BSC_TEST_CLICKHOUSE_USER:'bsc_admin',BSC_TEST_CLICKHOUSE_PASSWORD:password}});
    console.log('PASS existing schema/replay/crash/reorg ClickHouse tests');
  }
  if(dockerApi){
    await docker('stop','--time','5',apiName);
    assert.equal((await docker('inspect','--format','{{.State.ExitCode}}',apiName)).stdout.trim(),'0');
    console.log('PASS Linux API image: non-root/read-only runtime, healthcheck, embedded UI matches source and clean SIGTERM exit');
  }
} catch(error){
  if(apiCreated)apiLog+=(await docker('logs',apiName).catch(()=>({stdout:'',stderr:''}))).stderr;
  console.error(apiLog);throw error;
}
finally {
  if(api){api.kill();await Promise.race([once(api,'exit'),pause(5000)]);}
  if(apiCreated)await docker('rm','-f',apiName).catch(()=>{});
  if(rpc)await new Promise(r=>rpc.close(r));
  if(created)await docker('rm','-f',name).catch(()=>{});
  if(networkCreated)await docker('network','rm',name).catch(()=>{});
}
