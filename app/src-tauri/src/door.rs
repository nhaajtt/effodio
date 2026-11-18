//! The local search door: other tools on this computer (a script, an editor, an assistant) can search the index
//! over HTTP. It's off unless switched on in Settings. It listens on 127.0.0.1 only, never on the network; every
//! request must carry the token; and it can only read: `GET /search?q=…&limit=…` and `GET /status`.

use crate::App;
use anyhow::Result;
use effodio_core::config;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub struct Door {
    stop: Arc<AtomicBool>,
    listening: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Door {
    /// Waits for the listener to let go of the port, so a door opened again right after (new token, same port)
    /// can have it.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(listening) = self.listening.take() {
            let _ = listening.join();
        }
    }
}

/// Requests answered at once; more than this from other programs wait for the next turn instead of each getting a
/// thread.
const AT_ONCE: usize = 8;

struct Done(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 32 random bytes as hex, from the system's generator.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("the system has a random generator");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Compares without stopping at the first difference, so timing says nothing about the token.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |sum, (x, y)| sum | (x ^ y)) == 0
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() && text.is_char_boundary(index + 1) && text.is_char_boundary(index + 3) => {
                match u8::from_str_radix(&text[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            other => out.push(other),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(stream: &mut TcpStream, status: &str, body: &Value) {
    let body = body.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn handle(app: &AppHandle, stream: &mut TcpStream, token: &str, port: u16) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let mut reader = BufReader::new(stream.try_clone().expect("a socket can be cloned").take(16 * 1024));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let (mut authorized, mut host_ok) = (false, false);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line.trim().is_empty() => break,
            Ok(_) => {}
        }
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "authorization" => {
                authorized =
                    value.strip_prefix("Bearer ").is_some_and(|given| same(given.as_bytes(), token.as_bytes()));
            }
            // A web page that points another name at 127.0.0.1 sends that name here.
            "host" => host_ok = value == format!("127.0.0.1:{port}") || value == format!("localhost:{port}"),
            _ => {}
        }
    }
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    if !host_ok {
        return respond(stream, "403 Forbidden", &json!({"error": "unknown host"}));
    }
    if !authorized {
        return respond(stream, "401 Unauthorized", &json!({"error": "send the token as 'Authorization: Bearer …'"}));
    }
    if method != "GET" {
        return respond(stream, "405 Method Not Allowed", &json!({"error": "the door only reads"}));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let parameter = |name: &str| -> Option<String> {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, v)| percent_decode(v))
    };
    let shared = app.state::<App>();
    match path {
        "/search" => {
            let Some(words) = parameter("q").filter(|q| !q.trim().is_empty()) else {
                return respond(stream, "400 Bad Request", &json!({"error": "say what to look for: /search?q=…"}));
            };
            let limit = parameter("limit").and_then(|l| l.parse::<u64>().ok()).unwrap_or(10).clamp(1, 50);
            match shared.serve.ask(json!({"op": "search", "q": words, "limit": limit}), Duration::from_secs(60)) {
                Ok(result) => respond(stream, "200 OK", &result),
                Err(error) => respond(stream, "500 Internal Server Error", &json!({"error": error.to_string()})),
            }
        }
        "/status" => match shared.serve.ask(json!({"op": "counts"}), Duration::from_secs(30)) {
            Ok(result) => respond(stream, "200 OK", &result),
            Err(error) => respond(stream, "500 Internal Server Error", &json!({"error": error.to_string()})),
        },
        _ => respond(stream, "404 Not Found", &json!({"error": "there's /search and /status"})),
    }
}

impl Door {
    pub fn open(app: AppHandle, settings: &config::Door) -> Result<Door> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, settings.port)))?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (token, port, stopped) = (settings.token.clone(), settings.port, stop.clone());
        let busy = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listening = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                if busy.load(Ordering::Relaxed) >= AT_ONCE {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                match listener.accept() {
                    Ok((mut stream, address)) => {
                        if !address.ip().is_loopback() {
                            continue;
                        }
                        let _ = stream.set_nonblocking(false);
                        let (app, token, busy) = (app.clone(), token.clone(), busy.clone());
                        busy.fetch_add(1, Ordering::Relaxed);
                        std::thread::spawn(move || {
                            // Counted out however the request ends, a panic included.
                            let _done = Done(busy);
                            handle(&app, &mut stream, &token, port);
                            let _ = stream.flush();
                            let mut rest = [0u8; 64];
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
                            let _ = stream.read(&mut rest);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(150)),
                }
            }
        });
        Ok(Door { stop, listening: Some(listening) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What closing a door and opening it again on the same port does, without the app around it.
    #[test]
    fn the_port_is_free_once_the_door_is_closed() {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let listening = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                if listener.accept().is_err() {
                    std::thread::sleep(Duration::from_millis(150));
                }
            }
        });
        drop(Door { stop, listening: Some(listening) });
        assert!(TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).is_ok());
    }

    #[test]
    fn tokens_and_decoding() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(same(a.as_bytes(), a.as_bytes()));
        assert!(!same(a.as_bytes(), b.as_bytes()));
        assert!(!same(b"short", b"longer"));
        assert_eq!(percent_decode("dog+on+the%20beach%21"), "dog on the beach!");
        assert_eq!(percent_decode("h%C3%B3a+%C4%91%C6%A1n"), "hóa đơn");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
    }
}
