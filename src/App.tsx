import { useState } from "react";
import { Sidebar, type PageId } from "./components/Sidebar";
import { ToastProvider } from "./components/Toasts";
import { AppStateProvider, useAppState } from "./hooks/useAppState";
import { HomePage } from "./pages/HomePage";
import { SongsPage } from "./pages/SongsPage";
import { SettingsPage } from "./pages/SettingsPage";

function Shell() {
  const [page, setPage] = useState<PageId>("home");
  const { info } = useAppState();

  return (
    <div className="flex h-full">
      <Sidebar page={page} onNavigate={setPage} version={info?.version} />
      <main className="flex-1 overflow-y-auto p-8">
        {page === "home" && <HomePage onGoToSongs={() => setPage("songs")} />}
        {page === "songs" && <SongsPage />}
        {page === "settings" && <SettingsPage />}
      </main>
    </div>
  );
}

export function App() {
  return (
    <ToastProvider>
      <AppStateProvider>
        <Shell />
      </AppStateProvider>
    </ToastProvider>
  );
}
