import { useCallback, useEffect, useRef, useState } from "react";
import { Music2 } from "lucide-react";
import { ChickenDance } from "../components/ChickenDance";
import { api } from "../services/api";
import type { AlertContext } from "../types";

/**
 * Contenido de cada ventana de alerta (una por monitor). No reproduce audio:
 * el backend lo reproduce una sola vez. Cerrar desde cualquier ventana
 * termina la alerta completa.
 */
export function AlertWindow() {
  const [ctx, setCtx] = useState<AlertContext | null>(null);
  const [now, setNow] = useState(Date.now());
  const closing = useRef(false);

  const close = useCallback(() => {
    if (closing.current) return;
    closing.current = true;
    void api.closeAlert();
  }, []);

  useEffect(() => {
    let cancelled = false;
    api.getAlertContext().then((c) => {
      if (cancelled) return;
      if (c) setCtx(c);
      else close(); // la alerta ya terminó
    });
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => {
      cancelled = true;
      window.removeEventListener("keydown", onKey);
    };
  }, [close]);

  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(t);
  }, []);

  const animated = ctx?.animationsEnabled ?? true;
  const remaining = ctx ? Math.max(0, Math.ceil((ctx.endsAtMs - now) / 1000)) : 0;

  return (
    <main className={`flex h-full w-full cursor-default flex-col items-center justify-center gap-6 overflow-hidden bg-red-600 text-white ${animated ? "animate-alert-flash" : ""}`}>
      <ChickenDance animated={animated} className="h-[34vh] max-h-80 min-h-40 aspect-square" />

      <h1 className={`px-6 text-center font-black uppercase leading-none tracking-tight drop-shadow-[0_6px_0_rgba(0,0,0,0.35)] ${animated ? "animate-title-shake" : ""}`} style={{ fontSize: "clamp(3rem, 12vw, 11rem)" }}>
        ¡¡¡Dame bola!!!
      </h1>

      {ctx && (
        <p className="text-xl font-semibold text-red-100">
          {ctx.senderName} necesita tu atención
          {ctx.songName && (
            <span className="ml-3 inline-flex items-center gap-1 text-red-200">
              <Music2 className="size-5" /> {ctx.songName}
            </span>
          )}
        </p>
      )}

      <button
        autoFocus
        onClick={close}
        className="rounded-2xl bg-white px-10 py-4 text-2xl font-black uppercase tracking-wide text-red-700 shadow-2xl transition-transform hover:scale-105 focus:outline-none focus-visible:ring-4 focus-visible:ring-red-900 active:scale-95"
      >
        Ya te doy bola
      </button>

      <p className="text-sm text-red-100/80">
        Esc para cerrar{ctx ? ` · se cierra sola en ${remaining} s` : ""}
      </p>
    </main>
  );
}
