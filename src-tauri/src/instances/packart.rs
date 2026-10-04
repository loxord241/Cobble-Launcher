//! Обложки ресурспаков/шейдеров из zip (F25, D37-B). Агент B3.
//!
//! Для каждой записи манифеста с подходящим kind открываем zip-архив пакета
//! (активный файл или `.disabled` — по флагу записи, через `disk_path`) и
//! читаем `pack.png` (обложка → data-URL) и `pack.mcmeta` (описание).
//! Проблемы отдельного пакета — не ошибки: нет архива, нет pack.png, битый
//! mcmeta → запись с `None`; один битый пак не должен ломать весь список.

use crate::errors::Result;
use crate::instances::content::disk_path;
use crate::instances::{instance_dir, load, load_content_manifest, minecraft_dir, ContentKind};
use crate::paths::Paths;
use serde::Serialize;
use std::io::Read as _;

/// Лимит обложки (как у иконок инстансов): pack.png больше не превращаем
/// в data-URL — превью не будет, зато IPC-ответ и DOM не раздувает.
const MAX_PNG_BYTES: usize = 300_000;

/// Защита от zip-bomb при чтении pack.mcmeta: он вечно крошечный (<1 КБ),
/// всё, что претендует на большее, описанием не считаем.
const MAX_MCMETA_BYTES: u64 = 1_000_000;

/// Где искать обложку внутри zip: у ресурспаков pack.png в корне, у шейдеров
/// контент бывает завёрнут в `shaders/` — проверяем оба места по порядку.
const PNG_CANDIDATES: [&str; 2] = ["pack.png", "shaders/pack.png"];

/// Обложка и описание одного пакета (зеркало в UI — types.ts).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackArt {
    /// Логический путь из манифеста (например `resourcepacks/x.zip`) —
    /// UI матчит его с записями списка контента.
    pub file: String,
    /// Описание из pack.mcmeta (`string` или `{text}`), None если нет/битый.
    pub description: Option<String>,
    /// data-URL обложки, None если pack.png нет или он больше лимита.
    pub png: Option<String>,
}

/// Обложки для ресурспаков/шейдеров инстанса (kind filter снаружи).
/// Порядок результата = порядок манифеста; отсутствующий на диске файл —
/// запись с None, не ошибка.
pub fn analyze(paths: &Paths, instance_id: &str, kinds: &[ContentKind]) -> Result<Vec<PackArt>> {
    // load валидирует id (traversal-барьер для IPC) и существование инстанса.
    load(paths, instance_id)?;
    let base = minecraft_dir(&instance_dir(paths, instance_id));
    let mut out = Vec::new();
    for entry in load_content_manifest(paths, instance_id) {
        if !kinds.contains(&entry.kind) {
            continue;
        }
        // Как export_mrpack: батч-чтение по манифесту, суффикс .disabled —
        // через disk_path по флагу записи.
        let zip_path = base.join(disk_path(&entry.file, entry.enabled));
        let (png, description) = read_pack_meta(&zip_path);
        out.push(PackArt {
            file: entry.file,
            description,
            png,
        });
    }
    Ok(out)
}

/// pack.png + описание из одного zip-архива. Любая проблема — None (превью
/// нет), наружу ошибку не отдаём: список обязан быть полным.
fn read_pack_meta(zip_path: &std::path::Path) -> (Option<String>, Option<String>) {
    let file = match std::fs::File::open(crate::util::fs::long_path(zip_path)) {
        Ok(f) => f,
        Err(_) => return (None, None),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("packart: не открыть архив {:?}: {e}", zip_path);
            return (None, None);
        }
    };
    (read_pack_png(&mut archive), read_description(&mut archive))
}

/// Обложка: первый найденный из PNG_CANDIDATES, если он ≤ MAX_PNG_BYTES.
/// Размер берём из центрального каталога zip — огромную запись даже не
/// распаковываем (защита от zip-bomb), после чтения проверяем ещё раз.
fn read_pack_png(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<String> {
    for name in PNG_CANDIDATES {
        let Ok(mut entry) = archive.by_name(name) else {
            continue;
        };
        if entry.size() > MAX_PNG_BYTES as u64 {
            // Обложка есть, но слишком велика — честно остаёмся без превью.
            return None;
        }
        let mut data = Vec::new();
        if entry.read_to_end(&mut data).is_err() || data.len() > MAX_PNG_BYTES {
            return None;
        }
        // Как auth/skins.rs: фиксированный mime по имени pack.png.
        use base64::Engine as _;
        return Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&data)
        ));
    }
    None
}

/// Описание из pack.mcmeta: `{"pack":{"description": "..." | {"text":"..."}}}`.
/// Другие формы (массив текстовых компонентов, битый JSON) — None, не ошибка.
fn read_description(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<String> {
    let mut entry = archive.by_name("pack.mcmeta").ok()?;
    if entry.size() > MAX_MCMETA_BYTES {
        return None;
    }
    let mut raw = Vec::new();
    entry.read_to_end(&mut raw).ok()?;
    let meta: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let desc = &meta["pack"]["description"];
    desc.as_str()
        .map(str::to_string)
        // {"text": "..."} — форма спрайт-компонента ванильных pack.mcmeta.
        .or_else(|| desc["text"].as_str().map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::{save_content_manifest, ContentEntry, ContentSource, Instance};
    use std::io::Write as _;
    use std::path::PathBuf;

    fn setup() -> (tempfile::TempDir, Paths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let inst = Instance::new("Тест", "1.20.1");
        crate::instances::save(&paths, &inst).unwrap();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        for d in ["resourcepacks", "shaderpacks", "mods"] {
            std::fs::create_dir_all(long(&mc.join(d))).unwrap();
        }
        (dir, paths, inst)
    }

    fn long(p: &std::path::Path) -> PathBuf {
        crate::util::fs::long_path(p)
    }

    fn entry(file: &str, kind: ContentKind, enabled: bool) -> ContentEntry {
        ContentEntry {
            kind,
            file: file.into(),
            source: ContentSource::Local,
            project_id: None,
            version_id: None,
            sha1: None,
            url: None,
            enabled,
        }
    }

    /// Минимальный «PNG»: магия-заголовок — формат мы не парсим, только
    /// байты в base64 (как данные скина).
    fn png_bytes() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions = Default::default();
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn write_zip(mc: &std::path::Path, rel: &str, entries: &[(&str, &[u8])]) {
        std::fs::write(long(&mc.join(rel)), zip_bytes(entries)).unwrap();
    }

    /// Главный сценарий: pack.png (маленький) + pack.mcmeta → data-URL с теми
    /// же байтами и описание-строка.
    #[test]
    fn cover_and_description_from_zip() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        write_zip(
            &mc,
            "resourcepacks/rp.zip",
            &[
                ("pack.png", &png_bytes()),
                ("pack.mcmeta", br#"{"pack":{"description":"\u041c\u043e\u0439 \u043f\u0430\u043a"}}"#),
            ],
        );
        save_content_manifest(
            &paths,
            &inst.id,
            &[entry("resourcepacks/rp.zip", ContentKind::ResourcePack, true)],
        )
        .unwrap();

        let arts = analyze(&paths, &inst.id, &[ContentKind::ResourcePack]).unwrap();
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].file, "resourcepacks/rp.zip");
        assert_eq!(arts[0].description.as_deref(), Some("Мой пак"));
        let url = arts[0].png.as_deref().expect("обложка есть");
        assert!(url.starts_with("data:image/png;base64,"));
        let b64 = url.strip_prefix("data:image/png;base64,").unwrap();
        use base64::Engine as _;
        assert_eq!(
            base64::engine::general_purpose::STANDARD.decode(b64).unwrap(),
            png_bytes(),
            "data-URL содержит исходные байты pack.png"
        );
    }

    /// Нет pack.png → png=None, описание при этом читается; нет zip на диске →
    /// запись с None/None, analyze не падает (пустой список не ломаем).
    #[test]
    fn missing_cover_or_zip_gives_none_not_error() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        write_zip(
            &mc,
            "resourcepacks/nocover.zip",
            &[("pack.mcmeta", br#"{"pack":{"description":"\u0422\u043e\u043b\u044c\u043a\u043e \u043e\u043f\u0438\u0441\u0430\u043d\u0438\u0435"}}"#)],
        );
        save_content_manifest(
            &paths,
            &inst.id,
            &[
                entry("resourcepacks/nocover.zip", ContentKind::ResourcePack, true),
                entry("resourcepacks/ghost.zip", ContentKind::ResourcePack, true),
            ],
        )
        .unwrap();

        let arts = analyze(&paths, &inst.id, &[ContentKind::ResourcePack]).unwrap();
        assert_eq!(arts.len(), 2, "запись-призрак тоже в списке");
        assert_eq!(arts[0].png, None);
        assert_eq!(arts[0].description.as_deref(), Some("Только описание"));
        assert_eq!(arts[1].file, "resourcepacks/ghost.zip");
        assert_eq!(arts[1].png, None);
        assert_eq!(arts[1].description, None);
    }

    /// Описание объектом {"text": "..."} (форма спрайт-компонента).
    #[test]
    fn description_from_text_object() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        write_zip(
            &mc,
            "resourcepacks/obj.zip",
            &[(
                "pack.mcmeta",
                br#"{"pack":{"description":{"text":"\u0418\u0437 \u043e\u0431\u044a\u0435\u043a\u0442\u0430","color":"red"}}}"#,
            )],
        );
        save_content_manifest(
            &paths,
            &inst.id,
            &[entry("resourcepacks/obj.zip", ContentKind::ResourcePack, true)],
        )
        .unwrap();

        let arts = analyze(&paths, &inst.id, &[ContentKind::ResourcePack]).unwrap();
        assert_eq!(arts[0].description.as_deref(), Some("Из объекта"));
    }

    /// pack.png больше 300_000 байт → превью нет (None), описание остаётся.
    #[test]
    fn oversized_png_is_skipped() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        let big = vec![0xABu8; 300_001];
        write_zip(
            &mc,
            "resourcepacks/big.zip",
            &[
                ("pack.png", &big),
                ("pack.mcmeta", br#"{"pack":{"description":"\u0422\u044f\u0436\u0451\u043b\u044b\u0439 \u0430\u0440\u0442"}}"#),
            ],
        );
        save_content_manifest(
            &paths,
            &inst.id,
            &[entry("resourcepacks/big.zip", ContentKind::ResourcePack, true)],
        )
        .unwrap();

        let arts = analyze(&paths, &inst.id, &[ContentKind::ResourcePack]).unwrap();
        assert_eq!(arts[0].png, None, "большая обложка не превращается в data-URL");
        assert_eq!(arts[0].description.as_deref(), Some("Тяжёлый арт"));
    }

    /// Фильтр kind: мод не попадает; шейдер с выключенной записью читается
    /// из `.disabled`-файла, а pack.png у шейдера ищется и в shaders/.
    #[test]
    fn kind_filter_and_disabled_shader_from_shaders_dir() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        std::fs::write(long(&mc.join("mods/a.jar")), b"AAA").unwrap();
        write_zip(
            &mc,
            "resourcepacks/rp.zip",
            &[
                ("pack.png", &png_bytes()),
                ("pack.mcmeta", br#"{"pack":{"description":"\u0420\u041f"}}"#),
            ],
        );
        // pack.png не в корне, а в shaders/ — раскладка шейдерпаков.
        write_zip(&mc, "shaderpacks/sh.zip", &[("shaders/pack.png", &png_bytes())]);
        save_content_manifest(
            &paths,
            &inst.id,
            &[
                entry("mods/a.jar", ContentKind::Mod, true),
                entry("resourcepacks/rp.zip", ContentKind::ResourcePack, true),
                entry("shaderpacks/sh.zip", ContentKind::Shader, false),
            ],
        )
        .unwrap();
        // Файл шейдера переименован в .disabled (как делает toggle).
        std::fs::rename(
            long(&mc.join("shaderpacks/sh.zip")),
            long(&mc.join("shaderpacks/sh.zip.disabled")),
        )
        .unwrap();

        let arts = analyze(
            &paths,
            &inst.id,
            &[ContentKind::ResourcePack, ContentKind::Shader],
        )
        .unwrap();
        assert_eq!(arts.len(), 2, "мод отфильтрован");
        assert_eq!(arts[0].file, "resourcepacks/rp.zip");
        assert!(arts[0]
            .png
            .as_deref()
            .unwrap_or("")
            .starts_with("data:image/png;base64,"));
        // Выключенный шейдер: файл .disabled открыт, обложка найдена в shaders/.
        assert_eq!(arts[1].file, "shaderpacks/sh.zip");
        assert!(arts[1].png.is_some(), "обложка из shaders/ у .disabled-файла");
    }

    /// Шейдер с pack.png в корне тоже читается; несуществующий инстанс —
    /// NotFound, traversal-id отклоняется валидатором.
    #[test]
    fn root_cover_of_shader_and_id_validation() {
        let (_d, paths, inst) = setup();
        let mc = minecraft_dir(&instance_dir(&paths, &inst.id));
        write_zip(&mc, "shaderpacks/core.zip", &[("pack.png", &png_bytes())]);
        save_content_manifest(
            &paths,
            &inst.id,
            &[entry("shaderpacks/core.zip", ContentKind::Shader, true)],
        )
        .unwrap();

        let arts = analyze(&paths, &inst.id, &[ContentKind::Shader]).unwrap();
        assert_eq!(arts.len(), 1);
        assert!(arts[0].png.is_some(), "корневой pack.png у шейдера найден");

        assert_eq!(
            analyze(&paths, "ghost", &[]).unwrap_err().code(),
            "not_found",
            "нет инстанса — NotFound"
        );
        assert_eq!(
            analyze(&paths, "../evil", &[]).unwrap_err().code(),
            "invalid_input",
            "traversal-id отклонён"
        );
    }
}
