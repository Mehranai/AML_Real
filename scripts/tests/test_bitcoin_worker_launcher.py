"""Execute the actual VM helper against an isolated Docker command recorder."""
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
BASH=os.environ.get('AML_TEST_BASH') or (shutil.which('bash') if os.name!='nt' else None)

@unittest.skipUnless(BASH,'Set AML_TEST_BASH to run the real Bash launcher checks on Windows')
class BitcoinWorkerLauncher(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='bitcoin launcher ')
        self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        (self.root/'scripts').mkdir();(self.root/'dockerizd_bitcoin').mkdir();(self.root/'bin').mkdir()
        shutil.copy2(ROOT/'scripts/vm.sh',self.root/'scripts/vm.sh')
        (self.root/'dockerizd_bitcoin/.env').write_text('BITCOIN_CLICKHOUSE_PASSWORD=test-only\nAML_SERVICE_KEY=test-only\n')
        self.log=self.root/'commands.jsonl'
        hook=self.root/'docker_hook.py'
        hook.write_text('''import json,os,sys
with open(os.environ['AML_DOCKER_LOG'],'a',encoding='utf-8') as f:f.write(json.dumps(sys.argv[1:])+'\\n')
a=sys.argv[1:]
if 'ps' in a and '--quiet' in a:print('deadbeef' if a[-1]=='bitcoin-analytics' else 'abc123')
elif a[0]=='inspect':print('running unhealthy' if a[-1]=='deadbeef' and os.environ.get('AML_BAD_WORKER')=='1' else 'running healthy')
''')
        docker=self.root/'bin/docker'
        docker.write_text('#!/usr/bin/env bash\nexec '+shlex.quote(Path(sys.executable).as_posix())+' '+shlex.quote(hook.as_posix())+' "$@"\n',newline='\n')
        docker.chmod(0o755)
        self.env={**os.environ,'PATH':str(self.root/'bin')+os.pathsep+os.environ['PATH'],'AML_DOCKER_LOG':str(self.log)}

    def invoke(self,*args,ok=True):
        p=subprocess.run([BASH,str(self.root/'scripts/vm.sh'),'bitcoin',*args],env=self.env,capture_output=True,text=True,timeout=30)
        if ok:self.assertEqual(p.returncode,0,p.stderr)
        calls=[json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return p,calls

    def test_full_start_includes_both_background_workers(self):
        _,calls=self.invoke('up','--pull-never')
        self.assertEqual(len(calls),1)
        self.assertIn('bitcoin-analytics',calls[0]);self.assertIn('bitcoin-ingestion',calls[0])
        self.assertNotIn('--build',calls[0])

    def test_api_only_does_not_launch_workers(self):
        _,calls=self.invoke('up','--api-only')
        self.assertIn('bitcoin-api',calls[0]);self.assertNotIn('bitcoin-analytics',calls[0]);self.assertNotIn('bitcoin-ingestion',calls[0])

    def test_pause_background_keeps_api_and_database(self):
        _,calls=self.invoke('pause-ingestion')
        self.assertIn('stop',calls[0]);self.assertIn('bitcoin-analytics',calls[0]);self.assertIn('bitcoin-ingestion',calls[0])
        for value in ['bitcoin-api','clickhouse','down','--volumes']:self.assertNotIn(value,calls[0])

    def test_resume_uses_existing_database_and_images(self):
        _,calls=self.invoke('resume')
        self.assertEqual(len(calls),2)
        self.assertIn('clickhouse',calls[0]);self.assertIn('bitcoin-analytics',calls[1]);self.assertIn('--no-deps',calls[1]);self.assertIn('--no-build',calls[1])
        self.assertNotIn('clickhouse',calls[1]);self.assertNotIn('down',calls[1])

    def test_runtime_check_rejects_unhealthy_analytics_worker(self):
        self.env['AML_BAD_WORKER']='1'
        p,calls=self.invoke('check-runtime',ok=False)
        self.assertNotEqual(p.returncode,0);self.assertIn('bitcoin-analytics',p.stderr)
        self.assertTrue(any('bitcoin-analytics' in c for c in calls))

if __name__=='__main__':unittest.main()
