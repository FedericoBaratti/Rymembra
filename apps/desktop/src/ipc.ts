/**
 * Il confine con il nucleo.
 *
 * Ogni chiamata al processo nativo passa da qui, e da nessun'altra parte: i tipi
 * che questo file riesporta dalla cartella `ipc/` sono il contratto, e tenerli
 * in un posto solo è ciò che rende visibile in un `git diff` il momento in cui
 * cambia.
 *
 * Gli errori arrivano come record — codice, dominio, gravità, ritentabilità,
 * chiave di traduzione — non come stringhe. Nel vecchio albero arrivavano come
 * testo, e il risultato è che l'interfaccia mostrava «2» per un guasto di
 * riproduzione: il codice del motore veniva scartato per strada.
 */
import { invoke } from "@tauri-apps/api/core";

import type { ErroreIpc } from "./ipc/errori";
import type { AggiuntaPlaylist, AggiuntiDalCatalogo, Album, Artista, Avanzamento, AvanzamentoVolume, Avvio, Brano, Cancellazione, CartellaCandidata, CartelleFiltrate, CartelleOrdinate, Casa, CorrezioniMetadati, DispositivoAudio, EsitoAggiuntaTutti, EsitoRicerca, EsitoRimozioni, EsitoScansione, EsportazioneProfilo, FileCartella, ModoLivello, NodoCartella, Normalizzazione, Ordine, PianoProfilo, Playlist, PlaylistEsportata, QualitaSpettro, Raccolta, RimappaturaRadice, StatoRiproduzione, StatoUiCartelle, TracciaIncerta, Trascinati, Traslocati, VistaBrani, VoceCronologia, VocePreset } from "./ipc/libreria";
import type { AnteprimaAccount, AnteprimaImport, AnteprimaRegole, DaComprare, DiagnosticaImport, EsitoAccount, EsitoFilePlaylist, EsitoImport, EsitoImportazione, InsiemeSmart, PianoFilePlaylist, ScelteAccount, StatoAccount, StatoScarico } from "./ipc/importazioni";
import type { Causa, Istantanea, Migrato, Registro, Salvataggio, Skin, Validazione, Variabile, VoceFile, VoceSkin, VoceTavolozza } from "./ipc/skin";
import type { AppDaSalvare, AppNuvolaIpc, CandidatiTesto, CartellaRemota, ChiavePiattaforma, ContoNuvola, DirittiInsieme, DispositivoSincronia, EsitoAnnullamento, EsitoInvio, EsitoPulizia, EsitoRipristino, MessaggioIa, ModelloIa, ModoSorveglianza, ModuloCollega, OperazioniIa, PianoRipristino, ProfiloIaDaSalvare, QuadroSorveglianza, Resoconto, SaluteDatabase, ServizioNuvola, ServizioOfferto, StatoAggiornamenti, StatoArricchimento, StatoCacheNuvola, StatoIa, StatoInsieme, StatoManutenzione, StatoNuvola, StatoScrobble, StatoSincronia, StatoTesti, TestoBrano } from "./ipc/servizi";

export * from "./ipc/errori";
export * from "./ipc/libreria";
export * from "./ipc/importazioni";
export * from "./ipc/skin";
export * from "./ipc/servizi";

export const ipc = {
  // La finestra nasce nascosta: il fondo che il sistema operativo dipinge
  // prima che esista una pagina è quello di `tauri.conf.json`, e con una skin
  // chiara era un fotogramma scuro a ogni avvio. La si mostra quando skin e
  // tema sono già sul documento. Se questa chiamata non arriva mai, la mostra
  // il nucleo dopo due secondi.
  pronto: () => invoke<void>("pronto"),

  // ── la barra del titolo ───────────────────────────────────────────────────
  // La finestra non ha decorazioni: i quattro gesti che dava il sistema
  // operativo li disegna la pagina, e passano da comandi nostri invece che dai
  // permessi `core:window:*` — vedi `main.rs`, dove sta scritto perché. Il
  // quinto — lo schermo intero — quella fascia non lo dava nemmeno: arriva da
  // `F11`, cioè dalla tabella delle scorciatoie.
  finestraTrascina: () => invoke<void>("finestra_trascina"),
  finestraRiduci: () => invoke<void>("finestra_riduci"),
  // Torna com'è rimasta: un giro solo invece di «cambia» e poi «com'è?».
  finestraIngrandisci: () => invoke<boolean>("finestra_ingrandisci"),
  finestraIngrandita: () => invoke<boolean>("finestra_ingrandita"),
  // Lo schermo intero della **finestra**, che non è quello di «In riproduzione»:
  // quello riempie la finestra di un brano, questo toglie di mezzo il resto del
  // desktop. Torna com'è rimasta, come `finestraIngrandisci`.
  finestraSchermoIntero: () => invoke<boolean>("finestra_schermo_intero"),
  finestraChiudi: () => invoke<void>("finestra_chiudi"),

  // ── il secondo piano ──────────────────────────────────────────────────────
  // Se `finestraChiudi` spenga il programma o nasconda soltanto la finestra. La
  // barra del titolo non lo sa e non deve saperlo: chiede di chiudere, e chi
  // ascolta l'evento decide — vedi `vassoio.rs`.
  secondoPiano: () => invoke<boolean>("secondo_piano"),
  // Torna com'è rimasto, riletto dal database: l'interruttore si disegna da lì
  // e non dal click, come `accentoDinamicoAttiva`.
  secondoPianoAttiva: (attivo: boolean) =>
    invoke<boolean>("secondo_piano_attiva", { attivo }),
  // Le voci del menù dell'icona, nella lingua della finestra. Vanno mandate
  // perché i testi che si leggono stanno in `lingue/`, dove `lingue.js` li
  // controlla, e non in Rust dove nessuno li terrebbe allineati. Le quattro
  // della musica — riproduci, pausa, successivo, precedente — si mandano tutte
  // e due le etichette della prima voce, perché è il nucleo a sapere quale
  // mostrare mentre la finestra è nascosta.
  vassoioLingua: (
    mostra: string,
    esci: string,
    riproduci: string,
    pausa: string,
    successivo: string,
    precedente: string,
  ) =>
    invoke<void>("vassoio_lingua", {
      mostra,
      esci,
      riproduci,
      pausa,
      successivo,
      precedente,
    }),

  avvio: () => invoke<Avvio>("avvio"),
  // Scrive le cartelle sorvegliate e dice cosa ne ha fatto: le annidate si
  // raccolgono nella madre, e una cartella che è una già sorvegliata con un
  // altro nome (lettera mappata ↔ `\\server`) non entra — torna fra le
  // `gemelle`, per proporre il trasloco.
  impostaCartelle: (cartelle: string[]) =>
    invoke<CartelleOrdinate>("imposta_cartelle", { cartelle }),
  // Sposta la libreria da una cartella sorvegliata alla stessa cartella scritta
  // in un altro modo, riscrivendo i percorsi dei brani invece di rileggerli.
  cartelleTrasloca: (da: string, a: string) =>
    invoke<Traslocati>("cartelle_trasloca", { da, a }),
  // I percorsi lasciati cadere sulla finestra, divisi dal nucleo per quel che
  // se ne fa: è lui a chiedere al filesystem se un percorso è una cartella.
  smistaTrascinati: (percorsi: string[]) =>
    invoke<Trascinati>("smista_trascinati", { percorsi }),
  // Il file di un brano, selezionato in Esplora risorse. Un identificativo e non
  // un percorso: il percorso lo legge il nucleo.
  branoMostraNellaCartella: (id: number) =>
    invoke<void>("brano_mostra_nella_cartella", { id }),
  // La pagina pubblica di un brano di catalogo. Come sopra: un identificativo e
  // mai un indirizzo, perché aprire il browser su un indirizzo scelto dalla
  // finestra sarebbe l'unica capacità pericolosa che ha, data intera.
  branoApriPagina: (id: number) => invoke<void>("brano_apri_pagina", { id }),
  // Le due cancellazioni, e la differenza sta tutta nel nome: la prima toglie
  // la riga e lascia il file, la seconda manda il file nel Cestino e toglie la
  // riga solo per quelli che ci sono arrivati. Identificativi e mai percorsi,
  // per la stessa ragione di «mostra nella cartella».
  braniTogli: (brani: number[]) =>
    invoke<Cancellazione>("brani_togli", { brani }),
  // `definitivi` è la conferma che la finestra ha chiesto quando qualche file
  // sta su un'unità di rete, dove un Cestino non c'è: senza, il nucleo rifiuta
  // quei file con `fs.noRecycleBin` prima di toccare qualunque cosa.
  braniElimina: (brani: number[], definitivi = false) =>
    invoke<number>("brani_elimina", { brani, definitivi }),
  // Quanti di questi brani spariscono per sempre, perché il loro file sta dove
  // un Cestino non c'è: la conferma lo chiede prima di mostrarsi. Rifiuta con
  // `library.imageTrackPartialDelete` i brani di un'immagine CD scelti a metà.
  braniSuRete: (brani: number[]) =>
    invoke<number>("brani_su_rete", { brani }),
  // Le cartelle musicali del sistema che contengono davvero qualcosa. Elenco
  // vuoto anche quando la ricerca è scaduta: per la finestra i due casi si
  // disegnano uguali — non c'è niente da proporre — e distinguerli vorrebbe
  // dire un messaggio in più che non porta a nessuna azione diversa.
  cartelleCandidate: () =>
    invoke<CartellaCandidata[]>("cartelle_candidate"),
  // Stringa vuota = rimetti il valore di serie, cioè la prima cartella
  // sorvegliata. Il nucleo **toglie** la riga invece di scriverci dentro il
  // vuoto: «mai scelta» e «scelta vuota» devono restare la stessa cosa.
  impostaCartellaDownload: (percorso: string) =>
    invoke<void>("imposta_cartella_download", { percorso }),

  // ── il pannello «Cartelle» ────────────────────────────────────────────────
  // Tutti e tre girano **in disparte**, mai sotto il lucchetto della libreria:
  // il pannello si apre e si espande anche durante una scansione. Le radici
  // vanno mandate a ogni chiamata perché sono loro a decidere la forma
  // dell'albero — aggiungerne una lo fa ricostruire, ed è il nucleo ad
  // accorgersene confrontandole con quelle di prima.
  //
  // `percorso: null` chiede i nodi di primo livello.
  cartelleFiglie: (radici: string[], percorso: string | null) =>
    invoke<NodoCartella[]>("cartelle_figlie", { radici, percorso }),
  // Gli identificativi, già nell'ordine in cui si riproducono: dentro una
  // cartella è l'ordine dell'album, perché una cartella nel caso normale *è* un
  // album. Ricorsivo, come il conteggio; `ricorsivo: false` tiene i soli brani
  // che stanno direttamente lì — l'interruttore «includi sottocartelle».
  cartelleBrani: (radici: string[], percorso: string, ricorsivo = true) =>
    invoke<number[]>("cartelle_brani", { radici, percorso, ricorsivo }),
  // Le foglie di una cartella: i file che stanno direttamente lì, dal database
  // e mai dal disco. In un ordine qualunque — le ordina chi le disegna
  // (`albero.ts`, per nome di file).
  cartelleFile: (radici: string[], percorso: string) =>
    invoke<FileCartella[]>("cartelle_file", { radici, percorso }),
  // Il filtro sopra l'albero: cerca nell'albero intero, non solo nei nodi
  // aperti, e restituisce le corrispondenze con la strada per arrivarci.
  cartelleFiltra: (radici: string[], testo: string) =>
    invoke<CartelleFiltrate>("cartelle_filtra", { radici, testo }),
  // Apre una cartella in Esplora risorse. Il percorso arriva da qui — una
  // cartella non ha un identificativo — e per questo il nucleo lo controlla
  // contro le cartelle sorvegliate **sue**, non contro quelle che manda la
  // finestra.
  cartellaMostra: (percorso: string) =>
    invoke<void>("cartella_mostra", { percorso }),
  // Quali radici rispondono adesso, una risposta per posto e nello stesso
  // ordine. Una radice morta è `false`, non un errore: il pannello si disegna
  // **prima** di chiamare questa, e questa arriva dopo a spegnere quel che non
  // c'è. È il motivo per cui l'albero si apre col Wi-Fi staccato.
  cartelleRadiciVive: (radici: string[]) =>
    invoke<boolean[]>("cartelle_radici_vive", { radici }),
  // Com'era rimasto il pannello. Un comando suo e non un campo di `avvio`: chi
  // non apre mai quel pannello non deve pagarne la lettura.
  cartelleUi: () => invoke<StatoUiCartelle>("cartelle_ui"),
  // Stringa vuota in `scelta` = dimentica dov'era il fuoco. Il nucleo taglia
  // l'elenco degli aperti a duecento tenendo i più recenti, cioè la coda.
  impostaCartelleUi: (aperte: string[], scelta: string) =>
    invoke<void>("imposta_cartelle_ui", { aperte, scelta }),

  // ── le preferenze della finestra, e il profilo ────────────────────────────
  // Il tema stava in `localStorage` e adesso sta in `settings`: era l'unica
  // preferenza che non finiva né nel backup su Drive né nel profilo.
  impostaTema: (tema: string) => invoke<void>("imposta_tema", { tema }),
  // «Meno movimento di quanto la skin ne dichiari». Un comando suo e non un
  // campo di `avvio`: al contrario del tema non ha una chiave vecchia da
  // riconciliare, quindi non deve viaggiare insieme all'apertura.
  movimentoRidotto: () => invoke<boolean>("movimento_ridotto"),
  // Due valori e non tre: da qui si può solo **ridurre** sotto la skin. Un
  // livello «di più» contraddirebbe la frase che l'interfaccia mostra sopra il
  // comando.
  impostaMovimentoRidotto: (ridotto: boolean) =>
    invoke<void>("imposta_movimento_ridotto", { ridotto }),
  // Stringa vuota = rimetti il rilevamento dal sistema. Il nucleo non sa quali
  // lingue esistono — l'elenco è la cartella `src/lingue/` — e controlla
  // soltanto che il testo sia un codice di lingua.
  impostaLingua: (lingua: string) => invoke<void>("imposta_lingua", { lingua }),
  // Stringa vuota = rimetti quelle di serie. Il nucleo controlla soltanto che
  // sia JSON: i nomi dei comandi sono roba della finestra, e un nucleo che li
  // validasse andrebbe ricompilato per aggiungere una scorciatoia.
  impostaScorciatoie: (scorciatoie: string) =>
    invoke<void>("imposta_scorciatoie", { scorciatoie }),
  // Lo zoom della finestra. **Nessun numero attraversa questa interfaccia**:
  // di qua partono tre gesti — apri, un gradino su o giù, torna al vero — e
  // torna indietro il fattore che è stato applicato, buono solo da mostrare.
  // La scala dei gradini vive in `rymembra_app::preferenze::SCALA_ZOOM` e di
  // elenchi non ne esistono due: mandare un fattore da qui vorrebbe dire
  // conoscerne almeno uno, e da lì a copiarli tutti è un passo.
  //
  // È il backend ad applicare `set_zoom`, non la pagina: vedi il preambolo di
  // `zoom.rs` sul perché `core:webview` resta chiuso.
  zoomAvvio: () => invoke<number>("zoom_avvio"),
  zoomPasso: (su: boolean) => invoke<number>("zoom_passo", { su }),
  zoomNormale: () => invoke<number>("zoom_normale"),
  // Il giro guidato. Il confronto fra il copione visto e quello di oggi lo fa
  // il nucleo — di qua arriva un sì o un no — perché il numero del copione
  // deve stare in un posto solo.
  giroDaFare: () => invoke<boolean>("giro_da_fare"),
  // La chiama sia chi il giro lo finisce sia chi lo salta: sono due modi di
  // dire «questo l'ho visto».
  giroFatto: () => invoke<void>("giro_fatto"),
  // Un archivio `.ryprofile`, non più un `.json`: dentro ci sono anche la
  // libreria, le copertine e i pacchetti skin. Il vecchio formato si **legge**
  // ancora e non si scrive più.
  profiloEsporta: (percorso: string) =>
    invoke<EsportazioneProfilo>("profilo_esporta", { percorso }),
  // Il piano prima di applicare, come per ogni altra cosa irreversibile qui —
  // e qui più che altrove, perché non è più solo l'esecuzione annullata delle
  // preferenze ma anche quella della fusione della libreria.
  //
  // `rimappature` è vuoto la prima volta: allora il piano ne **propone** una
  // per ogni radice che di qua non esiste, e lo si richiama con quelle
  // compilate per vedere quanti percorsi seguirebbero.
  // `unisciComunque`: la libreria del profilo è di chi importa anche se le due
  // identità non si riconoscono. Di serie no, e il piano dice sempre se lo sono.
  profiloPiano: (
    percorso: string,
    rimappature: RimappaturaRadice[] = [],
    unisciComunque = false,
  ) =>
    invoke<PianoProfilo>("profilo_piano", {
      percorso,
      rimappature,
      unisciComunque,
    }),
  profiloImporta: (
    percorso: string,
    rimappature: RimappaturaRadice[] = [],
    unisciComunque = false,
  ) =>
    invoke<PianoProfilo>("profilo_importa", {
      percorso,
      rimappature,
      unisciComunque,
    }),
  // Rimette **solo** le preferenze e la skin attiva dall'ultima copia di
  // sicurezza. Il resto dell'importazione è additivo e non si annulla, e
  // l'interfaccia lo dice invece di promettere un ritorno che non c'è.
  profiloAnnulla: () => invoke<PianoProfilo>("profilo_annulla"),
  // La cronologia d'ascolto, dal più recente. Si scriveva dal primo giorno e
  // non la leggeva nessuno; dopo l'importazione di un account contiene anni.
  cronologia: (offset: number, limite: number) =>
    invoke<VoceCronologia[]>("cronologia", { offset, limite }),
  cronologiaConteggio: () => invoke<number>("cronologia_conteggio"),
  scansiona: () => invoke<EsitoScansione>("scansiona"),
  // ── i brani con i metadati messi male ──
  // Il conteggio sta a parte dall'elenco per la stessa ragione di
  // `cercaConteggio`: il numero si chiede a ogni scansione per la pastiglia,
  // l'elenco solo quando qualcuno apre la sezione.
  metadatiConteggio: () => invoke<number>("metadati_conteggio"),
  metadatiIncerti: (offset: number, limite: number) =>
    invoke<TracciaIncerta[]>("metadati_incerti", { offset, limite }),
  // Restituisce la traccia com'è rimasta: se è tornata a posto arriva `null`,
  // e la riga sparisce dall'elenco senza doverlo ricaricare tutto.
  metadatiCorreggi: (id: number, campi: CorrezioniMetadati) =>
    invoke<TracciaIncerta | null>("metadati_correggi", { id, campi }),
  metadatiConferma: (id: number) => invoke<void>("metadati_conferma", { id }),
  // «Rileggi tutti i tag»: segna ogni brano da rileggere e restituisce quanti.
  // Non scansiona — la scansione la avvia chi chiama, ed è quella di sempre.
  metadatiRileggiTutti: () => invoke<number>("metadati_rileggi_tutti"),
  // Le cartelle sorvegliate: leggere è istantaneo (un quadro in memoria),
  // cambiare ferma o avvia gli osservatori senza riavviare.
  sorveglianzaStato: () => invoke<QuadroSorveglianza>("sorveglianza_stato"),
  impostaSorveglianza: (modo: ModoSorveglianza) =>
    invoke<QuadroSorveglianza>("imposta_sorveglianza", { modo }),
  // Torna subito: fermarsi vuol dire «alla fine del lotto in corso», non
  // «adesso». Che sia successo lo dice `EsitoScansione.annullata`.
  annullaScansione: () => invoke<void>("annulla_scansione"),
  // La scansione a mano in corso — il suo ultimo avanzamento — o `null`. La
  // chiede la finestra all'apertura: una ricaricata a metà scansione riceve
  // gli eventi ma non la risposta di `scansiona`, che era della finestra di
  // prima. La fine, per tutte, la dice `scansione:finita`.
  scansioneAvanzamento: () =>
    invoke<Avanzamento | null>("scansione_avanzamento"),
  // Il sì alla domanda di una scansione a mano: toglie esattamente le righe
  // trattenute, senza ricamminare il disco. Il no le lascia cadere.
  rimozioniConferma: (gettone: number) =>
    invoke<EsitoRimozioni>("rimozioni_conferma", { gettone }),
  rimozioniScarta: () => invoke<void>("rimozioni_scarta"),
  cerca: (query: string, offset: number, limite: number) =>
    invoke<Brano[]>("cerca", { query, offset, limite }),
  // Separato dalla pagina: la pagina si chiede a ogni scorrimento, il conteggio
  // una volta per query. Serve perché «N risultati» dica un numero vero invece
  // della lunghezza della prima pagina.
  cercaConteggio: (query: string) =>
    invoke<number>("cerca_conteggio", { query }),
  casa: () => invoke<Casa>("casa"),
  // Il fuso lo sa solo la finestra: il nucleo tiene il tempo in millisecondi
  // universali, e il lunedì comincia sette ore prima a Roma che a Los Angeles.
  // Il segno è quello che si legge — minuti da AGGIUNGERE all'universale — cioè
  // l'opposto di `getTimezoneOffset()`.
  settimana: () =>
    invoke<Raccolta[]>("settimana", {
      scostamentoMinuti: -new Date().getTimezoneOffset(),
    }),
  settimanaApri: (raccolta: number) =>
    invoke<void>("settimana_apri", { raccolta }),
  brani: (ordine: Ordine, offset: number, limite: number) =>
    invoke<Brano[]>("brani", { ordine, offset, limite }),
  // Gli identificativi di tutta la vista, nello stesso ordine delle pagine.
  vistaBraniId: (vista: VistaBrani) =>
    invoke<number[]>("vista_brani_id", { vista }),
  album: (offset: number, limite: number) =>
    invoke<Album[]>("album", { offset, limite }),
  // Una query vera e non un filtro sull'elenco dei brani: prima se ne
  // chiedevano duemila e si tenevano quelli col cuore, cioè tutta la libreria
  // letta a ogni visita e i preferiti oltre il duemillesimo invisibili.
  preferiti: (offset: number, limite: number) =>
    invoke<Brano[]>("preferiti", { offset, limite }),
  // Anche questi al database: filtrare qui la pagina di album già scaricata
  // dava una pagina vuota agli artisti che stavano oltre.
  albumArtista: (nome: string, offset: number, limite: number) =>
    invoke<Album[]>("album_artista", { nome, offset, limite }),
  // Senza offset né limite: gli artisti sono pochi e la vista li mostra tutti
  // con un indice alfabetico invece che a pagine.
  artisti: () => invoke<Artista[]>("artisti"),
  braniAlbum: (chiave: string) =>
    invoke<Brano[]>("brani_album", { chiave }),
  preferito: (id: number, valore: boolean) =>
    invoke<void>("preferito", { id, valore }),
  valutazione: (id: number, stelle: number) =>
    invoke<void>("valutazione", { id, stelle }),
  // Senza `id` il nucleo compila la skin **scelta**, non quella di serie: la
  // scelta vive in `settings`, e la finestra non deve ricordarsela.
  skin: (id?: string) => invoke<Skin>("skin", { id: id ?? null }),
  skinElenco: () => invoke<VoceSkin[]>("skin_elenco"),
  skinInstalla: (percorso: string) =>
    invoke<VoceSkin>("skin_installa", { percorso }),
  // Installa un manifest senza passare da un file sul disco: è quel che serve
  // a un tema fatto qui dentro, che altrimenti dovrebbe uscire come `.ryskin`
  // e rientrare dalla stessa finestra di dialogo per essere usato.
  // `bozza` è l'id della bozza da cui prendere miniatura e risorse: senza,
  // si impacchetta il solo manifest.
  skinInstallaSorgente: (sorgente: string, bozza: string | null = null) =>
    invoke<VoceSkin>("skin_installa_sorgente", { sorgente, bozza }),
  // Restituisce la skin che **resta attiva**: togliere quella indossata torna a
  // quella di serie, e il CSS da applicare arriva di qui invece che da una
  // seconda chiamata.
  skinDisinstalla: (id: string) => invoke<Skin>("skin_disinstalla", { id }),
  skinScegli: (id: string) => invoke<Skin>("skin_scegli", { id }),

  // ── l'accento che segue la copertina ─────────────────────────────────────
  // Il colore non si decide qui. `accentoCopertina` risponde con le proprietà
  // già scritte, o con `null` quando non si deve toccare niente — preferenza
  // spenta, skin che non lo permette, copertina assente o in bianco e nero, o
  // nessuna chiarezza di quella tonalità che regga 4,5:1. Sono cinque motivi e
  // una sola risposta, perché l'azione da fare è la stessa: tenere l'accento
  // della skin.
  accentoDinamico: () => invoke<boolean>("accento_dinamico"),
  accentoDinamicoAttiva: (attivo: boolean) =>
    invoke<boolean>("accento_dinamico_attiva", { attivo }),
  accentoCopertina: (copertina: string | null, chiaro: boolean) =>
    invoke<Variabile[] | null>("accento_copertina", { copertina, chiaro }),

  // ── lo Studio ────────────────────────────────────────────────────────────
  studioRegistro: () => invoke<Registro>("studio_registro"),
  // Non fallisce mai: a metà di una parentesi il JSON non è JSON, ed è il caso
  // normale mentre si scrive. Gli errori sono un campo del risultato.
  // `bozza` è la bozza aperta: il foglio si compila sotto il suo id, e le
  // risorse vengono dalla sua cartella.
  studioValida: (sorgente: string, bozza: string | null = null) =>
    invoke<Validazione>("studio_valida", { sorgente, bozza }),
  studioDocumento: (id: string) => invoke<string>("studio_documento", { id }),
  // Non l'elenco di una cartella: le voci che il **formato** ammette, cioè
  // esattamente quel che `studioEsporta` rimetterà nell'archivio.
  studioPacchetto: (id: string) =>
    invoke<VoceFile[]>("studio_pacchetto", { id }),
  // Se il documento ha cambiato `id` la bozza lo segue nello stesso passo;
  // `origine` è l'id con cui lo Studio è stato aperto, l'unico installato su
  // cui la bozza può tornare.
  studioSalva: (id: string, sorgente: string, origine: string | null) =>
    invoke<Salvataggio>("studio_salva", { id, sorgente, origine }),
  // Butta la bozza e restituisce quel che dice il pacchetto: la sorgente torna
  // di qui e non da una seconda chiamata, così non esiste un istante in cui la
  // finestra mostra un documento che non sta più da nessuna parte.
  studioScarta: (id: string) => invoke<string>("studio_scarta", { id }),
  // Vuole l'`id` perché il manifest è quello dell'editor ma le risorse sono
  // quelle del pacchetto: senza, una skin col suo carattere dentro lo perdeva
  // ogni volta che usciva da qui.
  studioEsporta: (id: string, sorgente: string, percorso: string) =>
    invoke<void>("studio_esporta", { id, sorgente, percorso }),
  studioIstantanee: (id: string) =>
    invoke<Istantanea[]>("studio_istantanee", { id }),
  studioIstantanea: (id: string, sorgente: string, causa: Causa) =>
    invoke<void>("studio_istantanea", { id, sorgente, causa }),
  studioRipristina: (id: string, quando: number) =>
    invoke<string>("studio_ripristina", { id, quando }),
  // Un file scelto sul disco entra nella bozza col nome che il formato
  // ammette: la voce che torna dice quale ha preso.
  studioRisorsaAggiungi: (id: string, percorso: string) =>
    invoke<VoceFile>("studio_risorsa_aggiungi", { id, percorso }),
  studioRisorsaTogli: (id: string, nome: string) =>
    invoke<void>("studio_risorsa_togli", { id, nome }),
  studioMigra: (sorgente: string) => invoke<Migrato>("studio_migra", { sorgente }),
  // La bozza compilata per la finestra vera: «Prova sull'app». Sotto l'id
  // della bozza, come la tela.
  studioCompilaApp: (sorgente: string, bozza: string) =>
    invoke<Skin>("studio_compila_app", { sorgente, bozza }),
  // Un tema nuovo: il pacchetto con le risorse e la miniatura della base,
  // installato, e la bozza che parte da lì.
  studioCrea: (base: string, sorgente: string) =>
    invoke<VoceSkin>("studio_crea", { base, sorgente }),
  // Gli id che hanno già una cartella nello Studio: un tema nuovo non li prende.
  studioOccupati: () => invoke<string[]>("studio_occupati"),
  // La conversazione con la chat, salvata accanto alla bozza. Il contenuto lo
  // decide la chat (`studio/conversazione.ts`); un testo vuoto la cancella.
  studioChatLeggi: (id: string) => invoke<string | null>("studio_chat_leggi", { id }),
  studioChatScrivi: (id: string, testo: string) =>
    invoke<void>("studio_chat_scrivi", { id, testo }),
  // Una tavolozza intera da una tinta: un colore, o la copertina di un brano.
  studioTavolozzaDa: (da: { colore: string } | { copertina: string }, scura: boolean) =>
    invoke<VoceTavolozza[]>("studio_tavolozza_da", {
      colore: "colore" in da ? da.colore : null,
      copertina: "copertina" in da ? da.copertina : null,
      scura,
    }),

  // ── playlist ─────────────────────────────────────────────────────────────
  // Ogni comando che modifica restituisce la playlist aggiornata: i conteggi e
  // la durata li ricalcola il nucleo con una query sola, e rifarli qui
  // sommando le righe vorrebbe dire avere due definizioni di «quanto dura una
  // playlist» destinate a discostarsi.
  playlistElenco: () => invoke<Playlist[]>("playlist_elenco"),
  playlistBrani: (id: number) => invoke<Brano[]>("playlist_brani", { id }),
  playlistCrea: (nome: string) => invoke<Playlist>("playlist_crea", { nome }),
  playlistRinomina: (id: number, nome: string) =>
    invoke<Playlist>("playlist_rinomina", { id, nome }),
  // La cancellazione è già scritta quando torna: il numero è il gettone per
  // `playlistRipristina`, che rimette tutto com'era finché l'avviso con
  // «Annulla» è a schermo. `null` da lì vuol dire «troppo tardi», non un guasto.
  playlistCancella: (id: number) => invoke<number>("playlist_cancella", { id }),
  playlistRipristina: (gettone: number) =>
    invoke<Playlist | null>("playlist_ripristina", { gettone }),
  // Con quanti sono entrati e quanti non c'erano più: «2 non c'erano più» è
  // la frase che distingue una scansione passata nel frattempo da un guasto.
  playlistAggiungi: (id: number, brani: number[]) =>
    invoke<AggiuntaPlaylist>("playlist_aggiungi", { id, brani }),
  // Crea e aggiunge in una transazione sola: o tutte e due, o niente.
  playlistCreaEAggiungi: (nome: string, brani: number[]) =>
    invoke<AggiuntaPlaylist>("playlist_crea_e_aggiungi", { nome, brani }),
  playlistTogli: (id: number, posizione: number) =>
    invoke<Playlist>("playlist_togli", { id, posizione }),
  // ── le playlist intelligenti ──────────────────────────────────────────
  // Non si materializzano: una playlist intelligente **è** la sua
  // interrogazione, e `playlistBrani` la esegue. Scrivere le righe vorrebbe
  // dire tenerle aggiornate a ogni ascolto, a ogni scansione e a ogni
  // mezzanotte che passa — «ascoltati negli ultimi 30 giorni» cambia da sé.
  playlistCreaSmart: (nome: string, regole: InsiemeSmart) =>
    invoke<Playlist>("playlist_crea_smart", { nome, regole }),
  playlistRegole: (id: number) =>
    invoke<InsiemeSmart | null>("playlist_regole", { id }),
  playlistRegoleScrivi: (id: number, regole: InsiemeSmart) =>
    invoke<Playlist>("playlist_regole_scrivi", { id, regole }),
  // L'anteprima mentre si scrive: è l'unico modo di accorgersi di aver scritto
  // «anno maggiore di 2050» prima di salvare una playlist vuota.
  playlistRegoleProva: (regole: InsiemeSmart) =>
    invoke<AnteprimaRegole>("playlist_regole_prova", { regole }),

  // ── i file di playlist ────────────────────────────────────────────────
  // Il piano prima, come ogni altra cosa che scrive: reimportare un file
  // **sostituisce** la playlist con lo stesso nome, e cancellare va detto
  // prima di farlo.
  playlistFilePiano: (percorso: string) =>
    invoke<PianoFilePlaylist>("playlist_file_piano", { percorso }),
  playlistFileImporta: (percorso: string, nome: string) =>
    invoke<EsitoFilePlaylist>("playlist_file_importa", { percorso, nome }),
  // Il formato lo decide l'estensione del file scelto: chiederlo una seconda
  // volta in una tendina accanto sarebbe la stessa domanda con due risposte.
  playlistEsporta: (id: number, percorso: string) =>
    invoke<PlaylistEsportata>("playlist_esporta", { id, percorso }),

  playlistRiordina: (id: number, da: number, a: number) =>
    invoke<Playlist>("playlist_riordina", { id, da, a }),

  // ── importazione dal vecchio database ────────────────────────────────────
  // Due comandi e non uno con un booleano: il piano non tocca niente e
  // l'importazione sì, e due nomi diversi rendono impossibile confonderli in un
  // punto di chiamata.
  pianoImportazione: (percorso: string) =>
    invoke<EsitoImportazione>("piano_importazione", { percorso }),
  importa: (percorso: string) =>
    invoke<EsitoImportazione>("importa", { percorso }),

  // ── importazione da un link ──────────────────────────────────────────────
  // Una serie sola per tutti i cataloghi: i lettori di là producono lo stesso
  // tipo, quindi da qui in poi il percorso è uno. Chi incolla non deve
  // scegliere un catalogo prima di sapere che cosa ha negli appunti.
  //
  // I link che valgono sono quelli di **archive.org**, **jamendo.com** e
  // **audius.co**. Un link che non si riconosce non viene tentato: il nucleo
  // risponde `download.unrecognizedUrl` senza fare nessuna richiesta, perché un
  // link a cui non si sa bussare è un link a cui non si bussa.
  //
  // Tutti e tre i comandi ricevono il **link**, mai il contenuto: quel che è
  // stato letto resta di là, in una cella indicizzata da catalogo e
  // identificativo. Farlo viaggiare fin qui e indietro vorrebbe dire
  // serializzare trecento brani due volte e poi fidarsi che quel che torna sia
  // quel che era partito.
  //
  // Chiamarli in fila con lo stesso link fa **una** lettura di rete: il secondo
  // e il terzo trovano la cella già piena.
  //
  // `forza` salta il riuso della cella, ed è quel che rende «Riprova» una cosa
  // che riprova: senza, una lettura caduta all'ultimo livello — perché in quel
  // momento la rete singhiozzava — resterebbe la risposta di quel link per tutto
  // il tempo in cui l'applicazione è aperta. Una rilettura **fallita** lascia in
  // cella quella di prima: si perde il tentativo, non quel che si aveva.
  //
  // Mentre `importAnteprima` legge arriva `import:avanzamento` — pagine e brani,
  // vedi `AvanzamentoImport`. Fino a poco fa una famiglia `import:*` non
  // esisteva affatto, ed era la ragione per cui una playlist da trecento brani
  // era una chiamata bloccante muta dietro il tasto «Guarda». Non c'è un evento
  // di annullamento: l'abort del lettore è cablato a «no», e un tasto che non
  // annulla è peggio della sua assenza.
  // La ricerca nei cataloghi liberi. Va chiamata a **Invio** e non a ogni
  // carattere: costa da una a cinque richieste all'Internet Archive e una ad
  // Audius, contro archivi pubblici che ci ospitano gratis. La casella della
  // libreria cerca a ogni tasto perché interroga un indice che sta in casa;
  // questa no, ed è una differenza di buona educazione prima che di
  // prestazioni.
  esploraCerca: (testo: string) =>
    invoke<EsitoRicerca>("esplora_cerca", { testo }),
  esploraAnnulla: () => invoke<void>("esplora_annulla"),
  // Si manda l'**indirizzo**, non la riga: licenza e disponibilità le dichiara
  // il catalogo e restano di là. Vedi il `//!` di `esplora.rs`.
  esploraAggiungi: (url: string) =>
    invoke<AggiuntiDalCatalogo>("esplora_aggiungi", { url }),
  esploraAggiungiTutti: () =>
    invoke<EsitoAggiuntaTutti>("esplora_aggiungi_tutti"),
  esploraTogli: (url: string) => invoke<boolean>("esplora_togli", { url }),
  esploraIdentificativo: (url: string) =>
    invoke<number | null>("esplora_identificativo", { url }),
  // Si nomina il risultato, non la pagina: l'indirizzo lo tira fuori il nucleo
  // da quel che il catalogo aveva dichiarato, e lo fa passare dall'allowlist.
  // Stessa regola di `cercaDoveComprare`, e per la stessa ragione.
  esploraApriPagina: (url: string) =>
    invoke<void>("esplora_apri_pagina", { url }),
  // Quel che nessun catalogo libero consegna: si annota nella lista della
  // spesa, con la frase cercata come provenienza.
  esploraNellaLista: (url: string, frase: string) =>
    invoke<boolean>("esplora_nella_lista", { url, frase }),
  // Quel che la licenza permette di tenere: una riga in coda di prelievo, con
  // l'indirizzo esatto che si aveva davanti. `false` se c'era già. Non avvia
  // niente: la passata si accende da Importazioni, perché prende **tutte** le
  // righe in attesa e accenderla da qui farebbe partire scaricamenti che
  // nessuno ha chiesto adesso.
  esploraTieni: (url: string, frase: string) =>
    invoke<boolean>("esplora_tieni", { url, frase }),
  importAnteprima: (url: string, forza = false) =>
    invoke<AnteprimaImport>("import_anteprima", { url, forza }),
  importPiano: (url: string, creaPlaylist: boolean) =>
    invoke<EsitoImport>("import_piano", { url, creaPlaylist }),
  importEsegui: (url: string, creaPlaylist: boolean) =>
    invoke<EsitoImport>("import_esegui", { url, creaPlaylist }),
  // Il comando che rende leggibile un guasto invece di lasciare «non funziona»:
  // dice quali cataloghi rispondono adesso e da quali si può tenere una copia.
  // **Tocca la rete**, al contrario di `scaricoStato`: è l'unico posto in cui
  // farlo è giusto, perché è l'unico che l'utente apre per saperlo.
  importDiagnostica: () => invoke<DiagnosticaImport>("import_diagnostica"),
  /**
   * Il rapporto di un'importazione passata, o `null` se non c'è più.
   *
   * `null` **non è un errore**: è un'importazione più vecchia della persistenza,
   * o una di cui il rapporto è stato potato. Chi chiama lo distingue, perché
   * dirlo come guasto manderebbe qualcuno a cercare una riparazione che non
   * esiste.
   */
  importRapporto: (sourceId: string) =>
    invoke<EsitoImport | null>("import_rapporto", { sourceId }),
  /**
   * Gli ultimi rapporti, per la pagina: le concluse in sessioni passate.
   *
   * Si chiede un limite perché `desiderati` non cancella mai niente e i rapporti
   * seguono la stessa regola: un elenco che cresce per sempre va potato prima o
   * poi, e potarlo dalla finestra non si può — quindi si chiede solo quel che si
   * mostra.
   */
  importRapporti: (limite: number) =>
    invoke<EsitoImport[]>("import_rapporti", { limite }),

  // ── la procura dei desiderati ────────────────────────────────────────────
  // I brani che l'archivio nomina e la libreria non ha si cercano nei
  // **cataloghi liberi**: Internet Archive, e — quando ci saranno — Jamendo e
  // Audius. Non da Spotify, che non consegna file a nessuno, e non da YouTube,
  // le cui *API Developer Policies* (§ III.E.1.a) vietano di scaricare,
  // memorizzare o mettere in cache i loro contenuti. Nessuna parte di Rymembra ci
  // prova più.
  //
  // Il nome IPC è rimasto `scarica_*`, e vale la pena dire perché: cambiarlo
  // avrebbe rotto ogni chiamante per guadagnare una parola più giusta, e la
  // parola giusta sta nel modulo di là — `procura.rs` — dove chi legge il
  // codice la trova.
  //
  // Quel che il nucleo trova, lo prende **solo se la licenza lo consente**, e
  // la verifica avviene prima di qualunque richiesta. Quel che non trova
  // diventa una riga `introvabile`, che alimenta «Da comprare» invece di
  // ritentare per sempre.
  //
  // `scaricaDesiderati` torna **subito**: la coda gira su un filo suo di là, e
  // quel che succede arriva sui due eventi `scarico:*`. Chiamarlo mentre una
  // coda gira non ne avvia una seconda — quella in corso rilegge la tabella a
  // ogni lotto, quindi i brani appena importati li prende comunque.
  //
  // Di norma non serve chiamarlo: dopo un'importazione la coda parte da sé.
  // Resta per il caso in cui era stata annullata.
  scaricaDesiderati: () => invoke<StatoScarico>("scarica_desiderati"),
  // Torna subito anche questo: fermarsi vuol dire «appena il brano in corso si
  // interrompe», non «adesso». Il prelievo controlla il bit fra un blocco e
  // l'altro e scrive in una cartella temporanea, quindi il brano a metà non
  // lascia niente nella cartella sorvegliata.
  annullaScarico: () => invoke<void>("annulla_scarico"),
  scaricoStato: () => invoke<StatoScarico>("scarico_stato"),
  // Rimette in fila **solo** i falliti, mai gli introvabili: vedi
  // `ConteggiScarico`. Restituisce quanti ne sono tornati in coda, e se sono
  // più di zero riavvia la coda da sé.
  riprovaFalliti: () => invoke<number>("riprova_falliti"),
  // Quel che resta da comprare: le righe `introvabile`, distinte per titolo e
  // artista. Un comando a parte e non un campo di `scaricoStato`, che viene
  // chiamato decine di volte in una coda: questa lista può essere lunga come la
  // libreria di qualcuno, e vive in una sezione che di norma è chiusa.
  daComprare: (limite: number) =>
    invoke<DaComprare[]>("da_comprare", { limite }),
  // Il negozio si **nomina**, non si indirizza: l'indirizzo lo costruisce il
  // nucleo da un elenco chiuso di tre domini. Aprire un indirizzo nel browser di
  // sistema è l'unica capacità pericolosa che la finestra ha, e farle scegliere
  // *quale* vorrebbe dire darla intera a una pagina compromessa.
  cercaDoveComprare: (
    negozio: "bandcamp" | "qobuz" | "discogs",
    cosa: string,
  ) => invoke<void>("cerca_dove_comprare", { negozio, cosa }),
  // Accetta o rifiuta le registrazioni diverse da quella chiesta — concerti,
  // riletture. **Non** tocca quel che è già stato preso: decide cosa fa la coda
  // da adesso in poi. Restituisce lo stato aggiornato, così l'interruttore
  // riflette quel che il nucleo ha davvero scritto.
  alternativeAmmettile: (ammesse: boolean) =>
    invoke<StatoScarico>("alternative_ammettile", { ammesse }),

  // ── riproduzione ─────────────────────────────────────────────────────────
  // Nessuno di questi comandi restituisce lo stato: lo mandano tutti
  // sull'evento `riproduzione:stato`, e averne una sola sorgente è ciò che
  // impedisce alla finestra di credersi in pausa mentre il motore suona.
  suona: (brani: number[], indice: number) =>
    invoke<void>("suona", { brani, indice }),
  // La coda la costruisce il nucleo: sono trenta identificativi che la finestra
  // non guarda mai, e farseli mandare per rimandarli indietro sarebbe un
  // viaggio dell'IPC per niente.
  radio: (brano: number) => invoke<void>("radio", { brano }),
  pausa: () => invoke<void>("pausa"),
  riprendi: () => invoke<void>("riprendi"),
  alterna: () => invoke<void>("alterna"),
  prossimo: () => invoke<void>("prossimo"),
  precedente: () => invoke<void>("precedente"),
  vaiA: (ms: number) => invoke<void>("vai_a", { ms }),
  volume: (volume: number, muto: boolean) =>
    invoke<void>("volume", { volume, muto }),
  riproduzioneStato: () => invoke<StatoRiproduzione>("riproduzione_stato"),
  ripeti: () => invoke<void>("ripeti"),
  mescola: () => invoke<void>("mescola"),

  // ── l'equalizzatore ──────────────────────────────────────────────────────
  // `equalizzatore` è l'unico comando di riproduzione che **non** manda
  // `riproduzione:stato`: comporre quello stato richiede una lettura del brano
  // corrente dal database, e questo parte una dozzina di volte al secondo
  // finché un cursore è sotto il dito. Manda `riproduzione:eq`, che sono due
  // campi e nessuna query.
  equalizzatore: (guadagni: number[], attivo: boolean) =>
    invoke<void>("equalizzatore", { guadagni, attivo }),
  // La normalizzazione invece manda `riproduzione:stato`: arriva quando un dito
  // preme un interruttore, non dodici volte al secondo. Sui brani senza tag
  // ReplayGain non sposta niente in nessuna delle due posizioni — la
  // correzione esiste solo dove c'è un guadagno dichiarato da rispettare.
  normalizzazione: (livello: Normalizzazione) =>
    invoke<void>("normalizzazione", { livello }),
  // Il modo non viaggia nello stato della riproduzione: lo guarda solo il
  // selettore delle Impostazioni, che se lo chiede da sé. La scelta riporta
  // com'è rimasta, e vale dal prossimo brano che si apre.
  normalizzazioneModo: () => invoke<ModoLivello>("normalizzazione_modo"),
  normalizzazioneModoScegli: (modo: ModoLivello) =>
    invoke<ModoLivello>("normalizzazione_modo_scegli", { modo }),
  normalizzazioneAvanzamento: () =>
    invoke<AvanzamentoVolume>("normalizzazione_avanzamento"),
  // Il timer di spegnimento. `minuti` a zero lo spegne, `FINE_DEL_BRANO` chiede
  // di fermarsi dove finisce quel che suona. I minuti e non un istante: «fra
  // mezz'ora» è quel che si intende, e un istante calcolato qui sarebbe
  // calcolato con l'orologio della finestra.
  spegnimento: (minuti: number) => invoke<void>("spegnimento", { minuti }),
  // La coda che non finisce. Come sopra: lo stato torna sull'evento, non da
  // qui, così l'interruttore si muove quando il nucleo ha davvero cambiato
  // idea.
  autoplay: (attivo: boolean) => invoke<void>("autoplay", { attivo }),
  // La dissolvenza incrociata, in secondi. Zero la spegne e riporta il
  // passaggio a com'era: esatto al campione. Il nucleo taglia a
  // `DISSOLVENZA_MASSIMA_S` e rilegge quel che ha scritto, quindi il valore
  // che torna sull'evento è quello vero, non quello chiesto.
  dissolvenza: (secondi: number) => invoke<void>("dissolvenza", { secondi }),
  // Di quanto la catena d'uscita ritarda il suono, in millisecondi: il pezzo che
  // nessuna misura vede — mixer di sistema, driver, DAC, e su un'uscita senza
  // fili la radio. Il motore ci somma quel che cpal gli riporta e toglie il
  // totale dalla posizione raccontata, così cursore, testi e pannello di Windows
  // parlano di quel che l'orecchio riceve adesso. Non tocca il suono e non
  // sposta la fine di un brano. Risponde col numero **rimasto scritto**: il
  // nucleo taglia a ±`LATENZA_MASSIMA_MS`, e il cursore deve fermarsi dove si è
  // fermato davvero.
  latenza: (ms: number) => invoke<number>("latenza", { ms }),
  // Riapre il dispositivo audio a mano. Il nucleo lo fa già da sé quando
  // l'elenco delle uscite cambia; questo tasto resta per i guasti che
  // dall'elenco non si vedono — un driver piantato, un'apertura esclusiva
  // rubata da un'altra applicazione — dove il dispositivo c'è ancora e ha
  // ancora lo stesso nome. Sopravvivono coda, volume, curva, normalizzazione
  // **e** la posizione; la musica riparte se stava andando.
  riapriAudio: () => invoke<void>("riapri_audio"),
  // Le uscite che ci sono, più quella scelta che adesso non c'è. Serve al
  // primo disegno: dopo, l'elenco arriva da sé sull'evento
  // `riproduzione:dispositivi`, e non c'è nessun tasto «aggiorna» da premere.
  dispositiviAudio: () => invoke<DispositivoAudio[]>("dispositivi_audio"),
  // Da quale uscita far sentire Rymembra. `null` è «quella di sistema», ed è una
  // scelta come le altre: dice di seguire il sistema quando cambia, non di non
  // avere preferenze. Il dispositivo si riapre subito, e il brano riparte da
  // dov'era.
  scegliDispositivoAudio: (id: string | null) =>
    invoke<void>("scegli_dispositivo_audio", { id }),
  // Riprende il brano che una cartella di rete aveva interrotto, dal punto in
  // cui la musica si era fermata. È il tasto «Riprova» dell'avviso di rete: se
  // il NAS è ancora spento fallisce con lo stesso errore ritentabile e si può
  // premere di nuovo, se nel frattempo si è ascoltato altro non fa niente.
  riprovaCorrente: () => invoke<void>("riprova_corrente"),
  // Lo spettro si accende e si spegne: acceso, la callback audio scrive i
  // campioni in un terzo anello e un filo manda `riproduzione:spettro` trenta
  // volte al secondo. Spento non costa niente da nessuna delle due parti, ed è
  // la ragione per cui è un comando invece di essere sempre acceso.
  spettro: (attivo: boolean) => invoke<void>("spettro", { attivo }),
  // Quante barre disegna la scena dello spettro. La scelta sta in `settings`,
  // non in `localStorage`, perché `localStorage` non sopravvive a una
  // reinstallazione.
  //
  // Qui c'era scritto che «le preferenze viaggiano con il backup e con la
  // sincronia»: non era vero per questa chiave. Il backup su Drive copia due
  // righe di `settings`, la sincronia tre, e nessuna delle due è questa.
  // L'unico meccanismo che porta una preferenza su un altro computer è il
  // profilo, ed è un elenco di inclusioni in cui questa chiave non c'era.
  // Adesso c'è, insieme a `player.spectrum.visible`.
  spettroBande: () => invoke<number>("spettro_bande"),
  // Riporta quante ne sono rimaste: fra le potenze di due non c'è niente, e un
  // numero che non è una di quelle si porta alla più vicina invece di essere
  // rifiutato. Chi ha premuto deve vedere accesa la linguetta vera.
  spettroBandeScegli: (quante: number) =>
    invoke<number>("spettro_bande_scegli", { quante }),
  // Se la scena parte accesa. Di serie no — chi apre «In riproduzione» è venuto
  // a guardare la copertina — ma la scelta si ricorda, e viaggia nel profilo:
  // «voglio vedere lo spettro» resta vero su qualunque computer.
  spettroVisibile: () => invoke<boolean>("spettro_visibile"),
  // Riporta com'è rimasta: il nucleo scrive, rilegge e risponde con quel che
  // c'è nel database. L'interruttore si dipinge da lì e non dal click, così una
  // scrittura fallita non lascia acceso qualcosa che domani sarà spento.
  spettroVisibileScegli: (acceso: boolean) =>
    invoke<boolean>("spettro_visibile_scegli", { acceso }),
  // Il tetto di qualità della scena. Questa **non** viaggia nel profilo: è un
  // fatto di questa macchina, come l'uscita audio.
  spettroQualita: () => invoke<QualitaSpettro>("spettro_qualita"),
  // Entra una stringa ed esce un livello, e l'asimmetria è il ritaglio reso
  // visibile: un nome che non è uno dei tre vale `auto`, e la risposta è il
  // livello vero — non quello chiesto.
  spettroQualitaScegli: (livello: QualitaSpettro) =>
    invoke<QualitaSpettro>("spettro_qualita_scegli", { livello }),
  // Se i dati tecnici del file si mostrano sotto i comandi. Di serie **sì**, al
  // contrario dello spettro: quella riga non costa né una GPU né un filo, e chi
  // apre «In riproduzione» ha il diritto di sapere cosa sta sentendo senza prima
  // scoprire che esiste un interruttore. Lo chiede solo l'interruttore delle
  // Impostazioni: le due viste che disegnano la riga non leggono la preferenza,
  // guardano `stato.formato`.
  formatoVisibile: () => invoke<boolean>("formato_visibile"),
  // Riporta com'è rimasta, come per lo spettro. In più rimanda lo stato della
  // riproduzione: il nucleo decide se il dato viaggia, quindi senza quel rinvio
  // la riga resterebbe com'era fino al prossimo cambio di brano.
  formatoVisibileScegli: (acceso: boolean) =>
    invoke<boolean>("formato_visibile_scegli", { acceso }),
  eqPresetElenco: () => invoke<VocePreset[]>("eq_preset_elenco"),
  // Salva la curva **corrente**, quella che si sta ascoltando: il nome è
  // l'unica cosa che serve passare. `false` se il nome era vuoto; un nome che
  // c'è già sostituisce quella curva invece di affiancarla.
  eqPresetSalva: (nome: string) => invoke<boolean>("eq_preset_salva", { nome }),
  // `false` se non ce n'era una con quel nome. Quelle di serie non si cancellano.
  eqPresetCancella: (nome: string) =>
    invoke<boolean>("eq_preset_cancella", { nome }),

  // ── la coda ──────────────────────────────────────────────────────────────
  // Gli indici sono quelli dell'ORDINE DI RIPRODUZIONE, cioè dell'array `coda`
  // così come arriva: con lo shuffle acceso non è l'ordine interno del nucleo,
  // e ricalcolarlo qui vorrebbe dire riscrivere il mescolamento in TypeScript.
  codaAccoda: (brani: number[]) => invoke<void>("coda_accoda", { brani }),
  codaDopo: (brani: number[]) => invoke<void>("coda_dopo", { brani }),
  codaVai: (indice: number) => invoke<void>("coda_vai", { indice }),
  codaTogli: (indice: number) => invoke<void>("coda_togli", { indice }),
  codaRiordina: (da: number, a: number) =>
    invoke<void>("coda_riordina", { da, a }),
  codaSvuota: () => invoke<void>("coda_svuota"),
  // L'«Annulla» dell'avviso di coda sostituita. `numero` è quello arrivato con
  // l'evento `coda:sostituita`: `false` se nel frattempo la coda è stata
  // sostituita di nuovo, o se è già stata rimessa — l'avviso va chiuso comunque.
  codaRipristina: (numero: number) =>
    invoke<boolean>("coda_ripristina", { numero }),
  // Riempie una coda vuota senza farla partire: per il giro guidato. `false` se
  // la coda non era vuota, e allora non ha toccato niente.
  codaPrepara: (brani: number[]) => invoke<boolean>("coda_prepara", { brani }),
  braniPerId: (brani: number[]) => invoke<Brano[]>("brani_per_id", { brani }),

  // ── il backup su Drive ───────────────────────────────────────────────────
  // Cinque comandi restituiscono lo **stato intero** invece di un `void`: chi
  // collega, scollega o accende l'interruttore vuole vedere subito com'è finita,
  // e una seconda chiamata a `nuvolaStato` per scoprirlo lascerebbe un istante
  // in cui la schermata mostra la situazione di prima.
  nuvolaStato: () => invoke<StatoNuvola>("nuvola_stato"),
  // Apre il browser di **sistema** e aspetta il consenso, per non più di tre
  // minuti. Può quindi metterci a lungo: chi la chiama deve mostrare che sta
  // succedendo qualcosa.
  nuvolaCollega: () => invoke<StatoNuvola>("nuvola_collega"),
  // Ferma l'attesa del consenso aperta da `nuvolaCollega`. Torna subito: la
  // risposta vera è quella di `nuvolaCollega`, che finisce senza account e
  // senza errore, e l'evento `nuvola:stato`.
  nuvolaAnnulla: () => invoke<void>("nuvola_annulla"),
  // Scollega l'account Google del backup. L'app resta: è dell'utente, e la
  // usano forse anche le cartelle di Drive.
  nuvolaScollega: () => invoke<StatoNuvola>("nuvola_scollega"),
  // Sceglie l'app Google del backup (`null`: nessuna). Se l'app che vale
  // cambia davvero, l'account del backup si scollega: il suo token era
  // dell'app di prima.
  nuvolaBackupApp: (scelta: string | null) =>
    invoke<StatoNuvola>("nuvola_backup_app", { scelta }),
  nuvolaAttiva: (attivo: boolean) =>
    invoke<StatoNuvola>("nuvola_attiva", { attivo }),
  // Torna **subito**: sveglia il filo di sottofondo e basta. L'esito arriva
  // sull'evento `nuvola:stato`, come per la riproduzione — un comando che
  // aspettasse la fine del caricamento terrebbe fermo il canale per minuti.
  nuvolaSalva: () => invoke<void>("nuvola_salva"),
  // `piano` scarica e non applica; `ripristina` **riscarica e ricalcola**
  // invece di ricevere il piano mostrato, perché fra l'anteprima e la conferma
  // può essere finita una scansione.
  nuvolaPianoRipristino: () =>
    invoke<PianoRipristino>("nuvola_piano_ripristino"),
  nuvolaRipristina: () => invoke<EsitoRipristino>("nuvola_ripristina"),

  // ── le cartelle dal cloud ────────────────────────────────────────────────
  // I servizi che questa build sa offrire, nell'ordine in cui mostrarli.
  nuvolaServizi: () => invoke<ServizioOfferto[]>("nuvola_servizi"),
  // I conti collegati, con le loro radici. Cambiano da soli — un token
  // revocato, un elenco che finisce — e lo dice l'evento `nuvola:conti`, che li
  // porta tutti a ogni cambiamento.
  nuvolaConti: () => invoke<ContoNuvola[]>("nuvola_conti"),
  //
  // I tre gesti sul conto hanno un nome loro, e non quello del contratto
  // (`nuvola_collega`, `nuvola_annulla`, `nuvola_scollega`): quei tre comandi
  // esistono già, e sono del backup qui sopra. Due comandi Tauri con lo stesso
  // nome non convivono, e riusare `nuvola_annulla` vorrebbe dire contare su
  // un cambio del backup che da qui non si vede. Stanno vicini apposta: se il
  // nucleo sceglie altri nomi, si cambiano qui e in nessun altro punto.
  //
  // Apre il browser di **sistema** (OneDrive, Google Drive, Dropbox) e aspetta
  // il consenso fino a tre minuti; per MEGA e WebDAV prova le credenziali del
  // modulo. Se l'account era già collegato torna quello. `appId` è l'app con
  // cui chiedere il consenso — `null` la predefinita della piattaforma, e per
  // ricollegare un conto quella sua (`ContoNuvola.app`). Se l'indirizzo di
  // ritorno non è registrato, Microsoft e Dropbox mostrano l'errore nel
  // browser e qui non torna niente fino ad «Annulla» o ai tre minuti.
  nuvolaContoCollega: (
    servizio: ServizioNuvola,
    appId: string | null,
    modulo?: ModuloCollega,
  ) =>
    invoke<ContoNuvola>("nuvola_conto_collega", {
      servizio,
      appId,
      modulo: modulo ?? null,
    }),
  // Ferma l'attesa del consenso. Torna subito; la risposta vera è quella di
  // `nuvolaContoCollega`, che chi ha annullato non ascolta più.
  nuvolaContoAnnulla: () => invoke<void>("nuvola_conto_annulla"),
  // Con `togliCartelle` falso le cartelle restano, attenuate, «da ricollegare»:
  // ascolti, voti e playlist dei loro brani aspettano il prossimo collegamento.
  nuvolaContoScollega: (conto: string, togliCartelle: boolean) =>
    invoke<void>("nuvola_conto_scollega", { conto, togliCartelle }),
  //
  // Le sottocartelle di una cartella remota; `null` è la cima del drive. Solo
  // cartelle, già in ordine di nome: il selettore le mostra così come arrivano.
  nuvolaCartelle: (conto: string, dentro: string | null) =>
    invoke<CartellaRemota[]>("nuvola_cartelle", { conto, dentro }),
  // Aggiunge una cartella remota alle sorvegliate. `nomi` sono le briciole
  // dalla cima, per il messaggio d'attesa: il percorso vero lo ricava il
  // nucleo. Torna come `impostaCartelle`, e la finestra lo tratta allo stesso
  // modo (`dopoLeCartelle`).
  nuvolaAggiungi: (conto: string, id: string, nomi: string[]) =>
    invoke<CartelleOrdinate>("nuvola_aggiungi", { conto, id, nomi }),
  // Torna **subito**: chiede un giro al filo del conto. L'esito arriva con
  // `nuvola:conti`, come per il backup.
  nuvolaAggiorna: (conto: string) => invoke<void>("nuvola_aggiorna", { conto }),
  // La cache su disco dei brani del cloud: ogni gesto torna lo stato intero,
  // così il numero di quel che è occupato si aggiorna insieme al tasto.
  nuvolaCacheStato: () => invoke<StatoCacheNuvola>("nuvola_cache_stato"),
  nuvolaCacheImposta: (attiva: boolean, tettoMb: number) =>
    invoke<StatoCacheNuvola>("nuvola_cache_imposta", { attiva, tettoMb }),
  nuvolaCacheSvuota: () => invoke<StatoCacheNuvola>("nuvola_cache_svuota"),
  //
  // Le app dell'utente. Ogni gesto torna l'elenco intero, e lo manda anche
  // l'evento `nuvola:app` — dopo il quale `nuvolaServizi` va riletto: la
  // disponibilità di un servizio è «la sua piattaforma ha un'app». La forma
  // degli identificativi la decide il nucleo (`cloud.appIdInvalid`); quella
  // detta prima di mandarli sta in `appNuvola.ts`.
  nuvolaApp: () => invoke<AppNuvolaIpc>("nuvola_app"),
  // Cambiare l'identificativo di un'app usata da qualcuno spegne i suoi conti
  // («da ricollegare», `cloud.appChanged`) e scollega il backup se è sua;
  // cambiare solo il segreto li riapre da soli. `sync.busy` se un collegamento
  // o una passata del backup sono in corso.
  nuvolaAppSalva: (app: AppDaSalvare) =>
    invoke<AppNuvolaIpc>("nuvola_app_salva", { ...app }),
  // I conti che la usavano restano, slegati, «da ricollegare»; cartelle e
  // brani restano. Se era l'app del backup, il backup torna da configurare.
  nuvolaAppTogli: (id: string) => invoke<AppNuvolaIpc>("nuvola_app_togli", { id }),
  // La pagina del file sul suo servizio, nel browser di sistema. Un
  // identificativo e mai un indirizzo, per la stessa ragione di
  // `branoApriPagina`.
  branoApriSulWeb: (id: number) => invoke<void>("brano_apri_sul_web", { id }),

  // ── la sincronia fra dispositivi ─────────────────────────────────────────
  // Stessa forma del backup, e per la stessa ragione: chi tocca un interruttore
  // deve vedere subito com'è finita. `sincroniaAdesso` fa eccezione e restituisce
  // il **resoconto** — è l'unica cosa che rende leggibile un automatismo che
  // scrive nella libreria da solo, e chi ha appena premuto «Sincronizza adesso»
  // sta guardando proprio per sapere cosa è cambiato.
  sincroniaStato: () => invoke<StatoSincronia>("sincronia_stato"),
  sincroniaAttiva: (accesa: boolean) =>
    invoke<StatoSincronia>("sincronia_attiva", { accesa }),
  // `cartella` conta solo quando `dove` è `"cartella"`. Il percorso viene creato
  // subito se non esiste: un errore deve arrivare mentre si è ancora davanti al
  // campo che lo ha causato, non fra cinque minuti da un filo di sottofondo.
  sincroniaMagazzino: (dove: "cartella" | "drive", cartella: string | null) =>
    invoke<StatoSincronia>("sincronia_magazzino", { dove, cartella }),
  // Questa **aspetta**: una passata a vuoto è un'elencazione, e su una cartella
  // condivisa non tocca nemmeno la rete.
  sincroniaAdesso: () => invoke<Resoconto>("sincronia_adesso"),
  sincroniaDispositivi: () =>
    invoke<DispositivoSincronia[]>("sincronia_dispositivi"),
  sincroniaAccoppia: (id: string, nome: string | null) =>
    invoke<DispositivoSincronia[]>("sincronia_accoppia", { id, nome }),
  sincroniaDimentica: (id: string) =>
    invoke<DispositivoSincronia[]>("sincronia_dimentica", { id }),

  // ── ascolta insieme ──────────────────────────────────────────────────────
  // Chi cambia qualcosa riceve lo stato intero, come per la sincronia; il resto
  // arriva dall'evento `insieme:stato`, che il nucleo manda a ogni cambiamento
  // — anche a quelli decisi da un altro PC, che nessun comando di qui vede.
  insiemeStato: () => invoke<StatoInsieme>("insieme_stato"),
  insiemeAttiva: (attivo: boolean) =>
    invoke<StatoInsieme>("insieme_attiva", { attivo }),
  insiemeNome: (nome: string) => invoke<StatoInsieme>("insieme_nome", { nome }),
  insiemeFirewallVisto: () => invoke<void>("insieme_firewall_visto"),
  insiemeOspita: () => invoke<StatoInsieme>("insieme_ospita"),
  insiemeTermina: () => invoke<StatoInsieme>("insieme_termina"),
  insiemeEsci: () => invoke<StatoInsieme>("insieme_esci"),
  insiemeCerca: () => invoke<void>("insieme_cerca"),
  insiemeAssocia: (vicino: string) =>
    invoke<void>("insieme_associa", { vicino }),
  insiemeCodice: (associazione: number, uguale: boolean) =>
    invoke<void>("insieme_codice", { associazione, uguale }),
  insiemeInvita: (pari: number, diritti: DirittiInsieme) =>
    invoke<void>("insieme_invita", { pari, diritti }),
  insiemeRispondi: (invito: number, accetta: boolean) =>
    invoke<void>("insieme_rispondi", { invito, accetta }),
  insiemeChiedi: (pari: number) => invoke<void>("insieme_chiedi", { pari }),
  insiemeAmmetti: (richiesta: number, accetta: boolean, diritti: DirittiInsieme) =>
    invoke<void>("insieme_ammetti", { richiesta, accetta, diritti }),
  insiemeAllontana: (membro: number) =>
    invoke<void>("insieme_allontana", { membro }),
  insiemeConcedi: (membro: number, diritti: DirittiInsieme) =>
    invoke<void>("insieme_concedi", { membro, diritti }),
  insiemePariRinomina: (id: number, nome: string) =>
    invoke<StatoInsieme>("insieme_pari_rinomina", { id, nome }),
  insiemePariDimentica: (id: number) =>
    invoke<StatoInsieme>("insieme_pari_dimentica", { id }),
  insiemePariBlocca: (id: number, bloccato: boolean) =>
    invoke<StatoInsieme>("insieme_pari_blocca", { id, bloccato }),
  insiemeIndirizzi: (indirizzi: string[]) =>
    invoke<StatoInsieme>("insieme_indirizzi", { indirizzi }),
  insiemeCalibra: (attiva: boolean) =>
    invoke<void>("insieme_calibra", { attiva }),

  // ── gli aggiornamenti ────────────────────────────────────────────────────
  // Il controllo vero non passa mai di qui: lo fa un filo di sottofondo — una
  // volta a ogni avvio, poi ogni mezz'ora — e quel che la finestra vede arriva
  // dall'evento
  // `aggiornamenti:stato`. Anche `aggiornamentiAdesso` si limita a svegliare
  // quel filo e a restituire lo stato di **prima**, apposta: una via sola per
  // raccontare com'è andata, invece di due che possono dire cose diverse.
  aggiornamentiStato: () => invoke<StatoAggiornamenti>("aggiornamenti_stato"),
  // Spegnendolo si butta via anche l'aggiornamento già trovato: lasciare
  // l'avviso in piedi dopo che qualcuno ha chiesto di non essere avvisato
  // sarebbe rispondere «va bene» e continuare come prima.
  aggiornamentiAttivo: (attivo: boolean) =>
    invoke<StatoAggiornamenti>("aggiornamenti_attivo", { attivo }),
  aggiornamentiAdesso: () => invoke<StatoAggiornamenti>("aggiornamenti_adesso"),
  // «Non ora», e vale per quella versione sola.
  aggiornamentiSalta: (versione: string) =>
    invoke<StatoAggiornamenti>("aggiornamenti_salta", { versione }),
  // Torna **subito**: scaricare settanta megabyte e lanciare un installer non
  // sta dentro una chiamata IPC. Quel che succede arriva da
  // `aggiornamenti:avanzamento`, e se va bene l'ultima cosa che questa finestra
  // fa è chiudersi — su Windows è l'installer NSIS a terminare Rymembra e a
  // riaprirlo.
  aggiornamentiInstalla: () => invoke<void>("aggiornamenti_installa"),
  // Ferma lo scaricamento in corso. Torna subito; lo stato con
  // `installazione: false` arriva da `aggiornamenti:stato`. Dopo che
  // l'installer è partito non ferma più niente.
  aggiornamentiAnnulla: () => invoke<void>("aggiornamenti_annulla"),

  // ── il diario ────────────────────────────────────────────────────────────
  // L'unica cosa che la finestra può fare col diario: farlo vedere. Non lo
  // legge e non lo manda a nessuno — un log che l'applicazione sa spedire da
  // sé è telemetria, e `PRIVACY.md` dice che non ce n'è. Qui si apre la
  // cartella nel gestore file, e cosa farne lo decide chi guarda.
  diarioApri: () => invoke<void>("diario_apri"),
  // L'altra metà: scriverci dentro un guasto del davanti. Il diario del nucleo
  // raccoglieva tutto quel che succede sotto e niente di quel che succede
  // sopra, e in rilascio non c'è una console: un errore JavaScript non
  // catturato lasciava la finestra bianca e nessuna traccia. `dove` finisce
  // nella parentesi quadra del diario («finestra», «promessa», «recinto»),
  // `cosa` è il messaggio — che il nucleo riduce a una riga sola prima di
  // scriverlo, perché uno stack trace si porta dietro i percorsi del disco.
  //
  // Non restituisce errori e non ne lancia: è chiamato da un gestore di errori,
  // e un gestore di errori che fallisce non ha nessuno a cui dirlo.
  diarioAnnota: (dove: string, cosa: string) =>
    invoke<void>("diario_annota", { dove, cosa }).catch(() => {}),

  // La manutenzione: quanto pesa la cache delle copertine, com'è andato il
  // controllo del database, e la pulizia a mano. Tutte e due in disparte nel
  // nucleo: misurare la cache vuol dire leggere centinaia di cartelle.
  manutenzioneStato: () => invoke<StatoManutenzione>("manutenzione_stato"),
  // Il solo esito del controllo, senza misurare niente: lo chiede l'avviso di
  // database danneggiato all'apertura della finestra, che può nascere dopo
  // l'unico `manutenzione:salute` della sessione.
  manutenzioneSalute: () => invoke<SaluteDatabase>("manutenzione_salute"),
  manutenzionePulisciCopertine: () =>
    invoke<EsitoPulizia>("manutenzione_pulisci_copertine"),

  // I documenti pubblici, nel browser di sistema. Il nome e non l'indirizzo: di
  // là c'è un elenco chiuso, e un comando che aprisse l'indirizzo che gli si
  // passa sarebbe un comando che apre qualunque indirizzo — dall'interno di un
  // programma di cui ci si fida.
  apriDocumento: (
    quale:
      | "repository"
      | "segnalazioni"
      | "licenza"
      | "terze"
      | "privacy"
      | "condizioni"
      // Non un documento, ma la stessa serratura: l'elenco di là non tiene «i
      // testi legali», tiene gli indirizzi che questa finestra ha il permesso
      // di far aprire.
      | "donazioni"
      // Le tre console in cui l'utente registra la sua app: vedi
      // `parti/AppServizio.tsx`.
      | `console-${ChiavePiattaforma}`,
  ) => invoke<void>("apri_documento", { quale }),

  // La documentazione tecnica sul sito, alla voce di una scheda delle
  // Impostazioni. L'ancora e non l'indirizzo, per la stessa ragione di sopra:
  // di là c'è un elenco chiuso di ancore, e quel che non ne fa parte si
  // rifiuta. Niente lingua: la documentazione è in inglese e basta.
  apriGuida: (ancora: string) => invoke<void>("apri_guida", { ancora }),

  // ── l'account Spotify intero ───────────────────────────────
  // Il flusso è a tre tempi come per un link — anteprima, piano, conferma — ma
  // quel che si legge resta di là, in una cella. Qui viaggiano solo i conteggi:
  // un account sono decine di migliaia di brani più anni di cronologia, e
  // serializzarli tre volte per mostrarne il totale non ha senso.
  //
  // Una via sola: lo zip che Spotify consegna su richiesta. Il consenso OAuth
  // c'era e non c'è più — l'archivio è dell'utente per diritto di portabilità
  // (GDPR art. 20), mentre cosa si possa fare dei dati della Web API lo decide
  // il *Spotify Developer Policy*, e una libreria che li tiene per anni non ci
  // sta dentro.
  accountStato: () => invoke<StatoAccount>("account_stato"),
  // Lo zip arriva in due pezzi separati da settimane — i dati dell'account e la
  // cronologia estesa — e se ne può aprire uno solo: quel che manca resta
  // vuoto, e l'anteprima lo dice.
  archivioApri: (percorso: string) =>
    invoke<AnteprimaAccount>("archivio_apri", { percorso }),
  // Butta via quel che è in cella. Non succede da solo dopo l'importazione,
  // perché importare due volte con scelte diverse è una cosa che si fa; ma un
  // archivio di dieci anni sono centinaia di megabyte in memoria, e questo è il
  // tasto che li restituisce.
  archivioDimentica: () => invoke<StatoAccount>("archivio_dimentica"),
  // Tutti e due lavorano su quel che è in cella.
  // Il piano è l'importazione vera dentro una transazione abbandonata: i numeri
  // che mostra sono quelli che si otterranno, non una previsione.
  accountPiano: (scelte: ScelteAccount) =>
    invoke<EsitoAccount>("account_piano", { scelte }),
  accountImporta: (scelte: ScelteAccount) =>
    invoke<EsitoAccount>("account_importa", { scelte }),
  // L'operazione che `play_history.source` esiste per rendere possibile.
  // Restituisce quante righe se ne sono andate. I conteggi d'ascolto **non**
  // scendono: `merge_stats` non sa scendere, e non deve — vedi la nota di là.
  cronologiaDimenticaImportati: () =>
    invoke<number>("cronologia_dimentica_importati"),

  // ── i testi ──────────────────────────────────────────────────────────────
  // `testoBrano` non tocca la rete: dice quel che si sa già dal disco — il
  // sidecar accanto al file, la riga in tabella, il tag — e lo dice subito.
  // Quando risponde `daChiedere: true`, c'è ancora qualcuno a cui chiedere.
  testoBrano: (id: number) => invoke<TestoBrano>("testo_brano", { id }),
  // Lo scarto sta sul brano, non sul file: chi ha lo stesso brano in FLAC e in
  // mp3 lo corregge una volta sola.
  testoScarto: (id: number, scartoMs: number) =>
    invoke<void>("testo_scarto", { id, scartoMs }),
  // Questo invece la rete la tocca, ed è per questo che lo chiama solo il
  // pannello del testo quando è aperto: chiedere un testo dice al catalogo
  // cosa si sta ascoltando. Con l'interruttore spento risponde quel che si sa
  // dal disco, senza fallire — spegnere non è un errore.
  testoCerca: (id: number) => invoke<TestoBrano>("testo_cerca", { id }),
  // Il ritentativo esplicito, e non un doppione del precedente: fra il
  // pannello e la rete ci sono due memorie — il deposito di `rymembra-meta` e la
  // colonna `checked_at` — e con `testoCerca` tutt'e due risponderebbero prima
  // che parta una richiesta. Chi preme «Cerca di nuovo» rivedrebbe lo stesso
  // vuoto e concluderebbe che il pulsante è finto. Questo le salta entrambe.
  // Va chiamato solo su un gesto: costa una richiesta a un servizio pubblico.
  testoCercaDiNuovo: (id: number) =>
    invoke<TestoBrano>("testo_cerca_di_nuovo", { id }),
  // Tutte le voci del catalogo per questo brano, senza nessuna scelta: la fa
  // chi guarda. Con la rete dei testi spenta torna `rete: false` e nessuna
  // voce. Anche questo costa delle richieste, e va chiamato solo su un gesto.
  testoCandidati: (id: number) =>
    invoke<CandidatiTesto>("testo_candidati", { id }),
  // Il testo che torna è quello che il pannello deve mostrare adesso: la voce
  // scelta, a meno che un sidecar o un testo sincronizzato a mano vincano.
  testoScegli: (id: number, candidato: number) =>
    invoke<TestoBrano>("testo_scegli", { id, candidato }),
  // «Nessuno di questi»: toglie il testo arrivato dal catalogo, e il brano resta
  // segnato come cercato.
  testoRifiuta: (id: number) => invoke<TestoBrano>("testo_rifiuta", { id }),
  // Un pannello del testo si apre o si chiude. Il nucleo precarica il testo del
  // brano dopo solo mentre almeno un pannello è aperto: vedi `PRIVACY.md`.
  testiPannello: (aperto: boolean) =>
    invoke<void>("testi_pannello", { aperto }),
  testiStato: () => invoke<StatoTesti>("testi_stato"),
  testiRete: (attivo: boolean) => invoke<StatoTesti>("testi_rete", { attivo }),
  testiAccanto: (attivo: boolean) =>
    invoke<StatoTesti>("testi_accanto", { attivo }),
  // Torna subito: il lavoro va su un filo suo e l'avanzamento arriva con
  // l'evento `testi:avanzamento`.
  testiRiempi: () => invoke<StatoTesti>("testi_riempi"),
  testiFerma: () => invoke<void>("testi_ferma"),
  // Le battute date a orecchio tornano raddrizzate. Gli attacchi del brano non
  // attraversano l'IPC — sono centinaia per canzone, e qui non servirebbero a
  // niente: quel che serve è il risultato.
  testoAggancia: (id: number, battute: number[]) =>
    invoke<number[]>("testo_aggancia", { id, battute }),
  // Scrive gli `.lrc` accanto al brano e la riga in tabella. L'LRC lo compone
  // il nucleo, con la stessa funzione che lo rilegge.
  //
  // I file possono essere **due**: il `.lrc` di sempre, con i soli tempi di
  // riga, e — quando l'editor ha battuto anche le parole — un `.a2.lrc` con i
  // `<mm:ss.xx>` dentro la riga. Il secondo lo capiscono in pochi, e per tutti
  // gli altri quei `<…>` sarebbero testo stampato in mezzo alle parole: per
  // questo il primo resta, e non è un doppione ma la copia leggibile ovunque.
  //
  // `parole` non è facoltativo, e lo è di proposito **anche qui**: il campo in
  // Rust non ha `serde(default)`, quindi una chiamata che lo omettesse non
  // fallirebbe a compilazione ma alla deserializzazione, cioè col brano già
  // sincronizzato e il salvataggio che non arriva. Vuoto è la risposta giusta
  // per una riga di cui le parole non si sono battute — e va scritto.
  //
  // Nessun offset fra gli argomenti, e non è una svista: i due numeri che
  // portano quel nome — lo `[offset:]` del file e lo `scartoMs` di chi ascolta —
  // li azzera entrambi il nucleo, perché queste battute sono misurate sulla
  // posizione **grezza**. Il perché per esteso sta su `testo_salva`.
  testoSalva: (
    id: number,
    righe: {
      ms: number;
      testo: string;
      parole: { ms: number; testo: string }[];
    }[],
  ) =>
    invoke<{ testo: TestoBrano; fileNonScritto: ErroreIpc | null }>(
      "testo_salva",
      { id, righe },
    ),
  // Il verso opposto, e mai automatico: manda a LRCLIB il testo che si è
  // sincronizzato a mano, uno per volta e solo dopo averlo visto. Ci mette
  // secondi, e non per la rete — il catalogo chiede una prova di lavoro. Il
  // nucleo rifiuta da sé quel che non ha `fonte: "mano"`, così la regola non
  // dipende da questo file.
  testoPubblica: (id: number) => invoke<void>("testo_pubblica", { id }),

  // ── l'arricchimento dei metadati ─────────────────────────────────────────
  // Non c'è un «arricchisci adesso»: la passata è automatica per scelta, e un
  // pulsante che la lancia a mano sarebbe la schermata di revisione travestita.
  // Quel che serve davvero sono l'interruttore e il modo di disfare.
  arricchimentoStato: () => invoke<StatoArricchimento>("arricchimento_stato"),
  arricchimentoAttiva: (attivo: boolean) =>
    invoke<StatoArricchimento>("arricchimento_attiva", { attivo }),
  // **Spegne anche l'interruttore**, e non è un effetto collaterale: annullare
  // rimette i brani fra i candidati, quindi con l'automatico acceso la passata
  // successiva riscriverebbe entro mezz'ora quel che si è appena disfatto.
  // Può metterci decine di secondi: riapre e riscrive un file per brano.
  arricchimentoAnnulla: () =>
    invoke<EsitoAnnullamento>("arricchimento_annulla"),

  // ── lo scrobbling ────────────────────────────────────────────────────────
  // Nessuno di questi comandi manda niente da sé, tranne `scrobbleInvia`: la
  // coda si svuota da sola su un filo di sottofondo, e quel che si accoda è
  // deciso dalla riproduzione. Qui ci sono le credenziali e i due gesti che
  // riguardano ciò che è rimasto indietro.
  scrobbleStato: () => invoke<StatoScrobble>("scrobble_stato"),
  // Spento, la coda **smette di riempirsi**: mettere in pausa e poi ritrovarsi
  // tre giorni di ascolti spediti insieme sarebbe una sorpresa.
  scrobbleAttivo: (attivo: boolean) =>
    invoke<StatoScrobble>("scrobble_attivo", { attivo }),
  // Il token si verifica **prima** di essere salvato: uno sbagliato non darebbe
  // nessun sintomo finché il primo ascolto non fallisce, ore dopo.
  scrobbleListenbrainzCollega: (token: string) =>
    invoke<StatoScrobble>("scrobble_listenbrainz_collega", { token }),
  scrobbleListenbrainzScollega: () =>
    invoke<StatoScrobble>("scrobble_listenbrainz_scollega"),
  // Vuoti cancellano e scollegano: una sessione ottenuta con un'altra chiave
  // continuerebbe a funzionare, e nasconderebbe che le nuove non vanno.
  scrobbleLastfmCredenziali: (apiKey: string, segreto: string) =>
    invoke<StatoScrobble>("scrobble_lastfm_credenziali", { apiKey, segreto }),
  // Il consenso di Last.fm è a due tempi e **in mezzo c'è una persona**: apre il
  // browser, e non succede più niente finché non si chiama `completa`. Last.fm
  // non richiama nessuno, non c'è nessun socket in ascolto.
  scrobbleLastfmCollega: () =>
    invoke<StatoScrobble>("scrobble_lastfm_collega"),
  scrobbleLastfmCompleta: () =>
    invoke<StatoScrobble>("scrobble_lastfm_completa"),
  scrobbleLastfmScollega: () =>
    invoke<StatoScrobble>("scrobble_lastfm_scollega"),
  // Svuota adesso invece di aspettare il filo. Può metterci minuti.
  scrobbleInvia: () => invoke<EsitoInvio>("scrobble_invia"),
  // Azzera i tentativi di chi li ha finiti. Serve dopo aver rimediato a quel
  // che li aveva fermati — ricollegare un account, aspettare che il servizio
  // torni su. Per un servizio solo, con la sua chiave stabile
  // (`Servizio::chiave`): ogni coda della schermata ha i suoi pulsanti, e il
  // nucleo rifiuta una chiave che non conosce (`ipc.payloadInvalid`).
  scrobbleRiprova: (servizio: "listenbrainz" | "lastfm") =>
    invoke<StatoScrobble>("scrobble_riprova", { servizio }),
  scrobbleDimentica: (servizio: "listenbrainz" | "lastfm") =>
    invoke<StatoScrobble>("scrobble_dimentica", { servizio }),
  // Solo verso ListenBrainz, e non è una preferenza: Last.fm rifiuta gli
  // ascolti con una data vecchia e ha un tetto giornaliero, quindi quarantamila
  // righe di cronologia là comparirebbero come «ignorate».
  scrobbleImportaCronologia: (soloImportati: boolean) =>
    invoke<number>("scrobble_importa_cronologia", { soloImportati }),

  // ── i modelli di linguaggio ──────────────────────────────────────────────
  // Spenti finché non si salva un profilo: senza, nessuno di questi comandi
  // apre un socket. Le chiavi stanno nel portachiavi del sistema e non
  // attraversano mai questo confine — quel che passa è `conChiave`.
  iaProfili: () => invoke<StatoIa>("ia_profili"),
  // `chiave`: `null` lascia stare quella che c'è, `""` la cancella, un valore
  // la sostituisce. Tre casi e non due, o cambiare il modello di un profilo
  // costringerebbe a reincollare il segreto.
  iaSalvaProfilo: (profilo: ProfiloIaDaSalvare, chiave: string | null) =>
    invoke<StatoIa>("ia_salva_profilo", { profilo, chiave }),
  // Cancella prima il segreto, e si ferma se non ci riesce: togliere il profilo
  // dall'elenco lascerebbe nel portachiavi una voce che nessuno sa più a chi
  // apparteneva.
  iaEliminaProfilo: (id: string) =>
    invoke<StatoIa>("ia_elimina_profilo", { id }),
  iaScegliProfilo: (id: string) => invoke<StatoIa>("ia_scegli_profilo", { id }),
  // È anche la prova di connessione: un servizio che risponde a questa risponde
  // a tutto — indirizzo giusto, processo acceso, chiave che passa.
  iaModelli: (id: string) => invoke<ModelloIa[]>("ia_modelli", { id }),
  // Torna **subito** il numero del turno, non la risposta: quella arriva a
  // pezzi sugli eventi `ia:pezzo`, chiusi da `ia:fine` o da `ia:errore`. Il
  // numero serve a riconoscere i propri: una risposta cominciata prima di un
  // «Ferma» può consegnare un blocco dopo.
  iaConversa: (messaggi: MessaggioIa[]) =>
    invoke<number>("ia_conversa", { messaggi }),
  // `false` se quel turno era già finito, e non è un errore: chi preme «Ferma»
  // un istante dopo l'ultimo gettone ha fatto la cosa giusta con un tempismo
  // sfortunato.
  iaFerma: (turno: number) => invoke<boolean>("ia_ferma", { turno }),
  // Non tocca niente: né rete, né disco, né documento. Le operazioni le applica
  // la finestra con le stesse funzioni pure che usano i controlli, e finiscono
  // quindi nello stesso annullo.
  iaOperazioni: (testo: string) =>
    invoke<OperazioniIa>("ia_operazioni", { testo }),
};
