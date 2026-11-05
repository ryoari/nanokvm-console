use crate::Result;
use crate::profile::Profile;
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use tokio::{net::TcpStream, time::timeout};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Message, WebSocketConfig},
    },
};

pub const ROUTES_UNREACHABLE: &str = "All configured routes are unreachable. Check USB networking or the device LAN route. No device restart was requested.";
pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub struct Device {
    tls: native_tls::TlsConnector,
    cookie: String,
    password: String,
    pub info: Value,
    url: String,
    fingerprint: String,
    pub routes: Vec<Value>,
}
impl Device {
    pub async fn login(path: &Path) -> Result<Self> {
        let profile = Profile::load(path)?;
        let cert = profile.certificate()?;
        let fingerprint = profile.fingerprint()?;
        let tls = native_tls::TlsConnector::builder()
            .disable_built_in_roots(true)
            .add_root_certificate(cert)
            .danger_accept_invalid_hostnames(true)
            .build()
            .map_err(|_| "Device TLS unavailable.")?;
        let mut routes = Vec::new();
        for url in &profile.endpoints {
            let mut d = Self {
                tls: tls.clone(),
                cookie: String::new(),
                password: profile.password.clone(),
                info: Value::Null,
                url: url.clone(),
                fingerprint: fingerprint.clone(),
                routes: routes.clone(),
            };
            // A reachable peer with an authentication/identity/protocol fault is
            // a stop condition. Repeating login elsewhere could hide that fault.
            let login = match d
                .api(
                    "/api/auth/login",
                    Some(json!({"username":profile.username,"password":d.password})),
                )
                .await
            {
                Ok(value) => value,
                Err(e) if transport_retryable(e) => {
                    routes.push(json!({"endpoint":url,"state":"unreachable"}));
                    continue;
                }
                Err(e) => return Err(e),
            };
            let token = login["token"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("Device login failed.")?;
            d.cookie = format!("nano-kvm-token={token}");
            d.info = match d.api("/api/vm/info", None).await {
                Ok(value) => value,
                Err(e) if transport_retryable(e) => {
                    routes.push(json!({"endpoint":url,"state":"unreachable"}));
                    continue;
                }
                Err(e) => return Err(e),
            };
            if d.info["mdns"] != profile.device {
                return Err("Unexpected device identity. Recovery stopped.");
            }
            d.routes.push(json!({"endpoint":url,"state":"connected"}));
            return Ok(d);
        }
        Err(ROUTES_UNREACHABLE)
    }
    async fn transport(&self) -> Result<tokio_native_tls::TlsStream<TcpStream>> {
        let u = url::Url::parse(&self.url).map_err(|_| "Invalid device origin.")?;
        let host = u
            .host_str()
            .ok_or("Missing device host.")?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let port = u.port_or_known_default().ok_or("Missing device port.")?;
        let tcp = timeout(Duration::from_secs(3), TcpStream::connect((host, port)))
            .await
            .map_err(|_| "Device route unavailable.")?
            .map_err(|_| "Device route unavailable.")?;
        let connector = tokio_native_tls::TlsConnector::from(self.tls.clone());
        let stream = timeout(Duration::from_secs(4), connector.connect(host, tcp))
            .await
            .map_err(|_| "Device TLS timed out. Recovery stopped.")?
            .map_err(|_| "Device TLS trust failed. Recovery stopped.")?;
        let cert = stream
            .get_ref()
            .peer_certificate()
            .map_err(|_| "Cannot inspect peer certificate.")?
            .ok_or("Peer certificate missing.")?;
        let der = cert.to_der().map_err(|_| "Invalid peer certificate.")?;
        if format!("{:x}", Sha256::digest(der)) != self.fingerprint {
            return Err("Device certificate changed. Recovery stopped.");
        }
        Ok(stream)
    }
    pub async fn api(&self, path: &str, body: Option<Value>) -> Result<Value> {
        use http_body_util::{BodyExt, Full};
        // Pin this exact transport before sending any HTTP bytes or credentials.
        let stream = self.transport().await?;
        let (mut sender, connection) = timeout(
            Duration::from_secs(3),
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream)),
        )
        .await
        .map_err(|_| "Device request timed out.")?
        .map_err(|_| "Device request transport failed.")?;
        let _connection = ConnectionTask(tokio::spawn(async move {
            let _ = connection.await;
        }));
        timeout(Duration::from_secs(6), async {
            let u = url::Url::parse(&self.url).map_err(|_| "Invalid device origin.")?;
            let request = hyper::Request::builder()
                .method(if body.is_some() { "POST" } else { "GET" })
                .uri(path)
                .header(
                    "Host",
                    &u[url::Position::BeforeHost..url::Position::AfterPort],
                )
                .header("Origin", &self.url)
                .header("Referer", format!("{}/", self.url))
                .header("Cookie", &self.cookie)
                .header("Content-Type", "application/json")
                .body(Full::new(bytes::Bytes::from(
                    body.map(|b| b.to_string()).unwrap_or_default(),
                )))
                .map_err(|_| "Invalid device request.")?;
            let mut response = sender
                .send_request(request)
                .await
                .map_err(|_| "Device request transport failed.")?;
            if !response.status().is_success() {
                return Err("Device HTTP request declined.");
            }
            let mut bytes = Vec::new();
            while let Some(frame) = response.body_mut().frame().await {
                let frame = frame.map_err(|_| "Device response transport failed.")?;
                if let Ok(data) = frame.into_data() {
                    if bytes.len() + data.len() > 1_048_576 {
                        return Err("Device response too large.");
                    }
                    bytes.extend_from_slice(&data);
                }
            }
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| "Invalid device response.")?;
            if value["code"] != 0 {
                return Err("Device operation declined.");
            }
            Ok(value.get("data").cloned().unwrap_or(Value::Null))
        })
        .await
        .map_err(|_| "Device request timed out.")?
    }
    pub async fn socket(&self, path: &str) -> Result<Socket> {
        let mut request = format!("{}{path}", self.url.replacen("https:", "wss:", 1))
            .into_client_request()
            .map_err(|_| "Invalid socket path.")?;
        request.headers_mut().insert(
            "Origin",
            self.url.parse().map_err(|_| "Invalid device origin.")?,
        );
        request.headers_mut().insert(
            "Cookie",
            self.cookie.parse().map_err(|_| "Invalid device cookie.")?,
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(131072))
            .max_frame_size(Some(131072));
        let stream = self.transport().await?;
        timeout(
            Duration::from_secs(6),
            client_async_with_config(request, MaybeTlsStream::NativeTls(stream), Some(config)),
        )
        .await
        .map_err(|_| "Device socket timed out.")?
        .map(|r| r.0)
        .map_err(|e| match e {
            tokio_tungstenite::tungstenite::Error::Io(_) => "Device socket transport failed.",
            _ => "Device socket handshake declined.",
        })
    }
    pub async fn memory_off(&self) -> Result<Value> {
        let m = self.api("/api/ai/memory-fabric", None).await?;
        if m["enabled"] != false || m["running"] != false {
            return Err("Memory Fabric must be disabled and stopped.");
        }
        Ok(json!({"enabled":false,"running":false}))
    }
    pub async fn capture(&self) -> Result<Value> {
        let c = self
            .api(
                "/api/auth/2fa/verify-sensitive-action",
                Some(json!({"action":"open_web_terminal","password":self.password})),
            )
            .await?;
        self.api(
            "/api/vm/terminal/auth",
            Some(json!({"challenge":c["challenge"]})),
        )
        .await?;
        let mut socket = self.socket("/api/vm/terminal").await?;
        let result=timeout(Duration::from_secs(20),async{
            socket.send(Message::Binary(b"{\"rows\":40,\"cols\":180}".to_vec().into())).await.map_err(|_|"Diagnostic terminal failed.")?;
            let payload=STANDARD.encode(include_str!("capture_probe.py"));
            let command=format!("stty -echo\n{{ printf '\\nAPP_META_BEGIN\\n'; python3 -I -c \"import base64;exec(base64.b64decode('{payload}'))\"; printf '\\nAPP_META_END\\n'; }}\nexit\n");
            socket.send(Message::Text(command.into())).await.map_err(|_|"Diagnostic request failed.")?;
            let mut bytes=Vec::new();
            while let Some(message)=socket.next().await{
                let data=message.map_err(|_|"Diagnostic stream failed.")?.into_data();
                if bytes.len()+data.len()>65536{return Err("Diagnostic output too large.")}bytes.extend_from_slice(&data);
                if let Some(raw)=framed(&bytes){return serde_json::from_slice(raw).map_err(|_|"Invalid diagnostic output.")}
            }
            Err("Diagnostic stream closed early.")
        }).await.map_err(|_|"Capture diagnostics timed out.")?;
        let _ = timeout(Duration::from_secs(2), socket.close(None)).await;
        result
    }
    pub fn identity(&self) -> Value {
        let mut v = identity(&self.info);
        v["certificate_sha256"] = json!(self.fingerprint);
        v["endpoint"] = json!(self.url);
        v
    }
}
struct ConnectionTask(tokio::task::JoinHandle<()>);
impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub fn transport_retryable(e: &str) -> bool {
    matches!(
        e,
        "Device route unavailable."
            | "Device request timed out."
            | "Device request transport failed."
            | "Device response transport failed."
            | "Device socket transport failed."
    )
}
fn identity(info: &Value) -> Value {
    let version = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| {
                let v = &info[*name];
                if let Some(s) = v.as_str() {
                    Some(s.chars().take(128).collect::<String>())
                } else if v.is_i64() || v.is_u64() {
                    Some(v.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".into())
    };
    json!({"mdns":info["mdns"],"firmware":version(&["imageVersion","image","firmwareVersion"]),
        "application":version(&["appVersion","applicationVersion","application","version"])})
}
fn framed(bytes: &[u8]) -> Option<&[u8]> {
    for (begin, end) in [
        (
            b"\r\nAPP_META_BEGIN\r\n".as_slice(),
            b"\r\nAPP_META_END\r\n".as_slice(),
        ),
        (
            b"\nAPP_META_BEGIN\n".as_slice(),
            b"\nAPP_META_END\n".as_slice(),
        ),
    ] {
        if let Some(start) = bytes.windows(begin.len()).position(|w| w == begin) {
            let start = start + begin.len();
            if let Some(finish) = bytes[start..].windows(end.len()).position(|w| w == end) {
                return Some(&bytes[start..start + finish]);
            }
        }
    }
    None
}
pub async fn diagnostics(path: &Path) -> Value {
    let mut checks = json!({});
    let mut identity = Value::Null;
    match Device::login(path).await {
        Ok(d) => {
            identity = d.identity();
            checks["connection"] = json!({"ok":true,"routes":d.routes});
            checks["memory"] = match d.memory_off().await {
                Ok(v) => json!({"ok":true,"data":v}),
                Err(e) => json!({"ok":false,"detail":e}),
            };
            for (name, path, fields) in [
                ("capture", "/api/vm/hdmi/capture", vec!["enabled"]),
                ("low_power", "/api/vm/low-power", vec!["enabled"]),
                (
                    "encoder",
                    "/api/stream/encoder",
                    vec!["rate_control", "bitrate", "gop", "fps"],
                ),
            ] {
                checks[name] = match d.api(path, None).await {
                    Ok(v) => {
                        let data: serde_json::Map<String, Value> = fields
                            .into_iter()
                            .map(|k| (k.into(), v[k].clone()))
                            .collect();
                        json!({"ok":if name=="capture"{v["enabled"]==true}else{true},"data":data})
                    }
                    Err(e) => json!({"ok":false,"detail":e}),
                };
            }
            checks["capture_service"] = match d.capture().await {
                Ok(v) => json!({"ok":capture_ok(&v),"data":v}),
                Err(e) => json!({"ok":false,"detail":e}),
            };
        }
        Err(e) => {
            checks["connection"] = json!({"ok":false,"detail":e});
        }
    }
    json!({"source":"live device","identity":identity,"checks":checks,"lean_guard_ready":crate::guard::ready(),"implementation":"rust"})
}
pub fn capture_ok(v: &Value) -> bool {
    v["processes"].as_u64().is_some_and(|n| n > 0)
        && v["sockets"]["vin_ctrl.sock"] == true
        && v["libraries"].as_array().is_some_and(|rows| {
            rows.len() == 3 && rows.iter().all(|r| r["matches_package"] == true)
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_aliases_and_missing_values() {
        let v = identity(&json!({"mdns":"synthetic.local","image":"v1.0.9","application":"0.2.2"}));
        assert_eq!(v["firmware"], "v1.0.9");
        assert_eq!(v["application"], "0.2.2");
        assert_eq!(identity(&json!({}))["firmware"], "unknown");
        assert_eq!(
            identity(&json!({"imageVersion":"v2","image":"old"}))["firmware"],
            "v2"
        );
    }
    #[test]
    fn metadata_frame_ignores_terminal_echo() {
        let data =
            b"echo APP_META_BEGIN blah\r\nAPP_META_BEGIN\r\n{\"ok\":true}\r\nAPP_META_END\r\n";
        assert_eq!(
            serde_json::from_slice::<Value>(framed(data).unwrap()).unwrap(),
            json!({"ok":true})
        );
        assert!(framed(b"APP_META_BEGIN").is_none());
        assert_eq!(
            framed(b"\nAPP_META_BEGIN\n{}\nAPP_META_END\n"),
            Some(b"{}".as_slice())
        );
    }
    #[test]
    fn missing_or_bad_library_never_ready() {
        let mut v = json!({"processes":1,"sockets":{"vin_ctrl.sock":true},"libraries":vec![json!({"matches_package":true});3]});
        assert!(capture_ok(&v));
        v["libraries"][1]["matches_package"] = json!(false);
        assert!(!capture_ok(&v));
        v["libraries"] = json!([]);
        assert!(!capture_ok(&v));
    }
}
