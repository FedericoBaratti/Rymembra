import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { Simboli } from "./parti/Icone";
import "./stile.css";

const radice = document.getElementById("radice");
if (radice) {
  createRoot(radice).render(
    <StrictMode>
      {/*
       * I simboli **prima** dell'applicazione, e fuori da essa: `<use href="#…">`
       * cerca nel documento, quindi basta che esistano una volta. Montarli dentro
       * un componente che si smonta — la schermata dello Studio, per dire —
       * lascerebbe le icone di quella dopo senza disegno.
       */}
      <Simboli />
      <App />
    </StrictMode>,
  );
}
