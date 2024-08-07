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
				if (width as u16) * (height as u16) > 1000 {
					return Err(anyhow!("too many pieces"));
				}
				let mut puzzle_data = vec![width, height];
				puzzle_data.extend(url.as_bytes());
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
				let pieces_data: Vec<u8>;
				{
					let mut rng = rand::thread_rng();
					pieces_data = (0..(width as u16) * (height as u16) * 4).map(|_| rng.gen()).collect();
				}
				database.pieces.insert(id, pieces_data)?;
				ws.send(Message::Text(format!("id: {}", std::str::from_utf8(&id)?))).await?;
			} else if text == "poll" {
				let puzzle_id = puzzle_id.ok_or_else(|| anyhow!("poll without puzzle ID"))?;
				let pieces = database.pieces.get(&puzzle_id)?.ok_or_else(|| anyhow!("bad puzzle ID: {puzzle_id:?}"))?;
				let pieces = pieces.to_vec();
				ws.send(Message::Binary(pieces)).await?;
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
