//! Stand-in for llama-server used by the serve integration test.
//!
//! Records argv, answers `GET /health` with 200, and exits on `--version`
//! so the flash-attention probe does not hang.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpListener;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    // Record argv next to this executable. The integration test reads it back.
    // Avoid process-global env vars: `cargo test` runs tests on other threads.
    if let Ok(exe) = env::current_exe() {
        let record = exe.with_file_name("argv.txt");
        fs::write(record, args.join("\n")).expect("write argv record");
    }
    if args.iter().any(|arg| arg == "--version") {
        println!("fake llama-server");
        return;
    }

    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 9;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--host" => {
                if let Some(value) = rest.next() {
                    host = value.clone();
                }
            }
            "--port" => {
                if let Some(value) = rest.next() {
                    port = value.parse().expect("port is a u16");
                }
            }
            _ => {}
        }
    }

    let listener = TcpListener::bind((host.as_str(), port)).unwrap_or_else(|err| {
        eprintln!("fake backend failed to bind {host}:{port}: {err}");
        std::process::exit(1);
    });
    println!("fake backend listening on {host}:{port}");
    let _ = io::stdout().flush();

    for conn in listener.incoming() {
        let mut stream = match conn {
            Ok(stream) => stream,
            Err(_) => continue,
        };
        let mut buf = [0u8; 2048];
        let _ = stream.read(&mut buf);
        let body = b"ok";
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body);
    }
}
