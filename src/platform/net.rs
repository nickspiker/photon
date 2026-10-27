//! The socket types Photon's network layer names: tokio's on every OS with sockets of its own; on ferros the types in `ferros_net`, which speak to the ip server with the same calls (STD-TARGET.md in the ferros repo).

#[cfg(not(target_os = "ferros"))]
pub use tokio::net::{TcpListener, TcpStream, UdpSocket};
#[cfg(target_os = "ferros")]
pub use super::ferros_net::{TcpListener, TcpStream, UdpSocket};
