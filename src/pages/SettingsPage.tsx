import { useEffect, useRef, useState, type ReactNode } from "react";
import { RotateCcw, Save } from "lucide-react";
import { Button, Card, PageHeader, Switch } from "../components/ui";
import { useToasts } from "../components/Toasts";
import { useAppState } from "../hooks/useAppState";
import { useNetworkState } from "../hooks/useNetworkState";
import { ShortcutSetting } from "../components/ShortcutSetting";
import { describeShortcut } from "../lib/shortcut";
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
  const { devices } = useNetworkState();
  const { notify } = useToasts();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);

  const lastSaved = useRef<Settings | null>(null);
  useEffect(() => {
    if (!settings) return;
    const previous = lastSaved.current;
    lastSaved.current = settings;
    // Si cambió algo guardado por otra vía (p. ej. el atajo), se aplica al borrador
    // sin perder lo que el usuario ya editó en otros campos.
    setDraft((current) => {
      if (!current || !previous) return settings;
      const next: Record<string, unknown> = { ...current };
      for (const key of Object.keys(settings) as (keyof Settings)[]) {
        if (settings[key] !== previous[key]) next[key] = settings[key];
      }
      return next as unknown as Settings;
    });
  }, [settings]);

  if (!draft || !settings) return <p className="text-zinc-400">Cargando configuración…</p>;

  const patch = (p: Partial<Settings>) => setDraft({ ...draft, ...p });
  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);
  const nameOk = draft.userName.trim().length > 0;
  const paired = devices.filter((d) => d.pairing === "paired");
  const missingTarget = draft.shortcutDeviceId !== null && !paired.some((d) => d.deviceId === draft.shortcutDeviceId);

  const shortcutLabel = describeShortcut(settings.shortcutKey);

  async function saveShortcut(shortcutKey: string): Promise<boolean> {
    if (!settings) return false;
    // Solo cambia el atajo: el resto de lo guardado queda intacto.
    const ok = await saveSettings({ ...settings, shortcutKey });
    if (ok) notify("success", `Atajo guardado: ${describeShortcut(shortcutKey)}.`);
    return ok;
  }

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
        <Row title="Atajo de teclado" hint={`Envía DAME BOLA con esta ventana en primer plano (${shortcutLabel}). No funciona con la app minimizada ni cerrada.`}>
          <Switch checked={draft.shortcutEnabled} onChange={(v) => patch({ shortcutEnabled: v })} label="Activar atajo de teclado" />
        </Row>

        <Row title="Combinación" hint="Teclas F1–F24, o Ctrl/Cmd + letra o número. Se guarda al instante; por defecto F12.">
          <ShortcutSetting current={settings.shortcutKey} onSave={saveShortcut} />
        </Row>

        <Row title={`Destinatario (${shortcutLabel})`} hint="Con un solo equipo emparejado se elige automáticamente. Si hay varios, seleccioná uno.">
          <div className="w-56 space-y-2">
            <select aria-label="Destinatario del atajo" value={draft.shortcutDeviceId ?? ""} onChange={(e) => patch({ shortcutDeviceId: e.target.value || null })} className="w-full rounded-xl border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm">
              <option value="">Único equipo emparejado</option>
              {missingTarget && <option value={draft.shortcutDeviceId!}>Equipo ya no emparejado</option>}
              {paired.map((d) => <option key={d.deviceId} value={d.deviceId}>{d.name}</option>)}
            </select>
            {(missingTarget || (draft.shortcutDeviceId === null && paired.length !== 1)) && <p className="text-xs text-amber-400">{missingTarget ? "Elegí un equipo emparejado." : paired.length === 0 ? "Primero emparejá un equipo en Dispositivos." : "Elegí un destinatario para habilitar el envío."}</p>}
          </div>
        </Row>

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
