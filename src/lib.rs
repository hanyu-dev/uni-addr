//! # uni-addr

use std::borrow::Cow;
use std::str::FromStr;
use std::sync::Arc;
use std::{fmt, io};

pub mod listener;
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

impl From<std::net::SocketAddr> for UniAddr {
    fn from(addr: std::net::SocketAddr) -> Self {
        UniAddr::const_from(UniAddrInner::Inet(addr))
    }
}

#[cfg(unix)]
impl From<unix::SocketAddr> for UniAddr {
    fn from(addr: unix::SocketAddr) -> Self {
        UniAddr::const_from(UniAddrInner::Unix(addr))
    }
}

#[cfg(all(unix, feature = "feat-tokio"))]
impl From<tokio::net::unix::SocketAddr> for UniAddr {
    fn from(addr: tokio::net::unix::SocketAddr) -> Self {
        UniAddr::const_from(UniAddrInner::Unix(unix::SocketAddr::from(addr.into())))
    }
}

impl FromStr for UniAddr {
    type Err = ParseError;

    fn from_str(addr: &str) -> Result<Self, Self::Err> {
        if addr.is_empty() {
            return Err(ParseError::Empty);
        }

        if let Some(addr) = addr.strip_prefix(UNIX_URI_PREFIX) {
            #[cfg(unix)]
            {
                return unix::SocketAddr::new(addr)
                    .map(UniAddrInner::Unix)
                    .map(Self::const_from)
                    .map_err(ParseError::InvalidUDSAddress);
            }

            #[cfg(not(unix))]
            {
                return Err(ParseError::Unsupported);
            }
        }

        let Some((host, port)) = addr.rsplit_once(':') else {
            return Err(ParseError::InvalidPort);
        };

        {
            let Some(char) = host.chars().next() else {
                return Err(ParseError::InvalidHost);
            };

            if char.is_ascii_alphabetic() {
                if port.parse::<u16>().is_err() {
                    return Err(ParseError::InvalidPort);
                }

                return Ok(Self::const_from(UniAddrInner::Host(Arc::from(addr))));
            }
        }

        if let Ok(addr) = addr.parse::<std::net::SocketAddr>() {
            return Ok(Self::const_from(UniAddrInner::Inet(addr)));
        }

        if port.parse::<u16>().is_err() {
            return Err(ParseError::InvalidPort);
        }

        Ok(Self::const_from(UniAddrInner::Host(Arc::from(addr))))
    }
}

#[derive(Debug)]
/// Errors that can occur when parsing a [`UniAddr`] from a string.
pub enum ParseError {
    /// Empty input string
    Empty,

    /// Missing host address
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
            Self::InvalidUDSAddress(err) => write!(f, "invalid UDS address: {}", err),
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
        Self::new(<&str>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl UniAddr {
    #[inline]
    /// Creates a new [`UniAddr`] from its string representation.
    pub fn new(addr: &str) -> Result<Self, ParseError> {
        addr.parse()
    }

    #[inline]
    /// Serializes the address to a string.
    pub fn to_str(&self) -> Cow<'_, str> {
        match self.as_inner() {
            UniAddrInner::Inet(addr) => addr.to_string().into(),
            UniAddrInner::Unix(addr) => addr
                ._to_os_string(UNIX_URI_PREFIX, "@")
                .to_string_lossy()
                .to_string()
                .into(),
            UniAddrInner::Host(host) => (&**host).into(),
        }
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
    /// See [`std::net::SocketAddr`].
    Inet(std::net::SocketAddr),

    #[cfg(unix)]
    /// See [`unix::SocketAddr`].
    Unix(unix::SocketAddr),

    /// A host name with port. See [`ToSocketAddrs`](std::net::ToSocketAddrs).
    Host(Arc<str>),
}

impl fmt::Display for UniAddrInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inet(addr) => addr.fmt(f),
            #[cfg(unix)]
            Self::Unix(addr) => write!(f, "{}", addr._to_os_string(UNIX_URI_PREFIX, "@").to_string_lossy()),
            Self::Host(host) => host.fmt(f),
        }
    }
}

#[deprecated(since = "0.2.4", note = "Please use `UniAddr` instead")]
#[derive(Clone, PartialEq, Eq, Hash)]
/// A unified address type that can represent both
/// [`std::net::SocketAddr`] and [`unix::SocketAddr`] (a wrapper over
/// [`std::os::unix::net::SocketAddr`]).
///
/// ## Notes
///
/// For Unix domain sockets addresses, serialization/deserialization will be
/// performed in URI format (see [`UNIX_URI_PREFIX`]), which is different from
/// [`unix::SocketAddr`]'s serialization/deserialization behaviour.
pub enum SocketAddr {
    /// See [`std::net::SocketAddr`].
    Inet(std::net::SocketAddr),

    #[cfg(unix)]
    /// See [`unix::SocketAddr`].
    Unix(unix::SocketAddr),
}

#[allow(deprecated)]
impl fmt::Debug for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketAddr::Inet(addr) => addr.fmt(f),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => addr.fmt(f),
        }
    }
}

#[allow(deprecated)]
impl From<std::net::SocketAddr> for SocketAddr {
    fn from(addr: std::net::SocketAddr) -> Self {
        SocketAddr::Inet(addr)
    }
}

#[allow(deprecated)]
#[cfg(unix)]
impl From<unix::SocketAddr> for SocketAddr {
    fn from(addr: unix::SocketAddr) -> Self {
        SocketAddr::Unix(addr)
    }
}

#[allow(deprecated)]
#[cfg(all(unix, feature = "feat-tokio"))]
impl From<tokio::net::unix::SocketAddr> for SocketAddr {
    fn from(addr: tokio::net::unix::SocketAddr) -> Self {
        SocketAddr::Unix(unix::SocketAddr::from(addr.into()))
    }
}

#[allow(deprecated)]
impl FromStr for SocketAddr {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SocketAddr::new(s)
    }
}

#[allow(deprecated)]
impl SocketAddr {
    #[inline]
    /// Creates a new [`SocketAddr`] from its string representation.
    ///
    /// The string can be in one of the following formats:
    ///
    /// - Network socket address: `"127.0.0.1:8080"`, `"[::1]:8080"`
    /// - Unix domain socket (pathname): `"unix:///run/listen.sock"`
    /// - Unix domain socket (abstract): `"unix://@abstract.unix.socket"`
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use uni_addr::SocketAddr;
    /// // Network addresses
    /// let addr_v4 = SocketAddr::new("127.0.0.1:8080").unwrap();
    /// let addr_v6 = SocketAddr::new("[::1]:8080").unwrap();
    ///
    /// // Unix domain sockets
    /// let addr_unix_filename = SocketAddr::new("unix:///run/listen.sock").unwrap();
    /// let addr_unix_abstract = SocketAddr::new("unix://@abstract.unix.socket").unwrap();
    /// ```
    ///
    /// See [`unix::SocketAddr::new`] for more details on Unix socket address
    /// formats.
    pub fn new(addr: &str) -> io::Result<Self> {
        if let Some(addr) = addr.strip_prefix(UNIX_URI_PREFIX) {
            #[cfg(unix)]
            return unix::SocketAddr::new(addr).map(SocketAddr::Unix);

            #[cfg(not(unix))]
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unix socket addresses are not supported on this platform",
            ));
        }

        addr.parse()
            .map(SocketAddr::Inet)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "unknown format"))
    }

    #[inline]
    /// Binds a standard (TCP) listener to the address.
    pub fn bind_std(&self) -> io::Result<listener::StdListener> {
        match self {
            SocketAddr::Inet(addr) => std::net::TcpListener::bind(addr).map(listener::StdListener::Tcp),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => addr.bind_std().map(listener::StdListener::Unix),
        }
    }

    #[cfg(feature = "feat-tokio")]
    #[inline]
    /// Binds a Tokio (TCP) listener to the address.
    pub async fn bind(&self) -> io::Result<listener::Listener> {
        match self {
            SocketAddr::Inet(addr) => tokio::net::TcpListener::bind(addr).await.map(listener::Listener::Tcp),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => addr.bind().map(listener::Listener::Unix),
        }
    }

    /// Serializes the address to a `String`.
    pub fn to_string_ext(&self) -> Option<String> {
        match self {
            Self::Inet(addr) => Some(addr.to_string()),
            Self::Unix(addr) => addr._to_os_string(UNIX_URI_PREFIX, "@").into_string().ok(),
        }
    }
}

#[allow(deprecated)]
#[cfg(feature = "feat-serde")]
impl serde::Serialize for SocketAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(
            &self
                .to_string_ext()
                .ok_or_else(|| serde::ser::Error::custom("invalid UTF-8"))?,
        )
    }
}

#[allow(deprecated)]
#[cfg(feature = "feat-serde")]
impl<'de> serde::Deserialize<'de> for SocketAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(<&str>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::linux::net::SocketAddrExt;

    use super::*;

    #[test]
    fn test_socket_addr_new_ipv4() {
        let addr = UniAddr::new("127.0.0.1:8080").unwrap();

        match addr.as_inner() {
            UniAddrInner::Inet(std_addr) => {
                assert_eq!(std_addr.ip().to_string(), "127.0.0.1");
                assert_eq!(std_addr.port(), 8080);
            }
            _ => panic!("Expected Inet address, got {:?}", addr),
        }
    }

    #[test]
    fn test_socket_addr_new_ipv6() {
        let addr = UniAddr::new("[::1]:8080").unwrap();

        match addr.as_inner() {
            UniAddrInner::Inet(std_addr) => {
                assert_eq!(std_addr.ip().to_string(), "::1");
                assert_eq!(std_addr.port(), 8080);
            }
            #[cfg(unix)]
            _ => unreachable!(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_addr_new_unix_pathname() {
        let addr = UniAddr::new("unix:///tmp/test.sock").unwrap();

        match addr.as_inner() {
            UniAddrInner::Unix(unix_addr) => {
                assert!(unix_addr.as_pathname().is_some());
            }
            _ => unreachable!(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_addr_new_unix_abstract() {
        let addr = UniAddr::new("unix://@test.abstract").unwrap();

        match addr.as_inner() {
            UniAddrInner::Unix(unix_addr) => {
                assert!(unix_addr.as_abstract_name().is_some());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_socket_addr_new_host() {
        let addr = UniAddr::new("example.com:8080").unwrap();

        match addr.as_inner() {
            UniAddrInner::Host(host) => {
                assert_eq!(&**host, "example.com:8080");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_socket_addr_new_invalid() {
        // Invalid format
        assert!(UniAddr::new("invalid").is_err());
        assert!(UniAddr::new("127.0.0.1").is_err()); // Missing port
        assert!(UniAddr::new("example.com:invalid").is_err()); // Invalid port
        assert!(UniAddr::new("127.0.0.1:invalid").is_err()); // Invalid port
    }

    #[cfg(not(unix))]
    #[test]
    fn test_socket_addr_new_unix_unsupported() {
        // Unix sockets should be unsupported on non-Unix platforms
        let result = UniAddr::new("unix:///tmp/test.sock");

        assert!(matches!(result.unwrap_err(), ParseError::Unsupported));
    }

    #[test]
    fn test_socket_addr_display() {
        let addr = UniAddr::new("127.0.0.1:8080").unwrap();
        assert_eq!(&addr.to_str(), "127.0.0.1:8080");

        let addr = UniAddr::new("[::1]:8080").unwrap();
        assert_eq!(&addr.to_str(), "[::1]:8080");

        #[cfg(unix)]
        {
            let addr = UniAddr::new("unix:///tmp/test.sock").unwrap();
            assert_eq!(&addr.to_str(), "unix:///tmp/test.sock");

            let addr = UniAddr::new("unix://@test.abstract").unwrap();
            assert_eq!(&addr.to_str(), "unix://@test.abstract");
        }

        let addr = UniAddr::new("example.com:8080").unwrap();
        assert_eq!(&addr.to_str(), "example.com:8080");
    }

    #[test]
    fn test_socket_addr_debug() {
        let addr = UniAddr::new("127.0.0.1:8080").unwrap();
        let debug_str = format!("{:?}", addr);

        assert!(debug_str.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_edge_cases() {
        assert!(UniAddr::new("").is_err());
        assert!(UniAddr::new("not-an-address").is_err());
        assert!(UniAddr::new("127.0.0.1:99999").is_err()); // Port too high

        #[cfg(unix)]
        {
            assert!(UniAddr::new("unix://").is_ok()); // Empty path -> unnamed one
            #[cfg(any(target_os = "android", target_os = "linux"))]
            assert!(UniAddr::new("unix://@").is_ok()); // Empty abstract one
        }
    }
}
