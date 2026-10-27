// Tampoco se puede armar una FutureWindow con un literal: sus campos son privados.

use bender_runner::FutureWindow;

fn main() {
    let _future = FutureWindow { events: &[], from: 0, horizon_end: i64::MAX };
}
