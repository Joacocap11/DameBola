// Funciones puras de la sección Dispositivos (validación, tiempos relativos, silencio).

export const MIN_PORT = 1024;
export const MAX_PORT = 65535;
export const MIN_COOLDOWN = 5;
export const MAX_COOLDOWN = 3600;

export function isValidIPv4(value: string): boolean {
  const parts = value.split(".");
  if (parts.length !== 4) return false;
  return parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255);
}

/** Validación básica de literales IPv6 (sin zona ni corchetes). */
export function isValidIPv6(value: string): boolean {
  if (!value.includes(":") || !/^[0-9a-fA-F:.]+$/.test(value)) return false;
  const doubleCount = value.split("::").length - 1;
  if (doubleCount > 1 || value.includes(":::")) return false;

  let groups = value.split(":");
  let maxGroups = 8;
  const last = groups[groups.length - 1] ?? "";
  if (last.includes(".")) {
    if (!isValidIPv4(last)) return false;
    groups = groups.slice(0, -1);
    maxGroups = 6;
    // Un "::" final dejó un grupo vacío antes del IPv4: se tolera.
  }
  const hasCompression = doubleCount === 1;
  const nonEmpty = groups.filter((g) => g !== "");
  if (!nonEmpty.every((g) => /^[0-9a-fA-F]{1,4}$/.test(g))) return false;
  if (hasCompression) return nonEmpty.length < maxGroups;
  // Sin compresión no puede haber grupos vacíos.
  return groups.length === maxGroups && nonEmpty.length === maxGroups;
}

/** Devuelve un mensaje de error en español, o `null` si el host es válido. */
export function validateHost(raw: string): string | null {
  const host = raw.trim();
  if (host === "") return "Ingresá la dirección IP del otro equipo.";
  if (isValidIPv4(host) || isValidIPv6(host)) return null;
  return "Ingresá una dirección IP válida (por ejemplo 192.168.1.20). No se aceptan nombres de equipo.";
}

/** Devuelve un mensaje de error en español, o `null` si el puerto es válido. */
export function validatePort(raw: string | number): string | null {
  const text = String(raw).trim();
  if (!/^\d+$/.test(text)) return `El puerto debe ser un número entero entre ${MIN_PORT} y ${MAX_PORT}.`;
  const n = Number(text);
  if (n < MIN_PORT || n > MAX_PORT) return `El puerto debe estar entre ${MIN_PORT} y ${MAX_PORT}.`;
  return null;
}

export function validateCooldown(raw: string | number): string | null {
  const text = String(raw).trim();
  if (!/^\d+$/.test(text)) return `La espera debe ser un número entero de segundos entre ${MIN_COOLDOWN} y ${MAX_COOLDOWN}.`;
  const n = Number(text);
  if (n < MIN_COOLDOWN || n > MAX_COOLDOWN) return `La espera debe estar entre ${MIN_COOLDOWN} y ${MAX_COOLDOWN} segundos.`;
  return null;
}

/** "hace 5 s", "hace 3 min", "hace 2 h", "hace 4 d". */
export function formatRelativeTime(thenMs: number, nowMs: number): string {
  const secs = Math.floor((nowMs - thenMs) / 1000);
  if (secs < 1) return "ahora mismo";
  if (secs < 60) return `hace ${secs} s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `hace ${mins} min`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `hace ${hours} h`;
  return `hace ${Math.floor(hours / 24)} d`;
}

export type MuteOption = "15m" | "1h" | "tomorrow";

/** Instante (epoch ms) hasta el que se silencia. "Mañana" = 08:00 locales del día siguiente. */
export function muteUntil(option: MuteOption, nowMs: number): number {
  if (option === "15m") return nowMs + 15 * 60_000;
  if (option === "1h") return nowMs + 60 * 60_000;
  const d = new Date(nowMs);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1, 8, 0, 0, 0).getTime();
}

export const isMuted = (mutedUntilMs: number | null, nowMs: number): boolean =>
  mutedUntilMs !== null && mutedUntilMs > nowMs;

export function osLabel(os: string): string {
  switch (os) {
    case "macos":
      return "macOS";
    case "windows":
      return "Windows";
    case "linux":
      return "Linux";
    default:
      return "Otro sistema";
  }
}
