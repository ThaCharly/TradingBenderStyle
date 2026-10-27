//! Criterios de #27: eventos del usuario y estado versionado `as_of`.

use bender_kernel::{KnownVersions, UserObjectKind, UserProjection, VersionedStore, user_envelope};
use bender_types::proto::{self, user_event};

fn payload(version: u32, data: &str) -> Option<proto::OpaquePayload> {
    Some(proto::OpaquePayload {
        version,
        data: data.as_bytes().to_vec(),
    })
}

fn ev(event: user_event::Event) -> proto::UserEvent {
    proto::UserEvent {
        author: None,
        event: Some(event),
    }
}

const T1: i64 = 1_000;
const T2: i64 = 2_000;
const T3: i64 = 3_000;

fn drawing_lifecycle() -> Vec<proto::Envelope> {
    vec![
        user_envelope(
            "drawings",
            1,
            Some(T1 - 400), // hora del cliente: no manda
            T1,
            ev(user_event::Event::DrawingUpserted(proto::DrawingUpserted {
                drawing_id: "linea-1".into(),
                payload: payload(1, "horizontal 88000"),
            })),
        ),
        user_envelope(
            "drawings",
            2,
            None,
            T2,
            ev(user_event::Event::DrawingUpserted(proto::DrawingUpserted {
                drawing_id: "linea-1".into(),
                payload: payload(1, "horizontal 88500"),
            })),
        ),
        user_envelope(
            "drawings",
            3,
            None,
            T3,
            ev(user_event::Event::DrawingDeleted(proto::DrawingDeleted {
                drawing_id: "linea-1".into(),
            })),
        ),
    ]
}

#[test]
fn create_modify_delete_answers_as_of() {
    let mut p = UserProjection::default();
    p.apply_all(&drawing_lifecycle()).unwrap();
    let at = |t| {
        p.get(UserObjectKind::Drawing, "linea-1", t)
            .map(|o| String::from_utf8(o.data.clone()).unwrap())
    };
    assert_eq!(at(T1 - 1), None, "antes de crearlo");
    assert_eq!(at(T1).as_deref(), Some("horizontal 88000"));
    assert_eq!(
        at(T2 - 1).as_deref(),
        Some("horizontal 88000"),
        "entre T1 y T2"
    );
    assert_eq!(at(T2).as_deref(), Some("horizontal 88500"));
    assert_eq!(
        at(T3 - 1).as_deref(),
        Some("horizontal 88500"),
        "entre T2 y T3"
    );
    assert_eq!(at(T3), None, "borrado");
    assert_eq!(at(i64::MAX), None);
    assert_eq!(p.list(UserObjectKind::Drawing, T2).count(), 1);
    assert_eq!(p.list(UserObjectKind::Drawing, T3).count(), 0);
    assert_eq!(
        p.get(UserObjectKind::Drawing, "linea-1", T1)
            .unwrap()
            .author,
        "user"
    );
}

#[test]
fn user_envelope_uses_server_reception_time() {
    let env = &drawing_lifecycle()[0];
    assert_eq!(env.ts_available, T1);
    assert_eq!(env.ts_received, Some(T1));
    assert_eq!(env.ts_event, T1 - 400);
    assert_eq!(
        bender_types::source_key(env.source.as_ref().unwrap()),
        "USER:drawings"
    );
}

/// Varios tipos y autores mezclados: incremental (uno por uno, consultando en el medio)
/// y en lote dan la misma proyección.
#[test]
fn incremental_equals_batch() {
    let mut events = drawing_lifecycle();
    let mut seq = 10;
    let mut push = |t: i64, author: Option<&str>, e: user_event::Event| {
        seq += 1;
        let mut ue = ev(e);
        ue.author = author.map(str::to_string);
        user_envelope("misc", seq, None, t, ue)
    };
    events.extend([
        push(
            1_500,
            None,
            user_event::Event::LevelMarked(proto::LevelMarked {
                level_id: "n1".into(),
                payload: payload(1, "soporte"),
            }),
        ),
        push(
            1_500,
            Some("ai:claude"),
            user_event::Event::AlertDefined(proto::AlertDefined {
                alert_id: "a1".into(),
                payload: payload(1, "cruce 90k"),
            }),
        ),
        push(
            2_500,
            None,
            user_event::Event::ConfigChanged(proto::ConfigChanged {
                key: "chart.theme".into(),
                payload: payload(1, "oscuro"),
            }),
        ),
        push(
            2_600,
            None,
            user_event::Event::AlertRemoved(proto::AlertRemoved {
                alert_id: "a1".into(),
            }),
        ),
        push(
            2_700,
            None,
            user_event::Event::ConfigChanged(proto::ConfigChanged {
                key: "chart.theme".into(),
                payload: payload(1, "claro"),
            }),
        ),
    ]);
    events.sort_by_key(|e| e.ts_available);

    let mut incremental = UserProjection::default();
    for e in &events {
        incremental.apply(e).unwrap();
        let _ = incremental
            .list(UserObjectKind::Alert, e.ts_available)
            .count();
    }
    let mut batch = UserProjection::default();
    batch.apply_all(&events).unwrap();
    assert_eq!(incremental, batch);

    assert_eq!(
        batch
            .get(UserObjectKind::Alert, "a1", 2_000)
            .unwrap()
            .author,
        "ai:claude"
    );
    assert!(batch.get(UserObjectKind::Alert, "a1", 2_600).is_none());
    assert_eq!(
        batch
            .get(UserObjectKind::Config, "chart.theme", 2_650)
            .unwrap()
            .data,
        b"oscuro"
    );
    assert_eq!(
        batch
            .get(UserObjectKind::Config, "chart.theme", 9_999)
            .unwrap()
            .data,
        b"claro"
    );
    assert_eq!(
        batch.get(UserObjectKind::Level, "n1", 9_999).unwrap().data,
        b"soporte"
    );
}

#[test]
fn unknown_payload_version_is_kept_opaque_and_reported() {
    let mut p = UserProjection::new(KnownVersions {
        drawing: 2,
        ..KnownVersions::default()
    });
    let future = user_envelope(
        "drawings",
        1,
        None,
        T1,
        ev(user_event::Event::DrawingUpserted(proto::DrawingUpserted {
            drawing_id: "fibo-7".into(),
            payload: payload(9, "\u{1}formato que todavía no existe"),
        })),
    );
    p.apply(&future).unwrap();
    // Un evento posterior se sigue aplicando bien.
    p.apply_all(&drawing_lifecycle()[1..2]).unwrap();
    let kept = p.get(UserObjectKind::Drawing, "fibo-7", T1).unwrap();
    assert_eq!(kept.version, 9);
    assert_eq!(kept.data, "\u{1}formato que todavía no existe".as_bytes());
    assert_eq!(p.unknown_payloads().len(), 1);
    assert_eq!(p.unknown_payloads()[0].id, "fibo-7");
    assert_eq!(p.unknown_payloads()[0].version, 9);
    assert!(p.get(UserObjectKind::Drawing, "linea-1", T2).is_some());
}

#[test]
fn versioned_store_rejects_going_back_in_time() {
    let mut s: VersionedStore<&str, i32> = VersionedStore::new();
    s.apply(10, "x", Some(1)).unwrap();
    s.apply(10, "x", Some(2)).unwrap(); // mismo instante: vale el último
    assert_eq!(s.get(&"x", 10), Some(&2));
    assert!(s.apply(9, "x", Some(3)).is_err());
    assert_eq!(s.history(&"x").len(), 2);
    // Eventos que no son del usuario: la proyección los ignora.
    let mut p = UserProjection::default();
    p.apply(&proto::Envelope::default()).unwrap();
    assert_eq!(p, UserProjection::default());
}
