//! Platform-specific code for Unix-like systems

use std::ffi::{OsStr, OsString};
use std::hash::{Hash, Hasher};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::{fmt, fs, io};

wrapper_lite::general_wrapper! {
    #[wrapper_impl(Deref)]
    #[derive(Clone)]
    /// Wrapper over [`std::os::unix::net::SocketAddr`].
    ///
    /// See [`SocketAddr::new`] for more details.
    pub struct SocketAddr(std::os::unix::net::SocketAddr);
}

impl SocketAddr {
    /// Creates a new [`SocketAddr`] from its string representation.
    ///
    /// # Address Types
    ///
    /// - Strings starting with `@` or `\0` are parsed as abstract unix socket
    ///   addresses (Linux-specific).
    /// - All other strings are parsed as pathname unix socket addresses.
    /// - Empty strings create unnamed unix socket addresses.
    ///
    /// # Notes
    ///
    /// This method accepts an [`OsStr`] and does not guarantee proper null
    /// termination. While pathname addresses reject interior null bytes,
    /// abstract addresses accept them silently, potentially causing unexpected
    /// behavior (e.g., `\0abstract` differs from `\0abstract\0\0\0\0\0...`).
    /// Use [`SocketAddr::new_strict`] to ensure the abstract names do not
    /// contain null bytes, too.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use uni_addr::unix::SocketAddr;
    /// #[cfg(any(target_os = "android", target_os = "linux"))]
    /// // Abstract address (Linux-specific)
    /// let abstract_addr = SocketAddr::new("@abstract.example.socket").unwrap();
    /// // Pathname address
    /// let pathname_addr = SocketAddr::new("/run/pathname.example.socket").unwrap();
    /// // Unnamed address
    /// let unnamed_addr = SocketAddr::new("").unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the address is invalid or unsupported on the
    /// current platform.
    ///
    /// See [`SocketAddr::from_abstract_name`](std::os::linux::net::SocketAddrExt::from_abstract_name)
    /// and [`StdSocketAddr::from_pathname`] for more details.
    pub fn new<S: AsRef<OsStr> + ?Sized>(addr: &S) -> io::Result<Self> {
        let addr = addr.as_ref();

        match addr.as_bytes() {
            #[cfg(any(target_os = "android", target_os = "linux"))]
            [b'@', rest @ ..] | [b'\0', rest @ ..] => Self::new_abstract(rest),
            #[cfg(not(any(target_os = "android", target_os = "linux")))]
            [b'@', ..] | [b'\0', ..] => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "abstract unix socket address is not supported",
            )),
            _ => Self::new_pathname(addr),
        }
    }

    /// See [`SocketAddr::new`].
    pub fn new_strict<S: AsRef<OsStr> + ?Sized>(addr: &S) -> io::Result<Self> {
        let addr = addr.as_ref();

        match addr.as_bytes() {
            #[cfg(any(target_os = "android", target_os = "linux"))]
            [b'@', rest @ ..] | [b'\0', rest @ ..] => Self::new_abstract_strict(rest),
            #[cfg(not(any(target_os = "android", target_os = "linux")))]
            [b'@', ..] | [b'\0', ..] => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "abstract unix socket address is not supported",
            )),
            _ => Self::new_pathname(addr),
        }
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    /// Creates a Unix socket address in the abstract namespace.
    ///
    /// The abstract namespace is a Linux-specific extension that allows Unix
    /// sockets to be bound without creating an entry in the filesystem.
    /// Abstract sockets are unaffected by filesystem layout or permissions, and
    /// no cleanup is necessary when the socket is closed.
    ///
    /// An abstract socket address name may contain any bytes, including zero.
    /// However, we don't recommend using zero bytes, as they may lead to
    /// unexpected behavior. To avoid this, consider using
    /// [`new_abstract_strict`](Self::new_abstract_strict).
    ///
    /// # Errors
    ///
    /// Returns an error if the name is longer than `SUN_LEN - 1`.
    pub fn new_abstract(bytes: &[u8]) -> io::Result<Self> {
        use std::os::linux::net::SocketAddrExt;

        std::os::unix::net::SocketAddr::from_abstract_name(bytes).map(Self::const_from)
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    /// See [`SocketAddr::new_abstract`].
    pub fn new_abstract_strict(bytes: &[u8]) -> io::Result<Self> {
        use std::os::linux::net::SocketAddrExt;

        if bytes.contains(&b'\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "parse abstract socket name in strict mode: reject NULL bytes",
            ));
        }

        std::os::unix::net::SocketAddr::from_abstract_name(bytes).map(Self::const_from)
    }

    /// Constructs a [`SocketAddr`] with the family `AF_UNIX` and the provided
    /// path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is longer than `SUN_LEN` or if it contains
    /// NULL bytes.
    pub fn new_pathname<P: AsRef<Path>>(pathname: P) -> io::Result<Self> {
        let _ = fs::remove_file(pathname.as_ref());

        std::os::unix::net::SocketAddr::from_pathname(pathname).map(Self::const_from)
    }

    #[allow(clippy::missing_panics_doc)]
    /// Creates an unnamed [`SocketAddr`].
    pub fn new_unnamed() -> Self {
        // SAFETY: `from_pathname` will not fail at all.
        std::os::unix::net::SocketAddr::from_pathname("")
            .map(Self::const_from)
            .unwrap()
    }

    #[inline]
    /// Creates a new [`SocketAddr`] from bytes.
    ///
    /// # Errors
    ///
    /// See [`SocketAddr::new`].
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        Self::new(OsStr::from_bytes(bytes))
    }

    /// Serializes the [`SocketAddr`] to an `OsString`.
    ///
    /// # Returns
    ///
    /// - For abstract ones: returns the name prefixed with **`\0`**
    /// - For pathname ones: returns the pathname
    /// - For unnamed ones: returns an empty string.
    pub fn to_os_string(&self) -> OsString {
        self.to_os_string_impl("", "\0")
    }

    /// Likes [`to_os_string`](Self::to_os_string), but returns a `String`
    /// instead of `OsString`, performing lossy UTF-8 conversion.
    ///
    /// # Returns
    ///
    /// - For abstract ones: returns the name prefixed with **`@`**
    /// - For pathname ones: returns the pathname
    /// - For unnamed ones: returns an empty string.
    pub fn to_string_lossy(&self) -> String {
        self.to_os_string_impl("", "@")
            .to_string_lossy()
            .into_owned()
    }

    pub(crate) fn to_os_string_impl(&self, prefix: &str, abstract_identifier: &str) -> OsString {
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

        // An unnamed one...
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

impl Hash for SocketAddr {
    fn hash<H: Hasher>(&self, state: &mut H) {
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

        // `Path` cannot contain null bytes, and abstract names are started with
        // null bytes, this is Ok.
        b"(unnamed)\0".hash(state);
    }
}

#[cfg(feature = "feat-serde")]
impl serde::Serialize for SocketAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string_lossy())
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
        assert_eq!(addr.to_string_lossy(), TEST_CASE);
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
