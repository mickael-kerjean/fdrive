Dropbox democratised the idea of a folder that syncs across your devices. Fdrive delivers on that promise and extends it [beyond humans to agents](https://github.com/mickael-kerjean/fdrive/blob/master/crates/fdrive-docker/README.md). If you're looking for the Dropbox experience on Omarchy, here's what the code in this repo gives you:

<p align="center">
    <img src="https://downloads.filestash.app/img/app-filestash-www-img-screenshots-fdrive-omarchy.png" alt="Linux screenshot">
    <em>Screenshot</em>
</p>

To install, run:
```
omarchy plugin add https://github.com/mickael-kerjean/fdrive-omarchy.git --enable
```

*Note:* For testing, you can use [https://demo.filestash.app](https://demo.filestash.app) and our read-only WebDAV instance at `https://webdav.filestash.app`.

## Why?

Omarchy needs better sync options that let you stay in control of your data, fetch files on demand through a virtual filesystem, work offline, and sync changes in the background.

Like Dropbox, but with your own storage. It tooks almost 20 years to happen since BrandonM famously said:

<img src="https://raw.githubusercontent.com/mickael-kerjean/filestash_images/master/.assets/hn.png" />

Yes, Brandon, you can use any storage you want with this Omarchy plugin, not just FTP! These days, you might prefer SFTP and already be using [SSHFS](https://en.wikipedia.org/wiki/SSHFS) to mount your files locally. If that works for you, stick with it unless you need any of the following:

- an awesome GUI
- offline capabilities
- delta sync
- conflict handling
- a [driver to share your data with agents](https://github.com/mickael-kerjean/fdrive/blob/master/crates/fdrive-docker/README.md)
- a [plugin-based architecture](https://github.com/mickael-kerjean/filestash#plugins): you can extend everything with features like auditing and compliance [and many more options](https://www.filestash.app/docs/plugin)

## Where is this going?

As the CEO of NVIDIA said:

> when you deployed an agent ... the first thing you do is you take away all of its rights ... then you provision, you give it access to files ...

Fdrive, along with Filestash, provides that provisioning tool, making sure your agents only have access to what they need, with clear permission boundaries and controls.

Have fun!
