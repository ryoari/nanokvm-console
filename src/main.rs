mod bridge;
mod device;
mod guard;
mod profile;
use axum::{
    Json, Router,
    extract::{Request, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::Mutex};
use tokio_util::sync::CancellationToken;

type Result<T> = std::result::Result<T, &'static str>;
struct App {
    token: String,
    launch: String,
    session: String,
    cookie: String,
    host: String,
    live: bool,
    profile: PathBuf,
    owner: Mutex<()>,
    status: Mutex<Value>,
    health: Mutex<Value>,
    diagnostic_lock: Mutex<()>,
    stop: CancellationToken,
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
fn header_text<'a>(h: &'a HeaderMap, name: &str) -> &'a str {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}
fn authenticated(h: &HeaderMap, a: &App) -> bool {
    header_text(h, "cookie").split(';').any(|part| {
        part.trim()
            .split_once('=')
            .is_some_and(|(k, v)| k == a.cookie && equal(v, &a.session))
    })
}
fn denied() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"detail":"Use the private local launch link."})),
    )
        .into_response()
}
async fn protect(State(a): State<Arc<App>>, request: Request, next: Next) -> Response {
    let h = request.headers();
    if header_text(h, "host") != a.host {
        return denied();
    }
    let origin = header_text(h, "origin");
    if !origin.is_empty() && origin != format!("http://{}", a.host) {
        return denied();
    }
    if request.uri().path().starts_with("/api/")
        && request.uri().path() != "/api/session"
        && !authenticated(h, &a)
    {
        return denied();
    }
    let mut response = next.run(request).await;
    for (k, v) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    ] {
        response.headers_mut().insert(k, v.parse().unwrap());
    }
    response
}
async fn session(State(a): State<Arc<App>>, h: HeaderMap) -> Response {
    if !equal(header_text(&h, "x-launch-token"), &a.launch) && !authenticated(&h, &a) {
        return denied();
    }
    let mut r=Json(json!({"token":a.token,"mode":if a.live{"live"}else{"demo"},"target":"configured-device","implementation":"rust", "vendor_url": profile::Profile::load(&a.profile).ok().and_then(|p|p.endpoints.first().cloned())})).into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        format!(
            "{}={}; HttpOnly; SameSite=Strict; Path=/",
            a.cookie, a.session
        )
        .parse()
        .unwrap(),
    );
    r
}
async fn status(State(a): State<Arc<App>>) -> Json<Value> {
    let Ok(_check) = a.diagnostic_lock.try_lock() else {
        return Json(
            json!({"checks":{"connection":{"ok":false,"detail":"A connection check is already running. Wait for it to finish."}},"lean_guard_ready":false}),
        );
    };
    let mut r = if a.live {
        match tokio::time::timeout(
            std::time::Duration::from_secs(35),
            device::diagnostics(&a.profile),
        )
        .await
        {
            Ok(value) => value,
            Err(_) => {
                json!({"checks":{"connection":{"ok":false,"detail":"Connection check exceeded its 35-second deadline. Retry after checking the route."}},"lean_guard_ready":guard::ready()})
            }
        }
    } else {
        json!({"source":"synthetic demo","checks":{"connection":{"ok":false,"detail":"Demo does not access hardware."}},"lean_guard_ready":guard::ready(),"implementation":"rust"})
    };
    let observed = &r["checks"]["capture_service"]["data"];
    let mut previous = a.health.lock().await;
    if let (Some(boot), Some(uptime)) = (
        observed["boot_id"].as_str(),
        observed["uptime_seconds"].as_f64(),
    ) {
        let changed = previous["boot_id"].as_str().is_some_and(|old| old != boot);
        *previous = json!({"boot_id":boot,"uptime_seconds":uptime});
        r["checks"]["restart"] = json!({"ok":!changed,"data":*previous,"detail":if changed {format!("Device reboot detected since the previous check. Current uptime: {:.0} seconds. The cause is not established. Check the power supply and device logs.",uptime)}else{format!("Current uptime: {:.0} seconds. No reboot observed between available checks. Wi-Fi loss alone does not establish a reboot.",uptime)}});
    }
    r["session"] = a.status.lock().await.clone();
    r["build_pins"] = serde_json::from_str(guard::PINS).unwrap();
    Json(r)
}
async fn socket(State(a): State<Arc<App>>, h: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if header_text(&h, "origin") != format!("http://{}", a.host) || !authenticated(&h, &a) {
        return denied();
    }
    ws.max_message_size(65536)
        .max_frame_size(65536)
        .on_upgrade(move |socket| bridge::serve(socket, a))
}
fn router(a: Arc<App>) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/assets/app.js",
            get(|| async {
                (
                    [("content-type", "application/javascript")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route(
            "/assets/console.js",
            get(|| async {
                (
                    [("content-type", "application/javascript")],
                    include_str!("../web/console.js"),
                )
            }),
        )
        .route(
            "/assets/style.css",
            get(|| async {
                (
                    [("content-type", "text/css")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route("/api/session", post(session))
        .route("/api/console/status", get(status))
        .route("/api/console/ws", get(socket))
        .layer(middleware::from_fn_with_state(a.clone(), protect))
        .with_state(a)
}
fn private_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(temp, path)
}
struct Launch {
    path: PathBuf,
    html: PathBuf,
    pid: u32,
    _lock: File,
}
impl Drop for Launch {
    fn drop(&mut self) {
        let own = fs::read(&self.path)
            .ok()
            .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
            .is_some_and(|v| v["pid"] == self.pid);
        if own {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(&self.html);
        }
    }
}
fn open_browser(path: &Path) -> Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "Cannot open the browser. Use the private launch.html file.")?;
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let mut port = 8766u16;
    let mut open = false;
    let mut live = true;
    let mut dir = PathBuf::from(std::env::var_os("HOME").ok_or("HOME unavailable.")?)
        .join(".local/state/nanokvm-console");
    let mut profile = profile::default_path()?;
    let mut pairing = false;
    let mut endpoints = Vec::new();
    let mut expected = String::new();
    let mut certificate = PathBuf::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => profile = PathBuf::from(args.next().ok_or("Missing profile path.")?),
            "--pair" => pairing = true,
            "--verify" => {
                if !guard::ready() {
                    return Err("Input guard missing or changed.");
                }
                println!("Matching Lean input guard verified.");
                return Ok(());
            }
            "--endpoint" => endpoints.push(profile::endpoint(
                &args.next().ok_or("Missing HTTPS origin.")?,
            )?),
            "--device" => expected = args.next().ok_or("Missing device identity.")?,
            "--certificate" => certificate = PathBuf::from(args.next().ok_or("Missing PEM path.")?),
            "--open" => open = true,
            "--demo" => live = false,
            "--port" => {
                port = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or("Invalid port.")?
            }
            "--state-dir" => dir = PathBuf::from(args.next().ok_or("Missing state directory.")?),
            "--help" => {
                println!(
                    "NanoKVM console\n  nanokvm-console --open\n  --pair --endpoint HTTPS_ORIGIN --device MDNS_NAME --certificate PEM_FILE\n  Repeat --endpoint for ordered fallback routes to the same device.\n  --profile PATH: private connection profile\n  --demo: synthetic UI only\n  --port NUMBER --state-dir PATH: isolated tests\nClose with Ctrl+C. Input starts disabled. No device request occurs until Connect or Check connection."
                );
                return Ok(());
            }
            _ => return Err("Unknown option. Use --help."),
        }
    }
    profile = std::path::absolute(&profile).map_err(|_| "Cannot resolve profile path.")?;
    if pairing {
        return profile::pair(&profile, endpoints, expected, &certificate);
    }
    if !endpoints.is_empty() || !expected.is_empty() || !certificate.as_os_str().is_empty() {
        return Err("Pairing options require --pair.");
    }
    fs::create_dir_all(&dir).map_err(|_| "Cannot create private state directory.")?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .map_err(|_| "Cannot protect state directory.")?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join(".server.lock"))
        .map_err(|_| "Cannot open server lock.")?;
    if lock.try_lock_exclusive().is_err() {
        let record: Value = serde_json::from_slice(
            &fs::read(dir.join("launch.json"))
                .map_err(|_| "Existing server is starting. Try again.")?,
        )
        .map_err(|_| "Existing launch record needs inspection.")?;
        if open
            && record["implementation"] == "rust"
            && record["mode"] == if live { "live" } else { "demo" }
            && record["profile"] == profile.to_string_lossy().as_ref()
            && dir.join("launch.html").is_file()
        {
            open_browser(&dir.join("launch.html"))?;
            println!("Opened the existing console.");
            return Ok(());
        }
        return Err(
            "A workbench already owns this state directory. Use its tab or close it first.",
        );
    }
    if let Ok(raw) = fs::read(dir.join("launch.json")) {
        let old: Value =
            serde_json::from_slice(&raw).map_err(|_| "Existing launch record needs inspection.")?;
        let pid = old["pid"]
            .as_u64()
            .filter(|p| *p > 0)
            .ok_or("Existing launch record needs inspection.")?;
        if PathBuf::from(format!("/proc/{pid}")).exists() {
            return Err("Existing recorded process remains present. Its state was preserved.");
        }
        let url = url::Url::parse(old["url"].as_str().ok_or("Invalid old launch URL.")?)
            .map_err(|_| "Invalid old launch URL.")?;
        if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
            return Err("Invalid old launch host.");
        }
        let address = format!(
            "127.0.0.1:{}",
            url.port().ok_or("Missing old launch port.")?
        );
        match std::net::TcpStream::connect_timeout(
            &address.parse().unwrap(),
            std::time::Duration::from_millis(500),
        ) {
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
            _ => return Err("Old listener is active or unresolved. State preserved."),
        }
        fs::rename(
            dir.join("launch.json"),
            dir.join(format!("stale-launch-{}.json", uuid::Uuid::new_v4())),
        )
        .map_err(|_| "Cannot preserve stale launch record.")?;
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|_| "Local port is occupied. Existing listener preserved.")?;
    let host = listener
        .local_addr()
        .map_err(|_| "Cannot inspect listener.")?
        .to_string();
    let a = Arc::new(App {
        token: random(),
        launch: random(),
        session: random(),
        cookie: format!("kvm_rust_{}", uuid::Uuid::new_v4().simple()),
        host,
        live,
        profile,
        owner: Mutex::new(()),
        diagnostic_lock: Mutex::new(()),
        health: Mutex::new(json!({})),
        status: Mutex::new(json!({"state":"closed","input_armed":false,"error":null})),
        stop: CancellationToken::new(),
    });
    let url = format!("http://{}/#{}", a.host, a.launch);
    let executable = fs::read(std::env::current_exe().map_err(|_| "Cannot locate executable.")?)
        .map_err(|_| "Cannot identify executable.")?;
    let record = json!({"url":url,"pid":std::process::id(),"mode":if live{"live"}else{"demo"},"implementation":"rust","profile":a.profile,"executable_sha256":format!("{:x}",Sha256::digest(executable))});
    let html = dir.join("launch.html");
    let path = dir.join("launch.json");
    private_write(
        &html,
        format!(
            "<meta name=\"referrer\" content=\"no-referrer\"><script>location.replace({})</script>",
            serde_json::to_string(&url).unwrap()
        )
        .as_bytes(),
    )
    .map_err(|_| "Cannot write private browser handoff.")?;
    private_write(&path, &serde_json::to_vec(&record).unwrap())
        .map_err(|_| "Cannot write launch record.")?;
    let _launch = Launch {
        path,
        html: html.clone(),
        pid: std::process::id(),
        _lock: lock,
    };
    if open {
        open_browser(&html)?;
    }
    println!(
        "NanoKVM console on http://{} (private launch link required). Ctrl+C closes it.",
        a.host
    );
    let stop = a.stop.clone();
    axum::serve(listener, router(a.clone()))
        .with_graceful_shutdown(async move {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
            stop.cancel();
        })
        .await
        .map_err(|_| "Local server failed.")?;
    // Axum does not own upgraded WebSocket lifetimes. Wait for our sole console
    // owner to finish neutral reports before the Tokio runtime exits.
    let _closed = a.owner.lock().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;
    fn app() -> Arc<App> {
        Arc::new(App {
            token: "t".into(),
            launch: "l".into(),
            session: "s".into(),
            cookie: "test".into(),
            host: "127.0.0.1:8766".into(),
            live: false,
            profile: PathBuf::new(),
            owner: Mutex::new(()),
            diagnostic_lock: Mutex::new(()),
            health: Mutex::new(json!({})),
            status: Mutex::new(json!({"state":"closed"})),
            stop: CancellationToken::new(),
        })
    }
    #[tokio::test]
    async fn rejects_foreign_host_origin_and_missing_cookie() {
        for (host, origin, cookie) in [
            ("evil.test", "", "test=s"),
            ("127.0.0.1:8766", "https://evil.test", "test=s"),
            ("127.0.0.1:8766", "", ""),
        ] {
            let req = Request::builder()
                .uri("/api/console/status")
                .header("Host", host)
                .header("Origin", origin)
                .header("Cookie", cookie)
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                router(app()).oneshot(req).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
    }
    #[tokio::test]
    async fn private_bootstrap_sets_strict_cookie_and_demo_has_no_hardware() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/session")
            .header("Host", "127.0.0.1:8766")
            .header("X-Launch-Token", "l")
            .body(Body::empty())
            .unwrap();
        let response = router(app()).oneshot(req).await.unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("HttpOnly; SameSite=Strict")
        );
        let req = Request::builder()
            .uri("/api/console/status")
            .header("Host", "127.0.0.1:8766")
            .header("Cookie", "test=s")
            .body(Body::empty())
            .unwrap();
        let r = router(app()).oneshot(req).await.unwrap();
        let body = to_bytes(r.into_body(), 20000).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["source"], "synthetic demo");
    }
}
