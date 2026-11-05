"""Real loopback TLS and process tests. No physical device is accessed."""
import base64
import contextlib
import http.server
import json
import os
from pathlib import Path
import shutil
import ssl
import subprocess
import threading
import time
import urllib.request

ROOT=Path(__file__).resolve().parents[1]
BINARY=ROOT/'target/release/nanokvm-console'

@contextlib.contextmanager
def fixture(tmp_path, rotate=False, drop_login=False, delay_login=0):
    cert,key=tmp_path/'cert.pem',tmp_path/'key.pem'
    subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(key),'-out',str(cert),'-days','1','-subj','/CN=fixture','-addext','basicConstraints=critical,CA:TRUE'],check=True,capture_output=True)
    calls=[]
    leaf_ctx=None
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_GET(self):self.reply()
        def do_POST(self):self.reply()
        def reply(self):
            calls.append(('leaf:' if getattr(self.connection,'context',None) is leaf_ctx else '')+self.path)
            if drop_login and self.path=='/api/auth/login':self.close_connection=True;return
            if delay_login and self.path=='/api/auth/login':time.sleep(delay_login)
            count=int(self.headers.get('Content-Length',0));self.rfile.read(count)
            values={'/api/auth/login':{'token':'fixture'},'/api/vm/info':{'mdns':'fixture.local','image':'synthetic','application':'synthetic'},'/api/ai/memory-fabric':{'enabled':False,'running':False},'/api/vm/hdmi/capture':{'enabled':True}}
            value={'code':0,'data':values.get(self.path,{})} if 'terminal' not in self.path and 'sensitive' not in self.path else {'code':-1}
            raw=json.dumps(value).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
    ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);ctx.load_cert_chain(cert,key)
    if rotate:
        leaf_key,csr,leaf_cert=tmp_path/'leaf.key',tmp_path/'leaf.csr',tmp_path/'leaf.pem'
        subprocess.run(['openssl','req','-newkey','rsa:2048','-nodes','-keyout',str(leaf_key),'-out',str(csr),'-subj','/CN=leaf'],check=True,capture_output=True)
        subprocess.run(['openssl','x509','-req','-in',str(csr),'-CA',str(cert),'-CAkey',str(key),'-CAcreateserial','-out',str(leaf_cert),'-days','1'],check=True,capture_output=True)
        leaf_ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);leaf_ctx.load_cert_chain(leaf_cert,leaf_key)
        counter=[0]
        def switch(sock,name,context):
            counter[0]+=1
            if counter[0]>int(rotate):sock.context=leaf_ctx
        ctx.set_servername_callback(switch)
    server.socket=ctx.wrap_socket(server.socket,server_side=True)
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    try:yield f'https://127.0.0.1:{server.server_port}',ssl.PEM_cert_to_DER_cert(cert.read_text()),calls
    finally:server.shutdown();server.server_close();thread.join()

@contextlib.contextmanager
def console(tmp_path,profile,binary=BINARY):
    config=tmp_path/'connection.json';config.write_text(json.dumps(profile));config.chmod(0o600)
    state=tmp_path/'state'
    process=subprocess.Popen([str(binary),'--profile',str(config),'--state-dir',str(state),'--port','0'],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
    try:
        end=time.monotonic()+10
        while not (state/'launch.json').exists() and time.monotonic()<end:
            assert process.poll() is None;time.sleep(.03)
        record=json.loads((state/'launch.json').read_text());url,token=record['url'].split('#');url=url.rstrip('/')
        req=urllib.request.Request(url+'/api/session',data=b'',headers={'X-Launch-Token':token},method='POST')
        with urllib.request.urlopen(req) as r:cookie=r.headers['Set-Cookie'].split(';')[0];session=json.load(r)
        def status():
            req=urllib.request.Request(url+'/api/console/status',headers={'Cookie':cookie})
            with urllib.request.urlopen(req,timeout=30) as r:return json.load(r)
        status.url=url;status.cookie=cookie;status.token=session["token"]
        yield status
    finally:
        process.terminate();process.wait(timeout=15);process.stderr.close()

def profile(endpoints,der):
    return {'endpoints':endpoints,'device':'fixture.local','certificate_der':base64.b64encode(der).decode(),'username':'fixture','password':'synthetic-password'}

def test_unreachable_primary_uses_trusted_fallback(tmp_path):
    with fixture(tmp_path) as (url,der,calls):
        with console(tmp_path,profile(['https://127.0.0.1:1',url],der)) as status:
            result=status()
            assert result['checks']['connection']['ok']
            assert [r['state'] for r in result['checks']['connection']['routes']]==['unreachable','connected']
            assert calls.count('/api/auth/login')==1
            assert result['identity']['endpoint']==url

def test_wrong_identity_stops_without_next_login(tmp_path):
    with fixture(tmp_path) as (url,der,calls):
        p=profile([url,'https://127.0.0.1:1'],der);p['device']='different.local'
        with console(tmp_path,p) as status:
            result=status();assert not result['checks']['connection']['ok']
            assert 'identity' in result['checks']['connection']['detail']
            assert calls.count('/api/auth/login')==1

def test_wrong_certificate_sends_no_credentials(tmp_path):
    other=tmp_path/'other';other.mkdir()
    with fixture(tmp_path) as (url,der,calls),fixture(other) as (_,other_der,_calls):
        with console(tmp_path,profile([url,'https://127.0.0.1:1'],other_der)) as status:
            result=status();assert not result['checks']['connection']['ok']
            assert 'TLS trust' in result['checks']['connection']['detail']
            assert calls==[]

def test_relocated_pair_works_without_source_and_rejects_changed_guard(tmp_path):
    bundle=tmp_path/'bundle';bundle.mkdir();binary=bundle/'nanokvm-console';guard=bundle/'console_guard'
    shutil.copyfile(BINARY,binary);binary.chmod(0o700)
    shutil.copyfile(ROOT/'target/release/console_guard',guard);guard.chmod(0o700)
    env={**os.environ,'XDG_CONFIG_HOME':str(tmp_path/'config')}
    valid=subprocess.run([str(binary),'--verify'],env=env,cwd=tmp_path,capture_output=True)
    assert valid.returncode==0
    guard.write_bytes(b'changed')
    invalid=subprocess.run([str(binary),'--verify'],env=env,cwd=tmp_path,capture_output=True)
    assert invalid.returncode==1


def test_signed_leaf_change_receives_no_http_credentials(tmp_path):
    with fixture(tmp_path,rotate=True) as (url,der,calls):
        with console(tmp_path,profile([url],der)) as status:
            result=status()
            assert not result['checks']['connection']['ok']
            assert 'certificate changed' in result['checks']['connection']['detail']
            assert not any(c.startswith('leaf:') for c in calls)


def test_login_transport_loss_uses_fallback(tmp_path):
    other=tmp_path/'fallback';other.mkdir()
    # Both listeners use the same device certificate and identity.
    with fixture(tmp_path,drop_login=True) as (url,der,calls):
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_GET(self):self.reply()
            def do_POST(self):self.reply()
            def reply(self):
                n=int(self.headers.get('Content-Length',0));self.rfile.read(n)
                data={'token':'fixture'} if self.path=='/api/auth/login' else {'mdns':'fixture.local'}
                raw=json.dumps({'code':0,'data':data}).encode()
                self.send_response(200);self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
        fallback=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
        ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);ctx.load_cert_chain(tmp_path/'cert.pem',tmp_path/'key.pem');fallback.socket=ctx.wrap_socket(fallback.socket,server_side=True)
        thread=threading.Thread(target=fallback.serve_forever,daemon=True);thread.start()
        try:
            with console(tmp_path,profile([url,f'https://127.0.0.1:{fallback.server_port}'],der)) as status:
                result=status()
                assert result['checks']['connection']['ok']
                assert [r['state'] for r in result['checks']['connection']['routes']]==['unreachable','connected']
        finally:fallback.shutdown();fallback.server_close();thread.join()


def test_overlapping_diagnostics_do_not_create_second_login(tmp_path):
    with fixture(tmp_path,delay_login=.6) as (url,der,calls):
        with console(tmp_path,profile([url],der)) as status:
            first=[];thread=threading.Thread(target=lambda:first.append(status()));thread.start()
            deadline=time.monotonic()+5
            while '/api/auth/login' not in calls and time.monotonic()<deadline:time.sleep(.01)
            second=status()
            thread.join(timeout=10)
            assert 'already running' in second['checks']['connection']['detail']
            assert first[0]['checks']['connection']['ok']
            assert calls.count('/api/auth/login')==1


def test_websocket_leaf_change_receives_no_cookie(tmp_path):
    import websocket
    with fixture(tmp_path,rotate=4) as (url,der,calls):
        with console(tmp_path,profile([url],der)) as status:
            ws=websocket.create_connection(status.url.replace('http:','ws:')+'/api/console/ws',origin=status.url,cookie=status.cookie,timeout=15)
            try:
                ws.send(json.dumps({'token':status.token,'mode':'live'}))
                reply=json.loads(ws.recv())
                assert reply['type']=='error' and 'certificate changed' in reply['detail']
                assert not reply['retryable']
                assert not any(c.startswith('leaf:') for c in calls)
            finally:ws.close()
