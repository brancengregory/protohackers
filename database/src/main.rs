use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};

type Store = HashMap<String, String>;

#[derive(Debug)]
enum Request {
    Insert { key: String, value: String },
    Retrieve { key: String },
    Version,
}

impl From<&[u8]> for Request {
    fn from(val: &[u8]) -> Self {
        let s = str::from_utf8(val).unwrap();

        if s.contains("=") {
            let (k, v) = s.split_once("=").unwrap();
            Request::Insert {
                key: k.trim().to_string(),
                value: v.to_string(),
            }
        } else {
            match s.trim() {
                "version" => Request::Version,
                _ => Request::Retrieve { key: s.to_string() },
            }
        }
    }
}

fn handle_request(req: Request, socket: &mut UdpSocket, source: SocketAddr, db: &mut Store) {
    match req {
        Request::Insert { key, value } => {
            db.insert(key, value);
        }
        Request::Retrieve { key } => {
            if let Some(val) = db.get(&key) {
                let resp = format!("{}={}", key, val);
                socket.send_to(resp.as_bytes(), source).unwrap();
            };
        }
        Request::Version => {
            let resp = "version=0.0.9";
            socket.send_to(resp.as_bytes(), source).unwrap();
        }
    }
}

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:8080")?;
    let mut db: HashMap<String, String> = HashMap::new();

    let mut buf = [0; 999];
    let mut socket_clone = socket.try_clone().unwrap();
    loop {
        match socket.recv_from(&mut buf) {
            Ok((amt, source)) => {
                let packet = &buf[..amt];
                let req = Request::from(packet);
                println!("Request: {:?}", req);

                handle_request(req, &mut socket_clone, source, &mut db);
            }
            Err(e) => {
                eprintln!("{}", e);
                break;
            }
        };
    }

    Ok(())
}
