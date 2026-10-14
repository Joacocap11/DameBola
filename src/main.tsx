import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./index.css";
import { App } from "./App";
import { AlertWindow } from "./alert/AlertWindow";

// Las ventanas de alerta (etiqueta "alert-N") cargan este mismo bundle
// pero renderizan únicamente la vista de alerta.
const ALERT_LABEL_PREFIX = "alert-";
const isAlertWindow = getCurrentWindow().label.startsWith(ALERT_LABEL_PREFIX);

const root = document.getElementById("root");
if (!root) throw new Error("No se encontró el elemento #root");

createRoot(root).render(<StrictMode>{isAlertWindow ? <AlertWindow /> : <App />}</StrictMode>);
