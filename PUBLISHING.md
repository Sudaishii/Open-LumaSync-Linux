# Publish your project on GitHub

The project repository is **[Sudaishii/Open-LumaSync-Linux](https://github.com/Sudaishii/Open-LumaSync-Linux)**. A useful GitHub description is:

> Linux alternative for Robobloq SyncLight USB backlights, with screen/audio sync, custom effects and a dedicated Omarchy bar plugin.

Suggested topics: `linux`, `omarchy`, `hyprland`, `robobloq`, `synclight`, `ambient-lighting`, `audio-visualizer`, `tauri`, `rust`.

If you prefer a shorter future app name, options include **LumaSync**, **OpenBacklight**, **GlowSync** and **DeskGlow**. This publication keeps the current application identity. Regardless of naming, retain the openLightsSync credit and CC BY-NC-SA 4.0 license: this is your derivative project, with upstream components credited.

## Create a clean source copy

From the working project:

```sh
python3 scripts/export-source.py
```

This creates `dist/snzhy-OpenSycnlights-source.zip` and `.tar.gz`. They include app code, the locked Rust dependencies, documentation, tests and packaging sources. They exclude Git history/remotes, compiled binaries, caches, local design-session notes, desktop screenshots and personal settings. The public README image is an app-only browser-test capture.

Extract a source archive into a **new folder** for publication. The development checkout was adapted from upstream and may still have an upstream `origin`; a clean extracted copy avoids accidentally pushing to the wrong repository.

## Publish with GitHub's website

1. Create a new repository in your own GitHub account named `snzhy-OpenSycnlights`.
2. Add the description above and choose public visibility if you want anyone to download it.
3. Upload the extracted source files, keeping their folder structure. Include `.github/`, `.gitignore`, LICENSE and ATTRIBUTION.md.
4. Commit the upload. GitHub displays README.md as the project homepage.
5. Update the clone example in INSTALL.md with your repository's actual owner and URL.

GitHub's website may not preserve executable bits for uploaded shell files. Prefer the Git route below. For a web-uploaded checkout, users can run `bash install.sh --omarchy`; make `install.sh`, `run.sh` and `packaging/omarchy/install.sh` executable before using their direct `./…` commands.

## Publish with Git

In the **newly extracted folder**, use your own repository URL:

```sh
git init -b main
git add .
git commit -m "Initial snzhy-OpenSycnlights Linux controller"
git remote add origin https://github.com/YOUR_USERNAME/snzhy-OpenSycnlights.git
git push -u origin main
```

Create an empty repository on GitHub first, and replace `YOUR_USERNAME`. This creates a new public copy; it does not modify or publish the development checkout's upstream repository. Source archive executable permissions are preserved by Git when added locally.

Before pushing, inspect `git status` and `git diff --cached --stat`, confirm the intended repository with `git remote -v`, and check the clone example uses the correct owner. The GitHub workflow performs Rust, installer and browser checks; it has not been run on GitHub until the repository is published.

## First source release

After the published checks pass, create a release such as `v0.1.0` with the notes in CHANGELOG.md. Attach the source ZIP/tarball if desired. Describe it as a **source release**, with installation through `./install.sh --omarchy` or `./install.sh` after system dependencies and USB access are configured.

Do not advertise an AUR package, verified `.deb` or AppImage download until one has actually been built, tested and published. GitHub hosts the project and downloads; control of the physical light runs locally on Linux, not inside a GitHub Pages website.
