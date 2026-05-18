use std::{
    mem::MaybeUninit,
    net::{IpAddr, SocketAddr},
    sync::atomic::{AtomicU16, Ordering},
    time::{Duration, Instant},
};

use tokio::time::timeout;

use crate::error::{Error, Result};
use crate::icmp::{EchoReply, EchoRequest};
use crate::socket::AsyncSocket;

pub use crate::socket::SocketType;

const DEFAULT_PAYLOAD_SIZE: usize = 56;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const TOKEN_SIZE: usize = 8;

static NEXT_IDENT: AtomicU16 = AtomicU16::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct PingResult {
    pub reply: EchoReply,
    pub rtt: Duration,
    pub socket_type: SocketType,
}

/// A single ICMP echo request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PingRequest {
    sequence: u16,
    payload: Option<Vec<u8>>,
}

impl PingRequest {
    /// Creates a request with a sequence number and the pinger's default payload.
    pub fn new(sequence: u16) -> Self {
        Self {
            sequence,
            payload: None,
        }
    }

    /// Uses exact bytes as the ICMP echo payload for this request.
    pub fn payload(mut self, payload: impl Into<Vec<u8>>) -> Self {
        self.payload = Some(payload.into());
        self
    }

    /// Returns the ICMP sequence number.
    pub fn sequence(&self) -> u16 {
        self.sequence
    }

    /// Returns the custom payload bytes, if this request has one.
    pub fn payload_bytes(&self) -> Option<&[u8]> {
        self.payload.as_deref()
    }
}

impl From<u16> for PingRequest {
    fn from(sequence: u16) -> Self {
        Self::new(sequence)
    }
}

/// A sequence of ICMP echo requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PingSeries {
    start_sequence: u16,
    count: usize,
    interval: Duration,
    payload: Option<Vec<u8>>,
}

impl PingSeries {
    /// Creates a series of requests with incrementing sequence numbers.
    pub fn new(start_sequence: u16, count: usize) -> Self {
        Self {
            start_sequence,
            count,
            interval: Duration::ZERO,
            payload: None,
        }
    }

    /// Sets the delay between attempts. No delay is applied after the final attempt.
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Uses the same exact ICMP echo payload for each request in the series.
    pub fn payload(mut self, payload: impl Into<Vec<u8>>) -> Self {
        self.payload = Some(payload.into());
        self
    }
}

/// One attempt in a repeated ping series.
#[derive(Debug)]
#[non_exhaustive]
pub struct PingAttempt {
    pub sequence: u16,
    pub result: std::result::Result<PingResult, Error>,
}

/// Aggregate statistics for a repeated ping series.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PingSummary {
    pub transmitted: usize,
    pub received: usize,
    pub loss: f64,
    pub min_rtt: Option<Duration>,
    pub avg_rtt: Option<Duration>,
    pub max_rtt: Option<Duration>,
}

/// Results and aggregate statistics for a repeated ping series.
#[derive(Debug)]
#[non_exhaustive]
pub struct PingSeriesResult {
    pub attempts: Vec<PingAttempt>,
    pub summary: PingSummary,
}

/// A Ping struct represents the state of one particular ping instance.
#[derive(Debug, Clone)]
pub struct Pinger {
    target: SocketAddr,
    source: Option<SocketAddr>,
    ident: u16,
    size: usize,
    timeout: Duration,
    ttl: Option<u32>,
    socket: AsyncSocket,
}

impl Pinger {
    /// Creates a new raw-socket ping instance from `IpAddr`.
    pub fn new(host: IpAddr) -> Result<Pinger> {
        Self::with_socket_type(host, SocketType::Raw)
    }

    /// Creates a new ping instance using a specific socket type.
    pub fn with_socket_type(host: IpAddr, socket_type: SocketType) -> Result<Pinger> {
        Self::with_socket_addr(SocketAddr::new(host, 0), socket_type)
    }

    /// Creates a new ping instance using a specific socket address and socket type.
    ///
    /// The port is ignored. For IPv6, callers can use this to provide a
    /// `SocketAddrV6` scope ID, for example when targeting link-local multicast.
    pub fn with_socket_addr(target: SocketAddr, socket_type: SocketType) -> Result<Pinger> {
        Ok(Pinger {
            target,
            source: None,
            ident: default_ident(),
            size: DEFAULT_PAYLOAD_SIZE,
            timeout: DEFAULT_TIMEOUT,
            ttl: None,
            socket: AsyncSocket::new(target.ip(), socket_type)?,
        })
    }

    /// Changes the socket type and recreates the underlying socket.
    pub fn socket_type(&mut self, socket_type: SocketType) -> Result<&mut Pinger> {
        let socket = AsyncSocket::new(self.target.ip(), socket_type)?;
        if let Some(source) = self.source {
            socket.bind(&source.into())?;
        }
        if let Some(ttl) = self.ttl {
            socket.set_ttl(self.target.ip(), ttl)?;
        }
        self.socket = socket;
        Ok(self)
    }

    /// Returns the active socket type.
    pub fn active_socket_type(&self) -> SocketType {
        self.socket.socket_type()
    }

    /// Returns the target address.
    pub fn target(&self) -> SocketAddr {
        self.target
    }

    /// Returns the configured source address, if one has been bound.
    pub fn source(&self) -> Option<SocketAddr> {
        self.source
    }

    /// Binds the socket to a local source address.
    ///
    /// The port is ignored. For IPv6, the scope ID is preserved.
    pub fn bind_source(&mut self, source: SocketAddr) -> Result<&mut Pinger> {
        let source = socket_addr_without_port(source);
        self.socket.bind(&source.into())?;
        self.source = Some(source);
        Ok(self)
    }

    /// Sets the value for the `SO_BINDTODEVICE` option on this socket.
    ///
    /// If a socket is bound to an interface, only packets received from that
    /// particular interface are processed by the socket. Note that this only
    /// works for some socket types, particularly `AF_INET` sockets.
    ///
    /// If `interface` is `None` or an empty string it removes the binding.
    ///
    /// This function is only available on Fuchsia and Linux.
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    pub fn bind_device(&mut self, interface: Option<&[u8]>) -> Result<&mut Pinger> {
        self.socket.bind_device(interface)?;
        Ok(self)
    }

    /// Set the identification of ICMP.
    pub fn ident(&mut self, val: u16) -> &mut Pinger {
        self.ident = val;
        self
    }

    /// Returns the ICMP identifier.
    pub fn identifier(&self) -> u16 {
        self.ident
    }

    /// Set the packet payload size in bytes. (default: 56)
    pub fn size(&mut self, size: usize) -> &mut Pinger {
        self.size = size;
        self
    }

    /// Returns the default generated payload size in bytes.
    pub fn payload_size(&self) -> usize {
        self.size
    }

    /// Set the timeout of each ping. (default: 2s)
    pub fn timeout(&mut self, timeout: Duration) -> &mut Pinger {
        self.timeout = timeout;
        self
    }

    /// Returns the timeout used for each ping.
    pub fn timeout_duration(&self) -> Duration {
        self.timeout
    }

    /// Set the outgoing IPv4 TTL or IPv6 unicast hop limit.
    pub fn ttl(&mut self, ttl: u32) -> Result<&mut Pinger> {
        self.socket.set_ttl(self.target.ip(), ttl)?;
        self.ttl = Some(ttl);
        Ok(self)
    }

    /// Returns the configured outgoing IPv4 TTL or IPv6 unicast hop limit.
    pub fn ttl_value(&self) -> Option<u32> {
        self.ttl
    }

    async fn recv_reply(&self, request: &ResolvedPingRequest) -> Result<EchoReply> {
        let mut buffer = [MaybeUninit::new(0); 2048];
        loop {
            let (size, source) = self.socket.recv_from(&mut buffer).await?;
            let buf = unsafe { assume_init(&buffer[..size]) };
            let source = source.map(|addr| addr.ip()).unwrap_or(self.target.ip());
            let decoded = match self.socket.socket_type() {
                SocketType::Raw if self.target.ip().is_ipv6() => EchoReply::decode_raw(source, buf),
                SocketType::Raw => EchoReply::decode_raw(self.target.ip(), buf),
                SocketType::Dgram => EchoReply::decode_dgram(source, buf),
            };

            match decoded {
                Ok(reply) if self.reply_matches(&reply, request) => return Ok(reply),
                Ok(_) => continue,
                Err(Error::InvalidPacket)
                | Err(Error::NotEchoReply)
                | Err(Error::NotV6EchoReply)
                | Err(Error::OtherICMP)
                | Err(Error::UnknownProtocol) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn reply_matches(&self, reply: &EchoReply, request: &ResolvedPingRequest) -> bool {
        if reply.sequence != request.sequence {
            return false;
        }

        if self.socket.socket_type() == SocketType::Raw && reply.identifier != self.ident {
            return false;
        }

        !request.match_payload || reply.payload == request.payload
    }

    async fn send_request(&self, request: &ResolvedPingRequest) -> Result<Instant> {
        let packet = EchoRequest::new(self.target.ip(), self.ident, request.sequence)
            .encode_with_payload(&request.payload)?;

        let sent = Instant::now();
        let size = self.socket.send_to(&packet, &self.target.into()).await?;
        if size != packet.len() {
            return Err(Error::InvalidSize);
        }

        Ok(sent)
    }

    /// Send a ping request with sequence number.
    pub async fn ping(&self, request: impl Into<PingRequest>) -> Result<PingResult> {
        let request = self.resolve_request(request);
        let sent = self.send_request(&request).await?;

        let reply = timeout(self.timeout, self.recv_reply(&request))
            .await
            .map_err(|_| Error::Timeout)??;

        Ok(PingResult {
            reply,
            rtt: sent.elapsed(),
            socket_type: self.socket.socket_type(),
        })
    }

    /// Send one ping request and collect all matching replies until timeout.
    ///
    /// This is useful for multicast targets where more than one host can reply
    /// to the same echo request. Unlike [`Pinger::ping`], a timeout after the
    /// request is sent is not an error; it ends collection and returns the
    /// replies seen so far.
    pub async fn ping_replies(&self, request: impl Into<PingRequest>) -> Result<Vec<PingResult>> {
        let request = self.resolve_request(request);
        let sent = self.send_request(&request).await?;
        let deadline = sent + self.timeout;
        let mut replies = Vec::new();

        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            let reply = match timeout(remaining, self.recv_reply(&request)).await {
                Ok(reply) => reply?,
                Err(_) => break,
            };

            replies.push(PingResult {
                reply,
                rtt: sent.elapsed(),
                socket_type: self.socket.socket_type(),
            });
        }

        Ok(replies)
    }

    /// Sends a sequence of ping requests and returns each attempt plus summary statistics.
    pub async fn ping_many(&self, series: PingSeries) -> PingSeriesResult {
        let mut attempts = Vec::with_capacity(series.count);

        for index in 0..series.count {
            let sequence = series.start_sequence.wrapping_add(index as u16);
            let request = match &series.payload {
                Some(payload) => PingRequest::new(sequence).payload(payload.clone()),
                None => PingRequest::new(sequence),
            };
            let result = self.ping(request).await;
            attempts.push(PingAttempt { sequence, result });

            if index + 1 < series.count && !series.interval.is_zero() {
                tokio::time::sleep(series.interval).await;
            }
        }

        let summary = PingSummary::from_attempts(&attempts);
        PingSeriesResult { attempts, summary }
    }

    fn resolve_request(&self, request: impl Into<PingRequest>) -> ResolvedPingRequest {
        resolve_ping_request(self.ident, self.size, request.into())
    }
}

impl PingSummary {
    fn from_attempts(attempts: &[PingAttempt]) -> Self {
        let transmitted = attempts.len();
        let rtts: Vec<Duration> = attempts
            .iter()
            .filter_map(|attempt| attempt.result.as_ref().ok().map(|result| result.rtt))
            .collect();
        let received = rtts.len();
        let loss = if transmitted == 0 {
            0.0
        } else {
            ((transmitted - received) as f64 / transmitted as f64) * 100.0
        };
        let min_rtt = rtts.iter().copied().min();
        let max_rtt = rtts.iter().copied().max();
        let avg_rtt = average_duration(&rtts);

        Self {
            transmitted,
            received,
            loss,
            min_rtt,
            avg_rtt,
            max_rtt,
        }
    }
}

struct ResolvedPingRequest {
    sequence: u16,
    payload: Vec<u8>,
    match_payload: bool,
}

fn resolve_ping_request(
    ident: u16,
    default_payload_size: usize,
    request: PingRequest,
) -> ResolvedPingRequest {
    match request.payload {
        Some(payload) => ResolvedPingRequest {
            sequence: request.sequence,
            payload,
            match_payload: true,
        },
        None => {
            let payload = request_payload(ident, request.sequence, default_payload_size);
            let match_payload = !payload.is_empty();
            ResolvedPingRequest {
                sequence: request.sequence,
                payload,
                match_payload,
            }
        }
    }
}

fn average_duration(durations: &[Duration]) -> Option<Duration> {
    let total: u128 = durations.iter().map(Duration::as_nanos).sum();
    let average = total.checked_div(durations.len() as u128)?;
    Some(Duration::from_nanos(average.min(u64::MAX as u128) as u64))
}

fn default_ident() -> u16 {
    let pid = std::process::id() as u16;
    let next = NEXT_IDENT.fetch_add(1, Ordering::Relaxed);
    pid.wrapping_add(next)
}

fn socket_addr_without_port(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(mut addr) => {
            addr.set_port(0);
            SocketAddr::V4(addr)
        }
        SocketAddr::V6(mut addr) => {
            addr.set_port(0);
            SocketAddr::V6(addr)
        }
    }
}

fn request_payload(ident: u16, seq_cnt: u16, size: usize) -> Vec<u8> {
    let mut payload = vec![0; size];
    let token = [
        b't',
        b'p',
        (ident >> 8) as u8,
        ident as u8,
        (seq_cnt >> 8) as u8,
        seq_cnt as u8,
        (size >> 8) as u8,
        size as u8,
    ];
    let len = payload.len().min(TOKEN_SIZE);
    payload[..len].copy_from_slice(&token[..len]);
    payload
}

/// Assume the `buf`fer to be initialised.
///
/// # Safety
///
/// `socket2` initialises exactly the number of bytes returned by `recv_from`.
unsafe fn assume_init(buf: &[MaybeUninit<u8>]) -> &[u8] {
    unsafe { &*(buf as *const [MaybeUninit<u8>] as *const [u8]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn request_payload_respects_size() {
        assert_eq!(request_payload(1, 2, 0), Vec::<u8>::new());
        assert_eq!(request_payload(1, 2, 4), vec![b't', b'p', 0, 1]);
        assert_eq!(request_payload(1, 2, 8), vec![b't', b'p', 0, 1, 0, 2, 0, 8]);
    }

    #[test]
    fn ping_request_from_sequence_uses_default_payload() {
        let request = PingRequest::from(7);

        assert_eq!(request.sequence(), 7);
        assert_eq!(request.payload_bytes(), None);
    }

    #[test]
    fn ping_request_keeps_custom_payload() {
        let request = PingRequest::new(9).payload(b"hello");

        assert_eq!(request.sequence(), 9);
        assert_eq!(request.payload_bytes(), Some(b"hello".as_slice()));
    }

    #[test]
    fn default_request_with_empty_generated_payload_matches_any_payload() {
        let request = resolve_ping_request(1, 0, PingRequest::new(2));

        assert_eq!(request.sequence, 2);
        assert!(request.payload.is_empty());
        assert!(!request.match_payload);
    }

    #[test]
    fn custom_empty_payload_matches_exactly() {
        let request = resolve_ping_request(1, 56, PingRequest::new(2).payload(Vec::new()));

        assert_eq!(request.sequence, 2);
        assert!(request.payload.is_empty());
        assert!(request.match_payload);
    }

    #[test]
    fn ping_summary_counts_successes_and_rtts() {
        let attempts = vec![
            successful_attempt(1, Duration::from_millis(10)),
            PingAttempt {
                sequence: 2,
                result: Err(Error::Timeout),
            },
            successful_attempt(3, Duration::from_millis(30)),
        ];

        let summary = PingSummary::from_attempts(&attempts);

        assert_eq!(summary.transmitted, 3);
        assert_eq!(summary.received, 2);
        assert!((summary.loss - (100.0 / 3.0)).abs() < 1e-12);
        assert_eq!(summary.min_rtt, Some(Duration::from_millis(10)));
        assert_eq!(summary.avg_rtt, Some(Duration::from_millis(20)));
        assert_eq!(summary.max_rtt, Some(Duration::from_millis(30)));
    }

    #[test]
    fn empty_ping_summary_has_no_rtts() {
        let summary = PingSummary::from_attempts(&[]);

        assert_eq!(summary.transmitted, 0);
        assert_eq!(summary.received, 0);
        assert_eq!(summary.loss, 0.0);
        assert_eq!(summary.min_rtt, None);
        assert_eq!(summary.avg_rtt, None);
        assert_eq!(summary.max_rtt, None);
    }

    #[test]
    fn socket_addr_without_port_preserves_ipv6_scope() {
        let addr = "[fe80::1%4]:1234".parse().unwrap();

        assert_eq!(socket_addr_without_port(addr).to_string(), "[fe80::1%4]:0");
    }

    fn successful_attempt(sequence: u16, rtt: Duration) -> PingAttempt {
        PingAttempt {
            sequence,
            result: Ok(PingResult {
                reply: EchoReply {
                    ttl: Some(64),
                    source: IpAddr::V4(Ipv4Addr::LOCALHOST),
                    sequence,
                    identifier: 1,
                    payload_len: 0,
                    payload: Vec::new(),
                    #[allow(deprecated)]
                    size: 0,
                },
                rtt,
                socket_type: SocketType::Dgram,
            }),
        }
    }
}
