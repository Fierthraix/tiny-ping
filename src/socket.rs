use std::mem::MaybeUninit;
use std::net::SocketAddr;
#[cfg(unix)]
use std::sync::Arc;
use std::{io, net::IpAddr};

use socket2::SockAddr;
#[cfg(unix)]
use socket2::{Domain, Protocol, Socket, Type};
#[cfg(unix)]
use tokio::io::unix::AsyncFd;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketType {
    Raw,
    Dgram,
}

#[derive(Debug, Clone)]
pub struct AsyncSocket {
    #[cfg(unix)]
    inner: Arc<AsyncFd<Socket>>,
    socket_type: SocketType,
}

impl AsyncSocket {
    #[cfg(unix)]
    pub fn new(addr: IpAddr, socket_type: SocketType) -> io::Result<AsyncSocket> {
        let ty = match socket_type {
            SocketType::Raw => Type::RAW,
            SocketType::Dgram => Type::DGRAM,
        };
        let socket = match addr {
            IpAddr::V4(_) => Socket::new(Domain::IPV4, ty, Some(Protocol::ICMPV4))?,
            IpAddr::V6(_) => Socket::new(Domain::IPV6, ty, Some(Protocol::ICMPV6))?,
        };

        // TODO: Type filtering,
        // https://tools.ietf.org/html/rfc3542#section-3.2. Currently blocked
        // on https://github.com/rust-lang/socket2/issues/199

        // TODO: Get access to the hop limits
        // https://tools.ietf.org/html/rfc3542#section-4, to show the TTL for
        // ICMPv6.

        socket.set_nonblocking(true)?;
        Ok(AsyncSocket {
            inner: Arc::new(AsyncFd::new(socket)?),
            socket_type,
        })
    }

    #[cfg(not(unix))]
    pub fn new(_addr: IpAddr, socket_type: SocketType) -> io::Result<AsyncSocket> {
        Ok(AsyncSocket { socket_type })
    }

    pub fn socket_type(&self) -> SocketType {
        self.socket_type
    }

    #[cfg(unix)]
    pub fn bind(&self, addr: &SockAddr) -> io::Result<()> {
        self.inner.get_ref().bind(addr)
    }

    #[cfg(not(unix))]
    pub fn bind(&self, _addr: &SockAddr) -> io::Result<()> {
        unsupported()
    }

    #[cfg(unix)]
    pub fn set_ttl(&self, addr: IpAddr, ttl: u32) -> io::Result<()> {
        match addr {
            IpAddr::V4(_) => self.inner.get_ref().set_ttl_v4(ttl),
            IpAddr::V6(_) => self.inner.get_ref().set_unicast_hops_v6(ttl),
        }
    }

    #[cfg(not(unix))]
    pub fn set_ttl(&self, _addr: IpAddr, _ttl: u32) -> io::Result<()> {
        unsupported()
    }

    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    pub fn bind_device(&self, interface: Option<&[u8]>) -> io::Result<()> {
        self.inner.get_ref().bind_device(interface)
    }

    #[cfg(unix)]
    pub async fn recv_from(
        &self,
        buf: &mut [MaybeUninit<u8>],
    ) -> io::Result<(usize, Option<SocketAddr>)> {
        loop {
            let mut guard = self.inner.readable().await?;

            match guard.try_io(|inner| {
                inner
                    .get_ref()
                    .recv_from(buf)
                    .map(|(size, addr)| (size, addr.as_socket()))
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    #[cfg(not(unix))]
    pub async fn recv_from(
        &self,
        _buf: &mut [MaybeUninit<u8>],
    ) -> io::Result<(usize, Option<SocketAddr>)> {
        unsupported()
    }

    #[cfg(unix)]
    pub async fn send_to(&self, buf: &[u8], target: &SockAddr) -> io::Result<usize> {
        loop {
            let mut guard = self.inner.writable().await?;

            match guard.try_io(|inner| inner.get_ref().send_to(buf, target)) {
                Ok(n) => return n,
                Err(_would_block) => continue,
            }
        }
    }

    #[cfg(not(unix))]
    pub async fn send_to(&self, _buf: &[u8], _target: &SockAddr) -> io::Result<usize> {
        unsupported()
    }
}

#[cfg(not(unix))]
fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "tiny-ping sockets are only implemented on Unix targets",
    ))
}
