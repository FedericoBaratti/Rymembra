/**
 * La finestrella che battezza un tema nuovo.
 *
 * # Perché quattro domande e non una
 *
 * Perché `meta` ha quattro campi obbligatori — `name`, `author`, `version` — e
 * uno facoltativo che la scheda della skin mostra sotto il nome. Chiedere solo
 * il nome vorrebbe dire scrivere «Aether» come autore di un tema che l'utente ha
 * fatto da sé, e lasciare a lui il compito di scoprire che si corregge nella
 * scheda «Documento» dello Studio. La versione è l'unica che non si chiede: un
 * tema nuovo è `1.0.0` e non c'è una seconda risposta possibile.
 *
 * # Perché la base si sceglie qui
 *
 * Perché è la domanda che non si può più fare dopo. Nome, autore e descrizione
 * si riscrivono in qualunque momento; da cosa si è partiti no — a Studio aperto
 * la scelta è già stata fatta, e cambiarla vuol dire ricominciare.
 *
 * L'identificatore invece **non** si chiede: si deriva dal nome e si mostra
 * mentre lo si scrive. È un nome di file e un selettore CSS, non una cosa da
 * comporre a mano, e mostrarlo senza chiederlo è quel che serve a non
 * sorprendere chi poi lo ritrova nella cartella delle skin.
 */
import { useEffect, useMemo, useState } from "react";

import { Dialogo } from "./Dialogo";
import { ipc, type VoceSkin } from "./ipc";
import { idDa, type DatiTema } from "./studio/nuovo";
import { t } from "./lingue";

/** Le tre fasce di una skin, col ripiego di `Impostazioni`. */
function bande(skin: VoceSkin): readonly string[] {
  return skin.anteprima.length === 3
    ? skin.anteprima
    : ["var(--color-surface-0)", "var(--color-surface-2)", "var(--accent)"];
}

export function NuovoTema({
  skin: tutte,
  baseIniziale,
  onCrea,
  onChiudi,
}: {
  /** Le skin fra cui scegliere la base, e gli id già presi. */
  skin: VoceSkin[];
  /**
   * Da quale skin partire, quando si arriva da «Deriva…».
   *
   * Chi preme «Deriva…» sulla scheda di una skin ha già detto da quale vuole
   * partire: richiederglielo con la skin attiva preselezionata sarebbe un modo
   * di fargli rifare una scelta appena fatta, e di fargliela sbagliare se non
   * si accorge del campo.
   */
  baseIniziale?: string | null | undefined;
  onCrea: (dati: DatiTema) => void;
  onChiudi: () => void;
}) {
  // Le basi possibili: non una skin installata che non passa la validazione —
  // il tema nuovo nascerebbe rotto — ma il suo id resta preso, sotto.
  const skin = useMemo(() => tutte.filter((s) => s.guasto === undefined), [tutte]);
  /**
   * Gli id che hanno già una cartella nello Studio — una bozza rimasta da una
   * skin disinstallata, una serie di istantanee — e che quindi un tema nuovo
   * non deve prendere: ne erediterebbe le risorse e la conversazione. Il
   * nucleo lo rifiuterebbe comunque (`studio_crea`); qui serve a proporre
   * subito un id libero invece di un errore dopo aver premuto «Crea».
   */
  const [occupati, setOccupati] = useState<readonly string[]>([]);
  useEffect(() => {
    let vivo = true;
    ipc
      .studioOccupati()
      .then((presi) => {
        if (vivo) setOccupati(presi);
      })
      // Senza l'elenco si propone l'id dalle sole skin: il nucleo rifiuta
      // quello occupato, e il messaggio dice cosa fare.
      .catch(() => undefined);
    return () => {
      vivo = false;
    };
  }, []);
  const [nome, setNome] = useState("");
  const [autore, setAutore] = useState("");
  const [descrizione, setDescrizione] = useState("");
  const [base, setBase] = useState(
    () =>
      baseIniziale ?? skin.find((s) => s.attiva)?.id ?? skin[0]?.id ?? "plain",
  );
  // Il fuoco sul campo del nome, la trappola del Tab, l'Escape e il ritorno
  // del fuoco a chi l'aveva: sta in `Dialogo` (e sotto, in `finestrella.ts`).
  // L'Escape che c'era qui ascoltava su `window`, quindi arrivava dopo la scala
  // di `tastiera.ts` e le faceva consumare un livello di troppo; il fuoco sul
  // campo lo prende ora l'hook, che il primo focalizzabile lo trova da sé.

  /** Le frecce girano dentro il gruppo, come nel segmentato. */
  const daTastiera = (e: React.KeyboardEvent, indice: number) => {
    // È una griglia che va a capo, quindi non c'è un «sopra» che si possa dire
    // a colpo sicuro: le quattro frecce scorrono l'elenco, avanti e indietro.
    const passo =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (passo === 0) return;
    e.preventDefault();
    // Il modulo con l'addizione: in JavaScript `-1 % 3` fa `-1`, non `2`.
    const prossima = skin[(indice + passo + skin.length) % skin.length];
    if (prossima) setBase(prossima.id);
  };

  const pulito = nome.trim();
  const vuoto = pulito.length === 0;
  // Derivato a ogni carattere: è il campo che l'utente non compila e che deve
  // comunque poter prevedere.
  const id = vuoto ? "" : idDa(pulito, [...tutte.map((s) => s.id), ...occupati]);

  return (
    <Dialogo
      titolo={t("newtheme.aria")}
      classe="finestrella nuovo-tema glass-modal"
      onChiudi={onChiudi}
      onInvia={() => {
        if (vuoto) return;
        onCrea({
          id,
          nome: pulito,
          autore:
            autore.trim().length > 0 ? autore.trim() : t("newtheme.author.me"),
          descrizione: descrizione.trim(),
          base,
        });
      }}
    >
      <div className="campo-con-nota">
        <label>
          {t("newtheme.name")}
          <input
            className="campo field-input"
            value={nome}
            spellCheck={false}
            placeholder={t("newtheme.name.hint")}
            onChange={(e) => setNome(e.target.value)}
          />
        </label>
        {/* Fuori dalla `label` di proposito: dentro finirebbe nel nome
            accessibile del campo, e chi non vede il modulo si sentirebbe
            leggere «Nome l'identificatore viene dal nome». */}
        <p className="nota-id">
          {vuoto ? t("newtheme.id.fromName") : t("newtheme.id", { id })}
        </p>
      </div>

      <label>
        {t("newtheme.author")}
        <input
          className="campo field-input"
          value={autore}
          spellCheck={false}
          placeholder={t("newtheme.author.me")}
          onChange={(e) => setAutore(e.target.value)}
        />
      </label>

      <label>
        {t("newtheme.description")}{" "}
        <span className="facoltativo">
          {t("newtheme.description.optional")}
        </span>
        <input
          className="campo field-input"
          value={descrizione}
          placeholder={t("newtheme.description.hint")}
          onChange={(e) => setDescrizione(e.target.value)}
        />
      </label>

      <div className="scelta-base">
        <span className="etichetta-gruppo" id="da-quale-skin">
          {t("newtheme.basedOn")}
        </span>
        {/* `radiogroup` e non un elenco di bottoni, per la stessa ragione del
            segmentato: sono opzioni che si escludono, e il tabulatore deve
            attraversare il gruppo in una fermata sola invece che in una per
            skin installata. */}
        <div className="basi-tema" role="radiogroup" aria-labelledby="da-quale-skin">
          {skin.map((s, indice) => {
            const scelta = s.id === base;
            return (
              <button
                key={s.id}
                type="button"
                className="base"
                role="radio"
                aria-checked={scelta}
                data-scelta={scelta || undefined}
                tabIndex={scelta ? 0 : -1}
                onClick={() => setBase(s.id)}
                onKeyDown={(e) => daTastiera(e, indice)}
              >
                <span className="bande" aria-hidden="true">
                  {bande(s).map((colore, i) => (
                    <span key={i} style={{ background: colore }} />
                  ))}
                </span>
                <span className="nome-skin">{s.nome}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={onChiudi}>
          {t("common.cancel")}
        </button>
        <button type="submit" className="bottone primario" disabled={vuoto}>
          {t("newtheme.create")}
        </button>
      </div>
    </Dialogo>
  );
}
