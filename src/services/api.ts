// Única puerta de entrada al backend. El frontend solo envía ids internos:
// nunca rutas ni nombres de archivo.
import { invoke } from "@tauri-apps/api/core";
import type {
  AlertContext,
  AlertStarted,
  AppInfo,
  ImportReport,
  PlaybackStatus,
  Settings,
  Song,
} from "../types";

export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),

  listSongs: () => invoke<Song[]>("list_songs"),
  /** Abre el selector nativo de archivos en el backend e importa lo elegido. */
  importSongs: () => invoke<ImportReport>("import_songs"),
  renameSong: (id: string, name: string) => invoke<Song>("rename_song", { id, name }),
  setSongEnabled: (id: string, enabled: boolean) =>
    invoke<Song>("set_song_enabled", { id, enabled }),
  deleteSong: (id: string) => invoke<void>("delete_song", { id }),

  playSong: (id: string) => invoke<void>("play_song", { id }),
  pausePlayback: () => invoke<void>("pause_playback"),
  resumePlayback: () => invoke<void>("resume_playback"),
  stopPlayback: () => invoke<void>("stop_playback"),
  getPlaybackStatus: () => invoke<PlaybackStatus>("get_playback_status"),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),

  triggerAlert: () => invoke<AlertStarted>("trigger_alert"),
  closeAlert: () => invoke<void>("close_alert"),
  getAlertContext: () => invoke<AlertContext | null>("get_alert_context"),
  isAlertActive: () => invoke<boolean>("is_alert_active"),
};

/** Los errores del backend llegan como string en español. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Ocurrió un error inesperado.";
}
