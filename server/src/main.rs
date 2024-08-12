use futures_util::{SinkExt, StreamExt};
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashMap;
use std::io::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tungstenite::protocol::Message;

const PUZZLE_ID_CHARSET: &[u8] = b"23456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
const PUZZLE_ID_LEN: usize = 7;
const MAX_PLAYERS: u16 = 20;

fn generate_puzzle_id() -> [u8; PUZZLE_ID_LEN] {
	let mut rng = rand::thread_rng();
	[(); 7].map(|()| *PUZZLE_ID_CHARSET.choose(&mut rng).unwrap())
}

#[derive(Debug)]
struct Server {
	puzzles: sled::Tree,
	pieces: sled::Tree,
	connectivity: sled::Tree,
	// keep this in memory, since we want to reset it to 0 when the server restarts
	player_counts: Mutex<HashMap<[u8; PUZZLE_ID_LEN], u16>>,
	wikimedia_featured: Vec<String>,
	wikimedia_potd: RwLock<String>,
}

#[derive(Debug)]
enum Error {
	Tungstenite(tungstenite::Error),
	Sled(sled::Error),
	IO(std::io::Error),
	UTF8(std::str::Utf8Error),
	BadPuzzleID,
	BadPieceID,
	BadSyntax,
	ImageURLTooLong,
	TooManyPieces,
	TooManyPlayers,
	NotJoined,
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Error::BadPieceID => write!(f, "bad piece ID"),
			Error::BadPuzzleID => write!(f, "bad puzzle ID"),
			Error::BadSyntax => write!(f, "bad syntax"),
			Error::ImageURLTooLong => write!(f, "image URL too long"),
			Error::TooManyPieces => write!(f, "too many pieces"),
			Error::NotJoined => write!(f, "haven't joined a puzzle"),
			Error::TooManyPlayers => write!(f, "too many players"),
			Error::Sled(e) => write!(f, "{e}"),
			Error::IO(e) => write!(f, "{e}"),
			Error::UTF8(e) => write!(f, "{e}"),
			Error::Tungstenite(e) => write!(f, "{e}"),
		}
	}
}

impl From<sled::Error> for Error {
	fn from(value: sled::Error) -> Self {
		Self::Sled(value)
	}
}

impl From<tungstenite::Error> for Error {
	fn from(value: tungstenite::Error) -> Self {
		Self::Tungstenite(value)
	}
}
impl From<std::io::Error> for Error {
	fn from(value: std::io::Error) -> Self {
		Self::IO(value)
	}
}
impl From<std::str::Utf8Error> for Error {
	fn from(value: std::str::Utf8Error) -> Self {
		Self::UTF8(value)
	}
}

type Result<T> = std::result::Result<T, Error>;

fn get_puzzle_info(server: &Server, id: &[u8]) -> Result<Vec<u8>> {
	if id.len() != PUZZLE_ID_LEN {
		return Err(Error::BadPuzzleID);
	}
	let mut data = vec![1, 0, 0, 0, 0, 0, 0, 0]; // opcode + padding
	let puzzle = server.puzzles.get(id)?.ok_or(Error::BadPuzzleID)?;
	data.extend_from_slice(&puzzle);
	while data.len() % 8 != 0 {
		// padding
		data.push(0);
	}
	let pieces = server.pieces.get(id)?.ok_or(Error::BadPuzzleID)?;
	data.extend_from_slice(&pieces);
	let connectivity = server.connectivity.get(id)?.ok_or(Error::BadPuzzleID)?;
	data.extend_from_slice(&connectivity);
	Ok(data)
}

async fn handle_websocket(
	server: &Server,
	puzzle_id: &mut Option<[u8; PUZZLE_ID_LEN]>,
	ws: &mut tokio_tungstenite::WebSocketStream<&mut tokio::net::TcpStream>,
) -> Result<()> {
	while let Some(message) = ws.next().await {
		let message = message?;
		if matches!(message, Message::Close(_)) {
			break;
		}
		if let Message::Text(text) = &message {
			let text = text.trim();
			if let Some(dimensions) = text.strip_prefix("new ") {
				let mut parts = dimensions.split(' ');
				let width: u8 = parts
					.next()
					.ok_or(Error::BadSyntax)?
					.parse()
					.map_err(|_| Error::BadSyntax)?;
				let height: u8 = parts
					.next()
					.ok_or(Error::BadSyntax)?
					.parse()
					.map_err(|_| Error::BadSyntax)?;
				let url: &str = parts.next().ok_or(Error::BadSyntax)?;
				if url.len() > 255 {
					return Err(Error::ImageURLTooLong);
				}
				if (width as u16) * (height as u16) > 1000 {
					return Err(Error::TooManyPieces);
				}
				let mut puzzle_data = vec![width, height];
				let timestamp: u64 = SystemTime::now()
					.duration_since(SystemTime::UNIX_EPOCH)
					.expect("time went backwards :/")
					.as_secs();
				for byte in timestamp.to_le_bytes() {
					puzzle_data.push(byte);
				}
				// pick nib types
				{
					let mut rng = rand::thread_rng();
					for _ in 0..2u16 * (width as u16) * (height as u16)
						- (width as u16) - (height as u16)
					{
						puzzle_data.push(rng.gen());
						puzzle_data.push(rng.gen());
					}
				}
				// URL
				puzzle_data.extend(url.as_bytes());
				puzzle_data.push(0);
				// puzzle ID
				let mut id;
				loop {
					id = generate_puzzle_id();
					let data = std::mem::take(&mut puzzle_data);
					if server
						.puzzles
						.compare_and_swap(id, None::<&'static [u8; 0]>, Some(&data[..]))?
						.is_ok()
					{
						break;
					}
				}
				drop(puzzle_data); // should be empty now
				*puzzle_id = Some(id);
				let pieces_data: Box<[u8]>;
				{
					let mut rng = rand::thread_rng();
					let mut positions = vec![];
					positions.reserve_exact((width as usize) * (height as usize));
					// positions
					for y in 0..(height as u16) {
						for x in 0..(width as u16) {
							let dx: f32 = rng.gen_range(0.0..0.5);
							let dy: f32 = rng.gen_range(0.0..0.5);
							positions.push([
								(x as f32 + dx) / ((width + 1) as f32),
								(y as f32 + dy) / ((height + 1) as f32),
							]);
						}
					}
					positions.shuffle(&mut rng);
					// rust isn't smart enough to do the zero-copy with f32::to_le_bytes and Vec::into_flattened
					let ptr: *mut [[f32; 2]] = Box::into_raw(positions.into_boxed_slice());
					let ptr: *mut [u8] = std::ptr::slice_from_raw_parts_mut(
						ptr.cast(),
						(width as usize) * (height as usize) * 8,
					);
					// evil unsafe code >:3
					pieces_data = unsafe { Box::from_raw(ptr) };
				}
				server.pieces.insert(id, pieces_data)?;
				let mut connectivity_data =
					Vec::with_capacity((width as usize) * (height as usize) * 2);
				for i in 0..(width as u16) * (height as u16) {
					connectivity_data.extend(i.to_le_bytes());
				}
				server.connectivity.insert(id, connectivity_data)?;
				server.player_counts.lock().await.insert(id, 1);
				ws.send(Message::Text(format!(
					"id: {}",
					std::str::from_utf8(&id).expect("puzzle ID has bad utf-8???")
				)))
				.await?;
				let info = get_puzzle_info(server, &id)?;
				ws.send(Message::Binary(info)).await?;
			} else if let Some(id) = text.strip_prefix("join ") {
				let id = id.as_bytes().try_into().map_err(|_| Error::BadSyntax)?;
				let mut player_counts = server.player_counts.lock().await;
				let entry = player_counts.entry(id).or_default();
				if *entry >= MAX_PLAYERS {
					return Err(Error::TooManyPlayers);
				}
				*entry += 1;
				drop(player_counts); // release lock
				*puzzle_id = Some(id);
				let info = get_puzzle_info(server, &id)?;
				ws.send(Message::Binary(info)).await?;
			} else if text.starts_with("move ") {
				let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
				#[derive(Clone, Copy)]
				struct Motion {
					piece: usize,
					x: f32,
					y: f32,
				}
				let mut motions = vec![];
				for line in text.split('\n') {
					let mut parts = line.split(' ');
					parts.next(); // skip "move"
					let piece: usize = parts
						.next()
						.ok_or(Error::BadSyntax)?
						.parse()
						.map_err(|_| Error::BadSyntax)?;
					let x: f32 = parts
						.next()
						.ok_or(Error::BadSyntax)?
						.parse()
						.map_err(|_| Error::BadSyntax)?;
					let y: f32 = parts
						.next()
						.ok_or(Error::BadSyntax)?
						.parse()
						.map_err(|_| Error::BadSyntax)?;
					motions.push(Motion { piece, x, y });
				}
				let mut error = None;
				server
					.pieces
					.fetch_and_update(puzzle_id, |curr_pieces: Option<&[u8]>| {
						let Some(curr_pieces) = curr_pieces else {
							error = Some(Error::BadPuzzleID);
							return None;
						};
						let mut new_pieces = curr_pieces.to_vec();
						for Motion { piece, x, y } in motions.iter().copied() {
							let Some(slice) = new_pieces.get_mut(8 * piece..8 * piece + 8) else {
								error = Some(Error::BadPieceID);
								break;
							};
							slice[0..4].copy_from_slice(&x.to_le_bytes());
							slice[4..8].copy_from_slice(&y.to_le_bytes());
						}
						Some(new_pieces)
					})?;
				if let Some(error) = error {
					return Err(error);
				}
				ws.send(Message::Text("ack".to_string())).await?;
			} else if let Some(data) = text.strip_prefix("connect ") {
				let mut parts = data.split(' ');
				let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
				let piece1: usize = parts
					.next()
					.ok_or(Error::BadSyntax)?
					.parse()
					.map_err(|_| Error::BadSyntax)?;
				let piece2: usize = parts
					.next()
					.ok_or(Error::BadSyntax)?
					.parse()
					.map_err(|_| Error::BadSyntax)?;
				let mut error = None;
				server
					.connectivity
					.fetch_and_update(puzzle_id, |curr_connectivity| {
						let Some(curr_connectivity) = curr_connectivity else {
							error = Some(Error::BadPuzzleID);
							return None;
						};
						let mut new_connectivity = curr_connectivity.to_vec();
						if piece1 >= curr_connectivity.len() / 2
							|| piece2 >= curr_connectivity.len() / 2
						{
							error = Some(Error::BadPieceID);
							return Some(new_connectivity);
						}
						let piece2_group = u16::from_le_bytes([
							curr_connectivity[piece2 * 2],
							curr_connectivity[piece2 * 2 + 1],
						]);
						let a = curr_connectivity[piece1 * 2];
						let b = curr_connectivity[piece1 * 2 + 1];
						for piece in 0..curr_connectivity.len() / 2 {
							let piece_group = u16::from_le_bytes([
								curr_connectivity[piece * 2],
								curr_connectivity[piece * 2 + 1],
							]);
							if piece_group == piece2_group {
								new_connectivity[piece * 2] = a;
								new_connectivity[piece * 2 + 1] = b;
							}
						}
						Some(new_connectivity)
					})?;
				if let Some(error) = error {
					return Err(error);
				}
			} else if text == "poll" {
				let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
				let pieces = server.pieces.get(puzzle_id)?.ok_or(Error::BadPuzzleID)?;
				let connectivity = server
					.connectivity
					.get(puzzle_id)?
					.ok_or(Error::BadPuzzleID)?;
				let mut data = vec![2, 0, 0, 0, 0, 0, 0, 0]; // opcode / version number + padding
				data.extend_from_slice(&pieces);
				data.extend_from_slice(&connectivity);
				ws.send(Message::Binary(data)).await?;
			} else if text == "randomFeaturedWikimedia" {
				let choice = rand::thread_rng().gen_range(0..server.wikimedia_featured.len());
				ws.send(Message::Text(format!(
					"useImage {}",
					server.wikimedia_featured[choice]
				)))
				.await?;
			} else if text == "wikimediaPotd" {
				ws.send(Message::Text(format!(
					"useImage {}",
					server.wikimedia_potd.read().await
				)))
				.await?;
			}
		}
	}
	Ok(())
}

async fn handle_connection(server: &Server, conn: &mut tokio::net::TcpStream) -> Result<()> {
	let mut puzzle_id = None;
	let mut ws = tokio_tungstenite::accept_async_with_config(
		conn,
		Some(tungstenite::protocol::WebSocketConfig {
			max_message_size: Some(65536),
			max_frame_size: Some(65536),
			..Default::default()
		}),
	)
	.await?;
	let status = handle_websocket(server, &mut puzzle_id, &mut ws).await;
	if let Err(e) = &status {
		ws.send(Message::Text(format!("error {e}"))).await?;
	};
	if let Some(puzzle_id) = puzzle_id {
		*server
			.player_counts
			.lock()
			.await
			.entry(puzzle_id)
			.or_insert_with(|| {
				eprintln!("negative player count??");
				// prevent underflow
				1
			}) -= 1;
	}
	status
}

fn read_to_lines(path: &str) -> std::io::Result<Vec<String>> {
	let file = std::fs::File::open(path)?;
	let reader = std::io::BufReader::new(file);
	reader.lines().collect()
}

async fn try_get_potd() -> Result<String> {
	let output = tokio::process::Command::new("python3")
		.arg("potd.py")
		.output()
		.await?;
	Ok(String::from_utf8(output.stdout)
		.map_err(|e| e.utf8_error())?
		.trim()
		.to_string())
}
async fn get_potd() -> String {
	match try_get_potd().await {
		Ok(s) => s,
		Err(e) => {
			eprintln!("couldn't get potd: {e}");
			String::new()
		}
	}
}

#[tokio::main]
async fn main() {
	let port = 54472;
	let host_addr = SocketAddr::from(([127, 0, 0, 1], port));
	let listener = match tokio::net::TcpListener::bind(host_addr).await {
		Ok(l) => l,
		Err(e) => {
			eprintln!("Couldn't bind to localhost:{port}: {e}");
			return;
		}
	};
	let start_time = SystemTime::now();
	let server_arc: Arc<Server> = Arc::new({
		let wikimedia_featured =
			read_to_lines("featuredpictures.txt").expect("Couldn't read featuredpictures.txt");
		let db = sled::open("database.sled").expect("error opening database");
		let puzzles = db.open_tree("PUZZLES").expect("error opening puzzles tree");
		let pieces = db.open_tree("PIECES").expect("error opening pieces tree");
		let connectivity = db
			.open_tree("CONNECTIVITY")
			.expect("error opening connectivity tree");
		let potd = get_potd().await;
		Server {
			puzzles,
			pieces,
			player_counts: Mutex::new(HashMap::new()),
			connectivity,
			wikimedia_potd: RwLock::new(potd),
			wikimedia_featured,
		}
	});
	let server_arc_clone = server_arc.clone();
	tokio::task::spawn(async move {
		let server: &Server = server_arc_clone.as_ref();
		fn next_day(t: SystemTime) -> SystemTime {
			let day = 60 * 60 * 24;
			let dt = t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
			SystemTime::UNIX_EPOCH + Duration::from_secs((dt + day - 1) / day * day)
		}
		let mut last_time = start_time;
		loop {
			let time_to_sleep = next_day(last_time).duration_since(last_time).unwrap();
			tokio::time::sleep(time_to_sleep).await;
			let potd = get_potd().await;
			*server.wikimedia_potd.write().await = potd;
			last_time = SystemTime::now();
		}
	});
	let server_arc_clone = server_arc.clone();
	tokio::task::spawn(async move {
		let server: &Server = server_arc_clone.as_ref();
		loop {
			// TODO : sweep
			let now = SystemTime::now();
			let mut to_delete = vec![];
			for item in server.puzzles.iter() {
				let (key, value) = item.expect("sweep failed to read database");
				let timestamp: [u8; 8] = value[2..2 + 8].try_into().unwrap();
				let timestamp =
					SystemTime::UNIX_EPOCH + Duration::from_secs(u64::from_le_bytes(timestamp));
				if now.duration_since(timestamp).unwrap_or_default()
					>= Duration::from_secs(60 * 60 * 24 * 7)
				{
					// delete puzzles created at least 1 week ago
					to_delete.push(key);
				}
			}
			for key in to_delete {
				// technically there is a race condition here but stop being silly
				server
					.puzzles
					.remove(&key)
					.expect("sweep failed to delete puzzle");
				server
					.pieces
					.remove(&key)
					.expect("sweep failed to delete pieces");
				server
					.connectivity
					.remove(&key)
					.expect("sweep failed to delete connectivity");
				if let Some(key) = <[u8; PUZZLE_ID_LEN]>::try_from(&key[..]).ok() {
					server.player_counts.lock().await.remove(&key);
				}
			}
			tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
		}
	});
	loop {
		let (mut stream, addr) = match listener.accept().await {
			Ok(result) => result,
			Err(e) => {
				eprintln!("Error accepting connection: {e}");
				continue;
			}
		};
		let server_arc_clone = server_arc.clone();
		tokio::task::spawn(async move {
			let server: &Server = server_arc_clone.as_ref();
			match handle_connection(server, &mut stream).await {
				Ok(()) => {}
				Err(e) => {
					eprintln!("Error handling connection to {addr}: {e}");
				}
			}
			let _ = stream.shutdown().await;
		});
	}
}
