//! Listener Types

#[derive(Debug)]
/// A listener that can be either a [`std::net::TcpListener`] or a
/// [`std::os::unix::net::UnixListener`].
pub enum StdListener {
    /// [`std::net::TcpListener`]
    Tcp(std::net::TcpListener),

    #[cfg(unix)]
    /// [`std::os::unix::net::UnixListener`]
    Unix(std::os::unix::net::UnixListener),
}

#[cfg(feature = "feat-tokio")]
#[derive(Debug)]
/// A Tokio listener that can be either a [`tokio::net::TcpListener`] or a
/// [`tokio::net::UnixListener`].
pub enum Listener {
    /// [`tokio::net::TcpListener`]
    Tcp(tokio::net::TcpListener),

    #[cfg(unix)]
    /// [`tokio::net::UnixListener`]
    Unix(tokio::net::UnixListener),
}
