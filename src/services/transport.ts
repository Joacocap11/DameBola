// Contratos para fases futuras (comunicación entre computadoras).
// FASE 1: solo tipos. No hay implementación ni se usan en tiempo de ejecución.

export interface PeerInfo {
  deviceId: string;
  userName: string;
  platform: "macos" | "windows";
}

/** Mensajes que viajarán entre pares (p. ej. por WebSocket). */
export type PeerMessage =
  | { type: "alert"; from: PeerInfo; sentAt: number }
  | { type: "song-manifest"; songs: { sha256: string; name: string; sizeBytes: number }[] }
  | { type: "song-request"; sha256: string };

export interface AlertTransport {
  connect(peer: PeerInfo): Promise<void>;
  disconnect(): Promise<void>;
  sendAlert(): Promise<void>;
  onMessage(handler: (message: PeerMessage) => void): () => void;
}
