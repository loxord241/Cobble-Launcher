# Handoff Report: Explorer Cluster 2 (Directions 1, 2, 3)

## 1. Observation
1. **Отсутствие валидации `id` в `instance_delete`**:  
   В `src-tauri/src/instances/mod.rs:291-314` метод `delete` принимает `id: &str`. Метод `valid_id(id)` (строки 181–190) в нем не вызывается. `instance_dir(paths, id)` (строка 93) делает `paths.instances_dir().join(id)`.
2. **Инъекция PowerShell в `recycle_dir`**:  
   В `src-tauri/src/instances/mod.rs:320-327` путь форматируется прямо в строку команды PowerShell:
   ```rust
   let ps = format!(
       "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{}', 'OnlyErrorDialogs', 'SendToRecycleBin')",
       dir.display()
   );
   ```
3. **Отсутствие валидации `id` в `instance_open_dir`**:  
   В `src-tauri/src/commands/mod.rs:139-144` вызов `opener::open(dir)` производится без проверки `valid_id`.
4. **Паника на `.unwrap()` в логгере**:  
   В `src-tauri/src/lib.rs:52` в реализации `make_writer`:
   ```rust
   .unwrap_or_else(|_| std::fs::File::create(std::path::PathBuf::from("launcher.log")).unwrap());
   ```
5. **Тяжелые синхронные блокировки в асинхронных командах**:  
   В `src-tauri/src/commands/mod.rs:128-130` команда `instance_duplicate` напрямую вызывает `store::copy_tree_mixed(&src, &dst)` (полное рекурсивное копирование дерева файлов инстанса) без `spawn_blocking`. В строках 98 (`instance_list`) и 133 (`instance_delete`) также отсутствует `spawn_blocking`.
6. **Ложная запись в манифест при отмене установки мода**:  
   В `src-tauri/src/modrinth/install.rs:183-208` код проверяет `if st.failed > 0`, но игнорирует `st.cancelled > 0` и безусловно выполняет `manifest.push(entry)` и `save_content_manifest`.
7. **Отказ отката при сбое модпака**:  
   В `src-tauri/src/modrinth/mrpack.rs:194` вызывается `save(paths, &inst)?;`, после чего при ошибке в `install_loader` (строка 198) или `apply_overrides` (строка 210) `rollback_instance` не вызывается.
8. **Повреждение кэша в сторе**:  
   В `src-tauri/src/instances/store.rs:14-19` при падении `std::fs::copy` битый файл остается по пути `dst`. При следующем вызове проверка `if dst_l.exists()` пропускает копирование.
9. **Рассинхрон сериализации `JavaInstall` и `VersionEntry`**:  
   В `src-tauri/src/java/detect.rs:9-17` поля `java_exe` и `arch_bits` не имеют `camelCase` и сериализуются в snake_case, тогда как `src/api/types.ts:70-75` и `src/ui/components/InstanceSettingsModal.tsx:165-166` ожидают `j.javaExe` и `j.archBits`. В `src-tauri/src/commands/mod.rs:764-770` `VersionEntry` сериализует `release_time` вместо `releaseTime`.
10. **Потеря события `dl_queue_state`**:  
    В `src-tauri/src/events.rs:54` событие объявлено, в `src-tauri/src/lib.rs:921` эмитится, но в `src/api/client.ts:133-138` отсутствует в подписках `onCoreEvent`, а в `src/api/types.ts` отсутствует тип `DlQueueStateEvent`.
11. **Отставание `docs/UI-CONTRACT.md`**:  
    В `src-tauri/src/lib.rs` зарегистрировано 46 команд; в `docs/UI-CONTRACT.md` описано только 25 команд (21 команда отсутствует).

## 2. Logic Chain
1. Из Наблюдения 1 и 2: Вызов `instance_delete` с `id = "..\\backups"` и `wipe = true` обходит проверку `dir.exists()` и удаляет директорию `backups/`. При `wipe = false` передача `id` с одинарной кавычкой (`'`) внедряет произвольные команды в процесс PowerShell (`powershell -Command`), что является RCE-уязвимостью через IPC.
2. Из Наблюдения 3: Вызов `instance_open_dir` с путем к любому исполняемому файлу (`.exe`) запускает его через системный shell (`opener::open`).
3. Из Наблюдения 4: При запуске лаунчера из каталога `Program Files` (где нет прав на запись в CWD) сбой открытия лога в `logs_dir` приводит к вызову `File::create("launcher.log").unwrap()`, что вызывает панику и краш приложения при первой же записи лога.
4. Из Наблюдения 5: Синхронные вызовы `copy_tree_mixed` и `remove_dir_all` блокируют поток Tokio worker, замораживая отзывчивость интерфейса лаунчера.
5. Из Наблюдения 6: Отмена загрузки мода пользователем оставляет запись в `content-manifest.json`, из-за чего лаунчер считает мод установленным, хотя файла на диске нет.
6. Из Наблюдения 8: При сбое копирования игра в дальнейшем пытается запуститься с частично скопированным `.jar`.
7. Из Наблюдения 9: Из-за отсутствия `#[serde(rename_all = "camelCase")]` фронтенд получает `java_exe` вместо `javaExe`, поэтому выбор Java в `InstanceSettingsModal` передает пустое значение, а разрядность отображается как `undefined-bit`.

## 3. Caveats
- Аудит проводился исключительно методом статического и семантического анализа исходного кода (Read-Only) без модификации файлов `src/` и `src-tauri/src/`.
- Реальные запуски игрового клиента не производились в соответствии с ограничениями задачи.
- Анализ поведения PowerShell производился с учетом спецификации синтаксиса PowerShell 5.1/7.x на Windows 10/11.

## 4. Conclusion
1. **Безопасность IPC**: Выявлены 2 критические уязвимости (P0) — Path Traversal / Arbitrary Deletion и Command Injection в `instance_delete` / `recycle_dir`, а также 1 уязвимость высокого риска (P1) в `instance_open_dir`. Требуется обязательный вызов `valid_id` на всех входах и экранирование/замена PowerShell.
2. **Корректность ядра Rust**: Требуется убрать паникующий `.unwrap()` в `lib.rs:52`, обернуть `instance_duplicate`/`delete`/`list` в `spawn_blocking`, исправить обработку `st.cancelled` в `install_recursive`, а также очищать поврежденные файлы при сбое `std::fs::copy`.
3. **Контракт типов и событий**: Требуется добавить `#[serde(rename_all = "camelCase")]` на `JavaInstall` и `VersionEntry`, добавить `DlQueueStateEvent` в `types.ts` и `onCoreEvent`, а также синхронизировать `docs/UI-CONTRACT.md` с фактическими 46 командами ядра.

## 5. Verification Method
1. **Проверка уязвимости Path Traversal**:
   Запустить юнит-тест `cargo test --lib instances::tests::invalid_id_rejected` и убедиться, что тест проверяет только `load()`, но не `delete()`.
2. **Проверка рассинхрона `JavaInstall`**:
   Выполнить команду инспекции схемы `cargo test` или проверить сериализацию:
   ```rust
   let j = JavaInstall { java_exe: PathBuf::from("java.exe"), major: 21, arch_bits: 64, origin: "adoptium".into() };
   assert!(serde_json::to_string(&j).unwrap().contains("\"java_exe\"")); // подтверждает snake_case
   ```
3. **Проверка паники логгера**:
   Осмотреть `src-tauri/src/lib.rs:52`: наличие `.unwrap()` на ветке fallback.
4. **Сверка контракта**:
   Сравнить список команд `src-tauri/src/lib.rs:104-151` (46 команд) со списком в `docs/UI-CONTRACT.md:14-38` (25 команд).
