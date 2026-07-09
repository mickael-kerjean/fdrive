# What is fdrive?

A cross platform drive client that does not try to own your storage, but rather connects to it wherever it already lives. From S3 and SFTP to FTP, NFS, SMB, IPFS, Azure, Google Cloud, and beyond, it is powered by <a href="https://github.com/mickael-kerjean/filestash">Filestash</a>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-sync-windows.png" alt="windows screenshot" />
    <em>Windows screenshot</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-sync-apple.png" alt="apple screenshot" />
    <em>Apple screenshot</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-sync-android.png" alt="android screenshot" />
    <em>Android screenshot</em>
</p>

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-sync-linux.png" alt="linux screenshot">
    <em>Linux screenshot</em>
</p>

## Architecture

We use the hexagonal architecture / ports and adapters pattern. The core owns all policy, everything that decides *what moves where* lives there, once. Each platform adapts its own UI and filesystem technology to it.

| crate | technology |
|---|---|
| `fdrive-core` | `Engine` (ledger, conflict rules, upload scheduler), the `LocalTree` port, the Filestash HTTP sdk |
| `fdrive-linux` | FUSE, GTK |
| `fdrive-windows` | Win32, CfAPI, ReadDirectoryChangesW, IShellWindows |
| `fdrive-mac` | fuse-t |
| `fdrive-ios` | FileProvider |
| `fdrive-android` | Storage Access Framework (Kotlin wire, UniFFI) |

Two adapter families:

- **we are the filesystem** (linux, android, ios): online-first, listings answered live, content cached on open, edits pushed back by the core's scheduler; nothing durable beyond that cache.
- **the system owns the replica** (windows, mac): placeholders materialize on demand; the only durable local state is the spool of unpushed edits.

The model in one line: the filesystems are the state, the ledger is the memory of where local and remote last agreed — a single sqlite file, updated row by row — dirty is the debt owed upward, and conflicts are detected by comparing the server against that memory. Dirty wins locally; deletes and renames are verdicts (server first, a failure vetoes) that wait out any in-flight upload of the paths they touch, while uploads step aside for them — no orphans on the server, no deadlocks, by construction; a conflicting upload diverts to a "(conflicted copy)"; an unreadable ledger quarantines the cache instead of pruning it. The bytes move with the same care: transfers stream both ways in constant memory whatever the file size, uploads run four at a time and a file edited mid-flight simply goes again, a cold file opened twice is downloaded once, and on the mounts that serve reads directly the first byte lands after one round-trip, reads riding the download as it streams in. When the server is unreachable, listings fall back to the last thing you saw and edits keep queueing for its return.
