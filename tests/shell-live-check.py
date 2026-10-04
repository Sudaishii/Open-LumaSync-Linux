#!/usr/bin/env python3
"""Exercise the installed bridge and actual workers without another USB writer."""
import json
import subprocess
import time
from pathlib import Path
helper = str(Path.home() / '.local/bin/snzhy-backlight')

def call(command):
    reply = json.loads(subprocess.check_output([helper, command], text=True, timeout=15))
    if command != 'status':
        assert reply.get('accepted'), reply
    return reply

def wait(predicate):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        state = call('status')
        if not state.get('busy') and predicate(state):
            return state
        time.sleep(.2)
    raise AssertionError(state)

def pid():
    return subprocess.check_output(['systemctl','--user','show','snzhy-backlight.service','--property=MainPID','--value'],text=True).strip()

initial = wait(lambda s: s.get('ready') and s.get('open'))
initial_pid = pid()
call('screen')
wait(lambda s: s.get('screenRunning') and s.get('mode') == 'screen')
call('display:DP-1')
screen = wait(lambda s: s.get('output') == 'DP-1' and s.get('screenRunning') and 'fps' in s.get('activity',''))
print('Screen capture:', screen['activity'], flush=True)
call('brightness:90')
wait(lambda s: s.get('brightness') == 90 and s.get('screenRunning'))
call('stop')
wait(lambda s: not s.get('screenRunning') and not s.get('audioRunning') and not s.get('effectsRunning'))
call('audio')
audio = wait(lambda s: s.get('audioRunning') and s.get('mode') == 'audio' and '% signal' in s.get('activity',''))
print('Audio capture:', audio['activity'], flush=True)
call('show')
clients = json.loads(subprocess.check_output(['hyprctl','clients','-j'], text=True))
window = next(c for c in clients if c['pid'] == int(initial_pid))
subprocess.run(['hyprctl','dispatch','hl.dsp.window.close({ window = '+json.dumps('address:'+window['address'])+' })'],check=True,capture_output=True)
time.sleep(2)
wait(lambda s: s.get('audioRunning'))
assert pid() == initial_pid, 'closing the window stopped or replaced the service'
call('off')
wait(lambda s: not s.get('powered') and not s.get('audioRunning'))
call('on')
wait(lambda s: s.get('powered') and s.get('audioRunning'))
call('brightness:'+str(initial['brightness']))
wait(lambda s: s.get('brightness') == initial['brightness'])
if not call('status').get('resumeEnabled'):
    call('resume-toggle')
wait(lambda s: s.get('resumeEnabled'))
subprocess.run(['systemctl','--user','restart','snzhy-backlight.service'],check=True,timeout=20)
resumed = wait(lambda s: s.get('audioRunning') and s.get('resumeEnabled'))
assert resumed['audioStyle'] == initial['audioStyle'] and resumed['palette'] == initial['palette']
print('PASS: screen/audio workers, DP-1 selection, live brightness, Stop, Off/On, close-to-background, and saved audio resume after service restart.',flush=True)
