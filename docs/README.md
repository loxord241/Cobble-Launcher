# mc-launcher-v2

Полнофункциональный лаунчер Minecraft: vanilla + Fabric + Quilt + NeoForge +
Forge, Modrinth (моды, модпаки .mrpack, обновления), Microsoft-аккаунты,
ely.by, импорт инстансов.

Приоритеты: **честность и безопасность** (SHA1/SHA256 каждого скачанного файла,
ноль инжекций в игру — единственное исключение authlib-injector при явном выборе
ely.by, секреты только в Windows Credential Manager) → **скорость** (нативный
Tauri-бинарник, очередь загрузок с 16 параллельными соединениями, hardlink-стор)
→ **полнота** (поддержка старых версий вплоть до legacy-ассетов) → **сменяемость
UI** (весь бизнес в Rust-ядре, UI — тонкий слой поверх IPC).

## Сборка

```powershell
# разработка (окно + HMR)
npm run tauri dev

# полная проверка качества (tsc strict, clippy -D warnings, тесты)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1

# release-бинарник; инсталлятор NSIS: добавить -Bundle
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build.ps1
```

Требования: Node LTS, Rust stable (`x86_64-pc-windows-msvc`), MSVC Build Tools,
WebView2 (есть в Windows 11).

## Microsoft-вход (MSA)

1. Зарегистрируйте приложение на [portal.azure.com](https://portal.azure.com):
   Microsoft Entra ID → Регистрация приложений → Новая:
   - «Поддерживаемые типы учетных записей»: **Personal Microsoft accounts only**;
   - платформа **Мобильные и классические приложения** (public client).
2. Authentication → **Разрешить общедоступные потоки клиента = Да** (иначе
   device-code вернёт AADSTS70002).
3. Application (client) ID → Настройки лаунчера → `azureClientId`.
   ID не секретен (у Prism лежит публично), но в git его не коммитим — он живёт
   в локальном `settings.json`. Client secret НЕ нужен.
4. API permissions вручную не добавлять: `XboxLive.signin offline_access`
   запрашивается при входе.

Вход: аккаунт-меню лаунчера → Microsoft → введите код на
<https://microsoft.com/link>. Refresh-токен хранится в Credential Manager.

## CLI

`src-tauri/target/debug/mcl.exe` — тонкая обёртка над тем же ядром:

```text
mcl launch --mc latest --player MCL2     # vanilla-конвейер, запуск до меню
mcl instance-create / instance-list / instance-launch / instance-delete
mcl loader-install --loader fabric|quilt|neoforge|forge
mcl mrpack-install --file pack.mrpack    # модпак Modrinth в новый инстанс
mcl instance-import --file pack.zip      # CF-zip / MultiMC / свой формат
mcl auth-offline / auth-msa-start / auth-msa-poll / auth-list
mcl java-scan / java-install --major 17
mcl store-cleanup                        # удалить объекты стора без ссылок
```

## Документация

- [docs/PROGRESS.md](PROGRESS.md) — статус M0–M9 + стабилизация 2026-09-27, доказательства приёмки.
- [docs/DECISIONS.md](DECISIONS.md) — реестр решений (D1–D30) с причинами (расхождения со спекой).
- [docs/UI-CONTRACT.md](UI-CONTRACT.md) — контракт «ядро ↔ UI» (сменяемость UI).

## Лицензия

MIT — см. [LICENSE](../LICENSE) в корне репозитория и `license = "MIT"` в
`src-tauri/Cargo.toml`. Копилефт-требований нет: код можно использовать,
изменять и распространять при сохранении копирайта и текста лицензии.

Minecraft © Mojang Studios; проект не связан с Mojang/Microsoft и не
распространяет файлы игры — только скачивает с официальных серверов по
сверенным хэшам (дисклеймер о брендах — в корневом
[README](../README.md#лицензия-и-бренды)).

Third-party: Tauri, React, Tailwind, Modrinth API, Adoptium JRE,
authlib-injector (лицензия в репозитории проекта). Прямые зависимости и их
лицензии — в [THIRD-PARTY-LICENSES.md](../THIRD-PARTY-LICENSES.md),
атрибуция и товарные знаки — в [NOTICE](../NOTICE).
