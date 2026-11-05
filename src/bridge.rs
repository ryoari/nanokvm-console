use crate::{
    App, Result,
    device::{Device, Socket},
    guard::{self, Guard},
};
use axum::extract::ws::{Message as LocalMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::time::{Instant, timeout};
use tokio_tungstenite::tungstenite::Message;

const RELEASE_UNCONFIRMED: &str = "Input release is unconfirmed. Check the target before reconnecting; no automatic retry was started.";

async fn local(ws: &mut WebSocket, value: Value) -> Result<()> {
    timeout(
        Duration::from_secs(3),
        ws.send(LocalMessage::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| "Browser send timed out.")?
    .map_err(|_| "Browser connection closed.")
}
async fn send(ws: &mut Socket, message: Message) -> Result<()> {
    timeout(Duration::from_secs(3), ws.send(message))
        .await
        .map_err(|_| "Device send timed out.")?
        .map_err(|_| "Device socket closed.")
}
async fn input(guard: &mut Guard, hid: &mut Socket, event: Value, app: &App) -> Result<Value> {
    let reply = guard.event(event).await?;
    for frame in guard::frames(&reply)? {
        send(hid, Message::Binary(frame.into())).await?;
    }
    app.status.lock().await["input_armed"] = json!(reply["armed"] == true);
    Ok(reply)
}
fn signal(v: &Value) -> Result<&Value> {
    let s = &v["value"];
    if !matches!(s["event"].as_str(), Some("video-offer" | "video-candidate"))
        || !s["data"].is_string()
    {
        return Err("Unsupported video signal.");
    }
    Ok(s)
}
fn retryable(e: &str) -> bool {
    crate::device::transport_retryable(e)
        || matches!(
            e,
            crate::device::ROUTES_UNREACHABLE
                | "Device socket timed out."
                | "Device send timed out."
                | "Device socket closed."
                | "Video signaling closed. Reconnect the console."
                | "Input connection closed. Reconnect the console."
                | "Browser heartbeat stopped. Reconnect the console."
        )
}
pub async fn serve(mut ws: WebSocket, app: Arc<App>) {
    let first = timeout(Duration::from_secs(5), ws.recv()).await;
    let valid = if let Ok(Some(Ok(LocalMessage::Text(raw)))) = first {
        serde_json::from_str::<Value>(&raw).is_ok_and(|v| {
            crate::equal(v["token"].as_str().unwrap_or(""), &app.token) && v["mode"] == "live"
        })
    } else {
        false
    };
    if !valid || !app.live {
        let _ = local(
            &mut ws,
            json!({"type":"error","detail":"Open a live local session first."}),
        )
        .await;
        return;
    }
    let Ok(_owner) = app.owner.try_lock() else {
        let _ = local(
            &mut ws,
            json!({"type":"error","detail":"Close the other console session first."}),
        )
        .await;
        return;
    };
    if app.stop.is_cancelled() {
        return;
    }
    *app.status.lock().await = json!({"state":"connecting","input_armed":false,"error":null});
    let result = session(&mut ws, &app).await;
    if let Err(e) = result {
        app.status.lock().await["error"] = json!(e);
        let _ = local(
            &mut ws,
            json!({"type":"error","detail":e,"retryable":retryable(e),"release_unconfirmed":e==RELEASE_UNCONFIRMED}),
        )
        .await;
    }
    app.status.lock().await["state"] = json!("closed");
    app.status.lock().await["input_armed"] = json!(false);
    let _ = timeout(Duration::from_secs(2), ws.close()).await;
}
async fn session(ws: &mut WebSocket, app: &Arc<App>) -> Result<()> {
    let (guard, hid, video) = tokio::select! {
        prepared=prepare(app)=>prepared?,
        _=app.stop.cancelled()=>return Ok(()),
        _=ws.recv()=>return Ok(()),
    };
    connected(ws, app, guard, hid, video).await
}
async fn prepare(app: &Arc<App>) -> Result<(Guard, Socket, Socket)> {
    let guard = Guard::start()?;
    let device = Device::login(&app.profile).await?;
    device.memory_off().await?;
    device
        .api("/api/stream/mode", Some(json!({"mode":"h264-webrtc"})))
        .await?;
    let mut hid = device.socket("/api/ws").await?;
    send(&mut hid, Message::Binary(vec![0].into())).await?;
    let video = device.socket("/api/stream/h264/webrtc").await?;
    Ok((guard, hid, video))
}
async fn connected(
    ws: &mut WebSocket,
    app: &Arc<App>,
    mut guard: Guard,
    mut hid: Socket,
    mut video: Socket,
) -> Result<()> {
    guard.event(json!({"op":"connected"})).await?;
    app.status.lock().await["state"] = json!("connected");
    let result=async{
        local(ws,json!({"type":"ready","input_armed":false})).await?;
        let mut activity=Instant::now();
        loop{
            tokio::select!{
                _=app.stop.cancelled()=>return Ok(()),
                _=tokio::time::sleep_until(activity+Duration::from_secs(45))=>return Err("Browser heartbeat stopped. Reconnect the console."),
                message=ws.recv()=>{
                    let raw=match message{Some(Ok(LocalMessage::Text(raw)))=>raw,Some(Ok(LocalMessage::Ping(_)|LocalMessage::Pong(_)))=>continue,_=>return Ok(())};
                    activity=Instant::now();
                    let v:Value=serde_json::from_str(&raw).map_err(|_|"Invalid browser message.")?;
                    match v["type"].as_str(){
                        Some("signal")=>send(&mut video,Message::Text(signal(&v)?.to_string().into())).await?,
                        Some("input")=>{
                            let reply=input(&mut guard,&mut hid,guard::browser_event(&v)?,app).await?;
                            local(ws,json!({"type":"input-status","accepted":reply["accepted"],"armed":reply["armed"]==true})).await?;
                        },
                        Some("ping")=>{
                            send(&mut hid,Message::Binary(vec![0].into())).await?;
                            send(&mut video,Message::Text(json!({"event":"heartbeat","data":""}).to_string().into())).await?;
                            local(ws,json!({"type":"pong"})).await?;
                        },
                        Some("close")=>return Ok(()),
                        _=>return Err("Unsupported browser message.")
                    }
                },
                message=video.next()=>match message{
                    Some(Ok(Message::Text(raw)))=>{
                        let v:Value=serde_json::from_str(&raw).map_err(|_|"Invalid video signal.")?;
                        if v["event"]=="video-status"{app.status.lock().await["video_status"]=v["data"].clone();}
                        local(ws,json!({"type":"signal","value":v})).await?;
                    },
                    Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{},
                    _=>return Err("Video signaling closed. Reconnect the console.")
                },
                message=hid.next()=>match message{
                    Some(Ok(Message::Close(_)))|Some(Err(_))|None=>return Err("Input connection closed. Reconnect the console."),
                    _=>{}
                }
            }
        }
    }.await;
    // This single task owns all input sends. Every exit attempts neutral reports
    // before socket closure; no global device logout or hardware reset is sent.
    let release = input(&mut guard, &mut hid, json!({"op":"close"}), app).await;
    let _ = timeout(Duration::from_secs(2), hid.close(None)).await;
    let _ = timeout(Duration::from_secs(2), video.close(None)).await;
    guard.stop().await;
    if release.is_err() {
        app.status.lock().await["release_unconfirmed"] = json!(true);
        return Err(RELEASE_UNCONFIRMED);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::WebSocketUpgrade, routing::get};
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::Mutex,
    };
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::protocol::Role};

    async fn pair() -> (Socket, Socket) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, server) = tokio::join!(
            TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept()
        );
        (
            WebSocketStream::from_raw_socket(
                MaybeTlsStream::Plain(client.unwrap()),
                Role::Client,
                None,
            )
            .await,
            WebSocketStream::from_raw_socket(
                MaybeTlsStream::Plain(server.unwrap().0),
                Role::Server,
                None,
            )
            .await,
        )
    }

    #[tokio::test]
    async fn browser_loss_disarm_and_shutdown_release_keys_and_buttons() {
        for release in ["browser_loss", "disarm", "server_shutdown"] {
            let app = Arc::new(App {
                token: String::new(),
                launch: String::new(),
                session: String::new(),
                cookie: String::new(),
                host: String::new(),
                live: false,
                profile: std::path::PathBuf::new(),
                owner: Mutex::new(()),
                diagnostic_lock: Mutex::new(()),
                health: Mutex::new(json!({})),
                status: Mutex::new(json!({})),
                stop: tokio_util::sync::CancellationToken::new(),
            });
            let (hid, mut receiver) = pair().await;
            let (video, _video_peer) = pair().await;
            let sockets = Arc::new(Mutex::new(Some((hid, video))));
            let capture = tokio::spawn(async move {
                let mut frames = Vec::new();
                while let Some(message) = receiver.next().await {
                    match message.unwrap() {
                        Message::Binary(bytes) => frames.push(bytes.to_vec()),
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
                frames
            });
            let (done, mut complete) = tokio::sync::mpsc::channel(1);
            let app2 = app.clone();
            let route = Router::new().route(
                "/",
                get(move |ws: WebSocketUpgrade| {
                    let sockets = sockets.clone();
                    let app = app2.clone();
                    let done = done.clone();
                    async move {
                        ws.on_upgrade(move |mut ws| async move {
                            let (hid, video) = sockets.lock().await.take().unwrap();
                            let result =
                                connected(&mut ws, &app, Guard::start().unwrap(), hid, video).await;
                            done.send(result).await.unwrap();
                        })
                    }
                }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
            let (mut browser, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
                .await
                .unwrap();
            let ready = browser.next().await.unwrap().unwrap();
            assert!(ready.to_text().unwrap().contains("ready"));
            for event in [
                json!({"op":"keyboard","modifier":0,"keys":[4]}),
                json!({"op":"arm"}),
                json!({"op":"keyboard","modifier":2,"keys":[4]}),
                json!({"op":"pointer","buttons":1,"x":100,"y":200,"wheel":0}),
            ] {
                browser
                    .send(Message::Text(
                        json!({"type":"input","value":event}).to_string().into(),
                    ))
                    .await
                    .unwrap();
                let ack = browser.next().await.unwrap().unwrap();
                assert!(ack.to_text().unwrap().contains("input-status"));
            }
            if release == "server_shutdown" {
                app.stop.cancel();
            } else if release == "disarm" {
                browser
                    .send(Message::Text(
                        json!({"type":"input","value":{"op":"disarm"}})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
                let ack = browser.next().await.unwrap().unwrap();
                let ack: Value = serde_json::from_str(ack.to_text().unwrap()).unwrap();
                assert_eq!(ack["armed"], false);
                assert_eq!(ack["accepted"], true);
                browser
                    .send(Message::Text(json!({"type":"close"}).to_string().into()))
                    .await
                    .unwrap();
            } else {
                drop(browser);
            }
            timeout(Duration::from_secs(6), complete.recv())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let frames = timeout(Duration::from_secs(2), capture)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                frames.len(),
                4,
                "Unarmed input must not send a report; closure must send two neutral reports"
            );
            assert_eq!(&frames[2][13..], &[0; 8]);
            assert_eq!(&frames[3][13..], &[0, 100, 0, 200, 0, 0, 0]);
            assert_eq!(app.status.lock().await["input_armed"], false);
            server.abort();
            let _ = server.await;
        }
    }
    #[test]
    fn signaling_allowlist() {
        assert!(signal(&json!({"value":{"event":"video-offer","data":"{}"}})).is_ok());
        for event in ["heartbeat", "reset", "video-answer"] {
            assert!(signal(&json!({"value":{"event":event,"data":"{}"}})).is_err());
        }
    }
}
