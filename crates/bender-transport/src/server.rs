//! Servidor de red: gRPC (HTTP/2) para los componentes Rust y gRPC-Web (HTTP/1.1) para el
//! front, con versionado y TLS opcional.

use std::future::Future;

use tokio::net::TcpListener;
use tonic::service::Routes;
use tonic::transport::server::TcpIncoming;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic_web::GrpcWebLayer;

use crate::version::{ServiceVersions, VersionLayer};

/// Certificado y clave del servidor, en PEM. Sin TLS, el tráfico va en claro: sólo para
/// la máquina local o dentro de Tailscale, que ya cifra.
#[derive(Clone, Debug)]
pub struct ServerTls {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

/// Sirve `routes` en `listener` hasta que termine `shutdown`.
pub async fn serve(
    listener: TcpListener,
    routes: Routes,
    versions: ServiceVersions,
    tls: Option<ServerTls>,
    shutdown: impl Future<Output = ()>,
) -> Result<(), tonic::transport::Error> {
    let mut builder = Server::builder();
    if let Some(tls) = tls {
        builder = builder.tls_config(
            ServerTlsConfig::new().identity(Identity::from_pem(tls.cert_pem, tls.key_pem)),
        )?;
    }
    builder
        .accept_http1(true)
        .layer(GrpcWebLayer::new())
        .layer(VersionLayer::new(versions))
        .add_routes(routes)
        .serve_with_incoming_shutdown(
            // Sin Nagle: los eventos de un stream salen apenas se producen.
            TcpIncoming::from(listener).with_nodelay(Some(true)),
            shutdown,
        )
        .await
}
