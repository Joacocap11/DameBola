// Funciones puras de dispositivos emparejados (etiquetas de conexión, alias, reseteo de identidad).
import type { DeviceView } from "../types";

export const RESET_CONFIRMATION = "NUEVA IDENTIDAD";
export const MAX_ALIAS_LENGTH = 40;
export const IDENTITY_DOCS_PATH = "docs/PAIRING_SECURITY.md#recuperación-de-la-identidad";

/** Texto del estado de conexión de un dispositivo emparejado. */
export function connectionLabel(device: Pick<DeviceView, "connection" | "autoConnect">): string {
  if (device.connection === "connected") return "Conectado";
  if (device.connection === "connecting") return "Conectando…";
  return device.autoConnect ? "Desconectado" : "Desconectado por vos";
}

/** Devuelve un mensaje de error en español, o `null` si el alias es válido (vacío = quitar alias). */
export function validateAlias(text: string): string | null {
  const alias = text.trim();
  if (Array.from(alias).length > MAX_ALIAS_LENGTH) {
    return `El nombre no puede superar los ${MAX_ALIAS_LENGTH} caracteres.`;
  }
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(alias)) return "El nombre no puede tener caracteres de control.";
  return null;
}

/** `true` si el texto escrito coincide exactamente con la frase de confirmación. */
export const isResetConfirmed = (text: string): boolean => text === RESET_CONFIRMATION;

/** "Anunciado como X" solo si hay alias y difiere del nombre anunciado. */
export function announcedAsLabel(device: Pick<DeviceView, "alias" | "announcedName">): string | null {
  if (!device.alias || !device.announcedName || device.alias === device.announcedName) return null;
  return `Anunciado como ${device.announcedName}`;
}
