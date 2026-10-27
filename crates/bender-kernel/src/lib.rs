//! Núcleo temporal (doc 05): una sola librería con todas las reglas de tiempo del sistema.
//!
//! Este crate no lee el reloj del sistema (Pilar 2): el tiempo es el `ts_available` del
//! evento que se procesa. Lo garantizan el `clippy.toml` del crate y `tools/arch-tests`.

pub mod bars;
pub mod canonical;
pub mod clock;
pub mod composite;
pub mod coverage;
pub mod heartbeat;
pub mod merge;
pub mod order;
pub mod spec;
pub mod state;
pub mod user;
pub mod versioned;

pub use bars::{
    Bar, BarAggregator, BarConfig, BarError, BarEvent, BarEventKind, BarState, Timeframe, TradeTick,
};
pub use clock::{Fired, KernelClock, OutOfOrder, TimerId};
pub use composite::{
    CompositeAggregator, CompositeBar, CompositeConfig, CompositeDefinition, CompositeEvent, Ohlc,
    PriceRule, SourceVolume,
};
pub use coverage::{Coverage, CoverageProjection};
pub use heartbeat::{DEFAULT_GRID_NS, synthetic_heartbeats};
pub use merge::{Merge, MergeError};
pub use order::{Ordered, SourceOrder, SourceRankTable, UnknownSource, compare};
pub use spec::{SpecError, SpecHistory};
pub use state::{
    AvailabilityDelays, CheckpointSource, KernelConfig, KernelError, KernelOutput, KernelState,
    Restored, RunManifest, checkpoint_day_start, restore,
};
pub use user::{
    KnownVersions, UnknownPayload, UserObject, UserObjectKind, UserProjection, user_envelope,
};
pub use versioned::{StaleUpdate, VersionedStore};
