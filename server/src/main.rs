use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::io::Write;

async fn handle_request(_request: &[u8]) -> (Vec<u8>, &'static str) {
//	if let Some(rest) = request.strip_prefix(b"GET /") {
//	} else if let Some(rest) = request.strip_prefix(b"POST /") {
//	} else {
		(b"Bad request".to_vec(), "400 Bad Request")
//	}
}

async fn handle_connection(conn: &mut tokio::net::TcpStream) -> Result<(), String> {
	let mut request = vec![0; 60];
	let mut request_len = 0;
	loop {
		let n = conn.read(&mut request[request_len..]).await.map_err(|e| format!("read error: {e}"))?;
		if n == 0 {
			return Err(format!("unexpected EOF"));
		}
		match request[request_len..request_len + n].windows(2).position(|w| w == b"\r\n") {
			Some(end) => {
				request_len += end;
				break;
			}
			None => {
				request_len += n;
				if request_len == request.len() {
					break;
				}
			}
		}
	}
	let (response_content, status) = handle_request(&request[..request_len]).await;
	let mut response = vec![];
	let _ = write!(response, "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
		response_content.len());
	response.extend_from_slice(&response_content);
	let _ = conn.write_all(&response).await.map_err(|e| format!("write error: {e}"))?;
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
