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
