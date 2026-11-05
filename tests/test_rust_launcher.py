"""Rust launcher ownership and private browser handoff. No hardware access."""
import json
import os
from pathlib import Path
import subprocess
import time

ROOT=Path(__file__).resolve().parents[1]

def test_second_open_reuses_owner_without_exposing_capability(tmp_path):
    binary=ROOT/'target/release/nanokvm-console'
    state=tmp_path/'state'
    launcher=tmp_path/'xdg-open'
    opened=tmp_path/'opened'
    launcher.write_text('#!/bin/sh\nprintf "%s" "$1" > "$NANOKVM_OPEN_RESULT"\n')
    launcher.chmod(0o700)
    env={**os.environ,'PATH':str(tmp_path)+':'+os.environ['PATH'],'NANOKVM_OPEN_RESULT':str(opened)}
    command=[str(binary),'--demo','--port','0','--state-dir',str(state)]
    server=subprocess.Popen(command,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
    try:
        path=state/'launch.json'
        end=time.monotonic()+10
        while not path.exists() and time.monotonic()<end:
            assert server.poll() is None
            time.sleep(.05)
        before=path.read_bytes();record=json.loads(before)
        assert record['pid']==server.pid and record['implementation']=='rust'
        assert path.stat().st_mode&0o777==0o600
        duplicate=subprocess.run(command,env=env,text=True,capture_output=True,timeout=10)
        assert duplicate.returncode==1 and path.read_bytes()==before
        wrong_mode=subprocess.run([str(binary),'--open','--state-dir',str(state)],env=env,text=True,capture_output=True,timeout=10)
        assert wrong_mode.returncode==1 and not opened.exists()
        other_profile=subprocess.run(command+['--open','--profile',str(tmp_path/'another-profile.json')],env=env,text=True,capture_output=True,timeout=10)
        assert other_profile.returncode==1 and not opened.exists()
        reopened=subprocess.run(command+['--open'],env=env,text=True,capture_output=True,timeout=10)
        assert reopened.returncode==0 and path.read_bytes()==before
        end=time.monotonic()+3
        while not opened.exists() and time.monotonic()<end:time.sleep(.05)
        assert opened.read_text()==str(state/'launch.html')
        assert record['url'] not in reopened.stdout+reopened.stderr+opened.read_text()
        assert server.poll() is None
    finally:
        server.terminate();server.wait(timeout=10)
        server.stderr.close()
    assert not (state/'launch.json').exists()
    assert not (state/'launch.html').exists()
