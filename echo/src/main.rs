use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};

fn handle_client(stream: &mut TcpStream) {
	let mut buf = vec![];
	match stream.read_to_end(&mut buf) {
		Ok(m) => println!("Received: {}", m),
		Err(e) => eprintln!("Couldn't read stream: {}", e),
	};

	match stream.write_all(&buf) {
		Ok(_) => {},
		Err(e) => eprintln!("Couldn't write to stream: {}", e),
	}
}

fn main() -> std::io::Result<()> {
	let listener = TcpListener::bind("0.0.0.0:7")?;

	for stream in listener.incoming() {
		handle_client(&mut stream?);
	}

	Ok(())
}
