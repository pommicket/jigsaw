#![allow(clippy::too_many_arguments)]

use futures_util::{SinkExt, StreamExt};
use rand::seq::SliceRandom;
use rand::Rng;
use safe_transmute::{transmute_many_pedantic, transmute_to_bytes};
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
const MAX_PIECES: usize = 1000;

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

struct PieceInfo {
	positions: Vec<f32>,
	connectivity: Vec<i16>,
}

struct PuzzleInfo {
	width: u8,
	height: u8,
	url: String,
	nib_types: Vec<i16>,
	piece_info: PieceInfo,
}

impl Server {
	async fn create_table_if_not_exists(&self) -> Result<()> {
		if self
			.database
			.query("SELECT FROM puzzles", &[])
			.await
			.is_err()
		{
			self.database
				.execute(
					&format!(
						"CREATE TABLE puzzles (
				id char({PUZZLE_ID_LEN}) PRIMARY KEY,
				url varchar(256),
				width int4,
				height int4,
				create_time timestamp DEFAULT CURRENT_TIMESTAMP,
				nib_types int2[],
				connectivity int2[],
				positions float4[]
			)"
					),
					&[],
				)
				.await?;
		}
		Ok(())
	}
	async fn try_register_id(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<bool> {
		let id = std::str::from_utf8(&id)?;
		Ok(self
			.database
			.execute("INSERT INTO puzzles (id) VALUES ($1)", &[&id])
			.await
			.is_ok())
	}
	async fn set_puzzle_data(
		&self,
		id: [u8; PUZZLE_ID_LEN],
		width: u8,
		height: u8,
		url: &str,
		nib_types: Vec<u16>,
		piece_positions: Vec<f32>,
		connectivity: Vec<u16>,
	) -> Result<()> {
		let id = std::str::from_utf8(&id)?;
		let width = i32::from(width);
		let height = i32::from(height);
		// transmuting u16 to i16 should never give an error. they have the same alignment.
		let nib_types: &[i16] =
			transmute_many_pedantic(transmute_to_bytes(&nib_types[..])).unwrap();
		let connectivity: &[i16] =
			transmute_many_pedantic(transmute_to_bytes(&connectivity[..])).unwrap();
		let positions = &piece_positions;
		self.database
			.execute(
				"UPDATE puzzles SET width = $1, height = $2, url = $3, nib_types = $4,
					    connectivity = $5, positions = $6 WHERE id = $7",
				&[
					&width,
					&height,
					&url,
					&nib_types,
					&connectivity,
					&positions,
					&id,
				],
			)
			.await?;
		Ok(())
	}
	async fn move_piece(
		&self,
		id: [u8; PUZZLE_ID_LEN],
		piece: usize,
		x: f32,
		y: f32,
	) -> Result<()> {
		let id = std::str::from_utf8(&id)?;
		if piece > MAX_PIECES {
			return Err(Error::BadPieceID);
		}
		let piece = piece as i32;
		// NOTE: postgresql arrays start at index 1!
		let i0 = piece * 2 + 1;
		let i1 = piece * 2 + 2;
		self.database.execute(
			"UPDATE puzzles SET positions[$1] = $2, positions[$3] = $4 WHERE id = $5 AND $6 < width * height",
			// the $6 < width * height protects against OOB access!
			&[&i0, &x, &i1, &y, &id, &piece]
		).await?;
		Ok(())
	}
	async fn connect_pieces(
		&self,
		id: [u8; PUZZLE_ID_LEN],
		piece1: usize,
		piece2: usize,
	) -> Result<()> {
		let id = std::str::from_utf8(&id)?;
		// NOTE: postgresql arrays start at index 1!
		let piece1 = piece1 as i32 + 1;
		let piece2 = piece2 as i32 + 1;
		self.database.execute(
			"UPDATE puzzles SET connectivity = array_replace(connectivity, connectivity[$1], connectivity[$2]) WHERE id = $3 AND $4 < width * height AND $5 < width * height",
			// the $6 < width * height protects against OOB access!
			&[&piece1, &piece2, &id, &piece1, &piece2]
		).await?;
		Ok(())
	}
	async fn get_piece_info(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<PieceInfo> {
		let id = std::str::from_utf8(&id)?;
		let rows = self
			.database
			.query(
				"SELECT positions, connectivity FROM puzzles WHERE id = $1",
				&[&id],
			)
			.await?;
		let row = &rows[0];
		let positions: Vec<f32> = row.try_get(0)?;
		let connectivity: Vec<i16> = row.try_get(1)?;
		Ok(PieceInfo {
			positions,
			connectivity,
		})
	}
	async fn get_puzzle_info(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<PuzzleInfo> {
		let id = std::str::from_utf8(&id)?;
		let rows = self.database.query(
			"SELECT width, height, url, positions, nib_types, connectivity FROM puzzles WHERE id = $1",
			&[&id]
		).await?;
		let row = &rows[0];
		let width: i32 = row.try_get(0)?;
		let height: i32 = row.try_get(1)?;
		let url: String = row.try_get(2)?;
		let positions: Vec<f32> = row.try_get(3)?;
		let nib_types: Vec<i16> = row.try_get(4)?;
		let connectivity: Vec<i16> = row.try_get(5)?;
		Ok(PuzzleInfo {
			width: width as u8,
			height: height as u8,
			url,
			nib_types,
			piece_info: PieceInfo {
				positions,
				connectivity,
			},
		})
	}
	async fn sweep(&self) -> Result<()> {
		self.database
			.execute(
				"DELETE FROM puzzles WHERE create_time < current_timestamp - interval '1 week'",
				&[],
			)
			.await?;
		Ok(())
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
	let mut data = vec![1, 0, 0, 0, 0, 0, 0, 0]; // opcode / version number and padding
	let PuzzleInfo {
		width,
		height,
		url,
		nib_types,
		piece_info: PieceInfo {
			positions,
			connectivity,
		},
	} = server.get_puzzle_info(id).await?;
	data.push(width);
	data.push(height);
	data.extend(transmute_to_bytes(&nib_types[..]));
	data.extend(url.as_bytes());
	data.push(0);
	while data.len() % 8 != 0 {
		// padding
		data.push(0);
	}
	data.extend_from_slice(transmute_to_bytes(&positions[..]));
	data.extend_from_slice(transmute_to_bytes(&connectivity[..]));
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
				if usize::from(width) * usize::from(height) > MAX_PIECES {
					return Err(Error::TooManyPieces);
				}
				let nib_count =
					2 * (width as usize) * (height as usize) - (width as usize) - (height as usize);
				let mut nib_types: Vec<u16> = Vec::with_capacity(nib_count);
				let mut piece_positions: Vec<f32> =
					Vec::with_capacity((width as usize) * (height as usize) * 2);
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
				server
					.set_puzzle_data(
						id,
						width,
						height,
						url,
						nib_types,
						piece_positions,
						connectivity_data,
					)
					.await?;
				server.player_counts.lock().await.insert(id, 1);
				*puzzle_id = Some(id);
				ws.send(Message::Text(format!(
					"id: {}",
					std::str::from_utf8(&id)?
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
					server.move_piece(puzzle_id, piece, x, y).await?;
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
				server.connect_pieces(puzzle_id, piece1, piece2).await?;
			} else if text == "poll" {
				let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
				let mut data = vec![2, 0, 0, 0, 0, 0, 0, 0]; // opcode / version number + padding
				let PieceInfo {
					positions,
					connectivity,
				} = server.get_piece_info(puzzle_id).await?;
				data.extend_from_slice(transmute_to_bytes(&positions[..]));
				data.extend_from_slice(transmute_to_bytes(&connectivity[..]));
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
		let (client, connection) = tokio_postgres::connect(
			"host=/var/run/postgresql dbname=jigsaw",
			tokio_postgres::NoTls,
		)
		.await
		.expect("Couldn't connect to database");

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
	server_arc
		.create_table_if_not_exists()
		.await
		.expect("error creating table");
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
			if let Err(e) = server.sweep().await {
				eprintln!("error sweeping DB: {e}");
			}
			tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
		}
	});
	println!("Server initialized! Waiting for connections...");
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
