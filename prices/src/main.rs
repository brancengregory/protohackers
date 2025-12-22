use std::collections::BTreeMap;
use std::io::{BufReader, BufWriter, Error, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

// Need to know what client we are dealing with
// and hash it into some kind of session identifier
// so we save and query data attached to only that session.
//
// Messages are 9 bytes of binary.
// Multiple messages per connection.
// ** No newline between messages. Inferred from 9 byte structure **
//
// ---
// Byte Structure
// ---
// 1st: Message Type - 'I' (insert) or 'Q' (query) in ascii
// 2nd-8th: Two signed two's complement 32-bit integers
//          in network byte order (big_endian)
//
// Behavior is undefined if the type isn't I or Q.
//
// ---
// Insert Message
// ---
// Contains a timestamp and a price.
// Time stamp is formatted in seconds since Jan 1970, i.e. Unix epoch
// Price is in pennies at the given timestamp.
//
// Insertions may occur out-of-order.
// Prices can go negative.
// Behavior undefined for multiple prices with same timestamp for same client.
//
// ---
// Query Message
// ---
// Is a request to query the average price over a given time period.
//
// First int32 is mintime, second is maxtime.
// Both are inclusive.
//
// Round non-integer means up or down at our discretion.
//
// Send the mean as a single int32 (big-endian).
//
// ---
// Implementation Notes
// ---
// Rust's i32 does use two's complement representation, but defaults to
// system endianness which is often little-endian.
// We have to set endianness with i32::from_be_bytes()
//

#[derive(Debug)]
enum MessageType {
    Insert,
    Query,
}

#[derive(Debug)]
struct Message {
    kind: MessageType,
    content: (i32, i32),
}

impl TryFrom<&[u8]> for Message {
    type Error = &'static str;

    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        let raw_content: [u8; 9] = match b.try_into() {
            Ok(arr) => arr,
            Err(_) => return Err("Message must be 9 bytes"),
        };

        let message_kind = match b[0] {
            b'I' => MessageType::Insert,
            b'Q' => MessageType::Query,
            _ => return Err("First byte must be 'I' or 'Q'"),
        };

        let a = i32::from_be_bytes(
            raw_content[1..5]
                .try_into()
                .expect("Couldn't turn subset of raw content into array"),
        );
        let b = i32::from_be_bytes(
            raw_content[5..9]
                .try_into()
                .expect("Couldn't turn subset of raw content into array"),
        );

        Ok(Message {
            kind: message_kind,
            content: (a, b),
        })
    }
}

fn handle_insert(
    message_data: &(i32, i32),
    client_data: &mut BTreeMap<i32, i32>,
) -> Result<Option<i32>, Error> {
    client_data.insert(message_data.0, message_data.1);
    Ok(None)
}

fn handle_query(
    message_data: &(i32, i32),
    client_data: &mut BTreeMap<i32, i32>,
) -> Result<Option<i32>, Error> {
    if message_data.0 > message_data.1 {
        return Ok(Some(0));
    }

    let (count, sum) = client_data
        .range(message_data.0..=message_data.1)
        .fold((0i64, 0i64), |(c, s), (_, &price)| {
            (c + 1, s + price as i64)
        });

    let mean = if count == 0 { 0 } else { sum / count };

    Ok(Some(mean as i32))
}

fn handle_request(
    request: &[u8],
    writer: &mut BufWriter<TcpStream>,
    client_data: &mut BTreeMap<i32, i32>,
) -> std::io::Result<()> {
    let message = Message::try_from(request).map_err(std::io::Error::other)?;

    let res = match &message.kind {
        MessageType::Insert => handle_insert(&message.content, client_data),
        MessageType::Query => handle_query(&message.content, client_data),
    };

    match res {
        Ok(Some(n)) => {
            writer.write_all(&n.to_be_bytes())?;
            writer.flush()?;
        }
        Ok(None) => {}
        Err(e) => eprintln!("Failed to respond to message: {}", e),
    }

    Ok(())
}

fn handle_client(stream: TcpStream) {
    let write_stream = stream
        .try_clone()
        .expect("Couldn't clone stream for writing");

    let mut reader = BufReader::new(stream);
    let mut writer = BufWriter::new(write_stream);

    let mut client_data: BTreeMap<i32, i32> = BTreeMap::new();

    let chunk_size = 9;
    loop {
        let mut buffer = vec![0u8; chunk_size];
        match reader.read_exact(&mut buffer) {
            Ok(_) => {
                if let Err(e) = handle_request(&buffer, &mut writer, &mut client_data) {
                    eprintln!("Failed to handle request: {}", e);
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
