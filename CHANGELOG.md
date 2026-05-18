# Changelog

All notable user-facing changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

No user-facing changes yet.

## [0.7.0] - 2026-05-18

### Added

- Added `PingRequest` for custom ICMP echo payloads.
- Added `PingSeries`, `PingSeriesResult`, `PingAttempt`, and `PingSummary` for
  repeated ping attempts and summary statistics.
- Added `Pinger::bind_source` and read-only accessors for target, source,
  identifier, default payload size, timeout, and TTL/hop-limit.

## [0.6.2] - 2026-05-15

### Added

- Added support for scoped multicast ping replies.

## [0.6.0] - 2026-05-15

### Added

- Added `PingResult` for returning reply data and round-trip time together.
- Added opt-in datagram socket support with `SocketType::Dgram`.
- Added IPv6 echo request and reply handling.
- Added packet parsing tests and opt-in real network tests.
- Added Linux, macOS, and Windows CI.
- Added crates.io publishing from version tags.

### Changed

- Changed `Pinger::ping` to return `PingResult`.
- Kept `Pinger::new` as the raw-socket default.
- Reduced compile-time dependencies by removing `log`, `parking_lot`, and the
  duplicate old `socket2` dependency.
- Updated the crate to Rust 2024 edition.
- Made errors display useful messages.

## [0.5.0] - 2026-05-15

### Changed

- Replaced the previous ping backend with a small custom async ICMP
  implementation.
- Kept the crate focused on small compile times by avoiding `pnet`, `rand`, and
  proc-macro error dependencies.

### Fixed

- Improved IO error messages and socket dependency compatibility.

## [0.2.0] - 2019-10-27

### Added

- Added the initial ping API.
- Added support for specifying ICMP body, TTL, and timeout.
- Added README documentation and MIT license information.

### Fixed

- Improved ping error handling.
