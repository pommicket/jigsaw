#![allow(dead_code)] // TODO :  delete me
#![allow(unused_variables)] // TODO :  delete me

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
use zerocopy::AsBytes;

const PUZZLE_ID_CHARSET: &[u8] = b"23456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
const PUZZLE_ID_LEN: usize = 7;
const MAX_PLAYERS: u16 = 20;

fn generate_puzzle_id() -> [u8; PUZZLE_ID_LEN] {
	let mut rng = rand::thread_rng();
	[(); 7].map(|()| *PUZZLE_ID_CHARSET.choose(&mut rng).unwrap())
}

#[derive(Debug)]
struct Server {
	// keep this in memory, since we want to reset it to 0 when the server restarts
	player_counts: Mutex<HashMap<[u8; PUZZLE_ID_LEN], u16>>,
	wikimedia_featured: Vec<String>,
	wikimedia_potd: RwLock<String>,
	database: tokio_postgres::Client,
}


impl Server {
	async fn create_table_if_not_exists(&self) -> Result<()> {
		todo!()
	}
	async fn try_register_id(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<bool> {
		todo!()
	}
	async fn set_puzzle_data(&self, id: [u8; PUZZLE_ID_LEN], width: u8, height: u8, url: &str, nib_types: Vec<u16>, piece_positions: Vec<f32>, connectivity_data: Vec<u16>) -> Result<()> {
		todo!()
	}
	async fn move_piece(&self, piece: usize, x: f32, y: f32) -> Result<()> {
		todo!()
	}
	async fn connect_pieces(&self, piece1: usize, piece2: usize) -> Result<()> {
		todo!()
	}
	async fn get_connectivity(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<Vec<u16>> {
		todo!()
	}
	async fn get_positions(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<Vec<f32>> {
		todo!()
	}
	async fn get_details(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<(u8, u8, String)> {
		todo!()
	}
	async fn sweep(&self) -> Result<()> {
		todo!()
	}
}

#[derive(Debug)]
enum Error {
	Tungstenite(tungstenite::Error),
	Postgres(tokio_postgres::Error),
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
			Error::Postgres(e) => write!(f, "{e}"),
			Error::IO(e) => write!(f, "{e}"),
			Error::UTF8(e) => write!(f, "{e}"),
			Error::Tungstenite(e) => write!(f, "{e}"),
		}
	}
}

impl From<tokio_postgres::Error> for Error {
	fn from(value: tokio_postgres::Error) -> Self {
		Self::Postgres(value)
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

async fn get_puzzle_info(server: &Server, id: &[u8]) -> Result<Vec<u8>> {
	let id: [u8; PUZZLE_ID_LEN] = id.try_into().map_err(|_| Error::BadPuzzleID)?;
	let mut data = vec![1];
	let (width, height, url) = server.get_details(id).await?;
	data.push(width);
	data.push(height);
	data.extend(url.as_bytes());
	while data.len() % 8 != 0 {
		// padding
		data.push(0);
	}
	let pieces = server.get_positions(id).await?;
	data.extend_from_slice(pieces.as_bytes());
	let connectivity = server.get_connectivity(id).await?;
	data.extend_from_slice(connectivity.as_bytes());
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
				let nib_count = 2 * (width as usize) * (height as usize) - (width as usize) - (height as usize);
				let mut nib_types: Vec<u16> = Vec::with_capacity(nib_count);
				let mut piece_positions: Vec<f32> = Vec::with_capacity((width as usize) * (height as usize) * 2);
				{
					let mut rng = rand::thread_rng();
					// pick nib types
					for _ in 0..nib_count {
						nib_types.push(rng.gen());
					}
					// pick piece positions
					for y in 0..(height as u16) {
						for x in 0..(width as u16) {
							let dx: f32 = rng.gen_range(0.0..0.5);
							let dy: f32 = rng.gen_range(0.0..0.5);
							piece_positions.push((x as f32 + dx) / ((width + 1) as f32));
							piece_positions.push((y as f32 + dy) / ((height + 1) as f32));
						}
					}
					piece_positions.shuffle(&mut rng);
				}
				let mut connectivity_data: Vec<u16> =
					Vec::with_capacity((width as usize) * (height as usize));
				for i in 0..(width as u16) * (height as u16) {
					connectivity_data.push(i);
				}
				let mut id;
				loop {
					id = generate_puzzle_id();
					if server.try_register_id(id).await? {
						break;
					}
				}
				server.set_puzzle_data(id, width, height, url, nib_types, piece_positions, connectivity_data).await?;
				server.player_counts.lock().await.insert(id, 1);
				ws.send(Message::Text(format!(
					"id: {}",
					std::str::from_utf8(&id).expect("puzzle ID has bad utf-8???")
				)))
				.await?;
				let info = get_puzzle_info(server, &id).await?;
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
				let info = get_puzzle_info(server, &id).await?;
				ws.send(Message::Binary(info)).await?;
			} else if text.starts_with("move ") {
				let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
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
					server.move_piece(piece, x, y).await?;
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
				server.connect_pieces(piece1, piece2).await?;
			} else if text == "poll" {
				let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
				let mut data = vec![2, 0, 0, 0, 0, 0, 0, 0]; // opcode / version number + padding
				data.extend_from_slice(server.get_positions(puzzle_id).await?.as_bytes());
				data.extend_from_slice(server.get_connectivity(puzzle_id).await?.as_bytes());
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
		let potd = get_potd().await;
		let (client, connection) = tokio_postgres::connect("host=/var/run/postgresql dbname=jigsaw", tokio_postgres::NoTls).await.expect("Couldn't connect to database");
		
		// docs say: "The connection object performs the actual communication with the database, so spawn it off to run on its own."
		tokio::spawn(async move {
			if let Err(e) = connection.await {
				eprintln!("connection error: {}", e);
			}
		});
		Server {
			player_counts: Mutex::new(HashMap::new()),
			database: client,
			wikimedia_potd: RwLock::new(potd),
			wikimedia_featured,
		}
	});
	server_arc.create_table_if_not_exists().await.expect("error creating table");
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
			if let Err(e) = server.sweep().await {
				eprintln!("error sweeping DB: {e}");
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
