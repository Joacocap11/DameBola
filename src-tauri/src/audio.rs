//! Motor de audio único. Un hilo dedicado es dueño del dispositivo de salida
//! (los streams de audio no son `Send`) y garantiza que solo suene UNA pista
//! a la vez, sin importar quién la pida (vista previa o alerta).

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::cpal::BufferSize;
use rodio::{DeviceSinkBuilder, DeviceSinkError, MixerDeviceSink, Player};
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackOrigin {
    /// El usuario escucha una canción desde el listado.
    Preview,
    /// Música de una alerta.
    Alert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    pub state: PlaybackState,
    pub song_id: Option<String>,
    pub origin: Option<PlaybackOrigin>,
}

impl PlaybackStatus {
    fn stopped() -> Self {
        Self { state: PlaybackState::Stopped, song_id: None, origin: None }
    }
}

enum Command {
    Play {
        path: PathBuf,
        song_id: String,
        origin: PlaybackOrigin,
        /// 0.0..=1.0
        volume: f32,
        reply: Sender<AppResult<()>>,
    },
    Pause,
    Resume,
    Stop,
    StopOrigin(PlaybackOrigin),
}

#[derive(Clone)]
pub struct AudioPlayer {
    tx: Sender<Command>,
    status: Arc<Mutex<PlaybackStatus>>,
}

impl AudioPlayer {
    /// `on_change` se invoca desde el hilo de audio en cada cambio de estado.
    pub fn spawn(on_change: impl Fn(&PlaybackStatus) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(PlaybackStatus::stopped()));
        let thread_status = Arc::clone(&status);
        std::thread::Builder::new()
            .name("damebola-audio".into())
            .spawn(move || run(rx, thread_status, on_change))
            .expect("no se pudo crear el hilo de audio");
        Self { tx, status }
    }

    pub fn status(&self) -> PlaybackStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Reemplaza cualquier reproducción en curso. Bloquea hasta saber si
    /// la pista pudo iniciarse.
    pub fn play(&self, path: PathBuf, song_id: String, origin: PlaybackOrigin, volume_percent: u8) -> AppResult<()> {
        let (reply, wait) = mpsc::channel();
        let volume = f32::from(volume_percent.min(100)) / 100.0;
        self.send(Command::Play { path, song_id, origin, volume, reply })?;
        wait.recv_timeout(Duration::from_secs(10))
            .map_err(|_| AppError::Audio("El motor de audio no respondió.".into()))?
    }

    pub fn pause(&self) -> AppResult<()> {
        self.send(Command::Pause)
    }

    pub fn resume(&self) -> AppResult<()> {
        self.send(Command::Resume)
    }

    pub fn stop(&self) -> AppResult<()> {
        self.send(Command::Stop)
    }

    /// Detiene la reproducción solo si fue iniciada por `origin`.
    pub fn stop_origin(&self, origin: PlaybackOrigin) -> AppResult<()> {
        self.send(Command::StopOrigin(origin))
    }

    fn send(&self, cmd: Command) -> AppResult<()> {
        self.tx
            .send(cmd)
            .map_err(|_| AppError::Audio("El motor de audio no está disponible.".into()))
    }
}

fn run(
    rx: mpsc::Receiver<Command>,
    shared: Arc<Mutex<PlaybackStatus>>,
    on_change: impl Fn(&PlaybackStatus),
) {
    let mut device: Option<MixerDeviceSink> = None;
    let mut player: Option<Player> = None;
    let mut status = PlaybackStatus::stopped();

    let publish = |new: &PlaybackStatus, status: &mut PlaybackStatus| {
        if new != status {
            *status = new.clone();
            *shared.lock().unwrap_or_else(|e| e.into_inner()) = new.clone();
            on_change(new);
        }
    };

    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Command::Play { path, song_id, origin, volume, reply }) => {
                // Detiene lo anterior antes de abrir lo nuevo.
                player = None;
                publish(&PlaybackStatus::stopped(), &mut status);
                match start(&mut device, &path, volume) {
                    Ok(p) => {
                        player = Some(p);
                        publish(
                            &PlaybackStatus {
                                state: PlaybackState::Playing,
                                song_id: Some(song_id),
                                origin: Some(origin),
                            },
                            &mut status,
                        );
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Ok(Command::Pause) => {
                if let (Some(p), PlaybackState::Playing) = (&player, status.state) {
                    p.pause();
                    publish(&PlaybackStatus { state: PlaybackState::Paused, ..status.clone() }, &mut status);
                }
            }
            Ok(Command::Resume) => {
                if let (Some(p), PlaybackState::Paused) = (&player, status.state) {
                    p.play();
                    publish(&PlaybackStatus { state: PlaybackState::Playing, ..status.clone() }, &mut status);
                }
            }
            Ok(Command::Stop) => {
                player = None;
                publish(&PlaybackStatus::stopped(), &mut status);
            }
            Ok(Command::StopOrigin(origin)) => {
                if status.origin == Some(origin) {
                    player = None;
                    publish(&PlaybackStatus::stopped(), &mut status);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Fin natural de la pista.
                if status.state == PlaybackState::Playing && player.as_ref().is_some_and(|p| p.empty()) {
                    player = None;
                    publish(&PlaybackStatus::stopped(), &mut status);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn start(device: &mut Option<MixerDeviceSink>, path: &PathBuf, volume: f32) -> AppResult<Player> {
    if device.is_none() {
        let mut sink = open_sink()
            .map_err(|e| AppError::Audio(format!("No se pudo abrir el dispositivo de audio: {e}")))?;
        sink.log_on_drop(false);
        *device = Some(sink);
    }
    let sink = device.as_ref().expect("dispositivo inicializado arriba");

    let file = File::open(path).map_err(|e| AppError::Audio(format!("No se pudo abrir el archivo: {e}")))?;
    let decoder = rodio::Decoder::new(BufReader::new(file))
        .map_err(|e| AppError::Audio(format!("No se pudo decodificar el archivo: {e}")))?;

    let player = Player::connect_new(sink.mixer());
    player.set_volume(volume);
    player.append(decoder);
    Ok(player)
}

/// Abre la salida por defecto. rodio fija por defecto un buffer de 2048
/// muestras, que algunos dispositivos rechazan (p. ej. rango 14..=1024);
/// con `BufferSize::Default` el sistema elige uno válido. Si aun así falla,
/// se usa la búsqueda de alternativas propia de rodio.
fn open_sink() -> Result<MixerDeviceSink, DeviceSinkError> {
    DeviceSinkBuilder::from_default_device()
        .and_then(|b| b.with_buffer_size(BufferSize::Default).open_stream())
        .or_else(|_| DeviceSinkBuilder::open_default_sink())
}
