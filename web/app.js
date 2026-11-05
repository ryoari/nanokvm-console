'use strict';
const state={token:null,mode:null};
async function api(path){
  const response=await fetch(path,{signal:AbortSignal.timeout(40000)});
  let value;
  try {value=await response.json()} catch {throw new Error('The workbench returned an unreadable response. Retry Check connection.')}
  if(!response.ok)throw new Error(value.detail||'The request failed. Retry Check connection.');return value;
}
const consoleWorkbench=new ConsoleWorkbench(api,()=>state);
consoleWorkbench.setAvailability('Opening the private local session…');
document.getElementById('consoleRead').addEventListener('click',()=>{document.getElementById('diagnostics').open=true});
(async()=>{
  const launch=location.hash.slice(1);history.replaceState(null,'',location.pathname);
  try{
    const response=await fetch('/api/session',{method:'POST',headers:{'X-Launch-Token':launch}});
    if(!response.ok)throw new Error('Open the workbench with nanokvm-console --open.');
    Object.assign(state,await response.json());
    const vendor=document.getElementById("vendorLink");
    if(vendor&&state.vendor_url){vendor.href=state.vendor_url;vendor.hidden=false}
    consoleWorkbench.setAvailability();
    document.getElementById('mode').textContent=state.mode==='live'?'Local Go+':'Synthetic demo';
    consoleWorkbench.say('Select Connect. Input will stay disabled until you enable it.');
  }catch(e){consoleWorkbench.setAvailability(e.message)}
})();
