/**
 * Le pillole della Libreria: Playlist, Album, Artisti, Brani, Cartelle,
 * Preferiti.
 *
 * Sul computer sono sei voci della barra laterale; sul telefono sono una
 * scheda sola, e dentro la scheda una fila di pillole che scorre di lato. Sono
 * pulsanti con `aria-pressed` e non linguette (`role="tab"`): ognuna cambia la
 * pagina intera, testata compresa, e non un pannello accanto — la stessa cosa
 * che fa una voce della barra del computer.
 *
 * La pillola accesa si porta in vista da sé: con la fila scorsa fino ai
 * Preferiti, aprire la Libreria sulle Playlist la lascerebbe fuori dallo
 * schermo, cioè senza dire dove si è.
 */
import { useLayoutEffect, useRef } from "react";

import { t } from "../lingue";
import type { VistaLibreria } from "../pagina";
import { PILLOLE_LIBRERIA } from "../schedeTelefono";

function etichettaDi(vista: VistaLibreria): string {
  switch (vista) {
    case "playlist":
      return t("playlist.group");
    case "album":
      return t("nav.albums");
    case "artisti":
      return t("nav.artists");
    case "brani":
      return t("nav.tracks");
    case "cartelle":
      return t("nav.folders");
    case "preferiti":
      return t("nav.favorites");
  }
}

export function PilloleLibreria({
  vista,
  onVista,
}: {
  /** La pillola accesa, o `null` quando sopra c'è aperto qualcosa. */
  vista: VistaLibreria | null;
  onVista: (vista: VistaLibreria) => void;
}) {
  const fila = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const accesa = fila.current?.querySelector<HTMLElement>("[aria-pressed='true']");
    // `inline: "nearest"` e niente `behavior`: la fila si sposta il minimo, e
    // di scatto, perché il gesto che l'ha chiesto è un cambio di pagina e non
    // uno scorrimento da seguire con l'occhio.
    accesa?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [vista]);
  return (
    <div ref={fila} className="pillole-libreria" role="group" aria-label={t("library.pills.aria")}>
      {PILLOLE_LIBRERIA.map((v) => (
        <button
          key={v}
          type="button"
          className="pillola-libreria"
          aria-pressed={v === vista}
          onClick={() => onVista(v)}
        >
          {etichettaDi(v)}
        </button>
      ))}
    </div>
  );
}
