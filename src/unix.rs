//! Platform-specific code for Unix-like systems

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{SocketAddr as StdSocketAddr, UnixListener, UnixStream};
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
    /// Creates a new [`SocketAddr`] from its string representation.
    ///
    /// # Address Types
    ///
    /// - **Abstract**: Strings starting with `@` or `\0` are interpreted as
    ///   abstract socket addresses (Linux-specific namespace)
    /// - **Pathname**: All other strings are treated as filesystem-based socket
    ///   addresses with an actual file path
    /// - **Unnamed**: Empty paths are not supported and will be rejected
    ///
    /// # Important Notes
    ///
    /// This method only parses the address string and does not perform any
    /// filesystem operations. For pathname addresses, the corresponding
    /// socket file will not be created automatically. Use
    /// [`create_path_if_absent`](Self::create_path_if_absent) if you need
    /// to ensure the socket file exists.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use uni_addr::unix::SocketAddr;
    /// // Abstract socket (Linux-specific)
    /// let abstract_addr = SocketAddr::new("@abstract.example.socket").unwrap();
    /// // Pathname socket
    /// let path_addr = SocketAddr::new("/tmp/example.sock").unwrap();
    /// ```
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

    #[deprecated(since = "0.1.2", note = "Use `create_if_absent` instead")]
    /// See [`Self::create_if_absent`].
    pub fn create_path_if_absent(self) -> io::Result<Self> {
        self.create_if_absent(None)
    }

    /// Creates the socket file if it does not exist and sets its permissions
    /// (`0o644` by default).
    ///
    /// This function ensures the socket file is properly initialized with read
    /// permissions for all users and write permissions for the owner only.
    /// For abstract Unix domain socket addresses, this operation is skipped
    /// as no filesystem entry is created.
    pub fn create_if_absent(self, permissions: Option<u32>) -> io::Result<Self> {
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
                .set_permissions(PermissionsExt::from_mode(permissions.unwrap_or(0o644)))?;
        }

        Ok(self)
    }

    #[inline]
    /// Binds to the Unix domain socket address and creates a
    /// [`std::os::unix::net::UnixListener`].
    pub fn bind_std(&self) -> io::Result<UnixListener> {
        UnixListener::bind_addr(self)
    }

    #[cfg(feature = "feat-tokio")]
    /// Binds to the Unix domain socket address and creates a
    /// [`tokio::net::UnixListener`].
    pub fn bind(&self) -> io::Result<tokio::net::UnixListener> {
        self.bind_std()
            .and_then(|l| {
                l.set_nonblocking(true)?;
                Ok(l)
            })
            .and_then(tokio::net::UnixListener::from_std)
    }

    #[inline]
    /// Connects to the Unix domain socket address and returns a
    /// [`std::os::unix::net::UnixStream`].
    pub fn connect_std(&self) -> io::Result<UnixStream> {
        UnixStream::connect_addr(self)
    }

    #[cfg(feature = "feat-tokio")]
    /// Connects to the Unix domain socket address and returns a
    /// [`tokio::net::UnixStream`].
    pub async fn connect(&self) -> io::Result<tokio::net::UnixStream> {
        self.connect_std()
            .and_then(|s| {
                s.set_nonblocking(true)?;
                Ok(s)
            })
            .and_then(tokio::net::UnixStream::from_std)
    }

    /// Serializes the Unix domain socket address to a string representation.
    ///
    /// # Returns
    ///
    /// - For abstract addresses: returns the name prefixed with `@`
    /// - For pathname addresses: returns the filesystem path as a string
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

impl PartialEq for SocketAddr {
    fn eq(&self, other: &Self) -> bool {
        if let Some((l, r)) = self.as_pathname().zip(other.as_pathname()) {
            return l == r;
        }

        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            use std::os::linux::net::SocketAddrExt;

            if let Some((l, r)) = self.as_abstract_name().zip(other.as_abstract_name()) {
                return l == r;
            }
        }

        false
    }
}

impl Eq for SocketAddr {}

impl std::hash::Hash for SocketAddr {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if let Some(pathname) = self.as_pathname() {
            pathname.hash(state);

            return;
        }

        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            use std::os::linux::net::SocketAddrExt;

            if let Some(abstract_name) = self.as_abstract_name() {
                b'\0'.hash(state);
                abstract_name.hash(state);

                return;
            }
        }

        "(unamed)".hash(state);
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
            .create_if_absent(None)
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
