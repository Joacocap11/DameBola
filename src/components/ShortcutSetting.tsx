import { useEffect, useRef, useState } from "react";
import { Keyboard, RotateCcw } from "lucide-react";
import { Button } from "./ui";
import { beginShortcutCapture, createShortcutCapture, DEFAULT_SHORTCUT, describeShortcut } from "../lib/shortcut";

type Mode = "idle" | "capturing" | "candidate";

function Keys({ value }: { value: string }) {
  return (
    <kbd className="inline-block rounded-lg border border-zinc-600 bg-zinc-950 px-3 py-1.5 font-mono text-sm font-semibold">
      {describeShortcut(value)}
    </kbd>
  );
}

/** Captura de atajo: "Cambiar" → presionar combinación → "Guardar" / "Cancelar". Nunca envía alertas. */
export function ShortcutSetting({ current, onSave }: { current: string; onSave: (value: string) => Promise<boolean> }) {
  const [mode, setMode] = useState<Mode>("idle");
  const [preview, setPreview] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [candidate, setCandidate] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const modeRef = useRef(mode);
  modeRef.current = mode;

  useEffect(() => {
    if (mode !== "capturing") return;
    const end = beginShortcutCapture(); // suspende el envío mientras se captura
    const handle = createShortcutCapture({
      onPreview: (mods) => { setPreview(mods); setError(null); },
      onInvalid: (reason) => { setError(reason); setPreview([]); },
      onCandidate: (value) => { setCandidate(value); setError(null); setMode("candidate"); },
      onCancel: () => { setMode("idle"); setPreview([]); setError(null); },
    });
    // Fase de captura en window: corre antes que cualquier otro manejador y lo corta.
    const onKey = (event: KeyboardEvent) => {
      handle(event);
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      end();
    };
  }, [mode]);

  const cancel = () => { setMode("idle"); setCandidate(null); setPreview([]); setError(null); };

  async function save(value: string) {
    setSaving(true);
    const ok = await onSave(value);
    setSaving(false);
    if (ok) cancel();
  }

  return (
    <div className="w-72 space-y-3">
      {mode === "idle" && (
        <div className="flex items-center justify-end gap-2">
          <Keys value={current} />
        </div>
      )}

      {mode === "capturing" && (
        <div role="status" className="rounded-xl border border-red-700 bg-red-950/40 p-3 text-sm">
          <p className="font-semibold">Presioná la tecla o combinación…</p>
          <p className="mt-1 font-mono text-zinc-300">{preview.length > 0 ? `${describeShortcut(preview.join("+"))} + …` : "Esperando…"}</p>
          {error && <p className="mt-2 text-xs text-amber-400">{error}</p>}
          <p className="mt-2 text-xs text-zinc-400">Esc cancela. Mientras captura, no se envía ninguna alerta.</p>
        </div>
      )}

      {mode === "candidate" && candidate && (
        <div className="space-y-2 rounded-xl border border-zinc-700 bg-zinc-950 p-3 text-sm">
          <p className="text-zinc-400">Nuevo atajo</p>
          <Keys value={candidate} />
          {candidate === current && <p className="text-xs text-amber-400">Es el mismo atajo que ya tenés.</p>}
        </div>
      )}

      <div className="flex justify-end gap-2">
        {mode === "idle" && (
          <>
            <Button icon={<Keyboard className="size-4" />} onClick={() => { setError(null); setPreview([]); setMode("capturing"); }}>Cambiar atajo</Button>
            <Button variant="ghost" icon={<RotateCcw className="size-4" />} disabled={current === DEFAULT_SHORTCUT || saving} loading={saving} onClick={() => void save(DEFAULT_SHORTCUT)}>
              Restablecer {describeShortcut(DEFAULT_SHORTCUT)}
            </Button>
          </>
        )}
        {mode === "capturing" && <Button onClick={cancel}>Cancelar</Button>}
        {mode === "candidate" && candidate && (
          <>
            <Button onClick={cancel} disabled={saving}>Cancelar</Button>
            <Button variant="primary" loading={saving} disabled={candidate === current} onClick={() => void save(candidate)}>Guardar</Button>
          </>
        )}
      </div>
    </div>
  );
}
