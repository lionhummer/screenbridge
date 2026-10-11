use crate::error::{Error, Result};
use cipher::{BlockEncrypt, KeyInit};
use des::Des;
use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
const MAX_PIXELS: usize = 16_777_216;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelLayout {
    Bgra,
    Rgba,
}
impl PixelLayout {
    pub fn ffmpeg(self) -> &'static str {
        match self {
            Self::Bgra => "bgra",
            Self::Rgba => "rgba",
        }
    }
}
#[derive(Clone)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub layout: PixelLayout,
    pub pixels: Arc<[u8]>,
}
impl Frame {
    pub fn byte_len(width: u16, height: u16) -> Result<usize> {
        let pixels = usize::from(width) * usize::from(height);
        if width < 2 || height < 2 || pixels > MAX_PIXELS {
            return Err(Error::Limit("frame geometry"));
        }
        Ok(pixels * 4)
    }
}
#[derive(Debug)]
pub enum Input {
    Pointer { x: u16, y: u16, mask: u8 },
    Key { keysym: u32, down: bool },
    Release { done: oneshot::Sender<()> },
}
impl Input {
    pub fn encode(self, width: u16, height: u16) -> Vec<u8> {
        match self {
            Self::Release { .. } => Vec::new(),
            Self::Pointer { x, y, mask } => {
                let mut b = vec![5, mask];
                b.extend_from_slice(&x.min(width.saturating_sub(1)).to_be_bytes());
                b.extend_from_slice(&y.min(height.saturating_sub(1)).to_be_bytes());
                b
            }
            Self::Key { keysym, down } => {
                let mut b = vec![4, u8::from(down), 0, 0];
                b.extend_from_slice(&keysym.to_be_bytes());
                b
            }
        }
    }
}
pub fn challenge_response(challenge: [u8; 16], password: &str) -> [u8; 16] {
    let mut key = [0_u8; 8];
    for (dst, src) in key.iter_mut().zip(password.as_bytes()) {
        *dst = src.reverse_bits()
    }
    let cipher = Des::new((&key).into());
    let mut out = challenge;
    for block in out.chunks_exact_mut(8) {
        cipher.encrypt_block(block.into());
    }
    out
}
pub struct Capture {
    pub frames: watch::Receiver<Option<Frame>>,
    pub input: mpsc::Sender<Input>,
    pub task: JoinHandle<Result<()>>,
    cancel: CancellationToken,
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}
async fn read<const N: usize>(r: &mut (impl AsyncRead + Unpin)) -> Result<[u8; N]> {
    let mut b = [0; N];
    tokio::time::timeout(READ_TIMEOUT, r.read_exact(&mut b))
        .await
        .map_err(|_| Error::Timeout)??;
    Ok(b)
}
pub async fn connect(port: u16, password: &str, cancel: CancellationToken) -> Result<Capture> {
    let mut stream = tokio::select! {
        _=cancel.cancelled()=>return Err(Error::Cancelled),
        result=tokio::time::timeout(READ_TIMEOUT,TcpStream::connect((Ipv4Addr::LOCALHOST,port)))=>result.map_err(|_|Error::Timeout)??
    };
    stream.set_nodelay(true)?;
    let frame = tokio::select! {
        _=cancel.cancelled()=>return Err(Error::Cancelled),
        result=tokio::time::timeout(READ_TIMEOUT,handshake(&mut stream,password))=>result.map_err(|_|Error::Timeout)??
    };
    let (reader, writer) = stream.into_split();
    let (tx, rx) = mpsc::channel(128);
    let (frames, receiver) = watch::channel(None);
    let task_cancel = cancel.clone();
    let task = tokio::spawn(async move {
        let (requests, request_rx) = mpsc::channel(1);
        requests
            .send(request(false, frame.width, frame.height))
            .await
            .map_err(|_| Error::Closed)?;
        let mut writing = tokio::spawn(write_inputs(
            writer,
            rx,
            request_rx,
            frame.width,
            frame.height,
            task_cancel.clone(),
        ));
        let result = tokio::select! {
            _=task_cancel.cancelled()=>Ok(()),
            result=receive(reader,frame,frames,requests)=>result,
            result=&mut writing=>result.map_err(|_|Error::Closed)?
        };
        task_cancel.cancel();
        if !writing.is_finished() {
            let _ = tokio::time::timeout(Duration::from_secs(2), &mut writing).await;
        }
        result
    });
    Ok(Capture {
        frames: receiver,
        input: tx,
        task,
        cancel,
    })
}
async fn handshake(stream: &mut TcpStream, password: &str) -> Result<Frame> {
    let version = read::<12>(stream).await?;
    if !matches!(
        &version,
        b"RFB 003.007\n" | b"RFB 003.008\n" | b"RFB 003.889\n"
    ) {
        return Err(Error::Protocol("RFB version"));
    }
    stream.write_all(b"RFB 003.008\n").await?;
    let count = read::<1>(stream).await?[0] as usize;
    if count == 0 {
        return Err(Error::Auth);
    }
    let mut security = vec![0; count];
    stream.read_exact(&mut security).await?;
    if !security.contains(&2) {
        return Err(Error::Auth);
    }
    stream.write_all(&[2]).await?;
    let response = challenge_response(read::<16>(stream).await?, password);
    stream.write_all(&response).await?;
    if read::<4>(stream).await? != [0; 4] {
        return Err(Error::Auth);
    }
    stream.write_all(&[1]).await?;
    let init = read::<24>(stream).await?;
    let width = u16::from_be_bytes([init[0], init[1]]);
    let height = u16::from_be_bytes([init[2], init[3]]);
    let len = Frame::byte_len(width, height)?;
    if init[4] != 32
        || init[5] != 24
        || init[6] != 0
        || init[7] != 1
        || init[8..14] != [0, 255, 0, 255, 0, 255]
        || init[15] != 8
    {
        return Err(Error::Protocol("pixel format"));
    }
    let layout = match (init[14], init[16]) {
        (16, 0) => PixelLayout::Bgra,
        (0, 16) => PixelLayout::Rgba,
        _ => return Err(Error::Protocol("pixel shifts")),
    };
    let name_len = u32::from_be_bytes([init[20], init[21], init[22], init[23]]) as usize;
    if name_len > 4096 {
        return Err(Error::Limit("server name"));
    }
    let mut name = vec![0; name_len];
    stream.read_exact(&mut name).await?;
    // Only raw rectangles and desktop-size notifications are admitted.
    stream
        .write_all(&[2, 0, 0, 2, 0, 0, 0, 0, 255, 255, 255, 33])
        .await?;
    Ok(Frame {
        width,
        height,
        layout,
        pixels: vec![0; len].into(),
    })
}
fn request(incremental: bool, width: u16, height: u16) -> Vec<u8> {
    let mut b = vec![3, u8::from(incremental), 0, 0, 0, 0];
    b.extend_from_slice(&width.to_be_bytes());
    b.extend_from_slice(&height.to_be_bytes());
    b
}
async fn release_keys(
    writer: &mut OwnedWriteHalf,
    keys: &mut std::collections::HashSet<u32>,
    pointer: &mut Option<(u16, u16, u8)>,
    width: u16,
    height: u16,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(1), async {
        for keysym in keys.drain() {
            writer
                .write_all(
                    &Input::Key {
                        keysym,
                        down: false,
                    }
                    .encode(width, height),
                )
                .await?;
        }
        if let Some((x, y, mask)) = pointer.take() {
            if mask != 0 {
                writer
                    .write_all(&Input::Pointer { x, y, mask: 0 }.encode(width, height))
                    .await?;
            }
        }
        writer.flush().await?;
        Ok::<(), Error>(())
    })
    .await
    .map_err(|_| Error::Timeout)?
}
async fn write_inputs(
    mut writer: OwnedWriteHalf,
    mut inputs: mpsc::Receiver<Input>,
    mut requests: mpsc::Receiver<Vec<u8>>,
    width: u16,
    height: u16,
    cancel: CancellationToken,
) -> Result<()> {
    let mut keys = std::collections::HashSet::new();
    let mut pointer = None;
    let result=async {
        loop {
            let b=tokio::select! {
                _=cancel.cancelled()=>break,
                Some(input)=inputs.recv()=>match input {
                    Input::Release{done}=>{release_keys(&mut writer,&mut keys,&mut pointer,width,height).await?;let _=done.send(());continue},
                    Input::Key{keysym,down}=>{if down {if keys.len()>=128&&!keys.contains(&keysym){return Err(Error::Limit("held keys"))}keys.insert(keysym);}else{keys.remove(&keysym);}Input::Key{keysym,down}.encode(width,height)},
                    Input::Pointer{x,y,mask}=>{pointer=Some((x,y,mask));Input::Pointer{x,y,mask}.encode(width,height)}
                },
                Some(request)=requests.recv()=>request,
                else=>break
            };
            tokio::time::timeout(READ_TIMEOUT,writer.write_all(&b)).await.map_err(|_|Error::Timeout)??;
        }Ok::<(),Error>(())
    }.await;
    let _ = release_keys(&mut writer, &mut keys, &mut pointer, width, height).await;
    result
}
async fn receive(
    mut reader: OwnedReadHalf,
    mut frame: Frame,
    frames: watch::Sender<Option<Frame>>,
    requests: mpsc::Sender<Vec<u8>>,
) -> Result<()> {
    let mut pixels = frame.pixels.to_vec();
    loop {
        // A static desktop may remain idle indefinitely; cancellation drops this read.
        let kind = reader.read_u8().await?;
        match kind {
            0 => {
                tokio::time::timeout(READ_TIMEOUT, async {
                    let mut update_bytes = 0_usize;
                    let h = read::<3>(&mut reader).await?;
                    let count = u16::from_be_bytes([h[1], h[2]]);
                    if count > 4096 {
                        return Err(Error::Limit("rectangles"));
                    }
                    for _ in 0..count {
                        let r = read::<12>(&mut reader).await?;
                        let x = u16::from_be_bytes([r[0], r[1]]) as usize;
                        let y = u16::from_be_bytes([r[2], r[3]]) as usize;
                        let w = u16::from_be_bytes([r[4], r[5]]) as usize;
                        let h = u16::from_be_bytes([r[6], r[7]]) as usize;
                        let encoding = i32::from_be_bytes([r[8], r[9], r[10], r[11]]);
                        if encoding == -223 {
                            return Err(Error::Closed);
                        }
                        if encoding != 0 {
                            return Err(Error::Protocol("rectangle encoding"));
                        }
                        if x + w > frame.width as usize || y + h > frame.height as usize {
                            return Err(Error::Protocol("rectangle bounds"));
                        }
                        update_bytes = update_bytes.saturating_add(w * h * 4);
                        if update_bytes > MAX_PIXELS * 4 {
                            return Err(Error::Limit("update size"));
                        }
                        for row in 0..h {
                            let start = ((y + row) * frame.width as usize + x) * 4;
                            tokio::time::timeout(
                                READ_TIMEOUT,
                                reader.read_exact(&mut pixels[start..start + w * 4]),
                            )
                            .await
                            .map_err(|_| Error::Timeout)??;
                        }
                    }
                    if count > 0 {
                        frame.pixels = Arc::from(pixels.as_slice());
                        frames.send_replace(Some(frame.clone()));
                    }
                    requests
                        .send(request(true, frame.width, frame.height))
                        .await
                        .map_err(|_| Error::Closed)?;
                    Ok::<(), Error>(())
                })
                .await
                .map_err(|_| Error::Timeout)??;
            }
            2 => {}
            3 => {
                let h = read::<7>(&mut reader).await?;
                let len = u32::from_be_bytes([h[3], h[4], h[5], h[6]]) as usize;
                if len > 65536 {
                    return Err(Error::Limit("clipboard"));
                }
                let mut remaining = len;
                let mut scratch = [0; 4096];
                while remaining > 0 {
                    let n = remaining.min(scratch.len());
                    tokio::time::timeout(READ_TIMEOUT, reader.read_exact(&mut scratch[..n]))
                        .await
                        .map_err(|_| Error::Timeout)??;
                    remaining -= n;
                }
            }
            _ => return Err(Error::Protocol("server message")),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_rejects_before_allocation() {
        assert!(Frame::byte_len(0, 48).is_err());
        assert!(Frame::byte_len(65535, 65535).is_err());
        assert_eq!(Frame::byte_len(64, 48).ok(), Some(12288));
    }
    #[test]
    fn classic_vnc_known_vector() {
        let c = std::array::from_fn(|i| i as u8);
        assert_eq!(
            hex::encode(challenge_response(c, "password")),
            "b866924125c8eebb9debc1db61c538e2"
        );
    }
    #[test]
    fn pointer_is_clamped() {
        assert_eq!(
            Input::Pointer {
                x: 100,
                y: 200,
                mask: 1
            }
            .encode(64, 48),
            vec![5, 1, 0, 63, 0, 47]
        );
    }
}
