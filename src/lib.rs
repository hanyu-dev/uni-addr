#![doc = include_str!("../README.md")]
#![allow(clippy::must_use_candidate)]

use std::borrow::Cow;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::str::FromStr;
use std::sync::Arc;
use std::{fmt, io};

#[cfg(unix)]
pub mod unix;

/// The prefix for Unix domain socket URIs.
///
/// - `unix:///path/to/socket` for a pathname socket address.
/// - `unix://@abstract.unix.socket` for an abstract socket address.
pub const UNIX_URI_PREFIX: &str = "unix://";

wrapper_lite::wrapper!(
    #[wrapper_impl(Debug)]
    #[wrapper_impl(Display)]
    #[wrapper_impl(AsRef)]
    #[wrapper_impl(Deref)]
    #[repr(align(cache))]
    #[derive(Clone, PartialEq, Eq, Hash)]
    /// A unified address type that can represent:
    ///
    /// - [`std::net::SocketAddr`]
    /// - [`unix::SocketAddr`] (a wrapper over
    ///   [`std::os::unix::net::SocketAddr`])
    /// - A host name with port. See [`ToSocketAddrs`].
    ///
    /// # Parsing Behaviour
    ///
    /// - Checks if the address started with [`UNIX_URI_PREFIX`]: parse as a UDS
    ///   address.
    /// - Checks if the address is started with a alphabetic character (a-z,
    ///   A-Z): treat as a host name. Notes that we will not validate if the
    ///   host name is valid.
    /// - Tries to parse as a network socket address.
    /// - Otherwise, treats the input as a host name.
    pub struct UniAddr(UniAddrInner);
);

impl From<SocketAddr> for UniAddr {
    fn from(addr: SocketAddr) -> Self {
        UniAddr::from_inner(UniAddrInner::Inet(addr))
    }
}

#[cfg(unix)]
impl From<std::os::unix::net::SocketAddr> for UniAddr {
    fn from(addr: std::os::unix::net::SocketAddr) -> Self {
        UniAddr::from_inner(UniAddrInner::Unix(addr.into()))
    }
}

#[cfg(all(unix, feature = "feat-tokio"))]
impl From<tokio::net::unix::SocketAddr> for UniAddr {
    fn from(addr: tokio::net::unix::SocketAddr) -> Self {
        UniAddr::from_inner(UniAddrInner::Unix(unix::SocketAddr::from(addr.into())))
    }
}

#[cfg(feature = "feat-socket2")]
impl TryFrom<socket2::SockAddr> for UniAddr {
    type Error = io::Error;

    fn try_from(addr: socket2::SockAddr) -> Result<Self, Self::Error> {
        UniAddr::try_from(&addr)
    }
}

#[cfg(feature = "feat-socket2")]
impl TryFrom<&socket2::SockAddr> for UniAddr {
    type Error = io::Error;

    fn try_from(addr: &socket2::SockAddr) -> Result<Self, Self::Error> {
        if let Some(addr) = addr.as_socket() {
            return Ok(Self::from(addr));
        }

        #[cfg(unix)]
        if let Some(addr) = addr.as_unix() {
            return Ok(Self::from(addr));
        }

        #[cfg(any(target_os = "android", target_os = "linux", target_os = "cygwin"))]
        if let Some(addr) = addr.as_abstract_namespace() {
            return crate::unix::SocketAddr::new_abstract(addr).map(Self::from);
        }

        Err(io::Error::new(
            io::ErrorKind::Other,
            "unsupported address type",
        ))
    }
}

#[cfg(feature = "feat-socket2")]
impl TryFrom<UniAddr> for socket2::SockAddr {
    type Error = io::Error;

    fn try_from(addr: UniAddr) -> Result<Self, Self::Error> {
        socket2::SockAddr::try_from(&addr)
    }
}

#[cfg(feature = "feat-socket2")]
impl TryFrom<&UniAddr> for socket2::SockAddr {
    type Error = io::Error;

    fn try_from(addr: &UniAddr) -> Result<Self, Self::Error> {
        match &addr.inner {
            UniAddrInner::Inet(addr) => Ok(socket2::SockAddr::from(*addr)),
            #[cfg(unix)]
            UniAddrInner::Unix(addr) => socket2::SockAddr::unix(addr.to_os_string()),
            UniAddrInner::Host(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "The host name address must be resolved before converting to SockAddr",
            )),
        }
    }
}

#[cfg(unix)]
impl From<crate::unix::SocketAddr> for UniAddr {
    fn from(addr: crate::unix::SocketAddr) -> Self {
        UniAddr::from_inner(UniAddrInner::Unix(addr))
    }
}

impl FromStr for UniAddr {
    type Err = ParseError;

    fn from_str(addr: &str) -> Result<Self, Self::Err> {
        Self::new(addr)
    }
}

#[cfg(feature = "feat-serde")]
impl serde::Serialize for UniAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_str())
    }
}

#[cfg(feature = "feat-serde")]
impl<'de> serde::Deserialize<'de> for UniAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl UniAddr {
    #[inline]
    /// Creates a new [`UniAddr`] from its string representation.
    ///
    /// # Errors
    ///
    /// Not a valid address string.
    pub fn new(addr: &str) -> Result<Self, ParseError> {
        if addr.is_empty() {
            return Err(ParseError::Empty);
        }

        #[cfg(unix)]
        if let Some(addr) = addr.strip_prefix(UNIX_URI_PREFIX) {
            return unix::SocketAddr::new(addr)
                .map(UniAddrInner::Unix)
                .map(Self::from_inner)
                .map_err(ParseError::InvalidUDSAddress);
        }

        #[cfg(not(unix))]
        if let Some(_addr) = addr.strip_prefix(UNIX_URI_PREFIX) {
            return Err(ParseError::Unsupported);
        }

        let Some((host, port)) = addr.rsplit_once(':') else {
            return Err(ParseError::InvalidPort);
        };

        let Ok(port) = port.parse::<u16>() else {
            return Err(ParseError::InvalidPort);
        };

        // Short-circuit: IPv4 address starts with a digit.
        if host.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Ipv4Addr::from_str(host)
                .map(|ip| SocketAddr::V4(SocketAddrV4::new(ip, port)))
                .map(UniAddrInner::Inet)
                .map(Self::from_inner)
                .map_err(|_| ParseError::InvalidHost)
                .or_else(|_| {
                    // A host name may also start with a digit.
                    Self::new_host(addr, Some((host, port)))
                });
        }

        // Short-circuit: if starts with '[' and ends with ']', may be an IPv6 address
        // and can never be a host.
        if let Some(ipv6_addr) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            return Ipv6Addr::from_str(ipv6_addr)
                .map(|ip| SocketAddr::V6(SocketAddrV6::new(ip, port, 0, 0)))
                .map(UniAddrInner::Inet)
                .map(Self::from_inner)
                .map_err(|_| ParseError::InvalidHost);
        }

        // Fallback: check if is a valid host name.
        Self::new_host(addr, Some((host, port)))
    }

    /// Creates a new [`UniAddr`] from a string containing a host name and port,
    /// like `example.com:8080`.
    ///
    /// # Errors
    ///
    /// - [`ParseError::InvalidHost`] if the host name is invalid.
    /// - [`ParseError::InvalidPort`] if the port is invalid.
    pub fn new_host(addr: &str, parsed: Option<(&str, u16)>) -> Result<Self, ParseError> {
        let (hostname, _port) = match parsed {
            Some((hostname, port)) => (hostname, port),
            None => addr
                .rsplit_once(':')
                .ok_or(ParseError::InvalidPort)
                .and_then(|(hostname, port)| {
                    let Ok(port) = port.parse::<u16>() else {
                        return Err(ParseError::InvalidPort);
                    };

                    Ok((hostname, port))
                })?,
        };

        Self::validate_host_name(hostname.as_bytes()).map_err(|()| ParseError::InvalidHost)?;

        Ok(Self::from_inner(UniAddrInner::Host(Arc::from(addr))))
    }

    // https://github.com/rustls/pki-types/blob/b8c04aa6b7a34875e2c4a33edc9b78d31da49523/src/server_name.rs
    const fn validate_host_name(input: &[u8]) -> Result<(), ()> {
        enum State {
            Start,
            Next,
            NumericOnly { len: usize },
            NextAfterNumericOnly,
            Subsequent { len: usize },
            Hyphen { len: usize },
        }

        use State::{Hyphen, Next, NextAfterNumericOnly, NumericOnly, Start, Subsequent};

        /// "Labels must be 63 characters or less."
        const MAX_LABEL_LENGTH: usize = 63;

        /// <https://devblogs.microsoft.com/oldnewthing/20120412-00/?p=7873>
        const MAX_NAME_LENGTH: usize = 253;

        let mut state = Start;

        if input.len() > MAX_NAME_LENGTH {
            return Err(());
        }

        let mut idx = 0;
        while idx < input.len() {
            let ch = input[idx];
            state = match (state, ch) {
                (Start | Next | NextAfterNumericOnly | Hyphen { .. }, b'.') => {
                    return Err(());
                }
                (Subsequent { .. }, b'.') => Next,
                (NumericOnly { .. }, b'.') => NextAfterNumericOnly,
                (Subsequent { len } | NumericOnly { len } | Hyphen { len }, _)
                    if len >= MAX_LABEL_LENGTH =>
                {
                    return Err(());
                }
                (Start | Next | NextAfterNumericOnly, b'0'..=b'9') => NumericOnly { len: 1 },
                (NumericOnly { len }, b'0'..=b'9') => NumericOnly { len: len + 1 },
                (Start | Next | NextAfterNumericOnly, b'a'..=b'z' | b'A'..=b'Z' | b'_') => {
                    Subsequent { len: 1 }
                }
                (Subsequent { len } | NumericOnly { len } | Hyphen { len }, b'-') => {
                    Hyphen { len: len + 1 }
                }
                (
                    Subsequent { len } | NumericOnly { len } | Hyphen { len },
                    b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'0'..=b'9',
                ) => Subsequent { len: len + 1 },
                _ => return Err(()),
            };
            idx += 1;
        }

        if matches!(
            state,
            Start | Hyphen { .. } | NumericOnly { .. } | NextAfterNumericOnly | Next
        ) {
            return Err(());
        }

        Ok(())
    }

    #[inline]
    /// Serializes the address to a string.
    pub fn to_str(&self) -> Cow<'_, str> {
        self.as_inner().to_str()
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// See [`UniAddr`].
///
/// Generally, you should use [`UniAddr`] instead of this type directly, as
/// we expose this type only for easier pattern matching. A valid [`UniAddr`]
/// can be constructed only through [`FromStr`] implementation.
pub enum UniAddrInner {
    /// See [`SocketAddr`].
    Inet(SocketAddr),

    #[cfg(unix)]
    /// See [`SocketAddr`](crate::unix::SocketAddr).
    Unix(crate::unix::SocketAddr),

    /// A host name with port. See [`ToSocketAddrs`](std::net::ToSocketAddrs).
    Host(Arc<str>),
}

impl fmt::Display for UniAddrInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_str().fmt(f)
    }
}

impl UniAddrInner {
    #[inline]
    /// Serializes the address to a string.
    pub fn to_str(&self) -> Cow<'_, str> {
        match self {
            Self::Inet(addr) => addr.to_string().into(),
            #[cfg(unix)]
            Self::Unix(addr) => addr
                .to_os_string_impl(UNIX_URI_PREFIX, "@")
                .to_string_lossy()
                .to_string()
                .into(),
            Self::Host(host) => Cow::Borrowed(host),
        }
    }
}

#[derive(Debug)]
/// Errors that can occur when parsing a [`UniAddr`] from a string.
pub enum ParseError {
    /// Empty input string
    Empty,

    /// Invalid or missing hostname, or an invalid Ipv4 / IPv6 address
    InvalidHost,

    /// Invalid address format: missing or invalid port
    InvalidPort,

    /// Invalid UDS address format
    InvalidUDSAddress(io::Error),

    /// Unsupported address type on this platform
    Unsupported,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty address string"),
            Self::InvalidHost => write!(f, "invalid or missing host address"),
            Self::InvalidPort => write!(f, "invalid or missing port"),
            Self::InvalidUDSAddress(err) => write!(f, "invalid UDS address: {err}"),
            Self::Unsupported => write!(f, "unsupported address type on this platform"),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUDSAddress(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ParseError> for io::Error {
    fn from(value: ParseError) -> Self {
        io::Error::new(io::ErrorKind::Other, value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("0.0.0.0:0")]
    #[case("0.0.0.0:8080")]
    #[case("127.0.0.1:0")]
    #[case("127.0.0.1:8080")]
    #[case("[::]:0")]
    #[case("[::]:8080")]
    #[case("[::1]:0")]
    #[case("[::1]:8080")]
    #[case("example.com:8080")]
    #[case("1example.com:8080")]
    #[cfg_attr(unix, case("unix://"))]
    #[cfg_attr(
        any(target_os = "android", target_os = "linux", target_os = "cygwin"),
        case("unix://@")
    )]
    #[cfg_attr(unix, case("unix:///tmp/test_UniAddr_new_Display.socket"))]
    #[cfg_attr(
        any(target_os = "android", target_os = "linux", target_os = "cygwin"),
        case("unix://@test_UniAddr_new_Display.socket")
    )]
    fn test_UniAddr_new_Display(#[case] addr: &str) {
        let addr_displayed = UniAddr::new(addr).unwrap().to_string();

        assert_eq!(
            addr_displayed, addr,
            "addr_displayed {addr_displayed:?} != {addr:?}"
        );
    }

    #[rstest]
    #[case("example.com:8080")]
    #[case("1example.com:8080")]
    #[should_panic]
    #[case::panic("1example.com")]
    #[should_panic]
    #[case::panic("1example.com.")]
    #[should_panic]
    #[case::panic("1example.com.:14514")]
    #[should_panic]
    #[case::panic("1example.com:1919810")]
    #[should_panic]
    #[case::panic("this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name-this-is-a-long-host-name:19810")]
    fn test_UniAddr_new_host(#[case] addr: &str) {
        let addr_displayed = UniAddr::new_host(addr, None).unwrap().to_string();

        assert_eq!(
            addr_displayed, addr,
            "addr_displayed {addr_displayed:?} != {addr:?}"
        );
    }

    #[rstest]
    #[should_panic]
    #[case::panic("")]
    #[should_panic]
    #[case::panic("not-an-address")]
    #[should_panic]
    #[case::panic("127.0.0.1")]
    #[should_panic]
    #[case::panic("127.0.0.1:99999")]
    #[should_panic]
    #[case::panic("127.0.0.256:99999")]
    #[should_panic]
    #[case::panic("::1")]
    #[should_panic]
    #[case::panic("[::1]")]
    #[should_panic]
    #[case::panic("[::1]:99999")]
    #[should_panic]
    #[case::panic("[::gg]:99999")]
    #[should_panic]
    #[case::panic("example.com")]
    #[should_panic]
    #[case::panic("example.com:99999")]
    #[should_panic]
    #[case::panic("examp😀le.com:99999")]
    fn test_UniAddr_new_invalid(#[case] addr: &str) {
        let _ = UniAddr::new(addr).unwrap();
    }

    #[cfg(not(unix))]
    #[test]
    fn test_UniAddr_new_unsupported() {
        // Unix sockets should be unsupported on non-Unix platforms
        let result = UniAddr::new("unix:///tmp/test.sock");

        assert!(matches!(result.unwrap_err(), ParseError::Unsupported));
    }

    #[rstest]
    #[case("0.0.0.0:0")]
    #[case("0.0.0.0:8080")]
    #[case("127.0.0.1:0")]
    #[case("127.0.0.1:8080")]
    #[case("[::]:0")]
    #[case("[::]:8080")]
    #[case("[::1]:0")]
    #[case("[::1]:8080")]
    #[cfg_attr(unix, case("unix:///tmp/test_socket2_sock_addr_conversion.socket"))]
    #[cfg_attr(
        any(target_os = "android", target_os = "linux", target_os = "cygwin"),
        case("unix://@test_socket2_sock_addr_conversion.socket")
    )]
    fn test_socket2_SockAddr_conversion(#[case] addr: &str) {
        let uni_addr = UniAddr::new(addr).unwrap();
        let sock_addr = socket2::SockAddr::try_from(&uni_addr).unwrap();
        let uni_addr_converted = UniAddr::try_from(sock_addr).unwrap();

        assert_eq!(
            uni_addr, uni_addr_converted,
            "{uni_addr} != {uni_addr_converted}"
        );
    }
}
