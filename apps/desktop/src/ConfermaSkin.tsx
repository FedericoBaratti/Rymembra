/**
 * «Questa skin ha qualcosa da dirti, prima che tu la indossi.»
 *
 * # Perché esiste
 *
 * Una skin si scambia, e chi ne installa una arrivata da un altro non sa cosa
 * si sta mettendo addosso finché non l'ha addosso: una finestra che scatta a
 * ogni fotogramma, un testo grigio su grigio. Fino alla 2.4 installare voleva
 * dire indossare, subito. Ora l'installazione risponde con l'esame del nucleo
 * (`skin.rs::EsameIpc`), e quando l'esame dice `daConfermare` si chiede prima.
 * Quando non lo dice, non si chiede niente: una domanda a ogni installazione
 * insegna a premere «sì» senza leggere, cioè a non leggere proprio quella che
 * conta.
 *
 * Che cosa meriti la domanda lo decide il nucleo, non questo file. Qui si
 * mostra: i numeri sono quelli dei budget, le frasi dicono cosa vogliono dire
 * per chi guarda.
 *
 * # Due uscite, e nessuna toglie niente
 *
 * La skin **è già installata** quando questa finestrella si apre: rifiutare
 * vuol dire non indossarla adesso, e la si ritrova nell'elenco — dove la si
 * può scegliere più tardi, o togliere. Il comportamento — il fuoco che entra,
 * la trappola del Tab, Escape, il fuoco che torna a chi l'aveva, la pagina
 * dietro inerte — viene tutto da `Dialogo`.
 */
import { useState } from "react";

import { Dialogo } from "./Dialogo";
import type { EsameSkin, MisuraSkin, SostituzioneSkin, VersioneSkin } from "./ipc";
import { t } from "./lingue";
import { unaVolta } from "./modale";
import { Icona } from "./parti/Icone";
import type { Sostituita } from "./skinAttiva";

export function ConfermaSkin({
  nome,
  esame,
  onIndossa,
  onChiudi,
}: {
  nome: string;
  esame: EsameSkin;
  onIndossa: () => void;
  onChiudi: () => void;
}) {
  const titolo = t("skin.review.title", { nome });
  const numeri = (m: MisuraSkin) => ({ costo: m.costo, budget: m.budget });
  // Una risposta sola: un doppio clic su «Indossa» la indossava due volte.
  // Vedi `unaVolta` in `modale.ts`.
  const [prima] = useState(unaVolta);
  const chiudi = () => {
    if (prima()) onChiudi();
  };
  const indossa = () => {
    if (prima()) onIndossa();
  };

  // Una riga per quel che sfora, nell'ordine in cui si nota: prima il testo
  // che non si legge, poi la finestra che scatta, poi quel che non sta fermo.
  const righe: string[] = [];
  if (esame.contrasti > 0) {
    righe.push(t("skin.review.contrast", { n: esame.contrasti }));
  }
  // L'anello del fuoco sta col testo: è l'altra cosa che, se non si vede, lascia
  // qualcuno senza sapere dove si trova (WP27, `rymembra_skin::focus_ring_pairs`).
  if (esame.anelli > 0) {
    righe.push(t("skin.review.focusRing", { n: esame.anelli }));
  }
  if (esame.superficie.costo > esame.superficie.budget) {
    righe.push(t("skin.review.surface", numeri(esame.superficie)));
  }
  if (esame.movimentoParte.costo > esame.movimentoParte.budget) {
    righe.push(t("skin.review.motionPart", numeri(esame.movimentoParte)));
  }
  if (esame.movimento.costo > esame.movimento.budget) {
    righe.push(t("skin.review.motion", numeri(esame.movimento)));
  }
  if (esame.scafale.costo > esame.scafale.budget) {
    righe.push(t("skin.review.shell", numeri(esame.scafale)));
  }
  // I cicli non sono un avviso — non basterebbero ad aprire questa
  // finestrella — ma se la finestrella c'è, chi decide deve saperlo.
  if (esame.cicli.length > 0) {
    righe.push(t("skin.review.loops", { n: esame.cicli.length }));
  }

  return (
    <Dialogo
      titolo={titolo}
      descrizione={t("skin.review.body")}
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      onChiudi={chiudi}
    >
      <ul className="esame-skin">
        {righe.map((riga) => (
          <li key={riga}>{riga}</li>
        ))}
      </ul>
      <p>{t("skin.review.installed")}</p>
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={chiudi}>
          {t("skin.review.notNow")}
        </button>
        <button type="button" className="bottone primario" onClick={indossa}>
          {t("skin.review.wear")}
        </button>
      </div>
    </Dialogo>
  );
}

/** Nome, versione e autore, in una riga: quel che distingue due skin con lo stesso id. */
function Versione({ skin }: { skin: VersioneSkin }) {
  return (
    <>
      <strong>{skin.nome}</strong>
      {skin.versione !== "" && <> · {t("skin.replace.version", { versione: skin.versione })}</>}
      {skin.autore !== "" && <> · {t("skin.replace.by", { autore: skin.autore })}</>}
    </>
  );
}

/**
 * «Sostituire “Notte”?» — un file `.ryskin` con l'id di una skin installata.
 *
 * # Perché si chiede
 *
 * Fino alla 3.2 il file prendeva il posto della skin installata in silenzio, e
 * il backup portava la sostituzione su Drive: la skin fatta nello Studio, coi
 * suoi caratteri e la sua miniatura, se ne andava per il file di un altro che
 * portava lo stesso id. Il nucleo adesso non scrive niente e risponde con le
 * due (`skin.rs::installa_file`), e qui le si mettono una sotto l'altra: due
 * skin che si chiamano uguale si distinguono per versione e autore.
 *
 * Come le altre conferme di un gesto che toglie qualcosa, il primario e il
 * fuoco stanno su «Annulla». Quel che si toglie qui però torna: la skin di
 * prima resta da parte, e la notizia che segue (`AvvisoSostituita`) la rimette
 * con un tasto.
 *
 * È un consenso (`consenso` di `Dialogo`): la skin indossata non ridisegna la
 * domanda che decide se sostituire un'altra skin.
 */
export function ConfermaSostituzione({
  domanda,
  onSostituisci,
  onChiudi,
}: {
  domanda: SostituzioneSkin;
  onSostituisci: () => void;
  onChiudi: () => void;
}) {
  // Una risposta sola, come in `ConfermaSkin`.
  const [prima] = useState(unaVolta);
  const chiudi = () => {
    if (prima()) onChiudi();
  };
  const sostituisci = () => {
    if (prima()) onSostituisci();
  };
  return (
    <Dialogo
      titolo={t("skin.replace.title", { nome: domanda.presente.nome })}
      descrizione={t("skin.replace.body")}
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      onChiudi={chiudi}
      consenso
    >
      <dl className="sostituzione-skin">
        <dt>{t("skin.replace.current")}</dt>
        <dd>
          <Versione skin={domanda.presente} />
        </dd>
        <dt>{t("skin.replace.incoming")}</dt>
        <dd>
          <Versione skin={domanda.nuova} />
        </dd>
      </dl>
      <div className="tasti-finestrella">
        <button
          type="button"
          className="bottone primario"
          onClick={chiudi}
          data-fuoco-iniziale
        >
          {t("common.cancel")}
        </button>
        <button type="button" className="bottone" onClick={sostituisci}>
          {t("skin.replace.confirm")}
        </button>
      </div>
    </Dialogo>
  );
}

/**
 * «“Notte” ha preso il posto di “La mia notte”, che resta da parte — Rimettila».
 *
 * La stessa forma dell'«Annulla» di una playlist eliminata in `App.tsx`, classi
 * comprese: è lo stesso gesto, rimettere quel che un clic ha appena tolto. Non
 * ha scadenza, perché la copia resta da parte finché un'altra sostituzione
 * della stessa skin non ne prende il posto.
 */
export function AvvisoSostituita({
  sostituita,
  onRimetti,
  onChiudi,
}: {
  sostituita: Sostituita;
  onRimetti: () => void;
  onChiudi: () => void;
}) {
  const nomi = { nuova: sostituita.nuova.nome, vecchia: sostituita.vecchia.nome };
  return (
    <div className="notizia toast-card">
      <Icona nome="i-check" dim={16} />
      <span>{t("skin.replace.done", nomi)}</span>
      <button
        type="button"
        className="bottone minuto btn-ghost"
        aria-label={t("skin.replace.undo.aria", nomi)}
        onClick={onRimetti}
      >
        {t("skin.replace.undo")}
      </button>
      <button
        type="button"
        className="tasto icon-btn"
        aria-label={t("common.close")}
        onClick={onChiudi}
      >
        <Icona nome="i-x" dim={14} />
      </button>
    </div>
  );
}
