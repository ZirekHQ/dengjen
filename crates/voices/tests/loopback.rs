use dengjen_voices::transport::{Transport, UreqTransport};
use dengjen_voices::VoiceError;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const BODY: &[u8] = b"0123456789abcdef";

fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        head.push(byte[0]);
    }
    String::from_utf8_lossy(&head).to_ascii_lowercase()
}

fn range_start(head: &str) -> Option<usize> {
    head.lines()
        .find_map(|line| line.strip_prefix("range: bytes="))
        .and_then(|range| range.trim_end_matches('-').trim().parse().ok())
}

fn respond(mut stream: TcpStream, heads: &Sender<String>) {
    let head = read_head(&mut stream);
    let _ = heads.send(head.clone());
    let (status, extra, payload) = match range_start(&head) {
        Some(from) => (
            "206 Partial Content",
            format!(
                "Content-Range: bytes {from}-{}/{}\r\n",
                BODY.len() - 1,
                BODY.len()
            ),
            &BODY[from..],
        ),
        None => ("200 OK", String::new(), BODY),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(payload);
}

fn redirect(mut stream: TcpStream, heads: &Sender<String>) {
    let _ = heads.send(read_head(&mut stream));
    let _ = stream.write_all(
        b"HTTP/1.1 302 Found\r\nLocation: http://example.com/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
}

fn stall(mut stream: TcpStream, heads: &Sender<String>) {
    let _ = heads.send(read_head(&mut stream));
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        BODY.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&BODY[..4]);
    let _ = stream.flush();
    std::thread::sleep(Duration::from_secs(30));
}

fn trickle(mut stream: TcpStream, heads: &Sender<String>) {
    let _ = heads.send(read_head(&mut stream));
    let header = "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(header.as_bytes());
    for byte in b"abcdef" {
        let _ = stream.write_all(&[*byte]);
        let _ = stream.flush();
        std::thread::sleep(Duration::from_millis(400));
    }
}

fn serve_with(handler: fn(TcpStream, &Sender<String>)) -> (String, Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/voice", listener.local_addr().unwrap());
    let (heads, received) = channel();
    std::thread::spawn(move || {
        listener
            .incoming()
            .flatten()
            .for_each(|stream| handler(stream, &heads))
    });
    (url, received)
}

fn serve() -> (String, Receiver<String>) {
    serve_with(respond)
}

fn read_all(mut body: Box<dyn Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    body.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_plain_get_returns_the_whole_body() {
    let response = UreqTransport::new().get(&serve().0, None).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.content_range_total, None);
    assert_eq!(read_all(response.body), BODY);
}

#[test]
fn a_ranged_get_returns_the_tail_and_the_total_size() {
    let response = UreqTransport::new().get(&serve().0, Some(4)).unwrap();
    assert_eq!(response.status, 206);
    assert_eq!(response.content_range_total, Some(16));
    assert_eq!(read_all(response.body), &BODY[4..]);
}

#[test]
fn plain_http_to_a_remote_host_is_refused_before_connecting() {
    let result = UreqTransport::new().get("http://example.com/voices.json", None);
    assert!(
        matches!(result, Err(VoiceError::Network(message)) if message.contains("refusing non-HTTPS URL"))
    );
}

#[test]
fn requests_ask_for_an_identity_encoding() {
    for range_from in [None, Some(4)] {
        let (url, heads) = serve();
        UreqTransport::new().get(&url, range_from).unwrap();
        let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(head.contains("accept-encoding: identity"), "{head}");
    }
}

#[test]
fn loopback_redirects_are_not_followed() {
    let (url, heads) = serve_with(redirect);
    let response = UreqTransport::new().get(&url, None).unwrap();
    assert_eq!(response.status, 302);
    assert!(heads.recv_timeout(Duration::from_secs(5)).is_ok());
}

#[test]
fn a_stalled_body_fails_after_the_stall_timeout() {
    let (url, _heads) = serve_with(stall);
    let transport = UreqTransport::with_stall_timeout(Duration::from_millis(300));
    let mut body = transport.get(&url, None).unwrap().body;
    let started = Instant::now();
    let result = body.read_to_end(&mut Vec::new());
    assert!(result.is_err(), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_slow_but_steady_body_is_not_cut_off_by_the_stall_timeout() {
    let (url, _heads) = serve_with(trickle);
    let transport = UreqTransport::with_stall_timeout(Duration::from_secs(1));
    let body = transport.get(&url, None).unwrap().body;
    assert_eq!(read_all(body), b"abcdef");
}
