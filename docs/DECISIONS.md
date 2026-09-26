# DECISIONS

Каждое отклонение от спеки — с причиной. Реальность побеждает спеку, но не молча.

---

## D1. Rust установлен в сессии M0

Спека предполагала, что Rust может отсутствовать («проверить в начале и установить при
отсутствии»). Установлен rustup stable 1.98.1, target x86_64-pc-windows-msvc; MSVC Build
Tools 14.51 и Windows SDK уже были на машине. Отклонения нет — зафиксировано как факт.

## D2. Tauri CLI: npm (@tauri-apps/cli) как основной, cargo tauri-cli установлен

Критерий M0 в спеке — «cargo tauri dev». Установлены оба варианта: `@tauri-apps/cli`
(npm, используется в `npm run tauri …` и в скриптах) и `cargo tauri-cli ^2` (для
`cargo tauri dev` из спеки). Оба читают один `tauri.conf.json`.

## D3. README в docs/ + указатель в корне

Спека (§4.2) помещает README.md в `docs/`. Корневой README — короткий указатель на
`docs/README.md`, чтобы GitHub-визиты не терялись. Это не расширение scope.

## D4. CLI-бинарник `mcl` (доказательство M1)

Спека M1 требует «тестовая команда реально скачивает latest релиз и запускает игру».
Реализовано как `src/bin/mcl.rs` (`mcl launch`, `mcl java-scan`) поверх тех же доменных
модулей, что будет использовать UI. Оркестрация переедет в `instances/run.rs` на M2
без изменения доменов. Добавлен модуль `util/` (атомарные записи, zip-slip, имена,
FNV) — в списке §4.2 его нет, но это общие помощники, а не новый домен.

## D5. Mojang перешли на годовую нумерацию версий

Latest release на 2026-09-26 — id `26.3` (годовая схема), а не 1.21.x, как предполагала
спека. Конвейер версия-агностичен (id берётся из манифеста); fallback-минимумы Java
(§6.4) расширены случаем `major > 1 → Java 21`. Никаких изменений форматов не
потребовалось.

## D6. Действие правила `disallow` в старых версиях

Реальный JSON 1.12.2 содержит `"action": "disallow"` (старая семантика), спека знала
только allow/deny. `RuleAction::Deny` получил `#[serde(alias = "disallow")]`.

## D7. Log4j-конфиг: подстановка `${path}`

Шаблон `logging.client.argument` использует `${path}` (подтверждено спекой §6.1.8,
но не было в substitution-мапе — на первой приёмке это дало пустой
`-Dlog4j.configurationFile=` и лог без INFO). Исправлено; супервизор M1 ждёт 8 с
после «Sound engine started», потому что атласы логируются позже звука.

## D8. Adoptium API: singular `binary`

Спека §5 даёт URL `assets/latest/{major}/hotspot`, но реальный ответ — массив записей,
в каждой ОДИН бинарник в поле `binary` (не `binaries[]`). Модель — по реальности.
SHA256 из `package.checksum` сверяется всегда (спека допускала сверку размера —
хэш честнее).

## D9. Корзина: store-поддеревья удаляются напрямую

Крейт `trash` и PowerShell `VisualBasic.FileIO` падают («Неверный уровень системного
вызова») на каталогах с тысячами hardlinks. Решение: при удалении инстанса shared-контент
(libraries/versions/bin) удаляется напрямую — данные остаются в сторе и скачаются
заново при необходимости; в корзину ОС идёт только уникальное (saves/mods/config/
instance.json) через `[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory`.

## D10. Движок загрузок — на план запуска

Очередь `queue.json` больше не подмешивается в новый запуск автоматически: старые
Failed-задачи блокировали все последующие запуски. Валидные на диске файлы и так
пропускаются мгновенно (сверка хэша), а восстановление прерванной очереди станет
отдельным UI-потоком («продолжить прерванную загрузку») на M4+.

## D11. NeoForge: инсталлятор требует launcher_profiles.json

Headless `--installClient` отказывается работать без «вы запускали ванильный лаунчер».
Создаём минимальный `{"profiles": {}, "settings": {}, "version": 3}` в каталоге игры
перед установкой.

## D12. `\\?\`-пути и прямые слэши

`long_path` добавляет `\\?\` для путей ≥240 символов, но этот префикс отключает
нормализацию Win32: maven rel_path с `/` внутри давал os error 123 (реальный кейс —
`listenablefuture-9999.0-empty-to-avoid-conflict-with-guava.jar`). long_path теперь
нормализует `/` → `\` при префиксовании.

## D13. Интеграция NeoForge в запуск (4 находки приёмки)

1. **Дедуп библиотек по имени с `@jar`**: NeoForge пишет gradle-координаты
   (`org.apache.logging.log4j:log4j-core:2.22.1@jar`), без нормализации один jar
   попадает в classpath дважды → `Duplicate key` в UnionFileSystem. Дедуп и
   maven_path отрезают суффикс `@…`.
2. **Universal/client jar вне version JSON**: инсталлятор кладёт их в свой
   maven-каталог (`net/neoforged/neoforge/<v>/`); официальный лаунчер добавляет их в
   classpath сам (маркер — `-DignoreList` с префиксами `neoforge-`, `client-extra`).
   Мы добавляем все jar из каталога загрузчика.
3. **`-DlibraryDirectory` → каталог библиотек ИНСТАНСА**: FML ищет через него
   maven-артефакты (client srg/extra, universal); в сторе их нет — они в
   game_dir/libraries от инсталлятора. `-p` из JSON тоже резолвится оттуда.
4. **`--mavenRoot` + `--mods`**: мод neoforge (universal) обнаруживается FML только
   через игровые аргументы (FMLServiceProvider, joptsimple: «Maven root directories»,
   «List of mods to add»). Формат: `--mods net.neoforged:neoforge:<v>:universal`.
   `-DlegacyClassPath` не используется — нужен FML только в dev-режиме, а дублирование
   classpath в аргументах роняет спавн на Windows (os error 206, лимит 32K).

## D14. Отладочный CDP-порт в tauri.conf.json (снять к M9)

Для автоматизированной приёмки UI (клики мышкой через CDP) в `app.windows[0]`
добавлен `additionalBrowserArgs: "--remote-debugging-port=9223"` и dev-зависимость
`playwright-core` (скрипт `scripts/m4_acceptance.cjs`). В RELEASE-сборку не
пропускать: порт отладки на localhost — риск (спека §11). M9 обязан удалить аргумент
из конфига.


## D15. Кейс JSON: Modrinth API — snake_case, mrpack — camelCase, settings — camelCase

Modrinth REST API v2 отдаёт snake_case (`project_id`), а формат .mrpack
(modrinth.index.json) — camelCase (`formatVersion`, `fileSize`); settings.json по
спеке §7 — camelCase. Первая реализация предполагала camelCase в API — приёмка на
реальных ответах это опровергла. Settings: Rust-поля snake_case +
`rename_all = "camelCase"`; найденный на приёмке M4/M5 рассинхрон (UI-поля
игнорировались) устранён.

## D16. Установка модов в vanilla-инстанс: предпочитаем fabric

Без фильтра по загрузчику Modrinth возвращает первую попавшуюся сборку (для
Sodium это neoforge). Для vanilla-инстансов выбираем fabric-сборку (наиболее
распространённая), fallback — любая совместимая.


## D17. Стек оптимизации: реальные project_id и честные пропуски

Первоначальные id из памяти оказались неверными (Lithium → 404); все восемь id
сверены с API по slug. Мод без совместимой версии для пары (mc, loader) пропускается
с записью в лог — стек подстраивается под новые версии MC (ModernFix для 26.3+fabric
на момент приёмки ещё не вышел). Версии модов не хардкодятся (спека §6.9).


## D18. Forge: координата maven с MC-префиксом

promotions_slim.json отдаёт версию Forge без MC (`47.4.10`), но maven-путь
требует `{mc}-{version}` (`net/minecraftforge/forge/1.20.1-47.4.10/…`). Голая
версия даёт 404 — найдено первой приёмкой M7. version_id инстанса —
`1.20.1-forge-47.4.10`.

## D19. Общий headless-инсталлятор Forge-семейства

NeoForge и Forge используют один путь: shim `launcher_profiles.json` →
`--installClient <game_dir>` → перенос version JSON в versions/ инстанса.
Логика вынесена в `loaders::run_headless_installer` (была приватной в
neoforge).


## D20. Redaction через реестр секретов в аппендере

Вместо хрупких regex на «похоже на токен» — глобальный реестр: каждый полученный
access/refresh токен регистрируется в `util::redact`, file-логгер заворачивает
запись в RedactingWriter, который заменяет вхождения на `[REDACTED]` ДО записи
на диск. Токены длиной <8 символов не регистрируются (шум).

## D21. ely.by authlib-injector: GitHub API digest

Спека: «хэш со страницы релиза». GitHub API для releases/latest отдаёт
`assets[].digest` (`sha256:…`) — это и есть машинно-читаемый хэш релиза; без
digest скачивание запрещается (правило §3).
