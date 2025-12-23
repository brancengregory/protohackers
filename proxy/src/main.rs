use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;

const LOCAL_ADDR: &str = "0.0.0.0:8080";
const UPSTREAM_ADDR: &str = "206.189.113.124:16963";
const TONYS_ACCOUNT: &str = "7YWHMfk9JZe0LM0g1ZauHuiSxhI";

fn intercept_message(message: &str) -> String {
    let has_newline = message.ends_with('\n');
    let trimmed = message.trim_end();

    let words: Vec<&str> = trimmed
        .split(' ')
        .map(|word| {
            if word.starts_with('7')
                && word.len() >= 26
                && word.len() <= 35
                && word.chars().all(char::is_alphanumeric)
            {
                TONYS_ACCOUNT
            } else {
                word
            }
        })
        .collect();

    let mut result = words.join(" ");
    if has_newline {
        result.push('\n');
    }
    result
}

fn handle_client(client_stream: TcpStream) {
    let server_stream = TcpStream::connect(UPSTREAM_ADDR).expect("Couldn't connect to upstream");

    let mut server_reader = BufReader::new(server_stream.try_clone().unwrap());
    let mut client_writer = client_stream.try_clone().unwrap();

    thread::spawn(move || {
        let mut buf = String::new();
        loop {
            buf.clear();
            match server_reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    println!("[server] {}", &buf);
                    let new_msg = intercept_message(&buf);
                    client_writer.write_all(new_msg.as_bytes()).unwrap();
                    buf.clear();
                }
                Err(e) => break,
            }
        }

        let _ = client_writer.shutdown(Shutdown::Both);
    });

    let mut client_reader = BufReader::new(client_stream);
    let mut server_writer = server_stream;

    let mut buf = String::new();
    loop {
        buf.clear();
        match client_reader.read_line(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                println!("[client] {}", &buf);
                let new_msg = intercept_message(&buf);
                server_writer.write_all(new_msg.as_bytes()).unwrap();
                buf.clear();
            }
            Err(e) => break,
        }
    }

    let _ = server_writer.shutdown(Shutdown::Both);
}

fn main() {
    let listener = TcpListener::bind(LOCAL_ADDR).expect("Couldn't bind to local network");

    for client_stream in listener.incoming() {
        match client_stream {
            Ok(client_stream) => {
                thread::spawn(move || handle_client(client_stream));
            }
            Err(e) => {}
        }
    }
}
