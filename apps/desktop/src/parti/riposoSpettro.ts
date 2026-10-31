/**
 * La macchina del riposo della scena dello spettro: quando la presa nel motore
 * si apre e si chiude, quando il ciclo si ferma e quando riparte.
 *
 * # Perché un modulo a parte
 *
 * Perché è puro e si prova con `node --test` (`test/riposoSpettro.test.ts`):
 * `Spettro3D` porta con sé WebGL, l'IPC e le lingue. Qui ci sono solo le
 * decisioni; la scena esegue i passi che le funzioni restituiscono — il
 * rubinetto, il ciclo da fermare, il risveglio — e il resto (i manici, la
 * coda, la quarantena del controllore) resta suo.
 *
 * # La finestra nascosta è un fatto a parte dallo stato
 *
 * Stava tutto in `stato`: «nascosto» era lo stato, e un contesto perso a
 * finestra minimizzata lo sovrascriveva con «perso». Da lì la scena non sapeva
 * più di essere nascosta. Tornato il contesto con la finestra ancora giù, si
 * svegliava e disegnava per nessuno; riaperta la finestra, `mostra` trovava
 * uno stato che non era «nascosto» e usciva senza riaprire la presa — e la
 * presa l'aveva chiusa `nascondi`. Niente eventi, niente silenzio che cresce:
 * il ciclo girava a sessanta hertz su barre ferme con la musica che suonava,
 * il difetto peggiore che `Spettro3D` dichiara di voler impedire. Adesso la
 * visibilità sta in `nascosta`, che `nascondi` e `mostra` scrivono qualunque
 * sia lo stato, e il contesto ritrovato torna «nascosto» se la finestra è giù.
 *
 * # Il contesto che non torna
 *
 * Se `webglcontextrestored` non arriva — la WebView blocca il WebGL dopo
 * troppe perdite — o arriva e il contesto non si ricrea, la scena resterebbe
 * vuota per tutta la vita della schermata con la presa aperta: trenta
 * trasformate al secondo per niente, e nessuna fascia che lo dica. `scaduto`
 * chiude la presa e chiede di segnalare il guasto; la scena lo lancia nel suo
 * recinto, che mette «Riprova» al posto della tela.
 *
 * L'attesa del ritorno corre solo a finestra **visibile** (`attesa`): a
 * finestra nascosta la presa è già chiusa, e un driver che si aggiorna mentre
 * la finestra è minimizzata può metterci quanto vuole — scadere lì vorrebbe
 * dire trovare «Riprova» al ritorno invece della scena ricostruita da sé.
 */

/** Dove sta la scena. Vedi «La macchina del riposo» in `Spettro3D.tsx`. */
export type StatoScena = "vivo" | "assopito" | "nascosto" | "perso";

/** Quel che la macchina ricorda. */
export interface Riposo {
  stato: StatoScena;
  /** La finestra non si vede: minimizzata, in secondo piano, scorsa via. */
  nascosta: boolean;
}

/** Il passo da eseguire: lo stato nuovo, e le tre cose da fare. */
export interface Passo extends Riposo {
  /** Aprire (`true`) o chiudere (`false`) la presa; `null` la lascia com'è. */
  rubinetto: boolean | null;
  /** Fermare il ciclo e versare la coda negli anelli. */
  ferma: boolean;
  /** Chiamare il risveglio, che esce da sé su «perso» e «nascosto». */
  risveglia: boolean;
  /** Il contesto non è tornato: la scena lo segnala al suo recinto. */
  guasto: boolean;
  /**
   * L'attesa del ritorno del contesto: da armare, da togliere, o com'è
   * (`null`). Vedi «Il contesto che non torna».
   */
  attesa: "arma" | "togli" | null;
}

function passo(r: Riposo, cambia: Partial<Passo> = {}): Passo {
  return {
    ...r,
    rubinetto: null,
    ferma: false,
    risveglia: false,
    guasto: false,
    attesa: null,
    ...cambia,
  };
}

/**
 * La finestra non si vede più.
 *
 * Da «perso» si chiude la presa e lo stato resta «perso»: il ciclo è già
 * fermo, e il ripristino deciderà dove tornare guardando `nascosta`.
 */
export function nascondi(r: Riposo): Passo {
  const giu = { ...r, nascosta: true };
  if (r.stato === "nascosto") return passo(giu);
  if (r.stato === "perso") return passo(giu, { rubinetto: false, attesa: "togli" });
  return passo({ ...giu, stato: "nascosto" }, { ferma: true, rubinetto: false });
}

/**
 * La finestra si vede di nuovo.
 *
 * Da «nascosto» si riapre la presa e si risveglia, come sempre. Da «perso» si
 * riapre la presa e basta: a contesto perso e finestra visibile la presa sta
 * aperta apposta — sono gli eventi a tenere piena la memoria che il ripristino
 * ricaricherà. Da «vivo» e «assopito» niente: una scena assopita in pausa ha
 * la presa chiusa da `addormenta`, e riaprirla qui vorrebbe dire non
 * richiuderla più — chi l'ha chiusa la riapre, cioè la ripresa della musica.
 */
export function mostra(r: Riposo): Passo {
  const su = { ...r, nascosta: false };
  if (r.stato === "nascosto") {
    return passo({ ...su, stato: "assopito" }, { rubinetto: true, risveglia: true });
  }
  if (r.stato === "perso") {
    // L'attesa riparte solo se la finestra era davvero nascosta: un `mostra`
    // ripetuto a finestra già visibile non deve allungarla.
    return passo(su, { rubinetto: true, attesa: r.nascosta ? "arma" : null });
  }
  return passo(su);
}

/**
 * Il contesto se n'è andato: la presa resta com'è, il resto lo fa la scena.
 * A finestra visibile comincia l'attesa del ritorno; a finestra nascosta no,
 * comincerà quando la si mostra.
 */
export function perso(r: Riposo): Passo {
  return passo({ ...r, stato: "perso" }, { attesa: r.nascosta ? null : "arma" });
}

/**
 * Il contesto è tornato, e le risorse si sono ricreate.
 *
 * A finestra nascosta si torna «nascosto», e niente risveglio: lo darà
 * `mostra`. A finestra visibile si torna «assopito», si riapre la presa e si
 * risveglia; in pausa la regola del silenzio la richiuderà da sé.
 */
export function ritrovato(r: Riposo): Passo {
  if (r.nascosta) return passo({ ...r, stato: "nascosto" }, { attesa: "togli" });
  return passo(
    { ...r, stato: "assopito" },
    { rubinetto: true, risveglia: true, attesa: "togli" },
  );
}

/**
 * Il contesto non è tornato in tempo, o è tornato e non si è ricreato: la
 * presa si chiude e il guasto si segnala.
 */
export function scaduto(r: Riposo): Passo {
  return passo({ ...r, stato: "perso" }, { rubinetto: false, guasto: true });
}

/**
 * Un giro del ciclo di disegno: disegnare, riarmare il prossimo fotogramma,
 * addormentarsi.
 *
 * Addormentandosi **si disegna** l'ultimo fotogramma e non si riarma. Prima ci
 * si addormentava senza disegnare, e andava bene finché la tela teneva
 * l'ultimo fotogramma; ma un ridimensionamento a scena addormentata svuota il
 * buffer e sveglia il ciclo, e quel primo giro rientrava nel sonno prima di
 * disegnare: la scena restava vuota, senza nemmeno la griglia di base che
 * dice «c'è, e adesso è a zero», finché la musica non ripartiva.
 *
 * Senza una misura valida non si disegna e non si riarma: la sveglia la darà
 * il ridimensionamento che porta la misura.
 */
export function passoCiclo(s: {
  silenzio: number;
  copertura: number;
  misuraValida: boolean;
}): { disegna: boolean; riarma: boolean; addormenta: boolean } {
  const dorme = s.silenzio > s.copertura;
  if (!s.misuraValida) return { disegna: false, riarma: false, addormenta: dorme };
  return { disegna: true, riarma: !dorme, addormenta: dorme };
}
