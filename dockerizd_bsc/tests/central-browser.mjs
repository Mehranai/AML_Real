import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createServer} from 'node:http';
import {once} from 'node:events';
import {mkdir} from 'node:fs/promises';
import {chromium} from 'playwright';
const run=promisify(execFile);
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
export async function verifyCentral({root,apiPort,key,address,source,target}) {
  const reservation=createServer();reservation.listen(0);await once(reservation,'listening');
  const port=reservation.address().port;await new Promise(r=>reservation.close(r));
  const env={...process.env,AML_PORT:String(port),AML_BIND_ADDRESS:'127.0.0.1',AML_SERVICE_KEY:key,
    AML_TRON_UPSTREAM:'http://127.0.0.1:9',AML_ETHEREUM_UPSTREAM:'http://127.0.0.1:9',
    AML_BSC_UPSTREAM:'http://host.docker.internal:'+apiPort,NEO4J_PASSWORD:'bsc-browser-test-password',
    AML_NEO4J_HTTP_PORT:'0',AML_NEO4J_BOLT_PORT:'0',AML_BASIC_AUTH_REALM:'off'};
  const project='aml-bsc-browser-'+process.pid,base='http://127.0.0.1:'+port;
  const compose=(...args)=>run('docker',['compose','-p',project,'-f','compose.yaml',...args],{cwd:root,env,timeout:240000});
  let browser;
  try {
    await compose('up','-d','--no-build','gateway');
    try{browser=await chromium.launch({headless:true});}
    catch{browser=await chromium.launch({headless:true,channel:'msedge'});}
    const page=await browser.newPage({viewport:{width:1440,height:1000}});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.goto(base);
    await page.waitForFunction(()=>document.getElementById('bsc-api')?.textContent==='Ready');
    assert.equal(await page.locator('#ethereum-api').textContent(),'Unavailable');
    await page.locator('#network').selectOption('bsc');
    await page.locator('#address').fill(address);
    await page.getByRole('button',{name:'Investigate',exact:true}).click();
    await page.waitForFunction(()=>document.getElementById('snapshot-status')?.textContent.includes('Temporary'));
    assert.ok(page.url().includes('/networks/bsc/'));
    assert.match(await page.locator('#evidence-content').innerText(),/Daily activity|Observed asset flows/i);
    assert.match(await page.locator('#snapshot-status').innerText(),/eip155:56/);
    assert.equal(await page.locator('#graph-canvas').evaluate(canvas=>{
      const d=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data;let count=0;
      for(let i=0;i<d.length;i+=4)if(d[i]<180&&d[i+1]<180&&d[i+2]<180)count++;return count>50;
    }),true);
    await page.locator('#load-holdings').click();
    await page.waitForFunction(()=>document.getElementById('holdings-result')?.textContent.includes('Finalized block'));
    await mkdir(root+'/test-results',{recursive:true});
    for(const width of [390,768,1440]) {
      await page.setViewportSize({width,height:900});await pause(200);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
      assert.equal(await page.locator('#graph-canvas').evaluate(canvas=>{
        const {width,height}=canvas,d=canvas.getContext('2d').getImageData(0,0,width,height).data;
        let count=0,sumX=0,minX=width,maxX=0;
        for(let i=0;i<d.length;i+=4){
          if((d[i]===22&&d[i+1]===115&&d[i+2]===74)||(d[i]===102&&d[i+1]===115&&d[i+2]===108)){
            const x=(i/4)%width;count++;sumX+=x;minX=Math.min(minX,x);maxX=Math.max(maxX,x);
          }
        }
        return count>50&&minX>4&&maxX<width-5&&sumX/count>width*.15&&sumX/count<width*.85;
      }),true,'wallet nodes remain visible and centered after resize');
      await page.screenshot({path:root+'/test-results/bsc-wallet-'+width+'.png',fullPage:true});
    }
    await page.locator('#snapshot-export').click();
    await page.waitForFunction(()=>document.getElementById('snapshot-status')?.textContent.includes('Saved permanently'));
    const savedId=await page.locator('#snapshot-saved option').nth(1).getAttribute('value');
    const saved=await page.evaluate(async id=>await (await fetch('/api/investigations/'+id)).json(),savedId);
    assert.equal(saved.investigation.network_id,'eip155:56');
    assert.equal(saved.risk_engine.probability_claimed,false);assert.ok(saved.risk_engine.risk_score>0);
    const dbPort=(await compose('port','neo4j','7474')).stdout.trim().split(':').at(-1);
    const result=await (await fetch('http://127.0.0.1:'+dbPort+'/db/neo4j/query/v2',{
      method:'POST',headers:{'Content-Type':'application/json',Authorization:'Basic '+Buffer.from('neo4j:'+env.NEO4J_PASSWORD).toString('base64')},
      body:JSON.stringify({statement:'MATCH (i:Investigation {id:$id})-[:CONTAINS]->(w) RETURN i.network_id, count(w), collect(DISTINCT w.network_id)',parameters:{id:savedId}})
    })).json();
    assert.ok(!result.errors?.length,JSON.stringify(result.errors));
    assert.equal(result.data.values[0][0],'eip155:56');assert.ok(result.data.values[0][1]>1);
    assert.deepEqual(result.data.values[0][2],['eip155:56']);
    await page.locator('#path-tab').click();
    await page.locator('#source-address').fill(source);await page.locator('#target-address').fill(target);
    await page.locator('#max-hops').fill('10');
    await page.locator('#path-form button').click();
    await page.waitForFunction(()=>document.getElementById('evidence-content')?.textContent.includes('10 hops'));
    assert.equal(await page.locator('#holdings-result').count(),0);
    await mkdir(root+'/test-results',{recursive:true});
    for(const width of [390,768,1440]) {
      await page.setViewportSize({width,height:900});await pause(300);
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
      await page.screenshot({path:root+'/test-results/bsc-'+width+'.png',fullPage:true});
    }
    await page.locator('#snapshot-saved').selectOption(savedId);
    await page.waitForFunction(()=>document.getElementById('load-holdings')!==null);
    const reopened=await page.evaluate(async id=>await (await fetch('/api/investigations/'+id)).json(),savedId);
    assert.equal(saved.investigation.snapshot_hash,reopened.investigation.snapshot_hash);
    assert.deepEqual(errors,[]);
    console.log('PASS BSC browser + real ClickHouse + central Neo4j: risk, temporary/saved snapshots, Export/reopen, 10-hop graph, mobile/tablet/desktop');
  } catch(error) {
    const logs=await compose('logs','--tail','30','analytical-node','gateway').catch(()=>({stdout:''}));
    console.error(logs.stdout);throw error;
  } finally {
    if(browser)await browser.close();
    await compose('down','--volumes','--remove-orphans');
  }
}
