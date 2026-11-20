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
 * # A chi ascolta lo schermo
 *
 * La striscia **non** è una regione viva: ha un contatore dentro, che cambia a
 * ogni passo del catalogo (e quella dello scarico a ogni 2 MiB), e un lettore di
 * schermo rileggeva il testo intero a ogni cambio, per i minuti della prima
 * lettura. Come la scansione (`AnnuncioScansione.tsx`) si dice il passaggio e
 * non lo stato: una regione montata sempre e vuota (`AnnuncioSpecchio`) dice
 * quando una lettura comincia e quando finisce, e chi vuole il punto guarda la
 * striscia (S9-11).
 */
import { useEffect, useRef, useState } from "react";

import { useArchivio } from "../archivio";
import { numero } from "../formato";
import { t } from "../lingue";
import { ipc } from "../ipc";
import {
  annunciDelloSpecchio,
  archivioLontano,
  archivioScarico,
  archivioSpecchio,
  inErrore,
  inLettura,
  percentualeScarico,
  type AvanzamentiSpecchio,
} from "../lontano";
import { dispositiviDiLibreria } from "../origine";
import { Icona } from "./Icone";

/**
 * L'inizio e la fine di una lettura, detti a chi ascolta lo schermo.
 *
 * Una regione sempre montata e vuota, col testo che le cambia sotto: una che
 * nasce già piena spesso non viene annunciata (vedi `Annuncio.tsx`).
 */
function AnnuncioSpecchio({
  avanzamenti,
  nomi,
}: {
  avanzamenti: AvanzamentiSpecchio;
  nomi: ReadonlyMap<string, string>;
}) {
  const [detto, setDetto] = useState("");
  /** Com'era al disegno prima: si annuncia il passaggio, non lo stato. */
  const prima = useRef(avanzamenti);
  const nomiDiAdesso = useRef(nomi);
  nomiDiAdesso.current = nomi;
  /**
   * Chi ha già detto che la lettura si è interrotta: il nucleo riprova ogni
   * cinque minuti, e un dispositivo che non risponde la interromperebbe ogni
   * volta. Si dice una volta, finché non finisce bene.
   */
  const dettiInErrore = useRef(new Set<string>());
  useEffect(() => {
    const eventi = annunciDelloSpecchio(prima.current, avanzamenti);
    prima.current = avanzamenti;
    const frasi = eventi.flatMap((e) => {
      const nome = nomiDiAdesso.current.get(e.dispositivo);
      if (e.tipo === "fine") dettiInErrore.current.delete(e.dispositivo);
      if (nome === undefined) return [];
      if (e.tipo === "errore") {
        if (dettiInErrore.current.has(e.dispositivo)) return [];
        dettiInErrore.current.add(e.dispositivo);
        return [t("devices.mirror.failed", { nome })];
      }
      return [
        t(e.tipo === "inizio" ? "devices.mirror.announce.start" : "devices.mirror.announce.end", { nome }),
      ];
    });
    if (frasi.length > 0) setDetto(frasi.join(" "));
  }, [avanzamenti]);
  return (
    <div className="solo-lettori" role="status" data-annuncio="">
      {detto}
    </div>
  );
}

export function StrisciaSpecchio({ onErrore }: { onErrore: (errore: unknown) => void }) {
  const avanzamenti = useArchivio(archivioSpecchio);
  const stato = useArchivio(archivioLontano);
  const nomi = new Map(dispositiviDiLibreria(stato).map((d) => [d.chiave, d.nome]));
  const leggendo = inLettura(avanzamenti).filter((a) => nomi.has(a.dispositivo));
  // La lettura che si è interrotta resta detta finché il nucleo non riprova da sé
  // (ogni cinque minuti) o la persona non preme «Riprova adesso» (S9-12). Non per
  // un dispositivo che sta già rileggendo.
  const interrotte = inErrore(avanzamenti).filter((a) => nomi.has(a.dispositivo));
  return (
    <>
      <AnnuncioSpecchio avanzamenti={avanzamenti} nomi={nomi} />
      <div>
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
        {interrotte.map((a) => {
          const nome = nomi.get(a.dispositivo) ?? "";
          return (
            <div key={a.dispositivo} className="notizia toast-card">
              <Icona nome="i-alert" dim={16} />
              <span>{t("devices.mirror.failed", { nome })}</span>
              <button
                type="button"
                className="bottone minuto btn-ghost"
                onClick={() => {
                  ipc.lontanoSpecchioOra(a.dispositivo).catch(onErrore);
                }}
              >
                {t("devices.mirror.retryNow")}
              </button>
            </div>
          );
        })}
      </div>
    </>
  );
}

/**
 * La striscia di «Scarica qui», finché scende qualcosa: «Scarico <titolo> 45%
 * · 3 in coda», con «Annulla».
 *
 * Stava in cima alla pagina «Sfoglia». Con la libreria unica «Scarica qui» parte
 * dal menù di una riga o di un album, in qualunque vista, e l'avanzamento sta
 * dove sta quello della scansione: nella pila delle notifiche.
 *
 * Non è una regione viva: la percentuale cambia ogni 2 MiB, e un lettore di
 * schermo la rileggerebbe tutta. La fine dello scarico la dice già la notizia
 * di `Dispositivi.tsx` («Scaricati 12 brani»).
 */
export function StrisciaScarico({ onErrore }: { onErrore: (errore: unknown) => void }) {
  const stato = useArchivio(archivioScarico);
  if (stato === null || stato.titolo === null) return null;
  return (
    <div className="notizia toast-card">
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
