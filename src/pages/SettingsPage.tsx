import { useEffect, useRef, useState, type ReactNode } from "react";
import { LogOut, RotateCcw, Save } from "lucide-react";
import { Button, Card, PageHeader, Switch } from "../components/ui";
import { useToasts } from "../components/Toasts";
import { useAppState } from "../hooks/useAppState";
import { useNetworkState } from "../hooks/useNetworkState";
import { ShortcutSetting } from "../components/ShortcutSetting";
import { describeShortcut, globalShortcutIssue } from "../lib/shortcut";
import { api, errorMessage } from "../services/api";
import type { BackgroundStatus, Settings } from "../types";

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

  const [background, setBackground] = useState<BackgroundStatus | null>(null);
  useEffect(() => {
    api.getBackgroundStatus().then(setBackground).catch(() => undefined);
  }, [settings]);

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
  const globalIssue = draft.shortcutScope === "global" ? globalShortcutIssue(settings.shortcutKey) : null;
  const globalNotRegistered = settings.shortcutScope === "global" && settings.shortcutEnabled && background !== null && background.globalShortcut === null;

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
        <Row title="Atajo de teclado" hint={draft.shortcutScope === "global" ? `Envía DAME BOLA con ${shortcutLabel} desde cualquier aplicación, incluso con esta ventana oculta. No funciona si salís completamente.` : `Envía DAME BOLA con esta ventana en primer plano (${shortcutLabel}).`}>
          <Switch checked={draft.shortcutEnabled} onChange={(v) => patch({ shortcutEnabled: v })} label="Activar atajo de teclado" />
        </Row>

        <Row title="Combinación" hint="Teclas F1–F24, o Ctrl/Cmd + letra o número. Se guarda al instante; por defecto F12.">
          <ShortcutSetting current={settings.shortcutKey} onSave={saveShortcut} />
        </Row>

        <Row title="Alcance del atajo" hint="Por defecto solo funciona dentro de DameBola. En modo global el sistema operativo registra únicamente esta combinación; DameBola no lee ninguna otra tecla.">
          <div className="w-72 space-y-2">
            <select aria-label="Alcance del atajo" value={draft.shortcutScope} onChange={(e) => patch({ shortcutScope: e.target.value as Settings["shortcutScope"] })} className="w-full rounded-xl border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm">
              <option value="local">Solo dentro de DameBola</option>
              <option value="global">Global (incluso con la ventana oculta)</option>
            </select>
            {globalIssue && <p role={globalIssue.level === "error" ? "alert" : undefined} className={`text-xs ${globalIssue.level === "error" ? "text-red-400" : "text-amber-400"}`}>{globalIssue.text}</p>}
            {draft.shortcutScope === "global" && !globalIssue && <p className="text-xs text-zinc-400">Si otro programa o el sistema ya usa la combinación, no se activa y se mantiene la configuración anterior.</p>}
            {globalNotRegistered && <p role="alert" className="text-xs text-red-400">{background?.globalError ?? "El atajo global no está registrado."}</p>}
            {settings.shortcutScope === "global" && settings.shortcutEnabled && background?.globalShortcut && <p className="text-xs text-emerald-400">Registrado en el sistema: {describeShortcut(background.globalShortcut)}</p>}
          </div>
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

      </Card>

      <h2 className="mb-3 mt-8 text-lg font-bold">Segundo plano</h2>
      <Card>
        <Row title="Mantener DameBola ejecutándose al cerrar la ventana" hint={draft.showTrayIcon ? "Cerrar la ventana solo la oculta: la red, las alertas y el atajo siguen activos. Se abre y se sale desde la bandeja. Desactivado, cerrar la ventana cierra DameBola." : "Requiere el icono de la bandeja."}>
          <Switch checked={draft.keepRunningOnClose && draft.showTrayIcon} disabled={!draft.showTrayIcon} onChange={(v) => patch({ keepRunningOnClose: v })} label="Mantener DameBola ejecutándose al cerrar la ventana" />
        </Row>

        <Row title="Mostrar icono en la bandeja" hint="Windows: bandeja del sistema · macOS: barra de menús. Sin el icono, cerrar la ventana cierra DameBola.">
          <Switch checked={draft.showTrayIcon} onChange={(v) => patch(v ? { showTrayIcon: true } : { showTrayIcon: false, keepRunningOnClose: false })} label="Mostrar icono en la bandeja" />
        </Row>

        <Row title="Iniciar DameBola con el sistema" hint="Abre DameBola al iniciar sesión, sin permisos de administrador (macOS: Elementos de inicio · Windows: registro del usuario). Solo si lo activás.">
          <Switch checked={draft.launchAtLogin} onChange={(v) => patch({ launchAtLogin: v })} label="Iniciar con el sistema" />
        </Row>

        <Row title="Iniciar minimizado" hint={draft.showTrayIcon ? "Al iniciar con el sistema no se muestra la ventana; DameBola queda en la bandeja." : "Requiere el icono de la bandeja."}>
          <Switch checked={draft.startMinimized && draft.showTrayIcon} disabled={!draft.showTrayIcon} onChange={(v) => patch({ startMinimized: v })} label="Iniciar minimizado" />
        </Row>

        <Row title="Salir completamente" hint="Detiene las conexiones, el audio y el atajo global, y cierra el proceso.">
          <Button icon={<LogOut className="size-4" />} onClick={() => void api.quitApp().catch((e) => notify("error", errorMessage(e)))}>Salir de DameBola</Button>
        </Row>
      </Card>
    </div>
  );
}
