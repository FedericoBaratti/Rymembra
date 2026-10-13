/**
 * CodeMirror dentro lo Studio: il testo è dello Studio, l'editor lo mostra.
 *
 * La sorgente di verità resta la storia dello Studio. L'editor riceve il testo
 * e lo rimanda a ogni battuta; quando il testo cambia da fuori — un cursore,
 * un annullo, la chat — lo si riscrive dentro l'editor **solo nel tratto che
 * cambia**, con un'annotazione che dice «viene da fuori»: il cursore resta
 * dov'era, lo scorrimento pure, e la modifica non torna indietro come se
 * l'avesse battuta chi scrive.
 */
import { setDiagnostics } from "@codemirror/lint";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { useEffect, useMemo, useRef } from "react";

import { daFuori, diagnostiche, estensioni, type Fonti, type Segno } from "./codice";
import { useStudio } from "./contesto";
import { testo as testoDi } from "./negozio";
import { nomeRisorsa } from "./usaPacchetto";
import { descrizioneParte, descrizioneToken } from "./vocabolario";

/**
 * Quel che l'editor sa dello Studio, per il completamento.
 *
 * Il documento è l'ultimo che si è riusciti a leggere: mentre si scrive il
 * testo passa quasi sempre per stati che non sono JSON, e proprio allora il
 * completamento serve — i nomi della tavolozza e delle animazioni restano
 * quelli dell'ultima lettura buona.
 */
export function useFonti(radice: readonly string[]): Fonti {
  const { registro, documento, file } = useStudio();
  const ultimo = useRef(documento);
  if (documento !== null) ultimo.current = documento;
  const nomi = useRef<readonly string[]>([]);
  nomi.current = file.filter((v) => v.genere === "risorsa").map(nomeRisorsa);
  const chiave = radice.join(".");
  return useMemo(
    () => ({
      registro,
      documento: () => ultimo.current,
      file: () => nomi.current,
      radice: chiave === "" ? [] : chiave.split("."),
      descrizioneParte,
      descrizioneToken,
    }),
    [registro, chiave],
  );
}

/** I problemi della validazione di adesso, come segni sul testo. */
export function useSegni(): readonly Segno[] {
  const { s } = useStudio();
  const esito = s.esito;
  const attuale = s.validato === testoDi(s);
  return useMemo(() => {
    if (esito === null || !attuale) return [];
    return [
      ...esito.errori.flatMap((p) =>
        p.riga === null ? [] : [{ riga: p.riga, colonna: p.colonna ?? 1, messaggio: p.message, gravita: "error" as const }],
      ),
      ...esito.avvisi.flatMap((a) =>
        a.riga === null ? [] : [{ riga: a.riga, colonna: a.colonna ?? 1, messaggio: a.message, gravita: "warning" as const }],
      ),
    ];
  }, [esito, attuale]);
}

export function EditorCodice({
  testo,
  onCambia,
  onAnnulla,
  onRipeti,
  etichetta,
  fonti,
  segni = [],
  riga = null,
  onRigaMostrata,
  classe,
}: {
  testo: string;
  onCambia: (testo: string) => void;
  onAnnulla: () => void;
  onRipeti: () => void;
  etichetta: string;
  fonti: Fonti;
  /** I problemi del nucleo, per riga e colonna: solo per il documento intero. */
  segni?: readonly Segno[];
  /** Una riga da mostrare, con un contatore che la rende nuova ogni volta. */
  riga?: { riga: number; colonna: number; volta: number } | null;
  /**
   * La riga è stata mostrata: chi l'ha chiesta la può dimenticare.
   *
   * Senza, la richiesta restava nel negozio, e ogni volta che l'editor
   * rinasceva — tornando alla scheda del codice, tornando visibile dopo la
   * prova sull'app — riportava il cursore alla riga di un problema corretto
   * dieci minuti prima, rubando il fuoco a chi stava scrivendo altrove.
   */
  onRigaMostrata?: (volta: number) => void;
  classe?: string;
}) {
  const dove = useRef<HTMLDivElement>(null);
  const vista = useRef<EditorView | null>(null);
  const ultime = useRef({ onCambia, onAnnulla, onRipeti, fonti, onRigaMostrata });
  ultime.current = { onCambia, onAnnulla, onRipeti, fonti, onRigaMostrata };

  // L'editor nasce una volta: le funzioni cambiano, il riferimento no.
  useEffect(() => {
    const nodo = dove.current;
    if (nodo === null) return;
    const v = new EditorView({
      parent: nodo,
      state: EditorState.create({
        doc: testo,
        extensions: [
          ...estensioni({
            fonti: () => ultime.current.fonti,
            onAnnulla: () => ultime.current.onAnnulla(),
            onRipeti: () => ultime.current.onRipeti(),
            etichetta,
          }),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return;
            if (u.transactions.some((tr) => tr.annotation(daFuori) === true)) return;
            ultime.current.onCambia(u.state.doc.toString());
          }),
        ],
      }),
    });
    vista.current = v;
    return () => {
      v.destroy();
      vista.current = null;
    };
    // Il testo iniziale conta solo alla nascita; dopo lo porta l'effetto sotto.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [etichetta]);

  // Il testo che arriva da fuori, riscritto solo dove cambia.
  useEffect(() => {
    const v = vista.current;
    if (v === null) return;
    const ora = v.state.doc.toString();
    if (ora === testo) return;
    let inizio = 0;
    const minimo = Math.min(ora.length, testo.length);
    while (inizio < minimo && ora.charCodeAt(inizio) === testo.charCodeAt(inizio)) inizio += 1;
    let coda = 0;
    while (
      coda < minimo - inizio &&
      ora.charCodeAt(ora.length - 1 - coda) === testo.charCodeAt(testo.length - 1 - coda)
    )
      coda += 1;
    v.dispatch({
      changes: { from: inizio, to: ora.length - coda, insert: testo.slice(inizio, testo.length - coda) },
      annotations: daFuori.of(true),
    });
  }, [testo]);

  // I segni del nucleo: arrivano con la validazione, e valgono per quel testo.
  useEffect(() => {
    const v = vista.current;
    if (v === null) return;
    v.dispatch(setDiagnostics(v.state, diagnostiche(v.state, segni)));
  }, [segni]);

  // «Vai alla riga», una volta per richiesta: poi la si restituisce.
  useEffect(() => {
    const v = vista.current;
    if (v === null || riga === null) return;
    ultime.current.onRigaMostrata?.(riga.volta);
    if (riga.riga < 1 || riga.riga > v.state.doc.lines) return;
    const linea = v.state.doc.line(riga.riga);
    const pos = Math.min(linea.to, linea.from + Math.max(0, riga.colonna - 1));
    v.dispatch({
      selection: EditorSelection.cursor(pos),
      effects: EditorView.scrollIntoView(pos, { y: "center" }),
    });
    v.focus();
  }, [riga]);

  return <div ref={dove} className={classe === undefined ? "editor-codice" : `editor-codice ${classe}`} />;
}
