use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
    time::Duration,
};

use tiny_ping::{PingRequest, PingSeries, Pinger, SocketType};

#[test]
fn raw_loopback_ping_when_enabled() {
    if std::env::var_os("TINY_PING_RUN_RAW_TESTS").is_none() {
        return;
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut pinger = Pinger::new("127.0.0.1".parse::<IpAddr>().unwrap()).unwrap();
        pinger.timeout(Duration::from_secs(1));
        let result = pinger.ping(1).await.unwrap();

        assert_eq!(result.socket_type, SocketType::Raw);
        assert_eq!(result.reply.sequence, 1);
    });
}

#[test]
fn dgram_loopback_ping_when_enabled_and_supported() {
    if std::env::var_os("TINY_PING_RUN_NET_TESTS").is_none() {
        return;
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let Ok(mut pinger) =
            Pinger::with_socket_type("127.0.0.1".parse::<IpAddr>().unwrap(), SocketType::Dgram)
        else {
            return;
        };

        pinger.timeout(Duration::from_secs(1));
        let result = pinger.ping(1).await.unwrap();

        assert_eq!(result.socket_type, SocketType::Dgram);
        assert_eq!(result.reply.sequence, 1);

        let replies = pinger.ping_replies(2).await.unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].socket_type, SocketType::Dgram);
        assert_eq!(replies[0].reply.sequence, 2);

        let custom = pinger
            .ping(PingRequest::new(3).payload(b"network-test".as_slice()))
            .await
            .unwrap();
        assert_eq!(custom.reply.sequence, 3);
        assert_eq!(custom.reply.payload, b"network-test");

        let series = pinger
            .ping_many(PingSeries::new(4, 2).interval(Duration::from_millis(1)))
            .await;
        assert_eq!(series.attempts.len(), 2);
        assert_eq!(series.summary.transmitted, 2);
        assert_eq!(series.summary.received, 2);
    });
}

#[test]
fn socket_addr_target_ping_when_enabled_and_supported() {
    if std::env::var_os("TINY_PING_RUN_NET_TESTS").is_none() {
        return;
    }

    let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let Ok(mut pinger) = Pinger::with_socket_addr(target, SocketType::Dgram) else {
            return;
        };

        pinger.timeout(Duration::from_secs(1));
        let result = pinger.ping(1).await.unwrap();

        assert_eq!(result.socket_type, SocketType::Dgram);
        assert_eq!(result.reply.sequence, 1);
    });
}

#[test]
fn scoped_ipv6_multicast_ping_when_enabled_and_supported() {
    if std::env::var_os("TINY_PING_RUN_IPV6_MULTICAST_TESTS").is_none() {
        return;
    }

    let Ok(scope_id) = std::env::var("TINY_PING_IPV6_MULTICAST_SCOPE_ID") else {
        return;
    };
    let Ok(scope_id) = scope_id.parse() else {
        return;
    };

    let target = SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::from([0xff02, 0, 0, 0, 0, 0, 0, 1]),
        0,
        0,
        scope_id,
    ));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let Ok(mut pinger) = Pinger::with_socket_addr(target, SocketType::Dgram) else {
            return;
        };

        pinger.timeout(Duration::from_secs(1));
        let replies = pinger.ping_replies(1).await.unwrap();

        assert!(!replies.is_empty());
        println!("received {} IPv6 multicast replies", replies.len());

        for result in replies {
            assert_eq!(result.socket_type, SocketType::Dgram);
            assert_eq!(result.reply.sequence, 1);
            assert!(result.reply.source.is_ipv6());
        }
    });
}
