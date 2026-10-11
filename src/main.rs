use clap::Parser;
use screenbridge::{config::Config, server};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
};
use tokio_util::sync::CancellationToken;
#[derive(Parser)]
#[command(version, about = "Experimental macOS desktop streaming service")]
struct Args {
    #[arg(long)]
    config: PathBuf,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let port = config.listen_port;
    let shutdown = CancellationToken::new();
    let app = server::App::new(config, shutdown.clone());
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await?;
    println!("Screenbridge listening on loopback port {port}");
    let signal = shutdown.clone();
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::spawn(async move {
        #[cfg(unix)]
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}};
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        signal.cancel();
    });
    axum::serve(listener, server::router(app))
        .with_graceful_shutdown(async move { shutdown.cancelled().await })
        .await?;
    Ok(())
}
