use screenbridge::rfb::{self, PixelLayout};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};

pub async fn fixture(
    layout: PixelLayout,
    bad_rect: bool,
    bad_format: bool,
) -> (u16, mpsc::Receiver<Vec<u8>>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel(8);
    let task = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        macro_rules! recv {
            ($n:expr) => {{
                let mut b = vec![0; $n];
                if s.read_exact(&mut b).await.is_err() {
                    return;
                }
                b
            }};
        }
        for byte in b"RFB 003.889\n" {
            if s.write_all(&[*byte]).await.is_err() {
                return;
            }
        }
        recv!(12);
        s.write_all(&[1, 2]).await.unwrap();
        recv!(1);
        let challenge = [7; 16];
        s.write_all(&challenge).await.unwrap();
        assert_eq!(recv!(16), rfb::challenge_response(challenge, "fixture"));
        s.write_all(&[0; 4]).await.unwrap();
        recv!(1);
        let mut init = vec![0; 24];
        init[..4].copy_from_slice(&[0, 64, 0, 48]);
        init[4] = if bad_format { 16 } else { 32 };
        init[5] = 24;
        init[7] = 1;
        init[8..14].copy_from_slice(&[0, 255, 0, 255, 0, 255]);
        init[14] = if layout == PixelLayout::Bgra { 16 } else { 0 };
        init[15] = 8;
        init[16] = if layout == PixelLayout::Bgra { 0 } else { 16 };
        s.write_all(&init).await.unwrap();
        loop {
            let kind = recv!(1)[0];
            match kind {
                2 => {
                    let b = recv!(3);
                    let count = u16::from_be_bytes([b[1], b[2]]) as usize;
                    recv!(count * 4);
                }
                3 => {
                    recv!(9);
                    let mut b = vec![0; 16 + 64 * 48 * 4];
                    b[3] = 1;
                    b[5] = u8::from(bad_rect);
                    b[9] = 64;
                    b[11] = 48;
                    for pixel in b[16..].chunks_exact_mut(4) {
                        pixel.copy_from_slice(if layout == PixelLayout::Bgra {
                            &[160, 80, 40, 255]
                        } else {
                            &[40, 80, 160, 255]
                        })
                    }
                    if s.write_all(&b).await.is_err() {
                        return;
                    }
                }
                4 => {
                    let b = recv!(7);
                    let _ = tx.send(b).await;
                }
                5 => {
                    let b = recv!(5);
                    let _ = tx.send(b).await;
                }
                _ => panic!("unexpected client message"),
            }
        }
    });
    (port, rx, task)
}
