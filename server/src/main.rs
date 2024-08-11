use anyhow::anyhow;
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use rand::seq::SliceRandom;
use std::io::prelude::*;
use std::net::SocketAddr;
use std::sync::LazyLock;
use tokio::io::AsyncWriteExt;
use tungstenite::protocol::Message;
use std::time::{SystemTime, Duration};

const PUZZLE_ID_CHARSET: &[u8] = b"23456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
const PUZZLE_ID_LEN: usize = 7;

fn generate_puzzle_id() -> [u8; PUZZLE_ID_LEN] {
	let mut rng = rand::thread_rng();
	[(); 7].map(|()| *PUZZLE_ID_CHARSET.choose(&mut rng).unwrap())
}

struct Server {
	puzzles: sled::Tree,
	pieces: sled::Tree,
	connectivity: sled::Tree,
	wikimedia_featured: Vec<String>,
}

fn get_puzzle_info(server: &Server, id: &[u8]) -> anyhow::Result<Vec<u8>> {
	if id.len() != PUZZLE_ID_LEN {
		return Err(anyhow!("bad puzzle ID"));
	}
	let mut data = vec![1, 0, 0, 0, 0, 0, 0, 0]; // opcode + padding
	let puzzle = server
		.puzzles
		.get(id)?
		.ok_or_else(|| anyhow!("bad puzzle ID"))?;
	data.extend_from_slice(&puzzle);
	while data.len() % 8 != 0 {
		// padding
		data.push(0);
	}
	let pieces = server
		.pieces
		.get(id)?
		.ok_or_else(|| anyhow!("bad puzzle ID"))?;
	data.extend_from_slice(&pieces);
	let connectivity = server
		.connectivity
		.get(id)?
		.ok_or_else(|| anyhow!("bad puzzle ID"))?;
	data.extend_from_slice(&connectivity);
	Ok(data)
}

async fn handle_connection(
	server: &Server,
	conn: &mut tokio::net::TcpStream,
) -> anyhow::Result<()> {
	let mut ws = tokio_tungstenite::accept_async_with_config(
		conn,
		Some(tungstenite::protocol::WebSocketConfig {
			max_message_size: Some(65536),
			max_frame_size: Some(65536),
			..Default::default()
		}),
	)
	.await?;
	let mut puzzle_id = None;
	while let Some(message) = ws.next().await {
		let message = message?;
		if matches!(message, Message::Close(_)) {
			break;
		}
		if let Message::Text(text) = &message {
			let text = text.trim();
			if let Some(dimensions) = text.strip_prefix("new ") {
				let mut parts = dimensions.split(' ');
				let width: u8 = parts.next().ok_or_else(|| anyhow!("no width"))?.parse()?;
				let height: u8 = parts.next().ok_or_else(|| anyhow!("no height"))?.parse()?;
				let url: &str = parts.next().ok_or_else(|| anyhow!("no url"))?;
				if url.len() > 255 {
					return Err(anyhow!("image URL too long"));
				}
				if (width as u16) * (height as u16) > 1000 {
					return Err(anyhow!("too many pieces"));
				}
				let mut puzzle_data = vec![width, height];
				let timestamp: u64 = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("time went backwards :/").as_secs();
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
				puzzle_id = Some(id);
				let mut pieces_data: Vec<u8>;
				{
					let mut rng = rand::thread_rng();
					pieces_data = Vec::new();
					pieces_data.resize((width as usize) * (height as usize) * 8, 0);
					// positions
					let mut it = pieces_data.iter_mut();
					for _ in 0..(width as u16) * (height as u16) * 2 {
						let coord: f32 = rng.gen();
						let [a, b, c, d] = coord.to_le_bytes();
						*it.next().unwrap() = a;
						*it.next().unwrap() = b;
						*it.next().unwrap() = c;
						*it.next().unwrap() = d;
					}
				}
				server.pieces.insert(id, pieces_data)?;
				let mut connectivity_data = Vec::new();
				connectivity_data.resize((width as usize) * (height as usize) * 2, 0);
				let mut it = connectivity_data.iter_mut();
				for i in 0..(width as u16) * (height as u16) {
					let [a, b] = i.to_le_bytes();
					*it.next().unwrap() = a;
					*it.next().unwrap() = b;
				}
				server.connectivity.insert(id, connectivity_data)?;
				ws.send(Message::Text(format!("id: {}", std::str::from_utf8(&id)?)))
					.await?;
				let info = get_puzzle_info(&server, &id)?;
				ws.send(Message::Binary(info)).await?;
			} else if let Some(id) = text.strip_prefix("join ") {
				let id = id.as_bytes().try_into()?;
				puzzle_id = Some(id);
				let info = get_puzzle_info(&server, &id)?;
				ws.send(Message::Binary(info)).await?;
			} else if text.starts_with("move ") {
				let puzzle_id = puzzle_id.ok_or_else(|| anyhow!("move without puzzle ID"))?;
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
					let piece: usize =
						parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
					let x: f32 = parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
					let y: f32 = parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
					motions.push(Motion { piece, x, y });
				}
				loop {
					let curr_pieces = server
						.pieces
						.get(&puzzle_id)?
						.ok_or_else(|| anyhow!("bad puzzle ID"))?;
					let mut new_pieces = curr_pieces.to_vec();
					for Motion { piece, x, y } in motions.iter().copied() {
						new_pieces
							.get_mut(8 * piece..8 * piece + 4)
							.ok_or_else(|| anyhow!("bad piece ID"))?
							.copy_from_slice(&x.to_le_bytes());
						new_pieces
							.get_mut(8 * piece + 4..8 * piece + 8)
							.ok_or_else(|| anyhow!("bad piece ID"))?
							.copy_from_slice(&y.to_le_bytes());
					}
					if server
						.pieces
						.compare_and_swap(&puzzle_id, Some(curr_pieces), Some(new_pieces))?
						.is_ok()
					{
						break;
					}
					tokio::time::sleep(std::time::Duration::from_millis(1)).await; // yield maybe (don't let contention hog resources)
				}
				ws.send(Message::Text("ack".to_string())).await?;
			} else if let Some(data) = text.strip_prefix("connect ") {
				let mut parts = data.split(' ');
				let puzzle_id = puzzle_id.ok_or_else(|| anyhow!("connect without puzzle ID"))?;
				let piece1: usize = parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
				let piece2: usize = parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
				loop {
					let curr_connectivity = server
						.connectivity
						.get(&puzzle_id)?
						.ok_or_else(|| anyhow!("bad puzzle ID"))?;
					let mut new_connectivity = curr_connectivity.to_vec();
					if piece1 >= curr_connectivity.len() / 2
						|| piece2 >= curr_connectivity.len() / 2
					{
						return Err(anyhow!("bad piece ID"));
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
					if server
						.connectivity
						.compare_and_swap(
							&puzzle_id,
							Some(curr_connectivity),
							Some(new_connectivity),
						)?
						.is_ok()
					{
						break;
					}
					tokio::time::sleep(std::time::Duration::from_millis(1)).await; // yield maybe (don't let contention hog resources)
				}
			} else if text == "poll" {
				let puzzle_id = puzzle_id.ok_or_else(|| anyhow!("poll without puzzle ID"))?;
				let pieces = server
					.pieces
					.get(&puzzle_id)?
					.ok_or_else(|| anyhow!("bad puzzle ID"))?;
				let connectivity = server
					.connectivity
					.get(&puzzle_id)?
					.ok_or_else(|| anyhow!("bad puzzle ID"))?;
				let mut data = vec![2, 0, 0, 0, 0, 0, 0, 0]; // opcode / version number + padding
				data.extend_from_slice(&pieces);
				data.extend_from_slice(&connectivity);
				ws.send(Message::Binary(data)).await?;
			} else if text == "randomFeaturedWikimedia" {
				let choice = rand::thread_rng().gen_range(0..server.wikimedia_featured.len());
				ws.send(Message::Text(format!(
					"wikimediaImage {}",
					server.wikimedia_featured[choice]
				)))
				.await?;
			}
		}
	}
	Ok(())
}

fn read_to_lines(path: &str) -> std::io::Result<Vec<String>> {
	let file = std::fs::File::open(path)?;
	let reader = std::io::BufReader::new(file);
	reader.lines().collect()
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
	static SERVER_VALUE: LazyLock<Server> = LazyLock::new(|| {
		let wikimedia_featured =
			read_to_lines("featuredpictures.txt").expect("Couldn't read featuredpictures.txt");
		let db = sled::open("database.sled").expect("error opening database");
		let puzzles = db.open_tree("PUZZLES").expect("error opening puzzles tree");
		let pieces = db.open_tree("PIECES").expect("error opening pieces tree");
		let connectivity = db
			.open_tree("CONNECTIVITY")
			.expect("error opening connectivity tree");
		Server {
			puzzles,
			pieces,
			connectivity,
			wikimedia_featured,
		}
	});
	let server: &Server = &SERVER_VALUE;
	tokio::task::spawn(async {
		loop {
			// TODO : sweep
			let now = SystemTime::now();
			let mut to_delete = vec![];
			for item in server.puzzles.iter() {
				let (key, value) = item.expect("sweep failed to read database");
				let timestamp: [u8; 8] = value[2..2 + 8].try_into().unwrap();
				let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(u64::from_le_bytes(timestamp));
				if now.duration_since(timestamp).unwrap_or_default() >= Duration::from_secs(60 * 60 * 24 * 7) {
					// delete puzzles created at least 1 week ago
					to_delete.push(key);
				}
			}
			for key in to_delete {
				// technically there is a race condition here but stop being silly
				server.puzzles.remove(&key).expect("sweep failed to delete entry");
				server.pieces.remove(&key).expect("sweep failed to delete entry");
				server.connectivity.remove(&key).expect("sweep failed to delete entry");
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
