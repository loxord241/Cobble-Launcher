<div align="center">

<sub><b>English</b> · [Русский](README.ru.md) · [Українська](README.uk.md) · [Polski](README.pl.md)</sub>

<img src="src-tauri/icons/128x128.png" width="88" alt="Cobble Launcher">

# Cobble Launcher

**A fast and honest Minecraft launcher for Windows**

[![Version](https://img.shields.io/github/v/release/loxord241/Cobble-Launcher?style=flat-square&label=version)](https://github.com/loxord241/Cobble-Launcher/releases/latest)
[![License](https://img.shields.io/github/license/loxord241/Cobble-Launcher?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/Windows-10%2F11_x64-blue?style=flat-square&logo=windows)](https://github.com/loxord241/Cobble-Launcher/releases/latest)
[![Languages](https://img.shields.io/badge/languages-ru%C2%B7en%C2%B7uk%C2%B7pl-green?style=flat-square)](#languages)
[![Tests](https://img.shields.io/badge/tests-331%2B10-success?style=flat-square)](#building-from-source)

**[Download the latest release](https://github.com/loxord241/Cobble-Launcher/releases/latest)** ·
[Report an issue](https://github.com/loxord241/Cobble-Launcher/issues) ·
[Demo video](docs/anim-demo.mp4)
· [Cobble Launcher website](https://loxord241.github.io/Cobble-Launcher/)

<img src="docs/screenshots/home.png" alt="Modrinth catalog on the home page" width="820">

Tauri 2 · Rust · React 19 · TypeScript

</div>

---

Cobble is a desktop launcher with a native Rust core and a lightweight React
interface. No background browser engines, no "boosters" and no game injections:
official sources only, every file verified by hash, and the sodium stack for
optimization.

## 🗂 Modrinth catalog

- Catalog right on the home page: modpacks and mods, search, sorting
  (downloads / favorites / updates), loader filter, infinite scroll
- CurseForge-style project window: Overview / Changelog / Gallery / Versions
- Install modpacks into a new instance and mods into an existing one straight
  from the project window or the instance page; batch-update mods in one click

## 🎮 Instances

- Isolated instances: their own version, loader (Fabric / Quilt / Forge /
  NeoForge), mods and settings
- Download progress and live status right on the card; double-launch and
  busy-instance protection enforced at the core level
- Full control: RAM, Java version (Adoptium auto-install), JVM flag presets,
  Quick Play, per-instance profiles, renaming, duplication, backups and mod
  snapshots with rollback
- Worlds, screenshots and mod configs — straight from the instance page

## 👤 Profiles

- Microsoft (browser OAuth), Ely.by (authlib-injector) and offline profiles;
  offline ones are honestly marked as unlicensed
- Avatars: skin head for Microsoft / Ely.by, classic Steve for offline
- Refresh tokens are kept only in the Windows system keyring
- Discord Rich Presence: the instance name shows up in your Discord status

<div align="center">
  <table>
    <tr>
      <td><img src="docs/screenshots/instances.png" alt="Instances: cards with artwork and versions" width="420"></td>
      <td><img src="docs/screenshots/accounts.png" alt="Profile management: a real Steve for offline profiles" width="420"></td>
    </tr>
    <tr>
      <td><img src="docs/screenshots/instance.png" alt="Instance page: launch, tabs, statistics" width="420"></td>
      <td><img src="docs/screenshots/settings.png" alt="Settings: themes, language, memory" width="420"></td>
    </tr>
  </table>
</div>

## 🔄 Automatic updates

The launcher checks releases on its own (once per session, silently) and shows
an "Update available" badge. Installing takes one click: a signed update
(minisign), download progress, automatic relaunch. Your instances, profiles and
settings stay in place.

## 🛡 Security and privacy

- **Official sources only**: Mojang, Modrinth, Adoptium — every file is
  verified against the hash from the server. Mirrors without hash verification
  are forbidden by the project principles.
- **Zero game injections**: the launcher does not modify the Minecraft
  process. The only exception is authlib-injector for Ely.by, which is part of
  that service.
- **Secrets never leave the machine**: tokens live in the Windows keyring, and
  the logs automatically redact secrets.

## 🌍 Languages

Русский · English · Українська · Polski — picked during onboarding or in
settings, applied across the whole interface, error messages and date formats.

## ⬇️ Download

Ready-made installers (NSIS and MSI, Windows 10/11 x64) are on the
[releases](https://github.com/loxord241/Cobble-Launcher/releases/latest) page.
After installation updates arrive on their own; you can also use the "Check
for updates" button in settings.

## 🧑‍💻 Building from source

Requires Node.js 20+, Rust (stable) and npm.

```bash
npm install        # frontend dependencies
npm run tauri dev  # run in development mode
```

Release build (NSIS and MSI installers in `src-tauri/target/release/bundle/`):

```bash
npm run tauri build
```

Quality checks:

```bash
npm run build                # TypeScript strict + Vite
cd src-tauri && cargo test   # 331 core tests + 10 integration
```

## 🧱 Stack and structure

| Layer | Technologies |
|---|---|
| Interface | React 19, TypeScript strict, Tailwind 4 |
| Core | Rust, Tauri 2, tokio |
| Content | Modrinth API, Mojang manifests, Adoptium |

- `src/` — interface: pages, components, themes, i18n (ru, en, uk, pl)
- `src-tauri/src/` — core: instances and launching, hash-verified downloads,
  Modrinth, authentication, automatic updates
- `docs/` — [documentation map](docs/README.md):
  [core↔UI contract](docs/UI-CONTRACT.md),
  [decision registry](docs/DECISIONS.md), [milestone status](docs/PROGRESS.md)
- `scripts/` — acceptance tests: end-to-end UI checks, process lifecycle,
  Modrinth integration

## 📄 License and trademarks

The project code is licensed under **MIT** (see [LICENSE](LICENSE)); direct
dependencies and their licenses are listed in
[THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md), attribution and trademarks
in [NOTICE](NOTICE).

**Не официальный продукт Minecraft. Не связан с Mojang и Microsoft.**

**Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.**

**Не офіційний продукт Minecraft. Не пов'язаний з Mojang та Microsoft.**

**Nieoficjalny produkt Minecraft. Niepowiązany z Mojang i Microsoft.**

Minecraft is a trademark of Mojang Synergies AB. The names Mojang, Microsoft,
Xbox and Minecraft are used solely to describe compatibility; the launcher does
not distribute game files — it downloads them from Mojang's official servers
with hash verification.
