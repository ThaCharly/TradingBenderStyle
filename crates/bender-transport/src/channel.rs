//! [`Channel`]: el mismo tipo de canal para el transporte en proceso y el de red, así los
//! clientes generados (`XxxClient<Channel>`) no cambian según dónde corre el servicio.

use http::HeaderValue;
use tonic::body::Body;
use tonic::service::Routes;
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint};
use tower::util::BoxCloneSyncService;
use tower::{Layer, ServiceBuilder, ServiceExt};

use crate::BoxError;
use crate::version::{HEADER, ServiceVersions, Version, VersionLayer};

/// Canal de un cliente: en proceso o en red, con la versión del cliente ya incluida en
/// cada llamada.
#[derive(Clone)]
pub struct Channel(BoxCloneSyncService<http::Request<Body>, http::Response<Body>, BoxError>);

/// TLS del lado cliente: la CA que firmó el certificado del servidor y el nombre esperado.
#[derive(Clone, Debug)]
pub struct ClientTls {
    pub ca_pem: Vec<u8>,
    pub domain: String,
}

fn version_header(version: Version) -> HeaderValue {
    HeaderValue::from_str(&version.to_string()).expect("versión ASCII")
}

impl Channel {
    /// Transporte en proceso: llama a los servicios de `routes` sin red, con las mismas
    /// capas (versionado) que el servidor de red.
    pub fn in_process(routes: Routes, versions: ServiceVersions, client: Version) -> Self {
        let header = version_header(client);
        let service = ServiceBuilder::new()
            .map_request(move |mut req: http::Request<Body>| {
                req.headers_mut().insert(HEADER, header.clone());
                req
            })
            .service(VersionLayer::new(versions).layer(routes))
            .map_err(|never| match never {});
        Self(BoxCloneSyncService::new(service))
    }

    /// Transporte de red hacia `url` (p. ej. "http://servidor:7400"). Conecta en la
    /// primera llamada y se reconecta solo después de un corte.
    pub fn connect(url: &str, client: Version, tls: Option<ClientTls>) -> Result<Self, BoxError> {
        let mut endpoint = Endpoint::from_shared(url.to_string())?.tcp_nodelay(true);
        if let Some(tls) = tls {
            endpoint = endpoint.tls_config(
                ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(tls.ca_pem))
                    .domain_name(tls.domain),
            )?;
        }
        let header = version_header(client);
        let service = ServiceBuilder::new()
            .map_request(move |mut req: http::Request<Body>| {
                req.headers_mut().insert(HEADER, header.clone());
                req
            })
            .service(endpoint.connect_lazy())
            .map_err(BoxError::from);
        Ok(Self(BoxCloneSyncService::new(service)))
    }
}

impl tower::Service<http::Request<Body>> for Channel {
    type Response = http::Response<Body>;
    type Error = BoxError;
    type Future = <BoxCloneSyncService<http::Request<Body>, http::Response<Body>, BoxError> as tower::Service<http::Request<Body>>>::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<Body>) -> Self::Future {
        self.0.call(req)
    }
}
