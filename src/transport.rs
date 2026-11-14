//! Bounded Kodi JSON-RPC TCP transport, loopback only.
//!
//! Kodi's raw TCP JSON-RPC interface is not authenticated. This connector
//! intentionally cannot send to a remote IP address. Keep Kodi TCP JSON-RPC
//! bound to 127.0.0.1 and lock down local service users.
use serde_json::Value;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

pub const KODI_JSONRPC_PORT: u16 = 9090;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Sends an allowlisted playback request to Kodi on IPv4 loopback.
/// A successful JSON-RPC response only confirms acceptance, not actual video output.
pub fn send_playback(request: &Value) -> io::Result<Value> {
    send_to_loopback(KODI_JSONRPC_PORT, request)
}

fn send_to_loopback(port: u16, request: &Value) -> io::Result<Value> {
    if request["jsonrpc"] != "2.0" || request["method"] != "Player.Open" || !request["id"].is_u64()
    {
        return Err(invalid("only identified Player.Open requests are allowed"));
    }
    // Validate before connecting: invalid input must not touch Kodi at all.
    let mut payload = serde_json::to_vec(request)?;
    if payload.len() > 64 * 1024 {
        return Err(invalid("request exceeds Kodi control size limit"));
    }
    payload.push(b'\n');
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(&payload)?;

    let deadline = Instant::now() + TIMEOUT;
    let mut raw = Vec::with_capacity(4096);
    let mut buffer = [0u8; 4096];
    let mut announcements = 0usize;
    loop {
        // Kodi may send asynchronous announcements before the matching reply,
        // even as concatenated JSON documents with no separator.
        while !raw.is_empty() {
            let mut decoded = serde_json::Deserializer::from_slice(&raw).into_iter::<Value>();
            match decoded.next() {
                Some(Ok(response)) => {
                    let used = decoded.byte_offset();
                    raw.drain(..used);
                    if response.get("method").is_some() && response.get("id").is_none() {
                        announcements += 1;
                        if announcements > 32 {
                            return Err(invalid("too many unsolicited Kodi announcements"));
                        }
                        continue;
                    }
                    if response["id"] != request["id"] || response["jsonrpc"] != "2.0" {
                        return Err(invalid("unexpected JSON-RPC reply identifier"));
                    }
                    if response.get("error").is_some() {
                        return Err(invalid("Kodi rejected Player.Open"));
                    }
                    // Player.Open must explicitly acknowledge acceptance.
                    // A null/false/unexpected result is not playback success.
                    if response.get("result") != Some(&Value::String("OK".into())) {
                        return Err(invalid("Kodi did not acknowledge Player.Open"));
                    }
                    return Ok(response);
                }
                Some(Err(err)) if err.is_eof() => break,
                Some(Err(err)) => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, err));
                }
                None => {
                    raw.clear();
                    break;
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Kodi reply deadline expired",
            ));
        }
        stream.set_read_timeout(Some(remaining))?;
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Kodi closed connection without a complete reply",
            ));
        }
        if raw.len().saturating_add(count) > MAX_RESPONSE_BYTES {
            return Err(invalid("Kodi response exceeds size limit"));
        }
        raw.extend_from_slice(&buffer[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn receives_fragmented_local_playback_reply() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, addr) = listener.accept().unwrap();
            assert_eq!(addr.ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
            let mut input = [0u8; 1024];
            let count = client.read(&mut input).unwrap();
            let request: Value = serde_json::from_slice(&input[..count]).unwrap();
            assert_eq!(request["method"], "Player.Open");
            client.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":").unwrap();
            client.write_all(b"7,\"result\":\"OK\"}").unwrap();
        });
        let request = serde_json::json!({
            "jsonrpc":"2.0", "method":"Player.Open",
            "id":7, "params":{"item":{"channelid":12}}
        });
        assert_eq!(send_to_loopback(port, &request).unwrap()["result"], "OK");
        worker.join().unwrap();
    }

    #[test]
    fn rejects_oversized_request_before_connecting() {
        let request = serde_json::json!({
            "jsonrpc":"2.0", "method":"Player.Open", "id":7,
            "params":{"item":{"file":"x".repeat(70_000)}}
        });
        assert_eq!(
            send_to_loopback(1, &request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn rejects_unapproved_methods_before_connecting() {
        let request =
            serde_json::json!({"jsonrpc":"2.0","method":"Settings.SetSettingValue","id":7});
        assert_eq!(
            send_to_loopback(1, &request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn ignores_unsolicited_events_before_playback_acknowledgement() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut request = [0u8; 256];
            let _received = client.read(&mut request).unwrap();
            client
                .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"Player.OnPlay\",\"params\":{}}")
                .unwrap();
            client
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":\"OK\"}")
                .unwrap();
        });
        let request = serde_json::json!({
            "jsonrpc":"2.0","method":"Player.Open","id":7,
            "params":{"item":{"channelid":1}}
        });
        assert_eq!(send_to_loopback(port, &request).unwrap()["result"], "OK");
        worker.join().unwrap();
    }

    #[test]
    fn detects_mismatched_reply_identifier() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut request = [0u8; 256];
            let _received = client.read(&mut request).unwrap();
            client
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":999,\"result\":\"OK\"}")
                .unwrap();
        });
        let request = serde_json::json!({
            "jsonrpc":"2.0","method":"Player.Open","id":7,
            "params":{"item":{"channelid":1}}
        });
        assert_eq!(
            send_to_loopback(port, &request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        worker.join().unwrap();
    }

    #[test]
    fn rejects_truncated_json_reply_without_panicking() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut input = [0u8; 256];
            let _received = client.read(&mut input).unwrap();
            client
                .write_all(br#"{"jsonrpc":"2.0","id":7,"result":"#)
                .unwrap();
        });
        let request = serde_json::json!({
            "jsonrpc":"2.0","method":"Player.Open","id":7,
            "params":{"item":{"channelid":1}}
        });
        assert_eq!(
            send_to_loopback(port, &request).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        worker.join().unwrap();
    }

    #[test]
    fn rejects_kodi_error_response() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut input = [0u8; 256];
            let _received = client.read(&mut input).unwrap();
            client.write_all(br#"{"jsonrpc":"2.0","id":7,"error":{"code":-32602,"message":"Invalid params"}}"#).unwrap();
        });
        let request = serde_json::json!({"jsonrpc":"2.0","method":"Player.Open","id":7,"params":{"item":{"channelid":1}}});
        assert_eq!(
            send_to_loopback(port, &request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        worker.join().unwrap();
    }
    #[test]
    fn rejects_reply_without_result() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut input = [0u8; 256];
            let _ = client.read(&mut input).unwrap();
            client.write_all(br#"{"jsonrpc":"2.0","id":7}"#).unwrap();
        });
        let request = serde_json::json!({
            "jsonrpc":"2.0","method":"Player.Open","id":7,
            "params":{"item":{"channelid":1}}
        });
        assert_eq!(
            send_to_loopback(port, &request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        worker.join().unwrap();
    }

    #[test]
    fn rejects_null_result_in_player_open_reply() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut input = [0u8; 256];
            let _ = client.read(&mut input).unwrap();
            client
                .write_all(br#"{"jsonrpc":"2.0","id":7,"result":null}"#)
                .unwrap();
        });
        let request = serde_json::json!({
            "jsonrpc":"2.0","method":"Player.Open","id":7,
            "params":{"item":{"channelid":1}}
        });
        assert_eq!(
            send_to_loopback(port, &request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        worker.join().unwrap();
    }

    #[test]
    fn rejects_oversized_kodi_reply() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut input = [0u8; 256];
            let _ = client.read(&mut input).unwrap();
            // An incomplete JSON string forces the reader to enforce its byte
            // bound rather than accepting an early, valid JSON-RPC response.
            let prefix = br#"{"jsonrpc":"2.0","id":7,"result":""#;
            client.write_all(prefix).unwrap();
            let chunk = [b'x'; 4096];
            for _ in 0..=MAX_RESPONSE_BYTES / chunk.len() {
                if client.write_all(&chunk).is_err() {
                    break; // The client is expected to close on the size limit.
                }
            }
        });
        let request = serde_json::json!({
            "jsonrpc":"2.0","method":"Player.Open","id":7,
            "params":{"item":{"channelid":1}}
        });
        assert_eq!(
            send_to_loopback(port, &request).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        worker.join().unwrap();
    }
}
