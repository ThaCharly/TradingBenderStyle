//! Servidor del servicio de ejemplo, para el test de integración del cliente TS.
//! Escucha en 127.0.0.1 (puerto libre, o el de `--port N`), imprime `LISTENING <dir>` y
//! sirve hasta que se cierra su entrada estándar o recibe Ctrl-C.

use bender_transport::example::Example;
use bender_transport::server::serve;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .map_or(0, |p| p.parse::<u16>().expect("--port numérico"));
    let listener = TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
    println!("LISTENING {}", listener.local_addr().expect("dirección"));

    let shutdown = async {
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 64];
        let closed = async { while stdin.read(&mut buf).await.is_ok_and(|n| n > 0) {} };
        tokio::select! {
            () = closed => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    };
    let (routes, versions) = Example::default().routes();
    serve(listener, routes, versions, None, shutdown)
        .await
        .expect("servidor");
}
