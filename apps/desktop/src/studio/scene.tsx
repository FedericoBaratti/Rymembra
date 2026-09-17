/**
 * Il contenuto delle scene: le pagine che non stanno nell'albero, e quel che ci
 * sta sopra.
 *
 * # Perché serve un file in più
 *
 * `finto.ts` fa già la parte difficile: le scene sono **stati del mondo**, e il
 * renderer è quello vero, quindi la cornice — navigazione, lettore, coda, la
 * terza colonna — non può divergere dall'applicazione perché non è una copia.
 *
 * Ma la cornice non è tutta l'applicazione. Quale pagina si stia guardando è
 * instradamento, cioè stato dell'app, e l'albero dello scafale ha un **buco**
 * per il contenuto: la libreria, Impostazioni, Importazioni, «In riproduzione»
 * non sono widget e non compaiono in nessuna delle sue configurazioni. Lo stesso
 * vale per le sovrapposizioni, che in `App.tsx` stanno **fuori** dall'albero
 * proprio perché si sovrappongono.
 *
 * # La regola che li tiene onesti, e com'è cambiata
 *
 * Prima era «si scrivono le classi del registro, mai una geometria che l'app non
 * ha», e non bastava: le classi del registro erano giuste e tutto il resto era
 * inventato — `.titolo-scheda` dove l'app dice `.titolo`, `.scheda section-card`
 * per una copertina che nell'app è `.scheda list-row`, un cursore di dissolvenza
 * incrociata in Impostazioni che nell'applicazione non esiste. Il risultato era
 * una skin accordata su proporzioni che nessuno avrebbe mai visto.
 *
 * Adesso la regola è più stretta e più semplice: **si copia il markup vero**,
 * classe per classe, dal componente che disegna quella schermata. Dove il
 * componente si può montare — `Interruttore`, `Segmentato`, `Copertina`,
 * `Trasporto`, `Scrubber`, `Giudizio`, `Stelle` — non si copia affatto, si usa.
 * Quel che resta scritto a mano è markup che nasce da un `map` su dei dati, e la
 * sua forma sta in `stile.css` accanto a quella dell'app perché **è** quella
 * dell'app: gli stessi selettori, non dei gemelli.
 *
 * # Quel che qui dentro non si può disegnare
 *
 * `NON_ANCORA`, in fondo. Sono le parti che il registro dichiara e l'app non
 * monta da nessuna parte: disegnarle qui sarebbe la bugia peggiore che questa
 * vista possa dire, perché chi ridipinge le vedrebbe funzionare mentre scrive e
 * non funzionare una volta installata la skin. Lo Studio le dichiara invece che
 * mostrarle, e `strumenti/classi.js` controlla che questo elenco e il suo —
 * `ATTESE` — restino la stessa cosa.
 */
import { useRef } from "react";
import type { CSSProperties, ReactNode } from "react";

import { Copertina } from "../Copertina";
import type { ContestoWidget, SlotScafale } from "../Impaginazione";
import { Giudizio } from "../parti/Giudizio";
import { Icona, type NomeIcona } from "../parti/Icone";
import { Intestazione } from "../parti/Intestazione";
import { Interruttore } from "../parti/Interruttore";
import { Scrubber } from "../parti/Scrubber";
import { Segmentato } from "../parti/Segmentato";
import { Spettro3D } from "../parti/Spettro3D";
import { Stelle } from "../Stelle";
import { Trasporto } from "../parti/Trasporto";
import type { Accese, Pagina, Sovrapposizione } from "./finto";
import { t, type Chiave } from "../lingue";

/** Un comando che non fa niente: nell'anteprima non c'è niente da comandare. */
const niente = () => {
  /* apposta */
};

/**
 * Quante barre disegna la scena dell'anteprima.
 *
 * Sessantaquattro, cioè `RISOLUZIONE_DI_SERIE` del nucleo, e non le 1024 che il
 * cursore in «In riproduzione» arriva a chiedere. Due ragioni, e la seconda è
 * quella che conta qui dentro: è il numero che vede chi non ha mai scelto,
 * quindi è la scena su cui una skin va accordata; e sopra le 256 la tela
 * infittisce apposta le barre — `barre > 256 ? min(1, fill + 0.18) : fill` — al
 * punto che `canvas.viz.fill` smette di leggersi come «quanto una barra riempie
 * la sua fetta» e diventa un nastro continuo, cioè il cursore che si sta
 * trascinando non si vedrebbe più muovere.
 *
 * Non arriva da `ipc.spettroBande()`: quel numero è la preferenza di **questa**
 * macchina, e lo Studio non dipinge per questa macchina.
 */
const BARRE_ANTEPRIMA = 64;

/**
 * Un guasto della tela, nell'anteprima, si scrive e non si annuncia.
 *
 * Nell'applicazione `onErrore` risale fino agli avvisi, ed è giusto: se la scena
 * non parte, chi ascolta ha un buco nero al posto di quel che ha appena acceso.
 * Qui no. La tela è **un terzo di un pannello** dentro una miniatura, montata da
 * una vista che sta facendo un altro mestiere, e una macchina senza WebGL2 ne
 * ricaverebbe un avviso a livello di applicazione ogni volta che apre la pagina
 * dei pannelli — un allarme per una cosa che non è successa a nessuno.
 *
 * La console resta perché il difetto va comunque visto da chi sviluppa: il
 * silenzio completo è l'altra metà dello stesso sbaglio.
 */
const guastoDellaScena = (e: unknown) => {
  console.error("anteprima dello spettro:", e);
};

/** I brani finti degli elenchi. Tre, che bastano a vedere una riga accesa. */
const RIGHE = [
  { titolo: "Corale in mi minore", autore: "Anna Vestri", durata: "4:11", voto: 4 },
  { titolo: "Le stanze basse", autore: "Anna Vestri", durata: "3:02", voto: 0 },
  { titolo: "Controluce", autore: "Anna Vestri", durata: "5:48", voto: 2 },
  { titolo: "Ultimo piano", autore: "Anna Vestri", durata: "2:37", voto: 5 },
  { titolo: "Cortile d'inverno", autore: "Anna Vestri", durata: "4:55", voto: 0 },
] as const;

// ── la libreria ─────────────────────────────────────────────────────────────

/**
 * La griglia degli album, come la disegna `App.tsx`.
 *
 * `scheda list-row` e non `scheda section-card`: la copertina di un album porta
 * la parte delle **righe**, non quella delle schede, e per un pezzo l'anteprima
 * ha detto il contrario — chi ridipingeva `list-row` non vedeva cambiare la
 * libreria, chi ridipingeva `section-card` la vedeva cambiare e nell'app no.
 */
function Griglia() {
  return (
    <div className="dentro">
      <div className="griglia track-grid">
        {[
          "Le stanze basse",
          "Controluce",
          "Cortile d'inverno",
          "Ultimo piano",
          "Ore piccole",
          "Il muro d'acqua",
          "Sale",
          "Quattro stanze",
        ].map((titolo) => (
          <div key={titolo} className="scheda list-row">
            <Copertina hash={null} titolo={titolo} />
            <div className="titolo">{titolo}</div>
            <div className="sotto">Anna Vestri · 11</div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** L'intestazione di un elenco, copiata da `TestaElenco` in `App.tsx`. */
function TestaElenco() {
  return (
    <div className="testa-elenco" aria-hidden="true">
      <span className="indice">#</span>
      <span />
      <span>{t("list.title")}</span>
      <span className="disco">{t("list.album")}</span>
      <span className="voto">{t("list.rating")}</span>
      <span className="durata">{t("list.duration")}</span>
      <span />
    </div>
  );
}

/**
 * L'elenco dei brani, con la riga accesa e una scelta.
 *
 * I due stati stanno insieme apposta: `list-row` li dichiara entrambi nel
 * registro — «compresi lo stato attivo e quello selezionato» — e una scena che
 * mostrasse solo righe spente lascerebbe chi ridipinge a indovinare due terzi
 * del lavoro.
 */
function Elenco() {
  return (
    <div className="elenco track-grid">
      <TestaElenco />
      {RIGHE.map((riga, i) => (
        <div
          key={riga.titolo}
          className="riga list-row"
          data-active={i === 0 || undefined}
          data-scelta={i === 2 || undefined}
        >
          <button type="button" className="indice">
            <span className="numero">{i + 1}</span>
            <span className="via" aria-hidden="true">
              <Icona nome="i-play" dim={13} />
            </span>
          </button>
          <Copertina hash={null} titolo="Le stanze basse" classe="miniatura" />
          <div className="chi">
            <div className="nome">{riga.titolo}</div>
            <div className="autore">{riga.autore}</div>
          </div>
          <div className="disco">Le stanze basse</div>
          <Stelle valore={riga.voto} onVoto={niente} />
          <div className="durata">{riga.durata}</div>
          <button type="button" className="cuore icon-btn" aria-pressed={i === 0}>
            <Icona nome={i === 0 ? "i-heart-f" : "i-heart"} dim={15} />
          </button>
        </div>
      ))}
    </div>
  );
}

/** La pagina di un album: l'elenco dei suoi brani. La copertina sta in testa. */
function Album() {
  return (
    <div className="dentro">
      <Elenco />
    </div>
  );
}

/** I segnaposto, copiati da `GrigliaFinta` in `App.tsx`. */
function Finta() {
  return (
    <div className="dentro">
      <div className="griglia track-grid" role="status">
        {Array.from({ length: 8 }, (_, i) => (
          <div className="scheda finta" key={i} aria-hidden="true">
            <div className="copertina skeleton" />
            <div className="riga-finta skeleton" />
            <div className="riga-finta corta skeleton" />
          </div>
        ))}
      </div>
    </div>
  );
}

/** Lo stato vuoto, copiato da `App.tsx`. */
function Vuoto() {
  return (
    <div className="dentro">
      <div className="vuoto empty-state">
        <span className="empty-icon" aria-hidden="true">
          <Icona nome="i-scan" dim={30} />
        </span>
        <h2>{t("studio.mock.emptyTitle")}</h2>
        <p>{t("studio.mock.emptySub")}</p>
        <button type="button" className="bottone primario btn-accent">
          {t("studio.mock.import")}
        </button>
      </div>
    </div>
  );
}

// ── le impostazioni, e le sue sezioni ───────────────────────────────────────

/** Le voci dell'indice, con la loro icona. Le stesse di `sezioni()`. */
const INDICE: readonly (readonly [string, Chiave, NomeIcona])[] = [
  ["cartelle", "settings.section.cartelle", "i-folder"],
  ["aspetto", "settings.section.aspetto", "i-skin"],
  ["riproduzione", "settings.section.riproduzione", "i-play"],
  ["movimento", "settings.section.movimento", "i-eq"],
  ["nuvola", "settings.section.nuvola", "i-cloud"],
  ["esterno", "settings.section.esterno", "i-list"],
  ["scrobbling", "settings.section.scrobbling", "i-cloud"],
  ["dati", "settings.section.dati", "i-album"],
];

/** La scheda di una sezione, copiata da `Scheda` in `schermate/Impostazioni`. */
function Scheda({
  icona,
  titolo,
  nota,
  children,
}: {
  icona: NomeIcona;
  titolo: string;
  nota?: string | undefined;
  children: ReactNode;
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

/** Le quattro cifre di una scheda, copiate dal `<dl className="numeri">`. */
function Numeri({ voci }: { voci: readonly (readonly [string, string])[] }) {
  return (
    <dl className="numeri">
      {voci.map(([cosa, quanto]) => (
        <div key={cosa}>
          <dt>{cosa}</dt>
          <dd className="stat-number">{quanto}</dd>
        </div>
      ))}
    </dl>
  );
}

/** L'indice a sinistra: `nav-pill` nel suo secondo habitat. */
function Indice({ sezione }: { sezione: string }) {
  return (
    <nav className="indice">
      <div className="filtro-impostazioni">
        <Icona nome="i-search" dim={14} />
        <input
          type="search"
          readOnly
          value=""
          placeholder={t("settings.index.search")}
          aria-label={t("settings.index.search")}
        />
      </div>
      {INDICE.map(([chiave, etichetta, icona]) => (
        <button
          key={chiave}
          type="button"
          className="voce nav-pill"
          data-active={chiave === sezione || undefined}
          aria-current={chiave === sezione ? "true" : undefined}
        >
          <Icona nome={icona} dim={16} />
          <span>{t(etichetta)}</span>
        </button>
      ))}
    </nav>
  );
}

/**
 * Impostazioni › Cartelle: l'elenco delle radici e la barra della scansione.
 *
 * `progress-sheen` — il riflesso che scorre — vive solo qui: sta nel gruppo
 * «Lettore» del registro ma l'applicazione lo usa nelle barre di Impostazioni.
 */
function Cartelle() {
  return (
    <div className="dentro">
      <div className="impostazioni">
        <Indice sezione="cartelle" />
        <div className="corpo">
          <Scheda
            icona="i-folder"
            titolo={t("settings.roots.title")}
            nota={t("settings.roots.note")}
          >
            <ul className="cartelle">
              {["D:\\Musica", "D:\\Archivio\\Live"].map((via) => (
                <li className="cartella" key={via}>
                  <span className="percorso">{via}</span>
                  <button type="button" className="tasto icon-btn">
                    <Icona nome="i-x" dim={14} />
                  </button>
                </li>
              ))}
            </ul>
            <div className="azioni">
              <button type="button" className="bottone primario btn-accent">
                {t("studio.mock.addFolder")}
              </button>
              <button type="button" className="bottone btn-ghost">
                {t("studio.mock.rescan")}
              </button>
            </div>
            <div className="barra player-progress">
              <div className="riempita" style={{ width: "62%" }}>
                <span className="riflesso progress-sheen" aria-hidden="true" />
              </div>
            </div>
            <p className="nota">{t("studio.mock.scanProgress")}</p>
          </Scheda>

          <Scheda icona="i-play" titolo={t("settings.section.riproduzione")}>
            <Interruttore
              etichetta={t("studio.mock.normalize")}
              spiegazione={t("studio.mock.normalizeHint")}
              acceso
            />
            <Interruttore
              etichetta={t("studio.mock.gapless")}
              spiegazione={t("studio.mock.gaplessHint")}
              acceso={false}
            />
            <div className="azioni">
              <Segmentato
                etichetta={t("studio.mock.audioOut")}
                scelta="condivisa"
                onScegli={niente}
                classe="minuto"
                voci={[
                  { chiave: "condivisa", etichetta: t("studio.mock.shared") },
                  { chiave: "esclusiva", etichetta: t("studio.mock.exclusive") },
                ]}
              />
              <button type="button" className="bottone btn-ghost">
                {t("studio.mock.restore")}
              </button>
            </div>
          </Scheda>
        </div>
      </div>
    </div>
  );
}

/**
 * Impostazioni › Scrobbling: le cifre, l'interruttore, i campi e i tasti.
 *
 * È la seconda scheda del registro per numero di parti dopo la libreria, e non
 * compariva in nessuna scena: `stat-number` in fila, `switch` col suo
 * `switch-track`, `field-input`, `empty-state` dentro una scheda invece che al
 * posto di una pagina.
 */
function Account() {
  return (
    <div className="dentro">
      <div className="impostazioni">
        <Indice sezione="scrobbling" />
        <div className="corpo">
          <Scheda icona="i-cloud" titolo="ListenBrainz">
            <Interruttore
              etichetta={t("scrobble.toggle")}
              spiegazione={t("studio.mock.scrobbleHint")}
              acceso
            />
            <Numeri
              voci={[
                [t("studio.mock.sent"), "8 412"],
                [t("studio.mock.waiting"), "3"],
              ]}
            />
            <div className="azioni">
              <input
                className="campo field-input"
                type="text"
                readOnly
                value="lb_••••••••••••"
                aria-label={t("studio.mock.token")}
              />
              <button type="button" className="bottone btn-ghost">
                {t("studio.mock.connect")}
              </button>
            </div>
          </Scheda>

          <Scheda icona="i-cloud" titolo="Last.fm">
            <p className="nota">{t("studio.mock.lastfmNote")}</p>
            <div className="azioni">
              <button type="button" className="bottone btn-ghost">
                {t("studio.mock.connect")}
              </button>
            </div>
          </Scheda>

          <Scheda icona="i-list" titolo={t("settings.section.esterno")}>
            <p className="niente empty-state">{t("studio.mock.noImports")}</p>
          </Scheda>
        </div>
      </div>
    </div>
  );
}

// ── le importazioni ─────────────────────────────────────────────────────────

/**
 * La pagina delle importazioni: la coda che scende e la storia sotto.
 *
 * Le sue righe sono la seconda forma di `list-row` — fitta, senza copertina,
 * con una barra dentro — e una skin che accorda l'elenco della libreria e non
 * guarda questa scopre poi che qui la riga è alta la metà.
 */
function Importazioni() {
  return (
    <div className="dentro">
      <div className="importazioni-pagina">
        <div className="riepilogo-coda">
          <span className="stat-number">3</span>
          <div className="barra player-progress">
            <div className="riempita" style={{ width: "44%" }}>
              <span className="riflesso progress-sheen" aria-hidden="true" />
            </div>
          </div>
          <span className="stato">{t("studio.mock.importing")}</span>
        </div>

        {/* Titolo, genere, conteggio, barra e tasti stanno **dentro**
            `.che-cosa`, che è la riga: fuori si impilerebbero uno sotto
            l'altro, che è quel che succedeva prima di guardarla. */}
        <ul className="elenco-importazioni">
          {[
            ["Anna Vestri — Le stanze basse", "album", 8, 11, false],
            ["Raccolta d'inverno", "playlist", 12, 27, false],
            ["Controluce (live)", "brano", 1, 1, true],
          ].map(([titolo, genere, fatti, totale, conclusa]) => (
            <li
              key={String(titolo)}
              className="importazione"
              data-conclusa={conclusa === true || undefined}
            >
              <div className="che-cosa">
                <span className="titolo">{titolo}</span>
                <span className="genere">{genere}</span>
                <span className="provenienza">archive.org</span>
                <span className="conteggio">
                  {fatti} / {totale}
                </span>
                <div className="barra" role="progressbar">
                  <div
                    className="riempimento"
                    style={{
                      width: `${Math.round((Number(fatti) / Number(totale)) * 100)}%`,
                    }}
                  />
                </div>
                {conclusa === true && (
                  <>
                    <button type="button" className="bottone minuto btn-ghost">
                      {t("imports.row.openReport")}
                    </button>
                    <button type="button" className="tasto icon-btn">
                      <Icona nome="i-x" dim={12} />
                    </button>
                  </>
                )}
              </div>
              <div className="note">
                <span>{t("studio.mock.alreadyIn")}</span>
              </div>
            </li>
          ))}
        </ul>

        <ul className="elenco-importazioni magro">
          {["Ore piccole", "Sale", "Quattro stanze"].map((titolo) => (
            <li key={titolo} className="list-row">
              <span className="titolo">{titolo}</span>
              <span className="genere">album</span>
              <span className="conteggio">11</span>
              <button type="button" className="bottone minuto btn-ghost">
                {t("studio.mock.again")}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

// ── «In riproduzione», a schermo intero ─────────────────────────────────────

/**
 * La schermata grande, copiata da `schermate/InRiproduzione`.
 *
 * Cinque parti vivevano solo qui e non comparivano in nessuna scena —
 * `np-screen`, `np-scrim` e la copertina piena su tutte — e la tabella
 * `DOVE_SI_VEDE` mandava a cercarle nella terza colonna, dove non sono mai
 * state. Adesso è una pagina sua, e `grande` nel contesto finto è vero: lo
 * scafale toglie di mezzo lettore, colonna e coda esattamente come nell'app,
 * perché è lo stesso predicato a decidere.
 *
 * Lo spettro non c'è, e qui la ragione è la stessa che vale per il lettore e per
 * la coda: **una** scena per volta. La tela è cara — un contesto WebGL2, un
 * programma, una texture di mezzo minuto di storia — e montarla due volte
 * vorrebbe dire pagarla due volte per mostrarla in due riquadri di cui uno solo
 * si guarda. Sta nella pagina dei pannelli, dove si muove: là il riquadro è
 * grande abbastanza da giudicare un colore, e là accanto ci sono il testo e
 * l'equalizzatore, cioè le altre due superfici che si accordano con lei.
 */
function Schermo({ ctx }: { ctx: ContestoWidget }) {
  const brano = ctx.stato.brano;
  if (!brano) return null;
  return (
    <section className="np-screen in-riproduzione">
      <div className="ambiente ambient-backdrop" aria-hidden="true" />
      <div className="np-scrim" aria-hidden="true" />

      <header className="testa">
        <div className="chi-suona">
          <div className="occhiello hero-eyebrow">{t("np.nowPlaying")}</div>
        </div>
        <div className="comandi">
          <button type="button" className="tasto icon-btn">
            <Icona nome="i-text" dim={16} />
          </button>
          <button type="button" className="tasto icon-btn">
            <Icona nome="i-eq" dim={16} />
          </button>
          <button type="button" className="tasto icon-btn">
            <Icona nome="i-queue" dim={16} />
          </button>
          <button type="button" className="pillola btn-ghost">
            {t("common.close")}
            <Icona nome="i-chev-d" dim={13} />
          </button>
        </div>
      </header>

      <div className="corpo" data-con-coda>
        <div className="centro">
          <div className="sommario-np">
            <Copertina
              hash={brano.coverArtHash}
              titolo={brano.album}
              classe="np-art"
              piena
            />
            <div className="chi">
              <h1 className="np-title">{brano.title}</h1>
              <div className="np-meta">
                {brano.artist} · {brano.album} · {brano.year}
              </div>
            </div>
          </div>

          <div className="comandi-np">
            <Scrubber stato={ctx.stato} onErrore={niente} />
            <Trasporto stato={ctx.stato} taglia="grande" onErrore={niente} />
            <Giudizio
              stato={ctx.stato}
              brano={brano}
              taglia="grande"
              onPreferito={niente}
              onVoto={niente}
              onErrore={niente}
            />
          </div>
        </div>

        <aside className="coda-np">
          <header>
            <span className="occhiello hero-eyebrow">
              {t("np.queued", { n: 3 })}
            </span>
            <button type="button" className="bottone minuto btn-ghost">
              {t("queue.clear")}
            </button>
          </header>
          <ol className="righe-coda queue-list" data-compatta>
            {RIGHE.slice(0, 3).map((riga, i) => (
              <li
                key={riga.titolo}
                className="riga-coda list-row"
                data-active={i === 0 || undefined}
              >
                <span className="presa" aria-hidden="true">
                  <Icona nome="i-grip" dim={13} />
                </span>
                <span className="nome">{riga.titolo}</span>
                <span className="autore">{riga.autore}</span>
                <span className="durata">{riga.durata}</span>
              </li>
            ))}
          </ol>
        </aside>
      </div>
    </section>
  );
}

// ── i tre pannelli grandi ───────────────────────────────────────────────────

/**
 * Testo, spettro ed equalizzatore, affiancati — e quello di mezzo si muove.
 *
 * Nell'applicazione si accendono uno per volta dentro la schermata grande, e
 * durano quanto una canzone. Qui stanno insieme perché per ridipingerli bisogna
 * vederli, e uno alla volta vorrebbe dire tre pagine per tre pannelli — con in
 * più il fatto che due dei tre, spenti, non esistono affatto.
 *
 * # Il foglio in prova, portato accanto alla tela
 *
 * `foglio` non si disegna: è il CSS della skin in prova, e serve a **datarlo**.
 * La tela legge i suoi diciotto token da una `CSSStyleDeclaration` viva, ma li
 * rilegge solo quando qualcosa le dice che sono cambiati, e uno dei segnali che
 * ascolta è la comparsa o la riscrittura di un `<style>` dentro il proprio
 * genitore. Nello Studio quel foglio esiste — `Anteprima` lo inietta a ogni
 * validazione — ma sta in `.anteprima-guscio`, cioè **fuori** dal genitore della
 * tela di quattro livelli: l'osservatore non lo vede, e trascinare un cursore si
 * vedrebbe sulla scena solo alla prossima passata della rete di sicurezza, mezzo
 * secondo dopo. Mezzo secondo di ritardo su un cursore che si trascina non è un
 * ritardo: è un controllo che sembra rotto.
 *
 * Quindi accanto alla tela va un foglio suo, che non dipinge niente e porta un
 * numero: cambia esattamente quando cambia quello vero, e cambiando fa scattare
 * l'osservatore che sta già lì. Le due alternative erano peggiori — allargare
 * l'osservatore a `document` con `subtree` vuol dire farlo scattare a ogni riga
 * di ogni lista che si ridisegna, cioè il costo che tutto questo lavoro è
 * servito a togliere; e ricopiare qui il foglio vero vorrebbe dire far analizzare
 * al browser due volte qualche decina di kilobyte a ogni battuta di tasto.
 */
function Pannelli({ foglio }: { foglio: string }) {
  /*
   * La revisione del foglio: quante volte è cambiato da quando questa pagina è
   * aperta. Un contatore e non il testo, perché è il testo del `<style>` a
   * dover cambiare e non a dover dire qualcosa — e un numero che cresce cambia
   * sempre, mentre la lunghezza del foglio no: portare un cursore da 0,42 a
   * 0,43 riscrive il CSS senza spostarne un carattere.
   */
  const revisione = useRef({ foglio, n: 0 });
  if (revisione.current.foglio !== foglio) {
    revisione.current = { foglio, n: revisione.current.n + 1 };
  }

  return (
    <div className="dentro tre-schermate">
      <aside className="testo-np lyrics-screen">
        <header>
          <span className="occhiello hero-eyebrow">{t("np.lyrics")}</span>
        </header>
        <div className="righe-testo">
          {["Le stanze basse", "hanno una luce", "che non si accende"].map(
            (riga, i) => (
              <div
                key={riga}
                className="lyric-line"
                /* I due attributi dell'essere accesa: `data-active` lo cerca il
                   registro delle parti, `data-attiva` lo cerca lo stile di casa.
                   Stanno insieme anche nel componente vero, e la scena serve a
                   poco se mostra una riga accesa in un modo che nell'app non
                   esiste. */
                data-active={i === 1 || undefined}
                data-attiva={i === 1 || undefined}
                /* E quella di prima è passata: le tre righe erano tutte future
                   tranne quella accesa, cioè mancava uno dei tre stati che una
                   riga ha davvero. */
                data-passata={i === 0 || undefined}
                /* La riga accesa si mostra a parole, che è come si vede quando
                   l'LRC porta i tempi delle parole. Le altre due no: una scena
                   che mostra solo il caso raro insegnerebbe a ridipingere quello
                   e a dimenticare l'altro, che è quello di quasi tutti i file. */
                data-parole={i === 1 || undefined}
              >
                {i === 1 ? (
                  riga.split(/(?=\s)/).map((parola, quale) => (
                    <span
                      key={quale}
                      className="lyric-word"
                      data-active={quale === 0 || undefined}
                      /* Ferme: nello Studio non c'è una canzone che scorra, e
                         un'animazione qui renderebbe impossibile giudicare un
                         colore. Il fronte si vede lo stesso, a metà parola. */
                      style={
                        { "--avanzamento": quale === 0 ? 0.6 : 0 } as CSSProperties
                      }
                    >
                      {parola}
                    </span>
                  ))
                ) : (
                  /* L'involucro che porta il fronte. Non è una parte dichiarata
                     — una skin non lo nomina — ma è dove il gradiente vive
                     davvero, e una scena che mostrasse il testo senza di lui
                     mostrerebbe una riga che nell'app non esiste. */
                  <span className="lyric-fill">{riga}</span>
                )}
              </div>
            ),
          )}

          {/* Lo stacco: quel che prende il posto della riga quando l'LRC ha un
              tempo ma non ha parole — l'introduzione, o il vuoto fra due strofe.
              Porta gli stessi due attributi dell'essere accesa perché
              nell'applicazione compare **al posto** della riga accesa: una skin
              che ridipinge `lyric-line` ridipinge anche i puntini, che si
              colorano da `currentColor`.

              Fermo a metà, come le parole qui sopra: con `--avanzamento` a 0.5 i
              tre puntini mostrano in una volta i tre stati che attraversano —
              pieno, a metà, spento. */}
          <div
            className="lyric-line"
            data-active
            data-attiva
            data-stacco
            style={{ "--avanzamento": 0.5 } as CSSProperties}
          >
            <span className="lyric-breath">
              <span />
              <span />
              <span />
            </span>
          </div>
        </div>
      </aside>

      <div className="pannello-finto">
        <span className="occhiello hero-eyebrow">{t("np.spectrum")}</span>
        {/* Il foglio che non dipinge: vedi il saggio qui sopra. Sta prima della
            tela e non dopo perché l'osservatore guarda il genitore intero, e
            l'ordine dei fratelli non gli dice niente — ma a chi legge sì: la
            datazione arriva prima della cosa datata. */}
        <style>{`.pannello-finto{--revisione-anteprima:${revisione.current.n}}`}</style>
        {/* La scena vera, che si muove.

            Per una revisione qui c'era una `<canvas>` vuota, e il commento
            diceva che la scena non si poteva accendere «perché non c'è audio».
            Era vero a metà: senza audio resta la **sorgente**, non il renderer,
            e la dottrina di questo file — scritta in cima — è che dove il
            componente si può montare non lo si copia, lo si usa. `sorgente
            finto` è esattamente questo: nessuna presa nel motore, nessun evento
            ascoltato, e un generatore deterministico che scrive nello stesso
            posto in cui scriverebbero le bande vere. Tutto quel che viene dopo —
            smorzamento, cascata, texture, seguipulsazioni, e la lettura dei
            diciotto token — è la stessa strada, quindi quel che si vede qui è
            quel che si vedrà là.

            E siccome è la scena vera, la superficie sotto si accorda insieme al
            resto invece che da sola: fondo, raggio e contorno di `viz-screen` si
            giudicano con dentro l'immagine che ci starà davvero. */}
        <Spettro3D
          barre={BARRE_ANTEPRIMA}
          /* Nello Studio non suona niente, ed è la verità che questa prop
             chiede. È anche inerte: con la sorgente sintetica il rubinetto non
             si apre mai — `inPausa` decide solo se chiuderlo — quindi il valore
             non cambia un fotogramma. Passare `false` direbbe una cosa falsa in
             cambio di niente. */
          inPausa
          /* «Alta», non «automatica». L'automatica misura questa macchina e
             taglia quel che costa: prima la densità dei pixel, poi il riflesso.
             Su un portatile lento vorrebbe dire che il riflesso si spegne da sé
             **mentre** qualcuno trascina `canvas.viz.reflection` — cioè che si
             accorda un numero guardando un'immagine che il controllore ha
             cambiato sotto, e che l'autore attribuirebbe al proprio cursore.
             Uno strumento di disegno vuole tetti fissi, per la stessa ragione
             per cui il generatore è in forma chiusa sul numero dell'evento: due
             autori su due macchine devono vedere la stessa immagine, o una
             schermata non è un documento di revisione. Il costo è un riquadro
             piccolo dentro una miniatura, non uno sfondo a tutto schermo. */
          qualita="alta"
          sorgente="finto"
          onErrore={guastoDellaScena}
        />
      </div>

      {/* L'equalizzatore intero e non le sole bande: l'altezza dei cursori
          verticali è `--eq-h`, e quella variabile vive su `.equalizzatore`.
          Prendendo solo `.eq-bande` i sette cursori restavano alti zero — cioè
          la parte che si voleva guardare non c'era. */}
      <div className="pannello-finto">
        <span className="occhiello hero-eyebrow">{t("player.eq")}</span>
        <div className="equalizzatore" data-taglia="pagina">
          <div className="eq-testa">
            <button
              type="button"
              className="interruttore switch"
              role="switch"
              aria-checked
              aria-label={t("player.eq")}
            >
              <span className="pista switch-track" aria-hidden="true">
                <span className="pallina" />
              </span>
            </button>
            <select
              className="eq-preset"
              value=""
              onChange={niente}
              aria-label={t("eq.curve")}
            >
              <option value="">{t("eq.curve")}</option>
            </select>
            <button type="button" className="bottone minuto btn-ghost">
              {t("eq.reset")}
            </button>
          </div>
          <div className="eq-bande eq-bars">
            {[60, 150, 400, 1000, 2400, 6000, 12000].map((hz, i) => (
              <label key={hz} className="eq-banda">
                <span className="eq-db stat-number">
                  {[0, 2, 3, 1, 0, -2, -1][i] ?? 0}
                </span>
                <input
                  type="range"
                  className="eq-cursore eq-slider"
                  min={-12}
                  max={12}
                  step={1}
                  readOnly
                  value={[0, 2, 3, 1, 0, -2, -1][i] ?? 0}
                />
                <span className="eq-hz">{hz < 1000 ? hz : `${hz / 1000}k`}</span>
              </label>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

// ── le sovrapposizioni ──────────────────────────────────────────────────────

/**
 * Quel che sta sopra la pagina, e nell'app sta fuori dall'albero.
 *
 * Vanno rese **accanto** a `<Impaginazione>` e non dentro il buco del
 * contenuto, che è dove stavano prima: il menù, la notifica e la finestrella
 * sono `position: fixed` e si riferiscono alla finestra, e ficcarle nel
 * contenuto voleva dire mostrarle in un posto in cui nell'applicazione non
 * compaiono mai. Nel riquadro dell'anteprima `fixed` si riferisce al riquadro,
 * perché `.anteprima` porta `contain: layout paint` — ed è la stessa ragione per
 * cui la barra del lettore non scappa in fondo allo Studio.
 *
 * Restano immobili, che è l'unico modo di poterle ridipingere guardandole.
 */
export function Sovrapposte({ accese }: { accese: Accese }) {
  return (
    <>
      {accese.has("menu") && (
        <>
          <div className="velo" />
          <div className="menu menu-pop" style={{ left: "34%", top: "38%" }}>
            {[
              t("studio.mock.play"),
              t("studio.mock.queue"),
              t("studio.mock.goToAlbum"),
              t("studio.mock.favourite"),
            ].map((voce) => (
              <button key={voce} type="button">
                {voce}
              </button>
            ))}
          </div>
        </>
      )}

      {accese.has("avviso") && (
        <div className="pila-toast">
          <div className="toast toast-card" role="status">
            <Icona nome="i-scan" dim={16} />
            <div className="dentro">
              <div className="cosa">{t("studio.mock.scanProgress")}</div>
              <div className="toast-progress">
                <span style={{ width: "62%" }} />
              </div>
            </div>
            <button type="button" className="bottone minuto btn-ghost">
              {t("toast.open")}
            </button>
          </div>
        </div>
      )}

      {accese.has("dialogo") && (
        <div className="velo scuro">
          <div className="finestrella stretta glass-modal">
            <h2>{t("studio.mock.dialogTitle")}</h2>
            <p>{t("studio.mock.dialogBody")}</p>
            <input
              className="campo field-input"
              type="text"
              readOnly
              value="Ascolti d'inverno"
              aria-label={t("studio.mock.dialogTitle")}
            />
            <div className="tasti-finestrella">
              <button type="button" className="bottone btn-ghost">
                {t("common.cancel")}
              </button>
              <button type="button" className="bottone primario btn-accent">
                {t("studio.mock.create")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Il giro guidato, cioè l'unico posto in cui si può ridipingere
          `tour-tooltip` guardandola.

          Le misure sono in percentuale e non misurate, ed è una differenza che
          conta dirla: nell'applicazione il buco viene da un
          `getBoundingClientRect` sull'elemento vero e il fumetto da un calcolo
          che sceglie il lato dove ci sta, mentre qui sono un riflettore
          plausibile sulla barra del lettore e un fumetto sopra. Quel che si
          ridipinge — il fumetto, il colore dell'ombra, l'anello sul bersaglio —
          è identico; quel che qui non si può provare è **dove** il fumetto va a
          finire, perché dipende dalla finestra vera.

          Il testo è quello del quarto passo e non un finto: le dieci
          spiegazioni sono lunghe tre righe, e una skin che stringe il fumetto o
          ne alza il corpo del testo deve poterlo vedere su una lunghezza
          vera. */}
      {accese.has("giro") && (
        <>
          <div className="giro-velo">
            <div
              className="giro-buco"
              style={{
                top: "70%",
                left: "24%",
                width: "52%",
                height: "16%",
                borderRadius: "16px",
              }}
            />
          </div>
          <div
            className="giro-fumetto tour-tooltip"
            style={{ top: "22%", left: "26%" }}
          >
            <div className="giro-testa">
              <h2>{t("tour.step.trasporto.title")}</h2>
              <span className="giro-passi">
                {t("tour.progress", { n: 4, totale: 10 })}
              </span>
            </div>
            <p>{t("tour.step.trasporto.body")}</p>
            <div className="giro-tasti">
              <button type="button" className="bottone minuto btn-ghost">
                {t("tour.skip")}
              </button>
              <button type="button" className="bottone">
                {t("tour.back")}
              </button>
              <button type="button" className="bottone primario btn-accent">
                {t("tour.next")}
              </button>
            </div>
          </div>
        </>
      )}
    </>
  );
}

// ── il buco del contenuto ───────────────────────────────────────────────────

/**
 * I due slot che `Impaginazione` lascia aperti, per una pagina.
 *
 * L'intestazione e il corpo insieme, perché è la stessa pagina a decidere tutti
 * e due. `query` e `ordinamento` si passano dove l'app li passa: sono l'unico
 * posto in cui vivono `field-input` e uno dei `btn-ghost`, e non passandoli
 * l'anteprima mostrava una testata che nell'applicazione non esiste — senza
 * ricerca, cioè senza il campo di testo che una skin deve poter ridipingere.
 *
 * `foglio` è il CSS della skin in prova, e attraversa di qui per una pagina
 * sola: la scena dello spettro se ne serve per sapere **quando** rileggere i
 * suoi token, e il perché sta scritto sopra `Pannelli`. Passa come argomento e
 * non per contesto perché è quel che fanno già gli altri due, e un terzo
 * meccanismo per il terzo dato sarebbe un meccanismo di troppo.
 */
export function slotDellaPagina(
  pagina: Pagina,
  ctx: ContestoWidget,
  foglio: string,
): SlotScafale {
  const cercabile = {
    query: "",
    onQuery: niente,
  };

  switch (pagina) {
    case "pagina":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("page.album")}
            titolo="Le stanze basse"
            sottotitolo="Anna Vestri · 2019 · 11 brani"
            copertina={
              <Copertina
                hash={null}
                titolo="Le stanze basse"
                classe="hero-art"
                piena
              />
            }
            azioni={
              <>
                <button type="button" className="pillola btn-ghost">
                  <Icona nome="i-chev-l" dim={14} />
                  {t("page.album")}
                </button>
                <button type="button" className="pillola btn-accent">
                  <Icona nome="i-play" dim={14} />
                  {t("studio.mock.play")}
                </button>
              </>
            }
            {...cercabile}
          />
        ),
        contenuto: <Album />,
      };

    case "impostazioni":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("studio.mock.settings")}
            titolo={t("settings.section.cartelle")}
            sottotitolo={t("studio.mock.settingsSub")}
          />
        ),
        contenuto: <Cartelle />,
      };

    case "account":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("studio.mock.settings")}
            titolo={t("settings.section.scrobbling")}
            sottotitolo={t("studio.mock.accountSub")}
          />
        ),
        contenuto: <Account />,
      };

    case "importazioni":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("studio.mock.library")}
            titolo={t("imports.title")}
            sottotitolo={t("imports.sub.running", { n: 3 })}
          />
        ),
        contenuto: <Importazioni />,
      };

    case "schermo":
      // L'intestazione non si disegna: `grande` la toglie dallo scafale, come
      // nell'app. Resta qui perché lo slot è un contratto a due caselle, e una
      // casella vuota è più chiara di una casella assente.
      return { intestazione: null, contenuto: <Schermo ctx={ctx} /> };

    case "pannelli":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("studio.mock.nowPlaying")}
            titolo="Corale in mi minore"
            sottotitolo="Anna Vestri"
          />
        ),
        contenuto: <Pannelli foglio={foglio} />,
      };

    case "vuoto":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("studio.mock.library")}
            titolo={t("nav.albums")}
            sottotitolo={t("studio.mock.nothingIn")}
            {...cercabile}
          />
        ),
        contenuto: <Vuoto />,
      };

    case "caricamento":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("studio.mock.library")}
            titolo={t("nav.albums")}
            sottotitolo={t("studio.mock.reading")}
            {...cercabile}
          />
        ),
        contenuto: <Finta />,
      };

    default:
      return {
        intestazione: (
          <Intestazione
            occhiello={t("studio.mock.library")}
            titolo={t("nav.albums")}
            sottotitolo={t("studio.mock.inLibrary")}
            ordinamento={{ etichetta: t("studio.mock.byArtist"), onApri: niente }}
            {...cercabile}
          />
        ),
        contenuto: <Griglia />,
      };
  }
}

// ── dove si vede una parte ──────────────────────────────────────────────────

/**
 * Le parti che il registro dichiara e **nessuna** schermata disegna.
 *
 * Non sono un difetto di questo file: sono posti decisi e non ancora
 * costruiti. Contano qui perché lo Studio deve saper dire la differenza fra «la
 * tua skin non funziona», «quella parte sta in un'altra scena» e «quella parte
 * non la disegna ancora nessuno» — tre risposte che prima erano una sola, cioè
 * lo schermo che non cambia.
 *
 * Deve restare uguale ad `ATTESE` in `strumenti/classi.js`, che è il controllo
 * che gira in CI; il testo del perché sta là, in italiano lungo, e qui c'è la
 * chiave di traduzione da mostrare.
 */
export const NON_ANCORA: Readonly<Record<string, Chiave>> = {
  // Ed è vuoto: **nessuna** parte del registro è più senza un posto dove si
  // vede. Vale la pena tenerlo, vuoto, con le quattro ragioni scritte sotto:
  // il valore di questa tabella non è la lista, è la terza risposta che rende
  // possibile — «non la disegna ancora nessuno», accanto a «la tua skin non
  // funziona» e «sta in un'altra scena» — e quella risposta deve restare
  // disponibile per la prossima parte che arriva prima del suo markup.
  //
  // Quattro voci sono uscite di qui, e per quattro ragioni diverse. Tutte e
  // quattro nello stesso passo in cui uscivano da `ATTESE` di
  // `strumenti/classi.js`: i due elenchi devono dire la stessa cosa, e il
  // controllo 3 lo verifica.
  //
  // `tour-tooltip`: il giro guidato è arrivato. `Giro.tsx` monta il fumetto
  // come `.giro-fumetto.tour-tooltip`, quindi la parte si vede — e qui non
  // bastava toglierla dall'elenco, perché «sta in un'altra scena» avrebbe
  // rimandato a una scena che non la disegnava. L'interruttore «giro» in
  // `finto.ts` e il velo qui sotto sono l'altra metà di questa riga.
  //
  // `tooltip-pill`: adesso l'applicazione la disegna — sono le linguette spente
  // del pannello dei comandi, ognuna nel guscio `.con-ragione` — e una parte
  // che si vede non è «non ancora costruita».
  //
  // `app-shell`: la disegnava già, e questo elenco diceva il falso. La radice
  // dell'albero di serie porta `.parte("app-shell")` in
  // `aether-skin::layout::default_shell`, su tutti e due i rami, e
  // `Impaginazione.tsx` mette `nodo.part` in classe. Non si vedeva perché il
  // controllo leggeva solo i `className` del TypeScript; adesso legge anche i
  // letterali di `layout.rs`.
  //
  // `home-shortcuts`: non è arrivata, è **ritirata** — sta in
  // `aether_skin::parts::RITIRATE`. Era l'unica delle quattro attese senza un
  // proprietario, e un'attesa che nessun pacchetto ha preso in carico è una
  // promessa che questa vista non può mantenere: dire «non ancora» per sempre
  // è la peggiore delle due risposte sbagliate.
};

/**
 * Perché questa parte non si può vedere in nessuna scena, se è così.
 *
 * Sta qui e non in `Studio.tsx` perché la tabella sta qui, e perché la chiave di
 * traduzione non deve viaggiare: chi chiama riceve la frase già in italiano — o
 * in inglese — e non un identificatore da risolvere.
 */
export function perchePartMai(parte: string): string | null {
  const chiave = NON_ANCORA[parte];
  return chiave === undefined ? null : t(chiave);
}

/** Dove si va a vedere una parte: una pagina, e/o degli interruttori. */
export interface Dove {
  readonly pagina?: Pagina;
  readonly accendi?: readonly Sovrapposizione[];
}

/**
 * In quale scena si vede una parte.
 *
 * Serve a «portami dove si vede»: si sceglie una parte nell'albero, non compare
 * nella scena aperta, e questa tabella dice dove cercarla. È scritta a mano
 * perché la risposta vera — «in quali scene esiste un elemento con questa
 * classe» — si può solo misurare disegnandole tutte, e disegnarle tutte per
 * rispondere a una domanda è più caro della domanda.
 *
 * Le parti che non ci sono e non stanno in `NON_ANCORA` sono nella cornice, e la
 * cornice c'è sempre: per quelle la risposta giusta è «resta dove sei, e guarda
 * che l'albero la monti».
 */
export const DOVE_SI_VEDE: Readonly<Record<string, Dove>> = {
  // ── la libreria ───────────────────────────────────────────────────────────
  "track-grid": { pagina: "libreria" },
  "list-row": { pagina: "libreria" },
  skeleton: { pagina: "caricamento" },
  "empty-state": { pagina: "vuoto" },
  "empty-icon": { pagina: "vuoto" },
  "hero-art": { pagina: "pagina" },

  // ── le impostazioni ───────────────────────────────────────────────────────
  "section-card": { pagina: "impostazioni" },
  "section-heading": { pagina: "impostazioni" },
  "section-icon": { pagina: "impostazioni" },
  switch: { pagina: "impostazioni" },
  "switch-track": { pagina: "impostazioni" },
  segmented: { pagina: "impostazioni" },
  "btn-accent": { pagina: "impostazioni" },
  "progress-sheen": { pagina: "impostazioni" },
  "stat-number": { pagina: "account" },
  "field-input": { pagina: "account" },

  // ── in riproduzione ───────────────────────────────────────────────────────
  "np-screen": { pagina: "schermo" },
  "np-art": { pagina: "schermo" },
  "np-scrim": { pagina: "schermo" },
  "np-title": { pagina: "schermo" },
  "np-meta": { pagina: "schermo" },
  "lyrics-screen": { pagina: "pannelli" },
  "lyric-line": { pagina: "pannelli" },
  "lyric-word": { pagina: "pannelli" },
  "viz-screen": { pagina: "pannelli" },
  "eq-bars": { pagina: "pannelli" },
  "eq-slider": { pagina: "pannelli" },

  // ── la cornice, dove può mancare ──────────────────────────────────────────
  //
  // Sono parti che l'albero di serie monta sempre, e che spariscono lo stesso
  // in due casi: sulle pagine senza un brano che suoni — il lettore e tutto
  // quel che contiene non rendono — e a schermo intero, dove l'intestazione si
  // toglie di mezzo. Senza queste righe la risposta era «sta nella cornice,
  // aspetta che l'albero la monti», cioè un'attesa che su quelle pagine non
  // finisce: l'albero la montava già.
  //
  // Le due che restano fuori sono `ambient-backdrop` e `bottom-nav`, e per
  // loro quella risposta è quella giusta: l'albero di serie non le monta, e
  // compaiono davvero solo quando una skin le mette.
  "page-header": { pagina: "libreria" },
  "page-title": { pagina: "libreria" },
  "page-subtitle": { pagina: "libreria" },
  "hero-eyebrow": { pagina: "libreria" },
  "player-shell": { pagina: "libreria" },
  "player-progress": { pagina: "libreria" },
  "np-transport": { pagina: "libreria" },
  "play-btn-primary": { pagina: "libreria" },
  "range-accent": { pagina: "libreria" },
  "icon-btn": { pagina: "libreria" },
  "btn-ghost": { pagina: "impostazioni" },

  // ── quel che si accende ───────────────────────────────────────────────────
  "queue-list": { accendi: ["coda"] },
  "glass-modal": { accendi: ["dialogo"] },
  "selection-bar": { accendi: ["selezione"] },
  "menu-pop": { accendi: ["menu"] },
  "toast-card": { accendi: ["avviso"] },
  "toast-progress": { accendi: ["avviso"] },
  // Il fumetto del giro. Stava in `NON_ANCORA` — «non la disegna ancora
  // nessuno» — e adesso ha un posto vero: l'interruttore la accende su
  // qualunque pagina, perché nell'applicazione il giro attraversa dieci
  // schermate e non ne ha una sua.
  "tour-tooltip": { accendi: ["giro"] },
};
