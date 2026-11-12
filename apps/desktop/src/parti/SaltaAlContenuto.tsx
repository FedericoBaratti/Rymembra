/**
 * Il primo comando della finestra: «Salta al contenuto», visibile solo col
 * fuoco. Vedi `saltaAlContenuto.ts`.
 *
 * Sta in testa a `.finestra-app`, e non nello scafale: lo scafale lo disegna
 * la skin, e un salto che una skin potesse spostare o nascondere non sarebbe
 * più il primo. Il resto lo fa la finestra da sé: sotto una finestrella, al
 * primo avvio, nel giro e con lo Studio aperto `.finestra-app` (o il suo
 * contenuto) è inerte, e il salto con lei.
 */
import { t } from "../lingue";
import { saltaAlContenuto } from "../saltaAlContenuto";

export function SaltaAlContenuto() {
  return (
    <button
      type="button"
      className="salta-al-contenuto"
      onClick={() => {
        saltaAlContenuto(document);
      }}
    >
      {t("nav.skipToContent")}
    </button>
  );
}
