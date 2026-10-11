use crate::{
    auth::{self, Auth},
    config::Config,
    encoder::{self, Stream},
    rfb::Input,
};
use axum::{
    Router,
    extract::{
        DefaultBodyLimit, Form, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use futures_util::SinkExt;
use serde::Deserialize;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
const COOKIE: &str = "screenbridge_session";
struct Active {
    stream: Stream,
    viewers: AtomicUsize,
    controller: AtomicBool,
}
struct Lease(Arc<Active>);
impl Drop for Lease {
    fn drop(&mut self) {
        if self.0.viewers.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.stream.cancel.cancel();
        }
    }
}
pub struct App {
    config: Arc<Config>,
    auth: Auth,
    active: Mutex<Option<Arc<Active>>>,
    capacity: Arc<Semaphore>,
    shutdown: CancellationToken,
}
impl App {
    pub fn new(config: Config, shutdown: CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            auth: Auth::new(&config.access_token),
            config: Arc::new(config),
            active: Mutex::new(None),
            capacity: Arc::new(Semaphore::new(8)),
            shutdown,
        })
    }
    async fn attach(&self) -> crate::error::Result<Lease> {
        let mut slot = self.active.lock().await;
        if slot
            .as_ref()
            .is_some_and(|s| s.stream.cancel.is_cancelled())
        {
            *slot = None;
        }
        if slot.is_none() {
            let cancel = self.shutdown.child_token();
            let stream = encoder::start(self.config.clone(), cancel, false).await?;
            *slot = Some(Arc::new(Active {
                stream,
                viewers: AtomicUsize::new(0),
                controller: AtomicBool::new(false),
            }));
        }
        let active = slot.as_ref().ok_or(crate::error::Error::Closed)?.clone();
        active.viewers.fetch_add(1, Ordering::AcqRel);
        Ok(Lease(active))
    }
    fn authorized(&self, h: &HeaderMap) -> bool {
        h.get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.split(';')
                    .find_map(|p| p.trim().strip_prefix(&format!("{COOKIE}=")))
            })
            .is_some_and(|c| self.auth.verify(c, auth::now()))
    }
    fn origin(&self, h: &HeaderMap) -> bool {
        h.get("origin").and_then(|v| v.to_str().ok()) == Some(self.config.public_origin.as_str())
    }
}
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(|| async { "ok" }))
        .route("/login", post(login))
        .route("/stream", get(upgrade))
        .route("/stats", get(stats))
        .route(
            "/player.js",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/player.js"),
                )
            }),
        )
        .route(
            "/player.css",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/css")],
                    include_str!("../web/player.css"),
                )
            }),
        )
        .layer(DefaultBodyLimit::max(1024))
        .layer(middleware::from_fn(headers))
        .with_state(app)
}
async fn headers(req: axum::extract::Request, next: Next) -> Response {
    let mut response = match tokio::time::timeout(Duration::from_secs(10), next.run(req)).await {
        Ok(r) => r,
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    let h = response.headers_mut();
    h.insert("cache-control", HeaderValue::from_static("no-store"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("content-security-policy",HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; form-action 'self'; base-uri 'none'"));
    response
}
const LOGIN: &str = r#"<!doctype html><html lang="en"><meta charset="utf-8"><title>Screenbridge sign in</title><h1>Screenbridge</h1><form action="/login" method="post"><label>Access token <input name="token" type="password" autocomplete="off" required></label><button>Connect</button></form></html>"#;
async fn index(State(app): State<Arc<App>>, h: HeaderMap) -> Response {
    if app.authorized(&h) {
        Html(include_str!("../web/index.html")).into_response()
    } else {
        Html(LOGIN).into_response()
    }
}
#[derive(Deserialize)]
struct Login {
    token: String,
}
async fn login(State(app): State<Arc<App>>, h: HeaderMap, Form(form): Form<Login>) -> Response {
    if !app.origin(&h) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.auth.accepts(&form.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(cookie) = app.auth.issue(auth::now()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let value = format!(
        "{COOKIE}={cookie}; HttpOnly; SameSite=Strict; Path=/; Max-Age=3600{}",
        if app.config.public_origin.starts_with("https:") {
            "; Secure"
        } else {
            ""
        }
    );
    let Ok(value) = HeaderValue::from_str(&value) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert("location", HeaderValue::from_static("/"));
    response.headers_mut().insert("set-cookie", value);
    response
}
async fn stats(State(app): State<Arc<App>>, h: HeaderMap) -> Response {
    if !app.authorized(&h) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let slot = app.active.lock().await;
    axum::Json(slot.as_ref().map(|s| s.stream.counters.snapshot())).into_response()
}
async fn upgrade(State(app): State<Arc<App>>, h: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !app.authorized(&h) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !app.origin(&h) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = app.capacity.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(1024)
        .max_frame_size(1024)
        .write_buffer_size(0)
        .max_write_buffer_size(2 * 1024 * 1024 + 1024)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            viewer(app, socket).await
        })
}
#[derive(Deserialize)]
#[serde(tag = "t")]
enum Control {
    #[serde(rename = "ptr")]
    Pointer { x: f64, y: f64, m: u8 },
    #[serde(rename = "key")]
    Key { k: u32, d: bool },
    #[serde(rename = "release")]
    Release,
    #[serde(rename = "ping")]
    Ping { at: f64 },
}
async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(1), socket.send(message)).await,
        Ok(Ok(()))
    )
}
async fn release(active: &Active, keys: &mut HashSet<u32>, pointer: &mut (u16, u16, u8)) {
    let (done, ack) = tokio::sync::oneshot::channel();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        active
            .stream
            .input
            .send(Input::Release { done })
            .await
            .map_err(|_| ())?;
        ack.await.map_err(|_| ())
    })
    .await;
    if !matches!(result, Ok(Ok(()))) {
        active.stream.cancel.cancel();
    }
    keys.clear();
    pointer.2 = 0;
}
async fn viewer(app: Arc<App>, mut socket: WebSocket) {
    let attaching = app.attach();
    tokio::pin!(attaching);
    let lease = loop {
        tokio::select! {
            _=app.shutdown.cancelled()=>return,
            r=&mut attaching=>match r{Ok(l)=>break l,Err(_)=>{let _=tokio::time::timeout(Duration::from_secs(1),socket.close()).await;return}},
            m=socket.recv()=>{
                match m {
                    Some(Ok(Message::Text(text)))=>{
                        if let Ok(Control::Ping{at})=serde_json::from_str(&text){if at.is_finite()&&!send(&mut socket,Message::Text(serde_json::json!({"t":"pong","at":at}).to_string().into())).await{return}}
                    },Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{},_=>return
                }
            }
        }
    };
    let active = &lease.0;
    let control = active
        .controller
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    let mut video = active.stream.video.subscribe();
    let mut needs_key = true;
    let mut keys = HashSet::new();
    let mut pointer = (0, 0, 0);
    let key_deadline = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(key_deadline);
    let expiry = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(expiry);
    if !send(
        &mut socket,
        Message::Text(
            serde_json::json!({"t":"role","role":if control{"controller"}else{"viewer"}})
                .to_string()
                .into(),
        ),
    )
    .await
    {
        if control {
            active.controller.store(false, Ordering::Release)
        }
        return;
    }
    loop {
        tokio::select! {
            _=active.stream.cancel.cancelled()=>break,
            _=&mut expiry=>break,
        _=&mut key_deadline, if needs_key=>break,
            frame=video.recv()=>{
                let Ok(frame)=frame else{break};if needs_key&&!frame.key{continue}
                if frame.key&&!send(&mut socket,Message::Text(frame.init.to_string().into())).await{break}
                needs_key=false;if !send(&mut socket,Message::Binary(frame.packet)).await{break}
            },
            message=socket.recv()=>{
                let text=match message {Some(Ok(Message::Text(text)))=>text,Some(Ok(Message::Ping(_)|Message::Pong(_)))=>continue,_=>break};let Ok(message)=serde_json::from_str::<Control>(&text) else{continue};
                let input=match message {
                    Control::Ping{at} if at.is_finite()=>{if !send(&mut socket,Message::Text(serde_json::json!({"t":"pong","at":at}).to_string().into())).await{break}None},
                    Control::Release if control=>{release(active,&mut keys,&mut pointer).await;None},
                    Control::Pointer{x,y,m} if control&&x.is_finite()&&y.is_finite()=>{pointer=(x.clamp(0.0,65535.0) as u16,y.clamp(0.0,65535.0) as u16,m);Some(Input::Pointer{x:pointer.0,y:pointer.1,mask:m})},
                    Control::Key{k,d} if control&&(!d||keys.len()<128||keys.contains(&k))=>{if d{keys.insert(k);}else{keys.remove(&k);}Some(Input::Key{keysym:k,down:d})},_=>None
                };
                if let Some(input)=input {if active.stream.input.try_send(input).is_err(){break}}
            }
        }
    }
    if control {
        release(active, &mut keys, &mut pointer).await;
        active.controller.store(false, Ordering::Release);
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close()).await;
}
