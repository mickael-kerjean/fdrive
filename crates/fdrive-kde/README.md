<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-kde.png" alt="linux screenshot">
    <em>Screenshot</em>
</p>

```
sudo curl -fsSL https://downloads.filestash.app/latest/fdrive-linux-x86_64.bin -o /usr/local/bin/fdrive
sudo chmod +x /usr/local/bin/fdrive

mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/fdrive.service << 'EOF'
[Unit]
Description=Filestash Drive

[Service]
ExecStart=/usr/local/bin/fdrive daemon
Restart=on-failure

[Install]
WantedBy=default.target
EOF
systemctl --user enable --now fdrive

git clone --depth 1 https://github.com/mickael-kerjean/fdrive /tmp/fdrive
kpackagetool6 -t Plasma/Applet -i /tmp/fdrive/crates/fdrive-kde/kde/plasmoid
```

Then right click the panel, `Add Widgets...`, and add `Filestash Drive`.
