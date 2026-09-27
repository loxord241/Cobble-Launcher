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

## D22. UI «Сланец»: тема = CSS-переменные через data-theme

Дизайн ui-explorations/slate-pro.html перенесён в реальный React-UI без
структурных изменений (вердикт владельца: «Сланец» во всех темах). Тема —
только палитра: атрибут `data-theme` на `<html>` (dark — по умолчанию, без
атрибута; light, cb-dark, cb-light, mc). Палитры 1-в-1 из slate-pro.html.
Хранение — localStorage (`theme`), приоритет при старте: `?theme=` в URL >
localStorage > dark. Тема применяется синхронно в main.tsx до createRoot
(без мигания). settings.theme ядра (dark|light) продолжает работать: он
применяется, только если localStorage ещё не выбран; выбор Тёмной/Светлой
на странице «Темы» синхронизируется обратно в ядро через settings_set.
Премиум-темы (Гроза, Хвоя, Кармин, Янтарь) — некликабельные карточки с
замком; кнопка «Подписаться» disabled (URL подписки не согласован, фейковой
оплаты нет). Ограничение: border-strong (рамки карточек/кнопок) на
поверхности даёт <3:1 — это 1-в-1 значения эталона; интерактив различим
текстом (≥4.5:1) и акцентом (≥3:1), рамка не единственный носитель.

## D23. Кастомный титлбар: decorations:false + разрешения окна

Титлбар 52px свой (бренд, поиск, чип аккаунта, кнопки окна) — в
tauri.conf.json `decorations: false`. Кнопки — `@tauri-apps/api/window`
(minimize/toggleMaximize/close), двойной клик по титлбару = maximize
(встроенный обработчик data-tauri-drag-region, permission
`core:window:allow-internal-toggle-maximize` из core:default). Для кнопок
добавлены явные разрешения в src-tauri/capabilities/default.json
(minimize/toggle-maximize/close/is-maximized/start-dragging) — это
конфигурация ACL, не Rust-код. Перетаскивание — data-tauri-drag-region
на титлбаре и его пассивных детях; на кнопках/инпуте атрибута нет.

## D24. Шрифты офлайн: системные стеки вместо Inter

Прототип использует Inter (Google Fonts) — лаунчер обязан выглядеть
корректно без интернета, поэтому сетевые шрифты НЕ подключаются:
UI — "Segoe UI", system-ui, sans-serif; mono — "Cascadia Mono",
"JetBrains Mono", Consolas, monospace.

## D25. Instance: snake_case от ядра → camelCase на границе фронтенда

Rust-структура `Instance` сериализуется в snake_case (`mc_version`), а
контракт docs/UI-CONTRACT.md и типы фронтенда требуют camelCase — из-за
рассинхрона версии инстансов в UI были пустыми. Rust-ядро не трогалось:
нормализация ключей добавлена в `api/client.ts` (camelize на выходе,
snakify для instance_settings_set на входе). Правильное долгосрочное
решение — `#[serde(rename_all = "camelCase")]` в ядре.

Статус (2026-09-27): закрыто. Правильный фикс выполнен — `Instance`
и события ядра сериализуются в camelCase (`instances/mod.rs` —
`rename_all = "camelCase"` в c62334f/d2b2e7b; `events.rs` —
`rename_all_fields = "camelCase"` в 72c1c4e). Фронтовые трансформеры
(camelize/snakify/snakeToCamel) из `api/client.ts` удалены; invoke
нормализует ошибки IPC в `Error` (d2b2e7b). Контракт ядро↔UI теперь
camelCase на обоих концах.

## D26. Управление контентом, бэкап и экспорт инстанса (2026-09-27)

Закрыты два P2-гэпа из AUDIT (Prism/Modrinth App parity):

1. **Вкл/выкл контента** — паттерн Prism: переименование файла на диске
   `mods/x.jar` ↔ `mods/x.jar.disabled` (загрузчики его игнорируют) + флаг
   `enabled` в content-manifest. Логический путь в манифесте не меняется,
   обновления работают по `project_id` независимо от состояния. Путь из UI
   валидируется: только `mods|resourcepacks|shaderpacks`, только вниз,
   без `..` и `:` (traversal — ошибка `invalid_input`).
2. **Экспорт .mrpack** (формат Modrinth): `files[]` — записи манифеста
   с `url`+`sha1` (поле `url` добавлено в ContentEntry, пишется при
   установке; старые записи и Local-контент уходят в `overrides/`).
   Конфиги/опции включаются, **сейвы — нет** (личное и тяжёлое) — в отличие
   от бэкапа zip, куда сейвы входят. Шум (`logs`, `crash-reports`, `cache`)
   не входит никуда; `.lock` — в бэкап не входит.
3. **Бэкап zip** — весь каталог инстанса под корнем `instance/` в
   `%LOCALAPPDATA%\mc-launcher-v2\backups\`; экспорт — в `exports\`.
   Восстановление бэкапа = импорт zip (существующая `instance_import`),
   отдельной команды не нужно.
4. Контент-манифест получил поле `url` (serde default) — старые файлы
   читаются; в `files[]` экспорта попадают только включённые записи
   с известным url (честный .mrpack, ставящийся Modrinth App).

Команды: `content_toggle`, `content_remove`, `instance_backup`,
`instance_export`. UI: вкладки типов каталога (моды/шейдеры/ресурспаки/
модпаки — фильтр `project_type` уже был в ядре), панель «Установленные»
(тумблер/удаление), пункты кебаб-меню «Бэкап (zip)»/«Экспорт .mrpack».
Тесты: 91 lib (в т.ч. 6 новых в `content.rs`) + приёмка
`scripts/content_features.cjs` (вкладки, тумблер вживую, zip на диске).

## D27. Иконки инстансов, масштаб интерфейса, статистика в UI (2026-09-27)

1. **Иконки**: пользовательский файл (PNG/JPEG/WebP, ≤ 300 КБ, сниффинг
   магических байтов) → data-URL в `instance.json` (`icon`). Data-URL вместо
   пути в папке — фронт не зависит от asset-протокола Tauri и его scope;
   лимит размера защищает instance.json от раздутия. Команды
   `instance_icon_set` / `instance_icon_remove`; в строке инстанса `<img>`
   вместо буквы, пункты в кебаб-меню.
2. **Масштаб интерфейса**: `settings.uiScale` (u32, default 100, clamp
   80–150 при применении). Механика — `font-size` на `<html>` в %:
   Tailwind/ремы масштабируют весь UI честно, без transform-мыла.
   Применяется из `state/settings.ts` при загрузке и смене.
3. **Статистика**: ядро уже писало `launch_count/last_played/play_seconds`
   (обе ветки выхода процесса, run.rs) — добавлен показ чипами в модалке
   настроек инстанса. Экран «история запусков построчно» остаётся в P3.
Приёмка: `scripts/wave2_features.cjs` (иконка set/clear вживую, чипы
статистики, селектор 125%→100% с проверкой font-size).

## D28. Фиксы discovery-волны: отмена загрузок, лимиты zip, MSA-refresh, UX (2026-09-27)

По DISCOVERY_REPORT (бэклог AUDIT §6) закрыты шесть P1:

1. **Отмена загрузок (D1)**: в `AppState` появился реестр активных движков
   `engines: Mutex<HashMap<группа, Arc<DownloadEngine>>>`. Рабочие команды
   (launch/content_install/update_all/optimize) берут движок через
   `engine_for(группа)` и снимают регистрацию `engine_done` после завершения
   (успех/ошибка) — старый HttpClient не утекает при hot-swap прокси.
   `instance_kill` зовёт `cancel_engine_group` и отменяет РЕАЛЬНЫЙ движок,
   а не свежесозданный no-op. Движки внутри `install_mrpack`/`modpack_install`
   по-прежнему внутренние (D11, P2).
2. **ram_guide (D2)**: sysinfo 0.39 `total_memory()` возвращает байты —
   убрано лишнее `*1024` (завышение ×1024, рекомендация всегда 6144);
   предупреждение «>8G» считалось от зажатого clamp'ом значения и было
   мёртвым — теперь от четверти RAM. Регресс-тест правдоподобности total.
3. **Zip-bomb (D3)**: `extract_zip_stripping` (недоверенные .mrpack/CF)
   получил те же лимиты, что `extract_zip_filtered`: 200k записей,
   8 ГБ суммарно. Тест на сохранение стриппинг-поведения.
4. **MSA-refresh (D4)**: `refresh_session` больше не читает настройки из
   `Paths::default_root()` — client_id прокидывается вызывающим
   (`launch_identity`) из фактического корня данных; кастомный каталог
   данных больше не ломает refresh.
5. **Экран ошибки старта (D6)**: `useSettings` ловит ошибку `settings_get`;
   вместо вечного «…» — текст ошибки + «Повторить».
6. **Удаление инстанса (D7)**: window.confirm заменён диалогом
   «В корзину (восстановимо) / Удалить навсегда / Отмена» — вариант
   `wipe: true` стал достижимым, тексты в i18n ru+en.

Проверка: cargo test 94+10, clippy чист; приёмки ui_checks/e2e_features/
content_features/wave2_features — зелёные; диалог удаления проверен вживую.

## D29. Пятёрка агентов: релизность, перф, диалоги, модпаки, ядро (2026-09-27)

Волна выполнена пятью параллельными агентами с разведёнными по файлам
полосами; интеграция и приёмки — оркестратором. Ключевые решения:

1. **Имя и релиз**: productName/mainBinaryName/заголовки → «Cobble Launcher»
   (identifier, крейт и каталог данных не тронуты — keyring и данные живут);
   LICENSE = MIT; Cargo-метаданные честные; CSP со строгой политикой
   (connect-src ipc: — канонический транспорт Tauri; img-src cdn.modrinth.com
   для иконок каталога).
2. **Модпаки (D11/D12)**: движок и группа передаются снаружи
   (`mrpack:<project|file>`), прогресс установки модпаков идёт в UI через
   общую шину; инстанс сохраняется ТОЛЬКО после успешной загрузки, при сбое
   или отмене каталог откатывается (rollback_instance). CLI mcl обновлён.
3. **Ядро**: pid_alive через sysinfo (без спавна tasklist на каждый чек);
   InstanceLock через create_new(true) — снятие только мёртвых lock-файлов;
   манифесты из чужих zip с капом 16 МБ; import_archive в spawn_blocking.
4. **UI-перф**: memo(InstanceRow) + useShallow-селекторы + батчинг логов
   (200 мс, кап 500); dl_progress получает детерминированного владельца
   (полный по-instance_id фикс требует расширения контракта события —
   отдельное решение); настройки инстанса мержатся в свежий snapshot;
   настройки лаунчера с debounce; skip онбординга сохраняет флаг.
5. **Диалоги и ошибки**: tauri-plugin-dialog (нативный выбор .mrpack и
   иконок, drag&drop .mrpack через onDragDropEvent — HTML5-drop на Windows
   не работает при dragDropEnabled, путь берётся из payload Tauri);
   apiErrorText пробрасывает коды ядра через errors.json.
6. **Найдено агентами и исправлено**: двойная вложенность
   minecraft/minecraft при импорте MultiMC/своего zip (extract без стриппинга
   префикса) — тест закрепляет раскладку.

Проверка: cargo test 112 (102 lib + 10 engine) + clippy чист; 4 приёмки
зелёные на живом лаунчере (CDP), иконка через IPC-путь с withGlobalTauri
временным флагом. Остатки: D13/D14/D18 частично, D22/D25/D26/D28/D29 — P3
бэклог; CSP в dev-режиме (vite) не действует — контроль в релизной сборке.

## D30. Три языка: en (по умолчанию), ru, uk (2026-09-27)

1. Словари i18n: src/i18n/{ru,en,uk}.json, 226 ключей в каждом, полный
   паритет (проверяется скриптом). Fallback-цепочка: current → en → ключ.
2. Английский — язык свежих установок (Settings::default language = "en");
   существующие установки сохраняют записанный язык (settings.json).
3. Выбор языка: Настройки → Общие → «Language/Язык/Мова» и шаг 1 онбординга
   (для новых установок). Применяется мгновенно (setLanguage), персистится
   через settings_set. Названия языков в селекторе — на самих языках
   (native names), переводить их не нужно.
4. Словарь кодов ошибок errors.json стал трёхъязычным
   ({code: {ru,en,uk}}); errorText выбирает по текущему языку, fallback en.
   Известное ограничение: hint из ядра (LauncherError::hint) остаётся
   русским — перевод строк ядра отдельное решение, не блокирует.
5. Приёмка: живое переключение en→uk→ru в Настройках, персистентность
   после перезагрузки окна, ui_checks/e2e_features зелёные.
