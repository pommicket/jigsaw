use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tungstenite::protocol::Message;

async fn handle_connection(conn: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
	let mut ws = tokio_tungstenite::accept_async_with_config(
		conn,
		Some(tungstenite::protocol::WebSocketConfig {
			max_message_size: Some(4096),
			max_frame_size: Some(4096),
			..Default::default()
		}),
	)
	.await?;
	while let Some(message) = ws.next().await {
		let message = message?;
		if matches!(message, Message::Close(_)) {
			break;
		}
		println!("{:?}", message);
		ws.send(message).await?;
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
	loop {
		let (mut stream, addr) = match listener.accept().await {
			Ok(result) => result,
			Err(e) => {
				eprintln!("Error accepting connection: {e}");
				continue;
			}
		};
		tokio::task::spawn(async move {
			match handle_connection(&mut stream).await {
				Ok(()) => {}
				Err(e) => {
					eprintln!("Error handling connection to {addr}: {e}");
				}
			}
			let _ = stream.shutdown().await;
		});
	}
}
