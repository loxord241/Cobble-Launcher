//! Скачивание JRE с Adoptium по majorVersion (спека §6.5).
//! Хэш — SHA256 из metadata Adoptium (сильнее требуемого спекой сверки размера).

use crate::errors::{LauncherError, Result};
use crate::net::http::HttpClient;
use crate::util::fs::long_path;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const ASSETS_URL: &str = "https://api.adoptium.net/v3/assets/latest";

/// Реальный ответ `assets/latest/{major}/hotspot`: массив записей, в каждой —
/// ОДИН бинарник в поле `binary` (не массив, как можно было бы ожидать).
#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
    #[serde(rename = "release_name")]
    #[allow(dead_code)]
    release_name: String,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    #[serde(rename = "image_type")]
    image_type: String,
    os: String,
    architecture: String,
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    name: String,
    link: String,
    #[serde(default)]
    checksum: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

/// Скачать JRE `major` (windows x64, jre, hotspot) в `runtime_dir/jdk{major}`.
/// Возвращает путь к java-исполняемому файлу.
pub async fn install_jre(
    client: &HttpClient,
    runtime_dir: &Path,
    major: u32,
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<PathBuf> {
    let target = runtime_dir.join(format!("jdk{major}"));
    let java_exe = if cfg!(windows) {
        target.join("bin").join("java.exe")
    } else {
        target.join("bin").join("java")
    };
    if java_exe.exists() {
        return Ok(java_exe); // переиспользование между инстансами
    }

    // 1. Metadata: список бинарников последнего GA релиза.
    let url = format!("{ASSETS_URL}/{major}/hotspot");
    let assets: Vec<AdoptiumAsset> = client.get_json_retry(&url).await?;
    let Some(binary) = assets
        .iter()
        .map(|a| &a.binary)
        .find(|b| {
            b.image_type == "jre"
                && b.os == "windows"
                && b.architecture == "x64"
                && b.package.name.ends_with(".zip")
        })
    else {
        return Err(LauncherError::JavaNotFound(format!(
            "Adoptium не отдал JRE windows/x64 для Java {major}"
        )));
    };

    // 2. Скачать zip (обязательная проверка SHA256 — спека §3).
    let zip_data = download_with_progress(client, &binary.package.link, &binary.package, on_progress)
        .await?;

    // 3. Распаковать во временную папку, перенести внутренний каталог.
    let staging = runtime_dir.join(format!(".jdk{major}-staging"));
    let staging_long = long_path(&staging);
    let _ = std::fs::remove_dir_all(&staging_long);
    std::fs::create_dir_all(&staging_long)?;
    let extracted = crate::util::zip::extract_zip(std::io::Cursor::new(&zip_data), &staging, |_| {})?;
    let _ = std::fs::remove_dir_all(long_path(&target));
    // Внутренний каталог вида `jdk-25.0.2+10-jre` — единственный в staging.
    let mut inner: Option<PathBuf> = None;
    for e in extracted.iter() {
        // пути относительны staging; берём первый компонент любого файла
        if let Ok(rel) = e.strip_prefix(&staging) {
            if let Some(first) = rel.components().next() {
                let cand = staging.join(first);
                if cand.is_dir() {
                    inner = Some(cand);
                }
            }
        }
    }
    let Some(inner) = inner else {
        let _ = std::fs::remove_dir_all(&staging_long);
        return Err(LauncherError::Zip(
            "в архиве Adoptium не найден корневой каталог".into(),
        ));
    };
    std::fs::rename(long_path(&inner), long_path(&target))?;
    let _ = std::fs::remove_dir_all(&staging_long);

    if !java_exe.exists() {
        return Err(LauncherError::JavaNotFound(format!(
            "после распаковки нет {}",
            java_exe.display()
        )));
    }
    Ok(java_exe)
}

async fn download_with_progress(
    client: &HttpClient,
    url: &str,
    pkg: &AdoptiumPackage,
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<Vec<u8>> {
    use futures::StreamExt;
    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode(url.to_string()));
    }
    let resp = client.raw().get(url).send().await?;
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {url}",
            resp.status()
        )));
    }
    let expected_size = pkg.size.unwrap_or(0);
    let mut data = Vec::with_capacity(expected_size as usize);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        data.extend_from_slice(&chunk);
        on_progress(data.len() as u64, expected_size);
    }

    if let Some(checksum) = &pkg.checksum {
        // Adoptium даёт sha256 в metadata — сверяем (по спеке можно было бы
        // ограничиться размером, но хэш честнее).
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(&data);
        let actual = hex::encode(hasher.finalize());
        let expected = checksum.trim().to_lowercase();
        if actual != expected {
            // Может быть sha256 в формате base64? Нет — hex; но некоторые
            // выпуски дают `<sha256>:<filename>`? Парсим последний hex-токен.
            let tail = expected.rsplit(':').next().unwrap_or(&expected);
            if actual != tail {
                return Err(LauncherError::HashMismatch {
                    path: pkg.name.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }
    } else if expected_size > 0 && data.len() as u64 != expected_size {
        return Err(LauncherError::network(format!(
            "размер JRE {} != ожидаемого {expected_size}",
            data.len()
        )));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Реальный запуск: скачивание Adoptium против сети — #[ignore],
    /// гонять вручную на приёмке (спека §9).
    #[tokio::test]
    #[ignore]
    async fn installs_real_jre_17() {
        let dir = tempfile::tempdir().unwrap();
        let client = HttpClient::new(None).unwrap();
        let java = install_jre(&client, dir.path(), 17, |done, total| {
            println!("{done}/{total}");
        })
        .await
        .unwrap();
        assert!(java.exists());
        let inst = crate::java::detect::inspect(&java, "adoptium").await.unwrap();
        assert_eq!(inst.major, 17);
    }

    #[test]
    fn adoptium_metadata_parses() {
        // Реальная форма ответа: массив записей с singular `binary`.
        let j = r#"[
          {"release_name":"jdk-17.0.20.1+1","vendor":"eclipse",
           "binary":{"image_type":"jre","os":"windows","architecture":"x64",
             "heap_size":"normal","jvm_impl":"hotspot","project":"jdk",
             "package":{"name":"OpenJDK17U-jre_x64_windows_hotspot_17.0.20.1_1.zip",
                        "link":"https://github.com/adoptium/.../x.zip",
                        "checksum":"bc21a93923103cda","size":43780109}}
          }
        ]"#;
        let assets: Vec<AdoptiumAsset> = serde_json::from_str(j).unwrap();
        assert_eq!(assets[0].binary.image_type, "jre");
        assert_eq!(assets[0].binary.package.size, Some(43780109));
    }
}
