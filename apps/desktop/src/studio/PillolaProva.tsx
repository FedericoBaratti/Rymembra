/**
 * La pillola di «Prova sull'app»: la bozza indossata dalla finestra vera.
 *
 * Sta fuori dallo Studio, che intanto è nascosto sotto `<Activity>` con tutto
 * quel che aveva in mano: si naviga l'applicazione vera — le pagine, i menù, la
 * musica — con la skin che si sta disegnando, e un tasto riporta indietro.
 * «Tienila» la installa e la indossa per davvero. Esc torna.
 */
import { useEffect } from "react";

import { t } from "../lingue";
import { Icona } from "../parti/Icone";

export function PillolaProva({
  nome,
  onTorna,
  onTieni,
}: {
  nome: string;
  onTorna: () => void;
  onTieni: () => void;
}) {
  useEffect(() => {
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      onTorna();
    };
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, [onTorna]);

  return (
    <div className="pillola-prova" role="region" aria-label={t("studio3.try.aria")}>
      <Icona nome="i-eye" dim={16} />
      <span>{t("studio3.try.label", { nome })}</span>
      <button type="button" className="bottone minuto" onClick={onTorna}>
        {t("studio3.try.back")}
      </button>
      <button type="button" className="bottone minuto primario" onClick={onTieni}>
        {t("studio3.try.keep")}
      </button>
    </div>
  );
}
