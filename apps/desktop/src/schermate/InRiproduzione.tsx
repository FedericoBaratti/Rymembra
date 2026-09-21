/**
 * In riproduzione, a schermo quasi intero.
 *
 * # Perché «quasi»
 *
 * La barra di navigazione **resta**. Coprire tutta la finestra sembra la scelta
 * ovvia — è quel che fanno quasi tutti i lettori — e costa una cosa che si paga
 * ogni volta: per cambiare vista bisogna prima chiudere. Con la barra al suo
 * posto, guardare la copertina grande e passare agli Album sono due gesti
 * indipendenti, e nessuno dei due annulla l'altro.
 *
 * # Lo scrim è piatto, e non è un dettaglio
 *
 * Dietro c'è l'ambiente: due sfumature radiali che prendono il colore dalla
 * copertina. Sopra c'è `np-scrim`, che **non** è colorato — è un nero verticale
 * dal 42% all'80%, scritto con `--surface-0-rgb` così che una skin chiara ne
 * ottenga uno bianco.
 *
 * Sono due strati e non uno perché fare dello scrim una versione più scura
 * della tinta vorrebbe dire che il testo sta sopra un colore imprevedibile: con
 * una copertina gialla il titolo si leggerebbe su giallo scuro, che è ancora
 * giallo. Con due strati il testo sta sopra un nero neutro, e la tinta si vede
 * intorno.
 *
 * # E si chiama `np-scrim`, non «velo»
 *
 * Le ha portate tutte e due, e per un po' non è costato niente. Poi `.velo` ha
 * preso lo strato che gli spetta — un velo è la superficie che prende il clic
 * per chiudere, e deve stare davanti a quel che chiude — e questo, che non
 * chiude niente, si è ritrovato davanti alla schermata: slavata dal suo stesso
 * gradiente e sorda a ogni clic, e senza una riga nei log, perché un
 * `aria-hidden` senza gestori che intercetta tutto non ha niente da raccontare.
 *
 * Il nome che resta è quello del registro delle skin: dice cos'è a chi
 * ridipinge, e non promette niente a chi clicca.
 *
 * # Dove stanno le cose, e perché dipende
 *
 * I comandi si appoggiano **in fondo**, sempre. Il sommario — copertina, titolo,
 * artista — cambia posto con lo spettro, ed è l'unica cosa qui che si muove.
 *
 * Acceso lo spettro, si ritira **in alto a sinistra** e la copertina si fa
 * piccola: la fascia centrale è quella in cui la scena si legge meglio, e il
 * sommario in mezzo la coprirebbe proprio lì. Spento — che è come si apre
 * finché nessuno ha scelto il contrario — quel vuoto non lo guarda nessuno, e
 * tenerlo libero per una scena che non c'è
 * vorrebbe dire mostrare la copertina in un angolo con mezzo schermo intorno.
 * Allora torna **al centro**, e grande: chi apre questa schermata è venuto a
 * guardarla.
 *
 * Una striscia di dieci barre stava sotto il sommario, e non c'è più: diceva in
 * piccolo — dieci barre d'ottava — quel che la scena dice in grande, e copriva
 * la fascia in cui la scena si legge meglio.
 *
 * # Quel che è spento, e resta visibile
 *
 * Il bottone «testo» c'è, spento, e dice perché nel suggerimento: il nucleo non
 * legge ancora i testi, e una schermata vuota che si apre sarebbe peggio di un
 * bottone spento che spiega. Toglierlo del tutto nasconderebbe che il posto per
 * lui c'è ed è deciso.
 *
 * # Testo e coda, quando non ci stanno tutti e due
 *
 * Tre colonne — il centro, il testo, la coda — chiedono più di milleduecento
 * pixel perché il centro resti leggibile. Alla misura di serie della finestra,
 * con la navigazione aperta, ne restano poco più di mille, e il centro scendeva
 * sotto i trecento: il titolo tagliato a metà parola, l'artista una parola per
 * riga, il trasporto sotto il pannello del testo.
 *
 * Sotto quella soglia i due pannelli si **alternano**: aprirne uno chiude
 * l'altro, e se la finestra si stringe con tutti e due aperti resta quello
 * aperto per ultimo. La soglia non è scritta qui: la dice il foglio, con una
 * query di contenitore che accende `--un-pannello-solo` sul corpo, e questo file
 * la legge. È la stessa regola di `parti/BarraTitolo.tsx`, che l'altezza della
 * fascia la legge invece di saperla — e in più qui la soglia dipende dalla
 * densità, che il foglio conosce e questo file no.
 *
 * Chiuso resta chiuso: il pannello tolto per far posto all'altro non torna
 * da solo quando la finestra si riallarga. Riaprirlo da solo vorrebbe dire
 * un pannello che compare mentre si trascina un bordo, senza che nessuno
 * l'abbia chiesto.
 *
 * # Il fuoco
 *
 * La schermata non è una finestrella: la navigazione resta viva accanto, ed è
 * tutto il senso di «quasi». Niente trappola del tabulatore, quindi. Però il
 * fuoco **entra** all'apertura — chi l'ha aperta con la tastiera non deve
 * ritrovarla attraversando la pagina che le sta sotto — e alla chiusura torna
 * dov'era. La pagina sotto, intanto, è `inert` (lo decide `App`): è coperta, e
 * un Tab che ci finisse dentro porterebbe il fuoco su un elenco invisibile.
 */
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";

import { Copertina, Sfocata } from "../Copertina";
import { RigheCoda, useRigheCoda } from "../Coda";
import {
  ipc,
  type Brano,
  type QualitaSpettro,
  type StatoRiproduzione,
} from "../ipc";
import { Attribuzione } from "../parti/Attribuzione";
import { DettaglioSpettro } from "../parti/DettaglioSpettro";
import { Giudizio } from "../parti/Giudizio";
import { Icona } from "../parti/Icone";
import { Scrubber } from "../parti/Scrubber";
import { Spettro3D } from "../parti/Spettro3D";
import { Testo } from "../parti/Testo";
import { Trasporto } from "../parti/Trasporto";
import { t } from "../lingue";
import { nomeArtista, titoloAlbum } from "../formato";

export function InRiproduzione({
  stato,
  onChiudi,
  onPreferito,
  onVoto,
  onErrore,
  avvisi,
}: {
  stato: StatoRiproduzione;
  onChiudi: () => void;
  onPreferito: (brano: Brano) => void;
  onVoto: (brano: Brano, stelle: number) => void;
  onErrore: (e: unknown) => void;
  /**
   * Le notizie da mostrare sopra la schermata: «coda sostituita — Annulla»,
   * l'esito di un gesto, l'errore.
   *
   * Le passa `App` perché la loro casa è in cima alla pagina, e a schermo
   * intero la pagina è coperta e `inert`: un «Annulla» emesso lì non si vedeva
   * e non si poteva premere, e un brano che non si apriva falliva in silenzio.
   */
  avvisi: ReactNode;
}) {
  const [codaVisibile, setCodaVisibile] = useState(true);
  // Spento all'apertura come lo spettro, e per la stessa ragione: chi apre
  // questa schermata è venuto a guardare la copertina. Il testo è una seconda
  // cosa da guardare, e chi la vuole la accende.
  const [testoVisibile, setTestoVisibile] = useState(false);
  // Spento all'apertura, e la ragione non è più «di proposito, ogni volta»: la
  // scelta adesso si ricorda. Sta in `settings` sotto `player.spectrum.visible`,
  // vale `false` per chi non l'ha mai fatta — chi apre questa schermata la prima
  // volta è venuto a guardare la copertina — e viaggia nel profilo, perché
  // «voglio vedere lo spettro» è un gusto di chi ascolta e resta vero su
  // qualunque computer.
  //
  // Si parte comunque da `false` e si semina con un effetto, come per le barre:
  // così un avvio da spento a spento non costa un montaggio e uno smontaggio
  // della scena — cioè un contesto WebGL creato e distrutto per niente.
  const [spettroVisibile, setSpettroVisibile] = useState(false);
  useEffect(() => {
    ipc.spettroVisibile().then(setSpettroVisibile).catch(onErrore);
  }, [onErrore]);
  // Quante barre disegna la scena. Parte da quel che dice il nucleo — la
  // preferenza sta in `settings`, non qui — e si chiede una volta sola,
  // all'apertura: è una lettura da una tabella di chiavi, non da mezza libreria.
  const [barre, setBarre] = useState(64);
  useEffect(() => {
    ipc.spettroBande().then(setBarre).catch(onErrore);
  }, [onErrore]);
  // Quanto la scena può costare a questa macchina. Stessa forma degli altri due:
  // si parte dal valore di serie e si semina con un effetto, perché è una
  // lettura da una tabella di chiavi e non da mezza libreria.
  //
  // Si legge **qui** e non dentro la scena, che pure sarebbe il posto in cui
  // serve: la tela nasce e muore con l'interruttore, e leggerla di là vorrebbe
  // dire una chiamata IPC a ogni accensione dello spettro invece di una a ogni
  // apertura della schermata. La scheda in Impostazioni scrive nel database, e
  // quel che si vede qui si aggiorna alla riapertura — che è il momento in cui
  // la scena rinasce comunque.
  const [qualitaSpettro, setQualitaSpettro] = useState<QualitaSpettro>("auto");
  useEffect(() => {
    ipc.spettroQualita().then(setQualitaSpettro).catch(onErrore);
  }, [onErrore]);
  const scegliBarre = (quante: number) => {
    // Si prende quel che è rimasto e non quel che si è chiesto: fra le potenze
    // di due non c'è niente, e il nucleo porta alla più vicina un numero che
    // non è una di quelle.
    ipc.spettroBandeScegli(quante).then(setBarre).catch(onErrore);
  };
  const righe = useRigheCoda(stato.coda, onErrore);
  const brano = stato.brano;

  // ── un pannello solo, quando non ci stanno ──
  const schermata = useRef<HTMLElement>(null);
  const corpo = useRef<HTMLDivElement>(null);
  const [unPannelloSolo, setUnPannelloSolo] = useState(false);
  // Quale dei due si è aperto per ultimo: è quello che resta quando la finestra
  // si stringe con tutti e due aperti. Si parte dalla coda perché è quella che
  // la schermata apre da sé.
  const ultimoAperto = useRef<"testo" | "coda">("coda");
  useLayoutEffect(() => {
    const nodo = corpo.current;
    if (nodo === null) return;
    const misura = () =>
      setUnPannelloSolo(
        getComputedStyle(nodo).getPropertyValue("--un-pannello-solo").trim() ===
          "1",
      );
    misura();
    const osservatore = new ResizeObserver(misura);
    osservatore.observe(nodo);
    return () => osservatore.disconnect();
  }, []);
  // La finestra si è stretta con tutti e due aperti. Prima della pittura, così
  // il fotogramma con tre colonne schiacciate non si vede.
  useLayoutEffect(() => {
    if (!unPannelloSolo || !testoVisibile || !codaVisibile) return;
    if (ultimoAperto.current === "testo") setCodaVisibile(false);
    else setTestoVisibile(false);
  }, [unPannelloSolo, testoVisibile, codaVisibile]);
  const alternaTesto = () => {
    const apre = !testoVisibile;
    setTestoVisibile(apre);
    if (!apre) return;
    ultimoAperto.current = "testo";
    if (unPannelloSolo) setCodaVisibile(false);
  };
  const alternaCoda = () => {
    const apre = !codaVisibile;
    setCodaVisibile(apre);
    if (!apre) return;
    ultimoAperto.current = "coda";
    if (unPannelloSolo) setTestoVisibile(false);
  };

  // ── il fuoco entra, e alla chiusura torna ──
  useEffect(() => {
    const chiAveva =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    schermata.current?.focus({ preventScroll: true });
    return () => {
      if (
        chiAveva !== null &&
        chiAveva !== document.body &&
        chiAveva.isConnected
      )
        chiAveva.focus({ preventScroll: true });
    };
  }, []);

  // Chi apre questa schermata mentre non suona niente resterebbe davanti a un
  // fondo vuoto senza modo di capire cosa è successo. Non capita dai tre modi
  // previsti per aprirla — sono tutti spenti senza un brano — ma capita se il
  // brano finisce mentre è aperta.
  useEffect(() => {
    if (brano === null) onChiudi();
  }, [brano, onChiudi]);

  if (brano === null) return null;

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  return (
    <section
      ref={schermata}
      /* Raggiungibile dal codice e non dal tabulatore: è il punto in cui il
         fuoco entra, e da lì il primo Tab va al primo comando della testata. */
      tabIndex={-1}
      className="np-screen in-riproduzione"
      // L'ancora del giro guidato: la schermata intera, che è quel che il
      // passo racconta — copertina, spettro e testo nello stesso posto.
      data-giro="in-riproduzione"
      aria-label={t("column.aria")}
    >
      <div className="ambiente ambient-backdrop" aria-hidden="true">
        <Sfocata hash={brano.coverArtHash} classe="tinta" />
      </div>
      <div className="np-scrim" aria-hidden="true" />
      {/* La scena sta **sopra** lo scrim e sotto tutto il resto: sotto di lui
          sarebbe schiacciata dal nero verticale che rende leggibile il titolo,
          e sopra il testo lo coprirebbe. Montata solo quando lo spettro è
          acceso: spenta non costa né una texture né un fotogramma, e nemmeno la
          presa nel motore — quella nasce e muore con questa tela.

          `inPausa` non ferma la scena: mezzo minuto di passato deve finire di
          uscire invece di congelarsi a metà strada. Serve alla macchina del
          riposo, che quando la scena si assopisce nel silenzio può chiudere
          anche la presa nel motore — ma solo se non sta suonando niente, perché
          altrimenti chiuderebbe l'unica cosa che sa svegliarla.

          `qualita` è l'altra metà della linea che separa una skin da
          un'impostazione: la skin dice come la scena appare, questa dice quanto
          questa macchina è disposta a spenderci. Cambiarla non rimonta niente —
          la densità passa per il ridimensionamento, il riflesso è un `if` — e
          l'unico caso che ricostruisce la texture è il passaggio da o verso
          «bassa», che si paga una volta e non si può evitare: la larghezza di
          una texture si decide alla sua nascita. */}
      {spettroVisibile && (
        <Spettro3D
          barre={barre}
          inPausa={stato.inPausa}
          qualita={qualitaSpettro}
          onErrore={onErrore}
        />
      )}

      <header className="testa">
        <div className="chi-suona">
          {/* «In riproduzione», e basta. Diceva «dall'album», sempre, anche
              quando la coda veniva da una playlist, da una ricerca o dal
              lunedì: nessuno qui sa da dove la coda sia stata riempita — non
              c'è un'origine né nello stato del nucleo né in quel che la coda
              salva, e dopo un riavvio non ci sarebbe comunque. Dire «album» a
              caso è una riga d'interfaccia che si sbaglia più spesso di quanto
              ci prenda; l'album del brano, quando c'è, sta già qui sotto in
              `np-meta`. */}
          <div className="occhiello hero-eyebrow">{t("np.nowPlaying")}</div>
        </div>

        <div className="comandi">
          {/* Acceso. Stava spento con la sua ragione nel suggerimento — «il
              nucleo non legge ancora i testi» — e adesso li legge: il sidecar
              accanto al brano, la riga in tabella, il tag. Il posto era deciso
              da prima, e questo è quel che ci è arrivato dentro. */}
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={testoVisibile}
            aria-label={t("np.lyrics.toggle")}
            onClick={alternaTesto}
          >
            <Icona nome="i-text" dim={16} titolo={t("np.lyrics")} />
          </button>
          {/* Questo invece è acceso, e lo è da quando il motore espone le
              bande: `aether-play::spettro` le prende dai campioni che escono
              davvero. La regola non è cambiata — quel che si disegna viene dal
              suono — è cambiato che adesso il suono si può guardare. */}
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={spettroVisibile}
            aria-label={t("np.spectrum.toggle")}
            onClick={() => {
              // Si scrive e si dipinge dalla risposta, non dal click: la
              // preferenza sta nel database, e l'unico modo di non mentire su
              // quel che ci sarà alla prossima apertura è mostrare quel che ci
              // è finito davvero.
              ipc
                .spettroVisibileScegli(!spettroVisibile)
                .then(setSpettroVisibile)
                .catch(onErrore);
            }}
          >
            <Icona nome="i-eq" dim={16} titolo={t("np.spectrum")} />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-pressed={codaVisibile}
            aria-label={t("np.queue.toggle")}
            onClick={alternaCoda}
          >
            <Icona nome="i-queue" dim={16} />
          </button>
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={onChiudi}
          >
            {t("common.close")}
            <kbd className="scorciatoia">{t("keys.esc")}</kbd>
            <Icona nome="i-chev-d" dim={13} />
          </button>
        </div>
      </header>

      {/* Sotto la testata e sopra il corpo, in un posto che non copre i comandi:
          vedi `avvisi`. Il contenitore resta anche vuoto — niente da
          rimontare quando una notizia arriva. */}
      <div className="avvisi-np">{avvisi}</div>

      <div
        ref={corpo}
        className="corpo"
        data-con-coda={codaVisibile || undefined}
        data-con-testo={testoVisibile || undefined}
      >
        {/* Lo stato dello spettro arriva al CSS come attributo: quel che cambia
            accendendolo non è cosa c'è nella colonna, ma dove si appoggia — al
            centro o nell'angolo — e dove stanno le cose sta di là. */}
        <div className="centro" data-spettro={spettroVisibile || undefined}>
          {/* Il sommario: la copertina e, accanto o sotto, di chi è. Un
              contenitore suo e non due elementi sciolti nella colonna, perché
              sono una cosa sola che si sposta insieme — e perché il titolo deve
              poter andare a capo accanto alla copertina, non sotto di lei.

              La copertina sta da sola, senz'ombra. Aveva sotto sé stessa,
              sfocata e spostata in giù, a farle da riflesso: erano due
              sfocature della stessa immagine, una addosso all'altra, perché
              l'ambiente dietro è già la copertina sfocata a tutto schermo. La
              piccola sporcava la grande, e sotto la copertina restava una
              macchia del suo colore che nessuna luce di questa stanza
              spiegava. */}
          <div className="sommario-np">
            <Copertina
              hash={brano.coverArtHash}
              titolo={titoloAlbum(brano.album)}
              classe="np-art"
              piena
            />

            <div className="chi">
              <h1 className="np-title" title={brano.title}>
                {brano.title}
              </h1>
              <div className="np-meta">
                {nomeArtista(brano.artist)} · {titoloAlbum(brano.album)}
                {brano.year ? ` · ${brano.year}` : ""}
              </div>
              {/* Sotto il sommario e non in un angolo: è la schermata che si
                  guarda mentre il brano suona, ed è lì che il rimando alla
                  pagina di chi l'ha pubblicato è davvero visibile. Su un file
                  del disco non compare niente. */}
              <Attribuzione brano={brano} onErrore={onErrore} />
            </div>
          </div>

          {/* In fondo, e tutti insieme: cursore, trasporto e giudizio sono la
              stessa cosa — i comandi — e stando in un contenitore si appoggiano
              al bordo di sotto con una riga di CSS invece di tre. */}
          <div className="comandi-np">
            <Scrubber stato={stato} onErrore={onErrore} />

            <Trasporto stato={stato} taglia="grande" onErrore={onErrore} />

            <Giudizio
              stato={stato}
              brano={brano}
              taglia="grande"
              onPreferito={onPreferito}
              onVoto={onVoto}
              onErrore={onErrore}
            />
          </div>
        </div>

        {/* Prima della coda: si legge da sinistra, e il testo è quel che si
            guarda mentre la coda è quel che viene dopo. */}
        {testoVisibile && <Testo brano={brano} onErrore={onErrore} />}

        {codaVisibile && (
          <aside className="coda-np" aria-label={t("queue.panel.aria")}>
            <header>
              <span className="occhiello hero-eyebrow">
                {t("np.queued", { n: stato.coda.length })}
              </span>
              <button
                type="button"
                className="bottone minuto btn-ghost"
                disabled={stato.coda.length === 0}
                onClick={() => comanda(ipc.codaSvuota())}
              >
                {t("queue.clear")}
              </button>
            </header>
            <RigheCoda
              stato={stato}
              righe={righe}
              onErrore={onErrore}
              compatta
            />
            <p className="come-si-riordina">
              {t("np.reorderHint.before")}
              <kbd>Alt</kbd>+<kbd>↑↓</kbd>
              {t("np.reorderHint.after")}
            </p>
          </aside>
        )}
      </div>

      {/* Fuori dal corpo, e dopo: il corpo ha `overflow: hidden` e questo si
          appoggia all'angolo della schermata, non della colonna. Fra elementi
          posizionati senza `z-index` decide l'ordine nel DOM, quindi stare qui
          basta a stare sopra tutto — senza aggiungere un quinto `z-index` da
          tenere allineato agli altri quattro. */}
      {spettroVisibile && (
        <DettaglioSpettro barre={barre} onBarre={scegliBarre} />
      )}
    </section>
  );
}
