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

#[derive(Clone)]
/// A unified address type that can represent both
/// [`std::net::SocketAddr`] and [`unix::SocketAddr`] (a wrapper over
/// `std::os::unix::net::SocketAddr`).
///
/// ## Notes
///
/// For Unix domain sockets addresses, serialization/deserialization will be
/// performed in URI format (see [`UNIX_URI_PREFIX`]), which is different from
/// [`unix::SocketAddr`]'s serialization/deserialization behaviour.
pub enum SocketAddr {
    /// See [`std::net::SocketAddr`].
    Std(std::net::SocketAddr),

    #[cfg(unix)]
    /// See [`unix::SocketAddr`].
    Unix(unix::SocketAddr),
}

impl fmt::Debug for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketAddr::Std(addr) => addr.fmt(f),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => addr.fmt(f),
        }
    }
}

impl fmt::Display for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketAddr::Std(addr) => addr.fmt(f),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => format_args!("{UNIX_URI_PREFIX}{addr}").fmt(f),
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
    /// - Unix domain socket (filename): `"unix:///run/listen.sock"`
    /// - Unix domain socket (abstract namespace):
    ///   `"unix://@abstract.unix.socket"`
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
            .map(SocketAddr::Std)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "unknown format"))
    }

    #[inline]
    /// Binds a standard (TCP) listener to the address.
    pub fn bind_std(&self) -> io::Result<listener::StdListener> {
        match self {
            SocketAddr::Std(addr) => std::net::TcpListener::bind(addr).map(listener::StdListener::Tcp),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => addr.bind_std().map(listener::StdListener::Unix),
        }
    }

    #[cfg(feature = "feat-tokio")]
    #[inline]
    /// Binds a Tokio (TCP) listener to the address.
    pub async fn bind(&self) -> io::Result<listener::Listener> {
        match self {
            SocketAddr::Std(addr) => tokio::net::TcpListener::bind(addr).await.map(listener::Listener::Tcp),
            #[cfg(unix)]
            SocketAddr::Unix(addr) => addr.bind().map(listener::Listener::Unix),
        }
    }
}

#[cfg(feature = "feat-serde")]
impl serde::Serialize for SocketAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        format!("{self}").serialize(serializer)
    }
}

#[cfg(feature = "feat-serde")]
impl<'de> serde::Deserialize<'de> for SocketAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let addr = <&str>::deserialize(deserializer)?;
        Self::new(addr).map_err(serde::de::Error::custom)
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
            SocketAddr::Std(std_addr) => {
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
            SocketAddr::Std(std_addr) => {
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
            SocketAddr::Std(_) => unreachable!(),
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
            SocketAddr::Std(_) => unreachable!(),
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

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn test_socket_addr_display() {
        let addr = SocketAddr::new("127.0.0.1:8080").unwrap();
        assert_eq!(format!("{}", addr), "127.0.0.1:8080");

        let addr = SocketAddr::new("[::1]:8080").unwrap();
        assert_eq!(format!("{}", addr), "[::1]:8080");

        #[cfg(unix)]
        {
            let addr = SocketAddr::new("unix:///tmp/test.sock").unwrap();
            assert_eq!(format!("{}", addr), "unix:///tmp/test.sock");

            let addr = SocketAddr::new("unix://@test.abstract").unwrap();
            assert_eq!(format!("{}", addr), "unix://@test.abstract");
        }
    }

    #[test]
    fn test_socket_addr_debug() {
        let addr = SocketAddr::new("127.0.0.1:8080").unwrap();
        let debug_str = format!("{:?}", addr);

        assert!(debug_str.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_socket_addr_clone() {
        let addr = SocketAddr::new("127.0.0.1:8080").unwrap();
        let cloned = addr.clone();
        assert_eq!(format!("{}", addr), format!("{}", cloned));
    }

    #[test]
    fn test_bind_std() {
        let addr = SocketAddr::new("127.0.0.1:0").unwrap();
        let _listener = addr.bind_std().unwrap();
    }

    #[cfg(all(feature = "feat-tokio", test))]
    #[tokio::test]
    async fn test_bind_tokio() {
        let addr = SocketAddr::new("127.0.0.1:0").unwrap();
        let _listener = addr.bind().await.unwrap();
    }

    #[cfg(all(unix, feature = "feat-tokio", test))]
    #[tokio::test]
    async fn test_bind_tokio_unix() {
        let addr = SocketAddr::new("unix:///tmp/test.sock").unwrap();
        let _listener = addr.bind().await.unwrap();
    }

    #[cfg(all(unix, feature = "feat-tokio", test))]
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
            assert!(SocketAddr::new("unix://").is_err()); // Empty unix path
            assert!(SocketAddr::new("unix://@").is_err()); // Empty abstract
                                                           // name
        }
    }
}
