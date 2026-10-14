import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { useToasts } from "../components/Toasts";
import { api, errorMessage } from "../services/api";
import { onNetDevices, onNetPairing, onNetStatus } from "../services/events";
import type { DeviceView, NetStatus, NetworkSettings, PairingView } from "../types";

interface NetworkState {
  /** `false` si los comandos de red fallaron al iniciar (red deshabilitada o no disponible). */
  available: boolean;
  /** `true` hasta que termina la carga inicial. */
  loading: boolean;
  status: NetStatus | null;
  devices: DeviceView[];
  pairings: PairingView[];
  settings: NetworkSettings | null;
  saveSettings: (s: NetworkSettings) => Promise<boolean>;
}

const Ctx = createContext<NetworkState | null>(null);

export function useNetworkState(): NetworkState {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useNetworkState debe usarse dentro de <NetworkStateProvider>");
  return ctx;
}

export function NetworkStateProvider({ children }: { children: ReactNode }) {
  const { notify } = useToasts();
  const [available, setAvailable] = useState(true);
  const [loading, setLoading] = useState(true);
  const [status, setStatus] = useState<NetStatus | null>(null);
  const [devices, setDevices] = useState<DeviceView[]>([]);
  const [pairings, setPairings] = useState<PairingView[]>([]);
  const [settings, setSettings] = useState<NetworkSettings | null>(null);

  useEffect(() => {
    let cancelled = false;
    // Primero se suscribe, luego se carga: así no se pierde ningún evento intermedio.
    const unlisten = [onNetStatus(setStatus), onNetDevices(setDevices), onNetPairing(setPairings)];

    Promise.all([api.netGetStatus(), api.netListDevices(), api.netGetPairing(), api.netGetSettings()])
      .then(([st, dev, pair, set]) => {
        if (cancelled) return;
        setStatus(st);
        setDevices(dev);
        setPairings(pair);
        setSettings(set);
        setAvailable(true);
      })
      .catch(() => {
        if (!cancelled) setAvailable(false);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
      unlisten.forEach((p) => void p.then((fn) => fn()));
    };
  }, []);

  const saveSettings = useCallback(
    async (next: NetworkSettings) => {
      try {
        setSettings(await api.netSaveSettings(next));
        return true;
      } catch (e) {
        notify("error", errorMessage(e));
        return false;
      }
    },
    [notify],
  );

  const value = useMemo(
    () => ({ available, loading, status, devices, pairings, settings, saveSettings }),
    [available, loading, status, devices, pairings, settings, saveSettings],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
