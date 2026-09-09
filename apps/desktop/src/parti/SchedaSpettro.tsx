/**
 * Quanto la scena dello spettro può costare a questa macchina.
 *
 * # La linea fra questa scheda e una skin
 *
 * Una skin dice come la scena **appare** — colori, geometria, quanto la camera
 * gira — ed è materia dello Studio, dove si vede quel che si cambia mentre lo
 * si cambia. Questa scheda dice due cose che una skin non può dire: se la scena
 * parte accesa, e quanto **questo computer** è disposto a spenderci. Sono le due
 * metà che restano vere quando la skin cambia.
 *
 * # Perché in «Riproduzione» e non in «Aspetto»
 *
 * Perché il dominio delle chiavi è `player.*`, come ogni altro residente di
 * quella sezione, e «Aspetto» è dove vive il *look* — che per la linea qui sopra
 * è esattamente la metà che non è un'impostazione, ma un blocco di token.
 *
 * # Perché si gestisce da sé
 *
 * La stessa ragione di `SchedaUscita` e `SchedaChiusura`: `Impostazioni` riceve
 * già sessantasette prop, e queste due preferenze non le guarda nessun'altra
 * parte di questa schermata. Quel che le serve se lo chiede quando viene
 * montata, e lo lascia andare quando si smonta.
 *
 * # Perché i controlli sono pessimistici
 *
 * Perché lo stato arriva dal **ritorno** del comando e non dal click. È la
 * disciplina di `SchedaChiusura` e dell'equalizzatore, e qui vale doppio per la
 * qualità: il nucleo prende una stringa e restituisce un livello, quindi un nome
 * che non è uno dei tre torna indietro come `auto`. Dipingere dal click vorrebbe
 * dire mostrare accesa una linguetta che nel database non è mai stata scritta.
 *
 * # Perché il numero di barre non è qui
 *
 * Perché si sceglie guardandolo: la differenza fra 64 e 512 non si immagina
 * leggendo un numero in un'altra schermata. Il comando sta in basso a sinistra
 * dentro la scena (`DettaglioSpettro`), e questa scheda dice **dov'è** invece di
 * duplicarlo — due controlli per la stessa preferenza sono due posti in cui
 * guardare e uno in cui sbagliarsi.
 */
import { useEffect, useState } from "react";

import { Interruttore } from "./Interruttore";
import { Segmentato, type Voce } from "./Segmentato";
import { ipc, type QualitaSpettro } from "../ipc";
import { t } from "../lingue";

export function SchedaSpettro({
  onErrore,
}: {
  /** Le due scelte scrivono nel database: se non ci arrivano, va detto. */
  onErrore: (e: unknown) => void;
}) {
  const [visibile, setVisibile] = useState(false);
  const [qualita, setQualita] = useState<QualitaSpettro>("auto");

  // I tre livelli, nell'ordine in cui si leggono: l'automatico e poi i due
  // estremi. Costruiti qui dentro e non a livello di modulo, perché le
  // etichette sono testo tradotto: un elenco calcolato all'import resterebbe
  // nella lingua che c'era al primo caricamento.
  const livelli: readonly Voce<QualitaSpettro>[] = [
    { chiave: "auto", etichetta: t("settings.spectrum.quality.auto") },
    { chiave: "alta", etichetta: t("settings.spectrum.quality.alta") },
    { chiave: "bassa", etichetta: t("settings.spectrum.quality.bassa") },
  ];

  useEffect(() => {
    let annullato = false;
    ipc
      .spettroVisibile()
      .then((letto) => {
        if (!annullato) setVisibile(letto);
      })
      .catch(() => {
        /* Spenta, che è il valore di serie: una preferenza che non si legge non
           deve poter accendere una scena WebGL a chi non l'aveva chiesta. */
      });
    ipc
      .spettroQualita()
      .then((letta) => {
        if (!annullato) setQualita(letta);
      })
      .catch(() => {
        /* `auto`, che è il valore di serie e la risposta giusta ogni volta che
           non si sa niente di questa macchina. */
      });
    return () => {
      annullato = true;
    };
  }, []);

  return (
    <>
      <Interruttore
        etichetta={t("settings.spectrum.toggle")}
        spiegazione={t("settings.spectrum.toggle.hint")}
        acceso={visibile}
        onCambia={(valore) => {
          ipc.spettroVisibileScegli(valore).then(setVisibile).catch(onErrore);
        }}
      />
      <Segmentato
        voci={livelli}
        scelta={qualita}
        onScegli={(livello) => {
          ipc.spettroQualitaScegli(livello).then(setQualita).catch(onErrore);
        }}
        etichetta={t("settings.spectrum.quality.label")}
      />
      <p className="nota">{t("settings.spectrum.quality.hint")}</p>
      <p className="nota">{t("settings.spectrum.bars.where")}</p>
    </>
  );
}
