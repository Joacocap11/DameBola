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
