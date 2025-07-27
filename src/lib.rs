//! # uni-addr

use std::{fmt, io};

pub mod listener;
#[cfg(unix)]
pub mod unix;

/// The prefix for Unix domain socket URIs.
///
/// - `unix:///path/to/socket` for a pathname socket address.
/// - `unix://@abstract.unix.socket` for an abstract socket address.
pub const UNIX_URI_PREFIX: &str = "unix://";

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

impl fmt::Debug for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketAddr::Inet(addr) => addr.fmt(f),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => addr.fmt(f),
        }
    }
}

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
        let addr = SocketAddr::new("127.0.0.1:8080").unwrap();

        match addr {
            SocketAddr::Inet(std_addr) => {
                assert_eq!(std_addr.ip().to_string(), "127.0.0.1");
                assert_eq!(std_addr.port(), 8080);
            }
            #[cfg(unix)]
            SocketAddr::Unix(_) => unreachable!(),
        }
    }

    #[test]
    fn test_socket_addr_new_ipv6() {
        let addr = SocketAddr::new("[::1]:8080").unwrap();

        match addr {
            SocketAddr::Inet(std_addr) => {
                assert_eq!(std_addr.ip().to_string(), "::1");
                assert_eq!(std_addr.port(), 8080);
            }
            #[cfg(unix)]
            SocketAddr::Unix(_) => unreachable!(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_addr_new_unix_pathname() {
        let addr = SocketAddr::new("unix:///tmp/test.sock").unwrap();

        match addr {
            SocketAddr::Inet(_) => unreachable!(),
            SocketAddr::Unix(unix_addr) => {
                assert!(unix_addr.as_pathname().is_some());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_addr_new_unix_abstract() {
        let addr = SocketAddr::new("unix://@test.abstract").unwrap();

        match addr {
            SocketAddr::Inet(_) => unreachable!(),
            SocketAddr::Unix(unix_addr) => {
                assert!(unix_addr.as_abstract_name().is_some());
            }
        }
    }

    #[test]
    fn test_socket_addr_new_invalid() {
        // Invalid format
        assert!(SocketAddr::new("invalid").is_err());
        assert!(SocketAddr::new("127.0.0.1").is_err()); // Missing port
        assert!(SocketAddr::new("127.0.0.1:invalid").is_err()); // Invalid port
    }

    #[cfg(not(unix))]
    #[test]
    fn test_socket_addr_new_unix_unsupported() {
        // Unix sockets should be unsupported on non-Unix platforms
        let result = SocketAddr::new("unix:///tmp/test.sock");

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn test_socket_addr_display() {
        let addr = SocketAddr::new("127.0.0.1:8080").unwrap();
        assert_eq!(&addr.to_string_ext().unwrap(), "127.0.0.1:8080");

        let addr = SocketAddr::new("[::1]:8080").unwrap();
        assert_eq!(&addr.to_string_ext().unwrap(), "[::1]:8080");

        #[cfg(unix)]
        {
            let addr = SocketAddr::new("unix:///tmp/test.sock").unwrap();
            assert_eq!(&addr.to_string_ext().unwrap(), "unix:///tmp/test.sock");

            let addr = SocketAddr::new("unix://@test.abstract").unwrap();
            assert_eq!(&addr.to_string_ext().unwrap(), "unix://@test.abstract");
        }
    }

    #[test]
    fn test_socket_addr_debug() {
        let addr = SocketAddr::new("127.0.0.1:8080").unwrap();
        let debug_str = format!("{:?}", addr);

        assert!(debug_str.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_bind_std() {
        let addr = SocketAddr::new("127.0.0.1:0").unwrap();
        let _listener = addr.bind_std().unwrap();
    }

    #[cfg(feature = "feat-tokio")]
    #[tokio::test]
    async fn test_bind_tokio() {
        let addr = SocketAddr::new("127.0.0.1:0").unwrap();
        let _listener = addr.bind().await.unwrap();
    }

    #[cfg(all(unix, feature = "feat-tokio"))]
    #[tokio::test]
    async fn test_bind_tokio_unix() {
        let addr = SocketAddr::new("unix:///tmp/test_bind_tokio_unix.sock").unwrap();
        let _listener = addr.bind().await.unwrap();
    }

    #[cfg(all(any(target_os = "android", target_os = "linux"), feature = "feat-tokio"))]
    #[tokio::test]
    async fn test_bind_tokio_unix_abstract() {
        let addr = SocketAddr::new("unix://@abstract.test").unwrap();
        let _listener = addr.bind().await.unwrap();
    }

    #[test]
    fn test_edge_cases() {
        assert!(SocketAddr::new("").is_err());
        assert!(SocketAddr::new("not-an-address").is_err());
        assert!(SocketAddr::new("127.0.0.1:99999").is_err()); // Port too high

        #[cfg(unix)]
        {
            assert!(SocketAddr::new("unix://").is_ok()); // Empty path -> unnamed one
            #[cfg(any(target_os = "android", target_os = "linux"))]
            assert!(SocketAddr::new("unix://@").is_ok()); // Empty abstract one
        }
    }
}
