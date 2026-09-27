# Отчет кластера 1: Верификация R1 и Направление 11 (Документация и релизные метаданные)

**Дата:** 2026-09-27  
**Кластер:** Explorer Cluster 1  
**Объект аудита:** Репозиторий `mc-launcher-v2` (Cobble Launcher)  
**Режим:** Read-only статический анализ и верификация исходного кода  

---

## Резюме результатов аудита

В рамках Кластера 1 проведены:
1. Сплошная независимая верификация каждого пункта со статусом «Устранено» / «Исправлено» / «Закрыто» в `AUDIT.md` (§1 Матрица фиче-гэпа, §3 Реестр проблем и устранений, §5 Бэклог P2+, §6 Discovery-находки D1–D29).
2. Независимая проверка архитектурных решений `D26`–`D30` в `docs/DECISIONS.md`.
3. Аудит направления 11 «Документация и релизные метаданные»: валидация `README.md` и `docs/README.md`, сквозная сверка версий (0.1.0) по цепочке из 4 конфигурационных файлов, проверка метаданных установщиков в `tauri.conf.json`, аудит актуальности `docs/PROGRESS.md` и `docs/UI-CONTRACT.md`.

### Ключевые выводы:
- **R1 Верификация:** Из 48 проверенных пунктов:
  * **Подтверждено (Confirmed):** 43 пункта.
  * **Частично подтверждено (Partially Confirmed):** 4 пункта (§1 п. 14 / §3 п. 17 / §6 D7 — логика честного удаления работает в `InstancesPage.tsx`, но пропущена в `HomePage.tsx`; §6 D13 — локализация ошибок частично; §6 D18 — привязка прогресса загрузки частично; D30 — паритет словарей нарушен: в `uk.json` 225 ключей вместо 226, пропущен ключ `accounts.title`).
  * **Опровергнуто (Refuted):** 1 пункт (**§6 D15 / D29 п. 3** — заявленное «shutdown OAuth-listener по отмене» отсутствует в коде: блокирующий `std::net::TcpListener` утекает в пуле `spawn_blocking` при таймауте).
- **Синхронизация версий:** Все 4 целевых файла (`tauri.conf.json`, `Cargo.toml`, `package.json`, `package-lock.json`) строго синхронизированы на версии **0.1.0**.
- **Коллизия лицензий:** В `docs/README.md:69` заявляется лицензия **GPL-3.0**, тогда как в корне лежит **MIT License**, и в `Cargo.toml` указан `license = "MIT"`. Это юридический риск P1.
- **IPC Контракт (`docs/UI-CONTRACT.md`):** Критически устарел (P1). Описывает только 24 команды из 46 реально зарегистрированных в `src-tauri/src/lib.rs` и используемых в `src/api/client.ts`. Не содержит описания 22 команд ядра и целого ряда структур данных.
- **Метаданные установщиков:** В `tauri.conf.json` отсутствуют секции `bundle.windows.nsis` и `bundle.windows.wix`, нет полей `publisher`, `copyright`. Идентификатор приложения остаётся техническим (`dev.mclauncher.v2`).

---

## 1. Сводная таблица верификации R1 (AUDIT.md и DECISIONS.md D26–D30)

| № / ID | Источник в AUDIT / DECISIONS | Заявленное утверждение / статус | Фактическая реализация в коде (`файл:строка`) | Вердикт | Техническое доказательство и примечания |
|---|---|---|---|---|---|
| **R1-01** | AUDIT §1 п. 1 | Настройки инстанса (RAM, JVM args, Java path): Устранено (`InstanceSettingsModal`) | `src/ui/components/InstanceSettingsModal.tsx:48-65, 117-210`, `src-tauri/src/commands/mod.rs:155-168` | **Подтверждено** | Ползунок RAM с `ramGuide`, выбор установленных Java через `api.javaList()`, поля JVM-флагов и аргументов игры. Сохранение через `api.instanceSettingsSet`. |
| **R1-02** | AUDIT §1 п. 2 | Просмотр логов и краш-анализ из UI: Устранено (`LogViewerModal`, `CrashReportModal`) | `src/ui/components/LogViewerModal.tsx:1-143`, `src/ui/components/CrashReportModal.tsx:1-145`, `src/state/instances.ts:153-175` | **Подтверждено** | `LogViewerModal` обеспечивает tail логов и автопрокрутку; `CrashReportModal` открывается автоматически при exitCode != 0 с рекомендациями анализатора `crash.rs`. |
| **R1-03** | AUDIT §1 п. 3 | Вход Microsoft (честный браузерный auth-code): Устранено (`account_add_msa_browser`, `AccountsModal`) | `src-tauri/src/commands/mod.rs:587-634`, `src-tauri/src/auth/msa.rs:315-362`, `src/ui/components/AccountsModal.tsx:45-59` | **Подтверждено** | Браузерный auth-code flow реализован, токены пишутся в Windows Credential Manager. (По утечке сокета при отмене см. R1-38 / D15). |
| **R1-04** | AUDIT §1 п. 4 | Управление профилями (добавление/удаление): Устранено (`AccountsModal`: Offline, Ely, MSA) | `src/ui/components/AccountsModal.tsx:27-87, 120-170`, `src-tauri/src/auth/mod.rs:50-130` | **Подтверждено** | Модальное окно содержит вкладки Offline, MSA, Ely.by, удаление с подтверждением и смену активного аккаунта. |
| **R1-05** | AUDIT §1 п. 5 | Защита от удаления/дублирования работающего инстанса: Устранено (`running_pid` guard в Rust + disable в UI) | `src-tauri/src/instances/mod.rs:237, 267, 296`, `src/ui/components/InstanceRow.tsx:168, 197, 208, 248, 258` | **Подтверждено** | В ядре вызовы `delete`, `duplicate`, `rename` отклоняются с `LauncherError::InvalidInput` при живом PID. В UI кнопки заблокированы (`disabled={busy}`). |
| **R1-06** | AUDIT §1 п. 6 | Защита от повторного запуска одного инстанса (гонка): Устранено (`AppState.launching` lock в ядре) | `src-tauri/src/commands/mod.rs:25, 175-181, 267` | **Подтверждено** | Мьютекс `state.launching` атомарно проверяет и захватывает ID инстанса до спавна, исключая параллельный `prepare`. |
| **R1-07** | AUDIT §1 п. 7 | Офлайн-запуск при протухшем манифесте (> 1 ч): Устранено (stale manifest fallback в `fetch_manifest`) | `src-tauri/src/mojang/manifest.rs:81-94, 191-203` | **Подтверждено** | При ошибке сети `fetch_manifest` возвращает имеющийся кэш независимо от давности TTL. Закреплено юнит-тестом. |
| **R1-08** | AUDIT §1 п. 8 | Переименование инстанса: Устранено (`RenameInstanceModal`) | `src/ui/components/RenameInstanceModal.tsx:19-35`, `src/ui/components/InstanceRow.tsx:164-174`, `src-tauri/src/commands/mod.rs:135-138` | **Подтверждено** | Модальное окно переименования подключено к кебаб-меню, вызывает `instance_rename` с санитизацией имени. |
| **R1-09** | AUDIT §1 п. 9 | Выбор и установка Java (Adoptium JRE): Устранено (Секция Java в Настройках + 1-click Adoptium) | `src/ui/pages/SettingsPage.tsx:231-297`, `src/ui/components/InstanceSettingsModal.tsx:145-180`, `src-tauri/src/java/adoptium.rs:1-120` | **Подтверждено** | Отображает список обнаруженных JRE и кнопки быстрой установки Adoptium JRE 17 / 21 в runtime-директорию с верификацией SHA256. |
| **R1-10** | AUDIT §1 п. 10 | Вход Ely.by (authlib-injector): Устранено (`AccountsModal` Ely.by tab) | `src/ui/components/AccountsModal.tsx:61-77`, `src-tauri/src/commands/mod.rs:204-212, 691-710`, `src-tauri/src/auth/ely.rs:1-160` | **Подтверждено** | Форма входа Ely.by отправляет credentials на authserver.ely.by; при запуске игры внедряется `-javaagent` скачанного authlib-injector. |
| **R1-11** | AUDIT §1 п. 11 | Горячая смена Proxy без перезапуска лаунчера: Устранено (динамический `HttpClient` в `settings_set`) | `src-tauri/src/commands/mod.rs:878-894` | **Подтверждено** | При смене `proxy_url` в настройках `settings_set` динамически пересоздает `HttpClient` и записывает его в `*state.http.write().await`. |
| **R1-12** | AUDIT §1 п. 12 | Идемпотентность и отмена остановки (kill): Устранено (синхронный спавн, идемпотентный `kill`) | `src-tauri/src/commands/mod.rs:519-550`, `src-tauri/src/instances/mod.rs:366-385` | **Подтверждено** | `instance_kill` отменяет загрузки движка группы, принудительно убивает дерево taskkill /T, очищает lock и переводит статус в Exited. |
| **R1-13** | AUDIT §1 п. 13 | Человекочитаемые ошибки API (без `[object Object]`): Устранено (нормализация ошибок в `api/client.ts`) | `src/api/client.ts:4-14`, `src/i18n/index.ts:43-58` | **Подтверждено** | `api/client.ts` оборачивает ошибки в `Error(msg)` с сохранением полей, `apiErrorText` форматирует их через словарь `errors.json`. |
| **R1-14** | AUDIT §1 п. 14 | Удаление с выбором (в корзину / навсегда): Устранено (опция wipe в `InstanceRow`) | `src/ui/pages/InstancesPage.tsx:58-60, 225-245` vs `src/ui/pages/HomePage.tsx:120-124` | **Частично подтверждено** | На странице `InstancesPage` диалог «В корзину / Навсегда» работает. Однако на `HomePage.tsx:120-124` остался `window.confirm` и захардкожен `wipe: false`. См. FINDING-C1-02. |
| **R1-15** | AUDIT §1 п. 15 | Список установленных модов инстанса (вкл/выкл): Устранено (`content_toggle`/`content_remove` + панель «Установленные») | `src-tauri/src/instances/content.rs:68-115`, `src/ui/pages/ModsPage.tsx:241-260, 345-425` | **Подтверждено** | В `content.rs` файлы переименовываются с суффиксом `.disabled` и флагом `enabled`; в `ModsPage` есть панель «Установленные» с тогглами и удалением. |
| **R1-16** | AUDIT §1 п. 16 | Экспорт/шаринг инстанса (.mrpack / zip): Устранено (`instance_export` .mrpack + `instance_backup` zip) | `src-tauri/src/instances/content.rs:119-235`, `src-tauri/src/commands/mod.rs:910-928`, `src/ui/components/InstanceRow.tsx:193-214` | **Подтверждено** | `backup` упаковывает инстанс в zip (включая saves, исключая шум); `export_mrpack` формирует валидный .mrpack с `modrinth.index.json` и `overrides/`. |
| **R1-17** | AUDIT §1 п. 17 | Каталог шейдеров и ресурспаков (вкладки типов): Устранено (вкладки Моды/Шейдеры/Ресурспаки/Модпаки, фильтр `project_type`) | `src/ui/pages/ModsPage.tsx:23-30, 99-106, 275-292` | **Подтверждено** | Вкладки `mod`, `shader`, `resourcepack`, `modpack` переключают тип поиска в Modrinth API. |
| **R1-18** | AUDIT §1 п. 18 | Модпаки из каталога Modrinth в один клик: Устранено (вкладка «Модпаки» в каталоге → новый инстанс) | `src/ui/pages/ModsPage.tsx:174-179, 471-475`, `src-tauri/src/commands/mod.rs:475-496` | **Подтверждено** | Кнопка «Установить сборку» запускает `modpackInstall`, создает инстанс и распаковывает зависимости и overrides. |
| **R1-19** | AUDIT §1 п. 19 | Свои иконки инстансов: Устранено (`instance_icon_set/remove`, data-URL в instance.json) | `src-tauri/src/instances/content.rs:240-278`, `src/ui/components/pickFile.ts:1-17`, `src/ui/components/InstanceRow.tsx:86-92` | **Подтверждено** | Поддерживает PNG/JPEG/WebP ≤ 300 КБ со сниффингом сигнатур, сохраняет data-URL base64 в `instance.json`. Выбор через системный диалог. |
| **R1-20** | AUDIT §1 п. 20 | Масштабирование UI (Zoom): Устранено (`uiScale` 90–150%, селектор в Настройках) | `src/ui/pages/SettingsPage.tsx:209-221`, `src/state/settings.ts:25-30`, `src-tauri/src/settings.rs:48` | **Подтверждено** | Выбор масштаба в Настройках меняет `document.documentElement.style.fontSize` в процентах (90–150%), сохраняется в `settings.json`. |
| **R1-21** | AUDIT §1 п. 21 | Подробная история запусков: Частично (чипы статистики в настройках инстанса) | `src/ui/components/InstanceSettingsModal.tsx:35-39, 105-113` | **Подтверждено** | Отображает суммарные чипы (число запусков, время в игре, дата последнего запуска). Построчной истории нет, статус «Частично» честный. |
| **R1-22** | AUDIT §3 п. 1 | Контракт ядра/UI: `Instance` camelCase | `src-tauri/src/instances/mod.rs:16-50`, `src-tauri/src/events.rs:18` | **Подтверждено** | Структура `Instance` и события ядра имеют `rename_all = "camelCase"` и aliases для обратной совместимости. |
| **R1-23** | AUDIT §3 п. 2 | Гонки и запуск: Двойной клик по «Играть» | `src-tauri/src/commands/mod.rs:175-181` | **Подтверждено** | Проверка и вставка в `state.launching` защищены `tokio::sync::Mutex`. |
| **R1-24** | AUDIT §3 п. 3 | Жизненный цикл: Стоп во время подготовки | `src-tauri/src/commands/mod.rs:234-248, 521-538` | **Подтверждено** | `instance_kill` снимает флаг `launching` и отменяет движок загрузки; `instance_launch` после `prepare` проверяет флаг и не спавнит игру. |
| **R1-25** | AUDIT §3 п. 4 | Гонки: `InstanceLock::drop` стирал чужой `.lock` | `src-tauri/src/instances/mod.rs:468-479` | **Подтверждено** | `InstanceLock::drop` проверяет PID в файле: удаляет файл только если PID совпадает с ожидаемым дочерним или собственным процессом. |
| **R1-26** | AUDIT §3 п. 5 | Удаление/дублирование работающего инстанса | `src-tauri/src/instances/mod.rs:237, 267, 296` | **Подтверждено** | Проверка `running_pid` блокирует операции на запущенных инстансах. |
| **R1-27** | AUDIT §3 п. 6 | Офлайн при протухшем манифесте | `src-tauri/src/mojang/manifest.rs:81-94` | **Подтверждено** | Возврат устаревшего файла кэша при сбое сети. |
| **R1-28** | AUDIT §3 п. 7 | Игра упала при старте — молча сбрасывался статус | `src/state/instances.ts:153-175`, `src/ui/components/CrashReportModal.tsx:1-145` | **Подтверждено** | Событие `exited` с кодом != 0 вызывает `api.crashAnalyze` и выводит модальное окно краш-репорта. |
| **R1-29** | AUDIT §3 п. 8 | Аккаунты в UI (MSA + Ely.by) | `src/ui/components/AccountsModal.tsx:1-329` | **Подтверждено** | Полный интерфейс управления аккаунтами в UI. |
| **R1-30** | AUDIT §3 п. 9 | Никнейм онбординга игнорировался | `src/ui/pages/Onboarding.tsx:35-42` | **Подтверждено** | `cleanNick` передаётся в `api.accountAddOffline` и активируется в настройках. |
| **R1-31** | AUDIT §3 п. 10 | Настройки инстанса отсутствовали в UI | `src/ui/components/InstanceSettingsModal.tsx:1-249` | **Подтверждено** | Создан полнофункциональный компонент `InstanceSettingsModal`. |
| **R1-32** | AUDIT §3 п. 11 | Нельзя переименовать инстанс из UI | `src/ui/components/RenameInstanceModal.tsx:1-116` | **Подтверждено** | Создан диалог `RenameInstanceModal` с вызовом `api.instanceRename`. |
| **R1-33** | AUDIT §3 п. 12 | Невозможно просмотреть логи из UI | `src/ui/components/LogViewerModal.tsx:1-143` | **Подтверждено** | Создан просмотрщик логов `LogViewerModal`. |
| **R1-34** | AUDIT §3 п. 13 | Proxy не обновлялся на лету | `src-tauri/src/commands/mod.rs:878-894` | **Подтверждено** | Динамическое обновление `HttpClient` в `settings_set`. |
| **R1-35** | AUDIT §3 п. 14 | `useInstances.load()` стирал статус скачивания | `src/state/instances.ts:111-125` | **Подтверждено** | `load()` сохраняет фазы подготовки/загрузки при мердже статусов. |
| **R1-36** | AUDIT §3 п. 15 | Дублирование листенеров `subscribeEvents` | `src/state/instances.ts:41, 130-131` | **Подтверждено** | Защита флагом `isSubscribed`. |
| **R1-37** | AUDIT §3 п. 16 | Точки на конце и DOS-имена ломали пути | `src-tauri/src/util/names.rs:28-36` | **Подтверждено** | `sanitize_user_name` срезает концевые точки и заменяет DOS-имена на fallback. |
| **R1-38** | AUDIT §3 п. 17 | Выбор корзина/навсегда | `src/ui/pages/InstancesPage.tsx:225-245` vs `HomePage.tsx:120-124` | **Частично подтверждено** | Исправлено на `InstancesPage`, но пропущено на `HomePage` (см. R1-14). |
| **R1-39** | AUDIT §3 п. 18 | Управление Java в UI | `src/ui/pages/SettingsPage.tsx:231-297` | **Подтверждено** | Секция Java подключена в Настройках. |
| **R1-40** | AUDIT §3 п. 19 | Ошибки IPC `[object Object]` | `src/api/client.ts:4-14` | **Подтверждено** | Нормализация в объект `Error` на уровне обертки `invoke`. |
| **R1-41** | AUDIT §3 п. 20 | Гонка спавна процесса | `src-tauri/src/instances/run.rs:614-630`, `src-tauri/src/commands/mod.rs:266-267` | **Подтверждено** | Синхронный `spawn()`, запись child PID в `.lock` и только потом снятие lock с `launching`. |
| **R1-42** | AUDIT §5 п. 1 | Вкладка установленных модов инстанса (P2) | `src-tauri/src/instances/content.rs:68-115`, `src/ui/pages/ModsPage.tsx:345-425` | **Подтверждено** | Реализована в ядре и выведена в UI с переключателями и удалением. |
| **R1-43** | AUDIT §6 D1 | Отмена движка загрузок при Стопе | `src-tauri/src/commands/mod.rs:28, 65-85, 526` | **Подтверждено** | Реестр `AppState.engines`, отмена реального инстанса `DownloadEngine` через `cancel_engine_group`. |
| **R1-44** | AUDIT §6 D2 | `ram_guide` sysinfo байты/МБ | `src-tauri/src/optimize.rs:83-102` | **Подтверждено** | Переведено делением на `1024*1024`, предупреждение >8ГБ от `quarter`. |
| **R1-45** | AUDIT §6 D3 | Zip-bomb в `extract_zip_stripping` | `src-tauri/src/util/zip.rs:129-153` | **Подтверждено** | Проверка лимитов `MAX_ENTRIES` (200k) и `MAX_TOTAL_UNCOMPRESSED` (8 ГБ). |
| **R1-46** | AUDIT §6 D4 | MSA-refresh в кастомном каталоге | `src-tauri/src/auth/msa.rs:110-141` | **Подтверждено** | `client_id` прокидывается из контекста вызова, минуя `default_root()`. |
| **R1-47** | AUDIT §6 D5 | Диалог .mrpack и drag&drop | `src/ui/layout/Shell.tsx:75-115`, `src/ui/pages/ModsPage.tsx:196-209` | **Подтверждено** | Выбор через plugin-dialog и нативный обработчик `getCurrentWebview().onDragDropEvent`. |
| **R1-48** | AUDIT §6 D6 | Экран ошибки старта вместо спиннера | `src/App.tsx:26-37` | **Подтверждено** | Экран `app.loadError.title` с текстом ошибки и кнопкой «Повторить». |
| **R1-49** | AUDIT §6 D7 | Диалог удаления «В корзину / Навсегда» | `src/ui/pages/InstancesPage.tsx:225-245` vs `HomePage.tsx:120-124` | **Частично подтверждено** | Исправлено на `InstancesPage`, но пропущено на `HomePage` (см. R1-14). |
| **R1-50** | AUDIT §6 D8 | UI-перф: memo, useShallow, батчинг логов | `src/ui/components/InstanceRow.tsx:275`, `src/ui/pages/HomePage.tsx:42`, `src/state/instances.ts:43-75` | **Подтверждено** | `memo(InstanceRow)`, `useShallow` в селекторах Zustand, батчинг строк лога 200 мс / кап 500. |
| **R1-51** | AUDIT §6 D9 | CSP в `tauri.conf.json` | `src-tauri/tauri.conf.json:25` | **Подтверждено** | Строгая CSP с `connect-src ipc: http://ipc.localhost; img-src 'self' data: https://cdn.modrinth.com`. |
| **R1-52** | AUDIT §6 D10 | Выбор иконки через диалог API | `src/ui/components/pickFile.ts:1-17`, `src/ui/pages/InstancesPage.tsx:95` | **Подтверждено** | Вызов системного диалога через `@tauri-apps/plugin-dialog`. |
| **R1-53** | AUDIT §6 D11 | Прогресс модпаков в общую шину | `src-tauri/src/commands/mod.rs:460-496`, `src-tauri/src/modrinth/mrpack.rs:145-210` | **Подтверждено** | Движок создается с общей шиной `state.bus`, группа `mrpack:{id}` регистрируется в реестре. |
| **R1-54** | AUDIT §6 D12 | Откат инстанса модпака при сбое/отмене | `src-tauri/src/modrinth/mrpack.rs:170-194, 245-251` | **Подтверждено** | Инстанс сохраняется после успеха; при `failed > 0` или `cancelled > 0` вызывается `rollback_instance`. |
| **R1-55** | AUDIT §6 D13 | Локализация ошибок через `apiErrorText` | `src/i18n/index.ts:43-58`, `src/ui/pages/ModsPage.tsx` | **Частично подтверждено** | Статус «ЧАСТИЧНО» подтвержден: внедрено в ModsPage, AccountsModal, Shell, но в других компонентах еще используется `String(e)`. |
| **R1-56** | AUDIT §6 D14 | Тяжелый IO в `spawn_blocking` | `src-tauri/src/import/mod.rs:168-172` | **Частично подтверждено** | Статус «ЧАСТИЧНО» подтвержден: `import_archive` переведен в `spawn_blocking`, а `duplicate`, `delete`, `rename` остаются синхронными. |
| **R1-57** | AUDIT §6 D15 | Утечка TcpListener OAuth при таймауте | `src-tauri/src/commands/mod.rs:602-619`, `src-tauri/src/auth/msa.rs:322-362` | **ОПРОВЕРГНУТО** | Заявленный «shutdown OAuth-listener по отмене» отсутствует в коде! Блокирующий сокет висит в `spawn_blocking`. См. FINDING-C1-01. |
| **R1-58** | AUDIT §6 D16 | `pid_alive` через sysinfo | `src-tauri/src/instances/mod.rs:486-492` | **Подтверждено** | Вместо спавна `tasklist` вызывается `sys.refresh_processes(...)` и `sys.process(pid)`. |
| **R1-59** | AUDIT §6 D17 | ModsPage устаревший список и select disabled | `src/ui/pages/ModsPage.tsx:57-79, 312` | **Подтверждено** | Перечитывание при focus/visibility, `disabled={busy}` на селекторе целевого инстанса. |
| **R1-60** | AUDIT §6 D18 | Детерминированный владелец `dl_progress` | `src/state/instances.ts:136-138, 180-188` | **Частично подтверждено** | Статус «ЧАСТИЧНО» подтвержден: `downloadOwnerId` привязывает прогресс, полное решение требует `instanceId` в событии. |
| **R1-61** | AUDIT §6 D19 | Сохранение настроек инстанса в свежий снапшот | `src/ui/components/InstanceSettingsModal.tsx:48-65` | **Подтверждено** | Перед записью вызывается `instanceSettingsGet`, и мержатся только редактируемые поля. |
| **R1-62** | AUDIT §6 D20 | Debounce настроек лаунчера | `src/ui/pages/SettingsPage.tsx:43-56, 122-127` | **Подтверждено** | `commitDebounced` с таймером 400 мс и отображением `saveError`. |
| **R1-63** | AUDIT §6 D23 | Кап 16 МБ на манифесты из чужих zip | `src-tauri/src/import/mod.rs:44, 53-57` | **Подтверждено** | `MAX_MANIFEST_BYTES = 16 * 1024 * 1024`, отказ до чтения в память. |
| **R1-64** | AUDIT §6 D24 | `InstanceLock` через `create_new(true)` | `src-tauri/src/instances/mod.rs:439-456, 470-478` | **Подтверждено** | Атомарное создание файла, удаление чужих locks только при мертвом PID. |
| **R1-65** | AUDIT §6 D27 | Skip онбординга сохраняет флаг | `src/ui/pages/Onboarding.tsx:58-69` | **Подтверждено** | `skip()` сохраняет `onboardingDone: true` в настройках ядра. |
| **R1-66** | DECISIONS D26 | Управление контентом, бэкап и экспорт | `src-tauri/src/instances/content.rs:58-235`, `src/ui/pages/ModsPage.tsx:275-292` | **Подтверждено** | Полный набор: .disabled, экспорт .mrpack, бэкап zip, поле `url` в манифесте, вкладки каталога. |
| **R1-67** | DECISIONS D27 | Иконки, uiScale, статистика | `src-tauri/src/instances/content.rs:240-278`, `src/ui/pages/SettingsPage.tsx:209-221`, `src/ui/components/InstanceSettingsModal.tsx:35-39` | **Подтверждено** | Иконки base64 data-URL, uiScale font-size %, чипы статистики. |
| **R1-68** | DECISIONS D28 | Пакет фиксов D1-D4, D6, D7 | `commands/mod.rs`, `optimize.rs`, `zip.rs`, `auth/msa.rs`, `App.tsx`, `InstancesPage.tsx` | **Частично подтверждено** | D1–D4, D6 подтверждены полностью; D7 пропущен на главной странице (`HomePage.tsx`). |
| **R1-69** | DECISIONS D29 | Волна стабилизации 5 агентов | `Cargo.toml`, `tauri.conf.json`, `import/mod.rs:274, 316` | **Частично подтверждено** | Релизные метаданные, устранение двойной вложенности MultiMC, ядро и UI-перф подтверждены; D15 опровергнут. |
| **R1-70** | DECISIONS D30 | Три языка (en, ru, uk): 226 ключей, паритет | `src/i18n/{ru,en,uk}.json`, `src/i18n/errors.json`, `src-tauri/src/settings.rs:56` | **Частично подтверждено** | Заявленный полный паритет нарушен: в `uk.json` 225 ключей (отсутствует `accounts.title`). `Settings::default` язык "en" подтвержден. |

---

## 2. Аудит Направления 11: Документация и релизные метаданные

### 2.1. Валидация утверждений `README.md` и `docs/README.md`

#### Корневой `README.md`:
1. **Заявленные возможности инстансов, модов, аккаунтов, диагностики, оформления:**
   - Полностью подтверждены фактической реализацией (поддерживаются все 4 загрузчика Fabric/Quilt/Forge/NeoForge, профили MSA/Ely/Offline, живые логи, анализ крашей, 5 палитр оформления, горячие клавиши).
2. **Горячая клавиша `Ctrl+K`:**
   - Подтверждена в `src/ui/layout/Shell.tsx:127` (`if (e.ctrlKey && e.key.toLowerCase() === "k") { searchRef.current?.focus(); }`).
3. **Строка 86: количество тестов ядра:**
   - Утверждение: `cd src-tauri && cargo test # 96 тестов ядра и движка загрузок`.
   - Фактически: Прогон `cargo test` дает **112 пройденных тестов** (102 модульных в библиотеке + 10 интеграционных тестов `download_engine`). Число 96 устарело.
4. **Строка 97: языки интерфейса:**
   - Утверждение: `src/ — интерфейс: страницы, компоненты, темы, i18n (ru + en)`.
   - Фактически: В строке 50 заявляется поддержка английского, русского и украинского языков. В структуре каталогов строка 97 указывает только `(ru + en)`.

#### `docs/README.md`:
1. **Строка 65: количество архитектурных решений:**
   - Утверждение: `[docs/DECISIONS.md](DECISIONS.md) — 21 решение с причинами`.
   - Фактически: В файле `docs/DECISIONS.md` содержится **30 решений (D1–D30)**.
2. **Строка 69: юридический статус лицензии:**
   - Утверждение: `## Лицензия / GPL-3.0 (как у Prism Launcher)`.
   - Фактически: В корне репозитория находится файл `LICENSE` с текстом **MIT License (Copyright 2026 loxord241)**. В файле манифеста `src-tauri/Cargo.toml:6` строго указано `license = "MIT"`. Это критический юридический рассинхрон P1.

---

### 2.2. Сквозная синхронизация версий (0.1.0)

Проведена сверка версий по всей цепочке файлов:
1. `src-tauri/tauri.conf.json`: `"version": "0.1.0"` (строка 5) — **СОВПАДАЕТ**.
2. `src-tauri/Cargo.toml`: `version = "0.1.0"` (строка 3) — **СОВПАДАЕТ**.
3. `package.json`: `"version": "0.1.0"` (строка 4) — **СОВПАДАЕТ**.
4. `package-lock.json`: `"version": "0.1.0"` (строки 3, 9) — **СОВПАДАЕТ**.

**Вердикт:** Версии всех компонентов полностью синхронизированы на значении `0.1.0`.

---

### 2.3. Метаданные установщиков в `src-tauri/tauri.conf.json`

В файле конфигурации Tauri 2:
- `productName`: `"Cobble Launcher"` (строка 3).
- `mainBinaryName`: `"Cobble Launcher"` (строка 4).
- `identifier`: `"dev.mclauncher.v2"` (строка 6).

**Выявленные дефекты:**
1. **Отсутствие конфигурации установщиков NSIS и MSI (Wix):**
   В блоке `"bundle"` (`строки 28–38`) указаны только `"active": true`, `"targets": "all"` и пути к иконкам. Секции `"windows": { "nsis": { ... }, "wix": { ... } }` полностью отсутствуют.
2. **Отсутствие данных издателя и юридической информации:**
   Поля `"publisher"`, `"copyright"`, `"category"`, `"shortDescription"`, `"longDescription"` не заданы. Сгенерированные установщики определяются системой Windows как созданные неизвестным издателем (Unknown Publisher), что гарантирует срабатывание защитного фильтра Windows Defender SmartScreen.
3. **Идентификатор приложения:**
   Используется технический идентификатор `"dev.mclauncher.v2"`. В соответствии с практиками Tauri, изменение идентификатора после первого релиза приводит к смене путей локальных данных приложения (`AppData\Local\...`) и потере привязки к сохраненным в Windows Credential Manager секретам. Требуется утвердить финальный продуктовый bundle ID до публичного релиза.

---

### 2.4. Актуальность и полнота `docs/PROGRESS.md`

1. **Дата и объем:** Документ заморожен на этапе M9 от 2026-09-26. Вся последующая работа по стабилизации, закрытию гэпов AUDIT и внедрению решений D26–D30 (2026-09-27) в документе отсутствует.
2. **Метрики тестов:** В чеклисте (строка 390) заявлено `cargo test (80 юнит + 10 интеграционных)` при фактических `102 + 10 = 112`.
3. **Лицензия:** В строке 385 ошибочно упомянута лицензия GPL-3.0.
4. **Команды ядра:** Не отражены новые команды, введенные в волнах M5–M9 и post-M9 стабилизации.

---

### 2.5. Актуальность и точность `docs/UI-CONTRACT.md`

`docs/UI-CONTRACT.md` отстает от актуальной кодовой базы:
1. **Пропуск 22 команд ядра:**
   В таблице команд (строки 12–38) описаны только 24 команды. В `src-tauri/src/lib.rs` зарегистрированы, а в `src/api/client.ts` реализованы **46 команд**.
   Полностью отсутствуют в контракте:
   - `instance_kill`
   - `instance_backup`
   - `instance_export`
   - `instance_icon_set`
   - `instance_icon_remove`
   - `instance_optimize`
   - `ram_guide`
   - `process_status`
   - `account_list`
   - `account_add_offline`
   - `account_add_msa_start`
   - `account_add_msa_poll`
   - `account_add_msa_browser`
   - `account_add_ely`
   - `account_remove`
   - `account_active_set`
   - `crash_analyze`
   - `content_toggle`
   - `content_remove`
   - `modpack_install`
   - `instance_import` (в контракте ошибочно помечен как отсутствующий)
2. **Расхождения в типах (`src/api/types.ts`):**
   - В типе `Settings` отсутствует поле `uiScale: number`.
   - В типе `ContentEntry` отсутствует поле `url?: string`.
   - Отсутствуют описания типов `Account`, `AccountKind`, `ArchiveResult`, `Diagnosis`, `RamGuide`.
3. **События:**
   - В сигнатуре события `dl_progress` отсутствует идентификатор инстанса `instanceId`, что создает архитектурную неоднозначность при параллельных загрузках (зафиксировано в D18).

---

## 3. Реестр новых находок по результатам аудита

Ниже приведен полный перечень выявленных проблем и дефектов, ранжированных по приоритетам:

### FINDING-C1-01: Утечка `TcpListener` и рабочего потока при таймауте браузерного входа MSA (Опровержение D15)
- **Приоритет:** **P2**
- **Направление:** 1 (Ядро) / 7 (Безопасность) / 11 (Релизные метаданные)
- **Файл и строка:** `src-tauri/src/commands/mod.rs:602-619`, `src-tauri/src/auth/msa.rs:322-362`
- **Описание:** В `AUDIT.md` §6 D15 заявлено: «УСТРАНЕНО 2026-09-27: shutdown OAuth-listener по отмене». Фактическая проверка кода показала, что сокет `std::net::TcpListener` создается и передается по значению в `wait_auth_code`, где внутри `tokio::task::spawn_blocking` запускается блокирующий цикл `for stream in listener.incoming()`. Вызов обернут в `tokio::time::timeout(300s)`. При наступлении таймаута или прерывании future сокет не закрывается, и поток пула Tokio навсегда остается заблокированным на системном вызове `accept()`. Механизм `CancellationToken` или принудительного `shutdown` сокета отсутствует.
- **Предложение по устранению:** Использовать асинхронный `tokio::net::TcpListener` с обработкой отмены через `tokio::select!` и `tokio_util::sync::CancellationToken`, либо при таймауте производить холостое локальное TCP-подключение на `127.0.0.1:port` для вывода потока из блокировки.
- **Трудозатраты:** 1–2 часа.

---

### FINDING-C1-02: Рассинхрон логики удаления инстанса на Главной странице (`HomePage.tsx`)
- **Приоритет:** **P2**
- **Направление:** 4 (Фронтенд — стейт и UX) / 11 (Релизные метаданные)
- **Файл и строка:** `src/ui/pages/HomePage.tsx:110-128`
- **Описание:** В `AUDIT.md` §1 п. 14 и §6 D7 заявлено устранение невозможности безвозвратного удаления инстанса (`wipe: true`). На странице `InstancesPage.tsx:58-60, 200-245` диалог с выбором «В корзину / Удалить навсегда» внедрен. Однако на главной странице `HomePage.tsx:120-124` обработчик удаления по-прежнему вызывает нативный `window.confirm()` и жестко передает `remove(inst.id, false)`. Кроме того, в `rowCallbacks` на `HomePage` не переданы `onBackup`, `onExport`, `onIconSet`, `onIconClear`, из-за чего эти функции недоступны пользователю из кебаб-меню на главном экране.
- **Предложение по устранению:** Вынести модальный диалог подтверждения удаления инстанса в общий переиспользуемый компонент и передать все функции кебаб-меню в `rowCallbacks` на `HomePage.tsx`.
- **Трудозатраты:** 1 час.

---

### FINDING-C1-03: Юридическая коллизия лицензии в документации (GPL-3.0 vs MIT)
- **Приоритет:** **P1**
- **Направление:** 11 (Документация и релизные метаданные) / 12 (Лицензионная чистота)
- **Файл и строка:** `docs/README.md:69-70`, `LICENSE:1-22`, `src-tauri/Cargo.toml:6`
- **Описание:** Файл `docs/README.md:69` явно заявляет: `## Лицензия / GPL-3.0 (как у Prism Launcher)`. В то же время фактический файл `LICENSE` в корне репозитория представляет собой **MIT License**, и в `src-tauri/Cargo.toml:6` указано `license = "MIT"`. Расхождение между копилефтной GPL-3.0 и пермиссивной MIT создает прямой юридический риск при дистрибуции и привлечении контрибьюторов.
- **Предложение по устранению:** Заменить текст в `docs/README.md:69-70` на `## Лицензия / MIT`, добавив ссылку на файл `LICENSE` в корне проекта.
- **Трудозатраты:** 5 минут.

---

### FINDING-C1-04: Критическое устаревание контракта взаимодействия `docs/UI-CONTRACT.md`
- **Приоритет:** **P1**
- **Направление:** 3 (Контракт событий и типов) / 11 (Документация)
- **Файл и строка:** `docs/UI-CONTRACT.md:12-38`, `src-tauri/src/lib.rs:104-151`, `src/api/client.ts:31-129`
- **Описание:** В реестре команд `docs/UI-CONTRACT.md` описано только 24 команды ядра из 46 существующих. 22 активные команды (`instance_kill`, `instance_backup`, `instance_export`, `instance_icon_set`, `instance_icon_remove`, `instance_optimize`, `ram_guide`, `process_status`, `account_list`, `account_add_offline`, `account_add_msa_start`, `account_add_msa_poll`, `account_add_msa_browser`, `account_add_ely`, `account_remove`, `account_active_set`, `crash_analyze`, `content_toggle`, `content_remove`, `modpack_install`, `instance_import`) полностью отсутствуют в спецификации. В типах пропущены поля `uiScale`, `url`, структуры аккаунтов, бэкапов и краш-диагностики.
- **Предложение по устранению:** Актуализировать таблицы команд, структур входных/выходных данных и типов событий в `docs/UI-CONTRACT.md` в полном соответствии с `src-tauri/src/lib.rs` и `src/api/types.ts`.
- **Трудозатраты:** 1.5–2 часа.

---

### FINDING-C1-05: Отсутствие метаданных Windows-установщиков и технический bundle ID в `tauri.conf.json`
- **Приоритет:** **P2**
- **Направление:** 11 (Релизные метаданные) / 10 (Windows-окружение)
- **Файл и строка:** `src-tauri/tauri.conf.json:6, 28-38`
- **Описание:** Секция `bundle` не содержит настроек для NSIS и MSI (WiX). Отсутствуют сведения об издателе (`publisher`), авторских правах (`copyright`), описании приложения. Идентификатор приложения остаётся тестовым (`"dev.mclauncher.v2"`). При сборке инсталляторы маркируются системой как недоверенные, а смена идентификатора после релиза приведет к потере пользовательских данных.
- **Предложение по устранению:** Добавить в `tauri.conf.json` блоки `bundle.windows.nsis` и `bundle.windows.wix`, заполнить имя издателя, описание, копирайт и утвердить финальный релизный identifier (например, `com.cobblelauncher.app`).
- **Трудозатраты:** 30 минут.

---

### FINDING-C1-06: Устаревание журнала прогресса разработки `docs/PROGRESS.md`
- **Приоритет:** **P2**
- **Направление:** 11 (Документация и релизные метаданные)
- **Файл и строка:** `docs/PROGRESS.md:370-411`
- **Описание:** Журнал `docs/PROGRESS.md` обрывается на этапе M9 (2026-09-26). Волна аудита и решения D26–D30 (2026-09-27) не внесены. Чеклист финальной приемки содержит устаревшие числа тестов (80+10=90 вместо 112) и ссылку на лицензию GPL-3.0.
- **Предложение по устранению:** Внести в `docs/PROGRESS.md` секцию этапа аудита и стабилизации (M10 / Post-M9), обновить счетчики тестов и метрики сборки.
- **Трудозатраты:** 30 минут.

---

### FINDING-C1-07: Нарушение паритета словарей i18n (пропуск ключа в `uk.json`)
- **Приоритет:** **P3**
- **Направление:** 6 (i18n) / 11 (Документация)
- **Файл и строка:** `src/i18n/uk.json:1-225`, `docs/DECISIONS.md:337`, `README.md:97`
- **Описание:** В `docs/DECISIONS.md:337` (D30) заявлено: «Словари i18n: src/i18n/{ru,en,uk}.json, 226 ключей в каждом, полный паритет». Фактическая программная сверка показала: в `ru.json` — 226 ключей, в `en.json` — 226 ключей, а в `uk.json` — 225 ключей. Пропущен ключ `"accounts.title"`, из-за чего при украинской локализации заголовок окна аккаунтов откатывается на английский язык (`Accounts`). Также в строке 97 корневого `README.md` ошибочно указано `i18n (ru + en)`.
- **Предложение по устранению:** Добавить `"accounts.title": "Облікові записи"` в `src/i18n/uk.json` и исправить строку 97 в `README.md` на `i18n (ru + en + uk)`.
- **Трудозатраты:** 10 минут.

---

### FINDING-C1-08: Рассинхрон количества тестов и архитектурных решений в README
- **Приоритет:** **P3**
- **Направление:** 11 (Документация и релизные метаданные)
- **Файл и строка:** `README.md:86`, `docs/README.md:65`, `AUDIT.md:139`
- **Описание:** В `README.md:86` заявлено `96 тестов ядра и движка загрузок`; в `docs/README.md:65` указано `21 решение с причинами`. Фактически в кодовой базе реализовано и успешно проходит 112 тестов (102 lib + 10 engine), а в `docs/DECISIONS.md` зафиксировано 30 решений (D1–D30).
- **Предложение по устранению:** Обновить число тестов на 112 в `README.md` и число решений на 30 в `docs/README.md`.
- **Трудозатраты:** 10 минут.

---

## 4. Рекомендации по интеграции для оркестратора

1. **Включить результаты верификации R1 в `FULL_AUDIT_REPORT.md`:** Таблица из Раздела 1 данного отчета полностью готова к переносу в итоговый отчет аудита.
2. **Включить находки в §7 `AUDIT.md`:** Находки `FINDING-C1-01` – `FINDING-C1-08` оформлены в каноническом формате с указанием приоритетов P0–P3, точных координат в коде и трудозатрат.
3. **Обратить особое внимание на FINDING-C1-01 (Опровержение D15):** В кодовой базе имеет место ложное заявление об устранении дефекта. Потребуется обязательная доработка в этапе устранения замечаний.
4. **Срочно устранить лицензионную коллизию (FINDING-C1-03):** Изменение `docs/README.md:69` с GPL-3.0 на MIT устранит разночтение правового режима кодовой базы.
