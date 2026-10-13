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
import { Dialogo } from "./Dialogo";
import type { EsameSkin, MisuraSkin } from "./ipc";
import { t } from "./lingue";

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

  // Una riga per quel che sfora, nell'ordine in cui si nota: prima il testo
  // che non si legge, poi la finestra che scatta, poi quel che non sta fermo.
  const righe: string[] = [];
  if (esame.contrasti > 0) {
    righe.push(t("skin.review.contrast", { n: esame.contrasti }));
  }
  // L'anello del fuoco sta col testo: è l'altra cosa che, se non si vede, lascia
  // qualcuno senza sapere dove si trova (WP27, `aether_skin::focus_ring_pairs`).
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
      onChiudi={onChiudi}
    >
      <ul className="esame-skin">
        {righe.map((riga) => (
          <li key={riga}>{riga}</li>
        ))}
      </ul>
      <p>{t("skin.review.installed")}</p>
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={onChiudi}>
          {t("skin.review.notNow")}
        </button>
        <button type="button" className="bottone primario" onClick={onIndossa}>
          {t("skin.review.wear")}
        </button>
      </div>
    </Dialogo>
  );
}
