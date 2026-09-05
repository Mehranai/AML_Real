use std::{
    env,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = env::var("ETHEREUM_HEALTHCHECK_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:5001".to_string())
        .parse::<SocketAddr>()?;
    let timeout = Duration::from_secs(5);
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

    let mut response = [0_u8; 64];
    let bytes_read = stream.read(&mut response)?;
    let status_line = std::str::from_utf8(&response[..bytes_read])?
        .lines()
        .next()
        .unwrap_or_default();
    if status_line.starts_with("HTTP/1.1 200 ") || status_line.starts_with("HTTP/1.0 200 ") {
        return Ok(());
    }

    Err(format!("Ethereum readiness probe returned {status_line}").into())
}
