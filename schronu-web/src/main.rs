#[cfg(feature = "server")]
#[tokio::main]
async fn main() {
    use dioxus::fullstack::axum::Extension;
    let today_worker = schronu_web::app::worker_from_environment();
    let web_worker = schronu_web::app::web_worker_from_environment();
    let address = loopback_server_address(dioxus::cli_config::server_port().unwrap_or(8080));
    let router = dioxus::server::router(schronu_web::app::app)
        .layer(Extension(today_worker))
        .layer(Extension(web_worker));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("loopback web server must bind");
    dioxus::fullstack::axum::serve(listener, router)
        .await
        .expect("loopback web server must run");
}

#[cfg(feature = "server")]
fn loopback_server_address(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from(([127, 0, 0, 1], port))
}

#[cfg(all(not(feature = "server"), feature = "web"))]
fn main() {
    dioxus::launch(schronu_web::app::app);
}

#[cfg(not(any(feature = "server", feature = "web")))]
fn main() {
    eprintln!("enable the web or server feature");
}

#[cfg(all(test, feature = "server"))]
mod tests {
    #[test]
    fn server_address_is_always_loopback() {
        let address = super::loopback_server_address(4321);

        assert!(address.ip().is_loopback());
        assert_eq!(address.port(), 4321);
    }
}
