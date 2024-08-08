use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tungstenite::protocol::Message;
use rand::Rng;
use std::sync::LazyLock;
use anyhow::anyhow;

const PUZZLE_ID_CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const PUZZLE_ID_LEN: usize = 6;

fn generate_puzzle_id() -> [u8; PUZZLE_ID_LEN] {
	let mut rng = rand::thread_rng();
	[(); 6].map(|()| PUZZLE_ID_CHARSET[rng.gen_range(0..PUZZLE_ID_CHARSET.len())])
}

struct Database {
	puzzles: sled::Tree,
	pieces: sled::Tree,
}

fn get_puzzle_info(database: &Database, id: &[u8]) -> anyhow::Result<Vec<u8>> {
	if id.len() != PUZZLE_ID_LEN { return Err(anyhow!("bad puzzle ID")); }
	let mut data = vec![1, 0, 0, 0, 0, 0, 0, 0]; // opcode + padding
	let puzzle = database.puzzles.get(id)?.ok_or_else(|| anyhow!("bad puzzle ID"))?;
	data.extend_from_slice(&puzzle);
	while data.len() % 8 != 0 {
		// padding
		data.push(0);
	}
	let pieces = database.pieces.get(id)?.ok_or_else(|| anyhow!("bad puzzle ID"))?;
	data.extend_from_slice(&pieces);
	Ok(data)
}

async fn handle_connection(database: &Database, conn: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
	let mut ws = tokio_tungstenite::accept_async_with_config(
		conn,
		Some(tungstenite::protocol::WebSocketConfig {
			max_message_size: Some(4096),
			max_frame_size: Some(4096),
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
				// pick nib types
				{
					let mut rng = rand::thread_rng();
					for _ in 0..2u16 * (width as u16) * (height as u16) - (width as u16) - (height as u16) {
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
					if database.puzzles.compare_and_swap(id, None::<&'static [u8; 0]>, Some(&data[..]))?.is_ok() {
						break;
					}
				}
				drop(puzzle_data); // should be empty now
				puzzle_id = Some(id);
				let mut pieces_data: Vec<u8>;
				{
					let mut rng = rand::thread_rng();
					pieces_data = Vec::new();
					pieces_data.resize((width as usize) * (height as usize) * 10, 0);
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
					// connectivity
					for i in 0..(width as u16) * (height as u16) {
						let [a, b] = i.to_le_bytes();
						*it.next().unwrap() = a;
						*it.next().unwrap() = b;
					}
				}
				database.pieces.insert(id, pieces_data)?;
				ws.send(Message::Text(format!("id: {}", std::str::from_utf8(&id)?))).await?;
				let info = get_puzzle_info(&database, &id)?;
				ws.send(Message::Binary(info)).await?;
			} else if let Some(id) = text.strip_prefix("join ") {
				let id = id.as_bytes().try_into()?;
				puzzle_id = Some(id);
				let info = get_puzzle_info(&database, &id)?;
				ws.send(Message::Binary(info)).await?;
			} else if let Some(data) = text.strip_prefix("move ") {
				let mut parts = data.split(' ');
				let puzzle_id = puzzle_id.ok_or_else(|| anyhow!("move without puzzle ID"))?;
				let piece: usize = parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
				let x: f32 = parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
				let y: f32 = parts.next().ok_or_else(|| anyhow!("bad syntax"))?.parse()?;
				loop {
					let curr_pieces = database.pieces.get(&puzzle_id)?
						.ok_or_else(|| anyhow!("bad puzzle ID"))?;
					let mut new_pieces = curr_pieces.to_vec();
					new_pieces.get_mut(8 * piece..8 * piece + 4).ok_or_else(|| anyhow!("bad piece ID"))?
						.copy_from_slice(&x.to_le_bytes());
					new_pieces.get_mut(8 * piece + 4..8 * piece + 8).ok_or_else(|| anyhow!("bad piece ID"))?
						.copy_from_slice(&y.to_le_bytes());
					if database.pieces.compare_and_swap(&puzzle_id, Some(curr_pieces), Some(new_pieces))?.is_ok() {
						break;
					}
					tokio::time::sleep(std::time::Duration::from_millis(10)).await;
				}
			} else if text == "poll" {
				let puzzle_id = puzzle_id.ok_or_else(|| anyhow!("poll without puzzle ID"))?;
				let pieces = database.pieces.get(&puzzle_id)?.ok_or_else(|| anyhow!("bad puzzle ID"))?;
				let mut data = vec![2, 0, 0, 0, 0, 0, 0, 0]; // opcode / version number + padding
				data.extend_from_slice(&pieces);
				ws.send(Message::Binary(data)).await?;
			}
		}
	}
	Ok(())
}

#[tokio::main]
async fn main() {
	let port = 3000;
	let host_addr = SocketAddr::from(([127, 0, 0, 1], port));
	let listener = match tokio::net::TcpListener::bind(host_addr).await {
		Ok(l) => l,
		Err(e) => {
			eprintln!("Couldn't bind to localhost:{port}: {e}");
			return;
		}
	};
	tokio::task::spawn(async {
		loop {
			// TODO : sweep
			tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
		}
	});
	static DATABASE_VALUE: LazyLock<Database> = LazyLock::new(|| {
		let db = sled::open("database.sled").expect("error opening database");
		let puzzles = db.open_tree("PUZZLES").expect("error opening puzzles tree");
		let pieces = db.open_tree("PIECES").expect("error opening pieces tree");
		Database {
			puzzles,
			pieces
		}
	});
	let database: &Database = &DATABASE_VALUE;
	loop {
		let (mut stream, addr) = match listener.accept().await {
			Ok(result) => result,
			Err(e) => {
				eprintln!("Error accepting connection: {e}");
				continue;
			}
		};
		tokio::task::spawn(async move {
			match handle_connection(database, &mut stream).await {
				Ok(()) => {}
				Err(e) => {
					eprintln!("Error handling connection to {addr}: {e}");
				}
			}
			let _ = stream.shutdown().await;
		});
	}
}
