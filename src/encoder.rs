use crate::{
    annex_b::{AccessUnit, Parser},
    config::Config,
    error::{Error, Result},
    rfb::{self, Frame, Input},
};
use bytes::{BufMut, Bytes, BytesMut};
use serde::Serialize;
use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
pub struct Video {
    pub packet: Bytes,
    pub init: Arc<str>,
    pub key: bool,
}
#[derive(Default)]
pub struct Counters {
    pub submitted: AtomicU64,
    pub encoded: AtomicU64,
    pub bytes: AtomicU64,
}
#[derive(Serialize)]
pub struct Stats {
    pub submitted: u64,
    pub encoded: u64,
    pub bytes: u64,
}
impl Counters {
    pub fn snapshot(&self) -> Stats {
        Stats {
            submitted: self.submitted.load(Ordering::Relaxed),
            encoded: self.encoded.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}
pub struct Stream {
    pub video: broadcast::Sender<Video>,
    pub input: mpsc::Sender<Input>,
    pub counters: Arc<Counters>,
    pub cancel: CancellationToken,
    task: JoinHandle<Result<()>>,
}
impl Drop for Stream {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}
pub fn arguments(config: &Config, frame: &Frame, software: bool) -> Vec<String> {
    let mut a = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pixel_format".into(),
        frame.layout.ffmpeg().into(),
        "-video_size".into(),
        format!("{}x{}", frame.width, frame.height),
        "-framerate".into(),
        config.fps.to_string(),
        "-i".into(),
        "pipe:0".into(),
        "-vf".into(),
        "crop=trunc(iw/2)*2:trunc(ih/2)*2".into(),
        "-c:v".into(),
    ];
    if software {
        a.extend(["libx264", "-preset", "ultrafast", "-tune", "zerolatency"].map(str::to_owned));
    } else {
        a.extend(
            [
                "h264_videotoolbox",
                "-realtime",
                "1",
                "-allow_sw",
                "0",
                "-flags",
                "+low_delay",
            ]
            .map(str::to_owned),
        );
    }
    a.extend([
        "-profile:v".into(),
        "baseline".into(),
        "-bf".into(),
        "0".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-b:v".into(),
        format!("{}k", config.bitrate_kbps),
        "-g".into(),
        (config.fps * 2).to_string(),
        "-bsf:v".into(),
        "dump_extra=freq=keyframe,h264_metadata=aud=insert".into(),
        "-f".into(),
        "h264".into(),
        "pipe:1".into(),
    ]);
    a
}
pub async fn start(
    config: Arc<Config>,
    cancel: CancellationToken,
    software: bool,
) -> Result<Stream> {
    let mut capture = rfb::connect(config.vnc_port, &config.vnc_password, cancel.clone()).await?;
    let first = tokio::select! {
        _=cancel.cancelled()=>return Err(Error::Cancelled),
        result=tokio::time::timeout(Duration::from_secs(10),async {
            loop {if let Some(frame)=capture.frames.borrow_and_update().clone(){return Ok::<Frame,Error>(frame)}capture.frames.changed().await.map_err(|_|Error::Closed)?;}
        })=>result.map_err(|_|Error::Timeout)??
    };
    let mut child = Command::new(&config.ffmpeg)
        .args(arguments(&config, &first, software))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or(Error::Closed)?;
    let mut stdout = child.stdout.take().ok_or(Error::Closed)?;
    let (video, _) = broadcast::channel(8);
    let counters = Arc::new(Counters::default());
    let sender = video.clone();
    let input = capture.input.clone();
    let task_cancel = cancel.clone();
    let counts = counters.clone();
    let task = tokio::spawn(async move {
        let run=async {
            let feed=async {
                let mut interval=tokio::time::interval(Duration::from_secs_f64(1.0/config.fps as f64));interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {interval.tick().await;let frame=capture.frames.borrow().clone().ok_or(Error::Closed)?;
                    tokio::time::timeout(Duration::from_secs(2),stdin.write_all(&frame.pixels)).await.map_err(|_|Error::Timeout)??;
                    counts.submitted.fetch_add(1,Ordering::Relaxed);
                }
                #[allow(unreachable_code)] Ok::<(),Error>(())
            };
            let output=async {
                let mut parser=Parser::default();let mut buffer=[0;65536];let started=Instant::now();let mut seq=0_u32;let mut init:Option<Arc<str>>=None;
                loop {
                    let n=stdout.read(&mut buffer).await?;if n==0{return Err(Error::Closed)}
                    for au in parser.push(&buffer[..n])? {
                        if au.key {let codec=au.codec.as_ref().ok_or(Error::Protocol("missing SPS"))?;init=Some(serde_json::json!({"t":"init","version":1,"width":first.width&!1,"height":first.height&!1,"sourceWidth":first.width,"sourceHeight":first.height,"codec":codec}).to_string().into());}
                        if let Some(init)=&init {
                            let packet=packet(&au,seq,started.elapsed().as_secs_f64()*1000.0);seq=seq.wrapping_add(1);
                            counts.encoded.fetch_add(1,Ordering::Relaxed);counts.bytes.fetch_add(packet.len() as u64,Ordering::Relaxed);
                            let _=sender.send(Video{packet,init:init.clone(),key:au.key});
                        }
                    }
                }
            };
            tokio::select! {_=task_cancel.cancelled()=>Ok(()),r=feed=>r,r=output=>r,r=&mut capture.task=>r.map_err(|_|Error::Closed)?,r=child.wait()=>{r?;Err(Error::Closed)}}
        }.await;
        task_cancel.cancel();
        capture.task.abort();
        let _ = child.kill().await;
        run
    });
    Ok(Stream {
        video,
        input,
        counters,
        cancel,
        task,
    })
}
fn packet(au: &AccessUnit, seq: u32, timestamp_ms: f64) -> Bytes {
    let mut b = BytesMut::with_capacity(16 + au.data.len());
    b.put_u16(0x5342);
    b.put_u8(1);
    b.put_u8(u8::from(au.key));
    b.put_u32(seq);
    b.put_f64(timestamp_ms);
    b.extend_from_slice(&au.data);
    b.freeze()
}
