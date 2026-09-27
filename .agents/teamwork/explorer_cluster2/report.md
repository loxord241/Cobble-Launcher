# Комплексный отчет по аудиту: Направления 1, 2, 3 (mc-launcher-v2)

**Исследователь**: Explorer Cluster 2  
**Дата аудита**: 2026-09-27  
**Область аудита**:  
- **Направление 1**: Корректность ядра (Rust Core Correctness)  
- **Направление 2**: Границы IPC ядра (Core IPC Boundaries)  
- **Направление 3**: Контракт событий и типов (Events & Types Contract)  
**Режим проверки**: Read-Only статический и семантический анализ (продуктовый код в `src/` и `src-tauri/src/` не модифицировался).

---

## 1. Направление 1: Корректность ядра (Rust Core Correctness)

### 1.1. Состояния гонки и конкурентные записи в `instance.json` и `content-manifest.json`

#### 1.1.1. Несогласованная перезапись `instance.json` между UI и фоновым супервизором процесса
- **Файлы и строки**:  
  - `src-tauri/src/commands/mod.rs:155–161` (`instance_settings_set`)
  - `src-tauri/src/instances/run.rs:702–707, 721–726` (`supervise_detached`)
- **Проблема**:  
  Команда `instance_settings_set` принимает полную структуру `Instance` от фронтенда и вызывает `instances::save(&state.paths, &instance)`. При этом:
  1. В `instance_settings_set` отсутствует проверка `running_pid` (в отличие от `instance_delete`, `instance_rename` и `instance_duplicate`).
  2. В `AppState` отсутствует мьютекс или блокировка на уровне конкретного `instance_id`.
  3. Фоновый поток `supervise_detached` при выходе игры читает с диска `Instance`, инкрементирует `launch_count`, добавляет время в `play_seconds`, обновляет `last_played` и вызывает `instances::save(paths, &inst)`.
- **Сценарий гонки**:  
  Пользователь во время работы игры (или в момент её завершения) меняет параметры инстанса (RAM, JVM-флаги, заметки). Если `instance_settings_set` срабатывает почти одновременно с выходом игры, происходит гонка последнего писателя (`atomic_write`):
  - Либо фоновый супервизор перезаписывает свежие настройки пользователя старыми данными, прочитанными до сохранения настроек.
  - Либо `instance_settings_set` затирает накопленную статистику игрового времени (`play_seconds`) и `last_played`.
- **Предложение**:  
  Ввести синхронизацию обновлений `instance.json` через селективное слияние полей либо защищать операции чтения-модификации-записи инстанса именованными мьютексами в `AppState`.

#### 1.1.2. Гонка при параллельной установке и переключении контента (`content-manifest.json`)
- **Файлы и строки**:  
  - `src-tauri/src/instances/content.rs:68–94` (`toggle`)
  - `src-tauri/src/instances/content.rs:97–115` (`remove`)
  - `src-tauri/src/modrinth/install.rs:205–208` (`install_recursive`)
- **Проблема**:  
  Манифест контента `content-manifest.json` обновляется по схеме: `load_content_manifest` -> модификация вектора в памяти -> `save_content_manifest` (`atomic_write`).
  Если пользователь одновременно устанавливает мод из каталога и переключает состояние тумблера другого мода во вкладке «Установленные», обе операции читают старый манифест, параллельно пишут временный файл `.part` и перезаписывают `content-manifest.json`. Запись одной из операций бесследно теряется.
- **Предложение**:  
  Добавить `Mutex` на уровне `instance_id` в `AppState` для сериализации операций над `content-manifest.json`.

---

### 1.2. Полнота покрытия `spawn_blocking`

Согласно стандарту асинхронного рантайма Tokio, блокирующие файловые операции, запуск синхронных подпроцессов и вычисление криптографических хэшей больших файлов не должны выполняться непосредственно в пуле рабочих потоков `tokio-runtime-worker`, так как это приводит к голоданию пула и замораживанию обработки событий IPC и окна Tauri.

В ходе аудита выявлены множественные критические участки, где тяжелый блокирующий ввод-вывод выполняется прямо в async-функциях без `spawn_blocking`:

| № | Файл и строка | Функция / Контекст | Блокирующая операция | Влияние на производительность |
|---|---|---|---|---|
| 1 | `src-tauri/src/commands/mod.rs:128–130`<br>`src-tauri/src/instances/mod.rs:265–285` | `instance_duplicate` | `store::copy_tree_mixed` выполняет полный рекурсивный обход дерева каталогов инстанса, создание жестких ссылок и побайтное копирование уникальных данных (гигабайты сейвов/модов). | Замораживает поток Tokio на секунды или десятки секунд; блокирует весь IPC интерфейса. (В AUDIT D14 помечено как «дальше», фактически не исправлено). |
| 2 | `src-tauri/src/commands/mod.rs:133–136`<br>`src-tauri/src/instances/mod.rs:291–314` | `instance_delete` | При `wipe: true` — рекурсивный `std::fs::remove_dir_all`. При `wipe: false` — синхронный вызов `powershell` через `cmd.output()?`. | Блокирует поток на время работы внешнего процесса PowerShell или удаления десятков тысяч файлов. |
| 3 | `src-tauri/src/commands/mod.rs:98–100`<br>`src-tauri/src/instances/mod.rs:212–233` | `instance_list` | Синхронный `std::fs::read_dir`, чтение с диска каждого `instance.json` и `serde_json::from_slice`. | При наличии 50+ инстансов создает заметную задержку на рабочем потоке Tokio при каждом обновлении списка. |
| 4 | `src-tauri/src/commands/mod.rs:120–125`<br>`src-tauri/src/instances/mod.rs:235–246` | `instance_rename` | Синхронное чтение `.lock`, вызов `sysinfo::System::new_all()`, чтение `load()` и запись `save()` `instance.json`. | Блокирует рабочий поток Tokio. |
| 5 | `src-tauri/src/commands/mod.rs:937–946` | `logs_tail` | `std::fs::read` читает весь файл `launcher.log` (до 5 МБ) целиком в память синхронно. | Блокирует рабочий поток Tokio при частом опросе или открытии логов. |
| 6 | `src-tauri/src/instances/run.rs:195–214` | `prepare` | Синхронная распаковка zip-архивов нативных библиотек через `extract_native`. | Блокирует поток Tokio во время подготовки запуска. |
| 7 | `src-tauri/src/instances/run.rs:217–223`<br>`src-tauri/src/mojang/assets.rs:92–115` | `prepare` / `materialize_virtual` | Синхронный цикл по тысячам файлов ассетов: `create_dir_all` и `link_or_copy` для каждого объекта. | Блокирует рабочий поток Tokio на сотни миллисекунд. |
| 8 | `src-tauri/src/net/download.rs:218, 247`<br>`src-tauri/src/util/fs.rs:58–70` | `download_one` / `sha1_file` | Синхронное вычисление SHA1 по файлу через буфер 64 КБ (`sha1_file(&part)`). | При проверке `client.jar` (30–40 МБ) или JRE (50–100 МБ) блокирует поток Tokio на десятки/сотни миллисекунд. |
| 9 | `src-tauri/src/net/download.rs:314–325` | `stream_to_file` | Синхронные `std::fs::File::create`, `file.write_all(&chunk)`, `file.sync_all()` внутри асинхронного цикла обработки сетевых чанков. | Смешивание синхронного блокирующего файлового IO со стримом Tokio/reqwest. |

---

### 1.3. Корректность отмены операций (Cancellation Correctness)

#### 1.3.1. Утечка ложного состояния в `content-manifest.json` при отмене загрузки мода
- **Файл и строки**: `src-tauri/src/modrinth/install.rs:173–209`
- **Код**:
  ```rust
  ctx.engine.add_tasks(vec![DownloadTask { ... }]);
  ctx.engine.run().await?;
  let st = ctx.engine.queue_state();
  if st.failed > 0 {
      return Err(LauncherError::network(...));
  }
  // Манифест: заменить записи этого проекта (переустановка), добавить новую.
  let entry = ContentEntry { ... };
  ...
  manifest.push(entry.clone());
  crate::instances::save_content_manifest(paths, &inst.id, &manifest)?;
  ```
- **Дефект**:  
  Проверяется ТОЛЬКО условие `st.failed > 0`. Условие `st.cancelled > 0` НЕ проверяется!
  Когда пользователь нажимает «Стоп» или задача отменяется через `cancel_engine_group`, движок помечает задачи как `TaskState::Cancelled`. Соответственно, `st.failed == 0`, а `st.cancelled > 0`.
  В результате код игнорирует отмену и **записывает запись в `content-manifest.json` как успешно установленный и включенный мод**, несмотря на то, что файл мода был удален или вовсе не скачан! В UI мод отображается как установленный, но игра падает из-за отсутствия файла на диске.

#### 1.3.2. Неполный откат (Rollback) инстанса при сбое или отмене установки модпака `.mrpack`
- **Файл и строки**: `src-tauri/src/modrinth/mrpack.rs:175–211`
- **Дефект**:  
  В `install_mrpack` откат через `rollback_instance(paths, &inst.id)` вызывается только на шаге 4 (если `st.failed > 0` или `st.cancelled > 0`).
  Затем на шаге 5 выполняется `crate::instances::save(paths, &inst)?;`.
  Если последующий вызов `install_loader` (установка Fabric/Forge/NeoForge) падает с сетевой ошибкой или отменяется, либо если `apply_overrides` на шаге 6 падает из-за сбоя распаковки zip/диска, оператор `?` немедленно завершает функцию с ошибкой.
  При этом `rollback_instance` **не вызывается**, и недоустановленный битый инстанс остается в `instances/` на диске и отображается в списке инстансов.

#### 1.3.3. Частично записанные файлы-сироты (`.part`) при сбросе Future
- **Файл и строки**: `src-tauri/src/net/download.rs:314–325`
- **Дефект**:  
  Если асинхронный Future функции `stream_to_file` сбрасывается внешним таймаутом или отменой контекста Tokio до завершения, ветка `Err(LauncherError::Cancelled)` в `download_one` не выполняется, и временный файл `.part` остается на диске сиротой вплоть до следующей попытки загрузки именно этой задачи.

---

### 1.4. Анализ паник, `.unwrap()`, `.expect()` в продуктовом коде

По результатам сканирования кодовой базы `src-tauri/src/` (исключая `#[cfg(test)]` и модули тестов) выявлены следующие проблемы:

1. **Критический `.unwrap()` в файловом логгере**:
   - **Файл и строка**: `src-tauri/src/lib.rs:51–53`
   - **Код**:
     ```rust
     let file = std::fs::OpenOptions::new()
         .create(true)
         .append(true)
         .open(util::fs::long_path(&self.dir.join("launcher.log")))
         .unwrap_or_else(|_| std::fs::File::create(std::path::PathBuf::from("launcher.log")).unwrap());
     ```
   - **Опасность**:  
     Если открыть `launcher.log` в каталоге данных не удалось (каталог заблокирован, нет прав, антивирус), логгер пытается создать `launcher.log` в текущей рабочей директории через `.unwrap()`. Если лаунчер запущен из `C:\Program Files\Cobble Launcher\` (где у пользователя нет прав на запись в каталог установки), вызов `File::create` возвращает `PermissionDenied`, и `.unwrap()` вызывает мгновенную **панику и аварийное завершение процесса лаунчера** при первой же попытке записать лог!
     Кроме того, данный вызов происходит в реализации `MakeWriter::make_writer()`, то есть файл открывается на каждую отдельную запись лога (проблема D22 из бэклога).

2. **Потенциальная паника `.parent().unwrap()` при установке Forge/NeoForge**:
   - **Файлы и строки**:  
     - `src-tauri/src/loaders/forge.rs:85`: `std::fs::create_dir_all(crate::util::fs::long_path(dest.parent().unwrap()))?;`
     - `src-tauri/src/loaders/neoforge.rs:95`: `std::fs::create_dir_all(crate::util::fs::long_path(dest.parent().unwrap()))?;`
   - **Опасность**:  
     Если переменная `dest` окажется относительным однокомпонентным путем без родителя, `dest.parent()` вернет `None`, что приведет к панике потока установки загрузчика.

3. **Каскадная уязвимость через отравление мьютексов (`Mutex.lock().unwrap()`)**:
   - **Файлы и строки**:  
     - `src-tauri/src/commands/mod.rs:66, 72, 79, 86` (`self.engines.lock().unwrap()`)
     - `src-tauri/src/net/download.rs:96, 164, 194, 206, 343, 377, 407, 414, 421, 428, 443, 462` (`self.rt.tasks.lock().unwrap()`, `self.rt.group_cancels.lock().unwrap()`)
     - `src-tauri/src/util/redact.rs:16, 22` (`registry().lock().unwrap()`)
   - **Опасность**:  
     В стандартной библиотеке Rust метод `std::sync::Mutex::lock()` возвращает `PoisonError`, если любой поток запаниковал при удержании мьютекса. Использование `.unwrap()` означает, что единичная ошибка в любой фоновой задаче мгновенно отравляет мьютекс, после чего все последующие вызовы команд лаунчера, загрузок и даже логирования будут паниковать на `.unwrap()`, превращая локальный сбой в полный отказ приложения.

---

### 1.5. Обработка сбоев ввода-вывода (I/O Failures)

#### 1.5.1. Утечка поврежденных усеченных файлов при сбое копирования в сторе
- **Файл и строки**: `src-tauri/src/instances/store.rs:8–21`
- **Код**:
  ```rust
  pub fn link_or_copy(src: &Path, dst: &Path) -> Result<()> {
      let src_l = crate::util::fs::long_path(src);
      let dst_l = crate::util::fs::long_path(dst);
      if let Some(parent) = dst_l.parent() {
          std::fs::create_dir_all(parent)?;
      }
      if dst_l.exists() {
          return Ok(()); // уже на месте
      }
      if std::fs::hard_link(&src_l, &dst_l).is_err() {
          std::fs::copy(&src_l, &dst_l)?;
      }
      Ok(())
  }
  ```
- **Критический дефект**:  
  Если `hard_link` не удался (разные дисковые тома или файловая система FAT/exFAT), управление переходит к `std::fs::copy(&src_l, &dst_l)`.
  Если во время копирования переполняется диск, процесс прерывается или файл блокируется антивирусом, `std::fs::copy` возвращает ошибку ввода-вывода, оставляя на диске по пути `dst_l` частично скопированный, поврежденный файл нулевого или неполного размера.
  При следующем запуске лаунчера срабатывает проверка `if dst_l.exists() { return Ok(()); }`!
  Функция считает, что файл «уже на месте», и не выполняет ни повторное копирование, ни проверку размера/хэша. В результате игра запускается с битым `.jar` и завершается с `ZipException` / `ClassNotFoundException`.

#### 1.5.2. Устойчивость `atomic_write` к блокировкам Windows
- **Файл и строки**: `src-tauri/src/util/fs.rs:43–55`
- **Проблема**:  
  `atomic_write` выполняет запись в `.part` с последующим `fs::rename(long_path(&part), long_path(dest))`.
  В Windows системные службы индексации (Windows Search), синхронизаторы облака (OneDrive) и антивирусы удерживают дескрипторы открытых файлов с флагами, запрещающими удаление/замену. В такие моменты вызов `MoveFileExW` завершается с ошибкой `ERROR_ACCESS_DENIED` (os error 5) или `ERROR_SHARING_VIOLATION` (os error 32). В коде отсутствует механизм повторных попыток (retry loop с backoff) для атомарного переименования, что приводит к периодическим сбоям сохранения настроек и инстансов при активном OneDrive.

---

## 2. Направление 2: Границы IPC ядра (Core IPC Boundaries)

### 2.1. Сплошной аудит всех команд Tauri (`src-tauri/src/lib.rs`)

В `src-tauri/src/lib.rs:104–151` зарегистрировано ровно **46 команд**. Ниже приведена сводная матрица аудита каждой команды:

| № | Команда | Аргументы | Применяемая валидация | Потенциальный риск / Уязвимость |
|---|---|---|---|---|
| 1 | `instance_list` | — | Не требуется | Риск отсутствует (только чтение каталога инстансов). Синхронный IO. |
| 2 | `instance_create` | `mc_version: String`, `name: String` | Проверка `resolve_entry` по манифесту; `sanitize_user_name` имени; генерация UUID для ID. | Минимальный риск. Имя санитизируется, ID генерируется ядром. |
| 3 | `instance_rename` | `id: String`, `name: String` | `running_pid`; `load` валидирует `valid_id(id)`; `sanitize_user_name`. | Низкий риск. `valid_id` гарантирует отсутствие path traversal. |
| 4 | `instance_duplicate` | `id: String` | `running_pid`; `load` валидирует `valid_id(id)`. | Тяжелый IO в async без `spawn_blocking`. Риск traversal низок. |
| 5 | `instance_delete` | `id: String`, `wipe: bool` | **НЕТ валидации `id`!** Только `dir.exists()` и `running_pid`. | **КРИТИЧЕСКИЙ РИСК (P0)**: Path Traversal, произвольное удаление каталогов (`wipe: true`), Command Injection в PowerShell (`wipe: false`). См. разбор ниже. |
| 6 | `instance_open_dir` | `id: String` | **НЕТ валидации `id`!** | **ВЫСОКИЙ РИСК (P1)**: Path Traversal, создание каталогов в произвольных путях ОС, запуск исполняемых файлов через `opener::open`. |
| 7 | `instance_settings_get` | `id: String` | `load` валидирует `valid_id(id)`. | Низкий риск. Защищено `valid_id`. |
| 8 | `instance_settings_set` | `instance: Instance` | `save` валидирует `valid_id(&instance.id)`. | Гонка перезаписи со статистикой супервизора при работающей игре. |
| 9 | `instance_launch` | `id: String`, `player: String` | `running_pid`; `load` валидирует `valid_id(id)`. | Защищено. `player` передается в офлайн-идентичность. |
| 10 | `manifest_versions` | `show_snapshots: bool`, `show_old: bool` | Булевы флаги. | Риск отсутствует. Дефект схемы `release_time` (см. Направление 3). |
| 11 | `loader_versions` | `loader: String` | Строгое сопоставление по строкам `fabric`, `quilt`, `neoforge`. | Риск отсутствует. Неизвестный загрузчик возвращает ошибку. |
| 12 | `loader_install` | `id: String`, `loader: String`, `loader_version: Option<String>` | `load` валидирует `valid_id(id)`; парсинг версий. | Низкий риск. |
| 13 | `modrinth_search` | `query: String`, `mc_version?`, `loader?`, `project_type?`, `limit?`, `offset?` | Параметры кодируются reqwest в URL. | Низкий риск. |
| 14 | `content_install` | `instance_id: String`, `project_id: String`, `version_id?` | `load` валидирует `instance_id`. Хэши проверяются. | Дефект отмены: ложная запись в манифест при отмене (см. 1.3.1). |
| 15 | `content_installed` | `instance_id: String` | **НЕТ вызова `valid_id`!** | Средний риск: чтение манифеста по относительному пути `../`. |
| 16 | `content_toggle` | `instance_id: String`, `file: String` | `file` валидируется `resolve_content_file`. `instance_id` **НЕ валидируется**. | Средний риск: выход из каталога инстансов через `instance_id`. |
| 17 | `content_remove` | `instance_id: String`, `file: String` | `file` валидируется `resolve_content_file`. `instance_id` **НЕ валидируется**. | Средний риск: удаление файлов по пути относительно `instance_id`. |
| 18 | `content_update_check`| `instance_id: String` | `load` валидирует `instance_id`. | Низкий риск. |
| 19 | `content_update_all` | `instance_id: String` | `load` валидирует `instance_id`. | Низкий риск. |
| 20 | `mrpack_install` | `path: String`, `name: Option<String>` | Проверка записей zip через `safe_relative_path`. | Чтение произвольного zip с диска; отсутствие rollback при сбое loader. |
| 21 | `modpack_install` | `project_id: String`, `name: Option<String>` | Modrinth API, хэши. | Низкий риск. |
| 22 | `instance_backup` | `id: String` | `load` валидирует `valid_id(id)`. | Защищено. Архив пишется в доверенный `backups/`. |
| 23 | `instance_export` | `id: String` | `load` валидирует `valid_id(id)`. | Защищено. Архив пишется в доверенный `exports/`. |
| 24 | `instance_icon_set` | `id: String`, `path: String` | `load` валидирует `id`. Размер <= 300 КБ, сниффинг magic bytes. | Эксфильтрация произвольных картинок <= 300 КБ с диска в base64 (D10). |
| 25 | `instance_icon_remove`| `id: String` | `load` валидирует `valid_id(id)`. | Защищено. |
| 26 | `instance_optimize` | `instance_id: String` | `load` валидирует `instance_id`. | Защищено. |
| 27 | `ram_guide` | — | Нет аргументов | Риск отсутствует. Отсутствует в UI-CONTRACT.md. |
| 28 | `instance_kill` | `instance_id: String` | **НЕТ валидации `instance_id`!** | Попытка удаления `.lock` по несанитизированному пути. |
| 29 | `process_status` | — | Нет аргументов | Риск отсутствует. Отсутствует в UI-CONTRACT.md. |
| 30 | `account_list` | — | Нет аргументов | Риск отсутствует. Отсутствует в UI-CONTRACT.md. |
| 31 | `account_add_offline` | `name: String` | `sanitize_user_name(&name, "Player")`, UUID генерируется. | Защищено. Имя санитизировано. |
| 32 | `account_add_msa_start`| — | Чтение настроек. | Устаревший endpoint, рассинхрон типов с фронтендом. |
| 33 | `account_add_msa_poll` | `device_code: String` | Передача в Microsoft OAuth API. | Низкий риск. |
| 34 | `account_add_msa_browser`| — | Локальный TcpListener на 127.0.0.1, открытие URL. | Защищено. |
| 35 | `account_add_ely` | `username: String`, `password: String` | Передача по TLS в Ely authserver. | Защищено. Пароль не сохраняется, токен в keyring. |
| 36 | `account_remove` | `id: String` | Поиск по ID в `accounts.json`. | Защищено. |
| 37 | `account_active_set` | `id: String` | Поиск по ID в `accounts.json`. | Защищено. |
| 38 | `instance_import` | `path: String`, `name: Option<String>` | `extract_zip_stripping`, лимиты zip-bomb. | Чтение произвольного архива с диска. |
| 39 | `crash_analyze` | `log_text: String` | Регулярные выражения в памяти. | Риск отсутствует. |
| 40 | `java_list` | — | Сканирование фиксированных каталогов. | Дефект схемы `java_exe` / `arch_bits` (см. Направление 3). |
| 41 | `java_install` | `major: u32` | Проверка диапазона Adoptium API. | Дефект схемы возвращаемого `JavaInstall`. |
| 42 | `settings_get` | — | Чтение настроек. | Риск отсутствует. |
| 43 | `settings_set` | `settings: Settings` | Валидация proxy_url через `HttpClient::new`. | `extra_java_paths` и другие поля без валидации пути. |
| 44 | `data_dir` | — | Возврат пути `paths.root()`. | Безопасно. |
| 45 | `logs_tail` | `lines: u32` | Чтение фиксированного `launcher.log`. | Блокирующий IO на Tokio-потоке. |
| 46 | `dir_open` | — | Открытие `paths.root()` в проводнике. | Безопасно. |

---

### 2.2. Анализ уязвимостей Path Traversal и инъекции команд

#### 2.2.1. Критическая уязвимость (P0): Path Traversal и произвольное удаление каталогов в `instance_delete`
- **Файл и строки**: `src-tauri/src/instances/mod.rs:291–314`, `src-tauri/src/commands/mod.rs:133–136`
- **Код**:
  ```rust
  pub fn delete(paths: &crate::paths::Paths, id: &str, wipe: bool) -> Result<()> {
      let dir = instance_dir(paths, id);
      if !dir.exists() {
          return Err(LauncherError::NotFound(format!("инстанс {id}")));
      }
      if running_pid(&dir).is_some() {
          return Err(LauncherError::InvalidInput("нельзя удалить работающий инстанс: сначала остановите его".into()));
      }
      let mc = minecraft_dir(&dir);
      for shared in ["libraries", "versions", "bin"] {
          let p = mc.join(shared);
          if p.exists() {
              std::fs::remove_dir_all(long_path(&p))?;
          }
      }
      if wipe {
          std::fs::remove_dir_all(long_path(&dir))?;
      } else {
          recycle_dir(&dir)?;
      }
      Ok(())
  }
  ```
- **Механизм уязвимости**:
  1. Функция `valid_id(id)` реализована в `instances/mod.rs:181–190`, но вызывается **только** внутри `load()` и `save()`.
  2. В `delete()` вызов `valid_id(id)` **полностью отсутствует**!
  3. `instance_dir(paths, id)` выполняет `paths.instances_dir().join(id)`. В Rust на Windows метод `Path::join` с относительным путем, содержащим `..` (например, `..\backups` или `..\..\cache`), осуществляет обход каталога вверх.
  4. Если злоумышленник через скомпрометированный рендерер вызывает `invoke("instance_delete", { id: "..\\backups", wipe: true })`, условие `dir.exists()` проверяет существование каталога `backups/`, `running_pid` возвращает `None` (так как файла `.lock` там нет), после чего `std::fs::remove_dir_all` **полностью удаляет весь каталог бэкапов пользователя**!
  5. Если передан абсолютный путь Windows (например, `C:\Path\To\Dir`), в силу спецификации `PathBuf::join` в Rust аргумент `id` полностью замещает базовый путь, что при наличии прав позволяет удалить произвольную директорию на диске.

#### 2.2.2. Критическая уязвимость (P0): Инъекция команд в PowerShell через `recycle_dir`
- **Файл и строки**: `src-tauri/src/instances/mod.rs:318–335`
- **Код**:
  ```rust
  #[cfg(windows)]
  fn recycle_dir(dir: &Path) -> Result<()> {
      let ps = format!(
          "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{}', 'OnlyErrorDialogs', 'SendToRecycleBin')",
          dir.display()
      );
      let mut cmd = std::process::Command::new("powershell");
      cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps]);
      cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
      let out = cmd.output()?;
      ...
  ```
- **Механизм уязвимости**:
  Путь `dir.display()` подставляется напрямую внутрь одинарных кавычек строки PowerShell-команды.
  Если в `instance_delete` передается `id` (или путь каталога), содержащий одинарную кавычку и команды PowerShell:
  `id = "foo'; Start-Process calc; #'"`
  Сформированная команда для PowerShell принимает вид:
  `Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('C:\...\instances\foo'; Start-Process calc; #', 'OnlyErrorDialogs', 'SendToRecycleBin')`
  PowerShell парсит и выполняет точку с запятой, в результате чего процесс `powershell.exe` исполняет инъецированный пейлоад (`Start-Process calc`). Это классическая уязвимость Remote Code Execution (RCE) через IPC.

#### 2.2.3. Уязвимость высокого риска (P1): Path Traversal и произвольное исполнение файлов в `instance_open_dir`
- **Файл и строки**: `src-tauri/src/commands/mod.rs:139–144`
- **Код**:
  ```rust
  #[tauri::command]
  pub async fn instance_open_dir(state: State<'_, AppState>, id: String) -> Result<OkMsg> {
      let dir = instances::instance_dir(&state.paths, &id);
      std::fs::create_dir_all(crate::util::fs::long_path(&dir))?;
      opener::open(dir).map_err(|e| LauncherError::Internal(format!("открытие папки: {e}")))?;
      Ok(OkMsg { ok: true })
  }
  ```
- **Механизм уязвимости**:  
  Валидация `id` отсутствует. Если передать `id = "C:\\Windows\\System32\\cmd.exe"`, вызов `opener::open(dir)` запускает исполняемый файл через системный shell Windows (`ShellExecuteExW`).

---

### 2.3. Сопоставление с эталоном `resolve_content_file` (`instances/content.rs`)

Эталонная функция `resolve_content_file` (`src-tauri/src/instances/content.rs:40–56`):
```rust
fn resolve_content_file(paths: &Paths, id: &str, file: &str) -> Result<PathBuf> {
    let norm = file.replace('\\', "/");
    let parts: Vec<&str> = norm.split('/').collect();
    if parts.len() < 2
        || !CONTENT_DIRS.contains(&parts[0])
        || parts[1..].iter().any(|p| p.is_empty() || *p == ".." || p.contains(':'))
    {
        return Err(LauncherError::InvalidInput(format!(
            "недопустимый путь контента: {file}"
        )));
    }
    let mut p = minecraft_dir(&instance_dir(paths, id));
    for seg in &parts {
        p.push(seg);
    }
    Ok(p)
}
```

**Результаты сопоставления**:
1. По параметру `file`: `resolve_content_file` полностью защищает от выхода за пределы целевой директории:
   - Заменяет обратные слэши на прямые;
   - Требует минимум 2 компонента пути (`parts.len() >= 2`);
   - Разрешает только первый компонент из белого списка `CONTENT_DIRS` (`mods`, `resourcepacks`, `shaderpacks`);
   - Запрещает пустые компоненты, попытки выхода `..` и двоеточия `:`.
2. **Дефект самого эталона по параметру `id`**:
   `resolve_content_file` вызывает `instance_dir(paths, id)`, **не выполняя проверку `valid_id(id)`**! Это делает функции `content_toggle` и `content_remove` уязвимыми к передаче несанитизированного `id`.
3. Несоответствие других команд эталону:
   - Ни `instance_delete`, ни `instance_open_dir`, ни `instance_kill`, ни `content_installed` не используют принципы санитизации, примененные в `resolve_content_file` или `valid_id`.

---

## 3. Направление 3: Контракт событий и типов (Events & Types Contract)

### 3.1. Трёхстороннее сопоставление (Parity Matrix)

Сопоставлены три источника истины:
- **Rust Core**: `src-tauri/src/events.rs`, `commands/mod.rs`, `java/detect.rs`, `settings.rs`, `instances/mod.rs`
- **UI Contract**: `docs/UI-CONTRACT.md`
- **Frontend Types**: `src/api/types.ts` и `src/api/client.ts`

| Тип / Событие | Rust реализация | `docs/UI-CONTRACT.md` | `src/api/types.ts` | Статус согласованности |
|---|---|---|---|---|
| `dl_progress` | `DlProgress`<br>`camelCase` | `{event, doneFiles, totalFiles, doneBytes, totalBytes, bytesPerSec, etaSecs?}` | `DlProgressEvent`<br>`camelCase` | **Полное совпадение** |
| `dl_queue_state` | `DlQueueState`<br>`camelCase`<br>`failed_items: Vec<(String, String)>` | `{event, pending, downloading, done, failed, cancelled, failedItems}` | **ОТСУТСТВУЕТ** в `types.ts` и `onCoreEvent` | **КРИТИЧЕСКИЙ РАССИНХРОН (P1)**: Событие теряется на фронтенде |
| `launch_state` | `LaunchState`<br>`camelCase` | `{event, instanceId, phase, exitCode?}` | `LaunchStateEvent`<br>`camelCase` | **Полное совпадение** |
| `game_log_line` | `GameLogLine`<br>`camelCase` | `{event, instanceId, line, stream}` | `GameLogLineEvent`<br>`camelCase` | **Полное совпадение** |
| `account_refresh_failed`| `AccountRefreshFailed`<br>`camelCase` | `{event, accountId, reason}` | `AccountRefreshFailedEvent`<br>`camelCase` | **Полное совпадение** |
| `VersionEntry` | `commands/mod.rs:764`<br>**`release_time` (snake_case!)** | `releaseTime` (camelCase) | `releaseTime: string` (camelCase) | **ДЕФЕКТ СХЕМЫ (P1)**: в JSON приходит `release_time`, фронтенд получает `undefined` |
| `JavaInstall` | `java/detect.rs:9`<br>**`java_exe`, `arch_bits` (snake_case!)** | `javaExe`, `archBits` (camelCase) | `javaExe: string`, `archBits: number` (camelCase) | **ДЕФЕКТ СХЕМЫ (P1)**: в UI ломается отображение разрядности и выбор Java (пустой value) |
| `DeviceCodeStart` | `auth/msa.rs:18`<br>`interval`, `expires_in` | Не описан | `client.ts:120`<br>`interval_secs`, `expires_in_secs` | **РАССИНХРОН (P2)**: поля всегда `undefined` |
| `Instance` | `instances/mod.rs:18`<br>`rename_all = "camelCase"` | Полный список полей camelCase | Полный интерфейс camelCase | **Полное совпадение** |
| `Settings` | `settings.rs:29`<br>`rename_all = "camelCase"` | **Нет поля `uiScale`** (D27) | `uiScale: number` | **Пробел в документации UI-CONTRACT** |
| `ContentEntry` | `instances/mod.rs:120`<br>`rename_all = "camelCase"` | **Нет поля `url?`** (D26) | `url?: string` | **Пробел в документации UI-CONTRACT** |
| `ArchiveResult` | `instances/content.rs:32`<br>`camelCase` | **ОТСУТСТВУЕТ** в UI-CONTRACT | `ArchiveResult` в `types.ts` | **Пробел в документации UI-CONTRACT** |
| `RamGuide` | `optimize.rs:76`<br>`camelCase` | **ОТСУТСТВУЕТ** в UI-CONTRACT | `RamGuide` в `types.ts` | **Пробел в документации UI-CONTRACT** |
| `Diagnosis` | `crash.rs:6`<br>`camelCase` | **ОТСУТСТВУЕТ** в UI-CONTRACT | `Diagnosis` в `types.ts` | **Пробел в документации UI-CONTRACT** |

---

### 3.2. Детальный разбор критических расхождений

#### 3.2.1. Потеря события `dl_queue_state` на стороне клиента
- **Rust**: `src-tauri/src/events.rs:37–47, 54`, диспетчеризация в `src-tauri/src/lib.rs:921`.
- **UI-CONTRACT**: `docs/UI-CONTRACT.md:69`.
- **TypeScript**: `src/api/types.ts:115–119`, `src/api/client.ts:133–138`.
- В `src/api/client.ts` функция `onCoreEvent` подписывается только на 4 события:
  ```ts
  const unlisteners: Promise<UnlistenFn>[] = [
    listen<CoreEvent>("dl_progress", (e) => handler(e.payload)),
    listen<CoreEvent>("launch_state", (e) => handler(e.payload)),
    listen<CoreEvent>("game_log_line", (e) => handler(e.payload)),
    listen<CoreEvent>("account_refresh_failed", (e) => handler(e.payload)),
  ];
  ```
  Событие `dl_queue_state` не слушается! В `src/api/types.ts` отсутствует интерфейс `DlQueueStateEvent`, а объединение `CoreEvent` не включает данное событие. Вся телеметрия очереди загрузок ядра теряется и недоступна компонентам React.

#### 3.2.2. Сломанный контракт `JavaInstall` (`java_exe` vs `javaExe`)
- **Rust**: `src-tauri/src/java/detect.rs:9–17`
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct JavaInstall {
      pub java_exe: PathBuf,
      pub major: u32,
      pub arch_bits: u32,
      pub origin: String,
  }
  ```
  На структуре **отсутствует** атрибут `#[serde(rename_all = "camelCase")]`!
- **Frontend**: В `src/ui/components/InstanceSettingsModal.tsx:165–166`:
  ```tsx
  <option key={j.javaExe} value={j.javaExe}>
    Java {j.major} ({j.origin}, {j.archBits}-bit) — {j.javaExe}
  </option>
  ```
- **Симптом в UI**: Поскольку ядро отдает `{ "java_exe": "...", "arch_bits": 64, ... }`, поля `j.javaExe` и `j.archBits` равны `undefined`. Пользователь видит в выпадающем списке текст вида:
  `Java 21 (adoptium, undefined-bit) — `
  а выбор Java в инстансе сохраняет пустое значение `value=""`!

#### 3.2.3. Сломанный контракт `VersionEntry` (`release_time` vs `releaseTime`)
- **Rust**: `src-tauri/src/commands/mod.rs:764–770`
  ```rust
  #[derive(Serialize)]
  pub struct VersionEntry {
      pub id: String,
      #[serde(rename = "type")]
      pub version_type: String,
      pub release_time: String,
  }
  ```
  Атрибут `#[serde(rename_all = "camelCase")]` отсутствует. Поле сериализуется как `"release_time"`.
- **UI-CONTRACT и TypeScript**: `docs/UI-CONTRACT.md:55` и `src/api/types.ts:62` объявляют `releaseTime: string`.

#### 3.2.4. Катастрофическое отставание `docs/UI-CONTRACT.md` от реальности
Из **46 активных команд** ядра, зарегистрированных в `src-tauri/src/lib.rs` и вызываемых в `src/api/client.ts`, в `docs/UI-CONTRACT.md` описано всего **25 команд**!
**21 команда полностью отсутствует в спецификации контракта**:
`ram_guide`, `instance_kill`, `process_status`, `account_list`, `account_add_offline`, `account_add_msa_browser`, `account_add_ely`, `account_remove`, `account_active_set`, `crash_analyze`, `content_toggle`, `content_remove`, `instance_backup`, `instance_export`, `instance_icon_set`, `instance_icon_remove`, `instance_optimize`, `modpack_install`, `instance_import`, `account_add_msa_start`, `account_add_msa_poll`.

---

## 4. Реестр находок (A-Registry) для отчета

| № | Направление | Описание находки | Доказательство (`файл:строка`) | Приоритет | Предложение по устранению |
|---|---|---|---|---|---|
| A-C2-01 | Границы IPC | Уязвимость Path Traversal и произвольного удаления каталогов в `instance_delete` из-за отсутствия вызова `valid_id(id)`. | `instances/mod.rs:291–314`<br>`commands/mod.rs:133` | **P0** | Вызывать `valid_id(&id)?` в начале `instances::delete`. Запретить относительные сегменты `..` и двоеточия. |
| A-C2-02 | Границы IPC | Уязвимость Command Injection в `recycle_dir` через прямую интерполяцию пути в скрипт PowerShell без экранирования кавычек. | `instances/mod.rs:320–327` | **P0** | Использовать параметризованный вызов PowerShell через `-EncodedCommand` или экранировать одинарные кавычки (`replace("'", "''")`), либо вернуться к безопасному Win32 API (`SHFileOperationW` / `IFileOperation`). |
| A-C2-03 | Границы IPC | Path Traversal и запуск произвольных файлов через `opener::open` в `instance_open_dir`. | `commands/mod.rs:139–144` | **P1** | Добавить `valid_id(&id)?` перед формированием пути и открытием. |
| A-C2-04 | Корректность ядра | Паника логгера на `.unwrap()` при сбое создания `launcher.log` в CWD при старте из защищенных директорий (`Program Files`). | `src-tauri/src/lib.rs:51–53` | **P1** | Заменить fallback на `std::io::sink()` или запись в stderr без `.unwrap()`. Использовать `tracing-appender` с однократным открытием файла (D22). |
| A-C2-05 | Корректность ядра | Ложная запись отмененного мода в `content-manifest.json` при `st.cancelled > 0` в `install_recursive`. | `modrinth/install.rs:183–193` | **P1** | Добавить проверку `if st.cancelled > 0 { return Err(LauncherError::Cancelled); }` перед модификацией манифеста. |
| A-C2-06 | Корректность ядра | Отсутствие отката (`rollback_instance`) при сбое или отмене установки загрузчика/overrides в `install_mrpack`. | `modrinth/mrpack.rs:193–211` | **P2** | Обернуть шаги 5–6 в блок с гарантированным вызовом `rollback_instance` в случае возврата `Err`. |
| A-C2-07 | Корректность ядра | Утечка поврежденных файлов при сбое `std::fs::copy` в `link_or_copy` стора (ложное срабатывание `dst_l.exists()`). | `instances/store.rs:14–19` | **P1** | Копировать во временный `.part` с последующим атомарным переименованием или удалять `dst_l` при ошибке `copy`. |
| A-C2-08 | Корректность ядра | Тяжелые синхронные дисковые операции (`copy_tree_mixed`, `remove_dir_all`, `instance_list`) выполняются в async без `spawn_blocking`. | `commands/mod.rs:98, 128, 133`<br>`instances/mod.rs:265, 291` | **P2** | Обернуть `instance_duplicate`, `instance_delete`, `instance_rename`, `instance_list` в `tauri::async_runtime::spawn_blocking` по образцу `backup`/`export`. |
| A-C2-09 | Корректность ядра | Гонка между `instance_settings_set` и фоновым супервизором выхода игры при записи `instance.json`. | `commands/mod.rs:155–161`<br>`instances/run.rs:702–726` | **P2** | Ввести мьютекс на уровне инстанса в `AppState` либо выполнять селективное слияние редактируемых полей. |
| A-C2-10 | Контракт типов | Отсутствие `#[serde(rename_all = "camelCase")]` на `JavaInstall`: поломка выпадающего списка Java в UI (`java_exe` vs `javaExe`). | `java/detect.rs:9–17` | **P1** | Добавить `#[serde(rename_all = "camelCase")]` на структуру `JavaInstall`. |
| A-C2-11 | Контракт типов | Отсутствие `#[serde(rename_all = "camelCase")]` на `VersionEntry`: рассинхрон `release_time` vs `releaseTime`. | `commands/mod.rs:764–770` | **P2** | Добавить `#[serde(rename_all = "camelCase")]` на `VersionEntry`. |
| A-C2-12 | Контракт событий | Событие `dl_queue_state` не слушается в `client.ts` и отсутствует в типах `types.ts`. | `events.rs:54`<br>`api/client.ts:133–138` | **P2** | Добавить `DlQueueStateEvent` в `types.ts`, включить в `CoreEvent` и подписаться в `onCoreEvent`. |
| A-C2-13 | Контракт документации | Рассинхрон `docs/UI-CONTRACT.md`: отсутствуют 21 команда ядра и 5 ключевых интерфейсов типов. | `docs/UI-CONTRACT.md:14–62` | **P2** | Актуализировать `docs/UI-CONTRACT.md` в соответствии со всеми 46 командами ядра и актуальными D26–D30 структурами. |
