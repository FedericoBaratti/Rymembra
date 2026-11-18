/**
 * «Leggo la libreria di <PC>… 12.000/50.000»: il primo collegamento, in un
 * riquadro della pila delle notifiche.
 *
 * Quando un dispositivo si collega, il suo catalogo entra in questa libreria
 * un secchio alla volta (`lontano/specchio.rs`): per cinquantamila brani sono
 * qualche minuto. Nel frattempo la libreria si usa — i brani arrivano negli
 * elenchi man mano — e questo dice che c'è ancora da aspettare. Sparisce da sé a
 * giro finito.
 *
 * Una regione di stato sempre montata e col testo che cambia: una che nasce già
 * piena spesso non viene annunciata.
 */
import { useArchivio } from "../archivio";
import { numero } from "../formato";
import { t } from "../lingue";
import { ipc } from "../ipc";
import {
  archivioLontano,
  archivioScarico,
  archivioSpecchio,
  inLettura,
  percentualeScarico,
} from "../lontano";
import { dispositiviDiLibreria } from "../origine";
import { Icona } from "./Icone";

export function StrisciaSpecchio() {
  const avanzamenti = useArchivio(archivioSpecchio);
  const stato = useArchivio(archivioLontano);
  const nomi = new Map(dispositiviDiLibreria(stato).map((d) => [d.chiave, d.nome]));
  const leggendo = inLettura(avanzamenti).filter((a) => nomi.has(a.dispositivo));
  return (
    <div role="status">
      {leggendo.map((a) => (
        <div key={a.dispositivo} className="notizia toast-card">
          <Icona nome="i-scan" dim={16} />
          <span>
            {t("devices.mirror.reading", {
              nome: nomi.get(a.dispositivo) ?? "",
              fatti: numero(a.fatti),
              totali: numero(a.totali),
            })}
          </span>
        </div>
      ))}
    </div>
  );
}

/**
 * La striscia di «Scarica qui», finché scende qualcosa: «Scarico <titolo> 45%
 * · 3 in coda», con «Annulla».
 *
 * Stava in cima alla pagina «Sfoglia». Con la libreria unica «Scarica qui» parte
 * dal menù di una riga o di un album, in qualunque vista, e l'avanzamento sta
 * dove sta quello della scansione: nella pila delle notifiche.
 */
export function StrisciaScarico({ onErrore }: { onErrore: (errore: unknown) => void }) {
  const stato = useArchivio(archivioScarico);
  if (stato === null || stato.titolo === null) return null;
  return (
    <div className="notizia toast-card" role="status">
      <Icona nome="i-import" dim={16} />
      <span>
        {t("devices.download.running", {
          titolo: stato.titolo,
          percento: percentualeScarico(stato),
        })}
        {stato.inCoda > 0 ? ` · ${t("devices.download.queued", { n: stato.inCoda })}` : ""}
      </span>
      <button
        type="button"
        className="bottone minuto btn-ghost"
        onClick={() => {
          ipc
            .lontanoScaricoAnnulla()
            .then((adesso) => archivioScarico.scrivi(adesso))
            .catch(onErrore);
        }}
      >
        {t("common.cancel")}
      </button>
    </div>
  );
}
