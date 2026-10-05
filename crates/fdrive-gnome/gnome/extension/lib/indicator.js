import Clutter from "gi://Clutter";
import GLib from "gi://GLib";
import Gio from "gi://Gio";
import St from "gi://St";

import * as Main from "resource:///org/gnome/shell/ui/main.js";
import * as PopupMenu from "resource:///org/gnome/shell/ui/popupMenu.js";
import {QuickMenuToggle, SystemIndicator} from "resource:///org/gnome/shell/ui/quickSettings.js";

import * as Icons from "../icons/index.js";
import * as Fdrive from "./fdrive.js";

export class Indicator {
    constructor() {
        const serverRow = new PopupMenu.PopupBaseMenuItem({reactive: false});
        const tokenRow = new PopupMenu.PopupBaseMenuItem({reactive: false});
        const transfersHeader = new PopupMenu.PopupSeparatorMenuItem("Recent transfers");
        const transfersScroll = new St.ScrollView({style: "max-height: 16em", hscrollbar_policy: St.PolicyType.NEVER, overlay_scrollbars: true});
        this.systemIndicator = new SystemIndicator();
        this._icon = this.systemIndicator._addIndicator();
        this._toggle = new QuickMenuToggle({title: "FDrive", toggleMode: true});
        this._off = new PopupMenu.PopupMenuItem("Turn on FDrive to sync your files", {reactive: false, can_focus: false});
        this._error = new PopupMenu.PopupImageMenuItem("", "dialog-warning-symbolic", {reactive: false});
        this._login = new PopupMenu.PopupMenuSection();
        this._server = new St.Entry({hint_text: "Server, then Enter to sign in", x_expand: true, style: "background-color: rgba(0, 0, 0, 0.25)"});
        this._token = new St.PasswordEntry({hint_text: "Token, then Enter to connect", x_expand: true, style: "background-color: rgba(0, 0, 0, 0.25)"});
        this._account = new PopupMenu.PopupMenuSection();
        this._host = new PopupMenu.PopupMenuItem("");
        this._folder = new PopupMenu.PopupMenuItem("");
        this._transfers = new PopupMenu.PopupMenuSection();
        this._clear = new St.Button({label: "Clear", y_align: Clutter.ActorAlign.CENTER});
        this._transferList = new PopupMenu.PopupMenuSection();

        serverRow.add_child(this._server);
        tokenRow.add_child(this._token);
        transfersHeader.add_child(this._clear);
        this.systemIndicator.quickSettingsItems.push(this._toggle);
        this._login.addMenuItem(serverRow);
        this._login.addMenuItem(tokenRow);
        this._account.addMenuItem(this._host);
        this._account.addMenuItem(this._folder);
        this._transfers.addMenuItem(transfersHeader);
        transfersScroll.set_child(this._transferList.actor);
        this._transfers.actor.add_child(transfersScroll);
        for (const item of [this._off, this._error, this._login, this._account, this._transfers]) {
            this._toggle.menu.addMenuItem(item);
        }

        this._toggle.connect("clicked", () => _do(Fdrive.setActive(this._toggle.checked), this._refresh));
        this._server.clutter_text.connect("activate", () => _do(Fdrive.loginUrl(this._server.text.trim()).then(url => Gio.AppInfo.launch_default_for_uri(url, null)), this._refresh));
        this._token.clutter_text.connect("activate", () => _do(Fdrive.login(this._server.text.trim(), this._token.text.trim()), this._refresh));
        this._host.connect("activate", () => _do(Fdrive.logout(), this._refresh));
        this._folder.connect("activate", () => Gio.AppInfo.launch_default_for_uri(`file://${this._mount}`, null));
        this._clear.connect("clicked", () => _do(Fdrive.clear(), this._refresh));
        this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 3, () => {
            this._refresh();
            return GLib.SOURCE_CONTINUE;
        });
        this._refresh();
    }

    destroy() {
        GLib.source_remove(this._timer);
        this._timer = 0;
        this._toggle.destroy();
        this.systemIndicator.destroy();
    }

    _refresh = async () => {
        const status = await Fdrive.status();
        if (this._timer) this._render(status);
    };

    _render(status) {
        const icon = Icons.phaseIcon(status.phase);
        const signedIn = status.phase !== "stopped" && status.phase !== "loggedOut";
        const subtitle = status.phase === "syncing" ? `${status.phaseText} · ${status.rate}` : status.phaseText;
        const transfers = signedIn ? status.transfers.slice(0, 50) : [];
        const transfersKey = JSON.stringify(transfers);

        this._mount = status.mount;
        this._icon.gicon = icon;
        this._toggle.set({gicon: icon, subtitle: status.phaseText, checked: status.phase !== "stopped"});
        this._toggle.menu.setHeader(icon, "Filestash Drive", subtitle);
        this._error.label.text = status.lastError;
        this._off.visible = status.phase === "stopped";
        this._error.visible = status.lastError !== "";
        this._login.actor.visible = status.phase === "loggedOut";
        this._account.actor.visible = signedIn;
        this._host.label.clutter_text.set_markup(_keyValue("Server", status.host));
        this._folder.label.clutter_text.set_markup(_keyValue("Folder", status.mount));

        this._transfers.actor.visible = transfers.length > 0;
        if (transfersKey === this._transfersKey) return;
        this._transfersKey = transfersKey;
        this._transferList.removeAll();
        for (const transfer of transfers) {
            const item = this._transferList.addAction(
                "",
                () => {
                    Main.panel.closeQuickSettings();
                    Gio.AppInfo.launch_default_for_uri(transfer.uri, null);
                },
                Icons.transferIcon(transfer),
            );
            item.label.clutter_text.set_markup(_markup(transfer.name, transfer.detail));
        }
    }
}

async function _do(action, refresh) {
    try {
        await action;
    } catch (e) {
        Main.notifyError("Filestash Drive", e.message);
    }
    refresh();
}

function _markup(title, subtitle) {
    return `${_escape(title)}\n<small>${_escape(subtitle)}</small>`;
}

function _keyValue(key, value) {
    return `<span alpha="60%">${_escape(key)}</span> ${_escape(value)}`;
}

const _escape = text => GLib.markup_escape_text(text, -1);
