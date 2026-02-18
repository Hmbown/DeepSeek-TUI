use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use tauri::Manager;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 7878;

struct RuntimeState {
  host: String,
  port: u16,
  child: Mutex<Option<Child>>,
}

impl RuntimeState {
  fn from_env() -> Self {
    let host = std::env::var("DEEPSEEK_APP_RUNTIME_HOST")
      .ok()
      .and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
      })
      .unwrap_or_else(|| DEFAULT_HOST.to_string());

    let port = std::env::var("DEEPSEEK_APP_RUNTIME_PORT")
      .ok()
      .and_then(|value| value.parse::<u16>().ok())
      .filter(|port| *port > 0)
      .unwrap_or(DEFAULT_PORT);

    Self {
      host,
      port,
      child: Mutex::new(None),
    }
  }

  fn base_url(&self) -> String {
    format!("http://{}:{}", self.host, self.port)
  }

  fn health_url(&self) -> String {
    format!("{}/health", self.base_url())
  }
}

fn runtime_is_healthy(state: &RuntimeState) -> bool {
  let client = match reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(2))
    .build()
  {
    Ok(client) => client,
    Err(_) => return false,
  };

  let response = match client.get(state.health_url()).send() {
    Ok(response) => response,
    Err(_) => return false,
  };

  response.status().is_success()
}

fn spawn_runtime(state: &RuntimeState) -> Result<Child, String> {
  Command::new("deepseek")
    .arg("serve")
    .arg("--http")
    .arg("--host")
    .arg(&state.host)
    .arg("--port")
    .arg(state.port.to_string())
    .arg("--workers")
    .arg("2")
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|error| format!("failed to spawn runtime server: {error}"))
}

fn ensure_runtime(state: &RuntimeState) -> Result<(), String> {
  if runtime_is_healthy(state) {
    return Ok(());
  }

  let child = spawn_runtime(state)?;
  {
    let mut guard = state
      .child
      .lock()
      .map_err(|_| "runtime process mutex poisoned".to_string())?;
    *guard = Some(child);
  }

  for _ in 0..60 {
    if runtime_is_healthy(state) {
      return Ok(());
    }
    thread::sleep(Duration::from_millis(250));
  }

  stop_runtime(state);
  Err("runtime did not become healthy after startup".to_string())
}

fn stop_runtime(state: &RuntimeState) {
  if let Ok(mut guard) = state.child.lock() {
    if let Some(mut child) = guard.take() {
      let _ = child.kill();
      let _ = child.wait();
    }
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let runtime_state = RuntimeState::from_env();

  let app = tauri::Builder::default()
    .plugin(
      tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
        .build(),
    )
    .manage(runtime_state)
    .setup(|app| {
      let state = app.state::<RuntimeState>();
      // Runtime bootstrap should never crash the desktop shell. If startup fails
      // (missing binary, port conflict, etc.), the UI still launches and the
      // frontend's health checks show disconnected/offline state.
      if let Err(error) = ensure_runtime(&state) {
        log::warn!("runtime bootstrap failed: {error}");
      }
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("error while building tauri app");

  app.run(|app_handle, event| {
    if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit) {
      let state = app_handle.state::<RuntimeState>();
      stop_runtime(&state);
    }
  });
}
