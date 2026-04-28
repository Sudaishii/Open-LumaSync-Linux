import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as QuickSettings from 'resource:///org/gnome/shell/ui/quickSettings.js';

const DBUS_NAME = 'com.synclights.gui';
const DBUS_PATH = '/com/synclights/gui';
const DBUS_IFACE = `
<node>
  <interface name="${DBUS_NAME}">
    <method name="TogglePower">
      <arg type="b" direction="out" name="powered"/>
    </method>
    <method name="SetBrightness">
      <arg type="y" direction="in" name="value"/>
    </method>
    <method name="GetStatus">
      <arg type="s" direction="out" name="status_json"/>
    </method>
    <method name="ShowWindow"/>
    <signal name="StatusChanged">
      <arg type="s" name="status_json"/>
    </signal>
  </interface>
</node>`;

const SyncLightsToggle = GObject.registerClass(
class SyncLightsToggle extends QuickSettings.QuickToggle {
  _init() {
    super._init({
      title: 'SyncLights',
      iconName: 'preferences-color-symbolic',
      toggleMode: true,
    });
    this.checked = false;
    this.connect('clicked', () => this._toggle());
  }

  _toggle() {
    try {
      GLib.spawn_command_line_async(
        `dbus-send --session --type=method_call --dest=${DBUS_NAME} ${DBUS_PATH} ${DBUS_NAME}.TogglePower`
      );
    } catch (e) {
      // If DBus not available, try launching the app
      GLib.spawn_command_line_async('synclights');
    }
  }
});

const SyncLightsSlider = GObject.registerClass(
class SyncLightsSlider extends QuickSettings.QuickSlider {
  _init() {
    super._init({
      iconName: 'display-brightness-symbolic',
    });
    this.slider.value = 119 / 255;
    this.slider.connect('notify::value', () => {
      const brightness = Math.round(this.slider.value * 255);
      try {
        GLib.spawn_command_line_async(
          `dbus-send --session --type=method_call --dest=${DBUS_NAME} ${DBUS_PATH} ${DBUS_NAME}.SetBrightness byte:${brightness}`
        );
      } catch (e) {}
    });
  }
});

const SyncLightsMenuToggle = GObject.registerClass(
class SyncLightsMenuToggle extends QuickSettings.QuickMenuToggle {
  _init() {
    super._init({
      title: 'SyncLights',
      iconName: 'preferences-color-symbolic',
      toggleMode: true,
    });

    this.menu.setHeader('preferences-color-symbolic', 'SyncLights');

    // Brightness slider item
    this._slider = new SyncLightsSlider();
    this.menu.addMenuItem(this._slider);

    // Open app item
    this.menu.addMenuItem(new QuickSettings.QuickSettingsItem({
      style_class: 'icon-button',
    }));

    this.connect('clicked', () => this._toggle());
  }

  _toggle() {
    try {
      GLib.spawn_command_line_async(
        `dbus-send --session --type=method_call --dest=${DBUS_NAME} ${DBUS_PATH} ${DBUS_NAME}.TogglePower`
      );
    } catch (e) {
      GLib.spawn_command_line_async('synclights');
    }
  }
});

export default class SyncLightsExtension {
  _indicator = null;

  enable() {
    this._indicator = new SyncLightsToggle();
    Main.panel.statusArea.quickSettings.addExternalIndicator(this._indicator);
  }

  disable() {
    if (this._indicator) {
      this._indicator.destroy();
      this._indicator = null;
    }
  }
}
