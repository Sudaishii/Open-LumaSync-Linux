class SpectrumPicker {
  constructor(container, onChange) {
    this.onChange = onChange;
    this.h = 0; this.s = 1; this.v = 1;
    this._draggingSpectrum = false;
    this._draggingHue = false;

    this.el = document.createElement('div');
    this.el.className = 'spectrum-picker';

    // Spectrum canvas (saturation x value)
    this.specCanvas = document.createElement('canvas');
    this.specCanvas.className = 'spectrum-canvas';
    this.specCanvas.width = 256;
    this.specCanvas.height = 256;
    this.specCtx = this.specCanvas.getContext('2d');

    this.specCursor = document.createElement('div');
    this.specCursor.className = 'spectrum-cursor';

    const specWrap = document.createElement('div');
    specWrap.className = 'spectrum-wrap';
    specWrap.appendChild(this.specCanvas);
    specWrap.appendChild(this.specCursor);

    // Hue strip
    this.hueCanvas = document.createElement('canvas');
    this.hueCanvas.className = 'hue-strip';
    this.hueCanvas.width = 20;
    this.hueCanvas.height = 256;
    this.hueCtx = this.hueCanvas.getContext('2d');

    this.hueCursor = document.createElement('div');
    this.hueCursor.className = 'hue-cursor';

    const hueWrap = document.createElement('div');
    hueWrap.className = 'hue-wrap';
    hueWrap.appendChild(this.hueCanvas);
    hueWrap.appendChild(this.hueCursor);

    // Preview + hex
    this.preview = document.createElement('div');
    this.preview.className = 'picker-preview';

    this.hexInput = document.createElement('input');
    this.hexInput.className = 'hex-input';
    this.hexInput.type = 'text';
    this.hexInput.maxLength = 7;
    this.hexInput.value = '#ff0000';

    const infoRow = document.createElement('div');
    infoRow.className = 'picker-info';
    infoRow.appendChild(this.preview);
    const hexLabel = document.createElement('span');
    hexLabel.className = 'hex-label';
    hexLabel.textContent = '#';
    infoRow.appendChild(hexLabel);
    infoRow.appendChild(this.hexInput);

    // RGB inputs
    this.rInput = this._makeRgbInput('R');
    this.gInput = this._makeRgbInput('G');
    this.bInput = this._makeRgbInput('B');
    const rgbRow = document.createElement('div');
    rgbRow.className = 'picker-rgb';
    rgbRow.append(
      this._label('R'), this.rInput,
      this._label('G'), this.gInput,
      this._label('B'), this.bInput
    );

    this.el.appendChild(specWrap);
    this.el.appendChild(hueWrap);

    const rightCol = document.createElement('div');
    rightCol.className = 'picker-right';
    rightCol.appendChild(infoRow);
    rightCol.appendChild(rgbRow);
    this.el.appendChild(rightCol);

    container.appendChild(this.el);

    this._drawHueStrip();
    this._drawSpectrum();
    this._updateCursors();

    this._bindEvents(specWrap, hueWrap);
    this._emit();
  }

  _label(text) {
    const l = document.createElement('span');
    l.className = 'rgb-label';
    l.textContent = text;
    return l;
  }

  _makeRgbInput(ch) {
    const inp = document.createElement('input');
    inp.type = 'number'; inp.min = 0; inp.max = 255;
    inp.className = 'rgb-input';
    inp.addEventListener('change', () => {
      const r = +this.rInput.value, g = +this.gInput.value, b = +this.bInput.value;
      this.setRGB(r, g, b);
    });
    return inp;
  }

  _bindEvents(specWrap, hueWrap) {
    const specDown = (e) => { this._draggingSpectrum = true; this._specMove(e); };
    const hueDown = (e) => { this._draggingHue = true; this._hueMove(e); };

    specWrap.addEventListener('pointerdown', specDown);
    hueWrap.addEventListener('pointerdown', hueDown);

    document.addEventListener('pointermove', (e) => {
      if (this._draggingSpectrum) this._specMove(e);
      if (this._draggingHue) this._hueMove(e);
    });
    document.addEventListener('pointerup', () => {
      this._draggingSpectrum = false;
      this._draggingHue = false;
    });

    this.hexInput.addEventListener('change', () => {
      let hex = this.hexInput.value.trim();
      if (!hex.startsWith('#')) hex = '#' + hex;
      if (/^#[0-9a-fA-F]{6}$/.test(hex)) {
        const r = parseInt(hex.slice(1,3),16);
        const g = parseInt(hex.slice(3,5),16);
        const b = parseInt(hex.slice(5,7),16);
        this.setRGB(r, g, b);
      }
    });
  }

  _specMove(e) {
    const rect = this.specCanvas.getBoundingClientRect();
    const x = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const y = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
    this.s = x;
    this.v = 1 - y;
    this._updateCursors();
    this._emit();
  }

  _hueMove(e) {
    const rect = this.hueCanvas.getBoundingClientRect();
    const y = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
    this.h = y;
    this._drawSpectrum();
    this._updateCursors();
    this._emit();
  }

  _drawHueStrip() {
    const ctx = this.hueCtx;
    const w = this.hueCanvas.width, h = this.hueCanvas.height;
    for (let y = 0; y < h; y++) {
      const hue = y / h;
      ctx.fillStyle = `hsl(${hue * 360}, 100%, 50%)`;
      ctx.fillRect(0, y, w, 1);
    }
  }

  _drawSpectrum() {
    const ctx = this.specCtx;
    const w = this.specCanvas.width, h = this.specCanvas.height;
    const hueColor = `hsl(${this.h * 360}, 100%, 50%)`;

    // base hue
    ctx.fillStyle = hueColor;
    ctx.fillRect(0, 0, w, h);

    // white gradient left->right
    const gw = ctx.createLinearGradient(0, 0, w, 0);
    gw.addColorStop(0, 'rgba(255,255,255,1)');
    gw.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = gw;
    ctx.fillRect(0, 0, w, h);

    // black gradient top->bottom
    const gb = ctx.createLinearGradient(0, 0, 0, h);
    gb.addColorStop(0, 'rgba(0,0,0,0)');
    gb.addColorStop(1, 'rgba(0,0,0,1)');
    ctx.fillStyle = gb;
    ctx.fillRect(0, 0, w, h);
  }

  _updateCursors() {
    const sw = this.specCanvas.clientWidth || 256;
    const sh = this.specCanvas.clientHeight || 256;
    this.specCursor.style.left = (this.s * sw - 7) + 'px';
    this.specCursor.style.top = ((1 - this.v) * sh - 7) + 'px';

    const hh = this.hueCanvas.clientHeight || 256;
    this.hueCursor.style.top = (this.h * hh - 4) + 'px';
  }

  _emit() {
    const rgb = hsvToRgb(this.h, this.s, this.v);
    this.rInput.value = rgb.r;
    this.gInput.value = rgb.g;
    this.bInput.value = rgb.b;
    const hex = '#' + [rgb.r, rgb.g, rgb.b].map(v => v.toString(16).padStart(2,'0')).join('');
    this.hexInput.value = hex;
    this.preview.style.background = hex;
    if (this.onChange) this.onChange(rgb);
  }

  setRGB(r, g, b) {
    const hsv = rgbToHsv(r, g, b);
    this.h = hsv.h; this.s = hsv.s; this.v = hsv.v;
    this._drawSpectrum();
    this._updateCursors();
    this._emit();
  }

  getRGB() {
    return hsvToRgb(this.h, this.s, this.v);
  }
}

function hsvToRgb(h, s, v) {
  let r = 0, g = 0, b = 0;
  const i = Math.floor(h * 6);
  const f = h * 6 - i;
  const p = v * (1 - s);
  const q = v * (1 - f * s);
  const t = v * (1 - (1 - f) * s);
  switch (i % 6) {
    case 0: r = v; g = t; b = p; break;
    case 1: r = q; g = v; b = p; break;
    case 2: r = p; g = v; b = t; break;
    case 3: r = p; g = q; b = v; break;
    case 4: r = t; g = p; b = v; break;
    case 5: r = v; g = p; b = q; break;
  }
  return {
    r: Math.round(r * 255),
    g: Math.round(g * 255),
    b: Math.round(b * 255)
  };
}

function rgbToHsv(r, g, b) {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const d = max - min;
  let h = 0, s = max === 0 ? 0 : d / max, v = max;
  if (d !== 0) {
    switch (max) {
      case r: h = ((g - b) / d + (g < b ? 6 : 0)) / 6; break;
      case g: h = ((b - r) / d + 2) / 6; break;
      case b: h = ((r - g) / d + 4) / 6; break;
    }
  }
  return { h, s, v };
}
