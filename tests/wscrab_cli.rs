use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use assert_cmd::cargo::cargo_bin_cmd;
use futures_util::{SinkExt, StreamExt};
use predicates::str::contains;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;

fn write_cert_files(temp_dir: &Path) -> (std::path::PathBuf, std::path::PathBuf, Vec<u8>, Vec<u8>) {
    let cert =
        rcgen::generate_simple_self_signed(["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("generate cert");

    let cert_pem = cert.cert.pem();
    let cert_der = cert.cert.der().to_vec();
    let key_der = cert.key_pair.serialize_der();

    let pem_path = temp_dir.join("cert.pem");
    let der_path = temp_dir.join("cert.der");
    std::fs::write(&pem_path, cert_pem).expect("write pem");
    std::fs::write(&der_path, &cert_der).expect("write der");

    (pem_path, der_path, cert_der, key_der)
}

fn spawn_wss_server(
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
    send_ping_pong: bool,
    header_value: Option<Arc<Mutex<Option<String>>>>,
    capture_first: Option<Arc<Mutex<Option<String>>>>,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();

    let handle = thread::spawn(move || {
        let rt = Runtime::new().expect("runtime");
        rt.block_on(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            addr_tx.send(addr).unwrap();

            let (stream, _) = listener.accept().await.unwrap();

            let cert_chain = vec![CertificateDer::from(cert_der)];
            let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_der));
            let config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(cert_chain, key)
                .unwrap();
            let acceptor = TlsAcceptor::from(Arc::new(config));

            let tls_stream = acceptor.accept(stream).await.unwrap();

            let cb_header = header_value.clone();
            let callback = move |req: &Request, resp: Response| {
                if let Some(storage) = &cb_header {
                    if let Some(value) = req.headers().get("x-test") {
                        if let Ok(value) = value.to_str() {
                            *storage.lock().unwrap() = Some(value.to_string());
                        }
                    }
                }
                Ok(resp)
            };

            let mut ws_stream = accept_hdr_async(tls_stream, callback).await.unwrap();

            if let Some(storage) = capture_first {
                if let Some(Ok(message)) = ws_stream.next().await {
                    let record = match message {
                        Message::Ping(data) => {
                            format!("ping:{}", String::from_utf8_lossy(&data))
                        }
                        Message::Pong(data) => {
                            format!("pong:{}", String::from_utf8_lossy(&data))
                        }
                        Message::Close(_) => "close".to_string(),
                        Message::Text(text) => format!("text:{text}"),
                        Message::Binary(_) => "binary".to_string(),
                        Message::Frame(_) => "frame".to_string(),
                    };
                    *storage.lock().unwrap() = Some(record);
                }
            }

            if send_ping_pong {
                ws_stream
                    .send(Message::Ping(b"ping".to_vec()))
                    .await
                    .unwrap();
                ws_stream
                    .send(Message::Pong(b"pong".to_vec()))
                    .await
                    .unwrap();
            }

            ws_stream.send(Message::Close(None)).await.ok();
        });
    });

    let addr = addr_rx.recv().unwrap();
    (addr, handle)
}

#[test]
fn help_when_no_args() {
    let mut cmd = cargo_bin_cmd!("wscrab");
    cmd.assert().success().stdout(contains("wscrab"));
}

#[test]
fn header_is_sent() {
    let temp = tempfile::tempdir().unwrap();
    let (_pem_path, _der_path, cert_der, key_der) = write_cert_files(temp.path());
    let header = Arc::new(Mutex::new(None));

    let (addr, handle) = spawn_wss_server(cert_der, key_der, false, Some(header.clone()), None);

    let mut cmd = cargo_bin_cmd!("wscrab");
    cmd.arg("--connect")
        .arg(format!("wss://{addr}"))
        .arg("--no-check")
        .arg("--header")
        .arg("X-Test:hello");

    cmd.assert().success();

    handle.join().unwrap();
    assert_eq!(header.lock().unwrap().clone(), Some("hello".to_string()));
}

#[test]
fn no_check_allows_self_signed() {
    let temp = tempfile::tempdir().unwrap();
    let (_pem_path, _der_path, cert_der, key_der) = write_cert_files(temp.path());
    let (addr, handle) = spawn_wss_server(cert_der, key_der, false, None, None);

    let mut cmd = cargo_bin_cmd!("wscrab");
    cmd.arg("--connect")
        .arg(format!("wss://{addr}"))
        .arg("--no-check");

    cmd.assert().success();
    handle.join().unwrap();
}

#[test]
fn cert_pem_allows_self_signed() {
    let temp = tempfile::tempdir().unwrap();
    let (pem_path, _der_path, cert_der, key_der) = write_cert_files(temp.path());
    let (addr, handle) = spawn_wss_server(cert_der, key_der, false, None, None);

    let mut cmd = cargo_bin_cmd!("wscrab");
    cmd.arg("--connect")
        .arg(format!("wss://{addr}"))
        .arg("--cert")
        .arg(pem_path);

    cmd.assert().success();
    handle.join().unwrap();
}

#[test]
fn cert_der_allows_self_signed() {
    let temp = tempfile::tempdir().unwrap();
    let (_pem_path, der_path, cert_der, key_der) = write_cert_files(temp.path());
    let (addr, handle) = spawn_wss_server(cert_der, key_der, false, None, None);

    let mut cmd = cargo_bin_cmd!("wscrab");
    cmd.arg("--connect")
        .arg(format!("wss://{addr}"))
        .arg("--cert")
        .arg(der_path);

    cmd.assert().success();
    handle.join().unwrap();
}

#[test]
fn show_ping_pong_prints_messages() {
    let temp = tempfile::tempdir().unwrap();
    let (_pem_path, _der_path, cert_der, key_der) = write_cert_files(temp.path());
    let (addr, handle) = spawn_wss_server(cert_der, key_der, true, None, None);

    let mut cmd = cargo_bin_cmd!("wscrab");
    cmd.arg("--connect")
        .arg(format!("wss://{addr}"))
        .arg("--no-check")
        .arg("--show-ping-pong");

    cmd.assert()
        .success()
        .stdout(contains("< Received ping (data: \"ping\")"))
        .stdout(contains("< Received pong (data: \"pong\")"));

    handle.join().unwrap();
}

#[test]
fn slash_ping_sends_control_frame() {
    let temp = tempfile::tempdir().unwrap();
    let (_pem_path, _der_path, cert_der, key_der) = write_cert_files(temp.path());
    let capture = Arc::new(Mutex::new(None));
    let (addr, handle) = spawn_wss_server(cert_der, key_der, false, None, Some(capture.clone()));

    let mut cmd = cargo_bin_cmd!("wscrab");
    cmd.arg("--connect")
        .arg(format!("wss://{addr}"))
        .arg("--no-check")
        .arg("--slash")
        .write_stdin("/ping hello\n");

    cmd.assert().success();
    handle.join().unwrap();

    assert_eq!(
        capture.lock().unwrap().clone(),
        Some("ping:hello".to_string())
    );
}
