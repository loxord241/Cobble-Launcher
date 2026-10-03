//! Интеграционные тесты движка загрузок против локального mock-сервера
//! (спека §9: таймауты, 429, битый хэш, обрыв соединения, resume).

use mc_launcher_v2_lib::events::EventBus;
use mc_launcher_v2_lib::net::download::{DownloadEngine, DownloadTask};
use mc_launcher_v2_lib::net::http::HttpClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const GOOD: &[u8] = b"hello minecraft launcher content";

fn sha1_of(data: &[u8]) -> String {
    mc_launcher_v2_lib::util::fs::sha1_bytes(data)
}

fn task(id: &str, url: String, dest: std::path::PathBuf, sha1: Option<String>) -> DownloadTask {
    DownloadTask {
        id: id.into(),
        url,
        dest,
        sha1,
        sha512: None,
        size: None,
        group: "test".into(),
        priority: 1,
    }
}

/// Поднять mock-сервер, вернуть базовый URL.
async fn spawn_axum(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Сервер, который обрывает соединение до полного тела (Content-Length врёт).
async fn spawn_truncating(times: usize) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..times {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nshort-but-wrong")
                .await;
            let _ = sock.shutdown().await;
        }
    });
    format!("http://{addr}")
}

fn engine_with(client: Arc<HttpClient>) -> Arc<DownloadEngine> {
    DownloadEngine::new(client, 4, EventBus::default())
}

#[tokio::test]
async fn ok_download_verifies_hash_and_writes_file() {
    let base = spawn_axum(axum::Router::new().route(
        "/good",
        axum::routing::get(|| async { axum::body::Body::from(GOOD.to_vec()) }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    engine.add_tasks(vec![task(
        "t1",
        format!("{base}/good"),
        dest.clone(),
        Some(sha1_of(GOOD)),
    )]);
    engine.run().await.unwrap();
    let st = engine.queue_state();
    assert_eq!(st.done, 1, "state: {st:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), GOOD);
}

#[tokio::test]
async fn hash_mismatch_recovers_on_retry() {
    // Первая попытка отдаёт битое содержимое, вторая — правильное:
    // движок обязан обнаружить несовпадение и перекачать (спека §14).
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let c2 = counter.clone();
    let app = axum::Router::new().route(
        "/flaky-hash",
        axum::routing::get(move || {
            let n = c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if n == 0 {
                    axum::body::Body::from(b"corrupted bytes".to_vec())
                } else {
                    axum::body::Body::from(GOOD.to_vec())
                }
            }
        }),
    );
    let base = spawn_axum(app).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    engine.add_tasks(vec![task(
        "t1",
        format!("{base}/flaky-hash"),
        dest.clone(),
        Some(sha1_of(GOOD)),
    )]);
    engine.run().await.unwrap();
    let st = engine.queue_state();
    assert_eq!(st.done, 1, "должен перекачать и succeed: {st:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), GOOD);
}

#[tokio::test]
async fn permanent_hash_mismatch_fails_task() {
    let base = spawn_axum(axum::Router::new().route(
        "/always-bad",
        axum::routing::get(|| async { axum::body::Body::from(b"always wrong".to_vec()) }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    engine.add_tasks(vec![task(
        "t1",
        format!("{base}/always-bad"),
        dir.path().join("out.bin"),
        Some(sha1_of(GOOD)),
    )]);
    engine.run().await.unwrap();
    let st = engine.queue_state();
    assert_eq!(st.failed, 1);
    assert!(st.failed_items[0].1.contains("хэш не совпал"), "{st:?}");
    // .part не остаётся
    assert!(!dir.path().join("out.bin.part").exists());
}

#[tokio::test]
async fn http_429_retries_with_retry_after() {
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let c2 = counter.clone();
    let app = axum::Router::new().route(
        "/limited",
        axum::routing::get(move || {
            let n = c2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if n < 2 {
                    (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        [("Retry-After", "0")],
                        axum::body::Body::empty(),
                    )
                } else {
                    (
                        axum::http::StatusCode::OK,
                        [("Retry-After", "0")],
                        axum::body::Body::from(GOOD.to_vec()),
                    )
                }
            }
        }),
    );
    let base = spawn_axum(app).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    engine.add_tasks(vec![task(
        "t1",
        format!("{base}/limited"),
        dir.path().join("out.bin"),
        Some(sha1_of(GOOD)),
    )]);
    engine.run().await.unwrap();
    assert_eq!(engine.queue_state().done, 1);
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn connection_break_fails_after_retries() {
    let base = spawn_truncating(10).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    engine.add_tasks(vec![task(
        "t1",
        format!("{base}/file"),
        dir.path().join("out.bin"),
        Some(sha1_of(GOOD)),
    )]);
    engine.run().await.unwrap();
    let st = engine.queue_state();
    assert_eq!(st.failed, 1, "обрыв соединения → failed: {st:?}");
    assert!(!dir.path().join("out.bin.part").exists(), ".part чистится");
}

#[tokio::test]
async fn timeout_fails_task() {
    let app = axum::Router::new().route(
        "/slow",
        axum::routing::get(|| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "late"
        }),
    );
    let base = spawn_axum(app).await;
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(HttpClient::with_timeouts(None, Duration::from_secs(1), Duration::from_millis(300)).unwrap());
    let engine = engine_with(client);
    engine.add_tasks(vec![task(
        "t1",
        format!("{base}/slow"),
        dir.path().join("out.bin"),
        None,
    )]);
    engine.run().await.unwrap();
    assert_eq!(engine.queue_state().failed, 1);
}

#[tokio::test]
async fn existing_valid_file_skips_download() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("cached.bin");
    std::fs::write(&dest, GOOD).unwrap();
    // Сервер на несуществующем порту: если движок пойдёт в сеть — тест упадёт.
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    engine.add_tasks(vec![task(
        "t1",
        "http://127.0.0.1:1/nope".into(),
        dest.clone(),
        Some(sha1_of(GOOD)),
    )]);
    engine.run().await.unwrap();
    assert_eq!(engine.queue_state().done, 1, "resume: файл уже валиден");
}

#[tokio::test]
async fn cancel_group_stops_and_cleans_parts() {
    use futures::StreamExt as _;
    let app = axum::Router::new().route(
        "/trickle",
        axum::routing::get(|| async {
            let stream = futures::stream::repeat_with(|| Ok::<_, std::io::Error>(b"chunk".to_vec()))
                .take(200)
                .then(|c| async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    c
                });
            axum::body::Body::from_stream(stream)
        }),
    );
    let base = spawn_axum(app).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    engine.add_tasks(vec![task(
        "t1",
        format!("{base}/trickle"),
        dir.path().join("out.bin"),
        None,
    )]);
    // A42: ждём не фиксированные 400 мс, а появление `.part` — то есть момент,
    // когда загрузка реально началась и флаг группы уже существует. Иначе на
    // медленной машине отмена приходила раньше старта задачи, и тест флапал.
    let part = dir.path().join("out.bin.part");
    let e2 = engine.clone();
    let cancel_task = tokio::spawn(async move {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !part.exists() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        e2.cancel_group("test");
    });
    engine.run().await.unwrap();
    cancel_task.await.unwrap();
    let st = engine.queue_state();
    assert_eq!(st.cancelled, 1, "отмена группы → Cancelled: {st:?}");
    assert!(!dir.path().join("out.bin.part").exists(), ".part удалён");
}

#[tokio::test]
async fn dedup_same_url_and_dest() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    let t = task("a", "http://127.0.0.1:1/x".into(), dir.path().join("x"), None);
    let t2 = task("b", "http://127.0.0.1:1/x".into(), dir.path().join("x"), None);
    let added = engine.add_tasks(vec![t, t2]);
    assert_eq!(added, 1, "одинаковые URL/dest не качаются дважды");
}

/// SHA512: если sha1 не задан, движок обязан сверять sha512 (в index.json
/// модпаков бывает только он). Неверный sha512 → HashMismatch, файл НЕ
/// переименовывается в dest и `.part` не остаётся. sha2 нет в
/// dev-dependencies, поэтому валидное значение не считаем — фиксированное
/// тело гарантированно не совпадёт с заведомо неверным хэшем.
#[tokio::test]
async fn sha512_mismatch_fails_task_and_leaves_no_file() {
    let base = spawn_axum(axum::Router::new().route(
        "/sha512-bad",
        axum::routing::get(|| async { axum::body::Body::from(GOOD.to_vec()) }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let engine = engine_with(Arc::new(HttpClient::new(None).unwrap()));
    let mut t = task("t1", format!("{base}/sha512-bad"), dest.clone(), None);
    t.sha512 = Some("0".repeat(128)); // заведомо неверный SHA512 (128 hex-символов)
    engine.add_tasks(vec![t]);
    engine.run().await.unwrap();
    let st = engine.queue_state();
    assert_eq!(st.failed, 1, "неверный sha512 → failed: {st:?}");
    assert!(
        st.failed_items[0].1.contains("хэш не совпал"),
        "ожидался HashMismatch: {st:?}"
    );
    assert!(!dest.exists(), "битый файл не должен попасть в dest");
    assert!(!dir.path().join("out.bin.part").exists(), ".part чистится");
}
