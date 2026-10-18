//! Client-IP derivation for the HTTP surface (design D1/D2).
//!
//! The TCP peer is the trust root: forwarded headers are read **only** when the
//! peer itself is loopback, i.e. when a same-host reverse proxy sat in between.
//! The derived value is used for identification (throttle key, per-IP series,
//! logs) and never for authentication — `auth_required` keeps raw `peer`.

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

const XFF: &str = "x-forwarded-for";
const X_REAL_IP: &str = "x-real-ip";

/// Where a resolved client IP came from. `label` feeds Prometheus label values
/// and log fields; the variant set is deliberately disjoint from anything
/// `auth_required` can consume (design D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IpSource {
    Peer,
    XForwardedFor,
    XRealIp,
}

impl IpSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            IpSource::Peer => "peer",
            IpSource::XForwardedFor => "forwarded",
            IpSource::XRealIp => "real_ip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClientIp {
    pub(crate) ip: IpAddr,
    pub(crate) source: IpSource,
}

/// Whether the request carries a header that *could* have identified the client.
/// Callers use it to decide if a `Peer` fallback counts as a derivation failure
/// (an ordinary loopback request without proxy headers is not a failure).
pub(crate) fn has_proxy_header(headers: &HeaderMap) -> bool {
    headers.contains_key(XFF) || headers.contains_key(X_REAL_IP)
}

/// Split `X-Forwarded-For` into its hops; rightmost is the most trustworthy.
pub(crate) fn forward_chain(xff: &str) -> impl Iterator<Item = &str> {
    xff.split(',').map(str::trim)
}

/// Resolve the client IP for identification. `trust_forwarded` must already be
/// `trust_proxy_headers && peer_is_loopback(peer)` — this function adds no trust.
pub(crate) fn resolve_client_ip(
    peer: SocketAddr,
    headers: &HeaderMap,
    trust_forwarded: bool,
) -> ClientIp {
    let fallback = ClientIp {
        ip: peer.ip(),
        source: IpSource::Peer,
    };
    if !trust_forwarded {
        return fallback;
    }
    if let Some(ip) = headers
        .get(XFF)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| forward_chain(raw).filter_map(parse_usable).last())
    {
        return ClientIp {
            ip,
            source: IpSource::XForwardedFor,
        };
    }
    if let Some(ip) = headers
        .get(X_REAL_IP)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<IpAddr>().ok())
    {
        return ClientIp {
            ip,
            source: IpSource::XRealIp,
        };
    }
    fallback
}

/// A chain entry counts only when it is a bare IP and is not loopback: a
/// loopback hop is a proxy talking to itself, never the client.
fn parse_usable(entry: &str) -> Option<IpAddr> {
    let ip: IpAddr = entry.parse().ok()?;
    (!is_loopback_ip(ip)).then_some(ip)
}

/// The crate's single loopback predicate (v4, v6 and v4-mapped v6).
pub(crate) fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    const PROXY: &str = "127.0.0.1:41000";
    const REMOTE: &str = "203.0.113.7:41000";

    fn peer(value: &str) -> SocketAddr {
        value.parse().expect("socket address")
    }

    fn headers(pairs: &[(&str, &str)]) -> axum::http::HeaderMap {
        let mut map = axum::http::HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                value.parse().expect("header value"),
            );
        }
        map
    }

    fn xff(value: &str) -> axum::http::HeaderMap {
        headers(&[("x-forwarded-for", value)])
    }

    fn ip(value: &str) -> std::net::IpAddr {
        value.parse().expect("ip address")
    }

    #[test]
    fn trust_disabled_never_reads_any_header() {
        let h = headers(&[
            ("x-forwarded-for", "198.51.100.1"),
            ("x-real-ip", "198.51.100.2"),
        ]);
        let got = resolve_client_ip(peer(REMOTE), &h, false);
        assert_eq!(got.ip, ip("203.0.113.7"));
        assert_eq!(got.source, IpSource::Peer);
    }

    #[test]
    fn single_forwarded_value_is_the_client() {
        let got = resolve_client_ip(peer(PROXY), &xff("198.51.100.7"), true);
        assert_eq!(got.ip, ip("198.51.100.7"));
        assert_eq!(got.source, IpSource::XForwardedFor);
    }

    #[test]
    fn chain_takes_the_rightmost_non_loopback_hop() {
        let got = resolve_client_ip(peer(PROXY), &xff("198.51.100.7, 203.0.113.9"), true);
        assert_eq!(got.ip, ip("203.0.113.9"));
        assert_eq!(got.source, IpSource::XForwardedFor);
    }

    #[test]
    fn spoofed_leftmost_prefix_cannot_change_the_result() {
        // nginx appends to the client-supplied chain, so the rightmost entry is
        // the one the proxy actually observed.
        let got = resolve_client_ip(peer(PROXY), &xff("9.9.9.9, 203.0.113.9"), true);
        assert_eq!(got.ip, ip("203.0.113.9"));
    }

    #[test]
    fn loopback_hops_are_skipped() {
        let got = resolve_client_ip(peer(PROXY), &xff("203.0.113.9, 127.0.0.1, ::1"), true);
        assert_eq!(got.ip, ip("203.0.113.9"));
        assert_eq!(got.source, IpSource::XForwardedFor);
    }

    #[test]
    fn port_bearing_and_garbage_entries_are_skipped() {
        let got = resolve_client_ip(
            peer(PROXY),
            &xff("198.51.100.1:443, unknown, , 203.0.113.9"),
            true,
        );
        assert_eq!(got.ip, ip("203.0.113.9"));
    }

    #[test]
    fn ipv6_entries_are_parsed() {
        let got = resolve_client_ip(peer("[::1]:41000"), &xff("2001:db8::2, ::1"), true);
        assert_eq!(got.ip, ip("2001:db8::2"));
        assert_eq!(got.source, IpSource::XForwardedFor);
    }

    #[test]
    fn x_real_ip_is_the_fallback() {
        let h = headers(&[
            ("x-forwarded-for", "127.0.0.1, ::1"),
            ("x-real-ip", "198.51.100.3"),
        ]);
        let got = resolve_client_ip(peer(PROXY), &h, true);
        assert_eq!(got.ip, ip("198.51.100.3"));
        assert_eq!(got.source, IpSource::XRealIp);
    }

    #[test]
    fn all_loopback_chain_falls_back_to_the_peer() {
        let got = resolve_client_ip(peer(PROXY), &xff("127.0.0.1, ::1"), true);
        assert_eq!(got.ip, ip("127.0.0.1"));
        assert_eq!(got.source, IpSource::Peer);
    }

    #[test]
    fn v4_mapped_loopback_is_not_a_client() {
        let got = resolve_client_ip(peer(PROXY), &xff("::ffff:127.0.0.1, 203.0.113.9"), true);
        assert_eq!(got.ip, ip("203.0.113.9"));
    }

    #[test]
    fn has_proxy_header_detects_either_header() {
        assert!(has_proxy_header(&xff("1.2.3.4")));
        assert!(has_proxy_header(&headers(&[("x-real-ip", "1.2.3.4")])));
        assert!(!has_proxy_header(&headers(&[("x-other", "1.2.3.4")])));
    }
}
