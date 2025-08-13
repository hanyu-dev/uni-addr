//! Platform-specific code for Unix-like systems

use std::ffi::{CStr, OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr as StdSocketAddr, UnixDatagram, UnixListener, UnixStream};
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
    /// Creates a new unix [`SocketAddr`] from its string representation.
    ///
    /// # Address Types
    ///
    /// - Strings starting with `@` or `\0` are parsed as abstract unix socket
    ///   addresses (Linux-specific).
    /// - All other strings are parsed as pathname unix socket addresses.
    /// - Empty strings create unnamed unix socket addresses.
    ///
    /// # Important
    ///
    /// This method accepts an `OsStr` and does not guarantee proper null
    /// termination. While pathname addresses reject interior null bytes,
    /// abstract addresses accept them silently, potentially causing unexpected
    /// behavior (e.g., `\0abstract` differs from `\0abstract\0\0\0\0\0...`).
    ///
    /// Use [`SocketAddr::from_bytes_until_nul`] to ensure only the portion
    /// before the first null byte is used for address parsing.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use uni_addr::unix::SocketAddr;
    /// #[cfg(any(target_os = "android", target_os = "linux"))]
    /// // Abstract address (Linux-specific)
    /// let abstract_addr = SocketAddr::new("@abstract.example.socket").unwrap();
    ///
    /// // Pathname address
    /// let pathname_addr = SocketAddr::new("/run/pathname.example.socket").unwrap();
    ///
    /// // Unnamed address
    /// let unnamed_addr = SocketAddr::new("").unwrap();
    /// ```
    pub fn new<S: AsRef<OsStr> + ?Sized>(addr: &S) -> io::Result<Self> {
        let addr = addr.as_ref();

        match addr.as_bytes() {
            #[cfg(any(target_os = "android", target_os = "linux"))]
            [b'@', rest @ ..] | [b'\0', rest @ ..] => {
                use std::os::linux::net::SocketAddrExt;

                StdSocketAddr::from_abstract_name(rest).map(Self::const_from)
            }
            #[cfg(not(any(target_os = "android", target_os = "linux")))]
            [b'@', ..] | [b'\0', ..] => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "abstract unix socket address is not supported",
            )),
            _ => {
                let _ = fs::remove_file(addr);

                StdSocketAddr::from_pathname(addr).map(Self::const_from)
            }
        }
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    /// Creates a new abstract unix [`SocketAddr`].
    pub fn new_abstract(bytes: &[u8]) -> io::Result<Self> {
        use std::os::linux::net::SocketAddrExt;

        StdSocketAddr::from_abstract_name(bytes).map(Self::const_from)
    }

    /// Creates a new pathname unix [`SocketAddr`].
    pub fn new_pathname<P: AsRef<Path>>(pathname: P) -> io::Result<Self> {
        StdSocketAddr::from_pathname(pathname).map(Self::const_from)
    }

    #[allow(clippy::missing_panics_doc)]
    /// Creates a new unnamed unix [`SocketAddr`].
    pub fn new_unnamed() -> Self {
        // SAFEY: `from_pathname` will not fail at all.
        StdSocketAddr::from_pathname("").map(Self::const_from).unwrap()
    }

    #[inline]
    /// Creates a new unix [`SocketAddr`] from bytes.
    ///
    /// # Note
    ///
    /// This method does not validate null terminators. Pathname addresses
    /// will reject paths containing null bytes during parsing, but abstract
    /// addresses accept null bytes silently, which may lead to unexpected
    /// behavior.
    ///
    /// Consider using [`from_bytes_until_nul`](Self::from_bytes_until_nul)
    /// for null-terminated parsing.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        Self::new(OsStr::from_bytes(bytes))
    }

    /// Creates a new unix [`SocketAddr`] from bytes until the first null byte.
    pub fn from_bytes_until_nul(bytes: &[u8]) -> io::Result<Self> {
        let first_nul = match bytes {
            [b'\0', rest @ ..] => CStr::from_bytes_until_nul(rest),
            rest => CStr::from_bytes_until_nul(rest),
        }
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "bytes must be a valid C string with a null terminator",
            )
        })?;

        Self::new(OsStr::from_bytes(first_nul.to_bytes()))
    }

    #[inline]
    /// Creates a new [`UnixListener`] bound to the specified socket.
    pub fn bind_std(&self) -> io::Result<UnixListener> {
        UnixListener::bind_addr(self)
    }

    #[cfg(feature = "feat-tokio")]
    /// Creates a new [`tokio::net::UnixListener`] bound to the specified
    /// socket.
    pub fn bind(&self) -> io::Result<tokio::net::UnixListener> {
        self.bind_std()
            .and_then(|l| {
                l.set_nonblocking(true)?;
                Ok(l)
            })
            .and_then(tokio::net::UnixListener::from_std)
    }

    #[inline]
    /// Creates a Unix datagram socket bound to the given path.
    pub fn bind_dgram_std(&self) -> io::Result<UnixDatagram> {
        UnixDatagram::bind_addr(self)
    }

    #[cfg(feature = "feat-tokio")]
    /// Creates a Unix datagram socket bound to the given path.
    pub fn bind_dgram(&self) -> io::Result<tokio::net::UnixDatagram> {
        self.bind_dgram_std()
            .and_then(|d| {
                d.set_nonblocking(true)?;
                Ok(d)
            })
            .and_then(tokio::net::UnixDatagram::from_std)
    }

    #[inline]
    /// Connects to the Unix socket address and returns a
    /// [`std::os::unix::net::UnixStream`].
    pub fn connect_std(&self) -> io::Result<UnixStream> {
        UnixStream::connect_addr(self)
    }

    #[cfg(feature = "feat-tokio")]
    /// Connects to the Unix socket address and returns a
    /// [`tokio::net::UnixStream`].
    pub fn connect(&self) -> io::Result<tokio::net::UnixStream> {
        self.connect_std()
            .and_then(|s| {
                s.set_nonblocking(true)?;
                Ok(s)
            })
            .and_then(tokio::net::UnixStream::from_std)
    }

    /// Serializes the Unix socket address to an `OsString`.
    ///
    /// # Returns
    ///
    /// - For abstract ones: returns the name prefixed with **`\0`**
    /// - For pathname ones: returns the pathname
    /// - For unnamed ones: returns an empty string.
    pub fn to_os_string(&self) -> OsString {
        self._to_os_string("", "\0")
    }

    /// Likes [`to_os_string`](Self::to_os_string), but returns a `String`
    /// instead of `OsString`, performing UTF-8 verification.
    ///
    /// # Returns
    ///
    /// - For abstract ones: returns the name prefixed with **`@`**
    /// - For pathname ones: returns the pathname
    /// - For unnamed ones: returns an empty string.
    pub fn to_string_ext(&self) -> Option<String> {
        self._to_os_string("", "@").into_string().ok()
    }

    pub(crate) fn _to_os_string(&self, prefix: &str, abstract_identifier: &str) -> OsString {
        let mut os_string = OsString::from(prefix);

        if let Some(pathname) = self.as_pathname() {
            // Notice: cannot use `extend` here
            os_string.push(pathname);

            return os_string;
        }

        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            use std::os::linux::net::SocketAddrExt;

            if let Some(abstract_name) = self.as_abstract_name() {
                os_string.push(abstract_identifier);
                os_string.push(OsStr::from_bytes(abstract_name));

                return os_string;
            }
        }

        os_string
    }
}

impl fmt::Debug for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_inner().fmt(f)
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

        if self.is_unnamed() && other.is_unnamed() {
            return true;
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

        debug_assert!(self.is_unnamed(), "SocketAddr is not unnamed one");

        // `Path` cannot contain null bytes, so we can safely use it as a
        // sentinel value.
        b"(unnamed)\0".hash(state);
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
    use core::hash::{Hash, Hasher};
    use std::hash::DefaultHasher;

    use super::*;

    #[test]
    fn test_unnamed() {
        const TEST_CASE: &str = "";

        let addr = SocketAddr::new(TEST_CASE).unwrap();

        assert!(addr.as_ref().is_unnamed());
    }

    #[test]
    fn test_pathname() {
        const TEST_CASE: &str = "/tmp/test_pathname.socket";

        let addr = SocketAddr::new(TEST_CASE).unwrap();

        assert_eq!(addr.to_os_string().to_str().unwrap(), TEST_CASE);
        assert_eq!(addr.to_string_ext().unwrap(), TEST_CASE);
        assert_eq!(addr.as_pathname().unwrap().to_str().unwrap(), TEST_CASE);
    }

    #[test]
    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn test_abstract() {
        use std::os::linux::net::SocketAddrExt;

        const TEST_CASE_1: &[u8] = b"@abstract.socket";
        const TEST_CASE_2: &[u8] = b"\0abstract.socket";
        const TEST_CASE_3: &[u8] = b"@";
        const TEST_CASE_4: &[u8] = b"\0";

        assert_eq!(
            SocketAddr::new(OsStr::from_bytes(TEST_CASE_1))
                .unwrap()
                .as_abstract_name()
                .unwrap(),
            &TEST_CASE_1[1..]
        );

        assert_eq!(
            SocketAddr::new(OsStr::from_bytes(TEST_CASE_2))
                .unwrap()
                .as_abstract_name()
                .unwrap(),
            &TEST_CASE_2[1..]
        );

        assert_eq!(
            SocketAddr::new(OsStr::from_bytes(TEST_CASE_3))
                .unwrap()
                .as_abstract_name()
                .unwrap(),
            &TEST_CASE_3[1..]
        );

        assert_eq!(
            SocketAddr::new(OsStr::from_bytes(TEST_CASE_4))
                .unwrap()
                .as_abstract_name()
                .unwrap(),
            &TEST_CASE_4[1..]
        );
    }

    #[test]
    #[should_panic]
    fn test_pathname_with_null_byte() {
        let _addr = SocketAddr::new_pathname("(unamed)\0").unwrap();
    }

    #[test]
    fn test_partial_eq_hash() {
        let addr_pathname_1 = SocketAddr::new("/tmp/test_pathname_1.socket").unwrap();
        let addr_pathname_2 = SocketAddr::new("/tmp/test_pathname_2.socket").unwrap();
        let addr_unnamed = SocketAddr::new_unnamed();

        assert_eq!(addr_pathname_1, addr_pathname_1);
        assert_ne!(addr_pathname_1, addr_pathname_2);
        assert_ne!(addr_pathname_2, addr_pathname_1);

        assert_eq!(addr_unnamed, addr_unnamed);
        assert_ne!(addr_pathname_1, addr_unnamed);
        assert_ne!(addr_unnamed, addr_pathname_1);
        assert_ne!(addr_pathname_2, addr_unnamed);
        assert_ne!(addr_unnamed, addr_pathname_2);

        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            let addr_abstract_1 = SocketAddr::new_abstract(b"/tmp/test_pathname_1.socket").unwrap();
            let addr_abstract_2 = SocketAddr::new_abstract(b"/tmp/test_pathname_2.socket").unwrap();
            let addr_abstract_empty = SocketAddr::new_abstract(&[]).unwrap();
            let addr_abstract_unnamed_hash = SocketAddr::new_abstract(b"(unamed)\0").unwrap();

            assert_eq!(addr_abstract_1, addr_abstract_1);
            assert_ne!(addr_abstract_1, addr_abstract_2);
            assert_ne!(addr_abstract_2, addr_abstract_1);

            // Empty abstract addresses should be equal to unnamed addresses
            assert_ne!(addr_unnamed, addr_abstract_empty);

            // Abstract addresses should not be equal to pathname addresses
            assert_ne!(addr_pathname_1, addr_abstract_1);

            // Abstract unnamed address `@(unamed)\0`' hash should not be equal to unname
            // ones'
            let addr_unnamed_hash = {
                let mut state = DefaultHasher::new();
                addr_unnamed.hash(&mut state);
                state.finish()
            };
            let addr_abstract_unnamed_hash = {
                let mut state = DefaultHasher::new();
                addr_abstract_unnamed_hash.hash(&mut state);
                state.finish()
            };
            assert_ne!(addr_unnamed_hash, addr_abstract_unnamed_hash);
        }
    }
}
