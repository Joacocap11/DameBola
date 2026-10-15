import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DeviceView, NetStatus, NetworkSettings, PairingView, PlaybackStatus, SendResultEvent, Settings } from "../types";

export const onPlaybackStatus = (cb: (s: PlaybackStatus) => void): Promise<UnlistenFn> =>
  listen<PlaybackStatus>("playback://status", (e) => cb(e.payload));

export const onAlertState = (cb: (active: boolean) => void): Promise<UnlistenFn> =>
  listen<boolean>("alert://state", (e) => cb(e.payload));

export const onNetStatus = (cb: (s: NetStatus) => void): Promise<UnlistenFn> =>
  listen<NetStatus>("net://status", (e) => cb(e.payload));

export const onNetDevices = (cb: (d: DeviceView[]) => void): Promise<UnlistenFn> =>
  listen<DeviceView[]>("net://devices", (e) => cb(e.payload));

export const onNetPairing = (cb: (p: PairingView[]) => void): Promise<UnlistenFn> =>
  listen<PairingView[]>("net://pairing", (e) => cb(e.payload));

export const onSettingsChanged = (cb: (s: Settings) => void): Promise<UnlistenFn> =>
  listen<Settings>("settings://changed", (e) => cb(e.payload));

export const onNetSettings = (cb: (s: NetworkSettings) => void): Promise<UnlistenFn> =>
  listen<NetworkSettings>("net://settings", (e) => cb(e.payload));

export const onSendResult = (cb: (r: SendResultEvent) => void): Promise<UnlistenFn> =>
  listen<SendResultEvent>("send://result", (e) => cb(e.payload));
