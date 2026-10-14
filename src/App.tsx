import { useState } from "react";
import { Sidebar, type PageId } from "./components/Sidebar";
import { ToastProvider } from "./components/Toasts";
import { AppStateProvider, useAppState } from "./hooks/useAppState";
import { NetworkStateProvider } from "./hooks/useNetworkState";
import { useDameBolaShortcut } from "./hooks/useDameBolaShortcut";
import { DevicesPage } from "./pages/DevicesPage";
import { HomePage } from "./pages/HomePage";
import { SongsPage } from "./pages/SongsPage";
import { SettingsPage } from "./pages/SettingsPage";

function Shell() {
  useDameBolaShortcut();
  const [page, setPage] = useState<PageId>("home");
  const { info } = useAppState();

  return (
    <div className="flex h-full">
      <Sidebar page={page} onNavigate={setPage} version={info?.version} />
      <main className="flex-1 overflow-y-auto p-8">
        {page === "home" && <HomePage onGoToSongs={() => setPage("songs")} />}
        {page === "songs" && <SongsPage />}
        {page === "devices" && <DevicesPage />}
        {page === "settings" && <SettingsPage />}
      </main>
    </div>
  );
}

export function App() {
  return (
    <ToastProvider>
      <AppStateProvider>
        <NetworkStateProvider>
          <Shell />
        </NetworkStateProvider>
      </AppStateProvider>
    </ToastProvider>
  );
}
