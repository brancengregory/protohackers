use crossbeam_channel::{Sender, unbounded};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Error, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

enum ClientMessage {
    Welcome { id: usize, members: String },
    Text(String),
}

#[derive(Debug)]
struct ChatMessage {
    client_id: usize,
    content: String,
}

enum Event {
    Join {
        name: String,
        sender: Sender<ClientMessage>,
    },
    Message(ChatMessage),
    Leave {
        id: usize,
    },
}

struct Client {
    name: String,
    sender: Sender<ClientMessage>,
}

fn is_alphanumeric(text: &str) -> bool {
    text.chars().all(|t| char::is_alphanumeric(t))
}

fn handle_invite(
    reader: &mut BufReader<TcpStream>,
    writer: &mut BufWriter<TcpStream>,
) -> Result<String, std::io::Error> {
    let invite_message = "Welcome to budgetchat! What shall I call you?\n";
    writer.write_all(invite_message.as_bytes())?;
    writer.flush()?;

    let mut client_name = String::new();
    loop {
        client_name.clear();
        match reader.read_line(&mut client_name) {
            Ok(_) => {
                let formatted_name = client_name.trim().to_string();
                if formatted_name.is_empty() || !is_alphanumeric(&formatted_name) {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        "Name cannot be empty, and must be alphanumeric",
                    ));
                } else {
                    return Ok(client_name.trim().to_string());
                }
            }
            Err(e) => return Err(e),
        }
    }
}

fn handle_client(stream: TcpStream, broker_tx: Sender<Event>) {
    let write_stream = stream
        .try_clone()
        .expect("Couldn't clone stream for writing");

    let mut reader = BufReader::new(stream);
    let mut writer = BufWriter::new(write_stream);

    let client_name = match handle_invite(&mut reader, &mut writer) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Couldn't set client name: {}", e);
            return;
        }
    };

    let (client_tx, client_rx) = unbounded::<ClientMessage>();

    broker_tx
        .send(Event::Join {
            name: client_name.clone(),
            sender: client_tx,
        })
        .unwrap();

    let client_id = match client_rx.recv().unwrap() {
        ClientMessage::Welcome { id, members } => {
            println!("User '{}' assigned ID {}", client_name, id);
            let _ = writeln!(writer, "* The room contains: {} *", members);
            let _ = writer.flush();
            id
        }
        _ => {
            eprintln!("Protocol mismatch. No welcome completed yet.");
            return;
        }
    };

    let broker_tx_clone = broker_tx.clone();

    thread::spawn(move || {
        let mut buffer = String::new();
        loop {
            buffer.clear();
            match reader.read_line(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    let content = buffer.trim().to_string();
                    if !content.is_empty() {
                        broker_tx_clone
                            .send(Event::Message(ChatMessage { client_id, content }))
                            .unwrap();
                    }
                }
                Err(_) => break,
            }
        }
        broker_tx_clone
            .send(Event::Leave { id: client_id })
            .unwrap();
    });

    for msg in client_rx {
        match msg {
            ClientMessage::Text(text) => {
                let _ = writeln!(writer, "{}", text);
                let _ = writer.flush();
            }
            _ => {}
        }
    }
}

fn main() -> std::io::Result<()> {
    let (broker_tx, broker_rx) = unbounded::<Event>();

    let broker_handle = thread::spawn(move || {
        let mut clients: HashMap<usize, Client> = HashMap::new();
        let mut id_counter: usize = 0;

        for event in broker_rx {
            match event {
                Event::Join { name, sender } => {
                    let id = id_counter;
                    id_counter += 1;

                    let names: Vec<&str> = clients.values().map(|c| c.name.as_str()).collect();
                    let members = if names.is_empty() {
                        "...just you it seems...".to_string()
                    } else {
                        names.join(", ")
                    };

                    sender.send(ClientMessage::Welcome { id, members }).unwrap();
                    clients.insert(
                        id,
                        Client {
                            name: name.clone(),
                            sender,
                        },
                    );

                    let announcement = format!("* {} has entered the room", name);
                    for (client_id, client) in &clients {
                        if *client_id != id {
                            let _ = client
                                .sender
                                .send(ClientMessage::Text(announcement.clone()));
                        }
                    }
                }
                Event::Message(message) => {
                    if let Some(client_info) = clients.get(&message.client_id) {
                        let formatted_msg = format!("[{}] {}", client_info.name, message.content);
                        for (client_id, client) in &clients {
                            if *client_id != message.client_id {
                                let _ = client
                                    .sender
                                    .send(ClientMessage::Text(formatted_msg.clone()));
                            }
                        }
                    }
                }
                Event::Leave { id } => {
                    println!("Client {} left", id);
                    let name = clients.get(&id).unwrap().name.clone();
                    clients.remove(&id);

                    let announcement = format!("* {} has left the room", name);
                    for (_, client) in &clients {
                        let _ = client
                            .sender
                            .send(ClientMessage::Text(announcement.clone()));
                    }
                }
            }
        }
    });

    let listener = TcpListener::bind("0.0.0.0:8080")?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let tx = broker_tx.clone();
                thread::spawn(move || {
                    handle_client(stream, tx);
                });
            }
            Err(e) => {
                eprintln!("Connection failed: {}", e);
            }
        }
    }

    drop(broker_handle);

    Ok(())
}
