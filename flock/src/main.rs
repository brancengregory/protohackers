use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use uuid::Uuid;

const LOCAL_ADDR: &str = "0.0.0.0:8080";

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct Ticket {
    plate: String,
    road: u16,
    mile1: u16,
    timestamp1: u32,
    mile2: u16,
    timestamp2: u32,
    speed: u16,
}

impl Ticket {
    fn write(self, stream: &mut TcpStream) -> std::io::Result<()> {
        let plate_bytes = self.plate.as_bytes();
        let plate_len = plate_bytes.len() as u8;

        stream.write_all(&[0x21])?;
        stream.write_all(&[plate_len])?;
        stream.write_all(plate_bytes)?;
        stream.write_all(&self.road.to_be_bytes())?;
        stream.write_all(&self.mile1.to_be_bytes())?;
        stream.write_all(&self.timestamp1.to_be_bytes())?;
        stream.write_all(&self.mile2.to_be_bytes())?;
        stream.write_all(&self.timestamp2.to_be_bytes())?;
        stream.write_all(&self.speed.to_be_bytes())?;

        stream.flush()?;
        Ok(())
    }
}

#[derive(Debug)]
enum InboundMessage {
    Plate { plate: String, timestamp: u32 },
    WantHeartbeat { interval: u32 },
    IAmCamera { road: u16, mile: u16, limit: u16 },
    IAmDispatcher { roads: Vec<u16> },
}

#[derive(Debug)]
enum ClientType {
    Camera,
    Dispatcher,
    Unknown,
}

#[derive(Debug)]
enum ClientInfo {
    CameraInfo { road: u16, mile: u16, limit: u16 },
    DispatcherInfo { roads: Vec<u16>, stream: TcpStream },
    Unknown,
}

#[derive(Debug)]
struct Sighting {
    client_id: Uuid,
    plate: String,
    timestamp: u32,
}

struct SightingDetails {
    road: u16,
    mile: u16,
    limit: u16,
    timestamp: u32,
}

#[derive(Debug)]
struct FlockState {
    client_registry: HashMap<Uuid, (ClientType, ClientInfo)>,
    traffic_log: Vec<Sighting>,
}

impl FlockState {
    fn new() -> Self {
        let client_registry = HashMap::new();
        let traffic_log = Vec::new();

        FlockState {
            client_registry,
            traffic_log,
        }
    }
}

fn send_error(stream: &mut TcpStream, msg: &str) -> std::io::Result<()> {
    stream.write_all(&[0x10])?;
    stream.write_all(&[msg.len() as u8])?;
    stream.write_all(msg.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn read_message(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<InboundMessage>> {
    let mut message_type = [0u8; 1];
    let bytes_read = reader.read(&mut message_type)?;

    if bytes_read == 0 {
        return Ok(None);
    }

    let message = match message_type[0] {
        0x20 => {
            let mut plate_len = [0u8; 1];
            reader.read_exact(&mut plate_len)?;

            let mut plate = vec![0u8; plate_len[0] as usize];
            reader.read_exact(&mut plate)?;

            let mut timestamp = [0u8; 4];
            reader.read_exact(&mut timestamp)?;

            InboundMessage::Plate {
                plate: String::from_utf8(plate).expect("Couldn't convert bytes to utf8"),
                timestamp: u32::from_be_bytes(timestamp),
            }
        }
        0x40 => {
            let mut interval = [0u8; 4];
            reader.read_exact(&mut interval)?;

            InboundMessage::WantHeartbeat {
                interval: u32::from_be_bytes(interval),
            }
        }
        0x80 => {
            let mut road = [0u8; 2];
            reader.read_exact(&mut road)?;

            let mut mile = [0u8; 2];
            reader.read_exact(&mut mile)?;

            let mut limit = [0u8; 2];
            reader.read_exact(&mut limit)?;

            InboundMessage::IAmCamera {
                road: u16::from_be_bytes(road),
                mile: u16::from_be_bytes(mile),
                limit: u16::from_be_bytes(limit),
            }
        }
        0x81 => {
            let mut numroads_buf = [0u8; 1];
            reader.read_exact(&mut numroads_buf)?;
            let numroads = u8::from_be_bytes(numroads_buf);

            let mut roads_buf = vec![0u8; (numroads as usize) * 2];
            reader.read_exact(&mut roads_buf)?;

            let roads: Vec<u16> = roads_buf
                .chunks_exact(2)
                .map(|chunk| {
                    let array: [u8; 2] = chunk.try_into().unwrap();
                    u16::from_be_bytes(array)
                })
                .collect();

            InboundMessage::IAmDispatcher { roads }
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unsupported message type",
            ));
        }
    };

    Ok(Some(message))
}

fn handle_message(
    writer: &mut TcpStream,
    message: InboundMessage,
    flock: &mut Arc<Mutex<FlockState>>,
    client_id: &Uuid,
) -> Result<(), std::io::Error> {
    match message {
        InboundMessage::WantHeartbeat { interval } => {
            let mut heartbeat_writer = writer.try_clone().expect("Couldn't clone writer");

            if interval == 0 {
                return Ok(());
            }

            thread::spawn(move || {
                loop {
                    let _ = heartbeat_writer.write(&[0x41]);
                    let wait_time = std::time::Duration::from_secs_f64(interval as f64 / 10.0);
                    thread::sleep(wait_time);
                }
            });
        }
        InboundMessage::IAmCamera { road, mile, limit } => {
            let client_registry = &mut flock
                .lock()
                .expect("Couldn't obtain lock on flock")
                .client_registry;

            let (client_type, _): &(ClientType, ClientInfo) = client_registry
                .get(client_id)
                .expect("Client should already exist in registry");

            match *client_type {
                ClientType::Unknown => {
                    let client_info = ClientInfo::CameraInfo { road, mile, limit };

                    client_registry.insert(*client_id, (ClientType::Camera, client_info));
                }
                _ => {
                    let msg = "Client already identified";
                    send_error(writer, msg)?;
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg));
                }
            }
        }
        InboundMessage::IAmDispatcher { roads } => {
            let client_registry = &mut flock
                .lock()
                .expect("Couldn't obtain lock on flock")
                .client_registry;

            if let Some((client_type, _)) = client_registry.get_mut(client_id) {
                match client_type {
                    ClientType::Unknown => {
                        *client_type = ClientType::Dispatcher;

                        let stream_clone = writer
                            .try_clone()
                            .expect("Failed to clone stream for storage");

                        let client_info = ClientInfo::DispatcherInfo {
                            roads,
                            stream: stream_clone,
                        };

                        client_registry.insert(*client_id, (ClientType::Dispatcher, client_info));
                    }
                    _ => {
                        let msg = "Client already identified";
                        send_error(writer, msg)?;
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg));
                    }
                }
            }
        }
        InboundMessage::Plate { plate, timestamp } => {
            let mut guard = flock.lock().expect("Couldn't obtain lock on flock");
            let state = &mut *guard;

            let traffic_log = &mut state.traffic_log;

            let client_registry = &mut state.client_registry;

            let (client_type, _): &(ClientType, ClientInfo) = client_registry
                .get(client_id)
                .expect("Client should already exist in registry");

            if !matches!(client_type, ClientType::Camera) {
                let msg = "Only cameras can send plates";
                send_error(writer, msg)?;
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg));
            }

            traffic_log.push(Sighting {
                client_id: *client_id,
                plate,
                timestamp,
            });
        }
    }

    Ok(())
}

fn handle_client(stream: TcpStream, flock: &mut Arc<Mutex<FlockState>>) {
    let mut writer = stream.try_clone().expect("Failed to clone stream");
    let mut reader = BufReader::new(stream);

    let client_id = Uuid::new_v4();
    flock
        .lock()
        .expect("Couldn't obtain lock on flock")
        .client_registry
        .insert(client_id, (ClientType::Unknown, ClientInfo::Unknown));

    loop {
        match read_message(&mut reader) {
            Ok(Some(message)) => {
                println!("{:?}", message);
                if let Err(e) = handle_message(&mut writer, message, flock, &client_id) {
                    eprintln!("Failed to handle message: {}", e);
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("Client error: {}", e);

                if e.kind() == std::io::ErrorKind::InvalidData {
                    let _ = send_error(&mut writer, "Illegal message type");
                }

                break;
            }
        }
    }

    let mut guard = flock.lock().expect("Couldn't obtain lock on flock");
    let should_remove = !matches!(
        guard.client_registry.get(&client_id),
        Some((ClientType::Camera, _))
    );

    if should_remove {
        guard.client_registry.remove(&client_id);
    }
}

fn check_traffic_log(
    client_registry: &HashMap<Uuid, (ClientType, ClientInfo)>,
    traffic_log: &Vec<Sighting>,
) -> Vec<Ticket> {
    let mut candidates = Vec::new();

    let mut sightings_by_plate: HashMap<String, Vec<SightingDetails>> = HashMap::new();

    for sighting in traffic_log {
        if let Some((ClientType::Camera, ClientInfo::CameraInfo { road, mile, limit })) =
            client_registry.get(&sighting.client_id)
        {
            sightings_by_plate
                .entry(sighting.plate.clone())
                .or_default()
                .push(SightingDetails {
                    road: *road,
                    mile: *mile,
                    limit: *limit,
                    timestamp: sighting.timestamp,
                });
        }
    }

    for (plate, mut sightings) in sightings_by_plate {
        sightings.sort_by_key(|s| s.timestamp);

        for pair in sightings.windows(2) {
            let s1 = &pair[0];
            let s2 = &pair[1];

            if s1.road != s2.road {
                continue;
            }

            let time_delta = s2.timestamp - s1.timestamp;
            let distance = s1.mile.abs_diff(s2.mile);

            if distance == 0 || time_delta == 0 {
                continue;
            }

            let speed_mpg = (distance as f64 / time_delta as f64) * 3600.0;
            let speed_100x = (speed_mpg * 100.0) as u16;
            let limit_100x = s1.limit * 100;

            if speed_100x > limit_100x {
                candidates.push(Ticket {
                    plate: plate.clone(),
                    road: s1.road,
                    mile1: s1.mile,
                    timestamp1: s1.timestamp,
                    mile2: s2.mile,
                    timestamp2: s2.timestamp,
                    speed: speed_100x,
                });
            }
        }
    }

    candidates
}

fn main() {
    let listener = TcpListener::bind(LOCAL_ADDR).unwrap();

    let flock = Arc::new(Mutex::new(FlockState::new()));

    let dispatcher_flock = flock.clone();
    thread::spawn(move || {
        let mut tickets: HashSet<Ticket> = HashSet::new();
        let mut issued_days: HashSet<(String, u32)> = HashSet::new();

        loop {
            let new_tickets: Vec<Ticket> = {
                let guard = dispatcher_flock
                    .lock()
                    .expect("Couldn't obtain lock on flock");

                check_traffic_log(&guard.client_registry, &guard.traffic_log)
            };

            if !new_tickets.is_empty() {
                for t in new_tickets {
                    if tickets.contains(&t) {
                        continue;
                    }

                    let day1 = t.timestamp1 / 86400;
                    let day2 = t.timestamp2 / 86400;

                    if issued_days.contains(&(t.plate.clone(), day1))
                        || issued_days.contains(&(t.plate.clone(), day2))
                    {
                        continue;
                    }

                    let stream_to_write = {
                        let guard = dispatcher_flock
                            .lock()
                            .expect("Couldn't obtain lock on flock");

                        let dispatcher_entry =
                            guard.client_registry.values().find(|&(_, client_info)| {
                                if let ClientInfo::DispatcherInfo { roads, .. } = client_info {
                                    return roads.contains(&t.road);
                                }
                                false
                            });

                        if let Some((_, ClientInfo::DispatcherInfo { stream, .. })) =
                            dispatcher_entry
                        {
                            Some(
                                stream
                                    .try_clone()
                                    .expect("Failed to clone dispatcher stream"),
                            )
                        } else {
                            None
                        }
                    };

                    if let Some(mut stream) = stream_to_write
                        && t.clone().write(&mut stream).is_ok()
                    {
                        tickets.insert(t.clone());

                        issued_days.insert((t.plate.clone(), day1));
                        issued_days.insert((t.plate.clone(), day2));
                    };
                }
            }

            let wait_time = std::time::Duration::from_millis(100);
            thread::sleep(wait_time);
        }
    });

    for stream in listener.incoming() {
        let mut flock_clone = flock.clone();
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle_client(stream, &mut flock_clone));
            }
            Err(e) => eprintln!("Failed to listen to client: {}", e),
        }
    }
}
