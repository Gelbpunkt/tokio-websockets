//! Abstractions over DNS resolvers.

use std::{
    future::Future,
    net::{SocketAddr, ToSocketAddrs},
    pin::Pin,
};

use crate::Error;

/// Trait for a DNS resolver to resolve hostnames and ports to IP addresses.
pub trait Resolver {
    /// Resolve a hostname and port to an IP address, asynchronously.
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr, Error>>>>;
}

/// A [`Resolver`] that uses the blocking `getaddrinfo` syscall in the tokio
/// threadpool.
pub struct Gai;

impl Resolver for Gai {
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr, Error>>>> {
        let host = host.to_owned();

        Box::pin(async move {
            let task = tokio::task::spawn_blocking(move || {
                (host, port)
                    .to_socket_addrs()?
                    .next()
                    .ok_or(Error::CannotResolveHost)
            });

            task.await.expect("Tokio threadpool failed")
        })
    }
}
