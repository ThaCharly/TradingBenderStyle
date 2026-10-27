//! Versionado de protocolo (doc 07 §2.6): cada servicio publica su versión; los clientes
//! aceptan ±1 versión menor y reciben un error tipado si son incompatibles.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};

use bender_types::proto;
use bytes::Bytes;
use http::HeaderValue;
use prost::Message;
use tonic::body::Body;
use tonic::{Code, Status};
use tower::{Layer, Service};

/// Metadato con la versión: en las llamadas, la del cliente; en las respuestas, la del
/// servicio. Formato "MAYOR.MENOR".
pub const HEADER: &str = "bender-protocol-version";

/// `type_url` del detalle [`proto::VersionMismatch`] dentro de `google.rpc.Status`.
pub const MISMATCH_TYPE_URL: &str = "type.googleapis.com/bender.v1.VersionMismatch";

/// Versión de protocolo "MAYOR.MENOR".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for Version {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        let (major, minor) = s.trim().split_once('.').ok_or(())?;
        Ok(Self::new(
            major.parse().map_err(|_| ())?,
            minor.parse().map_err(|_| ())?,
        ))
    }
}

impl From<Version> for proto::ProtocolVersion {
    fn from(v: Version) -> Self {
        Self {
            major: v.major,
            minor: v.minor,
        }
    }
}

impl From<proto::ProtocolVersion> for Version {
    fn from(v: proto::ProtocolVersion) -> Self {
        Self::new(v.major, v.minor)
    }
}

/// Relación entre la versión de un servicio y la de un cliente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compatibility {
    Same,
    /// Misma mayor, menores a distancia 1: funciona, pero uno de los dos está
    /// desactualizado y conviene avisarlo.
    MinorSkew,
    Incompatible,
}

pub fn compatibility(service: Version, client: Version) -> Compatibility {
    if service.major != client.major || service.minor.abs_diff(client.minor) > 1 {
        Compatibility::Incompatible
    } else if service.minor != client.minor {
        Compatibility::MinorSkew
    } else {
        Compatibility::Same
    }
}

/// Versión que informa una respuesta, si la trae.
pub fn from_metadata(metadata: &tonic::metadata::MetadataMap) -> Option<Version> {
    metadata.get(HEADER)?.to_str().ok()?.parse().ok()
}

/// `google.rpc.Status`: el formato estándar de `grpc-status-details-bin`, que Connect
/// (TS) sabe leer.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct RpcStatus {
    #[prost(int32, tag = "1")]
    pub code: i32,
    #[prost(string, tag = "2")]
    pub message: String,
    #[prost(message, repeated, tag = "3")]
    pub details: Vec<prost_types::Any>,
}

/// Error `FAILED_PRECONDITION` con un [`proto::VersionMismatch`] en los detalles.
pub fn mismatch_status(service: &str, server: Version, client: Option<Version>) -> Status {
    let message = match client {
        Some(c) => format!("{service}: versión de protocolo {c} incompatible con {server}"),
        None => format!("{service}: falta el metadato {HEADER} (servidor {server})"),
    };
    let mismatch = proto::VersionMismatch {
        service: service.to_string(),
        server: Some(server.into()),
        client: client.map(Into::into),
    };
    let details = RpcStatus {
        code: Code::FailedPrecondition as i32,
        message: message.clone(),
        details: vec![prost_types::Any {
            type_url: MISMATCH_TYPE_URL.to_string(),
            value: mismatch.encode_to_vec(),
        }],
    };
    Status::with_details(
        Code::FailedPrecondition,
        message,
        Bytes::from(details.encode_to_vec()),
    )
}

/// Versiones de los servicios que expone un servidor, por nombre completo
/// (p. ej. "bender.example.v1.ExampleService").
#[derive(Clone, Debug, Default)]
pub struct ServiceVersions(Arc<BTreeMap<String, Version>>);

impl ServiceVersions {
    pub fn new(services: impl IntoIterator<Item = (&'static str, Version)>) -> Self {
        Self(Arc::new(
            services
                .into_iter()
                .map(|(name, v)| (name.to_string(), v))
                .collect(),
        ))
    }

    fn for_path(&self, path: &str) -> Option<(&str, Version)> {
        let service = path.trim_start_matches('/').split('/').next()?;
        self.0.get_key_value(service).map(|(k, v)| (k.as_str(), *v))
    }
}

/// Capa de servidor: rechaza las llamadas de clientes incompatibles y marca cada
/// respuesta con la versión del servicio.
#[derive(Clone, Debug)]
pub struct VersionLayer(ServiceVersions);

impl VersionLayer {
    pub fn new(versions: ServiceVersions) -> Self {
        Self(versions)
    }
}

impl<S> Layer<S> for VersionLayer {
    type Service = VersionCheck<S>;

    fn layer(&self, inner: S) -> VersionCheck<S> {
        VersionCheck {
            inner,
            versions: self.0.clone(),
        }
    }
}

/// Servicio de [`VersionLayer`].
#[derive(Clone, Debug)]
pub struct VersionCheck<S> {
    inner: S,
    versions: ServiceVersions,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

impl<S, B> Service<http::Request<B>> for VersionCheck<S>
where
    S: Service<http::Request<B>, Response = http::Response<Body>>,
    S::Future: Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = S::Error;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let Some((service, version)) = self.versions.for_path(req.uri().path()) else {
            return Box::pin(self.inner.call(req));
        };
        let stamp = HeaderValue::from_str(&version.to_string()).expect("versión ASCII");
        let client = req
            .headers()
            .get(HEADER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<Version>().ok());
        let compatible =
            client.is_some_and(|c| compatibility(version, c) != Compatibility::Incompatible);
        if !compatible {
            let mut response: http::Response<Body> =
                mismatch_status(service, version, client).into_http();
            response.headers_mut().insert(HEADER, stamp);
            return Box::pin(async move { Ok(response) });
        }
        let future = self.inner.call(req);
        Box::pin(async move {
            let mut response = future.await?;
            response.headers_mut().insert(HEADER, stamp);
            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_accepts_one_minor_of_skew() {
        let s = Version::new(1, 3);
        assert_eq!(compatibility(s, Version::new(1, 3)), Compatibility::Same);
        assert_eq!(
            compatibility(s, Version::new(1, 2)),
            Compatibility::MinorSkew
        );
        assert_eq!(
            compatibility(s, Version::new(1, 4)),
            Compatibility::MinorSkew
        );
        assert_eq!(
            compatibility(s, Version::new(1, 1)),
            Compatibility::Incompatible
        );
        assert_eq!(
            compatibility(s, Version::new(1, 5)),
            Compatibility::Incompatible
        );
        assert_eq!(
            compatibility(s, Version::new(2, 3)),
            Compatibility::Incompatible
        );
    }

    #[test]
    fn parses_and_prints() {
        assert_eq!("2.10".parse(), Ok(Version::new(2, 10)));
        assert_eq!(Version::new(1, 0).to_string(), "1.0");
        assert!("1".parse::<Version>().is_err());
        assert!("a.b".parse::<Version>().is_err());
    }
}
