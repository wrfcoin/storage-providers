use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

pub struct MockHttp {
    pub url: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl MockHttp {
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        count.fetch_add(1, Ordering::SeqCst);
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                            .unwrap();
                        let mut bytes = [0; 8192];
                        let n = stream.read(&mut bytes).unwrap_or(0);
                        let request = String::from_utf8_lossy(&bytes[..n]);
                        let body = if request.starts_with("GET /price/") {
                            "42"
                        } else if request.starts_with("GET /tx/") {
                            "{\"id\":\"abc\",\"data_size\":\"4\"}"
                        } else {
                            "data"
                        };
                        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1))
                    }
                    Err(e) => panic!("mock accept: {e}"),
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}
impl Drop for MockHttp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
