import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { bootstrapTheme } from "./theme";
import "./index.css";

// Resolve + apply the theme before React mounts so the first paint already
// has the correct surface / foreground colours — no "dark flash on a light
// system" or vice versa.
bootstrapTheme();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
