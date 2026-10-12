import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initTheme } from "./theme/theme";
import "./theme/tokens.css";

// Тема применяется синхронно до первой отрисовки — без мигания.
initTheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
