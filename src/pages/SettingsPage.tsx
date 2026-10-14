import { useEffect, useState, type ReactNode } from "react";
import { RotateCcw, Save } from "lucide-react";
import { Button, Card, PageHeader, Switch } from "../components/ui";
import { useToasts } from "../components/Toasts";
import { useAppState } from "../hooks/useAppState";
import type { Settings } from "../types";

const DEFAULT_DURATION = 15;
const MIN_DURATION = 3;
const MAX_DURATION = 120;

function Row({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-zinc-800 px-6 py-5 last:border-0">
      <div>
        <p className="font-semibold">{title}</p>
        {hint && <p className="mt-0.5 text-sm text-zinc-400">{hint}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function SettingsPage() {
  const { settings, saveSettings } = useAppState();
  const { notify } = useToasts();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (settings) setDraft(settings);
  }, [settings]);

  if (!draft || !settings) return <p className="text-zinc-400">Cargando configuración…</p>;

  const patch = (p: Partial<Settings>) => setDraft({ ...draft, ...p });
  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);
  const nameOk = draft.userName.trim().length > 0;

  async function save() {
    if (!draft) return;
    setSaving(true);
    const ok = await saveSettings({ ...draft, userName: draft.userName.trim() });
    setSaving(false);
    if (ok) notify("success", "Configuración guardada.");
  }

  return (
    <div className="mx-auto max-w-3xl animate-fade-in">
      <PageHeader
        title="Configuración"
        subtitle="Los cambios se aplican al guardar y se conservan al reiniciar."
        actions={
          <Button variant="primary" icon={<Save className="size-4" />} loading={saving} disabled={!dirty || !nameOk} onClick={() => void save()}>
            Guardar cambios
          </Button>
        }
      />

      <Card>
        <Row title="Tu nombre" hint="Se muestra en los otros equipos y aparece como remitente de tus alertas.">
          <input value={draft.userName} maxLength={40} onChange={(e) => patch({ userName: e.target.value })} aria-invalid={!nameOk} className={`w-56 rounded-xl border bg-zinc-950 px-3 py-2 text-sm outline-none focus:border-red-500 ${nameOk ? "border-zinc-700" : "border-red-600"}`} />
        </Row>

        <Row title="Volumen" hint="También se usa al escuchar canciones desde el listado.">
          <div className="flex items-center gap-3">
            <input type="range" min={0} max={100} value={draft.alertVolume} aria-label="Volumen" onChange={(e) => patch({ alertVolume: Number(e.target.value) })} className="w-44 accent-red-600" />
            <span className="w-12 text-right text-sm tabular-nums">{draft.alertVolume}%</span>
          </div>
        </Row>

        <Row title="Duración de la alerta" hint={`Entre ${MIN_DURATION} y ${MAX_DURATION} segundos (por defecto ${DEFAULT_DURATION}).`}>
          <div className="flex items-center gap-3">
            <input type="range" min={MIN_DURATION} max={MAX_DURATION} value={draft.alertDurationSecs} aria-label="Duración en segundos" onChange={(e) => patch({ alertDurationSecs: Number(e.target.value) })} className="w-44 accent-red-600" />
            <span className="w-12 text-right text-sm tabular-nums">{draft.alertDurationSecs} s</span>
            <Button variant="ghost" className="size-9 !p-0" title="Restablecer a 15 s" aria-label="Restablecer duración" onClick={() => patch({ alertDurationSecs: DEFAULT_DURATION })}>
              <RotateCcw className="size-4" />
            </Button>
          </div>
        </Row>

        <Row title="Sonido en las alertas" hint="Si se desactiva, la alerta se muestra sin música.">
          <Switch checked={draft.soundEnabled} onChange={(v) => patch({ soundEnabled: v })} label="Sonido en las alertas" />
        </Row>

        <Row title="Animaciones" hint="Pollo bailando y fondo parpadeante en la alerta.">
          <Switch checked={draft.animationsEnabled} onChange={(v) => patch({ animationsEnabled: v })} label="Animaciones" />
        </Row>

        <Row title="Iniciar con el sistema" hint="Abre DameBola al iniciar sesión (macOS: Elementos de inicio · Windows: registro del usuario).">
          <Switch checked={draft.launchAtLogin} onChange={(v) => patch({ launchAtLogin: v })} label="Iniciar con el sistema" />
        </Row>
      </Card>
    </div>
  );
}
