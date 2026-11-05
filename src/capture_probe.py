
import pathlib,stat,hashlib,json,time
out={'processes':0,'sockets':{},'libraries':[],'loader_error_in_log_tail':False}
for p in pathlib.Path('/proc').iterdir():
 if not p.name.isdigit():continue
 try:
  if (p/'comm').read_text().strip()=='kvm_vin':out['processes']+=1
 except OSError:pass
for name in ['vin_ctrl.sock','vin_video.sock','vin_snapshot.sock']:
 p=pathlib.Path('/run/kvm')/name
 try:out['sockets'][name]=stat.S_ISSOCK(p.stat().st_mode)
 except OSError:out['sockets'][name]=False
for package,alias in [('libgssapi-krb5-2','libgssapi_krb5.so.2'),('libharfbuzz0b','libharfbuzz.so.0'),('libglib2.0-0t64','libgobject-2.0.so.0')]:
 row={'name':alias,'matches_package':None}
 try:
  p=(pathlib.Path('/usr/lib/arm-linux-gnueabihf')/alias).resolve(strict=True)
  manifest=pathlib.Path('/var/lib/dpkg/info')/(package+':armhf.md5sums')
  expected=dict((line.split(None,1)[1],line.split(None,1)[0]) for line in manifest.read_text().splitlines())
  key=str(p).lstrip('/')
  if key in expected:row['matches_package']=hashlib.md5(p.read_bytes()).hexdigest()==expected[key]
 except (OSError,ValueError):pass
 out['libraries'].append(row)
p=pathlib.Path('/var/log/kvmcomm/kvm_vin.log')
try:
 with p.open('rb') as f:
  f.seek(0,2);size=f.tell();f.seek(max(0,size-8192));tail=f.read()
 out['loader_error_in_log_tail']=b'Inconsistency detected by ld.so' in tail
 out['log_age_seconds']=max(0,round(time.time()-p.stat().st_mtime))
except OSError:out['log_age_seconds']=None
def read(p):
 try:return pathlib.Path(p).read_text().strip()
 except OSError:return None
out['boot_id']=read('/proc/sys/kernel/random/boot_id')
try:out['uptime_seconds']=float(read('/proc/uptime').split()[0])
except (AttributeError,ValueError):out['uptime_seconds']=None
out['network']={}
for name in ['usb0','wlan0']:
 root=pathlib.Path('/sys/class/net')/name
 out['network'][name]={'state':read(root/'operstate'),'carrier':read(root/'carrier')}
print(json.dumps(out))
