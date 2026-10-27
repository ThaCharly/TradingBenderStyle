//! Benchmark de #23: fusión de 8 fuentes. Criterio: ≥ 5 M eventos/s en la PC de
//! desarrollo. Correr con `cargo bench -p bender-kernel --bench merge`.

use bender_kernel::{Merge, Ordered, SourceOrder};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

#[derive(Clone, Copy)]
struct Ev {
    ts: i64,
    seq: Option<u64>,
    ingest: u64,
}

impl Ordered for Ev {
    fn ts_available(&self) -> i64 {
        self.ts
    }
    fn source_seq(&self) -> Option<u64> {
        self.seq
    }
    fn ingest_seq(&self) -> u64 {
        self.ingest
    }
}

const SOURCES: usize = 8;
const PER_SOURCE: usize = 250_000;

/// Fuentes con timestamps entrelazados de forma pseudoaleatoria (SplitMix64, semilla fija).
fn data() -> Vec<Vec<Ev>> {
    let mut state = 0x5eed_u64;
    let mut next = move || {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    (0..SOURCES)
        .map(|_| {
            let mut ts = 1_761_600_000_000_000_000i64;
            (0..PER_SOURCE as u64)
                .map(|i| {
                    ts += (next() % 2_000_000) as i64;
                    Ev {
                        ts,
                        seq: Some(i),
                        ingest: i + 1,
                    }
                })
                .collect()
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let data = data();
    let mut group = c.benchmark_group("merge");
    group.throughput(Throughput::Elements((SOURCES * PER_SOURCE) as u64));
    group.sample_size(20);
    group.bench_function("8 fuentes × 250k", |b| {
        b.iter(|| {
            let streams = data.iter().enumerate().map(|(i, events)| {
                let order = SourceOrder {
                    rank: 2 + (i % 6) as u32,
                    key: format!("VENUE:PERP_LINEAR:S{i}:trades"),
                };
                (order, events.iter().copied())
            });
            let mut checksum = 0u64;
            for ev in Merge::new(streams).unwrap() {
                checksum = checksum.wrapping_add(ev.ingest);
            }
            std::hint::black_box(checksum)
        })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
