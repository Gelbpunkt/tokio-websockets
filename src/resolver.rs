//! Abstractions over DNS resolvers.

use std::{future::Future, net::SocketAddr, pin::Pin};

use crate::Error;

/// Trait for a DNS resolver to resolve hostnames and ports to IP addresses.
pub trait Resolver: Send {
    /// Resolve a hostname and port to an IP address, asynchronously.
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr, Error>> + Send>>;
}

/// A [`Resolver`] that uses the blocking `getaddrinfo` syscall in the tokio
/// threadpool.
pub struct Gai;

impl Resolver for Gai {
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr, Error>> + Send>> {
        let host = host.to_owned();

        Box::pin(async move {
            tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| Error::CannotResolveHost)?
                .next()
                .ok_or(Error::CannotResolveHost)
        })
    }
}
