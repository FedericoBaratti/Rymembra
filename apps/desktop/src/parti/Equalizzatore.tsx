/**
 * L'equalizzatore, in due taglie.
 *
 * # Perché un componente e non due
 *
 * Perché è lo stesso comando in due contesti, non due comandi che si
 * somigliano: il pannello che si apre dalla barra e la scheda in Impostazioni
 * muovono gli stessi dieci cursori e chiamano lo stesso `ipc.equalizzatore`.
 * Due copie divergerebbero alla prima modifica — è successo con il trasporto,
 * che aveva tre copie ed è per questo che oggi ha una `taglia`.
 *
 * `pannello` è compatto: interruttore, tendina dei preset, dieci cursori.
 * `pagina` aggiunge le etichette delle frequenze, i decibel letti a fianco, il
 * salvataggio delle curve e le righe che spiegano cosa fa.
 *
 * # Perché lo stato è anche qui, e non solo nel nucleo
 *
 * È l'eccezione al «non tenere niente che il nucleo già conosca», ed è la
 * stessa che vale per il cursore del tempo in `Scrubber`: mentre un cursore è
 * sotto il dito, l'unico che sa dove sta è la finestra. Aspettare che il valore
 * torni indietro dal nucleo vorrebbe dire disegnare il cursore un giro dopo il
 * dito, cioè un cursore che si muove a scatti.
 *
 * Il valore che torna si riconosce come **eco** e si ignora; quello che arriva
 * da un'altra parte — l'altra taglia, un preset, l'avvio — si adotta.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { CENTRI_EQ, LIMITE_EQ_DB, ipc, type VocePreset } from "../ipc";
import { Icona } from "./Icone";
import { t } from "../lingue";

/** Dove sta l'equalizzatore, e quindi quanto è grande. */
export type TagliaEq = "pannello" | "pagina";

/**
 * Ogni quanti millisecondi la curva può partire verso il nucleo.
 *
 * Un cursore trascinato produce un evento per pixel: senza questo freno
 * sarebbero un centinaio di chiamate IPC e altrettante scritture su SQLite al
 * secondo per un gesto che dura due secondi. A sessanta millisecondi il ritardo
 * non si sente — il motore ci mette comunque una quarantina di millisecondi a
 * percorrere la rampa fra due curve — e le chiamate diventano una quindicina.
 *
 * La prima parte **subito**: è l'ultima che si fa aspettare. Chi tocca un
 * cursore deve sentire il cambiamento al primo pixel, non al quarto.
 */
const PASSO_INVIO = 60;

/** Il passo di un cursore, in decibel. */
const PASSO_DB = 0.5;

/** Come si legge il centro di una banda. */
function etichettaBanda(hz: number): string {
  return hz >= 1000 ? `${hz / 1000} kHz` : `${Math.round(hz)} Hz`;
}

/** Una curva lunga quanto deve, tagliata dove deve. */
function allunga(guadagni: readonly number[]): number[] {
  return CENTRI_EQ.map((_, banda) => {
    const valore = guadagni[banda] ?? 0;
    return Number.isFinite(valore)
      ? Math.min(Math.max(valore, -LIMITE_EQ_DB), LIMITE_EQ_DB)
      : 0;
  });
}

/**
 * Due curve sono la stessa cosa.
 *
 * Con una tolleranza e non con `===`: il valore torna dal nucleo dopo essere
 * passato per un `f32` e per JSON, e un confronto esatto farebbe scambiare per
 * «curva nuova» l'eco della propria.
 */
function stessaCurva(a: readonly number[], b: readonly number[]): boolean {
  return CENTRI_EQ.every((_, i) => Math.abs((a[i] ?? 0) - (b[i] ?? 0)) < 0.01);
}

export function Equalizzatore({
  attivo,
  guadagni,
  taglia,
  onErrore,
}: {
  /** I filtri sono accesi, secondo il nucleo. */
  attivo: boolean;
  /** La curva, secondo il nucleo. */
  guadagni: number[];
  taglia: TagliaEq;
  onErrore: (e: unknown) => void;
}) {
  const [curva, setCurva] = useState(() => allunga(guadagni));
  const [acceso, setAcceso] = useState(attivo);
  const [preset, setPreset] = useState<VocePreset[]>([]);
  const [nomeNuovo, setNomeNuovo] = useState("");

  // Le curve mandate di cui non è ancora tornata l'eco.
  //
  // Una **coda** e non l'ultimo valore, ed è la differenza fra un cursore che
  // segue il dito e uno che rimbalza: mentre si trascina ne partono parecchie
  // di fila, e la prima torna indietro quando la terza è già stata mandata.
  // Confrontando solo con l'ultima, quel ritorno sembrerebbe un cambiamento
  // arrivato da fuori e riporterebbe il cursore dov'era due giri fa.
  const attesiDiRitorno = useRef<{ curva: number[]; acceso: boolean }[]>([]);
  const inSospeso = useRef<{ curva: number[]; acceso: boolean } | null>(null);
  const ultimoInvio = useRef(0);
  const attesa = useRef(0);

  useEffect(
    () => () => {
      if (attesa.current !== 0) window.clearTimeout(attesa.current);
    },
    [],
  );

  const spedisci = useCallback(
    (prossima: { curva: number[]; acceso: boolean }) => {
      ultimoInvio.current = performance.now();
      // Si annota **qui** e non a ogni movimento: a tornare indietro sono le
      // curve mandate, non quelle disegnate, e fra le une e le altre c'è il
      // freno di `PASSO_INVIO`.
      attesiDiRitorno.current.push(prossima);
      // Un tetto, per il caso in cui le eco non arrivino mai — un motore morto,
      // un comando rifiutato: la coda non deve crescere per tutta la sessione.
      if (attesiDiRitorno.current.length > 8) attesiDiRitorno.current.shift();
      ipc.equalizzatore(prossima.curva, prossima.acceso).catch(onErrore);
    },
    [onErrore],
  );

  /** Disegna subito, e manda al nucleo al più ogni `PASSO_INVIO`. */
  const applica = useCallback(
    (prossimaCurva: number[], prossimoAcceso: boolean) => {
      setCurva(prossimaCurva);
      setAcceso(prossimoAcceso);
      const prossima = { curva: prossimaCurva, acceso: prossimoAcceso };

      const trascorso = performance.now() - ultimoInvio.current;
      if (trascorso >= PASSO_INVIO) {
        spedisci(prossima);
        return;
      }
      // Troppo presto: si tiene solo l'ultima, e parte allo scadere.
      inSospeso.current = prossima;
      if (attesa.current !== 0) return;
      attesa.current = window.setTimeout(() => {
        attesa.current = 0;
        const ultima = inSospeso.current;
        inSospeso.current = null;
        if (ultima) spedisci(ultima);
      }, PASSO_INVIO - trascorso);
    },
    [spedisci],
  );

  // Quel che arriva dal nucleo: si adotta solo se non è l'eco di una che
  // abbiamo mandato noi. L'altra taglia, un preset scelto altrove e la curva
  // riletta all'avvio passano tutti di qui.
  useEffect(() => {
    const arrivata = allunga(guadagni);
    const quale = attesiDiRitorno.current.findIndex(
      (attesa) => attesa.acceso === attivo && stessaCurva(attesa.curva, arrivata),
    );
    if (quale >= 0) {
      // È la nostra. Si scarta insieme a quelle che l'hanno preceduta: se la
      // terza è tornata, la prima e la seconda non torneranno più.
      attesiDiRitorno.current.splice(0, quale + 1);
      return;
    }
    attesiDiRitorno.current = [];
    setCurva(arrivata);
    setAcceso(attivo);
  }, [guadagni, attivo]);

  const caricaPreset = useCallback(() => {
    ipc
      .eqPresetElenco()
      .then(setPreset)
      .catch((e: unknown) => onErrore(e));
  }, [onErrore]);

  useEffect(caricaPreset, [caricaPreset]);

  const muoviBanda = (banda: number, db: number) => {
    const prossima = curva.map((valore, i) => (i === banda ? db : valore));
    // Muovere un cursore accende: chi trascina sta chiedendo di sentire una
    // differenza, e lasciare i filtri spenti gli farebbe credere che il comando
    // sia rotto. È la stessa regola con cui il volume toglie il silenziamento.
    applica(prossima, true);
  };

  const azzera = () => applica(CENTRI_EQ.map(() => 0), acceso);

  const scegliPreset = (nome: string) => {
    const scelto = preset.find((p) => p.nome === nome);
    if (!scelto) return;
    applica(allunga(scelto.guadagni), true);
  };

  const salva = () => {
    const nome = nomeNuovo.trim();
    if (nome === "") return;
    ipc
      .eqPresetSalva(nome)
      .then((fatto) => {
        if (fatto) setNomeNuovo("");
        caricaPreset();
      })
      .catch((e: unknown) => onErrore(e));
  };

  const cancella = (nome: string) => {
    ipc
      .eqPresetCancella(nome)
      .then(caricaPreset)
      .catch((e: unknown) => onErrore(e));
  };

  // Quale preset corrisponde a quel che si sta ascoltando. Serve solo alla
  // tendina: un elenco in cui non è mai selezionato niente non dice a chi
  // guarda da dove viene la curva che sente.
  const attuale = preset.find((p) => stessaCurva(p.guadagni, curva));
  const miei = preset.filter((p) => !p.diSerie);
  const conNomi = taglia === "pagina";

  return (
    <div className="equalizzatore" data-taglia={taglia} data-spento={!acceso || undefined}>
      <div className="eq-testa">
        <button
          type="button"
          className="interruttore switch"
          role="switch"
          aria-checked={acceso}
          aria-label={t("player.eq")}
          onClick={() => applica(curva, !acceso)}
        >
          <span className="pista switch-track" aria-hidden="true">
            <span className="pallina" />
          </span>
        </button>
        <select
          className="eq-preset"
          aria-label={t("eq.curve")}
          value={attuale?.nome ?? ""}
          onChange={(e) => scegliPreset(e.target.value)}
        >
          {/* La voce vuota esiste solo finché la curva non corrisponde a
              nessuna dell'elenco: senza, il campo mostrerebbe il primo preset
              come se fosse quello in ascolto. */}
          {attuale === undefined && (
            <option value="">{t("eq.curve.own")}</option>
          )}
          {preset.map((p) => (
            <option key={`${p.diSerie ? "s" : "u"}:${p.nome}`} value={p.nome}>
              {p.nome}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          onClick={azzera}
          title={t("eq.reset.title")}
        >
          {t("eq.reset")}
        </button>
      </div>

      <div className="eq-bande eq-bars">
        {CENTRI_EQ.map((hz, banda) => {
          const db = curva[banda] ?? 0;
          return (
            <label key={hz} className="eq-banda">
              {conNomi && <span className="eq-db stat-number">{db > 0 ? `+${db}` : db}</span>}
              <input
                type="range"
                className="eq-cursore eq-slider"
                min={-LIMITE_EQ_DB}
                max={LIMITE_EQ_DB}
                step={PASSO_DB}
                value={db}
                aria-label={t("eq.band.aria", {
                  banda: etichettaBanda(hz),
                  db,
                })}
                onChange={(e) => muoviBanda(banda, Number(e.target.value))}
                /* Il doppio clic riporta a zero la banda: è la scorciatoia che
                   ogni equalizzatore ha, e senza di lei ricentrare un cursore
                   richiede di trovare lo zero a mano. */
                onDoubleClick={() => muoviBanda(banda, 0)}
              />
              <span className="eq-hz">{etichettaBanda(hz)}</span>
            </label>
          );
        })}
      </div>

      {conNomi && (
        <div className="eq-salva">
          <input
            type="text"
            className="campo field-input"
            placeholder={t("eq.save.name")}
            value={nomeNuovo}
            maxLength={40}
            onChange={(e) => setNomeNuovo(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") salva();
            }}
          />
          <button
            type="button"
            className="bottone btn-ghost"
            disabled={nomeNuovo.trim() === ""}
            onClick={salva}
          >
            <Icona nome="i-plus" dim={15} />
            {t("eq.save")}
          </button>
          {miei.length > 0 && (
            <ul className="eq-mie">
              {miei.map((p) => (
                <li key={p.nome}>
                  <span>{p.nome}</span>
                  <button
                    type="button"
                    className="tasto icon-btn"
                    aria-label={t("eq.delete", { nome: p.nome })}
                    title={t("eq.delete.title")}
                    onClick={() => cancella(p.nome)}
                  >
                    <Icona nome="i-x" dim={13} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

/** Il pannello che si apre dalla barra del lettore. */
export function PannelloEq({
  attivo,
  guadagni,
  onChiudi,
  onErrore,
}: {
  attivo: boolean;
  guadagni: number[];
  onChiudi: () => void;
  onErrore: (e: unknown) => void;
}) {
  useEffect(() => {
    const suTasto = (e: KeyboardEvent) => {
      if (e.key === "Escape") onChiudi();
    };
    window.addEventListener("keydown", suTasto);
    return () => window.removeEventListener("keydown", suTasto);
  }, [onChiudi]);

  return (
    <>
      {/* Lo stesso velo del menù contestuale: prende il clic di chiusura, così
          il pannello si chiude cliccando ovunque e non solo su un tasto. */}
      <div className="velo" onClick={onChiudi} />
      <div
        className="pannello-eq menu-pop"
        role="dialog"
        aria-label={t("player.eq")}
      >
        <Equalizzatore
          attivo={attivo}
          guadagni={guadagni}
          taglia="pannello"
          onErrore={onErrore}
        />
      </div>
    </>
  );
}
