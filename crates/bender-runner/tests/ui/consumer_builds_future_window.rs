// Un Consumer que intenta obtener una FutureWindow para mirar el futuro: no compila,
// porque FutureWindow no tiene constructor público.

use bender_runner::{Consumer, FutureWindow, PastView};
use bender_types::proto;

struct Cheater;

impl Consumer for Cheater {
    type Output = u64;

    fn on_event(&mut self, _event: &proto::Envelope, _past: &PastView<'_>, out: &mut Vec<u64>) {
        let future = FutureWindow::new(&[], 0, i64::MAX);
        out.push(future.events().len() as u64);
    }
}

fn main() {}
