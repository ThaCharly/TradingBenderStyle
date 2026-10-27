//! Proyección de los eventos del usuario (doc 05 §7): dibujos, niveles, alertas y
//! configuración, consultables `as_of`.

use bender_types::proto::{self, envelope, source_id, user_event};

use crate::versioned::{StaleUpdate, VersionedStore};

/// Tipo de objeto del usuario.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum UserObjectKind {
    Drawing,
    Level,
    Alert,
    Config,
}

/// Contenido de un objeto: el payload opaco, su versión y quién lo escribió.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserObject {
    pub version: u32,
    pub data: Vec<u8>,
    /// "user" o "ai:<cliente>".
    pub author: String,
}

/// Un payload con versión que esta proyección no conoce: se guardó igual, opaco.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownPayload {
    pub kind: UserObjectKind,
    pub id: String,
    pub version: u32,
    pub ts_available: i64,
}

/// Versiones de payload que entiende quien consume la proyección, por tipo. Las demás
/// se conservan y se reportan en [`UserProjection::unknown_payloads`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownVersions {
    pub drawing: u32,
    pub level: u32,
    pub alert: u32,
    pub config: u32,
}

impl Default for KnownVersions {
    fn default() -> Self {
        Self {
            drawing: 1,
            level: 1,
            alert: 1,
            config: 1,
        }
    }
}

/// Estado de los objetos del usuario, versionado en el tiempo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserProjection {
    known: KnownVersions,
    objects: VersionedStore<(UserObjectKind, String), UserObject>,
    unknown: Vec<UnknownPayload>,
}

impl Default for UserProjection {
    fn default() -> Self {
        Self::new(KnownVersions::default())
    }
}

impl UserProjection {
    pub fn new(known: KnownVersions) -> Self {
        Self {
            known,
            objects: VersionedStore::new(),
            unknown: Vec::new(),
        }
    }

    /// Aplica un `Envelope` en orden K. Ignora todo lo que no es un `UserEvent`.
    pub fn apply(&mut self, env: &proto::Envelope) -> Result<(), StaleUpdate> {
        let Some(envelope::Payload::User(ev)) = &env.payload else {
            return Ok(());
        };
        let author = ev.author.clone().unwrap_or_else(|| "user".into());
        let ts = env.ts_available;
        use user_event::Event;
        let (kind, id, payload) = match &ev.event {
            Some(Event::DrawingUpserted(e)) => {
                (UserObjectKind::Drawing, &e.drawing_id, Some(&e.payload))
            }
            Some(Event::DrawingDeleted(e)) => (UserObjectKind::Drawing, &e.drawing_id, None),
            Some(Event::LevelMarked(e)) => (UserObjectKind::Level, &e.level_id, Some(&e.payload)),
            Some(Event::AlertDefined(e)) => (UserObjectKind::Alert, &e.alert_id, Some(&e.payload)),
            Some(Event::AlertRemoved(e)) => (UserObjectKind::Alert, &e.alert_id, None),
            Some(Event::ConfigChanged(e)) => (UserObjectKind::Config, &e.key, Some(&e.payload)),
            None => return Ok(()),
        };
        let value = payload.map(|p| {
            let p = p.clone().unwrap_or_default();
            UserObject {
                version: p.version,
                data: p.data,
                author: author.clone(),
            }
        });
        if let Some(v) = &value {
            let known = match kind {
                UserObjectKind::Drawing => self.known.drawing,
                UserObjectKind::Level => self.known.level,
                UserObjectKind::Alert => self.known.alert,
                UserObjectKind::Config => self.known.config,
            };
            if v.version == 0 || v.version > known {
                self.unknown.push(UnknownPayload {
                    kind,
                    id: id.clone(),
                    version: v.version,
                    ts_available: ts,
                });
            }
        }
        self.objects.apply(ts, (kind, id.clone()), value)
    }

    /// Aplica una secuencia de eventos en lote (mismo resultado que uno por uno).
    pub fn apply_all<'a>(
        &mut self,
        envs: impl IntoIterator<Item = &'a proto::Envelope>,
    ) -> Result<(), StaleUpdate> {
        envs.into_iter().try_for_each(|e| self.apply(e))
    }

    pub fn get(&self, kind: UserObjectKind, id: &str, as_of: i64) -> Option<&UserObject> {
        self.objects.get(&(kind, id.to_string()), as_of)
    }

    /// Objetos vivos de un tipo en `as_of`, por id.
    pub fn list(
        &self,
        kind: UserObjectKind,
        as_of: i64,
    ) -> impl Iterator<Item = (&str, &UserObject)> {
        self.objects
            .list(as_of)
            .filter(move |((k, _), _)| *k == kind)
            .map(|((_, id), v)| (id.as_str(), v))
    }

    /// Payloads de versión desconocida que se conservaron opacos.
    pub fn unknown_payloads(&self) -> &[UnknownPayload] {
        &self.unknown
    }
}

/// Sobre de un evento del usuario: `ts_available` = recepción en el servidor
/// (`received_at`), nunca la hora del cliente; la hora que informa el cliente, si la hay,
/// queda como `ts_event`.
pub fn user_envelope(
    stream: &str,
    ingest_seq: u64,
    client_ts: Option<i64>,
    received_at: i64,
    event: proto::UserEvent,
) -> proto::Envelope {
    proto::Envelope {
        ts_event: client_ts.unwrap_or(received_at),
        ts_received: Some(received_at),
        ts_available: received_at,
        source: Some(proto::SourceId {
            kind: Some(source_id::Kind::User(proto::UserSource {
                stream: stream.into(),
            })),
        }),
        source_seq: None,
        ingest_seq,
        provenance: proto::Provenance::Live as i32,
        payload: Some(envelope::Payload::User(event)),
    }
}

// ---------------------------------------------------------------------------------------
// Serialización canónica (checkpoints).

use crate::canonical::{CheckpointError, Checkpointable, Decoder, Encoder};

impl Checkpointable for UserObjectKind {
    fn encode(&self, e: &mut Encoder) {
        e.put(&(*self as u8));
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        match d.get::<u8>()? {
            0 => Ok(Self::Drawing),
            1 => Ok(Self::Level),
            2 => Ok(Self::Alert),
            3 => Ok(Self::Config),
            x => Err(CheckpointError::Invalid(format!("UserObjectKind {x}"))),
        }
    }
}

crate::checkpoint_struct!(UserObject {
    version,
    data,
    author
});
crate::checkpoint_struct!(UnknownPayload {
    kind,
    id,
    version,
    ts_available
});
crate::checkpoint_struct!(KnownVersions {
    drawing,
    level,
    alert,
    config
});
crate::checkpoint_struct!(UserProjection {
    known,
    objects,
    unknown
});
