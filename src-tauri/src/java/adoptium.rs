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

/// Хвост ENV-аудита (D50): два параллельных install_jre одного major
/// гонялись за один `.jdk{major}.zip.part` и staging. Установки редки —
/// сериализуем их на процесс целиком; после захвата замка пере-проверяем
/// готовую Java, чтобы второй вызов просто взял результат первого.
static INSTALL_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

/// Скачать JRE `major` (windows x64, jre, hotspot) в `runtime_dir/jdk{major}`.
/// Возвращает путь к java-исполняемому файлу.
pub async fn install_jre(
    client: &HttpClient,
    runtime_dir: &Path,
    major: u32,
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<PathBuf> {
    let _guard = INSTALL_LOCK.get_or_init(|| tokio::sync::Mutex::new(())).lock().await;
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

    // 2. Скачать zip потоком во временный файл (D25: целиком в память JRE
    // не берём), SHA256 — обязательная проверка (спека §3).
    let zip_path = runtime_dir.join(format!(".jdk{major}.zip.part"));
    let zip_long = long_path(&zip_path);
    // ENV-14: недокачанный .part НЕ сносим — следующая попытка докачает
    // с обрыва (Range). Битый по хэшу/размеру .part сносит сама
    // download_to_file, так что зациклиться на мусоре нельзя.
    download_to_file(client, &binary.package.link, &binary.package, &zip_path, on_progress).await?;

    // 3. Распаковать во временную папку, перенести внутренний каталог.
    // Всё, что падает после создания staging, обязано его снести — иначе
    // в runtime_dir копится мусор `.jdk{major}-staging`.
    let staging = runtime_dir.join(format!(".jdk{major}-staging"));
    let staging_long = long_path(&staging);
    let _ = std::fs::remove_dir_all(&staging_long);
    std::fs::create_dir_all(&staging_long)?;
    let unpacked: Result<()> = (|| {
        let extracted =
            crate::util::zip::extract_zip(std::fs::File::open(&zip_long)?, &staging, |_| {})?;
        let _ = std::fs::remove_file(&zip_long);
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
            return Err(LauncherError::Zip(
                "в архиве Adoptium не найден корневой каталог".into(),
            ));
        };
        // ENV-11a: свежие dll легко придерживает антивирус — перенос
        // с повторами (A26), голый rename отказывал установку на ровном месте.
        crate::util::fs::rename_with_retry(&inner, &target)
    })();
    if let Err(e) = unpacked {
        let _ = std::fs::remove_dir_all(&staging_long);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&staging_long);

    if !java_exe.exists() {
        return Err(LauncherError::JavaNotFound(format!(
            "после распаковки нет {}",
            java_exe.display()
        )));
    }
    Ok(java_exe)
}

/// D62: три ветки GET ниже (Range-докачка / 416 / с нуля) страхуются
/// `send_timed` — у клиента общего таймаута больше нет, а голый `send`
/// на молчащем сервере подвешивал установку навечно.
async fn download_to_file(
    client: &HttpClient,
    url: &str,
    pkg: &AdoptiumPackage,
    dest: &std::path::Path,
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<()> {
    use futures::StreamExt;
    use sha2::Digest;
    use std::io::{Read as _, Write as _};
    // F16: офлайн-режим — мгновенный честный отказ вместо сетевого таймаута.
    if client.offline() {
        return Err(LauncherError::OfflineMode(url.to_string()));
    }
    let dest_long = long_path(dest);
    // ENV-14: докачка — если .part остался с прошлого раза (обрыв на середине),
    // просим у сервера только хвост. SHA256-проверка ниже остаётся страховкой
    // от битой докачки.
    let mut resumed: u64 = std::fs::metadata(&dest_long).map(|m| m.len()).unwrap_or(0);
    // D62: только send под TTFB-предохранителем (30 с) — у него своего
    // таймаута нет; READ_IDLE ниже страхует лишь чтение тела стрима.
    // Ветвление Range/206/416 не тронуто.
    let resp = if resumed > 0 {
        let r = client
            .send_timed(
                client
                    .raw()
                    .get(url)
                    .header(reqwest::header::RANGE, format!("bytes={resumed}-")),
            )
            .await?;
        if r.status().as_u16() == 416 {
            // Диапазон за концом файла: .part уже полон или мусорен —
            // начинаем с нуля.
            let _ = std::fs::remove_file(&dest_long);
            resumed = 0;
            client.send_timed(client.raw().get(url)).await?
        } else {
            r
        }
    } else {
        client.send_timed(client.raw().get(url)).await?
    };
    if !resp.status().is_success() {
        return Err(LauncherError::network(format!(
            "HTTP {} для {url}",
            resp.status()
        )));
    }
    // 206 — сервер понял Range и отдаёт хвост, дописываем; 200 — Range
    // проигнорирован, усекаем и качаем целиком.
    let resume = resp.status().as_u16() == 206 && resumed > 0;
    if !resume {
        resumed = 0;
    }
    let expected_size = pkg.size.unwrap_or(0);
    let mut hasher = sha2::Sha256::new();
    let mut out = if resume {
        // Один хендл: существующая часть уходит в хэш (итог — полный SHA256
        // файла), запись идёт в append-режиме — всегда в конец.
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .open(&dest_long)?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        f
    } else {
        std::fs::File::create(&dest_long)?
    };
    let mut done: u64 = resumed;
    let mut stream = resp.bytes_stream();
    // Тишина в стриме дольше 2 минут = «сервер не отвечает» (зеркало
    // READ_IDLE движка): общий таймаут у HttpClient снят (аудит 2026-10-06),
    // без этого капа молчащий сервер подвешивал установку JRE навечно.
    const READ_IDLE: std::time::Duration = std::time::Duration::from_secs(120);
    while let Some(chunk) = tokio::time::timeout(READ_IDLE, stream.next())
        .await
        .map_err(|_| crate::errors::LauncherError::network("Adoptium не отвечает"))?
    {
        let chunk = chunk?;
        hasher.update(&chunk);
        out.write_all(&chunk)?;
        done += chunk.len() as u64;
        on_progress(done, expected_size);
    }
    out.flush()?;

    if let Some(checksum) = &pkg.checksum {
        // Adoptium даёт sha256 в metadata — сверяем (по спеке можно было бы
        // ограничиться размером, но хэш честнее).
        let actual = hex::encode(hasher.finalize());
        let expected = checksum.trim().to_lowercase();
        if actual != expected {
            // Может быть sha256 в формате base64? Нет — hex; но некоторые
            // выпуски дают `<sha256>:<filename>`? Парсим последний hex-токен.
            let tail = expected.rsplit(':').next().unwrap_or(&expected);
            if actual != tail {
                // ENV-14: битая докачка не должна зациклиться — .part сносим,
                // следующая попытка качает с нуля.
                drop(out);
                let _ = std::fs::remove_file(&dest_long);
                return Err(LauncherError::HashMismatch {
                    path: pkg.name.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }
    } else if expected_size > 0 && done != expected_size {
        // Тот же принцип: неверный итоговый размер — мусор не оставляем.
        drop(out);
        let _ = std::fs::remove_file(&dest_long);
        return Err(LauncherError::network(format!(
            "размер JRE {done} != ожидаемого {expected_size}"
        )));
    }
    Ok(())
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

    /// ENV-14: докачка. Недокачанный .part дописывается по Range (206) и
    /// проходит SHA256; битый .part даёт HashMismatch и сносится (без вечного
    /// цикла на мусорном файле); сервер без поддержки Range (200) заставляет
    /// качать с нуля поверх. Сеть — локальный mock, внешней нет.
    #[tokio::test]
    async fn download_to_file_resumes_and_discards_corrupt_part() {
        use sha2::Digest as _;
        use std::sync::{Arc, Mutex};

        let seen_range: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let seen_srv = seen_range.clone();
        let app = axum::Router::new().route(
            "/jre.zip",
            axum::routing::get(move |headers: axum::http::HeaderMap| {
                let seen = seen_srv.clone();
                async move {
                    let range = headers
                        .get(axum::http::header::RANGE)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());
                    *seen.lock().unwrap() = range.clone();
                    if range.as_deref() == Some("bytes=4-") {
                        // Сервер поддержал Range: отдаём только хвост.
                        (axum::http::StatusCode::PARTIAL_CONTENT, "EFGH".to_string())
                    } else {
                        // Range проигнорирован: файл целиком.
                        (axum::http::StatusCode::OK, "ABCDEFGH".to_string())
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let url = format!("http://{addr}/jre.zip");
        let client = HttpClient::new(None).unwrap();

        let mut hasher = sha2::Sha256::new();
        hasher.update(b"ABCDEFGH");
        let pkg = AdoptiumPackage {
            name: "jre.zip".into(),
            link: url.clone(),
            checksum: Some(hex::encode(hasher.finalize())),
            size: Some(8),
        };

        // 1) Обрыв на 4 байтах ("ABCD"): докачка хвоста "EFGH" по Range.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("jre.zip.part");
        std::fs::write(&dest, b"ABCD").unwrap();
        download_to_file(&client, &url, &pkg, &dest, |_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"ABCDEFGH");
        assert_eq!(seen_range.lock().unwrap().as_deref(), Some("bytes=4-"));

        // 2) Битая докачка ("XXXX" + "EFGH"): хэш не сходится, .part снесён —
        // вечный цикл на мусорном файле невозможен.
        std::fs::write(&dest, b"XXXX").unwrap();
        let err = download_to_file(&client, &url, &pkg, &dest, |_, _| {})
            .await
            .unwrap_err();
        assert!(err.to_string().contains("хэш"), "ожидался HashMismatch: {err}");
        assert!(!dest.exists(), "битый .part обязан быть снесён");

        // 3) Сервер игнорирует Range (200 на bytes=5-): усечение и полная
        // скачка — файл ровно 8 байт, без «хвостов» от append.
        std::fs::write(&dest, b"ABCDE").unwrap();
        download_to_file(&client, &url, &pkg, &dest, |_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"ABCDEFGH");
    }
}
