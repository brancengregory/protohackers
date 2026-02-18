use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};
use std::collections::{BTreeMap, HashMap};

const RETRANSMISSION_TIMEOUT: Duration = Duration::from_secs(3);
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
enum SessionState {
	Handshake,
	Established,
	Closing,
}

#[derive(Debug)]
struct Session {
	id: String,
	source: SocketAddr,
	state: SessionState,
	last_active: Instant,
	next_expected_pos: usize,
	pending_data: BTreeMap<usize, String>,
	next_seq_to_send: usize,
	send_queue: BTreeMap<usize, (Instant, String)>,
}

impl Session {
	fn new(id: String, source: SocketAddr) -> Self {
		Self {
			id,
			source,
			state: SessionState::Handshake,
			last_active: Instant::now(),
			next_expected_pos: 0,
			pending_data: BTreeMap::new(),
			next_seq_to_send: 0,
			send_queue: BTreeMap::new(),
		}
	}
}

#[derive(Debug)]
enum Packet {
	Connect { session_id: String },
	Data { session_id: String, pos: usize, data: String },
	Ack { session_id: String, length: usize },
	Close { session_id: String },
}

impl TryFrom<&[u8]> for Packet {
	type Error = &'static str;

	fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
		let raw = str::from_utf8(value)
			.expect("Couldn't convert packet to string")
			.trim_ascii_end();
		println!("{}", raw);

		if raw.chars().next() != Some('/') {
			return Err("Expected first character to be '/'");
		}

		if raw.chars().last() != Some('/') {
			return Err("Expected last character to be '/'");
		}

		let trimmed = raw.trim_matches('/');
		println!("{:?}", trimmed);

		let splits: Vec<&str> = trimmed.split("/").collect();
		println!("{:?}", splits);

		if splits.is_empty() {
			return Err("Got empty message");
		}

		match splits[0] {
			"connect" => {
				if splits.len() != 2 {
					return Err("Message with type 'data' should have 4 parts including the type");
				}

				Ok(Packet::Connect {
					session_id: splits[1].to_string()
				})
			},
			"data" => {
				if splits.len() != 4 {
					return Err("Message with type 'data' should have 4 parts including the type");
				}

				let session_id = splits[1].to_string();
				let pos: usize = splits[2].parse().expect("Couldn't parse data position to usize");
				let data = splits[3].to_string();

				Ok(Packet::Data {
					session_id,
					pos,
					data
				})
			},
			"ack" => {
				if splits.len() != 3 {
					return Err("Message with type 'ack' should have 3 parts including the type");
				}

				let session_id = splits[1].to_string();
				let length: usize = splits[2].parse().expect("Couldn't parse ack length to usize");

				Ok(Packet::Ack {
					session_id,
					length
				})
			},
			"close" => {
				if splits.len() != 2 {
					return Err("Message with type 'close' should have 2 parts including the type");
				}

				let session_id = splits[1].to_string();
				Ok(Packet::Close {
					session_id
				})
			},
			_ => {
				return Err("Unsupported message type");
			},
		}
	}
}

fn handle_packet(packet: Packet, source: SocketAddr, socket: &mut UdpSocket, sessions: &mut HashMap<String, Session>) {
	match packet {
		Packet::Connect { session_id } => {
			let session = Session::new(session_id.clone(), source);
			sessions.entry(session_id.to_string())
				.or_insert(session);

			let response_str = format!("/ack/{}/0/", session_id);
			let response = response_str.as_bytes();
			let _ = socket.send_to(response, source);
		},
		Packet::Data { session_id, pos, data } => {
			match sessions.get(&session_id) {
				Some(session) => {
					if session.next_expected_pos == pos {
						let mut session_len = session.pending_data.values()
							.fold(0, |acc, s| {
								acc + s.len()
							});
						session_len += data.len();
						let response_str = format!("/ack/{}/{}/", session_id, session_len);
						let response = response_str.as_bytes();
						let _ = socket.send_to(response, source);
						return;
					} else {
						if session.pending_data.is_empty() {
							let response_str = format!("/ack/{}/0/", session_id);
							let response = response_str.as_bytes();
							let _ = socket.send_to(response, source);
							return;
						} else {
							let session_len = session.pending_data.values()
								.fold(0, |acc, s| {
									acc + s.len()
								});
							let response_str = format!("/ack/{}/{}/", session_id, session_len);
							let response = response_str.as_bytes();
							let _ = socket.send_to(response, source);
							return;
						}
					}
				},
				None => {
					let response_str = format!("/close/{}/", session_id);
					let response = response_str.as_bytes();
					let _ = socket.send_to(response, source);
					return;
				},
			}
		},
		Packet::Ack { session_id, length} => {
			match sessions.get(&session_id) {
				Some(session) => {
				},
				None => {
					let response_str = format!("/close/{}/", session_id);
					let response = response_str.as_bytes();
					let _ = socket.send_to(response, source);
					return;
				},
			}
		},
		Packet::Close { session_id } => {
			let _ = sessions.remove(&session_id);
			let response_str = format!("/close/{}/", session_id);
			let response = response_str.as_bytes();
			let _ = socket.send_to(response, source);
			return;
		},
	}
}

fn main() -> std::io::Result<()> {
	let socket = UdpSocket::bind("0.0.0.0:8080")?;

	let mut sessions: HashMap<String, Session> = HashMap::new();

	let mut buf = [0u8; 999];
	let mut socket_clone = socket.try_clone().expect("Couldn't clone socket");
	loop {
		match socket.recv_from(&mut buf) {
			Ok((amt, source)) => {
				match Packet::try_from(&buf[..amt]) {
					Ok(p) => {
						println!("{:?}", p);
						handle_packet(p, source, &mut socket_clone, &mut sessions)
					},
					Err(e) => eprintln!("Couldn't successfully parse the packet: {}", e),
				}
			},
			Err(e) => {
				eprintln!("Error receiving packet from client: {}", e);
				break;
			}
		};
	}

	Ok(())
}
