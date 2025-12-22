use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Error, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

#[derive(Debug, Serialize, Deserialize)]
struct PrimeRequest {
    method: String,
    number: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct PrimeResponse {
    method: String,
    prime: bool,
}

impl PrimeResponse {
    fn new(req: &PrimeRequest) -> Self {
        PrimeResponse {
            method: "isPrime".to_string(),
            prime: is_prime(req.number),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MalformedResponse {
    method: String,
}

impl MalformedResponse {
    fn new() -> Self {
        MalformedResponse {
            method: "Malformed".to_string(),
        }
    }

    fn write(self, writer: &mut BufWriter<TcpStream>) -> std::io::Result<()> {
        writer.write_all(&serde_json::to_vec(&self).expect("Couldn't serialize to JSON"))?;
        writer.write_all(b"\n")?;
        writer.flush()?;

        Ok(())
    }
}

fn is_prime(n: f64) -> bool {
    if n < 0.0 || n.fract() != 0.0 {
        return false;
    }

    let num = n as u64;

    match num {
        0 | 1 => false,
        2 => true,
        _ if num.is_multiple_of(2) => false,
        _ => {
            let limit = num.isqrt() + 1;
            !(3..=limit).step_by(2).any(|i| num.is_multiple_of(i))
        }
    }
}

fn handle_prime_request(
    request_str: &str,
    writer: &mut BufWriter<TcpStream>,
) -> std::io::Result<()> {
    let req: PrimeRequest = serde_json::from_str(request_str)?;
    println!("{:?}", req);

    if req.method != "isPrime" {
        return Err(Error::new(ErrorKind::InvalidData, "Invalid method"));
    }

    let resp = PrimeResponse::new(&req);
    writer
        .write_all(&serde_json::to_vec(&resp).expect("Couldn't serialize JSON to bytes"))
        .expect("Couldn't write response to buffer");

    writer
        .write_all(b"\n")
        .expect("Couldn't write newline to writer");
    writer.flush().expect("Couldn't flush writer");
    Ok(())
}

fn handle_client(stream: TcpStream) {
    let write_stream = stream
        .try_clone()
        .expect("Couldn't clone stream for writing");

    let mut reader = BufReader::new(stream);
    let mut writer = BufWriter::new(write_stream);

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Err(e) = handle_prime_request(&line, &mut writer) {
                    eprintln!("Failed to handle request: {}", e);
                    let resp = MalformedResponse::new();
                    if let Err(e) = resp.write(&mut writer) {
                        eprintln!("Failed to send malformed response: {}", e);
                    };
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8080")?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    handle_client(stream);
                });
            }
            Err(e) => {
                eprintln!("Connection failed: {}", e);
            }
        }
    }

    Ok(())
}
