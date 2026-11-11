//! Blocking TCP client on a background thread.

use std::cell::Cell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use mir_proto::{decode_frame, encode, ClientMessage, ServerMessage};

pub enum NetEvent {
    Message(ServerMessage),
    Disconnected(String),
}

pub struct Connection {
    rx: Receiver<NetEvent>,
    tx: Sender<ClientMessage>,
    closed_reported: Cell<bool>,
}

impl Connection {
    pub fn connect(addr: &str) -> std::io::Result<Connection> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;
        let mut writer = stream;
        let (ev_tx, ev_rx) = mpsc::channel::<NetEvent>();
        let (msg_tx, msg_rx) = mpsc::channel::<ClientMessage>();

        let ev_tx_read = ev_tx.clone();
        thread::Builder::new()
            .name("net-read".into())
            .spawn(move || {
                let mut buf = Vec::with_capacity(8192);
                let mut chunk = [0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) => {
                            let _ = ev_tx_read.send(NetEvent::Disconnected("server closed".into()));
                            return;
                        }
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            loop {
                                match decode_frame::<ServerMessage>(&mut buf) {
                                    Ok(Some(msg)) => {
                                        if ev_tx_read.send(NetEvent::Message(msg)).is_err() {
                                            return;
                                        }
                                    }
                                    Ok(None) => break,
                                    Err(e) => {
                                        let _ =
                                            ev_tx_read.send(NetEvent::Disconnected(e.to_string()));
                                        return;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = ev_tx_read.send(NetEvent::Disconnected(e.to_string()));
                            return;
                        }
                    }
                }
            })?;

        thread::Builder::new()
            .name("net-write".into())
            .spawn(move || loop {
                let msg = match msg_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(msg) => msg,
                    Err(RecvTimeoutError::Timeout) => ClientMessage::Ping { nonce: 0 },
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                let frame = match encode(&msg) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                if writer.write_all(&frame).is_err() {
                    let _ = ev_tx.send(NetEvent::Disconnected("write failed".into()));
                    return;
                }
            })?;

        Ok(Connection {
            rx: ev_rx,
            tx: msg_tx,
            closed_reported: Cell::new(false),
        })
    }

    pub fn send(&self, msg: ClientMessage) {
        let _ = self.tx.send(msg);
    }

    pub fn poll(&self) -> Option<NetEvent> {
        match self.rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) if !self.closed_reported.replace(true) => {
                Some(NetEvent::Disconnected("channel closed".into()))
            }
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn closed_network_channel_reports_disconnect_once() {
        let (event_tx, rx) = mpsc::channel();
        let (tx, _) = mpsc::channel();
        let connection = Connection {
            rx,
            tx,
            closed_reported: Cell::new(false),
        };
        drop(event_tx);
        assert!(matches!(connection.poll(), Some(NetEvent::Disconnected(_))));
        assert!(connection.poll().is_none());
    }

    #[test]
    fn heartbeat_continues_without_render_frames() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _connection = Connection::connect(&listener.local_addr().unwrap().to_string()).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(7)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(ClientMessage::Ping { .. }) = decode_frame(&mut bytes).unwrap() {
                break;
            }
        }
    }
}
