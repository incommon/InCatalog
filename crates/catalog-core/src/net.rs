// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Which addresses count as the public internet.
//!
//! Every URL in the catalog comes from metadata written by thousands of registrants.
//! The logo fetcher uses this to refuse to fetch anything inside a network, and the
//! Log in button ([`crate::login`]) to refuse to send a browser there.

use std::net::IpAddr;

/// False for private, loopback, link-local (and so the cloud metadata service),
/// carrier-grade NAT, documentation, benchmarking, multicast and reserved addresses.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local() // includes 169.254.169.254, the cloud metadata service
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64.0.0/10, carrier-grade NAT
                || (o[0] == 198 && (o[1] & 0xFE) == 18) // 198.18.0.0/15, benchmarking
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xFE00) == 0xFC00 // unique local
                || (seg[0] & 0xFFC0) == 0xFE80) // link local
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_reserved_addresses_are_not_public() {
        for bad in [
            "10.1.2.3",
            "127.0.0.1",
            "169.254.169.254",
            "192.168.0.1",
            "100.64.1.1",
            "198.18.0.1",
            "0.0.0.0",
            "240.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public(bad.parse().unwrap()), "{bad}");
        }
        for good in ["8.8.8.8", "2001:4860:4860::8888"] {
            assert!(is_public(good.parse().unwrap()), "{good}");
        }
    }
}
