'use strict';
// Uses the shipped device's H.264 WebRTC and InputRouter interfaces.
// The server-side Lean guard validates and encodes every input report.
class ConsoleWorkbench {
  constructor(api, session) {
    this.api=api; this.session=session; this.current=null; this.opening=false; this.reading=false; this.lastError=null; this.retryCount=0; this.retryTimer=null;
    this.video=document.getElementById('consoleVideo');
    this.message=document.getElementById('consoleMessage');
    this.keys=new Set(); this.buttons=0; this.x=32768; this.y=32768;
    this.bind(); this.render();
  }
  say(text) { if(this.message.textContent!==text)this.message.textContent=text; }
  setAvailability(reason=null) {this.unavailable=reason;this.render();if(reason)this.say(reason)}
  render() {
    const s=this.current, ready=this.videoReady(s);
    const phase=s?.closeTimedOut?'Closure unconfirmed':s?.closing?'Closing':s?.error?'Connection error':ready?'Connected':s||this.opening?'Connecting':this.retryTimer?'Reconnecting':this.lastError?'Connection error':'Disconnected';
    const set=(id,text)=>{const e=document.getElementById(id);if(e&&e.textContent!==text)e.textContent=text};
    set('connectionState',phase); set('inputState',this.releaseUnconfirmed||s?.closeTimedOut?'Input release unconfirmed':s?.armed&&ready?'Input enabled':'Input disabled');
    const arm=document.getElementById('consoleArm');
    arm.disabled=!!this.unavailable||!ready||!!(s?.wantsInput&&!s.armed); set('consoleArm',s?.armed?'Disable input':s?.wantsInput?'Enabling…':'Enable input');
    arm.setAttribute('aria-pressed',String(!!s?.armed&&ready));
    document.getElementById('consoleConnect').disabled=!!this.unavailable||this.opening||!!s?.closing;
    document.getElementById('consoleClose').disabled=!!this.unavailable||(!s&&!this.retryTimer)||!!s?.closing;
    document.getElementById('consoleRead').disabled=!!this.unavailable||this.reading;
    const empty=document.getElementById('consoleEmpty');
    if(empty){
      empty.hidden=ready;
      empty.textContent=this.unavailable|| (s?.closeTimedOut?'Close this tab, then reopen the console from the launcher. Input release is not confirmed.':s?.closing?'Closing the console. Input is disabled.':s?.error||this.lastError||
        (s||this.opening?'Connecting to the Go+… Input stays disabled. Select Close console to cancel.':'Select Connect to show the remote screen.'));
    }
  }
  videoReady(s=this.current) {
    return !!(s && !s.closing && !s.fatalError && !s.trackLost && s.playing && s.pc?.connectionState==='connected' && this.video.videoWidth &&
      (s.videoStatus===undefined || s.videoStatus===1));
  }
  send(value,s=this.current) {
    if(s && s.ws.readyState===WebSocket.OPEN) s.ws.send(JSON.stringify(value));
  }
  input(value) { this.send({type:'input',value}); }
  disarm() {
    this.keys.clear(); this.buttons=0;
    if(this.current){this.current.armed=false;this.current.wantsInput=false}
    document.getElementById('consoleArm').textContent='Enable input';
    this.input({op:'disarm'}); this.render();
  }
  cleanup(s) {
    clearInterval(s.timer); clearTimeout(s.peerTimer); clearTimeout(s.trackTimer); s.timer=null; s.pc?.close();
    if(this.current!==s) return;
    this.current=null; this.video.srcObject=null; this.keys.clear(); this.buttons=0;
    document.getElementById('consoleArm').disabled=true;
    document.getElementById('consoleArm').textContent='Enable input';
    this.opening=false; this.lastError=s.userClosed?null:s.error?.replace(/Close and reconnect\.?/gi,'Select Connect to retry.')||null; this.render();
    this.say(this.lastError || 'Console closed. Input is disabled. Select Connect to reconnect.');
    if(!s.userClosed&&s.retryable!==false&&!this.unavailable)this.scheduleRecovery();
  }
  stopRecovery() {clearTimeout(this.retryTimer);this.retryTimer=null;}
  scheduleRecovery() {
    if(this.retryTimer||this.current)return;
    const delays=[1000,3000,8000];
    if(this.retryCount>=delays.length){this.lastError='Automatic recovery stopped after three attempts. Check connection, then select Connect.';this.render();this.say(this.lastError);return}
    const delay=delays[this.retryCount++];
    this.lastError=`Connection lost. Recovery attempt ${this.retryCount} of 3 in ${delay/1000} seconds. Input is disabled. Select Close console to cancel.`;
    this.retryTimer=setTimeout(()=>{this.retryTimer=null;this.open(true).catch(e=>{this.lastError=e.message;this.render();this.say(e.message)})},delay);
    this.render();this.say(this.lastError);
  }
  async close() {
    this.stopRecovery();
    const s=this.current; if(!s){this.lastError=null;this.render();this.say("Recovery cancelled. Input is disabled.");return}
    s.userClosed=true; s.closing=true; this.disarm(); this.say('Closing the console. Input is disabled.'); this.render(); this.send({type:'close'},s); s.pc?.close(); s.ws.close();
    try {await this.bounded(s.closed,12000,'Close this tab, then reopen the console from the launcher. Input release is not confirmed.')}
    catch(e){if(this.current===s){s.closeTimedOut=true;this.render();this.say(e.message)}throw e}
  }
  async open(recovery=false) {
    if(!recovery){this.stopRecovery();this.retryCount=0}
    if(this.opening||this.current?.closing)return;
    if(this.unavailable)throw new Error(this.unavailable);
    const auth=this.session();
    if(auth.mode!=='live')throw new Error('Select Live Go+ before opening the console.');
    this.opening=true; this.lastError=null; this.render();
    try { await this.close(); } catch(e) {this.opening=false;this.render();throw e}
    this.opening=true;
    this.lastError=null;
    let socket;
    try {socket=new WebSocket(`${location.protocol==='https:'?'wss':'ws'}://${location.host}/api/console/ws`)}
    catch(e){this.opening=false;this.lastError='Connection could not start. Select Connect to retry.';this.render();this.say(this.lastError);throw new Error(this.lastError)}
    const s={ws:socket,pc:null,armed:false,wantsInput:false,playing:false,closing:false,candidates:[]};
    s.closed=new Promise(resolve=>s.resolveClosed=resolve);
    this.current=s; this.render(); this.say('Connecting to the trusted Go+…');
    s.ws.onopen=()=>{if(this.current===s&&!s.closing)this.send({token:auth.token,mode:auth.mode},s)};
    s.ws.onclose=()=>{this.cleanup(s);s.resolveClosed()};
    s.ws.onerror=()=>{if(this.current===s&&!s.closing){s.error='Console transport failed. Close and reconnect.';s.fatalError=true;this.opening=false;this.disarm();this.say(s.error)}};
    s.ws.onmessage=async message=>{
      if(this.current!==s||s.closing||s.fatalError) return;
      try {
        const v=JSON.parse(message.data);
        if(v.type==='ready') {
          const pc=new RTCPeerConnection({iceServers:[]}); s.pc=pc;
          pc.onicecandidate=e=>{if(e.candidate&&this.current===s&&!s.closing)this.send({type:'signal',value:{event:'video-candidate',data:JSON.stringify(e.candidate)}},s)};
          pc.ontrack=e=>{
            if(this.current!==s||s.closing||e.track.kind!=='video')return;
            const lost=()=>{if(this.current===s&&!s.closing){s.trackLost=true;s.error='Video stream stopped. Close and reconnect.';clearTimeout(s.trackTimer);s.trackTimer=setTimeout(()=>{if(this.current===s&&!s.closing&&s.trackLost){s.retryable=true;s.ws.close()}},10000);this.opening=false;this.disarm();this.say(s.error)}};
            e.track.onmute=lost;e.track.onended=lost;
            e.track.onunmute=()=>{if(this.current===s&&!s.closing&&!s.fatalError){clearTimeout(s.trackTimer);s.trackLost=false;if(s.error==='Video stream stopped. Close and reconnect.')s.error=null;if(this.videoReady(s))this.opening=false;this.render();this.say('Video stream resumed. Input is disabled.')}};
            this.video.srcObject=new MediaStream([e.track]);
            this.video.play().catch(()=>{if(this.current===s&&!s.closing){s.error='Video playback failed. Close and reconnect.';s.fatalError=true;this.opening=false;this.disarm();this.say(s.error)}});
          };
          pc.onconnectionstatechange=()=>{
            if(this.current!==s||s.closing||s.fatalError) return;
            if(['failed','disconnected','closed'].includes(pc.connectionState)) {
              this.disarm();document.getElementById('consoleArm').disabled=true;
              clearTimeout(s.peerTimer);
              s.peerTimer=setTimeout(()=>{if(this.current===s&&!s.closing&&s.pc.connectionState!=='connected'){s.retryable=true;s.ws.close()}},10000);
              s.peerError=true;s.error='Video connection '+pc.connectionState+'. Close and reconnect.';this.opening=false;this.render();this.say(s.error);
            } else if(pc.connectionState==='connected'){
              clearTimeout(s.peerTimer);
              if(s.peerError){s.peerError=false;s.error=null;this.say('Video connection recovered. Input is disabled.')}
              if(this.videoReady(s))this.opening=false;
              this.render();
            }
          };
          pc.addTransceiver('video',{direction:'recvonly'});
          const offer=await pc.createOffer();
          if(this.current!==s||s.closing)return;
          await pc.setLocalDescription(offer);
          if(this.current!==s||s.closing)return;
          if(this.current===s&&!s.closing)this.send({type:'signal',value:{event:'video-offer',data:JSON.stringify(pc.localDescription)}},s);
          s.timer=setInterval(()=>this.send({type:'ping'},s),10000);
        } else if(v.type==='signal'&&s.pc) {
          const signal=v.value;
          if(signal.event==='video-status'){
            s.videoStatus=Number(signal.data);
            const labels={1:'Video source ready.', '-1':'No video signal. Check connection diagnostics for capture service or HDMI faults.', '-2':'Device encoder error.', '-3':'Device image buffer full.', '-4':'Device video mode does not match H.264.', '-5':'Device capture is disabled.'};
            s.error=s.videoStatus===1?null:(labels[s.videoStatus]||'Unrecognized device video status.');
            if(s.error)this.opening=false;
            this.say(s.error||labels[1]);
            if(s.videoStatus!==1)this.disarm();
            this.render();
          }
          if(signal.event==='video-answer') {
            await s.pc.setRemoteDescription(JSON.parse(signal.data));
            if(this.current!==s||s.closing)return;
            for(const c of s.candidates){await s.pc.addIceCandidate(c);if(this.current!==s||s.closing)return}
            s.candidates=[];
          } else if(signal.event==='video-candidate') {
            const c=JSON.parse(signal.data);
            if(s.pc.remoteDescription) await s.pc.addIceCandidate(c); else s.candidates.push(c);
          }
        } else if(v.type==='input-status') {
          s.armed=v.armed===true&&s.wantsInput&&this.videoReady(s);
          if(!s.armed)s.wantsInput=false;
          if(v.armed===true&&!s.armed)this.input({op:'disarm'});
          this.render();
          if(!v.accepted)this.say('An input event was rejected. Disable input and try again.');
        } else if(v.type==='error') {if(v.release_unconfirmed)this.releaseUnconfirmed=true;s.retryable=v.retryable===true;s.error=v.detail;s.fatalError=true;this.opening=false;this.disarm();this.say(v.detail)}
      } catch(e) {if(this.current!==s||s.closing)return;s.fatalError=true;s.error='Console negotiation failed. Close and reconnect.';this.opening=false;this.say(s.error);this.disarm()}
    };
  }
  async bounded(promise, milliseconds, message) {
    let timer;
    try {return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error(message)),milliseconds)})])}
    finally {clearTimeout(timer)}
  }
  async diagnostics() {
    if(this.unavailable)throw new Error(this.unavailable);
    if(this.reading)return;
    this.reading=true;
    const button=document.getElementById('consoleRead'), summary=document.getElementById('consoleDiagnosticSummary'), raw=document.getElementById('consoleDiagnostics');
    document.getElementById('consoleChecks')?.replaceChildren();
    const identity=document.getElementById('consoleIdentity');if(identity)identity.textContent='';
    const technical=document.getElementById('technicalDetails');if(technical)technical.open=false;
    button.disabled=true;button.setAttribute('aria-busy','true');button.textContent='Checking…';
    summary.textContent='Checking the connection…';raw.textContent='Waiting for a new result.';this.say(summary.textContent);
    try {
    const s=this.current;
    const result=await this.bounded(this.api('/api/console/status'),45000,'Connection check timed out. Select Check connection to retry.');
    if(this.current!==s||s?.closing)throw new Error('Console session changed. Select Check connection to retry.');
    result.browser={connected:!!s,peer:s?.pc?.connectionState||'closed',
      ice:s?.pc?.iceConnectionState||'closed',video_status:s?.videoStatus??null,
      width:this.video.videoWidth,height:this.video.videoHeight,input_armed:!!s?.armed};
    if(s?.pc){
      const stats=await this.bounded(s.pc.getStats(),5000,'Video statistics timed out. Select Check connection to retry.');
      if(this.current!==s||s.closing)throw new Error('Console session changed. Select Check connection to retry.');
      const inbound=[...stats.values()].find(r=>r.type==='inbound-rtp'&&r.kind==='video');
      result.browser.frames_decoded=inbound?.framesDecoded||0;
      result.browser.bytes_received=inbound?.bytesReceived||0;
    }
    document.getElementById('consoleDiagnostics').textContent=JSON.stringify(result,null,2);
    const checks=Object.entries(result.checks||{});
    const ready=checks.length>0&&checks.every(([,value])=>value.ok===true)&&result.lean_guard_ready===true;
    summary.textContent=ready?'Connection checks passed. Select Connect to open the screen.':'Connection needs attention. Review the checks below before connecting.';
    if(s)summary.textContent=ready?'Connection checks passed. The browser snapshot is shown below.':summary.textContent;
    const list=document.getElementById('consoleChecks');
    if(list){
      const labels={connection:'Device connection',memory:'Memory Fabric stopped',capture:'Video capture',low_power:'Power setting',encoder:'Video encoder',capture_service:'Capture service',restart:'Device restart check'};
      const hints={connection:'Check the USB network connection and saved device profile.',memory:'Disable Memory Fabric in the vendor console, then retry.',capture:'Check HDMI and capture settings in the vendor console.',capture_service:'Check the capture process, sockets and library results in Technical details.'};
      const add=(name,ok,detail)=>{
        const row=document.createElement('li'),title=document.createElement('strong'),status=document.createElement('span'),note=document.createElement('small');
        row.className=ok?'check-ok':'check-failed';title.textContent=name;status.textContent=ok?'Pass':'Needs attention';note.textContent=detail||'';
        row.append(title,status,note);list.append(row);
      };
      for(const [key,value] of checks){
        let detail=value.detail||'';
        if(!value.ok)detail+=(detail?' ':'')+(hints[key]||'Inspect Technical details and retry the check.');
        else if(key==='encoder'&&value.data?.fps!=null)detail='Configured frame rate: '+value.data.fps+' fps.';
        else if(key==='low_power'&&value.data?.enabled!=null)detail='Low-power setting: '+(value.data.enabled?'enabled.':'disabled.');
        else if(key==='connection'&&value.routes)detail=value.routes.map(r=>`${r.endpoint}: ${r.state}`).join('; ');
        else if(key==='memory')detail='Recording service is disabled and stopped.';
        add(labels[key]||key.replaceAll('_',' '),value.ok===true,detail);
      }
      add('Input verification',result.lean_guard_ready===true,result.lean_guard_ready?'Input rules are ready.':'Rebuild and restart the local workbench.');
    }
    if(identity){
      const id=result.identity;
      const device=id?`${id.mdns||'Device'} · Image ${id.firmware||'unknown'} · App ${id.application||'unknown'}`:'Device identity unavailable.';
      const video=result.browser.width?`${result.browser.width} × ${result.browser.height}, ${result.browser.frames_decoded??0} decoded frames`:'No browser video';
      identity.textContent=`${device} · ${video}. Snapshot at ${new Date().toLocaleTimeString()}.`;
    }
    this.say('Connection check complete. See Connection details.');
    return result;
    } catch(e) {
      summary.textContent='Connection check failed. '+e.message;
      raw.textContent='No current result. Select Check connection to retry.';
      this.say(summary.textContent);throw e;
    } finally {
      this.reading=false;button.disabled=false;button.setAttribute('aria-busy','false');button.textContent='Check connection';
    }
  }
  pointer(event,wheel=0) {
    const s=this.current;if(!s?.armed||!s.wantsInput||!this.videoReady(s))return;
    const r=this.video.getBoundingClientRect();
    const scale=Math.min(r.width/this.video.videoWidth,r.height/this.video.videoHeight);
    const w=this.video.videoWidth*scale,h=this.video.videoHeight*scale;
    const nx=(event.clientX-r.left-(r.width-w)/2)/w,ny=(event.clientY-r.top-(r.height-h)/2)/h;
    if(!Number.isFinite(nx)||!Number.isFinite(ny)||nx<0||ny<0||nx>1||ny>1)return;
    this.x=Math.round(nx*65535);this.y=Math.round(ny*65535);
    this.input({op:'pointer',buttons:this.buttons,x:this.x,y:this.y,wheel});
  }
  keyboard(event,down) {
    if(event.code==='Escape'&&(this.current?.armed||this.current?.wantsInput)){event.preventDefault();event.stopPropagation();this.disarm();this.say('Input disabled.');return}
    if(!this.current?.armed||!this.current.wantsInput||!this.videoReady())return;
    event.preventDefault();event.stopPropagation();
    if(down)this.keys.add(event.code);else this.keys.delete(event.code);
    const modifiers={ControlLeft:1,ShiftLeft:2,AltLeft:4,MetaLeft:8,ControlRight:16,ShiftRight:32,AltRight:64,MetaRight:128};
    const special={Enter:40,Backspace:42,Tab:43,Space:44,Minus:45,Equal:46,BracketLeft:47,BracketRight:48,Backslash:49,Semicolon:51,Quote:52,Backquote:53,Comma:54,Period:55,Slash:56,CapsLock:57,Insert:73,Home:74,PageUp:75,Delete:76,End:77,PageDown:78,ArrowRight:79,ArrowLeft:80,ArrowDown:81,ArrowUp:82};
    let modifier=0;const keys=[];
    for(const code of this.keys){
      if(modifiers[code]){modifier|=modifiers[code];continue}
      let usage=special[code];
      if(/^Key[A-Z]$/.test(code))usage=code.charCodeAt(3)-65+4;
      if(/^Digit[0-9]$/.test(code))usage=code==='Digit0'?39:Number(code.at(-1))+29;
      if(/^F([1-9]|1[0-2])$/.test(code))usage=57+Number(code.slice(1));
      if(usage&&keys.length<6)keys.push(usage);
    }
    this.input({op:'keyboard',modifier,keys});
  }
  bind() {
    const run=fn=>()=>Promise.resolve(fn()).catch(e=>this.say(e.message));
    document.getElementById('consoleConnect').onclick=run(()=>this.open());
    document.getElementById('consoleClose').onclick=run(()=>this.close());
    document.getElementById('consoleRead').onclick=run(()=>this.diagnostics());
    document.getElementById('consoleArm').onclick=()=>{
      if(this.current?.armed)this.disarm();
      else if(this.videoReady()&&!this.current.wantsInput){this.current.wantsInput=true;this.input({op:'arm'});this.video.focus();this.render()}
    };
    document.getElementById('consoleArm').onpointerdown=e=>e.preventDefault();
    this.video.onplaying=()=>{if(this.current)this.current.playing=true;if(this.videoReady()){this.opening=false;this.render();this.say(`Video ${this.video.videoWidth} × ${this.video.videoHeight}. Press Escape to disable input.`)}};
    for(const event of ['pause','ended','error','waiting'])this.video.addEventListener(event,()=>{const s=this.current;if(s&&!s.closing){s.playing=false;this.disarm();this.say('Video is not playing. Wait for video, or close and reconnect.')}});
    this.video.addEventListener('keydown',e=>this.keyboard(e,true));
    this.video.addEventListener('keyup',e=>this.keyboard(e,false));
    this.video.addEventListener('pointermove',e=>this.pointer(e));
    this.video.addEventListener('pointerdown',e=>{if(this.current?.armed){e.preventDefault();this.video.focus();this.video.setPointerCapture(e.pointerId);this.buttons=e.buttons&31;this.pointer(e)}});
    this.video.addEventListener('pointerup',e=>{if(this.current?.armed){this.buttons=e.buttons&31;this.pointer(e);if(!this.buttons)this.input({op:'pointer',buttons:0,x:this.x,y:this.y,wheel:0})}});
    this.video.addEventListener('wheel',e=>{if(this.current?.armed){e.preventDefault();this.pointer(e,e.deltaY>0?-1:1)}},{passive:false});
    this.video.addEventListener('contextmenu',e=>e.preventDefault());
    this.video.addEventListener('blur',()=>this.disarm());
    this.video.addEventListener('pointercancel',()=>this.disarm());
    window.addEventListener('blur',()=>this.disarm());
    document.addEventListener('visibilitychange',()=>{if(document.hidden)this.disarm()});
    window.addEventListener('pagehide',()=>{this.stopRecovery();if(this.current)this.current.userClosed=true;this.disarm();this.send({type:'close'});this.current?.pc?.close();this.current?.ws.close()});
  }
}
