/**
 * Studio · il movimento: l'intensità, le curve, la rotta, le animazioni.
 *
 * # Perché è un pannello suo
 *
 * Perché il movimento era **quattro cose sparse in tre posti che non lo
 * riguardavano**. L'intensità stava in mezzo alle scelte d'impaginazione,
 * accanto a «lettore flottante» e «barra stretta», come se «quanto si muove»
 * fosse una questione di dove stanno le cose. Le curve nominate e la
 * transizione di rotta il formato le accettava da sempre e lo Studio non le
 * mostrava affatto: si scrivevano nel JSON o non si scrivevano. Le animazioni
 * nominate sono arrivate col pacchetto che ha scritto
 * `core/aether-skin/src/movimento.rs`, e sarebbero nate senza un posto in cui
 * guardarle.
 *
 * Le quattro cose sono una cosa sola: un'intensità che scala **tutte** le
 * durate, delle curve che le animazioni richiamano, una transizione fra
 * schermate e delle animazioni sulle parti. Metterle sotto lo stesso titolo non
 * è ordine, è la lettura giusta — chi gira l'intensità a `none` deve vedere
 * nella stessa colonna quel che sta spegnendo.
 *
 * # Il costo si legge qui, il verdetto viene dal nucleo
 *
 * Il numero accanto a ogni parte animata lo calcola questo file, con la stessa
 * formula di [`animation_cost`] — quattro, cioè `CostClass::Composited`, più una
 * per ogni ripetizione oltre la prima. **Chi dice che si sfora resta il
 * nucleo**: l'avviso `costBudget` su `parts.<nome>.animations` arriva già
 * validato in `esito.avvisi`, e questo pannello lo mostra invece di ridecidere.
 * È la stessa divisione del pannello delle superfici — là il costo di un
 * livello viene dal registro e il budget dal validatore — e serve a che due
 * numeri non possano contraddirsi: quello che si legge e quello che blocca.
 *
 * # I tetti vengono dal registro, e per questo il pannello aspetta
 *
 * `movimento.rs` li dichiara tutti — `MAX_ANIMAZIONI`, `MIN_FOTOGRAMMI`,
 * `MAX_FOTOGRAMMI`, `MAX_ITERAZIONI`, `MOTION_COST_BUDGET`, i versi, i trigger
 * — e `studio_registro` li porta di qua come porta `budget`, `shellBudget` e
 * `contrastoMinimo`. Sono gli stessi numeri con cui il validatore rifiuta: un
 * pannello che ne avesse una copia offrirebbe, il giorno in cui il crate
 * cambia idea, valori che il documento respinge — cioè un errore suggerito
 * dall'interfaccia.
 *
 * La conseguenza è la riga di guardia in cima al componente: **finché il
 * registro non è arrivato, qui non c'è niente da disegnare.** Non un valore di
 * ripiego pari alla costante — sarebbe la seconda verità rimessa altrove, e
 * per giunta in un posto dove nessuno la cercherebbe — e non una manopola
 * senza estremi, che accetterebbe quel che poi diventa rosso. `studio_registro`
 * è un comando statico, compilato dentro il binario e chiesto una volta
 * all'apertura dello Studio: risponde prima che il documento sia validato, e
 * il fotogramma senza questo pannello non lo vede nessuno.
 */
import type { Registro, Validazione } from "../ipc";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import {
  Cursore,
  Riga,
  Scheda,
  ValoreCurva,
  ValoreDurata,
} from "./controlli";
import { leggi, scriviIn, togliDa } from "./patch";
import { coloreCosto } from "./valori";
import { t } from "../lingue";

/** Le quattro intensità, come le scrive `MotionIntensity::as_str`. */
const INTENSITA = ["full", "essential", "none", "maximum"] as const;

/** Un'animazione come sta nel documento, letta senza fidarsi. */
type Animazione = Record<string, unknown>;

/** Un fotogramma come sta nel documento. */
type Fotogramma = Record<string, unknown>;

/** L'oggetto in un percorso, o `{}`. Non indovina: se non è un oggetto, è vuoto. */
function oggetto(cosa: unknown): Record<string, unknown> {
  return cosa !== null && typeof cosa === "object" && !Array.isArray(cosa)
    ? (cosa as Record<string, unknown>)
    : {};
}

/** I fotogrammi di un'animazione, o l'elenco vuoto. */
function fotogrammiDi(animazione: Animazione): Fotogramma[] {
  const scritti = animazione["frames"];
  return Array.isArray(scritti) ? scritti.map(oggetto) : [];
}

/**
 * Quanto costa un'animazione: `animation_cost`, riscritta.
 *
 * I fotogrammi non contano e non è una svista: il motore interpola fra due
 * fermate esattamente come fra sei. Il commento sta anche di là, ed è la stessa
 * frase perché è la stessa scelta.
 *
 * `peso` è il peso di `CostClass::Composited`, e arriva dal registro insieme ai
 * tetti: la formula è di qui, il numero da cui parte no.
 */
function costoAnimazione(animazione: Animazione, peso: number): number {
  const quante = Number(animazione["iterations"] ?? 1);
  const ripetizioni = Number.isFinite(quante) ? Math.max(1, quante) : 1;
  return peso + (ripetizioni - 1);
}

/** Un fotogramma senza i campi che nessuno ha dichiarato. */
function ripulito(fotogramma: Fotogramma): Fotogramma {
  return Object.fromEntries(
    Object.entries(fotogramma).filter(
      ([, valore]) => valore !== null && valore !== undefined,
    ),
  );
}

export function Movimento({
  sorgente,
  onSorgente,
  esito,
  registro,
}: {
  sorgente: string;
  onSorgente: (testo: string) => void;
  esito: Validazione | null;
  registro: Registro | null;
}) {
  // Vedi il preambolo: tutto quel che questo pannello disegna ha un estremo, e
  // gli estremi stanno nel registro. Senza, l'unica cosa onesta è non esserci.
  if (registro === null) return null;

  const documento = leggi(sorgente);
  const movimento = oggetto(documento?.["motion"]);
  const curve = oggetto(movimento["easings"]);
  const animazioni = oggetto(movimento["animations"]);
  const rotta = oggetto(movimento["routeTransition"]);
  const parti = oggetto(documento?.["parts"]);

  const nomiCurve = Object.keys(curve);
  const nomiAnimazioni = Object.keys(animazioni);

  /**
   * Chi richiama ogni animazione: nome → quante assegnazioni.
   *
   * Serve a due cose in un giro solo — il «mai usata» accanto al nome, e il
   * costo per parte del pannello del budget — e farlo due volte vorrebbe dire
   * due letture dell'albero delle parti che possono divergere.
   */
  const richiami = new Map<string, number>();
  /** Le parti animate, col loro costo. In ordine di nome, come il documento. */
  const animate: { parte: string; costo: number; quante: number }[] = [];
  for (const [parte, dichiarazione] of Object.entries(parti)) {
    const assegnate = oggetto(oggetto(dichiarazione)["animations"]);
    let costo = 0;
    let quante = 0;
    for (const trigger of registro.trigger) {
      const nome = assegnate[trigger];
      if (typeof nome !== "string") continue;
      richiami.set(nome, (richiami.get(nome) ?? 0) + 1);
      quante += 1;
      const definizione = animazioni[nome];
      if (definizione !== undefined) {
        costo += costoAnimazione(oggetto(definizione), registro.pesoComposito);
      }
    }
    if (quante > 0) animate.push({ parte, costo, quante });
  }

  /** Gli avvisi che il nucleo ha già emesso sul movimento di una parte. */
  const sforate = new Set(
    (esito?.avvisi ?? [])
      .filter((a) => a.kind === "costBudget" && a.path.endsWith(".animations"))
      .map((a) => a.path.split(".").slice(1, -1).join(".")),
  );

  const scriviMovimento = (percorso: readonly string[], valore: unknown) => {
    onSorgente(
      valore === null
        ? togliDa(sorgente, ["motion", ...percorso])
        : scriviIn(sorgente, ["motion", ...percorso], valore),
    );
  };

  /** Un nome libero, con la stessa meccanica della tavolozza dei colori. */
  const nomeLibero = (
    presi: Readonly<Record<string, unknown>>,
    base: string,
    separatore: string,
  ) => {
    for (let n = 1; ; n += 1) {
      const proposto = n === 1 ? base : `${base}${separatore}${n}`;
      if (presi[proposto] === undefined) return proposto;
    }
  };

  /**
   * Rinomina una curva, e con lei niente.
   *
   * Al contrario di un colore o di un motivo, una curva **non si richiama per
   * nome** da nessun campo del documento: il compilatore ne fa una variabile
   * CSS (`--skin-ease-<nome>`) e chi la vuole se la copia. Rinominarla quindi è
   * davvero solo cambiarle il nome — e vale la pena scriverlo, perché il
   * gemello di due righe più sotto fa l'opposto.
   */
  const rinominaCurva = (vecchio: string, nuovo: string) => {
    if (nuovo === "" || nuovo === vecchio || curve[nuovo] !== undefined) return;
    const riscritte = Object.fromEntries(
      Object.entries(curve).map(([chiave, valore]) => [
        chiave === vecchio ? nuovo : chiave,
        valore,
      ]),
    );
    scriviMovimento(["easings"], riscritte);
  };

  /**
   * Rinomina un'animazione, e con lei i richiami delle parti.
   *
   * Qui il nome **è** un legame: `parts.<x>.animations.<trigger>` contiene
   * quella stringa, e lasciarla indietro darebbe un richiamo a un'animazione
   * che non esiste — cioè una skin che smette di compilare per una modifica che
   * sembrava cosmetica. È la stessa ragione di `rinominaColore` e
   * `rinominaMotivo` in `Tavolozza.tsx`, e per la stessa ragione si riscrive
   * l'albero invece di cercarne le occorrenze a mano.
   */
  const rinominaAnimazione = (vecchio: string, nuovo: string) => {
    if (documento === null || nuovo === "" || nuovo === vecchio) return;
    if (animazioni[nuovo] !== undefined) return;

    const riscritte = Object.fromEntries(
      Object.entries(animazioni).map(([chiave, valore]) => [
        chiave === vecchio ? nuovo : chiave,
        valore,
      ]),
    );
    let testo = scriviIn(sorgente, ["motion", "animations"], riscritte);
    for (const [parte, dichiarazione] of Object.entries(parti)) {
      const assegnate = oggetto(oggetto(dichiarazione)["animations"]);
      for (const trigger of registro.trigger) {
        if (assegnate[trigger] !== vecchio) continue;
        testo = scriviIn(
          testo,
          ["parts", parte, "animations", trigger],
          nuovo,
        );
      }
    }
    onSorgente(testo);
  };

  /**
   * Toglie un'animazione, e con lei i richiami che la nominano.
   *
   * Togliere la sola dichiarazione lascerebbe `parts.<x>.animations.<trigger>`
   * a puntare un nome che non esiste, e quello il validatore non lo perdona:
   * «animazione inesistente». È il verso opposto della rinomina, e la stessa
   * regola — chi possiede il nome possiede anche chi lo scrive.
   */
  const togliAnimazione = (nome: string) => {
    let testo = togliDa(sorgente, ["motion", "animations", nome]);
    for (const [parte, dichiarazione] of Object.entries(parti)) {
      const assegnate = oggetto(oggetto(dichiarazione)["animations"]);
      for (const trigger of registro.trigger) {
        if (assegnate[trigger] !== nome) continue;
        testo = togliDa(testo, ["parts", parte, "animations", trigger]);
      }
    }
    onSorgente(testo);
  };

  /** Le curve della skin, per il controllo che le sa copiare. */
  const curveSkin = nomiCurve.map((nome) => ({ nome, curva: curve[nome] }));

  /**
   * Il totale, dal nucleo.
   *
   * `costoMovimento` è la somma che `compile_skin` fa già su tutte le parti
   * animate: lo stesso numero che si otterrebbe sommando le righe qui sotto, e
   * per un po' lo si è sommato davvero. Rifarlo qui vorrebbe dire una seconda
   * verità che il giorno in cui il nucleo cambia il modo di contare dice un
   * numero diverso da quello del budget che blocca — la stessa divisione già
   * scritta in cima a questo file, applicata al totale.
   *
   * Zero quando il documento non compila: senza un foglio non c'è niente in
   * movimento, ed è la stessa degradazione degli avvisi, che a metà di una
   * parentesi spariscono. Le righe qui sotto invece restano, perché si leggono
   * dal testo e servono proprio mentre lo si scrive.
   */
  const totale = esito?.costoMovimento ?? 0;

  return (
    <>
      <Scheda icona="i-play" titolo={t("studio.motion.title")}>
        <Riga che={t("studio.motion.intensity")}>
          <Segmentato
            etichetta={t("studio.motion.intensity")}
            classe="minuto denso"
            scelta={String(movimento["intensity"] ?? "full")}
            onScegli={(v) => scriviMovimento(["intensity"], v)}
            voci={INTENSITA.map((v) => ({ chiave: v, etichetta: v }))}
          />
        </Riga>
        <p className="nota">{t("studio.motion.intensity.note")}</p>
      </Scheda>

      {/*
        Le curve: dichiarate una volta, e poi copiate dove servono.
        Il formato le accettava da sempre — `sala.json` ne dichiara due — e
        l'unico modo di scriverne una era il JSON.
      */}
      <Scheda
        icona="i-eq"
        titolo={t("studio.motion.curves")}
        nota={String(nomiCurve.length)}
      >
        {nomiCurve.length === 0 && (
          <p className="niente">{t("studio.motion.curves.empty")}</p>
        )}
        <div className="elenco-curve">
          {nomiCurve.map((nome) => (
            <div key={nome} className="una-curva">
              <input
                className="nome field-input"
                type="text"
                aria-label={t("studio.palette.nameOf", { nome })}
                defaultValue={nome}
                spellCheck={false}
                // Alla conferma e non a ogni tasto: come per i colori e i
                // motivi, rinominare riscrive un blocco intero.
                onBlur={(e) => rinominaCurva(nome, e.target.value.trim())}
                onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
              />
              <ValoreCurva
                valore={curve[nome]}
                etichetta={nome}
                onCambia={(v) =>
                  scriviMovimento(
                    ["easings", nome],
                    // Una curva non dichiarata non è una curva: togliere il
                    // valore toglie la voce, invece di lasciare un nome che
                    // il compilatore scriverebbe come variabile vuota.
                    v,
                  )
                }
              />
              <button
                type="button"
                className="via icon-btn"
                aria-label={t("studio.palette.remove", { nome })}
                onClick={() => scriviMovimento(["easings", nome], null)}
              >
                <Icona nome="i-x" dim={12} />
              </button>
            </div>
          ))}
        </div>
        <button
          type="button"
          className="aggiungi-livello"
          onClick={() =>
            scriviMovimento(
              // `is_easing_name` vuole minuscole e cifre, senza trattini: il
              // nome proposto lo rispetta, così il primo documento è valido.
              ["easings", nomeLibero(curve, "curva", "")],
              { kind: "cubicBezier", points: [0.16, 1, 0.3, 1] },
            )
          }
        >
          <Icona nome="i-plus" dim={13} />
          <span>{t("studio.motion.curves.add")}</span>
        </button>
      </Scheda>

      {/*
        La transizione di rotta: come esce la schermata vecchia, come entra la
        nuova. Sono tre proprietà per lato e non di più — `RouteFrame` ha
        opacità, scala e spostamento verticale — perché quelle regole agiscono
        su un'istantanea dell'intera finestra.
      */}
      <Scheda icona="i-repeat" titolo={t("studio.motion.route")}>
        <div className="due-rotte">
          {(
            [
              ["out", t("studio.motion.route.out")],
              ["in", t("studio.motion.route.in")],
            ] as const
          ).map(([lato, etichetta]) => {
            const fermata = oggetto(rotta[lato]);
            const scriviFermata = (campo: string, valore: unknown) => {
              const nuova = ripulito({ ...fermata, [campo]: valore });
              // Un lato che non dichiara più niente si toglie invece di
              // restare come `{}`: il compilatore lo salterebbe comunque —
              // `RouteFrame::is_empty` — ma il documento porterebbe una riga
              // che non dice niente, e chi lo rilegge deve poterlo capire
              // senza conoscere quel metodo.
              scriviMovimento(
                ["routeTransition", lato],
                Object.keys(nuova).length === 0 ? null : nuova,
              );
            };
            return (
              <div key={lato} className="una-rotta">
                <span className="titolino">{etichetta}</span>
                <Riga che="opacity">
                  <Cursore
                    valore={fermata["opacity"]}
                    min={0}
                    max={1}
                    passo={0.01}
                    etichetta={`${etichetta} · opacity`}
                    onCambia={(v) => scriviFermata("opacity", v)}
                  />
                </Riga>
                <Riga che="scale">
                  <Cursore
                    valore={fermata["scale"]}
                    min={0.5}
                    max={1.5}
                    passo={0.005}
                    etichetta={`${etichetta} · scale`}
                    onCambia={(v) => scriviFermata("scale", v)}
                  />
                </Riga>
                <Riga che="translateY">
                  <Cursore
                    valore={fermata["translateY"]}
                    min={-100}
                    max={100}
                    passo={1}
                    etichetta={`${etichetta} · translateY`}
                    suffisso="px"
                    onCambia={(v) => scriviFermata("translateY", v)}
                  />
                </Riga>
              </div>
            );
          })}
        </div>
        <p className="nota">{t("studio.motion.route.note")}</p>
      </Scheda>

      {/*
        Le animazioni nominate. Il formato le tiene in cima al documento e le
        parti le richiamano per nome: qui si dichiarano, e ad assegnarle è
        l'ispettore della parte — dove sta già il selettore di stato, che è la
        stessa domanda.
      */}
      <Scheda
        icona="i-play"
        titolo={t("studio.motion.anims")}
        nota={`${nomiAnimazioni.length} / ${registro.maxAnimazioni}`}
      >
        {nomiAnimazioni.length === 0 && (
          <p className="niente">{t("studio.motion.anims.empty")}</p>
        )}
        <div className="elenco-animazioni">
          {nomiAnimazioni.map((nome) => {
            const animazione = oggetto(animazioni[nome]);
            const fotogrammi = fotogrammiDi(animazione);
            const quante = richiami.get(nome) ?? 0;
            const scriviCampo = (campo: string, valore: unknown) =>
              scriviMovimento(["animations", nome, campo], valore);
            const scriviFotogrammi = (nuovi: Fotogramma[]) =>
              scriviCampo("frames", nuovi.map(ripulito));

            return (
              <div
                key={nome}
                className="una-animazione"
                data-mai={quante === 0 || undefined}
              >
                <div className="testa-animazione">
                  <input
                    className="nome field-input"
                    type="text"
                    aria-label={t("studio.palette.nameOf", { nome })}
                    defaultValue={nome}
                    spellCheck={false}
                    onBlur={(e) =>
                      rinominaAnimazione(nome, e.target.value.trim())
                    }
                    onKeyDown={(e) =>
                      e.key === "Enter" && e.currentTarget.blur()
                    }
                  />
                  <span className="uso">
                    {quante === 0
                      ? t("studio.palette.never")
                      : t("studio.palette.used", { n: quante })}
                  </span>
                  <span
                    className="peso"
                    style={{
                      color: coloreCosto(
                        costoAnimazione(animazione, registro.pesoComposito),
                        registro.motionBudget,
                      ),
                    }}
                    title={t("studio.motion.cost.title")}
                  >
                    {costoAnimazione(animazione, registro.pesoComposito)}
                  </span>
                  <button
                    type="button"
                    className="via icon-btn"
                    aria-label={t("studio.palette.remove", { nome })}
                    onClick={() => togliAnimazione(nome)}
                  >
                    <Icona nome="i-x" dim={12} />
                  </button>
                </div>

                <Riga che={t("studio.motion.duration")}>
                  <ValoreDurata
                    valore={animazione["duration"]}
                    etichetta={`${nome} · ${t("studio.motion.duration")}`}
                    onCambia={(v) =>
                      // `duration` è obbligatoria: togliendola il documento
                      // smette di validare. Il pulsante «×» del controllo
                      // riporta quindi al valore di partenza invece di
                      // cancellare — è l'unico campo dove togliere non è
                      // un'opzione, e mentirgli sarebbe peggio che ignorarlo.
                      scriviCampo("duration", v ?? "240ms")
                    }
                  />
                </Riga>
                <Riga che={t("studio.motion.delay")}>
                  <ValoreDurata
                    valore={animazione["delay"]}
                    etichetta={`${nome} · ${t("studio.motion.delay")}`}
                    onCambia={(v) => scriviCampo("delay", v)}
                  />
                </Riga>
                <Riga che={t("studio.motion.curve")}>
                  <ValoreCurva
                    valore={animazione["easing"]}
                    etichetta={`${nome} · ${t("studio.motion.curve")}`}
                    curveSkin={curveSkin}
                    onCambia={(v) => scriviCampo("easing", v)}
                  />
                </Riga>
                <Riga che={t("studio.motion.iterations")}>
                  <Cursore
                    valore={animazione["iterations"] ?? 1}
                    min={1}
                    max={registro.maxIterazioni}
                    passo={1}
                    etichetta={`${nome} · ${t("studio.motion.iterations")}`}
                    onCambia={(v) => scriviCampo("iterations", v)}
                  />
                </Riga>
                <Riga che={t("studio.motion.direction")}>
                  <Segmentato
                    etichetta={`${nome} · ${t("studio.motion.direction")}`}
                    classe="minuto denso"
                    scelta={String(animazione["direction"] ?? "normal")}
                    onScegli={(v) => scriviCampo("direction", v)}
                    voci={registro.versi.map((v) => ({
                      chiave: v,
                      etichetta: v,
                    }))}
                  />
                </Riga>

                <div className="fotogrammi">
                  <span className="titolino">
                    {t("studio.motion.frames", { n: fotogrammi.length })}
                  </span>
                  {fotogrammi.map((fotogramma, indice) => {
                    const cambia = (campo: string, valore: unknown) =>
                      scriviFotogrammi(
                        fotogrammi.map((f, i) =>
                          i === indice ? { ...f, [campo]: valore } : f,
                        ),
                      );
                    return (
                      // L'indice è la chiave perché è l'identità: un fotogramma
                      // non ha un nome, e la sua posizione nell'elenco è quel
                      // che il formato legge.
                      <div key={indice} className="un-fotogramma">
                        <label className="quando-fotogramma">
                          <span>at</span>
                          <input
                            className="field-input"
                            type="number"
                            aria-label={t("studio.motion.frame.at", {
                              n: indice + 1,
                            })}
                            min={0}
                            max={100}
                            step={1}
                            // Il primo sta a 0 e il formato lo pretende: senza,
                            // il motore ricava la partenza dallo stato in cui la
                            // parte si trova, e l'animazione parte da un punto
                            // diverso ogni volta.
                            disabled={indice === 0}
                            value={Number(fotogramma["at"] ?? 0)}
                            onChange={(e) =>
                              cambia(
                                "at",
                                Number.parseFloat(e.target.value) || 0,
                              )
                            }
                          />
                        </label>
                        {(
                          [
                            ["opacity", 0, 1, 0.01, ""],
                            ["scale", 0.5, 1.5, 0.005, ""],
                            ["translateX", -100, 100, 1, "px"],
                            ["translateY", -100, 100, 1, "px"],
                            ["rotate", -30, 30, 1, "°"],
                          ] as const
                        ).map(([campo, min, max, passo, suffisso]) => (
                          <Riga key={campo} che={campo}>
                            <Cursore
                              valore={fotogramma[campo]}
                              min={min}
                              max={max}
                              passo={passo}
                              etichetta={`${nome} · ${indice + 1} · ${campo}`}
                              suffisso={suffisso}
                              onCambia={(v) => cambia(campo, v)}
                            />
                          </Riga>
                        ))}
                        <button
                          type="button"
                          className="via icon-btn"
                          aria-label={t("studio.motion.frames.remove", {
                            n: indice + 1,
                          })}
                          // Due è il minimo, e non è una gentilezza: con un
                          // fotogramma solo non c'è interpolazione, c'è uno
                          // stato — e uno stato si scrive in `states`.
                          disabled={fotogrammi.length <= registro.minFotogrammi}
                          onClick={() =>
                            scriviFotogrammi(
                              fotogrammi.filter((_, i) => i !== indice),
                            )
                          }
                        >
                          <Icona nome="i-x" dim={11} />
                        </button>
                      </div>
                    );
                  })}
                  <button
                    type="button"
                    className="aggiungi-livello"
                    disabled={fotogrammi.length >= registro.maxFotogrammi}
                    onClick={() =>
                      scriviFotogrammi([
                        ...fotogrammi,
                        // In coda e a 100 quando c'è posto: l'ordine crescente
                        // lo pretende il formato, e proporre una fermata che
                        // non lo rispetta vorrebbe dire un errore rosso appena
                        // premuto il tasto.
                        { at: 100, opacity: 1 },
                      ])
                    }
                  >
                    <Icona nome="i-plus" dim={13} />
                    <span>{t("studio.motion.frames.add")}</span>
                  </button>
                </div>
              </div>
            );
          })}
        </div>
        <button
          type="button"
          className="aggiungi-livello"
          disabled={nomiAnimazioni.length >= registro.maxAnimazioni}
          title={
            nomiAnimazioni.length >= registro.maxAnimazioni
              ? t("studio.motion.anims.full", { n: registro.maxAnimazioni })
              : undefined
          }
          onClick={() =>
            scriviMovimento(["animations", nomeLibero(animazioni, "anima", "-")], {
              duration: "240ms",
              frames: [
                { at: 0, opacity: 0 },
                { at: 100, opacity: 1 },
              ],
            })
          }
        >
          <Icona nome="i-plus" dim={13} />
          <span>{t("studio.motion.anims.add")}</span>
        </button>
      </Scheda>

      {/*
        Il budget del movimento. Non è quello delle superfici e non ci si somma:
        dodici per parte, e la parte che lo sfora la nomina il nucleo.
      */}
      <Scheda
        icona="i-alert"
        titolo={t("studio.motion.budget")}
        nota={`${animate.length} / ${registro.maxPartiAnimate}`}
      >
        {animate.length === 0 ? (
          <p className="niente">{t("studio.motion.budget.none")}</p>
        ) : (
          <div className="budget-movimento">
            {animate.map(({ parte, costo, quante }) => (
              <div
                key={parte}
                className="riga-budget"
                data-sopra={sforate.has(parte) || undefined}
              >
                <code className="nome">.{parte}</code>
                <span className="uso">
                  {t("studio.motion.budget.triggers", { n: quante })}
                </span>
                <div className="misuratore">
                  <span
                    style={{
                      width: `${Math.min(100, (costo / registro.motionBudget) * 100)}%`,
                      background: coloreCosto(costo, registro.motionBudget),
                    }}
                  />
                </div>
                <strong
                  style={{ color: coloreCosto(costo, registro.motionBudget) }}
                >
                  {costo}
                </strong>
                <span className="su">/ {registro.motionBudget}</span>
              </div>
            ))}
          </div>
        )}
        <p className="nota">
          {t("studio.motion.budget.note", {
            budget: registro.motionBudget,
            parti: registro.maxPartiAnimate,
            // Il formato che il registro dichiara, così la frase invecchia con
            // il crate e non con questo file.
            formato: registro.format,
            totale,
          })}
        </p>
      </Scheda>
    </>
  );
}
