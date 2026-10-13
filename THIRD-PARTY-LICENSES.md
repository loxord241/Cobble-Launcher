# THIRD-PARTY-LICENSES

Сторонние компоненты Cobble Launcher (mc-launcher-v2) и их лицензии.

> Полные тексты лицензий — в репозиториях пакетов; здесь перечислены ПРЯМЫЕ
> зависимости (из `src-tauri/Cargo.toml` и `package.json`), транзитивные
> унаследованы от них. Версии — по `src-tauri/Cargo.lock` и `package-lock.json`
> на момент составления (2026-09-27); актуальный список всегда можно получить
> командами `cargo tree` / `npm ls --all`.

Лицензии указаны так, как их объявляют сами пакеты (поле `license` в их
манифестах). Использование любой из альтернатив в двойной лицензии
(`OR`) — на выбор распространителя; проект использует MIT-вариант там, где он
есть.

## Rust: прямые зависимости (`src-tauri/Cargo.toml`)

| Пакет | Версия | Лицензия |
|---|---|---|
| `tauri` | 2.12.0 | Apache-2.0 OR MIT |
| `tauri-build` (build-dep) | 2.7.0 | Apache-2.0 OR MIT |
| `tauri-plugin-opener` | 2.5.5 | Apache-2.0 OR MIT |
| `tauri-plugin-dialog` | 2.8.0 | Apache-2.0 OR MIT |
| `tauri-plugin-updater` | 2.13.1 | Apache-2.0 OR MIT |
| `tauri-plugin-process` | 2.4.0 | Apache-2.0 OR MIT |
| `tauri-plugin-single-instance` | 2.5.2 | Apache-2.0 OR MIT |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 |
| `tokio` | 1.53.1 | MIT |
| `reqwest` | 0.13.5 | MIT OR Apache-2.0 |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 |
| `tracing` | 0.1.44 | MIT |
| `tracing-subscriber` | 0.3.23 | MIT |
| `tracing-appender` | 0.2.5 | MIT |
| `sha1` | 0.11.0 | MIT OR Apache-2.0 |
| `sha2` | 0.11.0 | MIT OR Apache-2.0 |
| `md-5` | 0.11.0 | MIT OR Apache-2.0 |
| `hex` | 0.4.3 | MIT OR Apache-2.0 |
| `uuid` | 1.26.1 | Apache-2.0 OR MIT |
| `dirs` | 7.0.0 | MIT OR Apache-2.0 |
| `regex` | 1.13.1 | MIT OR Apache-2.0 |
| `futures` | 0.3.34 | MIT OR Apache-2.0 |
| `zip` | 8.6.0 | MIT |
| `flate2` | 1.1.10 | MIT OR Apache-2.0 |
| `rand` | 0.10.3 | MIT OR Apache-2.0 |
| `clap` | 4.6.7 | MIT OR Apache-2.0 |
| `sysinfo` | 0.39.6 | MIT |
| `base64` | 0.23.1 | MIT OR Apache-2.0 |
| `keyring` | 4.2.0 | MIT OR Apache-2.0 |
| `axum` (dev-dep) | 0.8.9 | MIT |
| `tempfile` (dev-dep) | 3.27.0 | MIT OR Apache-2.0 |

Примечание: крейт `opener` снят (D29) — открытие путей/ссылок выполняет
`tauri-plugin-opener` (см. строки плагина и `@tauri-apps/plugin-opener` выше).

## npm: прямые зависимости (`package.json`)

| Пакет | Версия | Лицензия |
|---|---|---|
| `react` | 19.3.0 | MIT |
| `react-dom` | 19.3.0 | MIT |
| `zustand` | 5.0.15 | MIT |
| `lucide-react` | 1.48.0 | ISC |
| `@tauri-apps/api` | 2.12.0 | Apache-2.0 OR MIT |
| `@tauri-apps/plugin-dialog` | 2.8.0 | MIT OR Apache-2.0 |
| `@tauri-apps/plugin-opener` | 2.5.5 | MIT OR Apache-2.0 |
| `@tauri-apps/plugin-updater` | 2.13.1 | MIT OR Apache-2.0 |
| `@tauri-apps/plugin-process` | 2.4.0 | MIT OR Apache-2.0 |
| `tailwindcss` (dev) | 4.3.3 | MIT |
| `@tailwindcss/vite` (dev) | 4.3.3 | MIT |
| `vite` (dev) | 8.3.1 | MIT |
| `@vitejs/plugin-react` (dev) | 6.1.1 | MIT |
| `typescript` (dev) | 6.0.3 | Apache-2.0 |
| `@tauri-apps/cli` (dev) | 2.11.5 | Apache-2.0 OR MIT |
| `playwright-core` (dev) | 1.63.0 | Apache-2.0 |
| `@types/react` (dev) | 19.3.0 | MIT |
| `@types/react-dom` (dev) | 19.3.0 | MIT |

## MPL-2.0 зависимости (транзитивные)

Эти крейты приходят транзитивно (собственного кода проекта не содержат и не
модифицируются):

- `dom_query` 0.28.0 ← `tauri-utils`, `wry`; далее `html5ever` (MIT/Apache-2.0),
  `selectors` 0.38.0 (MPL-2.0), `cssparser` 0.37.0 (MPL-2.0),
  `cssparser-macros` 0.7.1 (MPL-2.0), `dtoa-short` 0.3.5 (MPL-2.0);
- `option-ext` 0.2.0 (MPL-2.0) ← `dirs-sys` ← `dirs` (прямая зависимость проекта).

| Пакет | Версия | Лицензия |
|---|---|---|
| `cssparser` | 0.37.0 | MPL-2.0 |
| `cssparser-macros` | 0.7.1 | MPL-2.0 |
| `dtoa-short` | 0.3.5 | MPL-2.0 |
| `option-ext` | 0.2.0 | MPL-2.0 |
| `selectors` | 0.38.0 | MPL-2.0 |

MPL-2.0 — файловый копилефт: **используются без модификаций** (в том виде, в
каком опубликованы в crates.io), поэтому обязательства MPL-2.0 ограничиваются
самими файлами этих библиотек; исходники доступны в их репозиториях и на
crates.io. Код проекта (MIT) от MPL-2.0 не наследует обязательств.

## Загружается во время работы (НЕ входит в дистрибутив)

| Компонент | Когда | Лицензия |
|---|---|---|
| Eclipse Temurin JRE (Adoptium) | по запросу `java_install` — скачивается в `runtime/` | GPL-2.0 WITH Classpath-exception-2.0 |
| authlib-injector | только при явном входе через ely.by | см. репозиторий проекта authlib-injector |
| Моды, шейдеры, ресурспаки, модпаки | по выбору пользователя (Modrinth) | по лицензии каждого проекта |
| Файлы игры, библиотеки, ассеты | из официальных серверов Mojang | EULA Minecraft |

Лаунчер не распространяет файлы игры и сторонние артефакты в своём
установщике: всё перечисленное скачивается с источников по сверенным хэшам.

## Товарные знаки

Minecraft, Mojang, Microsoft, Xbox — товарные знаки соответствующих
правообладателей (Minecraft — Mojang Synergies AB). Проект не является
официальным продуктом Minecraft и не связан с Mojang/Microsoft — см.
[NOTICE](NOTICE).
