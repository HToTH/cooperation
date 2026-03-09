use anyhow::{anyhow, Result};
use std::{
    net::{SocketAddr, TcpListener as StdTcpListener, TcpStream},
    sync::mpsc::Receiver,
    thread,
    time::{Duration, Instant},
};
use tauri::Manager;

fn main() {
    let (reserved_listener, server_addr) =
        reserve_embedded_backend_listener().expect("failed to reserve backend port");
    let runtime_script = build_runtime_script(server_addr);

    tauri::Builder::default()
        .append_invoke_initialization_script(runtime_script)
        .setup(move |app| {
            start_embedded_backend(app, reserved_listener, server_addr)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running cooperation desktop");
}

fn reserve_embedded_backend_listener() -> Result<(StdTcpListener, SocketAddr)> {
    let listener = StdTcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

fn build_runtime_script(server_addr: SocketAddr) -> String {
    let api_base = format!("http://{}", server_addr);
    let ws_base = format!("ws://{}", server_addr);

    format!(
        "window.__COOPERATION_RUNTIME__ = Object.freeze({{ apiBase: {:?}, wsBase: {:?} }});",
        api_base, ws_base
    )
}

fn start_embedded_backend(
    app: &mut tauri::App,
    reserved_listener: StdTcpListener,
    server_addr: SocketAddr,
) -> Result<()> {
    let app_data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_data_dir)?;

    let database_url = format!("sqlite:{}", app_data_dir.join("cooperation.db").display());
    let (startup_tx, startup_rx) = std::sync::mpsc::channel();

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(reserved_listener) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = startup_tx.send(Err(error.into()));
                return;
            }
        };

        let result = agentflow_server::run_server_with_listener(&database_url, listener).await;
        let _ = startup_tx.send(result);
    });

    wait_for_backend(server_addr, &startup_rx, Duration::from_secs(5))
}

fn wait_for_backend(
    server_addr: SocketAddr,
    startup_rx: &Receiver<anyhow::Result<()>>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        match startup_rx.try_recv() {
            Ok(result) => return result,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(anyhow!("embedded backend terminated during startup"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        if TcpStream::connect_timeout(&server_addr, Duration::from_millis(100)).is_ok() {
            return Ok(());
        }

        thread::sleep(Duration::from_millis(100));
    }

    match startup_rx.try_recv() {
        Ok(result) => result,
        Err(_) => Err(anyhow!(
            "timed out waiting for embedded backend on {}",
            server_addr
        )),
    }
}
