/**
 * «La scansione toglierebbe 1.340 brani: sicuro?»
 *
 * # Perché esiste
 *
 * Fino alla 2.4 una scansione chiesta a mano toglieva senza chiedere: chi la
 * guardava leggeva «1.340 tolti» a cose fatte, e con quei brani se n'erano già
 * andati cronologia, playlist, voti e preferiti. Il caso vero non è quasi mai
 * «li ho cancellati io»: è il disco staccato, la share che risponde a metà, la
 * cartella rinominata fuori da Aether. Adesso, sopra la stessa soglia che
 * protegge la scansione automatica, il nucleo **non toglie**: finisce il resto
 * del lavoro e consegna un riassunto — quanti, e da quali cartelle — che questa
 * finestrella mostra.
 *
 * Sotto la soglia non si chiede niente, ed è voluto: una domanda a ogni album
 * cancellato insegna a premere «Togli» senza leggere, cioè a non leggere
 * proprio la volta che conta.
 *
 * # Due uscite
 *
 * «Lasciali» è il primario, come in ogni finestrella che si apre sopra un gesto
 * distruttivo: l'azione che il colore invita a premere è quella che non toglie
 * niente. Lasciarli non costa nulla — le righe restano, e la prossima
 * scansione, se i file mancano ancora, chiederà di nuovo. «Togli» toglie
 * **esattamente** l'elenco che la scansione ha trattenuto, senza guardare di
 * nuovo il disco; e anche lì la storia dei brani resta da parte novanta giorni.
 *
 * Il comportamento — il fuoco che entra, la trappola del Tab, Escape, il fuoco
 * che torna a chi l'aveva, la pagina dietro inerte — viene tutto da `Dialogo`,
 * che la apre come `alertdialog`: è una domanda prima di un gesto che toglie.
 * Escape vale «Lasciali».
 */
import { Dialogo } from "./Dialogo";
import { brani_ } from "./formato";
import type { RimozioniDaConfermare } from "./ipc";
import { t } from "./lingue";

/**
 * Il percorso da sotto la sua cartella sorvegliata.
 *
 * La cartella è già nel titolo della voce: ripeterla in ogni riga — su una
 * share, sono ottanta caratteri di `\\nas\…` uguali — lasciava vedere solo
 * l'inizio, che è la parte che non distingue niente. Il percorso intero resta
 * nel `title`.
 */
function sottoLaRadice(percorso: string, radice: string): string {
  const cartella = radice.replace(/[\\/]+$/, "");
  if (cartella === "") return percorso;
  const dentro =
    percorso.toLowerCase().startsWith(cartella.toLowerCase()) &&
    /[\\/]/.test(percorso.charAt(cartella.length));
  return dentro ? percorso.slice(cartella.length + 1) : percorso;
}

export function ConfermaRimozioni({
  attesa,
  onTogli,
  onChiudi,
}: {
  attesa: RimozioniDaConfermare;
  onTogli: () => void;
  onChiudi: () => void;
}) {
  const titolo = t("scan.confirm.title", { brani: brani_(attesa.quante) });
  return (
    <Dialogo
      titolo={titolo}
      descrizione={t("scan.confirm.body")}
      ruolo="alertdialog"
      classe="finestrella media glass-modal"
      onChiudi={onChiudi}
    >
      {/* Una voce per cartella sorvegliata, aperta: chi deve decidere
          deve vedere subito se sono tutti da `\\nas\musica` — la rete —
          o sparsi — un riordino. I percorsi sono i primi dell'elenco, e
          bastano a riconoscere di cosa si sta parlando. */}
      {attesa.radici.map((voce) => (
        <details
          key={voce.radice}
          className="spostamenti"
          open={attesa.radici.length === 1}
        >
          <summary>
            {voce.radice === ""
              ? t("scan.confirm.elsewhere", { brani: brani_(voce.quante) })
              : t("scan.confirm.root", {
                  brani: brani_(voce.quante),
                  radice: voce.radice,
                })}
          </summary>
          <ul>
            {voce.esempi.map((percorso) => (
              <li key={percorso} className="percorso" title={percorso}>
                {sottoLaRadice(percorso, voce.radice)}
              </li>
            ))}
            {voce.quante > voce.esempi.length && (
              <li>
                {t("scan.confirm.more", {
                  n: voce.quante - voce.esempi.length,
                })}
              </li>
            )}
          </ul>
        </details>
      ))}
      <p>{t("scan.confirm.kept")}</p>
      <div className="tasti-finestrella">
        <button
          type="button"
          className="bottone primario"
          data-fuoco-iniziale
          onClick={onChiudi}
        >
          {t("scan.confirm.keep")}
        </button>
        <button type="button" className="bottone" onClick={onTogli}>
          {t("scan.confirm.remove")}
        </button>
      </div>
    </Dialogo>
  );
}
