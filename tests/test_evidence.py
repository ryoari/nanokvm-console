"""Synthetic contract cases. They are not physical observations."""
import json
from pathlib import Path
import subprocess

GUARD=Path(__file__).resolve().parents[1]/'target/release/console_guard'

def observation():
    return dict(sourceDigest='0'*64,currentDigest='0'*64,device='fixture.local',expectedDevice='fixture.local',targetActiveBefore=True,targetActiveAfter=True,heldKeysAfter=[],memoryBefore=False,runningBefore=False,memoryAfter=False,runningAfter=False,framesBefore=1,framesAfter=2,width=640,height=480,red=[200,0,0],green=[0,200,0],blue=[0,0,200],expectedText='test',receivedText='test',keyPresses=4,keyReleases=4,clicksBefore=0,clicksAfter=1,mouseX=100,mouseY=100,expectedX=100,expectedY=100,closed=True,reconnectFrames=1,reconnectArmed=False)

def check(value):
    return subprocess.run([str(GUARD),'--verify-evidence'],input=json.dumps(value),text=True,capture_output=True).returncode

def test_contract_accepts_complete_synthetic_case():assert check(observation())==0

def test_contract_rejects_inactive_target_or_held_key_or_armed_recovery():
    for key,value in [('targetActiveBefore',False),('targetActiveAfter',False),('heldKeysAfter',[4]),('reconnectArmed',True),('framesAfter',1),('keyReleases',3),('receivedText','wrong')]:
        fixture=observation();fixture[key]=value;assert check(fixture)==1
