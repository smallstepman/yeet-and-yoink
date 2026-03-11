#[cfg(not(unix))]
compile_error!("browser native bridge requires a Unix platform");

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use crate::engine::topology::Direction;

pub const FIREFOX_NATIVE_HOST_NAME: &str = "com.yeet_and_yoink.firefox_bridge";
pub const FIREFOX_EXTENSION_ID: &str = "browser-bridge@yeet-and-yoink.dev";
pub const FIREFOX_NATIVE_SOCKET_ENV: &str = "NIRI_DEEP_FIREFOX_NATIVE_SOCKET";

const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SOCKET_BASENAME: &str = "firefox-bridge.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserBridgeErrorKind {
    Unavailable,
    Protocol,
    Remote,
}

#[derive(Debug)]
pub struct BrowserBridgeError {
    kind: BrowserBridgeErrorKind,
    message: String,
}

impl BrowserBridgeError {
    fn new(kind: BrowserBridgeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> BrowserBridgeErrorKind {
        self.kind
    }
}

impl std::fmt::Display for BrowserBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BrowserBridgeError {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BrowserTabState {
    #[serde(default, rename = "windowId")]
    pub window_id: Option<u64>,
    #[serde(default, rename = "activeTabId")]
    pub active_tab_id: Option<u64>,
    #[serde(rename = "activeTabIndex")]
    pub active_tab_index: usize,
    #[serde(rename = "tabCount")]
    pub tab_count: usize,
    #[serde(rename = "pinnedTabCount")]
    pub pinned_tab_count: usize,
    #[serde(rename = "activeTabPinned")]
    pub active_tab_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum BrowserBridgeCommand {
    GetTabState,
    Focus {
        direction: Direction,
    },
    MoveTab {
        direction: Direction,
    },
    TearOut,
    MergeTab {
        source_window_id: u64,
        source_tab_id: u64,
        direction: Direction,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostRequestMessage {
    id: u64,
    #[serde(flatten)]
    command: BrowserBridgeCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostResponseMessage {
    id: u64,
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<BrowserTabState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClientResponseMessage {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<BrowserTabState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

struct HostState {
    stdout: Mutex<io::Stdout>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, mpsc::Sender<HostResponseMessage>>>,
    running: AtomicBool,
}

impl HostState {
    fn new() -> Self {
        Self {
            stdout: Mutex::new(io::stdout()),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            running: AtomicBool::new(true),
        }
    }

    fn dispatch(&self, command: BrowserBridgeCommand) -> Result<ClientResponseMessage> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| anyhow!("browser bridge pending request table was poisoned"))?
            .insert(id, tx);

        let request = HostRequestMessage { id, command };
        if let Err(err) = self.write_request(&request) {
            self.pending
                .lock()
                .map_err(|_| anyhow!("browser bridge pending request table was poisoned"))?
                .remove(&id);
            return Err(err);
        }

        let response = rx
            .recv_timeout(REQUEST_TIMEOUT)
            .context("browser extension did not answer the native bridge request in time")?;
        Ok(ClientResponseMessage {
            ok: response.ok,
            state: response.state,
            error: response.error,
        })
    }

    fn write_request(&self, request: &HostRequestMessage) -> Result<()> {
        let mut stdout = self
            .stdout
            .lock()
            .map_err(|_| anyhow!("browser bridge stdout was poisoned"))?;
        write_native_message(&mut *stdout, request)
    }

    fn handle_response(&self, response: HostResponseMessage) {
        let Some(sender) = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&response.id))
        else {
            return;
        };
        let _ = sender.send(response);
    }

    fn fail_all_pending(&self, message: &str) {
        let mut pending = match self.pending.lock() {
            Ok(pending) => pending,
            Err(_) => return,
        };
        for (id, sender) in pending.drain() {
            let _ = sender.send(HostResponseMessage {
                id,
                ok: false,
                state: None,
                error: Some(message.to_string()),
            });
        }
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.fail_all_pending("browser extension disconnected from the native bridge");
    }
}

pub fn browser_bridge_socket_path() -> PathBuf {
    if let Some(value) = std::env::var_os(FIREFOX_NATIVE_SOCKET_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return value;
    }

    default_socket_root().join(SOCKET_BASENAME)
}

pub fn tab_state() -> std::result::Result<BrowserTabState, BrowserBridgeError> {
    let response = request(BrowserBridgeCommand::GetTabState)?;
    response.state.ok_or_else(|| {
        BrowserBridgeError::new(
            BrowserBridgeErrorKind::Protocol,
            "browser bridge get_tab_state response was missing tab state",
        )
    })
}

pub fn focus(direction: Direction) -> std::result::Result<(), BrowserBridgeError> {
    request(BrowserBridgeCommand::Focus { direction }).map(|_| ())
}

pub fn move_tab(direction: Direction) -> std::result::Result<(), BrowserBridgeError> {
    request(BrowserBridgeCommand::MoveTab { direction }).map(|_| ())
}

pub fn tear_out() -> std::result::Result<(), BrowserBridgeError> {
    request(BrowserBridgeCommand::TearOut).map(|_| ())
}

pub fn merge_tab_into_focused_window(
    direction: Direction,
    source_window_id: u64,
    source_tab_id: u64,
) -> std::result::Result<(), BrowserBridgeError> {
    request(BrowserBridgeCommand::MergeTab {
        source_window_id,
        source_tab_id,
        direction,
    })
    .map(|_| ())
}

fn request(
    command: BrowserBridgeCommand,
) -> std::result::Result<ClientResponseMessage, BrowserBridgeError> {
    let socket_path = browser_bridge_socket_path();
    let mut stream = UnixStream::connect(&socket_path).map_err(|err| {
        BrowserBridgeError::new(
            BrowserBridgeErrorKind::Unavailable,
            format!(
                "browser native bridge is unavailable at {}: {}. Install/enable the yeet-and-yoink browser extension and keep LibreWolf/Firefox running.",
                socket_path.display(),
                err
            ),
        )
    })?;
    stream
        .set_read_timeout(Some(SOCKET_IO_TIMEOUT))
        .map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                format!("failed to configure browser bridge read timeout: {err}"),
            )
        })?;
    stream
        .set_write_timeout(Some(SOCKET_IO_TIMEOUT))
        .map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                format!("failed to configure browser bridge write timeout: {err}"),
            )
        })?;

    serde_json::to_writer(&mut stream, &command).map_err(|err| {
        BrowserBridgeError::new(
            BrowserBridgeErrorKind::Protocol,
            format!("failed to serialize browser bridge request: {err}"),
        )
    })?;
    stream.write_all(b"\n").map_err(|err| {
        BrowserBridgeError::new(
            BrowserBridgeErrorKind::Protocol,
            format!("failed to terminate browser bridge request: {err}"),
        )
    })?;
    stream.flush().map_err(|err| {
        BrowserBridgeError::new(
            BrowserBridgeErrorKind::Protocol,
            format!("failed to flush browser bridge request: {err}"),
        )
    })?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).map_err(|err| {
        BrowserBridgeError::new(
            BrowserBridgeErrorKind::Protocol,
            format!("failed to read browser bridge response: {err}"),
        )
    })?;
    if bytes == 0 {
        return Err(BrowserBridgeError::new(
            BrowserBridgeErrorKind::Unavailable,
            "browser native bridge closed the socket before replying",
        ));
    }
    let response: ClientResponseMessage = serde_json::from_str(line.trim()).map_err(|err| {
        BrowserBridgeError::new(
            BrowserBridgeErrorKind::Protocol,
            format!("failed to parse browser bridge response: {err}"),
        )
    })?;
    if !response.ok {
        return Err(BrowserBridgeError::new(
            BrowserBridgeErrorKind::Remote,
            response
                .error
                .clone()
                .unwrap_or_else(|| "browser bridge command failed".to_string()),
        ));
    }
    Ok(response)
}

pub fn run_native_host() -> Result<()> {
    let socket_path = browser_bridge_socket_path();
    let listener = bind_socket(&socket_path)?;
    listener
        .set_nonblocking(true)
        .with_context(|| format!("failed to make {} nonblocking", socket_path.display()))?;

    let state = Arc::new(HostState::new());
    let reader_state = Arc::clone(&state);
    let wake_path = socket_path.clone();
    let reader = thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        let result = read_extension_loop(&mut stdin, &reader_state);
        reader_state.stop();
        let _ = UnixStream::connect(&wake_path);
        result
    });

    while state.running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let _ = handle_local_client(stream, &state);
                });
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(err) => {
                state.stop();
                return Err(err)
                    .with_context(|| format!("failed to accept {}", socket_path.display()));
            }
        }
    }

    match reader.join() {
        Ok(result) => result?,
        Err(_) => bail!("browser native bridge stdin thread panicked"),
    }

    Ok(())
}

fn handle_local_client(stream: UnixStream, state: &HostState) -> Result<()> {
    stream
        .set_read_timeout(Some(SOCKET_IO_TIMEOUT))
        .context("failed to configure browser bridge local read timeout")?;
    stream
        .set_write_timeout(Some(SOCKET_IO_TIMEOUT))
        .context("failed to configure browser bridge local write timeout")?;

    let mut reader = BufReader::new(stream.try_clone().context("failed to clone local stream")?);
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .context("failed to read local bridge request")?;
    if bytes == 0 {
        bail!("local browser bridge client closed without sending a request");
    }
    let command: BrowserBridgeCommand =
        serde_json::from_str(line.trim()).context("failed to parse local bridge request")?;
    let response = match state.dispatch(command) {
        Ok(response) => response,
        Err(err) => ClientResponseMessage {
            ok: false,
            state: None,
            error: Some(format!("{err:#}")),
        },
    };

    let mut stream = reader.into_inner();
    serde_json::to_writer(&mut stream, &response)
        .context("failed to serialize local bridge response")?;
    stream
        .write_all(b"\n")
        .context("failed to terminate local bridge response")?;
    stream
        .flush()
        .context("failed to flush local bridge response")?;
    Ok(())
}

fn read_extension_loop(reader: &mut dyn Read, state: &HostState) -> Result<()> {
    loop {
        let Some(payload) = read_native_message(reader)? else {
            return Ok(());
        };
        let response: HostResponseMessage =
            serde_json::from_slice(&payload).context("failed to parse browser extension reply")?;
        state.handle_response(response);
    }
}

fn bind_socket(path: &Path) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create browser native bridge directory {}",
                parent.display()
            )
        })?;
    }

    if path.exists() {
        match UnixStream::connect(path) {
            Ok(_) => bail!(
                "browser native bridge socket {} is already active",
                path.display()
            ),
            Err(_) => fs::remove_file(path).with_context(|| {
                format!(
                    "failed to remove stale browser native bridge socket {}",
                    path.display()
                )
            })?,
        }
    }

    let listener = UnixListener::bind(path).with_context(|| {
        format!(
            "failed to bind browser native bridge socket {}",
            path.display()
        )
    })?;
    Ok(listener)
}

fn default_socket_root() -> PathBuf {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        home.join("Library")
            .join("Application Support")
            .join("yeet-and-yoink")
    } else {
        std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("yeet-and-yoink")
    }
}

fn write_native_message(writer: &mut dyn Write, payload: &impl Serialize) -> Result<()> {
    let body = serde_json::to_vec(payload).context("failed to encode browser native message")?;
    let len = u32::try_from(body.len()).context("browser native message was unexpectedly large")?;
    writer
        .write_all(&len.to_ne_bytes())
        .context("failed to write browser native message length")?;
    writer
        .write_all(&body)
        .context("failed to write browser native message body")?;
    writer
        .flush()
        .context("failed to flush browser native message")
}

fn read_native_message(reader: &mut dyn Read) -> Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err).context("failed to read browser native message length"),
    }
    let len = u32::from_ne_bytes(len) as usize;
    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .context("failed to read browser native message body")?;
    Ok(Some(payload))
}

#[cfg(test)]
mod tests {
    use super::{
        browser_bridge_socket_path, read_native_message, write_native_message,
        BrowserBridgeCommand, FIREFOX_NATIVE_SOCKET_ENV,
    };
    use crate::engine::topology::Direction;
    use serde_json::json;
    use std::io::Cursor;

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    #[test]
    fn socket_path_uses_env_override() {
        let _guard = env_guard();
        let old = std::env::var_os(FIREFOX_NATIVE_SOCKET_ENV);
        std::env::set_var(FIREFOX_NATIVE_SOCKET_ENV, "/tmp/yny-firefox-test.sock");

        assert_eq!(
            browser_bridge_socket_path(),
            std::path::PathBuf::from("/tmp/yny-firefox-test.sock")
        );

        if let Some(old) = old {
            std::env::set_var(FIREFOX_NATIVE_SOCKET_ENV, old);
        } else {
            std::env::remove_var(FIREFOX_NATIVE_SOCKET_ENV);
        }
    }

    #[test]
    fn native_message_roundtrips() {
        let payload = json!({
            "id": 7,
            "command": "focus",
            "direction": "East",
        });
        let mut bytes = Vec::new();
        write_native_message(&mut bytes, &payload).expect("message should encode");
        let decoded = read_native_message(&mut Cursor::new(bytes))
            .expect("message should decode")
            .expect("message should exist");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&decoded).expect("json should parse"),
            payload
        );
    }

    #[test]
    fn browser_bridge_command_serializes_direction() {
        let value = serde_json::to_value(BrowserBridgeCommand::MoveTab {
            direction: Direction::East,
        })
        .expect("command should serialize");
        assert_eq!(
            value,
            json!({
                "command": "move_tab",
                "direction": "East",
            })
        );
    }

    #[test]
    fn browser_bridge_merge_command_serializes_payload() {
        let value = serde_json::to_value(BrowserBridgeCommand::MergeTab {
            source_window_id: 17,
            source_tab_id: 23,
            direction: Direction::North,
        })
        .expect("command should serialize");
        assert_eq!(
            value,
            json!({
                "command": "merge_tab",
                "source_window_id": 17,
                "source_tab_id": 23,
                "direction": "North",
            })
        );
    }
}
