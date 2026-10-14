import type { AlertSendResult, DeviceView, Settings } from "../types";

export function shortcutRecipient(devices: DeviceView[], targetId: string | null): DeviceView {
  const paired = devices.filter((d) => d.pairing === "paired");
  if (targetId !== null) {
    const target = paired.find((d) => d.deviceId === targetId);
    if (!target) throw new Error("El destinatario de F12 ya no está emparejado. Elegí otro en Configuración.");
    return target;
  }
  if (paired.length === 0) throw new Error("Emparejá un dispositivo en Dispositivos antes de usar F12.");
  if (paired.length !== 1) throw new Error("Elegí el destinatario de F12 en Configuración: hay varios equipos emparejados.");
  return paired[0]!;
}

type KeyInput = Pick<KeyboardEvent, "key" | "repeat" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "isComposing" | "defaultPrevented" | "preventDefault">;
type Report = (kind: "success" | "warning" | "error", message: string) => void;

/** Main-window handler. One request at a time; holding the key never queues alerts. */
export function createShortcutHandler(options: {
  isFocused: () => boolean;
  getState: () => { settings: Settings | null; devices: DeviceView[] };
  send: (deviceId: string) => Promise<AlertSendResult>;
  report: Report;
}) {
  let pending = false;
  return async (event: KeyInput): Promise<void> => {
    const { settings, devices } = options.getState();
    if (!settings?.shortcutEnabled || !options.isFocused() || event.key !== "F12" ||
      event.ctrlKey || event.altKey || event.shiftKey || event.metaKey || event.isComposing || event.defaultPrevented) return;
    event.preventDefault();
    if (event.repeat || pending) return;
    pending = true;
    try {
      const recipient = shortcutRecipient(devices, settings.shortcutDeviceId);
      if (recipient.connection !== "connected") throw new Error(`${recipient.name} no está conectado. No se guardó ninguna alerta.`);
      const result = await options.send(recipient.deviceId);
      options.report(result.status === "shown" ? "success" : result.status === "rejected" ? "warning" : "error", result.message);
    } catch (error) {
      options.report("error", error instanceof Error ? error.message : typeof error === "string" ? error : "No se pudo enviar DAME BOLA.");
    } finally {
      pending = false;
    }
  };
}
