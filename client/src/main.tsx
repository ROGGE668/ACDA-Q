import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import { useSettingsStore } from "./stores/settingsStore";
import { setApiBase } from "./services/api";
import "./styles.css";

useSettingsStore.getState().loadSettings().then(() => {
  const state = useSettingsStore.getState();
  setApiBase(state.apiBase);
  document.documentElement.setAttribute("data-theme", state.theme);
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </React.StrictMode>
);
