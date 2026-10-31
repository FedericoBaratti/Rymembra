/**
 * Le Impostazioni della tela: la sezione «Aspetto», coi valori di un'app vera.
 *
 * Una sola sezione, e non è pigrizia: le altre montano schede che leggono dal
 * nucleo appena nascono — i testi, la dissolvenza, lo spegnimento, la
 * chiusura — e sulla tela quelle letture andrebbero al disco di chi disegna.
 * «Aspetto» vive di prop, e ha tutto quel che una skin ridipinge nelle
 * impostazioni: l'indice, le schede, i segmentati, gli interruttori, le
 * pastiglie.
 */
import type { ComponentProps } from "react";

import type { UsoImportazioni } from "../parti/Importazioni";
import type { Impostazioni } from "../schermate/Impostazioni";
import { leggiAssociazioni } from "../tastiera";

const niente = () => {
  /* apposta */
};

/**
 * Le importazioni, vuote. La sezione «Aspetto» non le guarda; il tipo le
 * vuole, e un elenco vuoto è quel che un'app appena installata ha davvero.
 */
const IMPORTAZIONI = {
  elenco: [],
  stato: null,
  brano: null,
  errore: null,
  rientro: null,
} as unknown as UsoImportazioni;

export function impostazioniFinte(): ComponentProps<typeof Impostazioni> {
  return {
    sezione: "aspetto",
    onSezione: niente,
    avvio: null,
    scansione: null,
    esito: null,
    skin: [
      {
        id: "plain",
        nome: "Plain",
        autore: "Rymembra",
        descrizione: null,
        diSerie: true,
        attiva: false,
        anteprima: ["#0f0f14", "#1b1b24", "#8b7cf6"],
        scheme: "dark",
        variante: true,
      },
      {
        id: "vinile",
        nome: "Vinile",
        autore: "Rymembra",
        descrizione: null,
        diSerie: true,
        attiva: true,
        anteprima: ["#f3ede2", "#e6dccb", "#a3301c"],
        scheme: "light",
        variante: true,
      },
    ],
    ripiego: null,
    accentoPermesso: true,
    accentoDinamico: true,
    onAccentoDinamico: niente,
    movimento: "full",
    movimentoUtente: "sistema",
    onMovimentoUtente: niente,
    zoom: 1,
    onZoom: niente,
    onZoomNormale: niente,
    tema: "sistema",
    onTema: niente,
    lingua: "it",
    onLingua: niente,
    scorciatoie: leggiAssociazioni(null),
    onScorciatoie: niente,
    onGiro: niente,
    onProfiloImportato: niente,
    eqAttivo: false,
    eqGuadagni: [],
    replaygain: "normale",
    onReplaygain: niente,
    spegnimentoMs: null,
    onSpegnimento: niente,
    autoplay: false,
    onAutoplay: niente,
    dissolvenzaS: 0,
    onDissolvenza: niente,
    onErrore: niente,
    onNotizia: niente,
    onAggiungiCartella: niente,
    onTogliCartella: niente,
    onScollegaConto: niente,
    onScegliCartellaDownload: niente,
    onCartellaDownloadDiSerie: niente,
    onScansiona: niente,
    onAnnullaScansione: niente,
    onScegliSkin: niente,
    onAnteprimaSkin: niente,
    onInstallaSkin: niente,
    onCreaTema: niente,
    onDeriva: niente,
    onDisinstallaSkin: niente,
    onApriStudio: niente,
    onImporta: niente,
    onImportaLink: niente,
    onImportaAccount: niente,
    onVista: niente,
    importazioni: IMPORTAZIONI,
    arricchimento: null,
    avanzaArricchimento: null,
    esitoArricchimento: null,
    onArricchimentoAttiva: niente,
    onArricchimentoAnnulla: niente,
    nuvola: null,
    onNuvolaCollega: niente,
    onNuvolaScollega: niente,
    onNuvolaAttiva: niente,
    onNuvolaSalva: niente,
    onNuvolaRipristina: niente,
    onNuvolaBackupApp: niente,
    sincronia: null,
    onSincroniaAttiva: niente,
    onSincroniaAdesso: niente,
    onSincroniaMagazzino: niente,
    onSincroniaAccoppia: niente,
    onSincroniaDimentica: niente,
  };
}
