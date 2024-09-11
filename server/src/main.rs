#![allow(clippy::too_many_arguments)]
#![allow(clippy::manual_range_contains)]

use futures_util::{SinkExt, StreamExt};
use rand::seq::SliceRandom;
use rand::Rng;
use safe_transmute::{transmute_many_pedantic, transmute_to_bytes};
use std::collections::HashMap;
use std::io::prelude::*;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tungstenite::protocol::Message;

const PORT: u16 = 54472;
const PUZZLE_ID_CHARSET: &[u8] = b"23456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
const PUZZLE_ID_LEN: usize = 7;
const MAX_PLAYERS: u32 = 20;
const MAX_PIECES: usize = 1000;
const ACTION_MOVE: u32 = 3;
const ACTION_CONNECT: u32 = 4;

fn generate_puzzle_id() -> [u8; PUZZLE_ID_LEN] {
	let mut rng = rand::thread_rng();
	[(); 7].map(|()| *PUZZLE_ID_CHARSET.choose(&mut rng).unwrap())
}

#[derive(Debug)]
struct Server {
	// keep this in memory, since we want to reset it to 0 when the server restarts
	player_counts: Mutex<HashMap<[u8; PUZZLE_ID_LEN], u32>>,
	wikimedia_featured: Vec<String>,
	wikimedia_potd: RwLock<String>,
	database: tokio_postgres::Client,
	create_puzzle: tokio_postgres::Statement,
	set_puzzle_data: tokio_postgres::Statement,
	move_piece: tokio_postgres::Statement,
	connect_pieces: tokio_postgres::Statement,
	get_piece_info: tokio_postgres::Statement,
	get_puzzle_info: tokio_postgres::Statement,
}

struct PieceInfo {
	positions: Vec<f32>,
	connectivity: Vec<i16>,
}

struct PuzzleInfo {
	width: u8,
	height: u8,
	url: String,
	seed: u32,
	piece_info: PieceInfo,
}

impl Server {
	async fn try_register_id(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<bool> {
		let id = std::str::from_utf8(&id)?;
		Ok(self
			.database
			.execute(&self.create_puzzle, &[&id])
			.await
			.is_ok())
	}
	async fn increase_player_count(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<()> {
		let mut player_counts = self.player_counts.lock().await;
		let entry = player_counts.entry(id).or_insert(0);
		if *entry >= MAX_PLAYERS {
			Err(Error::TooManyPlayers)
		} else {
			*entry += 1;
			Ok(())
		}
	}
	async fn decrease_player_count(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<()> {
		let mut player_counts = self.player_counts.lock().await;
		let std::collections::hash_map::Entry::Occupied(mut o) = player_counts.entry(id) else {
			return Err(Error::BadPuzzleID);
		};
		if *o.get() <= 1 {
			o.remove();
		} else {
			*o.get_mut() -= 1;
		}
		Ok(())
	}
	async fn set_puzzle_data(
		&self,
		id: [u8; PUZZLE_ID_LEN],
		width: u8,
		height: u8,
		url: &str,
		piece_positions: &[f32],
		connectivity: Vec<u16>,
		seed: u32,
	) -> Result<()> {
		let id = std::str::from_utf8(&id)?;
		let width = i32::from(width);
		let height = i32::from(height);
		let seed = seed as i32;
		// transmuting u16 to i16 should never give an error. they have the same alignment.
		let connectivity: &[i16] =
			transmute_many_pedantic(transmute_to_bytes(&connectivity[..])).unwrap();
		let positions = &piece_positions;
		self.database
			.execute(
				&self.set_puzzle_data,
				&[&width, &height, &url, &connectivity, &positions, &seed, &id],
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
		self.database
			.execute(&self.move_piece, &[&i0, &x, &i1, &y, &id, &piece])
			.await?;
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
		self.database
			.execute(&self.connect_pieces, &[&piece1, &piece2, &id])
			.await?;
		Ok(())
	}
	async fn get_piece_info(&self, id: [u8; PUZZLE_ID_LEN]) -> Result<PieceInfo> {
		let id = std::str::from_utf8(&id)?;
		let rows = self.database.query(&self.get_piece_info, &[&id]).await?;
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
		let rows = self.database.query(&self.get_puzzle_info, &[&id]).await?;
		let row = &rows[0];
		let width: i32 = row.try_get(0)?;
		let height: i32 = row.try_get(1)?;
		let url: String = row.try_get(2)?;
		let positions: Vec<f32> = row.try_get(3)?;
		let seed: i32 = row.try_get(4)?;
		let connectivity: Vec<i16> = row.try_get(5)?;
		Ok(PuzzleInfo {
			width: width as u8,
			height: height as u8,
			url,
			seed: seed as u32,
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
	BadSyntax(&'static str),
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
			Error::BadSyntax(s) => write!(f, "{s}"),
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
		seed,
		piece_info: PieceInfo {
			positions,
			connectivity,
		},
	} = server.get_puzzle_info(id).await?;
	data.extend(seed.to_le_bytes());
	data.push(width);
	data.push(height);
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
	let mut last_poll_hash = 0u64;
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
					.ok_or(Error::BadSyntax("no width"))?
					.parse()
					.map_err(|_| Error::BadSyntax("width not integer"))?;
				let height: u8 = parts
					.next()
					.ok_or(Error::BadSyntax("no height"))?
					.parse()
					.map_err(|_| Error::BadSyntax("height not integer"))?;
				if width < 3 || height < 3 {
					return Err(Error::BadSyntax("dimensions too small"));
				}
				if usize::from(width) * usize::from(height) > MAX_PIECES {
					return Err(Error::TooManyPieces);
				}
				let url: String = parts
					.next()
					.ok_or(Error::BadSyntax("no URL"))?
					.replace(';', " ");
				if url.len() > 2048 {
					return Err(Error::ImageURLTooLong);
				}
				let seed = parts
					.next()
					.ok_or(Error::BadSyntax("no seed"))?
					.parse()
					.map_err(|_| Error::BadSyntax("seed not integer"))?;
				let piece_positions = vec![0.0f32; 2 * (width as usize) * (height as usize)];
				let mut connectivity_data: Vec<u16> =
					Vec::with_capacity(usize::from(width) * usize::from(height));
				for i in 0..u16::from(width) * u16::from(height) {
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
						&url,
						&piece_positions,
						connectivity_data,
						seed,
					)
					.await?;
				server.player_counts.lock().await.insert(id, 1);
				*puzzle_id = Some(id);
				ws.send(Message::Text(format!("id: {}", std::str::from_utf8(&id)?)))
					.await?;
			} else if let Some(id) = text.strip_prefix("join ") {
				let id = id
					.as_bytes()
					.try_into()
					.map_err(|_| Error::BadSyntax("bad join ID"))?;
				server.increase_player_count(id).await?;
				*puzzle_id = Some(id);
				let info = get_puzzle_info(server, &id).await?;
				ws.send(Message::Binary(info)).await?;
			} else if let Some(id) = text.strip_prefix("rejoin ") {
				let id = id
					.as_bytes()
					.try_into()
					.map_err(|_| Error::BadSyntax("bad join ID"))?;
				if puzzle_id.is_some() {
					return Err(Error::BadSyntax("unexpected rejoin"));
				}
				server.increase_player_count(id).await?;
				*puzzle_id = Some(id);
				ws.send(Message::Text("rejoined".to_string())).await?;
			} else if text == "poll" {
				let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
				let PieceInfo {
					positions,
					connectivity,
				} = server.get_piece_info(puzzle_id).await?;
				let mut hasher = std::hash::DefaultHasher::new();
				let positions_bytes = transmute_to_bytes(&positions[..]);
				let connectivity_bytes = transmute_to_bytes(&connectivity[..]);
				use std::hash::Hasher;
				hasher.write(positions_bytes);
				hasher.write(connectivity_bytes);
				let hash = hasher.finish();
				if hash != last_poll_hash {
					// don't send update if nothing's changed
					last_poll_hash = hash;
					let mut data = vec![2, 0, 0, 0, 0, 0, 0, 0]; // opcode / version number + padding
					data.extend_from_slice(positions_bytes);
					data.extend_from_slice(connectivity_bytes);
					ws.send(Message::Binary(data)).await?;
				}
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
		} else if let Message::Binary(data) = &message {
			if data.len() % 4 != 0 {
				return Err(Error::BadSyntax("binary message not multiple of 4 bytes"));
			}
			let puzzle_id = puzzle_id.ok_or(Error::NotJoined)?;
			let mut reader_data = std::io::Cursor::new(data);
			let reader = &mut reader_data;
			fn read<const N: usize>(reader: &mut std::io::Cursor<&Vec<u8>>) -> Result<[u8; N]> {
				let mut data = [0; N];
				reader
					.read_exact(&mut data)
					.map_err(|_| Error::BadSyntax("unexpected EOF in action sequence"))?;
				Ok(data)
			}
			fn read_u32(reader: &mut std::io::Cursor<&Vec<u8>>) -> Result<u32> {
				Ok(u32::from_le_bytes(read(reader)?))
			}
			fn read_f32(reader: &mut std::io::Cursor<&Vec<u8>>) -> Result<f32> {
				Ok(f32::from_le_bytes(read(reader)?))
			}
			let message_id = read_u32(reader)?;
			while reader.position() < reader.get_ref().len() as u64 {
				let action = read_u32(reader)?;
				if action == ACTION_MOVE {
					let piece: usize = read_u32(reader)? as _;
					let x: f32 = read_f32(reader)?;
					let y: f32 = read_f32(reader)?;
					for coord in [x, y] {
						if !coord.is_finite() || coord < 0.0 || coord > 2.0 {
							return Err(Error::BadSyntax("piece position out of bounds"));
						}
					}
					server.move_piece(puzzle_id, piece, x, y).await?;
				} else if action == ACTION_CONNECT {
					let piece1: usize = read_u32(reader)? as _;
					let piece2: usize = read_u32(reader)? as _;
					server.connect_pieces(puzzle_id, piece1, piece2).await?;
				} else {
					return Err(Error::BadSyntax("bad action"));
				}
			}
			ws.send(Message::Text(format!("ack {message_id}"))).await?;
		}
	}
	Ok(())
}

async fn handle_connection(server: &Server, conn: &mut tokio::net::TcpStream) -> Result<()> {
	let mut puzzle_id = None;
	let mut ws = tokio_tungstenite::accept_async_with_config(
		conn,
		Some(tungstenite::protocol::WebSocketConfig {
			max_message_size: Some(128 << 10),
			max_frame_size: Some(128 << 10),
			..Default::default()
		}),
	)
	.await?;
	let status = handle_websocket(server, &mut puzzle_id, &mut ws).await;
	if let Err(e) = &status {
		ws.send(Message::Text(format!("error {e}"))).await?;
	};
	if let Some(puzzle_id) = puzzle_id {
		if let Err(e) = server.decrease_player_count(puzzle_id).await {
			eprintln!(
				"unexpected error while decreasing player count for puzzle {puzzle_id:?}: {e}"
			);
		}
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

async fn create_table_if_doesnt_exist(database: &tokio_postgres::Client) -> Result<()> {
	if database.query("SELECT FROM puzzles", &[]).await.is_err() {
		database
			.execute(
				&format!(
					"CREATE TABLE puzzles (
			id char({PUZZLE_ID_LEN}) PRIMARY KEY,
			url text,
			width int4,
			height int4,
			create_time timestamp DEFAULT CURRENT_TIMESTAMP,
			seed int4,
			connectivity int2[],
			positions float4[]
		)"
				),
				&[],
			)
			.await?;
		database
			.execute("CREATE INDEX by_id ON puzzles (id)", &[])
			.await?;
	}
	Ok(())
}

#[tokio::main]
async fn main() {
	let host_addr = SocketAddr::from(([127, 0, 0, 1], PORT));
	let listener = match tokio::net::TcpListener::bind(host_addr).await {
		Ok(l) => l,
		Err(e) => {
			eprintln!("Couldn't bind to localhost:{PORT}: {e}");
			return;
		}
	};
	let start_time = SystemTime::now();
	let server: &'static Server = Box::leak(Box::new({
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
		if let Err(e) = create_table_if_doesnt_exist(&client).await {
			eprintln!("couldn't create table: {e}");
			return;
		};
		use tokio_postgres::types::Type;
		let create_puzzle = client
			.prepare_typed("INSERT INTO puzzles (id) VALUES ($1)", &[Type::BPCHAR])
			.await
			.expect("couldn't prepare create_puzzle statement");
		let set_puzzle_data = client
			.prepare_typed(
				"UPDATE puzzles SET width = $1, height = $2, url = $3,
    			connectivity = $4, positions = $5, seed = $6 WHERE id = $7",
				&[
					Type::INT4,
					Type::INT4,
					Type::TEXT,
					Type::INT2_ARRAY,
					Type::FLOAT4_ARRAY,
					Type::INT4,
					Type::BPCHAR,
				],
			)
			.await
			.expect("couldn't prepare set_puzzle_data statement");
		let move_piece = client.prepare_typed("UPDATE puzzles SET positions[$1] = $2, positions[$3] = $4 WHERE id = $5 AND $6 < width * height",
			&[Type::INT4, Type::FLOAT4, Type::INT4, Type::FLOAT4, Type::BPCHAR, Type::INT4])
			.await.expect("couldn't prepare move_piece statement");
		let connect_pieces = client.prepare_typed(
			"UPDATE puzzles SET connectivity = array_replace(connectivity, connectivity[$1], connectivity[$2]) WHERE id = $3 AND $1 <= width * height AND $2 <= width * height",
			&[Type::INT4, Type::INT4, Type::BPCHAR])
			.await.expect("couldn't prepare connect_pieces statement");
		let get_piece_info = client
			.prepare_typed(
				"SELECT positions, connectivity FROM puzzles WHERE id = $1",
				&[Type::BPCHAR],
			)
			.await
			.expect("couldn't prepare get_piece_info statement");
		let get_puzzle_info = client.prepare_typed("SELECT width, height, url, positions, seed, connectivity FROM puzzles WHERE id = $1", &[Type::BPCHAR])
			.await.expect("couldn't prepare get_puzzle_info statement");
		Server {
			player_counts: Mutex::new(HashMap::new()),
			create_puzzle,
			set_puzzle_data,
			move_piece,
			connect_pieces,
			get_piece_info,
			get_puzzle_info,
			database: client,
			wikimedia_potd: RwLock::new(potd),
			wikimedia_featured,
		}
	}));
	tokio::task::spawn(async move {
		fn next_day(t: SystemTime) -> SystemTime {
			let day = 60 * 60 * 24;
			let dt = t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
			SystemTime::UNIX_EPOCH + Duration::from_secs((dt + day - 1) / day * day)
		}
		let mut last_time = start_time;
		loop {
			// sleep for a little longer to make sure new PotD is available
			let extra_sleep = Duration::from_secs(60);
			let time_to_sleep =
				next_day(last_time).duration_since(last_time).unwrap() + extra_sleep;
			tokio::time::sleep(time_to_sleep).await;
			let potd = get_potd().await;
			*server.wikimedia_potd.write().await = potd;
			last_time = SystemTime::now();
		}
	});
	tokio::task::spawn(async move {
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
		tokio::task::spawn(async move {
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
