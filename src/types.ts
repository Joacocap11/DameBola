// Tipos que reflejan exactamente los DTO serializados por el backend Rust (camelCase).

export interface Song {
  id: string;
  name: string;
  originalFilename: string;
  format: "mp3" | "wav" | "ogg";
  sizeBytes: number;
  sha256: string;
  enabled: boolean;
  /** Milisegundos desde epoch Unix. */
  createdAt: number;
  updatedAt: number;
  fileMissing: boolean;
}

export interface ImportIssue {
  fileName: string;
  reason: string;
}

export interface ImportReport {
  imported: Song[];
  duplicates: ImportIssue[];
  failed: ImportIssue[];
  cancelled: boolean;
}

export interface Settings {
  userName: string;
  /** 0..=100 */
  alertVolume: number;
  alertDurationSecs: number;
  soundEnabled: boolean;
  animationsEnabled: boolean;
  launchAtLogin: boolean;
  shortcutEnabled: boolean;
  /** Combinación canónica, p. ej. "F12" o "Ctrl+Shift+B". */
  shortcutKey: string;
  /** null: use the only paired device; never choose among several. */
  shortcutDeviceId: string | null;
  /** "local": solo con la ventana enfocada (por defecto). "global": registrado en el sistema operativo. */
  shortcutScope: ShortcutScope;
  /** Cerrar la ventana la oculta y deja DameBola en la bandeja. */
  keepRunningOnClose: boolean;
  /** Al iniciar con el sistema, no mostrar la ventana principal. */
  startMinimized: boolean;
  showTrayIcon: boolean;
  /** Lo escribe solo el backend (aviso del primer cierre). */
  backgroundNoticeShown: boolean;
}

export type ShortcutScope = "local" | "global";

export interface BackgroundStatus {
  /** Combinación global registrada ahora mismo. */
  globalShortcut: string | null;
  /** Motivo por el que no se pudo registrar (ocupada, reservada…). */
  globalError: string | null;
  trayActive: boolean;
  os: NetOs;
}

/** Resultado de un envío iniciado desde la bandeja o el atajo global (sin ventana que lo muestre). */
export interface SendResultEvent extends AlertSendResult {
  origin: "tray" | "global";
}

export type PlaybackState = "stopped" | "playing" | "paused";

export interface PlaybackStatus {
  state: PlaybackState;
  songId: string | null;
  origin: "preview" | "alert" | null;
}

export interface AppInfo {
  version: string;
  connectionMode: "local";
  deviceId: string;
}

export interface AlertContext {
  senderName: string;
  durationSecs: number;
  animationsEnabled: boolean;
  songName: string | null;
  endsAtMs: number;
}

export interface AlertStarted {
  monitors: number;
  songName: string | null;
  audioError: string | null;
}

// ---- Fase 2: red local (P2P) ----

export type NetOs = "macos" | "windows" | "linux" | "other";

export interface NetIdentity {
  state: "ok" | "error";
  code: "key_missing" | "key_mismatch" | "store_unavailable" | "cert_missing" | "cert_corrupt" | "internal" | (string & {}) | null;
  message: string | null;
  /** `true` si se puede crear una identidad nueva. */
  canReset: boolean;
}

export interface NetStatus {
  enabled: boolean;
  state: "stopped" | "starting" | "running" | "error";
  error: string | null;
  deviceId: string;
  deviceName: string;
  os: NetOs;
  /** Huella pública del certificado, ya agrupada para mostrar. */
  fingerprint: string;
  port: number;
  /** IPs locales de la LAN en las que se escucha. */
  addresses: string[];
  mdns: "active" | "disabled" | "error";
  mdnsError: string | null;
  keyStorage: "keychain" | "file";
  identity: NetIdentity;
}

export interface DeviceView {
  deviceId: string;
  name: string;
  os: NetOs;
  connection: "connected" | "connecting" | "disconnected";
  /** `pending` = hay una sesión de emparejamiento en curso. */
  pairing: "paired" | "unpaired" | "pending";
  /** Visible ahora por mDNS o por sondeo manual. */
  discovered: boolean;
  addresses: string[];
  port: number | null;
  /** Milisegundos desde epoch. */
  lastSeenMs: number | null;
  /** Último nombre anunciado por el dispositivo (autenticado). */
  announcedName: string | null;
  /** Nombre personalizado local. `name` ya es el nombre a mostrar (alias o anunciado). */
  alias: string | null;
  /** `false` = el usuario lo desconectó a propósito; no reconecta solo hasta pulsar Conectar. */
  autoConnect: boolean;
  lastError: string | null;
}

export interface PairingView {
  deviceId: string;
  name: string;
  os: NetOs;
  /** 8 dígitos ya agrupados: "1234 5678". */
  code: string;
  role: "initiator" | "responder";
  localDecision: "pending" | "accepted";
  peerDecision: "pending" | "accepted";
  state: "active" | "completed" | "rejected" | "expired";
  message: string | null;
}

export interface NetworkSettings {
  enabled: boolean;
  /** 1024..=65535 */
  port: number;
  remoteAlertsEnabled: boolean;
  /** Milisegundos desde epoch; `null` = sin silencio. */
  mutedUntilMs: number | null;
  /** 5..=3600 */
  alertCooldownSecs: number;
}

export interface AlertSendResult {
  status: "shown" | "rejected" | "offline" | "timeout" | "error";
  /** El receptor aceptó antes de mostrar. */
  accepted: boolean;
  reason: string | null;
  /** Texto en español listo para un toast. */
  message: string;
}
