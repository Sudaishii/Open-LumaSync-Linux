const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const { availableMonitors, currentMonitor } = window.__TAURI__.window;

function $(id) { return document.getElementById(id); }
const brightnessEl = $('brightness'), brightnessVal = $('brightnessVal');
const powerToggle = $('powerToggle');
const fadeMsEl = $('fadeMs');
const ledIndexEl = $('ledIndex'), setLedBtn = $('setLed');
const logEl = $('log');
const deviceStatus = $('deviceStatus');
const secLeft = $('secLeft'), secTop = $('secTop'), secRight = $('secRight');
const totalLedsEl = $('totalLeds'), saveSectionsBtn = $('saveSections');
const effectSelect = $('effectSelect');
const effectSpeed = $('effectSpeed'), effectSpeedVal = $('effectSpeedVal');
const startEffectBtn = $('startEffect');
const connectBtn = $('connectBtn');

let currentTotalLeds = 71;
let currentRgb = { r: 255, g: 0, b: 0 };
let activeMode = null;
let activeEffect = null;

// Titlebar window controls
const appWindow = getCurrentWindow();
$('titleMinimize').onclick = () => appWindow.minimize();
$('titleMaximize').onclick = async () => {
  (await appWindow.isMaximized()) ? appWindow.unmaximize() : appWindow.maximize();
};
$('titleClose').onclick = () => appWindow.hide();

// Debounce helpers
let _colorTimer = null;
let _briTimer = null;

function sendColor() {
  clearTimeout(_colorTimer);
  _colorTimer = setTimeout(() => {
    const apply = activeEffect === 'static' || activeMode === null;
    invoke('update_global_color', { r: currentRgb.r, g: currentRgb.g, b: currentRgb.b, apply }).catch(() => {});
  }, 50);
}

function sendBrightness() {
  clearTimeout(_briTimer);
  _briTimer = setTimeout(() => {
    invoke('set_brightness', { value: +brightnessEl.value }).catch(() => {});
  }, 50);
}

// Stop all modes
async function stopAll() {
  try { await invoke('effects_stop', {}); } catch (e) {}
  activeMode = null;
  activeEffect = null;
  updateActiveIndicators();
}

function updateActiveIndicators() {
  $('effectsCard').classList.toggle('card-active', activeMode === 'effect');
  if ($('audioCard')) $('audioCard').classList.toggle('card-active', activeMode === 'audio');
  if ($('screenSyncCard')) $('screenSyncCard').classList.toggle('card-active', activeMode === 'screensync');
}

// Spectrum picker
const picker = new SpectrumPicker($('pickerContainer'), (rgb) => {
  currentRgb = rgb;
  sendColor();
});

function log(...args) {
  logEl.textContent += args.join(' ') + '\n';
  logEl.scrollTop = logEl.scrollHeight;
}

async function cmd(name, args) {
  try {
    const result = await invoke(name, args);
    log(name, '->', JSON.stringify(result));
    return result;
  } catch (e) {
    log(name, 'ERROR:', e);
    throw e;
  }
}

brightnessEl.addEventListener('input', () => {
  brightnessVal.textContent = brightnessEl.value;
  sendBrightness();
});

effectSpeed.addEventListener('input', () => {
  effectSpeedVal.textContent = effectSpeed.value;
  if (activeMode === 'effect' && activeEffect !== 'static') {
    restartCurrentEffect();
  }
});

let _speedRestartTimer = null;
function restartCurrentEffect() {
  clearTimeout(_speedRestartTimer);
  _speedRestartTimer = setTimeout(() => {
    cmd('effects_start', {
      name: effectSelect.value,
      ledCount: currentTotalLeds,
      speed: +effectSpeed.value,
    }).catch(() => {});
  }, 200);
}

// Power toggle with debounce
let isPoweredOn = false;
let _powerBusy = false;
powerToggle.onclick = async () => {
  if (_powerBusy) return;
  _powerBusy = true;
  try {
    if (isPoweredOn) {
      await stopAll();
      await cmd('power_off_fade', {
        currentBrightness: +brightnessEl.value, durationMs: +fadeMsEl.value || 300,
        section: 1, r: currentRgb.r, g: currentRgb.g, b: currentRgb.b
      });
      isPoweredOn = false;
    } else {
      await cmd('power_on', {
        section: 1, r: currentRgb.r, g: currentRgb.g, b: currentRgb.b
      });
      isPoweredOn = true;
    }
    powerToggle.classList.toggle('on', isPoweredOn);
  } finally {
    setTimeout(() => { _powerBusy = false; }, 500);
  }
};

setLedBtn.onclick = () => cmd('set_single_led', {
  index: +ledIndexEl.value, r: currentRgb.r, g: currentRgb.g, b: currentRgb.b
});

// Sections
function updateTotalLeds() {
  currentTotalLeds = (+secLeft.value || 0) + (+secTop.value || 0) + (+secRight.value || 0);
  totalLedsEl.textContent = '= ' + currentTotalLeds + ' LEDs';
}
secLeft.addEventListener('input', updateTotalLeds);
secTop.addEventListener('input', updateTotalLeds);
secRight.addEventListener('input', updateTotalLeds);
saveSectionsBtn.onclick = () => cmd('set_sections', {
  sections: [+secLeft.value, +secTop.value, +secRight.value]
});

// Mode — start stops everything else; changing mode restarts
startEffectBtn.onclick = async () => {
  const name = effectSelect.value;
  await cmd('effects_start', {
    name,
    ledCount: currentTotalLeds,
    speed: +effectSpeed.value,
  });
  activeMode = 'effect';
  activeEffect = name;
  updateActiveIndicators();
};

effectSelect.addEventListener('change', () => {
  if (activeMode === 'effect') {
    startEffectBtn.click();
  }
});

// Audio sync
const audioMode = $('audioMode');
const audioSensitivity = $('audioSensitivity');
const audioSensVal = $('audioSensVal');
const audioStart = $('audioStart');
const audioSource = $('audioSource');

audioSensitivity.addEventListener('input', () => {
  audioSensVal.textContent = (audioSensitivity.value / 10).toFixed(1);
});
audioSensVal.textContent = (audioSensitivity.value / 10).toFixed(1);

// Load audio sources
(async () => {
  try {
    const sources = await invoke('list_audio_sources', {});
    for (const [name, label] of sources) {
      const opt = document.createElement('option');
      opt.value = name;
      opt.textContent = label;
      audioSource.appendChild(opt);
    }
  } catch (e) { log('list_audio_sources error', e); }
})();

audioStart.onclick = async () => {
  const src = audioSource.value || null;
  await cmd('audio_start', {
    mode: audioMode.value,
    sensitivity: +audioSensitivity.value / 10,
    source: src
  });
  activeMode = 'audio';
  activeEffect = null;
  updateActiveIndicators();
};

// Screen Sync
const ambiFps = $('ambiFps');
const ambiFpsVal = $('ambiFpsVal');
const ambiStart = $('ambiStart');

ambiFps.addEventListener('input', () => { ambiFpsVal.textContent = ambiFps.value; });

ambiStart.onclick = async () => {
  await cmd('ambilight_start', { fps: +ambiFps.value });
  activeMode = 'screensync';
  activeEffect = null;
  updateActiveIndicators();
};

// Settings overlay
const settingsBtn = $('settingsBtn');
const settingsOverlay = $('settingsOverlay');
const settingsClose = $('settingsClose');

settingsBtn.onclick = () => settingsOverlay.classList.remove('hidden');
settingsClose.onclick = () => settingsOverlay.classList.add('hidden');
settingsOverlay.addEventListener('click', (e) => {
  if (e.target === settingsOverlay) settingsOverlay.classList.add('hidden');
});

// Section visibility toggles
const visibilityMap = {
  showEffects: 'effectsCard',
  showAudio: 'audioCard',
  showScreenSync: 'screenSyncCard',
};

const defaultVisibility = {
  showEffects: true,
  showAudio: false,
  showScreenSync: false,
};

function loadVisibility() {
  try {
    const saved = JSON.parse(localStorage.getItem('ols_visibility') || 'null');
    const state = saved || defaultVisibility;
    for (const [checkId, cardId] of Object.entries(visibilityMap)) {
      const checkbox = $(checkId);
      const card = $(cardId);
      if (!card || !checkbox) continue;
      const visible = state[checkId] !== undefined ? state[checkId] : defaultVisibility[checkId];
      checkbox.checked = visible;
      card.style.display = visible ? '' : 'none';
    }
  } catch (e) {}
}

function saveVisibility() {
  const state = {};
  for (const checkId of Object.keys(visibilityMap)) {
    state[checkId] = $(checkId).checked;
  }
  localStorage.setItem('ols_visibility', JSON.stringify(state));
}

for (const [checkId, cardId] of Object.entries(visibilityMap)) {
  const checkbox = $(checkId);
  if (!checkbox) continue;
  checkbox.addEventListener('change', () => {
    const card = $(cardId);
    if (card) card.style.display = checkbox.checked ? '' : 'none';
    saveVisibility();
    fitWindow();
  });
}
loadVisibility();

// Device
connectBtn.onclick = () => cmd('connect_device', {});

// Init — load sections
(async () => {
  try {
    const data = await invoke('get_sections', {});
    if (data.sections && data.sections.length === 3) {
      secLeft.value = data.sections[0];
      secTop.value = data.sections[1];
      secRight.value = data.sections[2];
      currentTotalLeds = data.totalLeds;
      totalLedsEl.textContent = '= ' + currentTotalLeds + ' LEDs';
    }
  } catch (e) { log('getSections error', e); }
})();

// Auto-resize window to content, respecting usable screen area
async function fitWindow() {
  try {
    const monitor = await currentMonitor();
    if (!monitor) return;
    const scale = monitor.scaleFactor || 1;
    const screenW = monitor.size.width / scale;
    const screenH = monitor.size.height / scale;
    const reservedTop = 32;
    const reservedBottom = 48;
    const maxH = screenH - reservedTop - reservedBottom;

    await new Promise(r => setTimeout(r, 50));
    const contentH = document.querySelector('.window-frame').scrollHeight;
    const targetH = Math.min(contentH + 2, maxH);
    const targetW = Math.min(680, screenW - 40);

    const { LogicalSize } = window.__TAURI__.window;
    await appWindow.setSize(new LogicalSize(targetW, targetH));
  } catch (e) {}
}
setTimeout(fitWindow, 300);

// Poll device status
setInterval(async () => {
  try {
    const s = await invoke('device_status', {});
    deviceStatus.textContent = s.open ? 'Connected' : (s.found ? 'Found' : 'Disconnected');
    deviceStatus.className = 'status ' + (s.open ? 'connected' : 'disconnected');
    if (s.open && !isPoweredOn) {
      isPoweredOn = true;
      powerToggle.classList.add('on');
    } else if (!s.open) {
      isPoweredOn = false;
      powerToggle.classList.remove('on');
    }
  } catch (e) {}
}, 3000);
