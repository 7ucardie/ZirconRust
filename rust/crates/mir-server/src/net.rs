//! TCP listener: length-prefixed postcard frames in both directions.
//!
//! Hardening (Zircon `SConnection`/`Config`): bounded outbound queues (a
//! slow client is dropped rather than buffered without limit), a smaller
//! frame cap for client messages, an inbound frame budget per second, an
//! idle timeout (`Config.TimeOut` 20 s; the client pings every 5 s), and
//! caps on total and per-address connections.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mir_proto::{decode_frame_max, encode, ClientMessage, ServerMessage, CLIENT_MAX_FRAME_LEN};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::world::ConnId;

/// Queued server messages per connection before it counts as too slow.
pub const OUTBOUND_CAP: usize = 4096;
/// Client frames accepted per second before the connection is dropped.
pub const INBOUND_PER_SECOND: u32 = 200;
/// Zircon `Config.TimeOut`.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(20);
pub const MAX_CONNECTIONS: usize = 1000;
pub const MAX_PER_ADDRESS: usize = 32;

pub enum Inbound {
    Connected {
        conn: ConnId,
        addr: SocketAddr,
        tx: mpsc::Sender<ServerMessage>,
    },
    Message {
        conn: ConnId,
        msg: ClientMessage,
    },
    Disconnected {
        conn: ConnId,
    },
    /// Re-read the database (SIGHUP or a changed file).
    Reload,
    /// SIGINT/SIGTERM: save everything and exit.
    Shutdown,
}

#[derive(Default)]
struct Gate {
    total: AtomicUsize,
    per_ip: Mutex<HashMap<IpAddr, usize>>,
}

impl Gate {
    fn admit(&self, ip: IpAddr) -> bool {
        let mut per_ip = self.per_ip.lock().unwrap();
        let count = per_ip.entry(ip).or_insert(0);
        if *count >= MAX_PER_ADDRESS || self.total.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            return false;
        }
        *count += 1;
        self.total.fetch_add(1, Ordering::Relaxed);
        true
    }

    fn release(&self, ip: IpAddr) {
        let mut per_ip = self.per_ip.lock().unwrap();
        if let Some(c) = per_ip.get_mut(&ip) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                per_ip.remove(&ip);
            }
        }
        self.total.fetch_sub(1, Ordering::Relaxed);
    }
}

pub async fn listen(
    addr: SocketAddr,
    inbound: mpsc::UnboundedSender<Inbound>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "listening");
    let gate = Arc::new(Gate::default());
    let mut next_conn: ConnId = 1;
    loop {
        // EMFILE / ECONNABORTED are transient: log, back off, keep serving.
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                tracing::warn!("accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        if !gate.admit(peer.ip()) {
            tracing::warn!(%peer, "connection refused: limit reached");
            drop(stream);
            continue;
        }
        let conn = next_conn;
        next_conn += 1;
        let inbound = inbound.clone();
        let gate = gate.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, peer, conn, inbound).await {
                tracing::debug!(conn, "connection ended: {e}");
            }
            gate.release(peer.ip());
        });
    }
}

async fn handle(
    stream: TcpStream,
    peer: SocketAddr,
    conn: ConnId,
    inbound: mpsc::UnboundedSender<Inbound>,
) -> anyhow::Result<()> {
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(OUTBOUND_CAP);
    inbound.send(Inbound::Connected {
        conn,
        addr: peer,
        tx,
    })?;

    // Ends when the server drops its sender (a server-side disconnect) or
    // the socket breaks.
    let mut writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let frame = match encode(&msg) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("encode failed: {e}");
                    continue;
                }
            };
            // A client that stops reading blocks write_all forever and
            // would keep the connection alive after the server dropped it.
            match tokio::time::timeout(IDLE_TIMEOUT, writer.write_all(&frame)).await {
                Ok(Ok(())) => {}
                _ => break,
            }
        }
        let _ = writer.shutdown().await;
    });

    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let mut window = Instant::now();
    let mut budget = INBOUND_PER_SECOND;
    let result: anyhow::Result<()> = async {
        loop {
            let n = tokio::select! {
                r = tokio::time::timeout(IDLE_TIMEOUT, reader.read(&mut chunk)) => match r {
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => return Err(e.into()),
                    Err(_) => anyhow::bail!("idle for {}s", IDLE_TIMEOUT.as_secs()),
                },
                _ = &mut writer_task => return Ok(()),
            };
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
            while let Some(msg) = decode_frame_max::<ClientMessage>(&mut buf, CLIENT_MAX_FRAME_LEN)?
            {
                if window.elapsed() >= Duration::from_secs(1) {
                    window = Instant::now();
                    budget = INBOUND_PER_SECOND;
                }
                if budget == 0 {
                    anyhow::bail!("flooding: more than {INBOUND_PER_SECOND} frames per second");
                }
                budget -= 1;
                inbound.send(Inbound::Message { conn, msg })?;
            }
        }
    }
    .await;
    if let Err(error) = &result {
        tracing::warn!(conn, %peer, %error, "connection ended");
    }
    let _ = inbound.send(Inbound::Disconnected { conn });
    writer_task.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_caps_per_address_and_releases() {
        let gate = Gate::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..MAX_PER_ADDRESS {
            assert!(gate.admit(ip));
        }
        assert!(!gate.admit(ip));
        gate.release(ip);
        assert!(gate.admit(ip));
        assert_eq!(gate.total.load(Ordering::Relaxed), MAX_PER_ADDRESS);
    }
}
