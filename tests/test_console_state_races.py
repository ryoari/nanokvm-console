"""Browser regressions for delayed acknowledgements, old sessions and video loss.

Synthetic DOM/peers only. No target input or device connection.
"""
import os
import shutil
from pathlib import Path
import pytest
from playwright.sync_api import sync_playwright

ROOT=Path(os.environ.get('NANOKVM_UI_SNAPSHOT',Path(__file__).resolve().parents[1]))
SCRIPT=r'''
window.sockets=[];
class Socket {
 static OPEN=1;
 constructor(){this.readyState=1;this.sent=[];sockets.push(this)}
 send(v){this.sent.push(JSON.parse(v))}
 close(){this.readyState=3;queueMicrotask(()=>this.onclose?.())}
 receive(v){return this.onmessage({data:JSON.stringify(v)})}
}
class Peer {
 constructor(){this.connectionState='connecting';this.iceConnectionState='checking'}
 addTransceiver(){} async createOffer(){return {type:'offer',sdp:'test'}}
 async setLocalDescription(v){this.localDescription=v}
 async setRemoteDescription(v){this.remoteDescription=v}
 async addIceCandidate(){} close(){this.connectionState='closed'}
 async getStats(){return new Map()}
}
window.WebSocket=Socket;window.RTCPeerConnection=Peer;
window.apiResult={source:'synthetic fixture',checks:{connection:{ok:true}},lean_guard_ready:true};
window.w=new ConsoleWorkbench(()=>Promise.resolve(structuredClone(apiResult)),()=>({mode:'live',token:'synthetic'}));
Object.defineProperty(w.video,'videoWidth',{value:640});Object.defineProperty(w.video,'videoHeight',{value:360});
window.ready=async()=>{await w.open();await w.current.ws.receive({type:'ready'});w.current.pc.connectionState='connected';w.video.dispatchEvent(new Event('playing'))};
window.arm=async()=>{document.getElementById('consoleArm').click();await w.current.ws.receive({type:'input-status',accepted:true,armed:true})};
'''

@pytest.fixture
def page():
    with sync_playwright() as pw:
        b=pw.chromium.launch(executable_path=os.environ.get('CHROME') or shutil.which('google-chrome') or shutil.which('chromium'),headless=True,chromium_sandbox=True)
        c=b.new_context(service_workers='block');c.route('**/*',lambda r:r.abort())
        p=c.new_page();p.set_content('''<button id="consoleConnect">Connect</button><button id="consoleArm">Enable input</button><button id="consoleClose">Close</button><button id="consoleRead">Check</button><p id="consoleMessage" role="status"></p><span id="connectionState"></span><span id="inputState"></span><div id="consoleEmpty"></div><video id="consoleVideo" tabindex="0" muted></video><p id="consoleDiagnosticSummary"></p><pre id="consoleDiagnostics"></pre>''')
        p.add_script_tag(path=str(ROOT/'web/console.js'));p.add_script_tag(content=SCRIPT)
        yield p
        c.close();b.close()

@pytest.mark.parametrize('event',['Escape','blur'])
def test_disable_cancels_pending_arm(page,event):
    page.evaluate('ready()');page.locator('#consoleArm').click()
    if event=='Escape':page.locator('#consoleVideo').press('Escape')
    else:page.locator('#consoleRead').focus()
    page.evaluate("w.current.ws.receive({type:'input-status',accepted:true,armed:true})")
    assert page.evaluate('!w.current.armed')
    assert page.evaluate("w.current.ws.sent.some(v=>v.value?.op==='disarm')")

@pytest.mark.parametrize('event',['mute','ended'])
def test_video_track_loss_disarms(page,event):
    page.evaluate('''async()=>{await ready();const c=document.createElement('canvas');c.width=640;c.height=360;window.track=c.captureStream(1).getVideoTracks()[0];w.current.pc.ontrack({track});await arm()}''')
    page.evaluate('(event)=>track.dispatchEvent(new Event(event))',event)
    assert page.evaluate('!w.current.armed')
    assert page.locator('#consoleArm').is_disabled()
    assert page.locator('#connectionState').inner_text()!='Connected'


def test_connected_after_playing_updates_controls(page):
    page.evaluate('''async()=>{await w.open();await w.current.ws.receive({type:'ready'});w.video.dispatchEvent(new Event('playing'));w.current.pc.connectionState='connected';w.current.pc.onconnectionstatechange()}''')
    assert not page.locator('#consoleArm').is_disabled()
    assert not page.locator('#consoleConnect').is_disabled()
    assert page.locator('#connectionState').inner_text()=='Connected'


def test_fatal_error_disarms_before_close(page):
    page.evaluate('''async()=>{await ready();await arm();await w.current.ws.receive({type:'error',detail:'Synthetic session failure. Reconnect.'})}''')
    assert page.evaluate('!w.current.armed')
    assert page.locator('#consoleArm').is_disabled()
    assert page.locator('#connectionState').inner_text()=='Connection error'

@pytest.mark.parametrize('reject',[True,False])
def test_old_negotiation_does_not_change_new_session(page,reject):
    page.evaluate('''async()=>{await w.open();window.old=w.current;window.pending=new Promise((resolve,reject)=>{window.resolveOld=resolve;window.rejectOld=reject});Peer.prototype.createOffer=()=>pending;window.oldTask=old.ws.receive({type:'ready'});await w.close();Peer.prototype.createOffer=async()=>({type:'offer',sdp:'new'});await ready();await arm();window.before={message:w.message.textContent,opening:w.opening}}''')
    page.evaluate('''async(reject)=>{if(reject)rejectOld(new Error('old failure'));else resolveOld({type:'offer',sdp:'old'});await oldTask}''',reject)
    assert page.evaluate('w.current.armed && w.message.textContent===before.message && w.opening===before.opening')
    assert page.evaluate('!old.timer')


def test_diagnostic_stats_do_not_publish_old_session(page):
    page.evaluate('''async()=>{await ready();w.current.pc.getStats=()=>new Promise(resolve=>window.resolveStats=resolve);window.readTask=w.diagnostics().catch(()=>{});await Promise.resolve()}''')
    page.wait_for_function('typeof resolveStats==="function"')
    page.evaluate('''async()=>{await w.close();await ready();resolveStats(new Map([['old',{type:'inbound-rtp',kind:'video',framesDecoded:999}]]));await readTask}''')
    assert '999' not in page.locator('#consoleDiagnostics').inner_text()
    assert 'retry' in page.locator('#consoleDiagnosticSummary').inner_text().lower()

@pytest.mark.parametrize('which',['api','stats'])
def test_pending_diagnostics_time_out_and_late_result_is_ignored(page,which):
    page.evaluate('ready()')
    page.evaluate('''(which)=>{const original=setTimeout;window.setTimeout=(fn,ms,...args)=>original(fn,ms>=1000?40:ms,...args);if(which==='api')w.api=()=>new Promise(resolve=>window.resolveLate=resolve);else w.current.pc.getStats=()=>new Promise(resolve=>window.resolveLate=resolve);window.readTask=w.diagnostics().catch(()=>{})}''',which)
    page.wait_for_function('!w.reading',timeout=1500)
    assert not page.locator('#consoleRead').is_disabled()
    assert 'failed' in page.locator('#consoleDiagnosticSummary').inner_text().lower()
    page.evaluate('''async(which)=>{w.api=()=>Promise.resolve(structuredClone(apiResult));w.current.pc.getStats=async()=>new Map();await w.diagnostics();resolveLate(which==='api'?{source:'OBSOLETE',checks:{},lean_guard_ready:true}:new Map([['old',{type:'inbound-rtp',kind:'video',framesDecoded:999}]]));await Promise.resolve()}''',which)
    assert 'synthetic fixture' in page.locator('#consoleDiagnostics').inner_text()
    assert 'checks passed' in page.locator('#consoleDiagnosticSummary').inner_text()
    assert 'OBSOLETE' not in page.locator('#consoleDiagnostics').inner_text()
    assert '999' not in page.locator('#consoleDiagnostics').inner_text()


def test_repeated_acks_do_not_repeat_status_announcements(page):
    count=page.evaluate('''async()=>{await ready();await arm();let count=0;const observer=new MutationObserver(rows=>count+=rows.length);observer.observe(document.getElementById('inputState'),{childList:true,subtree:true});observer.observe(document.getElementById('connectionState'),{childList:true,subtree:true});for(let i=0;i<20;i++)await w.current.ws.receive({type:'input-status',accepted:true,armed:true});await Promise.resolve();observer.disconnect();return count}''')
    assert count==0


def test_socket_construction_failure_allows_retry(page):
    page.evaluate('''async()=>{window.savedSocket=WebSocket;window.WebSocket=class {constructor(){throw new Error('Synthetic constructor failure')}};await w.open().catch(()=>{})}''')
    assert not page.locator('#consoleConnect').is_disabled()
    assert not page.evaluate('w.opening')
    page.evaluate('''async()=>{window.WebSocket=savedSocket;await ready()}''')
    assert page.locator('#connectionState').inner_text()=='Connected'


def test_closure_timeout_keeps_owner_and_gives_tab_recovery(page):
    page.evaluate('''async()=>{await ready();await arm();w.current.ws.close=()=>{};const original=setTimeout;window.setTimeout=(fn,ms,...args)=>original(fn,ms>=1000?40:ms,...args);await w.close().catch(()=>{})}''')
    assert page.evaluate('!!w.current && w.current.closing && !w.current.armed')
    assert 'Close this tab' in page.locator('#consoleMessage').inner_text()
    assert page.locator('#connectionState').inner_text()=='Closure unconfirmed'


def test_peer_recovery_clears_old_failure_message(page):
    page.evaluate('''async()=>{await ready();await arm();const pc=w.current.pc;pc.connectionState='disconnected';pc.onconnectionstatechange();pc.connectionState='connected';pc.onconnectionstatechange()}''')
    assert not page.locator('#consoleArm').is_disabled()
    assert 'disconnected' not in page.locator('#consoleMessage').inner_text()
    assert page.locator('#inputState').inner_text()=='Input disabled'


def load_app(page,failed=False):
    page.evaluate("history.replaceState=()=>{}")
    page.evaluate("document.body.insertAdjacentHTML('beforeend','<span id=mode></span><details id=diagnostics></details>')")
    page.evaluate('(failed)=>{window.fetch=async()=>({ok:!failed,json:async()=>({token:"synthetic",mode:"live"})})}',failed)
    page.add_script_tag(path=str(ROOT/'web/app.js'))
    page.wait_for_function("() => state.token || document.getElementById('consoleMessage').textContent.includes('nanokvm-console --open')")


def test_failed_bootstrap_keeps_actionable_guidance(page):
    load_app(page,True)
    page.evaluate("window.dispatchEvent(new Event('blur'))")
    assert page.locator('#consoleRead').is_disabled()
    assert page.locator('#consoleConnect').is_disabled()
    assert 'nanokvm-console --open' in page.locator('#consoleMessage').inner_text()


def test_unreadable_response_has_retry_guidance(page):
    load_app(page)
    message=page.evaluate('''async()=>{window.fetch=async()=>({ok:false,json:async()=>{throw new SyntaxError('Unexpected token')}});return api('/api/console/status').catch(e=>e.message)}''')
    assert 'Retry' in message
    assert 'Unexpected token' not in message


def test_recovery_is_bounded_and_stays_disarmed(page):
    page.evaluate("""async()=>{await ready();await arm();const native=setTimeout;window.setTimeout=(fn,ms,...args)=>native(fn,[1000,3000,8000].includes(ms)?20:ms,...args);w.current.ws.close()}""")
    page.wait_for_function('sockets.length===2')
    assert page.evaluate('!w.current.armed && !w.current.wantsInput')
    for n in [3,4]:
        page.evaluate('w.current.ws.close()')
        page.wait_for_function(f'sockets.length==={n}')
    page.evaluate('w.current.ws.close()')
    page.wait_for_function('!w.current && !w.retryTimer')
    assert 'three attempts' in page.locator('#consoleMessage').inner_text()
    assert page.evaluate("sockets.slice(1).every(s=>s.sent.every(v=>v.value?.op!=='arm'))")


def test_close_cancels_pending_recovery(page):
    page.evaluate('''async()=>{await ready();w.current.ws.close();await Promise.resolve();await w.close()}''')
    page.wait_for_timeout(1100)
    assert page.evaluate('sockets.length===1 && !w.retryTimer && !w.current')


def test_identity_error_does_not_retry(page):
    page.evaluate('''async()=>{await ready();await w.current.ws.receive({type:'error',detail:'Device certificate changed.',retryable:false});w.current.ws.close()}''')
    page.wait_for_timeout(1100)
    assert page.evaluate('sockets.length===1 && !w.retryTimer')


def test_track_resume_allows_manual_arm_but_never_rearms(page):
    page.evaluate("""async()=>{await ready();const c=document.createElement('canvas');c.width=640;c.height=360;window.track=c.captureStream(10).getVideoTracks()[0];w.current.pc.ontrack({track});window.paint=setInterval(()=>c.getContext('2d').fillRect(0,0,640,360),40);await w.video.play()}""")
    page.wait_for_function('w.videoReady()')
    page.evaluate("""async()=>{await arm();track.dispatchEvent(new Event('mute'));track.dispatchEvent(new Event('unmute'))}""")
    page.wait_for_function('!document.getElementById("consoleArm").disabled')
    assert page.evaluate('!w.current.armed && !w.current.wantsInput && !w.current.trackLost')


def test_track_stall_enters_bounded_recovery(page):
    page.evaluate("""async()=>{await ready();const c=document.createElement('canvas');c.width=640;c.height=360;window.track=c.captureStream(1).getVideoTracks()[0];w.current.pc.ontrack({track});const native=setTimeout;window.setTimeout=(fn,ms,...args)=>native(fn,[1000,10000].includes(ms)?20:ms,...args);track.dispatchEvent(new Event('mute'))}""")
    page.wait_for_function('sockets.length===2')
    assert page.evaluate('!w.current.armed')


def test_unconfirmed_release_is_visible_and_stops_recovery(page):
    page.evaluate("""async()=>{await ready();await arm();await w.current.ws.receive({type:'error',detail:'Input release is unconfirmed.',retryable:false,release_unconfirmed:true});w.current.ws.close()}""")
    page.wait_for_function('!w.current')
    assert page.locator('#inputState').inner_text()=='Input release unconfirmed'
    assert not page.evaluate('w.retryTimer')
