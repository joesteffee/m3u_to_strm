//! Minimal HTTP server for integration tests: no extra dependencies, records every request.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Everything between the method and `HTTP/1.1`, e.g. `/emby/Items?Path=%2Fmnt%2Fa&Recursive=false`.
    pub target: String,
}

impl Request {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or(&self.target)
    }

    pub fn query_param(&self, wanted: &str) -> Option<String> {
        let (_, query) = self.target.split_once('?')?;
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == wanted).then(|| value.to_string())
        })
    }
}

pub type Handler = dyn Fn(&Request) -> (u16, String) + Send + Sync + 'static;

pub struct Server {
    pub url: String,
    hits: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start<F>(handler: F) -> Server
    where
        F: Fn(&Request) -> (u16, String) + Send + Sync + 'static,
    {
        Self::start_shared(Arc::new(handler))
    }

    pub fn start_shared(handler: Arc<Handler>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
        let port = listener.local_addr().expect("local address").port();
        let hits: Arc<Mutex<Vec<Request>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let thread = {
            let hits = Arc::clone(&hits);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                listener.set_nonblocking(true).expect("non-blocking accept");
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let hits = Arc::clone(&hits);
                            let handler = Arc::clone(&handler);
                            std::thread::spawn(move || {
                                let Some((method, target)) =
                                    read_request(stream.try_clone().expect("clone connection"))
                                else {
                                    return;
                                };
                                let request = Request { method, target };
                                hits.lock().expect("hits lock").push(request.clone());
                                let _ = respond(stream, handler.as_ref(), &request);
                            });
                        }
                        Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(_) => return,
                    }
                }
            })
        };

        Server {
            url: format!("http://127.0.0.1:{port}"),
            hits,
            stop,
            thread: Some(thread),
        }
    }

    pub fn hits(&self) -> Vec<Request> {
        self.hits.lock().expect("hits lock").clone()
    }

    pub fn count_matching(&self, predicate: impl Fn(&Request) -> bool) -> usize {
        self.hits().iter().filter(|hit| predicate(hit)).count()
    }
}

fn read_request(stream: TcpStream) -> Option<(String, String)> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        return None;
    }
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) => break,
            Ok(_) => {
                if header == "\r\n" || header == "\n" {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    Some((method, target))
}

fn respond(mut stream: TcpStream, handler: &Handler, request: &Request) -> std::io::Result<()> {
    let (status, body) = handler(request);
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Response",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
