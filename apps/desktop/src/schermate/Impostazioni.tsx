/**
 * Impostazioni: tutto quel che prima stava nella barra laterale.
 *
 * # Perché è una pagina e non un pannello
 *
 * Cartelle, scansione, importazione, skin e statistiche vivevano in cinque
 * blocchi impilati sotto la navigazione. Non era una scelta di disegno: era il
 * posto dove c'era spazio. Il risultato è che la barra che dice «dove sono»
 * conteneva anche una barra di avanzamento, due menù a tendina e un pulsante che
 * riscrive i nomi dei file sul disco — comandi rari, permanenti, e alla stessa
 * distanza dal dito di «Album».
 *
 * Qui sono sei sezioni con un indice a sinistra, e la larghezza per spiegare
 * cosa fanno. Un comando che sposta dei file merita una riga di testo accanto;
 * in una colonna da 240 pixel quella riga non ci stava, quindi non c'era.
 */
import { useState } from "react";

import type {
  Avanzamento,
  AvanzamentoArricchimento,
  Avvio,
  EsitoArricchimento,
  EsitoScansione,
  StatoArricchimento,
  StatoNuvola,
  VoceSkin,
} from "../ipc";
import { testoErrore } from "../ipc";
import { ore } from "../formato";
import { Equalizzatore } from "../parti/Equalizzatore";
import { Icona, type NomeIcona } from "../parti/Icone";
import { Importazioni, type UsoImportazioni } from "../parti/Importazioni";
import { Segmentato } from "../parti/Segmentato";
import type { Tema } from "../tema";

/** Le otto sezioni, nell'ordine in cui si visitano la prima volta. */
export type Sezione =
  | "cartelle"
  | "aspetto"
  | "riproduzione"
  | "movimento"
  | "nuvola"
  | "legacy"
  | "spotify"
  | "dati";

// «Backup su Drive» sta fra «movimento» e «legacy»: parla di dati che si
// spostano da un computer all'altro, e i suoi due vicini sono l'importazione
// dalla versione precedente e le statistiche della libreria. «Da Spotify» sta
// accanto a «Dalla versione precedente» perché è la stessa cosa da un'altra
// parte: portare dentro qualcosa che l'utente ha già altrove.
const SEZIONI: readonly (readonly [Sezione, string, NomeIcona])[] = [
  ["cartelle", "Cartelle e scansione", "i-folder"],
  ["aspetto", "Aspetto", "i-skin"],
  ["riproduzione", "Riproduzione", "i-play"],
  ["movimento", "Movimento e accesso", "i-eq"],
  ["nuvola", "Backup su Drive", "i-cloud"],
  ["legacy", "Dalla versione precedente", "i-import"],
  ["spotify", "Da Spotify", "i-list"],
  ["dati", "Libreria e dati", "i-album"],
];

/** Quando è successa una cosa, in una forma che si legge a colpo d'occhio. */
function quando(ms: number | null): string {
  if (ms === null || ms <= 0) return "mai";
  const passati = Date.now() - ms;
  if (passati < 60_000) return "poco fa";
  if (passati < 3_600_000) return `${Math.round(passati / 60_000)} minuti fa`;
  return new Date(ms).toLocaleString();
}

/** Una scheda di sezione. */
function Scheda({
  icona,
  titolo,
  nota,
  children,
}: {
  icona: NomeIcona;
  titolo: string;
  nota?: string | undefined;
  children: React.ReactNode;
}) {
  return (
    <section className="scheda section-card">
      <header>
        <span className="section-icon" aria-hidden="true">
          <Icona nome={icona} dim={16} />
        </span>
        <h2 className="section-heading">{titolo}</h2>
        {nota !== undefined && <span className="nota-testa">{nota}</span>}
      </header>
      {children}
    </section>
  );
}

/** Un interruttore. Spento con la sua ragione quando il comando non c'è. */
function Interruttore({
  etichetta,
  spiegazione,
  acceso,
  onCambia,
  impedito,
}: {
  etichetta: string;
  spiegazione: string;
  acceso: boolean;
  onCambia?: ((valore: boolean) => void) | undefined;
  /** Perché non si può toccare. Presente ⇒ spento. */
  impedito?: string | undefined;
}) {
  return (
    <div className="riga-opzione">
      <div className="che-cosa">
        <div className="etichetta">{etichetta}</div>
        <div className="spiegazione">{impedito ?? spiegazione}</div>
      </div>
      <button
        type="button"
        className="interruttore switch"
        role="switch"
        aria-checked={acceso}
        aria-label={etichetta}
        disabled={impedito !== undefined}
        title={impedito}
        onClick={() => onCambia?.(!acceso)}
      >
        <span className="pista switch-track" aria-hidden="true">
          <span className="pallina" />
        </span>
      </button>
    </div>
  );
}

export function Impostazioni({
  sezione,
  onSezione,
  avvio,
  scansione,
  esito,
  skin,
  dinamici,
  accentoPermesso,
  accentoDinamico,
  onAccentoDinamico,
  movimento,
  tema,
  onTema,
  eqAttivo,
  eqGuadagni,
  onErrore,
  onAggiungiCartella,
  onTogliCartella,
  onRiordina,
  onScansiona,
  onAnnullaScansione,
  onScegliSkin,
  onAnteprimaSkin,
  onInstallaSkin,
  onCreaTema,
  onApriStudio,
  onImporta,
  onImportaSpotify,
  importazioni,
  arricchimento,
  avanzaArricchimento,
  esitoArricchimento,
  onArricchimentoAttiva,
  onArricchimentoAnnulla,
  nuvola,
  onNuvolaCollega,
  onNuvolaScollega,
  onNuvolaAttiva,
  onNuvolaSalva,
  onNuvolaRipristina,
  onNuvolaCredenziali,
}: {
  sezione: Sezione;
  onSezione: (s: Sezione) => void;
  avvio: Avvio | null;
  scansione: Avanzamento | null;
  esito: EsitoScansione | null;
  skin: VoceSkin[];
  /** Quanti token della skin attiva seguono la copertina. */
  dinamici: number;
  /**
   * La skin attiva permette l'accento dinamico (`capabilities.dynamicAccent`).
   *
   * È una dichiarazione dell'autore e vince sulla preferenza: una skin può
   * essere costruita attorno al suo accento, e `sala` lo è.
   */
  accentoPermesso: boolean;
  /** La preferenza, com'è nel database. */
  accentoDinamico: boolean;
  onAccentoDinamico: (attivo: boolean) => void;
  /** Il movimento che la skin dichiara: `none`, `essential`, `full`, `maximum`. */
  movimento: string;
  tema: Tema;
  onTema: (t: Tema) => void;
  /** L'equalizzatore è acceso, secondo il nucleo. */
  eqAttivo: boolean;
  /** La sua curva, in decibel per banda. */
  eqGuadagni: number[];
  onErrore: (e: unknown) => void;
  onAggiungiCartella: () => void;
  onTogliCartella: (percorso: string) => void;
  onRiordina: (percorso: string) => void;
  onScansiona: () => void;
  onAnnullaScansione: () => void;
  onScegliSkin: (id: string) => void;
  /** Anteprima al passaggio: compila senza scegliere. `null` la revoca. */
  onAnteprimaSkin: (id: string | null) => void;
  onInstallaSkin: () => void;
  /** Apre la finestrella che battezza un tema nuovo. */
  onCreaTema: () => void;
  /** Apre lo Skin Studio su questa skin. */
  onApriStudio: (id: string) => void;
  onImporta: () => void;
  /** Apre la finestrella in cui si incolla un link di Spotify. */
  onImportaSpotify: () => void;
  /**
   * Le importazioni da Spotify e la coda che le scarica.
   *
   * Un oggetto solo e non otto prop sciolte: è uno stato coeso che vive in
   * `App` — la coda gira anche quando questa schermata non è aperta — e passa
   * di qui soltanto per essere mostrato.
   */
  importazioni: UsoImportazioni;
  /** Lo stato dell'arricchimento, o `null` finché non è stato chiesto. */
  arricchimento: StatoArricchimento | null;
  /** A che punto è la passata in corso, in gruppi d'album. `null` = ferma. */
  avanzaArricchimento: AvanzamentoArricchimento | null;
  /** Cosa ha prodotto l'ultima passata di questa sessione. */
  esitoArricchimento: EsitoArricchimento | null;
  onArricchimentoAttiva: (attivo: boolean) => void;
  /** Riporta indietro i tag, e spegne l'automatico. */
  onArricchimentoAnnulla: () => void;
  /** Lo stato del backup, o `null` finché non è stato chiesto. */
  nuvola: StatoNuvola | null;
  onNuvolaCollega: () => void;
  onNuvolaScollega: () => void;
  onNuvolaAttiva: (attivo: boolean) => void;
  onNuvolaSalva: () => void;
  /** Apre la finestrella del ripristino. */
  onNuvolaRipristina: () => void;
  onNuvolaCredenziali: (clientId: string, clientSecret: string) => void;
}) {
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const attiva = skin.find((s) => s.attiva);
  const percentuale =
    scansione && scansione.totale > 0
      ? Math.round((scansione.fatti / scansione.totale) * 100)
      : 0;

  return (
    <div className="impostazioni">
      <nav className="indice" aria-label="Sezioni delle impostazioni">
        {SEZIONI.map(([chiave, etichetta, icona]) => (
          <button
            key={chiave}
            type="button"
            className="voce nav-pill"
            aria-current={sezione === chiave ? "true" : undefined}
            data-active={sezione === chiave || undefined}
            onClick={() => onSezione(chiave)}
          >
            <Icona nome={icona} dim={16} />
            <span>{etichetta}</span>
          </button>
        ))}
      </nav>

      <div className="corpo">
        {sezione === "cartelle" && (
          <Scheda
            icona="i-folder"
            titolo="Cartelle sorvegliate"
            nota="Aether legge, non sposta e non modifica"
          >
            {avvio && avvio.cartelle.length > 0 ? (
              <ul className="cartelle">
                {avvio.cartelle.map((c) => (
                  <li className="cartella" key={c}>
                    {/* `unicode-bidi: plaintext` accanto a `direction: rtl` sta
                        nel foglio: senza, un percorso UNC che comincia con due
                        barre rovesciate si disegnava con quelle in fondo. */}
                    <span className="percorso" title={c}>
                      {c}
                    </span>
                    <button
                      type="button"
                      className="tasto icon-btn"
                      aria-label={`Riordina ${c}`}
                      /* Il riordino è per cartella e non per libreria:
                         `plan_organize` ragiona su una radice sola, e quel che
                         sta fuori lo lascia fermo. */
                      title="Riordina questa cartella…"
                      onClick={() => onRiordina(c)}
                    >
                      <Icona nome="i-sort" dim={14} />
                    </button>
                    <button
                      type="button"
                      className="tasto icon-btn"
                      aria-label={`Smetti di sorvegliare ${c}`}
                      title="Togli"
                      onClick={() => onTogliCartella(c)}
                    >
                      <Icona nome="i-x" dim={14} />
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="niente empty-state">
                Nessuna cartella. Aether non sa ancora dove tieni la musica.
              </p>
            )}

            <div className="azioni">
              <button type="button" className="bottone btn-ghost" onClick={onAggiungiCartella}>
                <Icona nome="i-plus" dim={15} />
                Aggiungi cartella…
              </button>
              <button
                type="button"
                className="bottone primario btn-accent"
                onClick={onScansiona}
                disabled={scansione !== null || (avvio?.cartelle.length ?? 0) === 0}
              >
                <Icona nome="i-scan" dim={15} />
                {scansione ? "Scansione in corso…" : "Scansiona"}
              </button>
              <span className="oppure">o trascinala qui</span>
            </div>

            {scansione && (
              <div className="avanzamento-scansione">
                <div className="riga-avanzamento">
                  <span className="quanti">
                    {scansione.totale > 0
                      ? `${scansione.fatti.toLocaleString("it")} / ${scansione.totale.toLocaleString("it")}`
                      : "confronto col disco…"}
                  </span>
                  <button
                    type="button"
                    className="bottone minuto btn-ghost"
                    onClick={onAnnullaScansione}
                  >
                    Annulla
                  </button>
                </div>
                <div className="barra player-progress">
                  <div className="riempita" style={{ width: `${percentuale}%` }}>
                    <span className="riflesso progress-sheen" aria-hidden="true" />
                  </div>
                </div>
                <p className="nota">
                  Una scansione interrotta lascia una libreria giusta e incompleta:
                  ogni lotto sta nella sua transazione, e la passata dopo riprende
                  da dove questa si ferma.
                </p>
              </div>
            )}

            {esito && (
              <p className="esito">
                {esito.inseriti} aggiunti · {esito.aggiornati} aggiornati ·{" "}
                {esito.spostati} spostati · {esito.tolti} tolti, in{" "}
                {(esito.durataMs / 1000).toFixed(1)} s
                {esito.illeggibili > 0 && ` · ${esito.illeggibili} illeggibili`}
                {/* Chi ha premuto Annulla deve leggere che si è fermata, non
                    «completata»: la libreria è giusta ma incompleta, e il modo
                    di finirla è rifarla. */}
                {esito.annullata && (
                  <>
                    {" · "}
                    <strong>fermata a metà</strong>: una nuova scansione riprende
                    da qui senza rileggere quel che c&apos;è già.
                  </>
                )}
              </p>
            )}
          </Scheda>
        )}

        {/* Accanto alle cartelle e non in una sezione sua: l'arricchimento è
            quel che succede ai file **dopo** che la scansione li ha trovati, e
            chi viene qui a sistemare la libreria è chi ha bisogno di trovarlo. */}
        {sezione === "cartelle" && (
          <Scheda
            icona="i-scan"
            titolo="Metadati"
            nota="Scrive nei file, e sa tornare indietro"
          >
            <p className="nota">
              Quando un brano entra senza album, senza copertina o con un titolo
              preso da YouTube, Aether cerca il disco su MusicBrainz e scrive
              quel che trova nei tag del file. Cerca <strong>l&apos;album
              intero</strong>, non il singolo brano: dodici durate che
              coincidono sono una prova, un titolo che somiglia non lo è.
            </p>
            <p className="nota">
              Quando le prove non bastano <strong>non scrive niente</strong> e
              riprova più avanti. È il motivo per cui non c&apos;è una schermata
              da approvare: le corrispondenze incerte vengono scartate invece di
              essere messe in coda per qualcuno.
            </p>

            {arricchimento?.errore && (
              <div className="errore">{testoErrore(arricchimento.errore)}</div>
            )}

            <Interruttore
              etichetta="Arricchisci i metadati da solo"
              spiegazione="Una passata ogni mezz'ora, e dopo ogni scansione o scaricamento. I tag di prima restano salvati, così si può tornare indietro."
              acceso={arricchimento?.attivo ?? false}
              onCambia={onArricchimentoAttiva}
            />

            <dl className="numeri">
              <div>
                <dt>Completati</dt>
                <dd className="stat-number">
                  {(arricchimento?.completati ?? 0).toLocaleString("it")}
                </dd>
              </div>
              <div>
                <dt>Senza corrispondenza</dt>
                <dd className="stat-number">
                  {(arricchimento?.senzaCorrispondenza ?? 0).toLocaleString("it")}
                </dd>
              </div>
              <div>
                <dt>In attesa</dt>
                <dd className="stat-number">
                  {(arricchimento?.daFare ?? 0).toLocaleString("it")}
                </dd>
              </div>
              <div>
                <dt>Ultima passata</dt>
                <dd className="stat-number">
                  {arricchimento?.inCorso
                    ? "in corso…"
                    : quando(arricchimento?.ultimoMs ?? null)}
                </dd>
              </div>
            </dl>

            {avanzaArricchimento && (
              <div className="avanzamento-scansione">
                <div className="riga-avanzamento">
                  <span className="quanti">
                    {avanzaArricchimento.fatti} / {avanzaArricchimento.totale}{" "}
                    dischi
                  </span>
                </div>
                <div className="barra player-progress">
                  <div
                    className="riempita"
                    style={{
                      width: `${
                        avanzaArricchimento.totale > 0
                          ? Math.round(
                              (avanzaArricchimento.fatti /
                                avanzaArricchimento.totale) *
                                100,
                            )
                          : 0
                      }%`,
                    }}
                  >
                    <span className="riflesso progress-sheen" aria-hidden="true" />
                  </div>
                </div>
                <p className="nota">
                  Un disco per volta, con una pausa fra una richiesta e
                  l&apos;altra: MusicBrainz ne concede una al secondo, ed è la
                  ragione per cui si cerca l&apos;album e non i singoli brani.
                </p>
              </div>
            )}

            {esitoArricchimento && !avanzaArricchimento && (
              <p className="esito">
                Ultima passata: {esitoArricchimento.applicati} applicati ·{" "}
                {esitoArricchimento.astenuti} lasciati stare (di cui{" "}
                {esitoArricchimento.senzaCorrispondenza} senza corrispondenza)
                {esitoArricchimento.copertine > 0 &&
                  ` · ${esitoArricchimento.copertine} copertine`}
              </p>
            )}

            <div className="azioni">
              <button
                type="button"
                className="bottone btn-ghost"
                /* Senza righe da riportare non c'è niente da disfare, e un
                   pulsante attivo prometterebbe qualcosa che non succede. */
                disabled={
                  arricchimento === null ||
                  arricchimento.inCorso ||
                  arricchimento.annullabili === 0
                }
                onClick={onArricchimentoAnnulla}
              >
                <Icona nome="i-x" dim={15} />
                {arricchimento?.inCorso
                  ? "Passata in corso…"
                  : `Annulla l'arricchimento${
                      arricchimento && arricchimento.annullabili > 0
                        ? ` (${arricchimento.annullabili.toLocaleString("it")} brani)`
                        : ""
                    }`}
              </button>
            </div>

            <p className="nota">
              Annullare rimette nei file i tag di prima e{" "}
              <strong>spegne l&apos;automatico</strong>: senza, i brani appena
              riportati indietro tornerebbero candidati e la passata successiva
              riscriverebbe entro mezz&apos;ora quel che hai appena disfatto.
            </p>
          </Scheda>
        )}

        {sezione === "aspetto" && (
          <>
            <Scheda icona="i-skin" titolo="Tema">
              <Segmentato
                etichetta="Tema chiaro o scuro"
                scelta={tema}
                onScegli={onTema}
                voci={[
                  { chiave: "scuro", etichetta: "Scuro" },
                  {
                    chiave: "chiaro",
                    etichetta: "Chiaro",
                    /* Una skin che non dichiara `capabilities.light` non ha una
                       variante chiara: mostrare l'interruttore acceso e non far
                       succedere niente è la promessa che `check_skin` chiama
                       `UnkeptCapability`. */
                    spenta:
                      attiva && !attiva.chiara
                        ? `«${attiva.nome}» non ha una variante chiara`
                        : undefined,
                  },
                  { chiave: "sistema", etichetta: "Sistema" },
                ]}
              />
              <p className="nota">
                «Sistema» segue <code>prefers-color-scheme</code>, e cambia da sé
                quando il computer passa alla sera.
              </p>
            </Scheda>

            <Scheda icona="i-skin" titolo="Skin" nota={`${skin.length} installate`}>
              <div className="griglia-skin">
                {skin.map((s) => {
                  /* Tre colori scritti dall'autore in `meta.preview`. Quando non
                     ci sono si ripiega sul fondo, sulla superficie e
                     sull'accento — che per una skin sobria dà tre grigi quasi
                     uguali, ed è esattamente il motivo per cui il campo esiste. */
                  const bande =
                    s.anteprima.length === 3
                      ? s.anteprima
                      : ["var(--color-surface-0)", "var(--color-surface-2)", "var(--accent)"];
                  return (
                    <div
                      key={s.id}
                      className="scheda-skin section-card"
                      data-active={s.attiva || undefined}
                      onMouseEnter={() => !s.attiva && onAnteprimaSkin(s.id)}
                      onMouseLeave={() => !s.attiva && onAnteprimaSkin(null)}
                    >
                      <button
                        type="button"
                        className="prova"
                        aria-label={`Usa la skin ${s.nome}`}
                        onClick={() => onScegliSkin(s.id)}
                      >
                        <span className="bande" aria-hidden="true">
                          {bande.map((colore, i) => (
                            <span key={i} style={{ background: colore }} />
                          ))}
                        </span>
                        <span className="nome-skin">{s.nome}</span>
                        <span className="stato">
                          {s.attiva ? "in uso" : s.diSerie ? "di serie" : s.autore}
                        </span>
                      </button>
                      {s.descrizione !== null && (
                        <p className="descrizione">{s.descrizione}</p>
                      )}
                      <button
                        type="button"
                        className="bottone minuto btn-ghost"
                        onClick={() => onApriStudio(s.id)}
                      >
                        <Icona nome="i-text" dim={13} />
                        {s.diSerie ? "Deriva…" : "Apri nello Studio"}
                      </button>
                    </div>
                  );
                })}
              </div>
              <div className="azioni">
                {/* Prima di «Installa»: fare un tema è il comando principale di
                    questa scheda, installarne uno fatto da altri è il caso
                    raro. Il pulsante per-scheda qui sopra apre lo Studio su una
                    skin che c'è già; questo ne fa una che non c'è. */}
                <button type="button" className="bottone primario btn-accent" onClick={onCreaTema}>
                  <Icona nome="i-plus" dim={15} />
                  Crea tema…
                </button>
                <button type="button" className="bottone btn-ghost" onClick={onInstallaSkin}>
                  <Icona nome="i-import" dim={15} />
                  Installa una skin…
                </button>
                <span className="oppure">.aeskin · o trascinala qui</span>
              </div>
            </Scheda>
          </>
        )}

        {sezione === "riproduzione" && (
          <>
          <Scheda icona="i-eq" titolo="Equalizzatore">
            <p className="nota">
              Dieci bande a ottave, da 31 Hz a 16 kHz: ognuna copre il doppio di
              frequenze della precedente, che è il modo in cui l&apos;orecchio le
              sente: a fasce larghe uguali in hertz, il primo cursore da solo
              conterrebbe bassi, medi e quasi tutte le fondamentali, e gli altri
              nove si dividerebbero l&apos;aria che resta.
            </p>
            <p className="nota">
              I filtri lavorano dentro la callback audio, non prima:
              così un cursore si sente <strong>mentre</strong> lo trascini
              invece che un quinto di secondo dopo. Alzare più bande non fa
              saturare — la preamplificazione misura il picco della curva e
              abbassa quanto basta, in automatico.
            </p>
            <Equalizzatore
              attivo={eqAttivo}
              guadagni={eqGuadagni}
              taglia="pagina"
              onErrore={onErrore}
            />
          </Scheda>
          <Scheda icona="i-play" titolo="Riproduzione">
            {/* Acceso, finalmente, e senza scorciatoie. La tinta dominante la
                estrae `aether-app` dalla miniatura del disco; se sia leggibile
                lo decide `aether-skin` in OKLCH, dove vive `contrast_ratio`.
                Quando nessuna chiarezza di quella tonalità regge 4,5:1 contro
                le superfici del tema, la risposta è «no» e vince l'accento
                della skin: è la ragione per cui questo interruttore è rimasto
                spento finché non c'è stato un posto in cui metterlo che non
                fosse «scrivi il colore e spera». */}
            <Interruttore
              etichetta="L'accento segue la copertina"
              spiegazione="L'accento e le sue varianti prendono il colore dal disco che sta suonando, ritagliato in OKLCH perché regga il contrasto su ogni superficie del tema. Il testo non segue mai la copertina, e un disco in bianco e nero non cambia niente."
              acceso={accentoDinamico && accentoPermesso}
              onCambia={onAccentoDinamico}
              impedito={
                accentoPermesso
                  ? undefined
                  : "Questa skin dichiara «dynamicAccent: false»: è una scelta di chi l'ha fatta, e vince su questa preferenza — una skin può essere costruita attorno al suo accento."
              }
            />
            {accentoPermesso && dinamici > 0 && (
              <p className="nota">
                Di questa skin, <strong>{dinamici}</strong>{" "}
                {dinamici === 1 ? "token dichiara" : "token dichiarano"} una
                sorgente dalla copertina: seguono l&apos;accento da sé, perché il
                compilatore li scrive come <code>var(--accent)</code>.
              </p>
            )}
            <Interruttore
              etichetta="Normalizza il volume (ReplayGain)"
              spiegazione="Porta tutti i dischi allo stesso volume percepito."
              acceso={false}
              impedito="Il motore audio lo sa fare — `aether-play` legge i tag e applica il guadagno — ma non c'è ancora un comando che lo accenda dalla finestra."
            />
          </Scheda>
          </>
        )}

        {sezione === "movimento" && (
          <Scheda icona="i-eq" titolo="Movimento e accesso">
            <p className="nota">
              Il movimento lo dichiara la skin (<code>motion.intensity</code>), e
              questa ne dice <strong>{movimento}</strong>.
              Se hai spento le animazioni nel sistema operativo, quella scelta
              vince su questa — sempre, e non è configurabile: una preferenza di
              accessibilità che una skin può sovrascrivere non è una preferenza.
            </p>
            <p className="nota">
              Le stesse regole valgono per <code>layout</code>: player flottante o
              agganciato, barra stretta o aperta, densità comoda o compatta. Sono
              quattro campi che il formato accetta da sempre e che fino a oggi
              nessuno leggeva.
            </p>
          </Scheda>
        )}

        {sezione === "nuvola" && (
          <Scheda icona="i-cloud" titolo="Backup su Drive">
            <p className="nota">
              Salva su Google Drive quel che una scansione <strong>non</strong>{" "}
              sa ricostruire: ascolti, voti, preferiti, playlist, cartelle
              sorvegliate, skin e bozze dello Studio. Non i file audio, non le
              copertine — quelli si ritrovano da soli.
            </p>
            <p className="nota">
              Finiscono in una cartella privata dell&apos;applicazione: invisibile
              in Drive, illeggibile da qualunque altra app, e cancellata quando
              togli i dati di Aether dal tuo account.
            </p>

            {nuvola?.errore && (
              <div className="errore">{testoErrore(nuvola.errore)}</div>
            )}

            {nuvola && !nuvola.configurato && (
              <p className="nota">
                Questa copia di Aether è stata compilata senza credenziali
                Google, quindi il backup non può partire. Puoi metterne di tue
                qui sotto.
              </p>
            )}

            <Interruttore
              etichetta="Backup automatico"
              spiegazione="Una passata ogni quarto d'ora, e dopo ogni modifica. Solo quel che è cambiato viene ricaricato."
              acceso={nuvola?.attivo ?? false}
              onCambia={onNuvolaAttiva}
              impedito={
                nuvola?.collegato
                  ? undefined
                  : "Collega prima un account Google"
              }
            />

            <dl className="numeri">
              <div>
                <dt>Account</dt>
                <dd className="stat-number">
                  {nuvola?.email ?? (nuvola?.collegato ? "collegato" : "nessuno")}
                </dd>
              </div>
              <div>
                <dt>Ultimo salvataggio</dt>
                <dd className="stat-number">{quando(nuvola?.ultimoMs ?? null)}</dd>
              </div>
            </dl>

            <div className="azioni">
              {nuvola?.collegato ? (
                <>
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={nuvola.inCorso}
                    onClick={onNuvolaSalva}
                  >
                    <Icona nome="i-cloud" dim={15} />
                    {nuvola.inCorso ? "Salvataggio in corso…" : "Salva adesso"}
                  </button>
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={nuvola.inCorso}
                    onClick={onNuvolaRipristina}
                  >
                    <Icona nome="i-import" dim={15} />
                    Ripristina…
                  </button>
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={nuvola.inCorso}
                    onClick={onNuvolaScollega}
                  >
                    Scollega
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  className="bottone btn-ghost"
                  disabled={!nuvola?.configurato || nuvola.inCorso}
                  onClick={onNuvolaCollega}
                >
                  <Icona nome="i-cloud" dim={15} />
                  {nuvola?.inCorso
                    ? "Aspetto il consenso…"
                    : "Collega un account Google"}
                </button>
              )}
            </div>

            {nuvola?.collegato === false && nuvola.configurato && (
              <p className="nota">
                Il consenso si dà nel <strong>browser di sistema</strong>, non
                qui dentro: una finestra dell&apos;applicazione che sa disegnare
                la pagina di Google è esattamente la forma che ha un tentativo di
                phishing.
              </p>
            )}

            <details className="non-ritrovati">
              <summary>Usa il tuo progetto Google</summary>
              <p>
                Serve solo se vuoi che la quota sia la tua invece di quella di
                Aether. Da Google Cloud Console: abilita la Drive API, crea un
                client OAuth di tipo <strong>Desktop app</strong>, e{" "}
                <strong>pubblica</strong> la schermata di consenso — restando in
                «Testing» i permessi scadono dopo sette giorni e il backup
                smetterebbe in silenzio.
              </p>
              <p className="nota">
                Lasciare vuoto l&apos;identificativo rimette quelle compilate
                dentro l&apos;applicazione.
              </p>
              <div className="azioni">
                <input
                  type="text"
                  className="campo"
                  placeholder="client id"
                  value={clientId}
                  onChange={(e) => setClientId(e.target.value)}
                />
                <input
                  type="password"
                  className="campo"
                  placeholder="client secret"
                  value={clientSecret}
                  onChange={(e) => setClientSecret(e.target.value)}
                />
                <button
                  type="button"
                  className="bottone btn-ghost"
                  onClick={() => onNuvolaCredenziali(clientId, clientSecret)}
                >
                  Usa queste
                </button>
              </div>
            </details>
          </Scheda>
        )}

        {sezione === "legacy" && (
          <Scheda icona="i-import" titolo="Dalla versione precedente">
            <p className="nota">
              Porta ascolti, voti, preferiti, cronologia e playlist dalla 1.0.0.
              Il vecchio database si apre in <strong>sola lettura</strong> e non
              viene toccato: sono le uniche cose che una scansione non può
              ricostruire.
            </p>
            <div className="azioni">
              <button type="button" className="bottone btn-ghost" onClick={onImporta}>
                <Icona nome="i-import" dim={15} />
                Scegli il vecchio database…
              </button>
            </div>
          </Scheda>
        )}

        {sezione === "spotify" && (
          <Scheda
            icona="i-list"
            titolo="Da Spotify"
            nota="nessun account, nessuna chiave"
          >
            <p className="nota">
              Incolla il link di un brano, di un album o di una playlist
              pubblica: Aether legge cosa c'è dentro e cerca in libreria i brani
              che hai già. Non chiede di collegare un account.
            </p>
            <p className="nota">
              Quelli che non hai si scaricano da <strong>YouTube</strong>, non
              da Spotify: di là si prendono solo i nomi. Il video lo sceglie
              Aether, preferendo i canali ufficiali e scartando live, cover e
              remix quando non è quello che hai chiesto; sopra ci finiscono i
              tag di Spotify, che sono più affidabili di quelli ricavati dal
              titolo di un video.
            </p>
            <p className="nota">
              I file vanno nella prima cartella sorvegliata e una scansione li
              porta in libreria da sé. Quel che su YouTube non c'è resta come
              elenco, con tutto quel che Spotify ne dice.
            </p>
            <div className="azioni">
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={onImportaSpotify}
              >
                <Icona nome="i-list" dim={15} />
                Incolla un link…
              </button>
            </div>

            {/* Subito sotto il tasto, perché è il posto in cui si torna: la
                finestrella si chiude da sola a importazione confermata, e quel
                che succede dopo — che dura minuti — succede qui. */}
            <Importazioni importazioni={importazioni} />
          </Scheda>
        )}

        {sezione === "dati" && avvio && (
          <Scheda icona="i-album" titolo="Libreria e dati">
            <dl className="numeri">
              <div>
                <dt>Brani</dt>
                <dd className="stat-number">{avvio.numeri.tracks.toLocaleString("it")}</dd>
              </div>
              <div>
                <dt>Album</dt>
                <dd className="stat-number">{avvio.numeri.albums.toLocaleString("it")}</dd>
              </div>
              <div>
                <dt>Artisti</dt>
                <dd className="stat-number">{avvio.numeri.artists.toLocaleString("it")}</dd>
              </div>
              <div>
                <dt>Ascolto</dt>
                <dd className="stat-number">{ore(avvio.numeri.durationMs)}</dd>
              </div>
            </dl>
            <p className="percorso" title={avvio.dataDir}>
              {avvio.dataDir}
            </p>
            <p className="nota">
              {avvio.migrazioni === 0
                ? "Nessuna migrazione all'ultima apertura."
                : `${avvio.migrazioni} migrazioni applicate all'ultima apertura.`}{" "}
              {avvio.fts5
                ? "La ricerca a tutto testo è disponibile."
                : "FTS5 non è disponibile: la ricerca non funziona."}
            </p>
          </Scheda>
        )}
      </div>
    </div>
  );
}
