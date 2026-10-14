// Única puerta de entrada al backend. El frontend solo envía ids internos:
// nunca rutas ni nombres de archivo.
import { invoke } from "@tauri-apps/api/core";
import type {
  AlertContext,
  AlertStarted,
  AlertSendResult,
  AppInfo,
  DeviceView,
  ImportReport,
  NetStatus,
  NetworkSettings,
  PairingView,
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

  netGetStatus: () => invoke<NetStatus>("net_get_status"),
  netListDevices: () => invoke<DeviceView[]>("net_list_devices"),
  netGetPairing: () => invoke<PairingView[]>("net_get_pairing"),
  netGetSettings: () => invoke<NetworkSettings>("net_get_settings"),
  netSaveSettings: (settings: NetworkSettings) => invoke<NetworkSettings>("net_save_settings", { settings }),
  netPair: (deviceId: string) => invoke<void>("net_pair", { deviceId }),
  netPairingDecision: (deviceId: string, accept: boolean) =>
    invoke<void>("net_pairing_decision", { deviceId, accept }),
  netConnect: (deviceId: string) => invoke<void>("net_connect", { deviceId }),
  netDisconnect: (deviceId: string) => invoke<void>("net_disconnect", { deviceId }),
  netForget: (deviceId: string) => invoke<void>("net_forget", { deviceId }),
  netAddManual: (host: string, port: number) => invoke<DeviceView>("net_add_manual", { host, port }),
  netRetryIdentity: () => invoke<NetStatus>("net_retry_identity"),
  /** Destructivo: crea una identidad nueva. El backend exige `confirm === "NUEVA IDENTIDAD"`. */
  netResetIdentity: (confirm: string) => invoke<NetStatus>("net_reset_identity", { confirm }),
  /** `null` o texto vacío quita el alias (máx. 40 caracteres). */
  netRenameDevice: (deviceId: string, alias: string | null) =>
    invoke<void>("net_rename_device", { deviceId, alias }),
  /** Puede tardar hasta ~15 s. */
  netSendAlert: (deviceId: string) => invoke<AlertSendResult>("net_send_alert", { deviceId }),
};

/** Los errores del backend llegan como string en español. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Ocurrió un error inesperado.";
}
