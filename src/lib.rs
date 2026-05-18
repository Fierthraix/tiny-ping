mod error;
mod icmp;
mod packet;
mod ping;
mod socket;

pub use error::Error;
pub use icmp::EchoReply;
pub use ping::{
    PingAttempt, PingRequest, PingResult, PingSeries, PingSeriesResult, PingSummary, Pinger,
    SocketType,
};

#[deprecated(
    since = "0.6.0",
    note = "packet internals are not part of the stable API"
)]
pub use packet::{
    EchoReply as ER1, EchoRequest, ICMP_HEADER_SIZE, IcmpV4, IcmpV6, IpV4Packet, IpV4Protocol,
};
