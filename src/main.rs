use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{CommandFactory, Parser};
use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderName;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_tls_with_config, Connector};

// CLI options (connect-only subset)
#[derive(Parser, Debug)]
#[command(name = "wscrab", version, about = "WebSocket cat (Rust subset)")]
struct Opts {
    #[arg(long, short = 'c', help = "Connect to a WebSocket server")]
    connect: Option<String>,

    #[arg(long, help = "Client certificate file (PEM/DER)")]
    cert: Option<PathBuf>,

    #[arg(long = "header", short = 'H', help = "Set an HTTP header (repeatable)")]
    header: Vec<String>,

    #[arg(long = "no-check", help = "Skip server certificate verification")]
    no_check: bool,

    #[arg(long = "show-ping-pong", help = "Print notifications for ping/pong")]
    show_ping_pong: bool,

    #[arg(long, help = "Enable slash commands (/ping, /pong, /close)")]
    slash: bool,
}

// Custom verifier for --no-check (skip server certificate validation)
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

// Entry: parse args; show help when --connect is missing
#[tokio::main]
async fn main() {
    let opts = Opts::parse();

    if opts.connect.is_none() {
        let mut cmd = Opts::command();
        cmd.print_help().ok();
        println!();
        return;
    }

    if let Err(err) = run(opts).await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

// Connect and enter the interactive loop
async fn run(opts: Opts) -> Result<(), Box<dyn std::error::Error>> {
    let mut connect_url = opts.connect.unwrap();
    if !connect_url.contains("://") {
        // Match wscat: default to ws:// when scheme is missing
        connect_url = format!("ws://{connect_url}");
    }

    let mut request = connect_url.clone().into_client_request()?;
    // Parse repeatable -H/--header values
    for header in opts.header {
        let (name, value) = parse_header(&header)?;
        request.headers_mut().insert(name, value);
    }

    // TLS config is only needed for wss
    let connector = if connect_url.starts_with("wss://") {
        Some(Connector::Rustls(Arc::new(build_tls_config(
            opts.cert.as_deref(),
            opts.no_check,
        )?)))
    } else {
        None
    };

    let (ws_stream, _) = connect_async_tls_with_config(request, None, false, connector).await?;
    println!("Connected (press CTRL+C to quit)");

    let (mut write, mut read) = ws_stream.split();
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    // Handle stdin input, server messages, and Ctrl+C concurrently
    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if opts.slash && line.starts_with('/') {
                            if handle_slash_command(&line, &mut write).await? {
                                break;
                            }
                        } else {
                            println!("> {line}");
                            write.send(Message::Text(line)).await?;
                        }
                    }
                    Ok(None) => break,
                    Err(err) => return Err(err.into()),
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(message)) => {
                        if handle_message(message, &mut write, opts.show_ping_pong).await? {
                            break;
                        }
                    }
                    Some(Err(err)) => return Err(err.into()),
                    None => break,
                }
            }
            _ = tokio::signal::ctrl_c() => {
                write.send(Message::Close(None)).await.ok();
                break;
            }
        }
    }

    Ok(())
}

// Handle slash commands for control frames. Returns true if connection should close.
async fn handle_slash_command(
    line: &str,
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
) -> Result<bool, tokio_tungstenite::tungstenite::Error> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let cmd = tokens
        .first()
        .and_then(|v| v.strip_prefix('/'))
        .unwrap_or("");

    match cmd {
        "ping" => {
            let data = tokens.get(1).copied().unwrap_or("").as_bytes().to_vec();
            write.send(Message::Ping(data)).await?;
        }
        "pong" => {
            let data = tokens.get(1).copied().unwrap_or("").as_bytes().to_vec();
            write.send(Message::Pong(data)).await?;
        }
        "close" => {
            let code = tokens
                .get(1)
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(1000);
            let reason = tokens.get(2..).unwrap_or(&[]).join(" ");
            let frame = CloseFrame {
                code: CloseCode::from(code),
                reason: reason.into(),
            };
            write.send(Message::Close(Some(frame))).await?;
            return Ok(true);
        }
        _ => {
            eprintln!("error: Unrecognized slash command.");
        }
    }

    Ok(false)
}

// Handle server messages; return true to exit the main loop
async fn handle_message(
    message: Message,
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    show_ping_pong: bool,
) -> Result<bool, tokio_tungstenite::tungstenite::Error> {
    match message {
        Message::Text(text) => {
            println!("< {text}");
        }
        Message::Binary(data) => {
            let text = String::from_utf8_lossy(&data);
            println!("< {text}");
        }
        Message::Ping(data) => {
            if show_ping_pong {
                let text = String::from_utf8_lossy(&data);
                println!("< Received ping (data: \"{text}\")");
            }
            write.send(Message::Pong(data)).await?;
        }
        Message::Pong(data) => {
            if show_ping_pong {
                let text = String::from_utf8_lossy(&data);
                println!("< Received pong (data: \"{text}\")");
            }
        }
        Message::Close(_) => return Ok(true),
        Message::Frame(_) => {}
    }
    Ok(false)
}

// Parse "Header:Value" (split on the first colon only)
fn parse_header(header: &str) -> Result<(HeaderName, HeaderValue), Box<dyn std::error::Error>> {
    let pos = header.find(':').ok_or("header must contain ':'")?;
    let name = header[..pos].trim();
    let value = header[pos + 1..].trim();
    let name = HeaderName::from_bytes(name.as_bytes())?;
    let value = HeaderValue::from_str(value)?;
    Ok((name, value))
}

// Build TLS config: support self-signed via --no-check and custom cert via --cert
fn build_tls_config(
    cert_path: Option<&std::path::Path>,
    no_check: bool,
) -> Result<ClientConfig, Box<dyn std::error::Error>> {
    let mut root_store = RootCertStore::empty();
    if !no_check {
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }

    // Allow cert chain + private key in one PEM file; DER is treated as cert only
    let (certs, key) = if let Some(path) = cert_path {
        let bytes = fs::read(path)?;
        load_certs_and_key(&bytes)?
    } else {
        (Vec::new(), None)
    };

    if !certs.is_empty() && !no_check {
        for cert in &certs {
            root_store.add(cert.clone())?;
        }
    }

    let builder = if no_check {
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
    } else {
        ClientConfig::builder().with_root_certificates(root_store)
    };

    let config = if let Some(key) = key {
        builder.with_client_auth_cert(certs, key)?
    } else {
        builder.with_no_client_auth()
    };

    Ok(config)
}

// Load PEM/DER certs and keys (PEM supports PKCS#8/RSA/EC)
fn load_certs_and_key(
    bytes: &[u8],
) -> Result<
    (Vec<CertificateDer<'static>>, Option<PrivateKeyDer<'static>>),
    Box<dyn std::error::Error>,
> {
    if bytes.windows(10).any(|w| w == b"-----BEGIN") {
        let mut reader = std::io::Cursor::new(bytes);
        let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;

        let mut reader = std::io::Cursor::new(bytes);
        let mut keys = Vec::new();
        for key in rustls_pemfile::pkcs8_private_keys(&mut reader) {
            keys.push(PrivateKeyDer::from(key?));
        }
        let mut reader = std::io::Cursor::new(bytes);
        for key in rustls_pemfile::rsa_private_keys(&mut reader) {
            keys.push(PrivateKeyDer::from(key?));
        }
        let mut reader = std::io::Cursor::new(bytes);
        for key in rustls_pemfile::ec_private_keys(&mut reader) {
            keys.push(PrivateKeyDer::from(key?));
        }

        Ok((certs, keys.into_iter().next()))
    } else {
        Ok((vec![CertificateDer::from(bytes.to_vec())], None))
    }
}
