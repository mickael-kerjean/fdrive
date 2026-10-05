import {Extension} from "resource:///org/gnome/shell/extensions/extension.js";
import * as Main from "resource:///org/gnome/shell/ui/main.js";

import {Indicator} from "./lib/index.js";

export default class FdriveExtension extends Extension {
    enable() {
        this.indicator = new Indicator();
        Main.panel.statusArea.quickSettings.addExternalIndicator(this.indicator.systemIndicator);
    }

    disable() {
        this.indicator?.destroy();
        this.indicator = null;
    }
}
