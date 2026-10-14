import { Home, Music2, Settings as SettingsIcon, type LucideIcon } from "lucide-react";

export type PageId = "home" | "songs" | "settings";

// Para sumar una sección (p. ej. "Dispositivos" en una fase futura) basta con
// agregar una entrada aquí y su página en App.tsx.
const NAV: { id: PageId; label: string; icon: LucideIcon }[] = [
  { id: "home", label: "Inicio", icon: Home },
  { id: "songs", label: "Canciones", icon: Music2 },
  { id: "settings", label: "Configuración", icon: SettingsIcon },
];

export function Sidebar({ page, onNavigate, version }: { page: PageId; onNavigate: (p: PageId) => void; version?: string }) {
  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-zinc-800 bg-zinc-950 p-4">
      <div className="mb-8 flex items-center gap-3 px-2 pt-2">
        <div className="grid size-10 place-items-center rounded-xl bg-red-600 text-xl shadow-lg shadow-red-900/40">🐔</div>
        <span className="text-xl font-extrabold tracking-tight">DameBola</span>
      </div>
      <nav className="flex flex-1 flex-col gap-1">
        {NAV.map(({ id, label, icon: Icon }) => {
          const active = id === page;
          return (
            <button
              key={id}
              onClick={() => onNavigate(id)}
              aria-current={active ? "page" : undefined}
              className={`flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm font-semibold transition-all duration-150 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-400 ${
                active ? "bg-red-600/15 text-red-400" : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-100"
              }`}
            >
              <Icon className="size-5" />
              {label}
            </button>
          );
        })}
      </nav>
      {version && <p className="px-2 text-xs text-zinc-600">v{version} · Fase 1</p>}
    </aside>
  );
}
