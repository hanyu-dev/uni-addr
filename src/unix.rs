//! Platform-specific code for Unix-like systems

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{SocketAddr as StdSocketAddr, UnixListener};
use std::path::Path;
use std::{fmt, fs, io};

use wrapper_lite::general_wrapper;

general_wrapper! {
    #[wrapper_impl(Deref)]
    #[derive(Clone)]
    /// Wrapper over [`std::os::unix::net::SocketAddr`].
    ///
    /// See [`SocketAddr::new`] for more details.
    pub SocketAddr(StdSocketAddr)
}

impl SocketAddr {
    /// Creates a new [`SocketAddr`] from a path.
    ///
    /// - All strings that start with `@` or `\0` are treated as an abstract
    ///   socket address.
    /// - All other strings are treated as pathname socket addresses.
    /// - Empty path is not supported (the unnamed one).
    ///
    /// This will not create the file for a pathname socket address. See
    /// [`create_path_if_absent`](Self::create_path_if_absent).
    pub fn new(addr: &str) -> io::Result<Self> {
        match addr.chars().next() {
            #[cfg(any(target_os = "android", target_os = "linux"))]
            Some('@') | Some('\0') if addr.len() > 1 => {
                use std::os::linux::net::SocketAddrExt;

                StdSocketAddr::from_abstract_name(&addr[1..]).map(Self::const_from)
            }
            Some('@') | Some('\0') => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid unix domain socket addr: not support or malformed",
            )),
            Some(_) => StdSocketAddr::from_pathname(Path::new(addr)).map(Self::const_from),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid unix path: unnamed unix socket address is not supported",
            )),
        }
    }

    /// Create the socket file if it does not exist, then set the permissions to
    /// `0o644`.
    ///
    /// For an abstract unix domain socket addr, this is a no-op.
    pub fn create_path_if_absent(self) -> io::Result<Self> {
        if let Some(pathname) = self.as_pathname() {
            if let Some(parent) = pathname.parent() {
                // Create parent directories if they do not exist
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }

            fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(pathname)?
                .set_permissions(PermissionsExt::from_mode(0o644))?;
        }

        Ok(self)
    }

    #[inline]
    /// Bind and create a [`std::os::unix::net::UnixListener`].
    pub fn bind_std(&self) -> io::Result<UnixListener> {
        UnixListener::bind_addr(self)
    }

    #[cfg(feature = "feat-tokio")]
    /// Bind and create a [`tokio::net::UnixListener`].
    pub fn bind(&self) -> io::Result<tokio::net::UnixListener> {
        self.bind_std()
            .and_then(|l| {
                l.set_nonblocking(true)?;
                Ok(l)
            })
            .and_then(tokio::net::UnixListener::from_std)
    }

    /// Serializes the socket address to a string (may return error).
    ///
    /// - For abstract socket addresses, it returns the name prefixed with `@`.
    /// - For pathname socket addresses, it returns the path as a string.
    pub fn to_string_ext(&self) -> io::Result<String> {
        if let Some(pathname) = self.as_pathname() {
            return Ok(pathname
                .to_str()
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "invalid pathname"))?
                .to_string());
        }

        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            use std::os::linux::net::SocketAddrExt;

            if let Some(abstract_name) = self.as_abstract_name() {
                return Ok(format!("@{}", String::from_utf8_lossy(abstract_name)));
            }
        }

        Err(io::Error::new(io::ErrorKind::Other, "invalid socket address"))
    }
}

impl fmt::Debug for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_inner().fmt(f)
    }
}

impl fmt::Display for SocketAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_string_ext().map_err(|_| fmt::Error)?.as_str())
    }
}

#[cfg(feature = "feat-serde")]
impl serde::Serialize for SocketAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let addr = self.to_string_ext().map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&addr)
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
    use super::*;

    #[test]
    fn test_unix_socket_addr() {
        let addr = SocketAddr::new("/tmp/test_unix_socket_addr.socket").unwrap();

        assert_eq!(
            addr.as_ref().as_pathname().unwrap().to_str().unwrap(),
            "/tmp/test_unix_socket_addr.socket"
        );
    }

    #[test]
    fn test_unix_socket_addr_with_create() {
        let addr = SocketAddr::new("/tmp/test_unix_socket_addr_with_create.socket")
            .unwrap()
            .create_path_if_absent()
            .unwrap();

        assert_eq!(
            addr.as_ref().as_pathname().unwrap().to_str().unwrap(),
            "/tmp/test_unix_socket_addr_with_create.socket"
        );
    }

    #[test]
    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn test_unix_socket_addr_abstract() {
        use std::os::linux::net::SocketAddrExt;

        let addr = SocketAddr::new("@abstract.socket").unwrap();
        assert_eq!(&addr.as_ref().as_abstract_name().unwrap(), b"abstract.socket");

        let addr = SocketAddr::new("\0abstract.socket").unwrap();
        assert_eq!(&addr.as_ref().as_abstract_name().unwrap(), b"abstract.socket");
    }
}
