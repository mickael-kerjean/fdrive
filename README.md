## What is this?

Dropbox democratised the idea of a folder that syncs across your devices. Fdrive not only matches that promise, but also extends it beyond humans to agents, without creating yet another data silo and by staying interoperable with the storage you already have, from S3 and SFTP to SMB, NFS, the [infamous FTP](https://github.com/mickael-kerjean/filestash#why) and virtually anything.

## Who is it for? Why?

1. Humans: you want the Dropbox experience while staying in control of your data and you want something that does not create its own island but is interoperable

2. Agents: as the CEO of Nvidia say: « when you deployed an agent ... the first thing you do is you take away all of its rights ... then you provision, you give it access to files ...». Fdrive along with [Filestash](https://github.com/mickael-kerjean/filestash) is that provisioning tool that will make sure your agent only have access to what it needs, you set clear permission boundaries, with audit trail showing what the agent tried to do, what was allowed, and what was blocked

## What it looks like?

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-mac.png?v=20260831" alt="mac screenshot" />
    <em>Mac</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-windows.png?v=20260831" alt="windows screenshot" />
    <em>Windows</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-linux.png?v=20260929" alt="linux screenshot">
    <em>Linux GTK</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-omarchy.png" alt="linux screenshot">
    <em>Linux Omarchy</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-iphone.png" alt="mac screenshot" />
    <em>Iphone</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-android.png" alt="android screenshot" />
    <em>Android</em>
</p>

```
docker plugin install machines/fdrive --alias fdrive
export FILESTASH_SERVER=https://demo.filestash.app
export FILESTASH_TOKEN=uKzArshpw49Pta2tJZmg1mywkHcmimpW4lCjtVDNTbUFpmN0W2PXajSRR_fA5VrRr4Ks1S5SHwn9YffL74qRrVr1jssRUCXp4_uZdItrYUhQegWAGh5xT45-DgHowJb5aFtO-nODOMpFa6Y84Sit7za3GyM1miEpYm0wWgVucCs4tA==
docker compose -f - up <<EOF
services:
  agent:
    image: alpine
    command: ["ls", "-la", "/mnt"]
    volumes:
    - files:/mnt

volumes:
  files:
    driver: fdrive
    driver_opts:
      server: ${FILESTASH_SERVER}
      token: ${FILESTASH_TOKEN}
EOF
```

## Release

- Windows: <a href="https://downloads.filestash.app/latest/Filestash.exe">exe</a>
- Mac: <a href="https://downloads.filestash.app/latest/Filestash.dmg">silicon</a>
- Linux: <a href="https://downloads.filestash.app/latest/Filestash-x86.bin">amd64</a>
- Iphone: apple store (coming soon)
- Android: <a href="https://downloads.filestash.app/latest/Filestash.apk">apk</a>, play store (coming soon)

## Architecture

We use the hexagonal architecture / ports and adapters pattern. The core owns all policy, everything that decides *what moves where* lives there, once. Each platform adapts its own UI and filesystem technology to it.

| crate | technology |
|---|---|
| `fdrive-core` | `model` (the sync vocabulary: `Operation`, `Plan`, `Fate`), `engine` (the journal and its state, plan replay, conflict rules, cache policy), the `LocalStore` port, the Filestash HTTP sdk |
| `fdrive-linux` | FUSE, daemon + CLI |
| `fdrive-gtk` | GTK, FUSE |
| `fdrive-windows` | CfAPI, Win32 |
| `fdrive-mac` | FileProvider, Swift |
| `fdrive-ios` | FileProvider, Swift |
| `fdrive-android` | Storage Access Framework, Kotlin |
| `fdrive-docker` | Docker volume plugin API, FUSE |
| `fdrive-omarchy` | QML, FUSE |

## Features

- [X] Sane Architecture: one Rust core makes every decision and each platform only implements the lipstick, so sync behaves the same everywhere
- [X] Delta sync: only ship the bytes that changed, not the whole file
- [X] Performance: if another sync solution is faster on your workload, send us a reproducible case. We’ll treat it as a bug
- [X] Lives in the tray: the tray icon shows the status, synced, syncing or in trouble at a glance
- [X] No polling or periodic sweep shenanigans
- [X] Files on demand: a file only downloads when you open it, with both content and listings cached so browsing stays snappy and the next open is instant
- [X] Streaming: large files open immediately, reads are served as the bytes arrive
- [X] Offline mode: cached files stay readable and editable, changes upload once the link returns
- [X] Conflict handling: your work is never lost, when both sides changed you get a `(conflicted copy)` so both versions survive
- [X] Coalesced uploads: the journal folds editor save dances and rapid edits into the fewest server operations, and retries back off instead of hammering the server
- [X] Thumbnails: they are generated on the server through fine tuned C code that works fast!
- [X] Safe deletes: removes and renames carry a lease, they only apply if the server still holds the version you last saw, so nothing you have not seen can ever be destroyed
- [X] Crash safe: unpushed edits survive crashes and restarts
- [X] Reset friendly: rage deleting the local cache partially or entirely is not undefined behavior
- [X] Live view: changes made elsewhere show up in the folder you are browsing, no manual refresh
- [X] Pinning: mark a folder always available offline (`setfattr -n user.pin -v always <dir>` on linux)
- [X] Ignore list: `node_modules`, `.DS_Store` and friends stay home by default, adjustable via `fdrive.toml`
- [X] Login: done through your server's own login page, password, LDAP, SSO, 2FA all just work
- [X] No Electron: native everything from the tray, filesystem integration into one single binary
- [ ] Add P2P transfer
- [X] handle hundreds of thousands of files
- [X] Linux GTK
- [X] Linux Omarchy
- [ ] Linux Gnome
- [ ] Linux KDE Plasma
- [ ] handles millions of files
- [ ] add support for more conflict strategies
- [ ] Profiles: connect to several servers / accounts in the same time
- [ ] Deep integration with Filestash for file locks
- [ ] Deep integration with Filestash for file versioning
- [ ] Deep integration with Filestash for search
- [ ] Explorer actions: surface share links and friends right from the file manager
- [X] MacOS FileProvider: ~~we are using fuse-t temporarly until we have actual apple hardware~~
- [ ] Testing: test on all possible devices / configuration
- [X] Support for delta download: same as the existing upload but for download. Awaiting for server support
- [X] Docker Volume Driver: give any container, AI agent included, a plain folder backed by your S3, SFTP, FTP, IPFS, AzureBlob, Sharepoint, etc...
- [ ] MDM integration: preconfigure the client and roll it out across a fleet
- [ ] full POSIX compliance
- [ ] finetune performance
- [ ] Ransomware protection
