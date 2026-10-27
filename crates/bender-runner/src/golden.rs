//! Test dorado "vivo = replay" (doc 05 §11.3): la corrida en vivo anota, por día UTC, el
//! hash de las salidas que emitió; `bender-golden` replaya ese día desde la grabación
//! (con el checkpoint del inicio del día) y compara.

use bender_kernel::KernelState;
use bender_kernel::canonical::{self, Checkpointable};
use bender_types::{NS_PER_DAY, proto};

use crate::{Consumer, RunError, Runner};

/// Hash de las salidas de un día: cadena BLAKE3 que arranca en cero al inicio del día.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DayHash {
    hash: [u8; 32],
    pub outputs: u64,
    pub events: u64,
}

impl DayHash {
    pub fn push<T: Checkpointable>(&mut self, output: &T) {
        let mut h = blake3::Hasher::new();
        h.update(&self.hash);
        h.update(&canonical::to_bytes(output));
        self.hash = *h.finalize().as_bytes();
        self.outputs += 1;
    }

    pub fn hex(&self) -> String {
        self.hash.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Línea del registro diario que deja la corrida en vivo:
/// `AAAA-MM-DD <hash> <eventos> <salidas>`.
pub fn day_line(day: &str, h: &DayHash) -> String {
    format!("{day} {} {} {}", h.hex(), h.events, h.outputs)
}

/// Replaya los eventos de un día (`day_start ≤ ts_available < day_start + 1 día`) a
/// partir del estado del núcleo al inicio del día y devuelve el hash de sus salidas.
pub fn replay_day<C: Consumer>(
    state_at_start: KernelState,
    consumer: C,
    day_start: i64,
    events: impl IntoIterator<Item = proto::Envelope>,
) -> Result<DayHash, RunError> {
    let mut runner = Runner::from_state(state_at_start, consumer);
    let mut out = Vec::new();
    let mut day = DayHash::default();
    for e in events {
        if e.ts_available < day_start || e.ts_available >= day_start + NS_PER_DAY {
            continue;
        }
        out.clear();
        runner.process(&e, &mut out)?;
        day.events += 1;
        for o in &out {
            day.push(o);
        }
    }
    Ok(day)
}
