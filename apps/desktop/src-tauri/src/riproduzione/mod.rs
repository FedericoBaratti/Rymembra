//! La riproduzione, dal lato della finestra.
//!
//! Tiene insieme tre cose che vivono in tre posti diversi: la **coda**
//! (`rymembra_domain::queue`, pura), il **motore** (`rymembra-play`, che non sa
//! cosa sia una libreria) e il **database** (`rymembra_app::playback`). Nessuno
//! dei tre conosce gli altri due, ed è voluto — ma qualcuno deve pur
//! presentarli, e quel qualcuno è questo modulo.
//!
//! # I due lucchetti, e l'ordine in cui si prendono
//!
//! Il lettore sta dietro un mutex **diverso** da quello della libreria. Deve:
//! una scansione tiene il lucchetto della libreria per venti secondi, e se il
//! tasto pausa passasse di lì l'utente premerebbe pausa e non succederebbe
//! niente per venti secondi.
//!
//! Due lucchetti però sono un abbraccio mortale in attesa di succedere, e
//! l'unica difesa che regge è una regola sola: **prima il lettore, poi la
//! libreria, mai il contrario**. Ogni funzione di questo modulo la rispetta,
//! testata e sottomoduli allo stesso modo: ognuno di loro rimanda qui, e
//! nessuno la riscrive per conto suo.
//!
//! Un terzo, `StatoLettore::preferenze`, sta **fuori** da quell'ordine: si
//! prende per primo e senza niente in mano, e chi lo tiene prende la libreria,
//! la lascia, e solo dopo il lettore. Serve a mettere in fila i comandi delle
//! preferenze da quando la loro scrittura non sta più dentro il lucchetto del
//! lettore — vedi `cambia_preferenza`.
//!
//! # E la terza attesa, che non è un lucchetto: la rete
//!
//! Da quando un brano può non essere un file, aprirlo può voler dire aprire una
//! **connessione**: una richiesta `Range` da 256 KiB verso un catalogo pubblico
//! che ci ospita gratis, con dentro — per Audius — anche la scelta del nodo che
//! risponde adesso. Sono secondi, non microsecondi, e la regola che ne discende
//! è la stessa già scritta per l'apertura di un file, estesa: **nessun lucchetto
//! si tiene mentre si aspetta la rete**.
//!
//! In pratica vuol dire che [`brano_di`] resta com'era — si legge la riga sotto
//! il lucchetto della libreria, lo si lascia, e l'apertura va su un filo suo
//! con i cinque secondi di [`APERTURA_BRANO`] addosso — e che il ramo nuovo sta
//! **dentro** quella scadenza, non accanto. Vale anche per il gapless: il
//! [`filo del preparatore`](fili::avvia_preparatore) apre il brano successivo
//! in anticipo, e da oggi «in anticipo» può voler dire una connessione aperta
//! mentre il corrente suona ancora. Quella connessione la apre lui, sul suo
//! filo, senza niente in mano — che è esattamente la ragione per cui quel filo
//! esiste.
//!
//! La riserva di connessioni è quella dei cataloghi ([`Cataloghi`]), non una
//! nuova per brano: sono i cataloghi a portarsi dietro l'agente, le scadenze e
//! il modo di trattare un `429` per ogni servizio, e aprirne una seconda idea
//! qui vorrebbe dire un saluto TLS per canzone contro un servizio che nessuno
//! paga.
//!
//! # Perché il conteggio d'ascolto si scrive qui e non nel motore
//!
//! Perché il motore non sa se quel che ha suonato conta. La regola —
//! `rymembra_domain::listen::counts_as_play` — è del dominio, e la scrittura è di
//! `rymembra-app`. Qui c'è solo il filo che le mette in fila.
//!
//! # Dov'è cosa
//!
//! Il modulo è una cartella, e i pezzi stanno dove si vanno a cercare:
//!
//! - [`flusso`]: il brano che non è un file — il riconoscimento, l'apertura
//!   della connessione, e l'involucro che fa di un flusso HTTP qualcosa che il
//!   motore sa suonare;
//! - [`fili`]: quel che gira per conto suo — il preparatore del gapless,
//!   l'orologio, lo spettro, il timer di spegnimento;
//! - [`uscite`]: i dispositivi audio, chi li sorveglia e chi li sceglie;
//! - [`suono`]: volume, equalizzatore, normalizzazione;
//! - [`riapertura`]: rimettere in piedi il motore, e la puntina nel solco;
//! - [`coda`]: la coda e i gesti che la cambiano;
//! - [`seguito`]: il lavoro lento messo in fila fuori dal lucchetto — i fili
//!   «rymembra-seguito» e «rymembra-scritture» — e la fotografia dello stato;
//! - [`insieme`]: il ponte con «Ascolta insieme», l'unico punto che tocca il
//!   `Lettore` per conto di `crate::insieme`;
//! - [`ospiti`]: il confine fra le righe della libreria e le voci di una
//!   sessione, cioè fra gli id positivi e quelli negativi.
//!
//! Qui restano i tipi che attraversano tutto, i due lucchetti che li tengono,
//! l'apertura di un brano e i comandi che fanno partire e fermare la musica.

mod coda;
mod fili;
mod flusso;
pub(crate) use flusso::sorgente_dal_cloud;
pub mod insieme;
mod ospiti;
mod riapertura;
mod seguito;
mod suono;
mod uscite;

// Perché i sottomoduli si riesportano qui.
//
// `generate_handler![riproduzione::coda_accoda]` va a cercare, a quel percorso,
// **due** cose: la funzione e il `macro_rules!` che `#[tauri::command]` le
// mette accanto. Riesportare qui quel che i sottomoduli espongono lascia
// intatte le righe di `main.rs` che registrano i comandi: si sono spostati di
// cartella, non di nome, e un elenco di percorsi lungo il doppio non
// racconterebbe niente in più.
pub use self::coda::*;
pub use self::fili::*;
pub use self::riapertura::*;
pub use self::suono::*;
pub use self::uscite::*;

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use crate::spegnimento::Emette as _;
use rymembra_app::library::{FormatoFile, TrackSummary};
use rymembra_app::playback::{self, Equalizzazione, Normalizzazione, Qualita, Volume};
use rymembra_catalogo::Cataloghi;
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::listen::{Listen, ListenTracker};
use rymembra_domain::queue::{Queue, RepeatMode, Step};
use rymembra_play::motore::Annuncio;
use rymembra_play::{Evento, Motore, Posizione};
use serde::Serialize;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria, con_libreria_entro};

use crate::insieme::{Gesto, Instradato};

use self::coda::{gesto_di_coda, ricorda_coda_di_prima, salva_coda};
use self::fili::prepara_prossimo;
use self::flusso::{apri_flusso, riferimento_di, sorgente_da_flusso};
use self::ospiti::Chi;
use self::seguito::{
    Seguito, abbozzo, aspetta_le_risposte, avvia_scritture, avvia_seguito, completa, da_scrivere,
    guasto_di, guasto_di_apertura, nome_ripetizione, pubblica,
};
use self::suono::nome_normalizzazione;

pub(crate) use self::ospiti::{riga_di_libreria, righe_di_libreria};
pub(crate) use self::seguito::in_ordine;

/// Ogni quanto la finestra riceve la posizione.
///
/// Quattro volte al secondo, non sessanta. Il cursore si muove liscio lo stesso
/// perché la finestra interpola fra un colpo e l'altro; sessanta eventi al
/// secondo attraverso l'IPC sarebbero sessanta serializzazioni JSON al secondo
/// per spostare un pixel, e si vedrebbero nel consumo della batteria molto prima
/// che nella fluidità.
const PASSO_TEMPO: Duration = Duration::from_millis(250);

/// Sotto quanti millisecondi d'avanzamento in un giro d'orologio la musica è
/// ferma. Un quinto del giro: chi suona davvero ne fa duecentocinquanta, e
/// l'imprecisione dell'orologio della callback sta nell'ordine della decina.
/// Vedi [`Tempo::in_attesa`].
const AVANZAMENTO_MINIMO_MS: u64 = 50;

/// Ogni quanto si rilegge l'elenco delle uscite audio.
///
/// Due secondi, e non i 250 millisecondi dell'orologio, perché ogni passata è
/// un'enumerazione WASAPI — una chiamata al sistema, non una lettura da una
/// casella. Due secondi è il ritardo massimo fra l'attaccare le cuffie e il
/// sentirle: si nota appena, e costa una trentina di enumerazioni al minuto
/// invece di quasi mille.
const PASSO_DISPOSITIVI: Duration = Duration::from_secs(2);

/// Il tetto dell'attesa quando riaprire non funziona.
///
/// L'attesa raddoppia a ogni tentativo fallito e si ferma qui. Senza, un
/// computer con il sistema audio rotto — o senza nessuna scheda — si prenderebbe
/// un tentativo di apertura ogni due secondi per tutto il tempo in cui resta
/// acceso, e ognuno di quelli parla col driver.
const ATTESA_MASSIMA_DISPOSITIVI: Duration = Duration::from_secs(30);

/// Ogni quanto la finestra riceve le bande dello spettro.
///
/// Trenta volte al secondo, che è sette volte la posizione — e non è una
/// contraddizione con la disciplina qui sopra. La posizione va a quattro perché
/// **la finestra la sa interpolare**: fra un colpo e l'altro il tempo passa da
/// solo, e il cursore avanza senza chiedere niente. Le bande no: la scena
/// smussa il passaggio da una fila alla successiva, ma quel che smussa sono due
/// misure vere, e a quattro colpi al secondo fra le due misure ci sarebbe un
/// quarto di secondo di musica che nessuno ha guardato.
///
/// Il costo è da otto a mille byte per colpo e nessuna interrogazione al
/// database, e si paga solo mentre la schermata è aperta.
const PASSO_SPETTRO: Duration = Duration::from_millis(33);

/// Ogni quanto il filo dello spettro si sveglia quando nessuno guarda.
///
/// Dorme a lungo invece di uscire: farlo nascere e morire vorrebbe dire
/// coordinare la sua fine con l'apertura successiva, e un quarto di secondo di
/// ritardo all'apertura della schermata non lo vede nessuno.
const PASSO_SPETTRO_FERMO: Duration = Duration::from_millis(250);

/// Ogni quanti giri d'orologio si conserva la posizione dentro il brano.
///
/// Venti giri da 250 ms, cioè cinque secondi.
const BATTITI_PER_SEGNO: u32 = 20;

/// Le bande dello spettro, come vanno sul filo verso la finestra.
///
/// # Perché sono byte
///
/// Perché possono essere 1024, e trenta volte al secondo: in JSON un `f32`
/// occupa una ventina di caratteri — `0.123456789` e la sua coda — che fa mezzo
/// megabyte al secondo di testo da scrivere di qua e riparsare di là, per
/// disegnare barre alte qualche centinaio di pixel. Un byte per banda ne fa
/// quattro, e la differenza fra `0.501` e `0.5019` non esiste su uno schermo: un
/// livello in `0..=1` diventa `0..=255`, e chi disegna divide.
///
/// # Cosa dice il byte, adesso che è cambiato
///
/// **La potenza misurata in questo mezzo secondo, e nient'altro.** Fino alla 2.2
/// arrivava già smorzato: il lettore gli applicava un attacco e un rilascio, e
/// la finestra disegnava quel che riceveva. Adesso il livello è grezzo, e a
/// smorzarlo è la scena, che ha due token per farlo — `canvas.viz.attack` e
/// `canvas.viz.release`, in millisecondi — e che conosce l'intervallo vero fra
/// un evento e l'altro perché lo misura.
///
/// Lo zero e l'uno del byte, invece, **non** si sono spostati e non si
/// sposteranno: sono `rymembra_play::spettro::FONDO_DB` e il fondo scala, cioè la
/// definizione di cosa significhi questo numero. Il token `canvas.viz.floor`
/// rialza il fondo dalla parte di chi disegna, rimappando lo stesso byte;
/// spostarlo qui vorrebbe dire un secondo formato di trasporto, e due formati
/// per lo stesso evento sono il modo di scoprire un giorno che si sta guardando
/// quello sbagliato. Per la stessa ragione il minimo di quel token è
/// esattamente `-70`: sotto non c'è informazione da rimappare.
///
/// # Perché le ottave non ci sono, e adesso nemmeno si calcolano
///
/// Il lettore sa ricavarle dalla stessa trasformata — sono quelle
/// dell'equalizzatore, e le sue prove le controllano — ma nella finestra non le
/// legge più nessuno: le disegnava la striscia a dieci barre sotto la copertina,
/// e quella striscia non c'è più. Dieci `f32` per trenta eventi al secondo che
/// nessuno guarda sono dieci `f32` di troppo.
///
/// Per un po' la cosa è costata comunque: erano fuori dal filo ma dentro il
/// motore, cioè una scansione di tutti i bin e dieci logaritmi trenta volte al
/// secondo per un numero che finiva in un campo e moriva lì. Adesso il calcolo
/// sta dietro un interruttore spento di serie (`Spettro::guarda_ottave`), e
/// questo comando non lo accende. **Non** è stato cancellato: la riduzione a
/// ottave è giusta e legata all'equalizzatore, e il giorno che una striscia a
/// dieci barre torna si riaccende invece di riscriverla.
///
/// # Due cose che non si sono fatte, e perché
///
/// **Non** si è reso adattivo [`PASSO_SPETTRO`]. Rallentare la cadenza mentre la
/// riproduzione è ferma sembra gratis e non lo è: la scena tiene mezzo minuto di
/// passato e lo fa scorrere **a file che arrivano**, non a tempo che passa.
/// Quintuplicare l'intervallo in pausa vorrebbe dire quattro minuti perché quel
/// mezzo minuto finisca di uscire, cioè la coda di un brano che resta appesa
/// sullo schermo molto dopo che il brano è finito. Chi deve smettere di pagare è
/// il **rubinetto** — le bande non si chiedono affatto quando nessuno le guarda —
/// non l'orologio.
///
/// **Non** si è cambiata la codifica. A 64 bande, che è quel che si spedisce
/// quasi sempre, questo evento è un array JSON di numeri interi corti: circa
/// trecento byte, trenta volte al secondo. Base64 ne toglierebbe forse un
/// quarto e in cambio obbligherebbe chi disegna a decodificare una stringa; un
/// canale binario di Tauri v2 li toglierebbe quasi tutti e in cambio cambierebbe
/// la **forma** dell'evento — non più un `emit` che chiunque può ascoltare, ma
/// un canale con un proprietario e un ciclo di vita da gestire. Sono due
/// riscritture del trasporto per un guadagno che a 64 bande non si misura; a
/// 1024 si misurerebbe, ma 1024 barre le chiede chi ha deciso di pagarle.
///
/// Riguardato nella 3.0.2, con due fatti in più, e confermato. Il primo è nei
/// sorgenti di Tauri 2.12: anche il canale finisce in un `eval` sul filo della
/// finestra — `ipc/channel.rs` manda direttamente i JSON fino a 8 KB e i grezzi
/// sotto i 1024 byte, e oltre fa fare alla pagina un `fetch` in più — quindi non
/// toglie di mezzo il filo che decide quando un evento arriva, che era la sola
/// cosa che contasse. Il secondo è il banco di misura del 22-10-2026, a 512
/// bande su questa codifica: 29,9 eventi al secondo, nessuna raffica, nessun
/// fotogramma perso. Lo scatto che si vedeva non era il peso dell'evento ma il
/// suo **ritmo**, e lo toglie l'orologio della scena (`orologioSpettro.ts`), che
/// non disegna più le file quando arrivano.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BandeIpc {
    /// Le fini, per la scena: quante ne ha chieste chi guarda.
    fini: Vec<u8>,
}

impl BandeIpc {
    /// Quantizza quel che il lettore ha appena misurato.
    fn da(bande: &rymembra_play::Bande) -> Self {
        Self {
            fini: bande
                .fini
                .iter()
                .map(|livello| {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "il livello sta in 0..=1 e la moltiplicazione lo porta in 0..=255"
                    )]
                    let byte = (livello.clamp(0.0, 1.0) * 255.0).round() as u8;
                    byte
                })
                .collect(),
        }
    }
}

/// Lo stato della riproduzione, come lo vede la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoRiproduzione {
    /// Il brano corrente, per intero: la barra deve disegnarne titolo e copertina.
    pub brano: Option<TrackSummary>,
    /// È fermo.
    pub in_pausa: bool,
    /// A che punto è.
    pub posizione_ms: u64,
    /// Quanto dura.
    pub durata_ms: u64,
    /// Lo shuffle è acceso.
    pub shuffle: bool,
    /// Come si ripete: `off`, `one`, `all`.
    pub ripeti: String,
    /// Il volume, da 0 a 1.
    pub volume: f32,
    /// È silenziato.
    pub muto: bool,
    /// La coda, nell'ordine in cui suonerà.
    ///
    /// Solo gli identificativi: mandare millequattrocento righe intere a ogni
    /// cambio di brano vorrebbe dire spedire qualche megabyte per aggiornare un
    /// titolo. Chi apre il pannello della coda chiede le righe con
    /// [`brani_per_id`].
    pub coda: Vec<i64>,
    /// Dove siamo dentro la coda.
    pub posizione_coda: Option<usize>,
    /// L'equalizzatore è acceso.
    pub eq_attivo: bool,
    /// La curva dell'equalizzatore, in decibel per banda.
    pub eq_guadagni: Vec<f32>,
    /// A che livello normalizza: `spento`, `basso`, `normale`, `alto`.
    ///
    /// Qui e non in [`crate::comandi::Avvio`] perché è una cosa del lettore, e
    /// perché il lettore la cambia anche da solo: al primo avvio non c'è nessuna
    /// riga in `settings` e il valore che vale è quello del motore. Uno stato
    /// che dice cos'è vero adesso non deve avere due sorgenti.
    ///
    /// Un nome e non il booleano di prima: gli stati sono quattro, e uno spento
    /// che non dice a quale livello tornerebbe è uno spento che chi riaccende
    /// deve scoprire per tentativi.
    pub replaygain: String,
    /// Fra quanto si spegne da solo, in millisecondi.
    ///
    /// `null` se nessun timer è acceso; `0` se il timer è «alla fine di questo
    /// brano», che non è una durata e che la finestra scrive a parole.
    ///
    /// Un tempo che **manca** e non l'istante in cui scade: l'istante
    /// obbligherebbe la finestra a conoscere l'orologio del nucleo, e i due
    /// orologi sono lo stesso solo finché nessuno cambia fuso mentre la musica
    /// suona.
    pub spegnimento_ms: Option<i64>,
    /// A coda finita si continua da soli.
    pub autoplay: bool,
    /// Quanto si sovrappongono due brani, in secondi. `0` è spenta.
    ///
    /// In secondi e non in millisecondi perché è così che si sceglie: il
    /// cursore ha dodici tacche, e mandare millisecondi vorrebbe dire che la
    /// finestra divide per mille per disegnare e moltiplica per mille per
    /// chiedere, cioè due conversioni che possono divergere per niente.
    pub dissolvenza_s: u64,
    /// Di quanti millisecondi la catena d'uscita ritarda il suono, dichiarati.
    ///
    /// La correzione a mano, non quella misurata: il pezzo di catena che `cpal`
    /// non vede — mixer di sistema, driver, DAC, e su un'uscita senza fili la
    /// radio. Positiva racconta la posizione più indietro, negativa più avanti.
    /// Quel che il motore misura da sé non passa di qui: è già dentro
    /// `posizione_ms`, e mandarlo anche sarebbe un secondo numero da spiegare
    /// per una cosa su cui nessuno può agire.
    pub latenza_ms: i64,
    /// Quanto prima del suo tempo una riga di testo si accende.
    ///
    /// # Perché una costante viaggia dentro uno stato
    ///
    /// Perché l'alternativa era peggio: questo numero è
    /// `rymembra_domain::testo::ANTICIPO_MS`, e fino a ieri stava scritto **due
    /// volte** — là e a mano dentro `parti/Testo.tsx`, che cerca la riga accesa
    /// venti volte al secondo e non può attraversare l'IPC per ogni riga. Due
    /// copie a mano dello stesso numero sono durate finché nessuno ha toccato una
    /// delle due.
    ///
    /// Costa due byte di JSON a ogni cambio di stato — non a ogni colpo
    /// d'orologio, che viaggia su [`Tempo`] — e in cambio la finestra legge il
    /// numero invece di ripeterlo. Sta qui e non nello stato d'avvio perché è di
    /// questo modulo il mestiere di dire come la posizione riportata si lega a
    /// quel che si sente, e l'anticipo è l'altra metà di quel legame.
    pub anticipo_ms: i64,
    /// Il dispositivo audio non c'è più, o non si è mai aperto.
    ///
    /// Un campo dello stato e non solo un evento: chi apre la finestra dopo che
    /// il dispositivo è sparito deve trovarlo detto, non aspettare che sparisca
    /// una seconda volta.
    pub audio: Option<GuastoAudio>,
    /// Da quale uscita esce il suono, adesso.
    ///
    /// Il nome del dispositivo **davvero** aperto, che non è per forza quello
    /// scelto: una preferenza che punta a una scheda staccata ripiega sul
    /// predefinito. `None` quando il motore non si è aperto.
    ///
    /// Un campo dello stato per la stessa ragione di `audio` qui sopra: da
    /// quando l'uscita cambia da sé, «dove sta suonando» è un fatto che una
    /// finestra aperta dopo il cambio deve poter ritrovare, non un evento che
    /// se l'è perso.
    pub uscita: Option<String>,
    /// Perché il brano dopo è quello, quando l'ha scelto l'autoplay.
    ///
    /// Il **codice** e non la frase: la frase va tradotta, e una stringa
    /// italiana che attraversa l'IPC resta italiana anche per chi ha
    /// l'interfaccia in inglese. Vedi [`rymembra_app::autoplay::Motivo::codice`].
    ///
    /// `None` quando il brano dopo l'hai messo tu, e allora non c'è niente da
    /// spiegare.
    pub motivo_prossimo: Option<String>,
    /// Com'è fatto il file che sta suonando: formato, frequenza, canali, bitrate.
    ///
    /// Un campo dedicato e **non** quattro campi in più su [`TrackSummary`]: là
    /// starebbero su ogni riga di ogni elenco e di tutta la coda, cioè spediti
    /// qualche migliaio di volte per disegnarne uno solo. La parsimonia di quel
    /// tipo è un impegno scritto nella sua carta, e qui sopra — nel commento di
    /// `coda` — c'è la stessa decisione presa per gli identificativi.
    ///
    /// # `None` vuol dire «non si disegna», e nient'altro
    ///
    /// Tre cose diverse arrivano qui come `None`: non c'è nessun brano, il brano
    /// non è più in libreria, oppure **la preferenza è spenta**. La finestra non
    /// le distingue e non deve: mostra la riga se e solo se il dato c'è.
    ///
    /// È una decisione, e la ragione è che l'alternativa — mandare sempre il
    /// dato e lasciare che la finestra legga `player.fileFormat.visible` —
    /// obbligherebbe ogni schermata a tenersi una copia di quella preferenza e a
    /// ricordarsi di rinfrescarla. Chi spegne l'interruttore nelle Impostazioni
    /// non sta guardando «In riproduzione»: al ritorno troverebbe la riga
    /// ancora lì, disegnata da una copia stantia. Il filtro sta in un posto
    /// solo, e [`manda_stato`] lo fa arrivare a tutte e due le viste nello
    /// stesso istante.
    ///
    /// Il dato è quello di **quel che si sente**: le colonne del database
    /// corrette dal decodificatore appena il brano comincia a suonare (vedi
    /// [`FormatoFile::completato_da`]), più la frequenza a cui esce. Il
    /// ricampionamento la riga non lo scrive — descrive l'edizione, e cambiare
    /// cuffia non la cambia — ma lo dice a chi ci si ferma sopra.
    pub formato: Option<FormatoFile>,
    /// «Ascolta insieme», quando questo PC è in una sessione: chi ospita, di
    /// chi è la voce che suona, se la sincronia è agganciata. `None` fuori
    /// sessione. Vedi [`crate::insieme::InsiemeNelLettore`].
    pub insieme: Option<crate::insieme::InsiemeNelLettore>,
}

/// Il motore audio non c'è: perché, e da quando.
///
/// # Perché questo tipo esiste
///
/// Perché fino a ieri un dispositivo perso era **indistinguibile da un brano
/// che non parte**. `Motore::dispositivo_perso` c'era da sempre, `uscita.rs`
/// alzava quel bit quando `cpal` segnalava un guasto del flusso, e in tutta
/// l'applicazione non c'era una riga che lo leggesse: staccare le cuffie voleva
/// dire premere play e non sentire niente, per sempre, senza nessuna schermata
/// che dicesse perché.
///
/// Trovato provando: la riproduzione si è fermata a otto secondi e la finestra
/// continuava a dire che suonava.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuastoAudio {
    /// Il codice del catalogo: `playback.deviceLost` o `playback.engineUnavailable`.
    pub codice: String,
    /// Cosa è successo, in una frase.
    ///
    /// Italiana, perché nasce dentro il motore. Resta il **ripiego** di
    /// `causa_codice`, non quel che la finestra disegna quando può fare di
    /// meglio.
    pub causa: String,
    /// Perché il dispositivo è sparito, in un codice che si può tradurre.
    ///
    /// Il **codice** e non la frase, per la ragione già scritta venti righe
    /// sopra in [`StatoRiproduzione::motivo_prossimo`]: una stringa italiana
    /// che attraversa l'IPC resta italiana anche per chi ha l'interfaccia in
    /// inglese. Qui si leggeva «There is no audio. dispositivo non più
    /// disponibile.», cioè metà frase tradotta e metà no, nell'unico riquadro
    /// che si apre quando qualcosa è già andato storto.
    ///
    /// `None` per il motore che non si è **mai** aperto: là la causa è un
    /// [`AppError`], che il suo codice ce l'ha già in `codice` — e una seconda
    /// copia dello stesso codice sarebbe due sorgenti per la stessa frase.
    pub causa_codice: Option<String>,
    /// Riaprire ha senso provarlo.
    ///
    /// Sempre vero, oggi, e resta un campo perché la finestra non deve saperlo:
    /// il giorno in cui una causa non fosse più ritentabile, il tasto sparisce
    /// senza toccare la finestra.
    pub riapribile: bool,
    /// L'uscita fissata che si sta aspettando, se è lei che se n'è andata.
    ///
    /// Una scheda scelta nelle impostazioni che sparisce — le cuffie Bluetooth
    /// che si staccano — non fa più passare il suono agli altoparlanti: si
    /// aspetta che torni, e quando torna si riprende da dov'era (vedi
    /// `uscite::da_riaprire`). Senza questo nome la finestra direbbe soltanto
    /// «non c'è audio», cioè un guasto da riparare invece di un'attesa che si
    /// risolve da sé; e il tasto «Riapri», che in questo caso suona su un'altra
    /// uscita, non direbbe perché.
    pub attesa: Option<String>,
}

/// Solo il tempo che passa.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tempo {
    /// A che punto è.
    pub posizione_ms: u64,
    /// Quanto dura.
    pub durata_ms: u64,
    /// È fermo.
    pub in_pausa: bool,
    /// Non è in pausa, ma da mezzo secondo la callback serve silenzio e la
    /// posizione non avanza: si aspettano dati che non arrivano — la rete di
    /// un flusso, una condivisione lenta.
    ///
    /// La finestra smette di far correre il cursore da sola, che altrimenti
    /// andrebbe avanti su una musica ferma e tornerebbe indietro di colpo al
    /// colpo dopo, e dice che sta aspettando. Lo calcola il filo
    /// dell'orologio: vedi `avvia_orologio`.
    pub in_attesa: bool,
}

/// Quel che l'applicazione tiene aperto per suonare.
pub struct Lettore {
    motore: Motore,
    coda: Queue,
    /// L'ascolto in corso, se ce n'è uno.
    ascolto: Option<ListenTracker>,
    volume: Volume,
    eq: Equalizzazione,
    normalizzazione: Normalizzazione,
    /// A coda finita si continua da soli.
    ///
    /// Una copia di quel che sta in `settings`, tenuta accanto alla coda
    /// perché la legge `prepara_prossimo` — cioè il percorso che decide il
    /// brano successivo, che gira a ogni cambio di traccia e non deve
    /// interrogare il database per sapere un bit.
    autoplay: bool,
    /// Quanto dura la sovrapposizione fra due brani, in secondi.
    ///
    /// Una copia di quel che il motore ha già, come per `eq` e
    /// `normalizzazione`: il motore la usa e non la racconta — `Contesto` vive
    /// nel filo del decodificatore e non ha un modo di rispondere a una
    /// domanda — e senza questa copia il cursore delle impostazioni tornerebbe
    /// a zero a ogni ridisegno della finestra.
    dissolvenza_s: u64,
    /// La latenza d'uscita dichiarata a mano che vale **per l'uscita aperta
    /// adesso**, in millisecondi.
    ///
    /// Il valore dell'uscita se ne ha uno suo, la globale altrimenti — vedi
    /// `playback::latenza_effettiva` — ed è quel che il motore sta usando: per
    /// questo cambia da sé quando una riapertura passa dalle casse alle cuffie.
    /// È una copia, accanto a `dissolvenza_s` e per la stessa ragione: il
    /// motore la usa e non la racconta, e senza questa copia il cursore delle
    /// impostazioni tornerebbe a zero a ogni ridisegno.
    latenza_ms: i64,
    /// Perché il brano che l'autoplay ha accodato è quello.
    ///
    /// Solo l'ultimo, e solo quello scelto **da solo**: quel che si è messo in
    /// coda a mano non ha un perché da spiegare — l'hai messo tu. `None`
    /// significa quindi «questo lo hai scelto», che è già l'informazione giusta
    /// e non ha bisogno di una frase.
    ///
    /// Un campo e non una colonna: è vero finché quel brano è in coda, si
    /// ricalcola gratis alla scelta successiva, e conservarlo attraverso i
    /// riavvii vorrebbe dire spiegare una scelta che il programma non ricorda
    /// più di aver fatto.
    motivo_prossimo: Option<(i64, &'static str)>,
    /// La coda che l'ultimo gesto ha sostituito, per l'«Annulla».
    ///
    /// Una sola, la più recente: l'avviso che la offre dura qualche secondo, e
    /// una pila di code passate sarebbe una cronologia che nessuno sfoglia.
    /// Dentro il lettore perché si scrive nello stesso istante in cui la coda
    /// cambia, con lo stesso lucchetto in mano. Vedi [`coda::coda_ripristina`].
    coda_di_prima: Option<coda::CodaDiPrima>,
    /// La coda è stata toccata a mano dopo l'ultima volta che è stata sostituita.
    ///
    /// Accodare, mettere dopo, togliere, riordinare: una coda così vale anche
    /// se è corta, e sostituirla offre sempre un «Annulla». Una coda nata da un
    /// doppio clic su un album invece si sostituisce in silenzio. Non si
    /// conserva fra un avvio e l'altro: all'avvio non c'è nessun avviso da
    /// offrire, e il primo gesto ricomincia il conto.
    coda_curata: bool,
    /// I brani che il giro guidato ha messo in una coda vuota, finché nessuno
    /// li ha fatti partire.
    ///
    /// Una coda così non l'ha scelta nessuno: il giro ce l'ha messa perché i
    /// passi sul lettore avessero qualcosa da mostrare. Sostituirla non perde
    /// niente, e il primo «Suona» vero diceva «Coda sostituita: 50 brani —
    /// Annulla» a chi non sapeva di averne una. [`coda::ricorda_coda_di_prima`]
    /// la riconosce confrontando i brani — qualunque gesto che la cambi la rende
    /// una coda come le altre, senza doverlo scrivere in ogni gesto — e
    /// ogni avvio la dimentica (`Lettore::dopo_un_avvio`), perché una coda che
    /// ha suonato è stata ascoltata.
    coda_del_giro: Option<Vec<i64>>,
    /// L'ultima pausa o ripresa chiesta da qualcuno: un numero che cresce a
    /// ogni gesto, e se il gesto era una pausa. Serve a [`consegna`]:
    /// un brano che si apre da un disco di rete può metterci secondi, e una
    /// pausa premuta nel frattempo fermava il brano di prima — poi l'apertura
    /// finiva e faceva partire quello nuovo, come se nessuno avesse detto
    /// niente. In sessione succedeva a ogni membro che metteva in pausa
    /// mentre l'host apriva la voce dopo.
    pausa_chiesta: (u64, bool),
    /// Il brano che il motore ha in canna come successivo, se ne ha uno.
    ///
    /// Si scrive soltanto passando dagli avvii (`Lettore::dopo_un_avvio`), da
    /// [`Lettore::prepara`] e da [`Lettore::ferma`], che sono le tre strade per
    /// cui il motore prende o butta un successivo. Serve a due domande che la coda da sola non sa
    /// rispondere.
    ///
    /// **L'inizio annunciato è quello del preparato?** Con lo stesso brano due
    /// volte di fila — «ripeti uno», o una coda `[A, A, B]` — l'identificativo
    /// non distingue il brano che riparte da quello che già suonava, e la coda
    /// non avanzava: il secondo A tornava a preparare sé stesso e B non partiva
    /// mai. Vedi il ramo `Iniziato` di [`su_evento`].
    ///
    /// **Il successivo in canna è ancora quello giusto?** Togliere o spostare il
    /// brano dopo lasciava nel motore quello vecchio fino a che il preparatore
    /// non ne consegnava un altro — e se l'altro non si apriva, non arrivava
    /// mai: alla fine del corrente suonava il brano tolto. Vedi
    /// [`fili::prepara_prossimo`].
    in_canna: Option<i64>,
    /// Il brano dopo che il preparatore non è riuscito ad aprire.
    ///
    /// Quando il corrente finisce il motore si ferma, perché in canna non c'è
    /// niente; senza questo la coda restava sul brano finito e «riproduci»
    /// ricominciava quello. Il ramo `Fermato` di [`su_evento`] avanza la coda
    /// sul brano che non si è aperto: la finestra lo mostra, fermo, accanto
    /// all'errore che già lo diceva.
    non_aperto: Option<i64>,
    /// Il brano che il motore ha appena ricevuto da suonare e non ha ancora
    /// annunciato.
    ///
    /// L'annuncio arriva quando il primo campione raggiunge l'orecchio, e nel
    /// frattempo il preparatore ha già messo in canna il successivo: con una coda
    /// `[A, A, B]` il successivo è di nuovo A, e l'inizio del primo A sembrava
    /// quello del preparato — la coda avanzava mentre suonava ancora il primo.
    /// Il primo annuncio dopo un avvio è del brano avviato.
    ///
    /// Il brano e non un sì o un no, perché nel frattempo la coda può essersi
    /// già mossa: vedi [`dopo_l_annuncio`].
    annuncio_atteso: Option<i64>,
    /// Il punto chiesto con [`vai_a`] mentre il motore non aveva niente in mano
    /// — o aveva ancora il brano di prima, perché quello della coda si stava
    /// aprendo — e di quale brano.
    ///
    /// Il caso è la coda rimessa all'avvio: si trascina il cursore, si preme
    /// «play», e il brano partiva dall'inizio — il salto era andato a un motore
    /// vuoto, che non aveva dove farlo. Lo raccoglie [`riprendi`], o
    /// [`avvia_corrente`] se il brano è quello che sta aprendo; qualunque altra
    /// partenza lo butta, perché parla di un brano che non era ancora aperto.
    puntina_a_vuoto: Option<(i64, u64)>,
    /// La sessione d'«Ascolta insieme» in cui il lettore è, se ce n'è una: il
    /// ruolo, la coda di prima da rimettere all'uscita, l'avvio programmato e
    /// la calibrazione. Vedi [`insieme`].
    ///
    /// # Perché qui dentro e non accanto, come `ripresa`
    ///
    /// Perché le regole della sessione cambiano **insieme** alla coda: entrare
    /// vuol dire coda di maniglie, dissolvenza a zero e niente autoplay nello
    /// stesso istante, e chi li leggesse da due lucchetti diversi potrebbe
    /// vedere la coda nuova con le regole vecchie — un autoplay che accoda un
    /// brano della libreria in una coda di sessione. Sotto un lucchetto solo
    /// non si può.
    ///
    /// Non sopravvive a niente, ed è voluto: una sessione è una connessione
    /// aperta, non una preferenza. Il motore sostituito da una riapertura la
    /// ritrova qui, perché il lettore resta lo stesso.
    sessione: Option<insieme::Sessione>,
    /// Il prossimo `Fermato` del motore è quello dello stop con cui si è
    /// usciti da una sessione, e non deve toccare niente dell'utente.
    ///
    /// # Il difetto che chiude
    ///
    /// Uscendo, il motore si ferma e **nello stesso giro** di lucchetto torna
    /// la coda dell'utente. Il `Fermato` di quello stop arriva dopo, sul filo
    /// degli annunci, e non risulta superato — è la risposta allo stop, non un
    /// annuncio di prima — quindi [`fermato`] lo leggeva come la fine della
    /// coda dell'utente: il segno di «riprendi dov'eri» del suo brano tornava a
    /// zero, e il brano lasciato a cinquanta secondi ripartiva dall'inizio.
    /// Visto nella prova a due istanze; se si esce senza aver suonato niente il
    /// motore è già fermo, non annuncia niente, e il difetto non si vedeva.
    ///
    /// Si alza uscendo e si abbassa al primo brano dato al motore (vedi
    /// [`Lettore::dopo_un_avvio`]): ogni `Fermato` vero dopo l'uscita ha bisogno
    /// di musica, e la musica ha bisogno di un brano nuovo. Un `Fermato` della
    /// sessione arrivato prima dell'uscita — la coda di sessione finita da sé
    /// un attimo prima di «Termina» — lo consuma lo stesso, ed è giusto: anche
    /// quello parlava della sessione.
    fermato_della_sessione: bool,
}

impl Lettore {
    /// Fa partire un brano dal millisecondo `da_ms`, e il successivo in canna
    /// se ne va con quello che c'era: vedi `Motore::suona_da`.
    fn suona_da(&mut self, brano: rymembra_play::BranoAperto, da_ms: u64) {
        let avviato = brano.track_id();
        self.motore.suona_da(brano, da_ms);
        self.dopo_un_avvio(avviato);
    }

    /// Come [`Lettore::suona_da`], ma il brano parte **fermo** a `da_ms`: in
    /// pausa, con l'anello già pieno dal punto giusto. Vedi
    /// `Motore::apri_fermo`, e [`consegna`] per chi lo usa.
    fn apri_fermo(&mut self, brano: rymembra_play::BranoAperto, da_ms: u64) {
        let avviato = brano.track_id();
        self.motore.apri_fermo(brano, da_ms);
        self.dopo_un_avvio(avviato);
    }

    /// Quel che un brano appena dato al motore rende vecchio, comunque parta.
    fn dopo_un_avvio(&mut self, avviato: i64) {
        self.puntina_a_vuoto = None;
        self.coda_del_giro = None;
        self.in_canna = None;
        self.non_aperto = None;
        self.annuncio_atteso = Some(avviato);
        self.fermato_della_sessione = false;
        insieme::dopo_un_avvio(self);
    }

    /// Mette in canna il brano dopo, o toglie quello che c'era.
    fn prepara(&mut self, brano: Option<rymembra_play::BranoAperto>) {
        self.in_canna = brano.as_ref().map(rymembra_play::BranoAperto::track_id);
        self.motore.prepara(brano);
    }

    /// Ferma tutto: col corrente se ne va anche il successivo.
    fn ferma(&mut self) {
        self.motore.ferma();
        self.in_canna = None;
        self.non_aperto = None;
        self.annuncio_atteso = None;
        insieme::dopo_un_avvio(self);
    }
}

/// Che cosa fa la coda quando il motore annuncia l'inizio di un brano, per chi
/// non segue una sessione: vedi [`iniziato`].
#[derive(Debug, PartialEq, Eq)]
enum DopoLAnnuncio {
    /// La coda indica già il brano giusto, o lo sceglierà chi l'ha mossa.
    Resta,
    /// Il motore ha attaccato il brano in canna: la coda va avanti di uno.
    AvanzaDallaCanna,
    /// Il motore ha attaccato un brano che la coda non indica, senza passare
    /// dalla canna — un entrante rimesso in canna da un salto a metà
    /// dissolvenza: la coda va avanti di uno.
    Avanza,
}

/// La decisione di [`iniziato`], senza il lettore intorno.
///
/// Il brano avviato apposta (`atteso`) non sposta mai la coda. Se la coda nel
/// frattempo indica un altro brano, l'ha mossa un gesto mentre questo aspettava
/// il suo primo campione — un avvio programmato in sessione, un'uscita
/// Bluetooth — e chi l'ha mossa sta aprendo il suo: avanzarla come per un
/// passaggio gapless la portava una voce **oltre** quella scelta, che poi
/// nessuno apriva. Lo si è visto con due scelte dalla coda a un secondo l'una
/// dall'altra: l'host restava sul brano di prima, col titolo di quello dopo la
/// voce scelta, e chi segue con lui.
///
/// Se l'annuncio è di un altro brano, quello avviato non è mai uscito — un file
/// che il motore ha saltato — e l'annuncio si legge come gli altri.
fn dopo_l_annuncio(
    brano: i64,
    atteso: Option<i64>,
    corrente: Option<i64>,
    in_canna: Option<i64>,
) -> DopoLAnnuncio {
    if atteso == Some(brano) {
        DopoLAnnuncio::Resta
    } else if in_canna == Some(brano) {
        DopoLAnnuncio::AvanzaDallaCanna
    } else if corrente != Some(brano) {
        DopoLAnnuncio::Avanza
    } else {
        DopoLAnnuncio::Resta
    }
}

/// Il lettore, o il motivo per cui non c'è.
///
/// Come per la libreria in [`crate::stato`], il guasto si **conserva** invece di
/// far fallire l'avvio: senza scheda audio l'applicazione resta un catalogo
/// consultabile, e un catalogo consultabile è meglio di una finestra che non si
/// apre.
pub struct StatoLettore {
    pub lettore: Mutex<Result<Lettore, AppError>>,
    /// La schermata dello spettro è aperta.
    ///
    /// Qui e non solo dentro il motore perché lo legge anche il filo che manda
    /// l'evento, e leggerlo di là vorrebbe dire prendere il lucchetto del
    /// lettore trenta volte al secondo per scoprire che non c'è niente da fare.
    pub spettro: std::sync::atomic::AtomicBool,
    /// Quante barre fini disegna la scena dello spettro.
    ///
    /// Qui e non solo dentro il motore per la stessa ragione dell'interruttore
    /// qui sopra, più una: riaprire il dispositivo audio costruisce un lettore
    /// nuovo, e senza una copia di questa scelta fuori dal motore la finestra
    /// tornerebbe a sessantaquattro barre da sola.
    pub spettro_bande: std::sync::atomic::AtomicU16,
    /// Quando si spegne da solo.
    ///
    /// Tre significati in un intero: `0` è spento, un numero positivo è
    /// l'istante assoluto in cui mettere in pausa, e [`FINE_DEL_BRANO`] dice di
    /// non preparare il brano successivo e fermarsi dove la musica finisce da
    /// sé.
    ///
    /// # Perché qui e non dentro `Lettore`
    ///
    /// Per la stessa ragione di `spettro` qui sopra: lo legge il filo
    /// dell'orologio quattro volte al secondo, e metterlo dietro il mutex del
    /// lettore vorrebbe dire prenderlo quattro volte al secondo per scoprire
    /// quasi sempre che non c'è niente da fare — cioè contendere il lucchetto
    /// col tasto pausa per leggere un intero.
    ///
    /// # Perché non sopravvive alla chiusura
    ///
    /// Un timer è una decisione di stasera, non una preferenza. Ritrovarlo
    /// acceso domani mattina vorrebbe dire una musica che si spegne da sola
    /// senza che nessuno ricordi di averlo chiesto, ed è per lo stesso motivo
    /// che non entra nel profilo esportabile.
    pub spegnimento: std::sync::atomic::AtomicI64,
    /// Dove riprendere quando una cartella di rete ha smesso di rispondere.
    ///
    /// # Perché fuori dal lucchetto del lettore
    ///
    /// Perché [`riapri_audio`] costruisce un [`Lettore`] nuovo di zecca, e un
    /// punto di ripresa che vivesse là dentro sparirebbe proprio nel caso in
    /// cui serve: quando si rimette in piedi l'audio dopo un guasto. Qui
    /// sopravvive a tutti e due, al motore e al dispositivo.
    ///
    /// # Perché non sopravvive alla chiusura
    ///
    /// Perché è la memoria di un cavo staccato adesso, non una preferenza.
    /// Ritrovare domani mattina un tasto «Riprova» che punta a un NAS di ieri
    /// sera vorrebbe dire offrire una cosa che non funziona.
    pub ripresa: Mutex<Option<Ripresa>>,
    /// La scheda del brano corrente com'era all'ultima lettura riuscita, e di
    /// quale brano.
    ///
    /// La legge [`completa`] quando la libreria non risponde. Sul filo
    /// della finestra `con_libreria` rinuncia dopo quattro secondi, e allo stato
    /// arrivava `brano: null` con la musica che suonava: la barra del lettore
    /// spariva e lo schermo intero si chiudeva, per un lucchetto tenuto da una
    /// share lenta. Il titolo di un brano non cambia perché il database è
    /// occupato, e il ricordo dice il vero finché parla dello stesso brano.
    scheda_corrente: Mutex<Option<(i64, TrackSummary, Option<FormatoFile>)>>,
    /// Il canale con cui si sveglia il filo che apre il brano successivo.
    ///
    /// # Perché una spinta e non una chiamata
    ///
    /// Perché chi la manda ha il lucchetto del lettore in mano: un gesto sulla
    /// coda, o l'osservatore degli eventi — che gira sul filo «rymembra-eventi»
    /// del motore e a ogni cambio di traccia chiede di preparare il successivo.
    /// Aprire un file lì vorrebbe dire tenere chiuso il lettore per tutta
    /// l'apertura, e con lui la pausa, l'orologio e gli annunci dopo. Il filo
    /// del preparatore apre a lucchetto lasciato. Vedi [`prepara_prossimo`].
    ///
    /// Fuori dal lucchetto del lettore per la stessa ragione degli atomici qui
    /// sopra: chi manda la spinta il lucchetto ce l'ha già in mano.
    pub prepara: std::sync::mpsc::Sender<()>,
    /// Il volume in memoria non è ancora quello sul disco.
    ///
    /// # Perché il salvataggio non sta più nel comando
    ///
    /// Perché `volume` ed `equalizzatore` arrivano da un `<input type="range">`,
    /// cioè **a ogni pixel** in cui il cursore si muove: una dozzina al secondo
    /// finché resta sotto il dito. Scrivere lì dentro voleva dire, dodici volte
    /// al secondo, prendere il lucchetto della libreria e aprire una
    /// transazione su SQLite — sul filo principale, che è quello che disegna la
    /// finestra, e tenendo già in mano il lucchetto del lettore. Dietro quei due
    /// lucchetti si accodano l'orologio, lo spettro e ogni altro comando: una
    /// manciata di secondi di cursore bastava a far sembrare la finestra morta.
    ///
    /// Quel che chi ascolta deve sentire subito è il **suono**, e quello cambia
    /// nel comando come prima. Quel che può aspettare un quarto di secondo è la
    /// riga nel database, che serve solo alla prossima apertura.
    ///
    /// Una bandiera e non una copia del valore: il valore vero sta già in
    /// `Lettore`, e tenerne due vorrebbe dire poterli far divergere. Qui si dice
    /// soltanto «va riletto di là e scritto», e chi scrive rilegge l'ultimo —
    /// che è il solo che interessi. Dodici cambi in un secondo diventano una
    /// scrittura sola.
    ///
    /// Fuori dal lucchetto del lettore come gli altri atomici qui sopra, e per
    /// il motivo di sempre: chi la alza il lucchetto ce l'ha già in mano.
    pub volume_da_salvare: std::sync::atomic::AtomicBool,
    /// Il brano che [`avvia_corrente`] sta aprendo adesso, o [`NESSUN_BRANO`].
    ///
    /// # Perché
    ///
    /// Perché un'apertura su una cartella di rete che si sta svegliando dura
    /// fino a quindici secondi, e chi non sente niente preme play di nuovo. Ogni
    /// pressione era un'apertura in più dello stesso file. Un secondo play
    /// sullo stesso brano, mentre il primo lo sta ancora aprendo, non apre
    /// niente: il primo risponderà per tutti e due.
    ///
    /// Le aperture di brani **diversi** invece si fanno — il gesto più recente
    /// vince — e fino alla 3.2 ognuna teneva un worker del runtime, uno per
    /// processore e prestati a tutti i comandi asincroni: quattro clic su brani
    /// diversi fermavano la libreria e la ricerca. Adesso i comandi del lettore
    /// girano sul pool bloccante, che cresce: vedi [`col_lettore_in_disparte`].
    pub aprendo: std::sync::atomic::AtomicI64,
    /// La curva in memoria non è ancora quella sul disco.
    ///
    /// Vedi [`StatoLettore::volume_da_salvare`]: stessa ragione, stesso cursore.
    pub eq_da_salvare: std::sync::atomic::AtomicBool,
    /// I cataloghi da cui arrivano i byte di un brano che non è un file.
    ///
    /// # Perché una terza copia, e non quella della coda
    ///
    /// Perché nell'albero ce ne sono già due, e la ragione della seconda vale
    /// identica per questa: `StatoImport` li tiene separati da quelli di
    /// `StatoProcura` perché «la coda tiene tre fili occupati per minuti, e una
    /// lettura fatta da chi sta guardando la finestra non deve mettersi dietro
    /// di loro». Qui la frase diventa più netta ancora: dietro non c'è qualcuno
    /// che guarda una schermata, c'è **la musica che sta suonando**. Un brano
    /// che aspetta il suo turno in fila dietro a un prelievo da centoventi
    /// megabyte è un silenzio fra due canzoni.
    ///
    /// # Perché fuori dal lucchetto del lettore
    ///
    /// Per la stessa ragione degli atomici qui sopra, più una: chi lo usa lo usa
    /// **mentre non ha il lucchetto**, ed è tutto il punto — vedi il preambolo
    /// del modulo. Tenerlo dentro `Lettore` vorrebbe dire prendere il lucchetto
    /// per poter aprire una connessione, cioè esattamente ciò che non si fa.
    ///
    /// Il clone costa niente: l'agente di `ureq` sta dentro un `Arc` e la
    /// riserva di connessioni si condivide fra le copie. È così che il filo del
    /// preparatore se lo porta dentro la scadenza senza portarci uno `State`.
    cataloghi: Cataloghi,
    /// Su quale uscita si vuole sentire Rymembra. `None`: quella di sistema.
    ///
    /// # Perché una copia qui e non una lettura dal database
    ///
    /// Perché a leggerla è il filo che sorveglia i dispositivi, ogni due
    /// secondi, per tutta la durata della sessione. Andarla a prendere di là
    /// vorrebbe dire una transazione su SQLite ogni due secondi — dietro il
    /// lucchetto della libreria, cioè in fila con la scansione e con chi sta
    /// scorrendo l'elenco — per rileggere quasi sempre la stessa stringa.
    ///
    /// L'originale resta `player.output` in `settings`: questa copia si scrive
    /// solo dopo che la scrittura di là è riuscita, così non può restare in
    /// piedi una preferenza che il disco non ha accettato.
    pub uscita_voluta: Mutex<Option<String>>,
    /// C'è già una riapertura in corso.
    ///
    /// Aprire un dispositivo audio può prendersi dei secondi, e il sorvegliante
    /// bussa ogni due: senza questa bandiera un'apertura lenta si
    /// accavallerebbe con la successiva, e due motori aperti insieme sulla
    /// stessa scheda sono due flussi che suonano la stessa musica sfasata.
    pub riapertura_in_corso: std::sync::atomic::AtomicBool,
    /// Il canale con cui si sveglia il filo che guarda i dispositivi.
    ///
    /// Serve a non far aspettare due secondi chi apre le impostazioni: la
    /// schermata chiede l'elenco, il comando pungola il filo, e la lista è
    /// quella di adesso invece di quella dell'ultima passata. Stessa forma del
    /// risveglio di `crate::aggiornamenti`.
    pub sveglia_dispositivi: std::sync::mpsc::Sender<()>,
    /// Il canale verso il filo «rymembra-seguito»: vedi [`in_ordine`].
    seguito: std::sync::mpsc::Sender<Seguito>,
    /// Il canale verso il filo «rymembra-scritture»: vedi [`da_scrivere`].
    scritture: std::sync::mpsc::Sender<Seguito>,
    /// Quante scritture sono state messe in fila con [`da_scrivere`], da
    /// sempre. Lo legge la fotografia dello stato: vedi [`seguito::Abbozzo::scritture`].
    scritture_chieste: std::sync::atomic::AtomicU64,
    /// Quante di quelle il filo delle scritture ha finito, riuscite o no.
    scritture_fatte: std::sync::atomic::AtomicU64,
    /// La coda in memoria non è ancora quella sul disco.
    ///
    /// La stessa bandiera di [`StatoLettore::volume_da_salvare`], per la stessa
    /// ragione e con un motivo in più. La coda cambia a ogni gesto e a ogni
    /// cambio di brano, e fino a ieri la si scriveva lì, dentro il lucchetto del
    /// lettore: un database occupato da una scansione teneva quel lucchetto per
    /// tutto il tempo della scrittura, e dietro di lui si fermavano la pausa,
    /// il volume e l'annuncio del brano dopo. Adesso il gesto alza la bandiera,
    /// e la scrive il filo dell'orologio entro un quarto di secondo, con una
    /// scadenza — vedi `fili::salva_quel_che_manca`. Dieci trascinamenti in un
    /// secondo diventano una scrittura sola.
    pub coda_da_salvare: std::sync::atomic::AtomicBool,
    /// Quando finisce la sfumatura del timer di spegnimento, in millisecondi
    /// dall'epoca. `0`: nessuna in corso.
    ///
    /// Il motore scende e si mette in pausa da solo (vedi
    /// `rymembra_play::Motore::sfuma`); a chi sta sopra resta da fermare
    /// l'ascolto e dirlo alla finestra quando la discesa è finita, e questa è
    /// la sveglia per farlo. Fuori dal lucchetto per la ragione di
    /// [`StatoLettore::spegnimento`]: la legge l'orologio a ogni giro.
    pub sfumando_fino: std::sync::atomic::AtomicI64,
    /// Mette in fila i comandi che cambiano una preferenza del lettore.
    ///
    /// Non protegge un dato: protegge un **ordine**. La scrittura di una
    /// preferenza è uscita dal lucchetto del lettore, e quel lucchetto era
    /// anche quel che impediva a due comandi ravvicinati di arrivare al disco in
    /// un ordine e al lettore in quello opposto. Vedi [`cambia_preferenza`].
    ///
    /// Si prende **per primo e da solo**: chi lo tiene prende poi la libreria e
    /// la lascia, e poi il lettore; nessuno lo chiede tenendo in mano uno degli
    /// altri due. Così non entra nell'ordine «prima il lettore, poi la
    /// libreria», e non può chiudere un cerchio con nessuno.
    preferenze: Mutex<()>,
    /// Le voci della sessione d'«Ascolta insieme»: maniglia → voce.
    ///
    /// Un lucchetto suo e **foglia**: si può prendere col lettore in mano, per
    /// tradurre una maniglia mentre si fotografa la coda, ma chi lo tiene non
    /// chiede mai nient'altro. Fuori dal lettore perché lo legge chi non deve
    /// aspettarlo — la finestra che chiede le schede della coda, il protocollo
    /// delle copertine — e una scheda non vale una pausa in fila. Vedi
    /// [`ospiti`].
    ospiti: Mutex<ospiti::Registro>,
}

/// Il filo della finestra, segnato quando il lettore si apre.
///
/// Serve a [`prendi_il_lettore`] per la stessa domanda che `crate::stato` si
/// fa per la libreria: chi aspetta è il filo che disegna la finestra? Lì
/// un'attesa senza limite non è un comando lento, è una finestra che Windows
/// dichiara «non risponde». Una copia sua e non quella di `crate::stato`
/// perché quella è privata del suo modulo; la segna [`StatoLettore::avvia`],
/// che gira nel `setup` di Tauri — sullo stesso filo.
static FILO_DELLA_FINESTRA: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

/// Quanto il filo della finestra aspetta il lucchetto del lettore.
///
/// Due secondi. Da quando il lavoro lento — database, scheda di Windows, stato
/// da comporre — è uscito dal lucchetto (vedi [`in_ordine`]), chi lo tiene lo
/// tiene per microsecondi: arrivare a due secondi vuol dire che qualcuno sta
/// aspettando il database con il lettore in mano, e la finestra non deve
/// aspettare con lui. Sotto i quattro secondi con cui la stessa finestra
/// rinuncia alla libreria, perché il lettore, di solito, la libreria la
/// aspetta.
const ATTESA_DELLA_FINESTRA: Duration = Duration::from_secs(2);

/// Ogni quanto si riprova il lucchetto mentre si aspetta con una scadenza.
const PASSO_ATTESA: Duration = Duration::from_millis(5);

/// Quanto il filo del seguito aspetta la libreria per comporre uno stato.
///
/// Oltre, la scheda del brano la dà il ricordo — vedi
/// [`StatoLettore::scheda_corrente`] — e lo stato parte lo stesso: un titolo
/// letto un minuto fa vale più di una finestra che non si aggiorna.
const ATTESA_SCHEDA: Duration = Duration::from_secs(1);

/// Il punto in cui la musica si è interrotta per colpa della rete.
///
/// Serve perché «Riprova» rimetta la puntina nel solco e non all'inizio del
/// disco: chi ascoltava era a metà del secondo movimento, e ricominciare da
/// capo sarebbe una punizione per un cavo staccato.
#[derive(Debug, Clone, Copy)]
pub struct Ripresa {
    /// Quale brano si stava ascoltando.
    pub track_id: i64,
    /// A che punto era arrivato, in millisecondi.
    pub ms: u64,
    /// La musica stava andando, non era ferma in pausa.
    ///
    /// # Perché serve saperlo
    ///
    /// Perché la riapertura del dispositivo adesso è automatica, e far ripartire
    /// il suono da sola è giusto **solo se il suono stava andando**. Chi aveva
    /// messo in pausa e poi ha staccato le cuffie non ha chiesto niente:
    /// ritrovare la musica in corso al ritorno del dispositivo sarebbe un
    /// comando che nessuno ha dato.
    ///
    /// Si legge nell'istante del guasto e non dopo: il motore che riapre è un
    /// motore nuovo, e di quel che il primo stava facendo non sa niente.
    pub suonava: bool,
    /// Quando si è segnato, in millisecondi dall'epoca.
    ///
    /// # Perché serve saperlo
    ///
    /// Perché adesso l'attesa può essere lunga. Un'uscita fissata che sparisce
    /// non fa più passare il suono agli altoparlanti: si aspetta che torni, e
    /// tornare può voler dire domattina — le cuffie spente ieri sera e riaccese
    /// al risveglio. Ripartire da soli dopo una notte non è riprendere
    /// l'ascolto, è una musica che parte senza che nessuno l'abbia chiesta.
    /// Oltre [`RIPRESA_FRESCA`] la puntina torna al suo posto ma resta ferma.
    pub quando_ms: i64,
}

/// Fino a quando una [`Ripresa`] può far ripartire la musica da sola.
///
/// Mezz'ora: copre le cuffie che si scaricano e si rimettono in carica, il
/// giro fino all'altra stanza, il telefono che se le è prese per una chiamata.
/// Oltre, chi riaccende le cuffie ritrova il brano al suo posto e preme play.
pub const RIPRESA_FRESCA: i64 = 30 * 60 * 1000;

/// Il valore di [`StatoLettore::spegnimento`] che dice «quando finisce questo».
///
/// Negativo perché non è un istante: è un modo, e mescolarlo agli istanti
/// veri senza un valore che nessun orologio produrrà mai vorrebbe dire un
/// timer che scatta nel 1970.
pub const FINE_DEL_BRANO: i64 = -1;

/// Apre il dispositivo audio e collega l'osservatore degli eventi.
///
/// In una funzione sua perché adesso i posti che aprono un motore sono due:
/// l'avvio e [`riapri_audio`]. Due copie di questa riga vorrebbero dire un
/// motore riaperto che non manda più eventi alla finestra — cioè un lettore che
/// suona e non lo dice a nessuno, che è il difetto peggiore da diagnosticare
/// perché somiglia a tutto.
///
/// L'osservatore riceve l'[`Annuncio`] intero e non il solo evento: gira sul
/// filo «rymembra-eventi» del motore, e i controlli che l'annuncio porta — il
/// motore è ancora questo? qualcuno ha già voltato pagina? — vanno fatti sotto
/// il lucchetto del lettore, che è qui. Vedi [`su_evento`].
fn apri_motore(app: &tauri::AppHandle, voluto: Option<String>) -> Result<Motore, AppError> {
    let manico = app.clone();
    Motore::avvia_con_annunci(move |annuncio| su_evento(&manico, annuncio), voluto)
}

impl StatoLettore {
    /// Avvia il motore e riprende la coda di ieri.
    ///
    /// Restituisce anche gli estremi da ascoltare dei due canali — quello del
    /// preparatore e quello del sorvegliante dei dispositivi — che vanno
    /// passati ad [`avvia_preparatore`] e [`avvia_sorveglianza`]: la stessa
    /// forma di `nuvola::StatoNuvola::nuovo`, e per la stessa ragione — un
    /// canale nasce insieme allo stato che ne tiene l'estremo da cui si manda.
    pub fn avvia(
        app: &tauri::AppHandle,
    ) -> (
        Self,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Receiver<()>,
    ) {
        // La preferenza si legge **prima** di aprire, ed è l'unica lettura dal
        // database su questa strada: da qui in poi vive nella copia in memoria
        // qui sotto. Un database che non risponde vale «predefinito di
        // sistema», che è il ripiego di tutto il resto del modulo.
        // Si gira nel `setup`, cioè sul filo della finestra: è il momento di
        // segnarlo. Vedi [`FILO_DELLA_FINESTRA`].
        let _ = FILO_DELLA_FINESTRA.set(std::thread::current().id());
        let voluta = uscita_scelta(app);
        let motore = apri_motore(app, voluta.clone());
        let lettore = motore.map(|motore| Lettore {
            motore,
            coda: Queue::new(),
            ascolto: None,
            volume: Volume::default(),
            eq: Equalizzazione::default(),
            normalizzazione: Normalizzazione::default(),
            autoplay: false,
            dissolvenza_s: 0,
            latenza_ms: 0,
            motivo_prossimo: None,
            coda_di_prima: None,
            coda_curata: false,
            coda_del_giro: None,
            pausa_chiesta: (0, false),
            in_canna: None,
            non_aperto: None,
            annuncio_atteso: None,
            puntina_a_vuoto: None,
            sessione: None,
            fermato_della_sessione: false,
        });
        let (prepara, orecchio) = std::sync::mpsc::channel();
        let (sveglia_dispositivi, orecchio_dispositivi) = std::sync::mpsc::channel();
        let seguito = avvia_seguito(app);
        let scritture = avvia_scritture(app);
        (
            Self {
                lettore: Mutex::new(lettore),
                spettro: std::sync::atomic::AtomicBool::new(false),
                spettro_bande: std::sync::atomic::AtomicU16::new(
                    rymembra_play::RISOLUZIONE_DI_SERIE,
                ),
                spegnimento: std::sync::atomic::AtomicI64::new(0),
                ripresa: Mutex::new(None),
                scheda_corrente: Mutex::new(None),
                prepara,
                volume_da_salvare: std::sync::atomic::AtomicBool::new(false),
                eq_da_salvare: std::sync::atomic::AtomicBool::new(false),
                cataloghi: Cataloghi::nuovi(),
                uscita_voluta: Mutex::new(voluta),
                riapertura_in_corso: std::sync::atomic::AtomicBool::new(false),
                sveglia_dispositivi,
                seguito,
                scritture,
                scritture_chieste: std::sync::atomic::AtomicU64::new(0),
                scritture_fatte: std::sync::atomic::AtomicU64::new(0),
                coda_da_salvare: std::sync::atomic::AtomicBool::new(false),
                sfumando_fino: std::sync::atomic::AtomicI64::new(0),
                aprendo: std::sync::atomic::AtomicI64::new(NESSUN_BRANO),
                preferenze: Mutex::new(()),
                ospiti: ospiti::registro_nuovo(),
            },
            orecchio,
            orecchio_dispositivi,
        )
    }
}

/// Aspetta che il filo del seguito e quello delle scritture abbiano finito
/// quel che hanno in fila, ma non oltre `fine`.
///
/// Serve all'uscita: gli ascolti, il segno e gli stati spediti un istante prima
/// di chiudere devono arrivare al database prima che il processo se ne vada.
/// Un segnaposto in fondo a ciascuna fila, e l'attesa delle due risposte:
/// quando rispondono, tutto quel che c'era prima è stato fatto.
///
/// Una scadenza sola per le due file, e non una ciascuna: l'attesa all'uscita
/// è corta apposta, e due attese di fila la raddoppierebbero.
pub(crate) fn aspetta_le_file(app: &tauri::AppHandle, fine: std::time::Instant) {
    let (fatto_seguito, seguito) = std::sync::mpsc::channel::<()>();
    in_ordine(app, move |_| {
        let _ = fatto_seguito.send(());
    });
    let (fatto_scritture, scritture) = std::sync::mpsc::channel::<()>();
    da_scrivere(app, move |_| {
        let _ = fatto_scritture.send(());
    });
    aspetta_le_risposte(&[seguito, scritture], fine);
}

/// L'uscita scelta, letta dal database.
///
/// Fuori da [`StatoLettore::avvia`] perché la rilegge anche
/// [`scegli_dispositivo_audio`] dopo aver scritto: rileggere quel che si è
/// appena scritto, invece di fidarsi di quel che si voleva scrivere, è la
/// stessa regola che il modulo segue per la dissolvenza e per le barre dello
/// spettro — il nucleo può normalizzare, e la finestra deve vedere il valore
/// vero.
fn uscita_scelta(app: &tauri::AppHandle) -> Option<String> {
    let stato = app.try_state::<Stato>()?;
    con_libreria(&stato, |libreria| {
        rymembra_app::playback::load_uscita(&libreria.connection)
    })
    .ok()
    .flatten()
}

/// La libreria, se è ancora fra gli stati gestiti.
///
/// `try_state` e non `state`, ed è la regola di tutto questo modulo da quando i
/// suoi fili sono sorvegliati: `state` **panica** se lo stato non c'è, e le
/// strade che passano di qui girano sui fili di sottofondo — il preparatore,
/// gli annunci del motore, il seguito, il sorvegliante dei dispositivi — che
/// possono ritrovarsi vivi mentre l'applicazione si chiude e gli stati sono
/// già stati lasciati cadere. Un panico lì voleva dire un filo morto in
/// silenzio; adesso è un errore che chi chiama sa già trattare. La stessa
/// forma, e lo stesso codice, di `comandi::libreria_di`.
fn libreria_di<'a>(
    app: &'a tauri::AppHandle,
    cosa: &'static str,
) -> Result<State<'a, Stato>, AppError> {
    app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some(cosa.to_owned()),
        })
        .with_cause("la libreria non è più fra gli stati gestiti")
    })
}

/// Stampa se il dispositivo audio si è aperto, e come.
///
/// Una riga sola e c'è sempre, come quella della libreria: quando qualcuno dirà
/// «non si sente niente», la frequenza e i canali con cui il dispositivo si è
/// aperto sono la prima cosa da guardare, e senza questa riga non esisterebbero
/// da nessuna parte.
pub fn riga_di_avvio_lettore(stato: &StatoLettore) {
    let guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_ref() {
        Ok(lettore) => {
            let f = lettore.motore.formato();
            // `bluetooth` è quel che ha visto `cpal`, e decide quanto presto la
            // pausa libera l'uscita: se le cuffie non passano al telefono, è
            // la prima cosa da guardare.
            nota!(
                "[avvio] audio aperto uscita={} frequenza={} canali={} bluetooth={}",
                lettore.motore.dispositivo(),
                f.frequenza,
                f.canali,
                if lettore.motore.su_bluetooth() {
                    "sì"
                } else {
                    "no"
                }
            );
        }
        Err(err) => nota!(
            "[avvio] audio NON aperto codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
    }
}

/// Esegue `azione` sul lettore aperto.
///
/// Sul filo della finestra rinuncia dopo [`ATTESA_DELLA_FINESTRA`]: vedi
/// [`prendi_il_lettore`]. Altrove aspetta quanto serve, come prima.
fn con_lettore<T>(
    stato: &StatoLettore,
    azione: impl FnOnce(&mut Lettore) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut guardia = prendi_il_lettore(stato)?;
    match guardia.as_mut() {
        Ok(lettore) => azione(lettore),
        Err(err) => Err(err.clone()),
    }
}

/// Il lucchetto del lettore, con la regola del filo della finestra.
///
/// # Perché una scadenza, e solo lì
///
/// Perché i comandi scritti senza `(async)` — `pausa`, `vai_a`, `volume`,
/// l'equalizzatore — girano sul filo che disegna la finestra, e restano
/// sincroni apposta: arrivano a raffica da un cursore sotto il dito, e sul
/// filo della finestra si eseguono nell'ordine in cui sono partiti, cosa che
/// su un gruppo di fili nessuno garantisce. Il prezzo era che il lucchetto lo
/// aspettavano **senza limite**, e chi lo teneva poteva star aspettando il
/// database: la finestra si fermava con lui, e dopo cinque secondi Windows la
/// dichiarava morta.
///
/// Adesso lì si rinuncia con `db.locked`, che è ritentabile e dice la verità —
/// quando il lettore resta chiuso per secondi, è quasi sempre perché qualcuno
/// lo tiene aspettando il database. Sugli altri fili — gli eventi, il
/// preparatore, l'orologio, i comandi asincroni — l'attesa resta quella di
/// sempre: lì aspettare non ferma niente che si veda.
///
/// Il veleno si recupera, come ovunque in questo modulo.
fn prendi_il_lettore(
    stato: &StatoLettore,
) -> Result<MutexGuard<'_, Result<Lettore, AppError>>, AppError> {
    let sulla_finestra = FILO_DELLA_FINESTRA
        .get()
        .is_some_and(|filo| *filo == std::thread::current().id());
    if !sulla_finestra {
        return Ok(stato
            .lettore
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner));
    }
    let inizio = std::time::Instant::now();
    loop {
        match stato.lettore.try_lock() {
            Ok(guardia) => return Ok(guardia),
            Err(std::sync::TryLockError::Poisoned(veleno)) => return Ok(veleno.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {
                if inizio.elapsed() >= ATTESA_DELLA_FINESTRA {
                    nota!(
                        "[riproduzione] il filo della finestra rinuncia al lettore dopo {} s",
                        ATTESA_DELLA_FINESTRA.as_secs()
                    );
                    return Err(AppError::new(ErrorCode::DbLocked).with_cause(format!(
                        "lucchetto del lettore non disponibile entro {} s sul filo della finestra",
                        ATTESA_DELLA_FINESTRA.as_secs()
                    )));
                }
                std::thread::sleep(PASSO_ATTESA);
            }
        }
    }
}

/// La presa dello spettro, se il lettore è libero **adesso** — e niente se no.
///
/// La usa il filo dello spettro, che trenta volte al secondo prendeva il
/// lettore con l'attesa senza limite degli altri fili: la presa è un `Arc` da
/// clonare, ma per clonarlo si metteva in fila dietro a chiunque tenesse il
/// lucchetto, e ogni attesa era un evento in ritardo — cioè uno scatto nella
/// scena, che scorre a file che arrivano. Adesso non aspetta mai: se il lettore
/// è occupato, il filo legge con la presa che aveva già.
///
/// Quella presa cambia solo quando cambia il motore, cioè a una riapertura del
/// dispositivo, e quella vecchia non fa danni: l'anello di un motore buttato
/// non lo riempie più nessuno, e il lettore lo legge come silenzio
/// (`rymembra_play::spettro`, «Quando il suono si ferma») finché un giro non
/// trova il lettore libero e la rinnova.
///
/// Il veleno si recupera, come in [`prendi_il_lettore`]; un lettore che non
/// c'è — nessun dispositivo — non ha una presa da dare.
fn presa_spettro_se_libera(stato: &StatoLettore) -> Option<rymembra_play::motore::PresaSpettro> {
    let guardia = match stato.lettore.try_lock() {
        Ok(guardia) => guardia,
        Err(std::sync::TryLockError::Poisoned(veleno)) => veleno.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    guardia
        .as_ref()
        .ok()
        .map(|lettore| lettore.motore.presa_spettro())
}

/// Un seme per il mescolamento.
///
/// L'orologio: il dominio non ne ha uno — è la sua regola — quindi glielo porta
/// chi ce l'ha. Due mescolamenti nello stesso nanosecondo darebbero lo stesso
/// ordine, il che è esattamente ciò che serve per provarli e non è un problema
/// per usarli.
fn seme() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        // I bit bassi dei nanosecondi: sono quelli che cambiano, e sono l'unica
        // parte che serve. Troncare i bit alti di un `u128` che conta i
        // nanosecondi dal 1970 toglie l'epoca e lascia il rumore.
        .map_or(0, |d| {
            u64::try_from(d.as_nanos() & u128::from(u64::MAX)).unwrap_or(0)
        })
}

/// Chiude l'ascolto in corso e lo scrive, se conta.
///
/// La chiusura si fa **qui**, con il lucchetto del lettore in mano, perché è
/// adesso che l'ascolto finisce e l'orologio va letto adesso. La scrittura no:
/// va sul filo delle scritture (vedi [`da_scrivere`]), perché prende il
/// lucchetto della libreria, e aspettarlo tenendo in mano il lettore voleva
/// dire far aspettare anche la pausa premuta nel frattempo. Non sul seguito:
/// lì dietro aspetterebbe lo stato del brano nuovo.
fn chiudi_ascolto(app: &tauri::AppHandle, lettore: &mut Lettore) {
    let Some(tracker) = lettore.ascolto.take() else {
        return;
    };
    let ascolto = tracker.finish(adesso_ms());
    if !ascolto.counts {
        return;
    }
    da_scrivere(app, move |app| scrivi_ascolto(app, &ascolto));
}

/// Scrive un ascolto finito, e avvisa chi ne deve sapere. Gira sul filo delle
/// scritture, senza il lucchetto del lettore: qui l'attesa della libreria non
/// fa aspettare nessuno stato.
fn scrivi_ascolto(app: &tauri::AppHandle, ascolto: &Listen) {
    let scritto = libreria_di(app, "ascolto").and_then(|stato| {
        con_libreria(&stato, |libreria| {
            // L'ascolto si intesta a questo computer. Non è contabilità: il
            // conteggio è la somma dei contatori di tutti i dispositivi, e un
            // ascolto senza mittente non saprebbe in quale sommarsi.
            let dispositivo = crate::nuvola::dispositivo(&libreria.connection)?;
            playback::record_play(&mut libreria.connection, ascolto, &dispositivo)
        })
    });
    match scritto {
        // `true` vuol dire che il conteggio è cresciuto davvero. Segnalarlo
        // anche quando l'ascolto non contava riempirebbe il canale del backup
        // di sveglie a vuoto: chi salta un brano dopo tre secondi non ha
        // cambiato niente da salvare.
        Ok(true) => {
            crate::nuvola::sporca(app);
            // Dopo `record_play` e solo se ha scritto: la coda degli scrobble
            // deve contenere le stesse righe della cronologia, non un insieme
            // suo. Non va in rete da qui — accoda e sveglia il filo.
            crate::scrobble::dopo_un_ascolto(app, ascolto);
        }
        Ok(false) => {}
        Err(err) => nota!(
            "[riproduzione] ascolto non registrato codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
    }
}

/// Quanto si aspetta l'apertura di un brano prima di rinunciare.
///
/// Cinque secondi: un file locale si apre in microsecondi e uno su una share
/// viva in millisecondi, mentre su una share morta Windows ci mette una
/// quarantina di secondi a rinunciare da solo. Più stretto della sonda delle
/// radici (otto secondi) perché lì si sta preparando una scansione e qui c'è
/// qualcuno che ha appena premuto play: cinque secondi di nulla sono già
/// parecchi, quaranta sono un'applicazione rotta.
const APERTURA_BRANO: Duration = Duration::from_secs(5);

/// Quanto si aspetta l'apertura di un brano che arriva dalla rete — un flusso
/// di un catalogo, o un file su una cartella di rete.
///
/// Quindici secondi e non cinque, perché i cinque sono il conto di un disco e
/// qui il conto è un altro. Per i file di un NAS il conto è quello dei suoi
/// dischi che ripartono: vedi [`brano_di`]. Misurato sull'Internet Archive: due secondi prima
/// del primo byte, di cui novecento millisecondi di rimando al nodo, e il
/// primo suono a quattro secondi e sei decimi — a un soffio dalla scadenza dei
/// file. Audius, che prima deve scegliere un nodo, l'ha superata al primo
/// tentativo e non al secondo. Con cinque secondi un brano sano falliva una
/// volta su qualche, con un avviso che parlava d'altro.
///
/// Qui nessun lucchetto è in mano mentre si aspetta (vedi [`brano_di`]), quindi
/// l'attesa più lunga costa solo a chi ha premuto play, e a lui costa meno di
/// un errore.
const APERTURA_FLUSSO: Duration = Duration::from_secs(15);

/// Il nome che il filo dell'apertura porta nel diario dei panici.
const APERTURA_BRANO_NOME: &str = "apertura-brano";

/// Chi sta accanto a `track_id` nella coda, e se la coda è mescolata.
///
/// Lo chiede il livello automatico per capire se si sta ascoltando un disco.
/// Il lucchetto del lettore si tiene il tempo di due letture e si lascia
/// prima di prendere quello della libreria: nessuno dei due si aspetta con
/// l'altro in mano. Se il lettore non risponde — sul filo della finestra si
/// rinuncia dopo [`ATTESA_DELLA_FINESTRA`] — il brano si apre lo stesso, col
/// livello suo: meglio un disco un po' meno fedele che un brano che non parte.
fn intorno_di(app: &tauri::AppHandle, track_id: i64) -> playback::Intorno {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return playback::Intorno::default();
    };
    let intorno = con_lettore(&stato, |lettore| {
        let (prima, dopo) = lettore.coda.neighbors(track_id);
        Ok(playback::Intorno {
            mescolato: lettore.coda.shuffle(),
            prima,
            dopo,
        })
    })
    .unwrap_or_default();
    // In sessione i vicini sono maniglie, e il database confronta righe: la
    // riga dietro ciascuna, o nessun vicino. Si traduce a lettore lasciato —
    // il registro è un lucchetto foglia, ma qui non serve nemmeno tenerli
    // insieme. Senza, chi ospita il suo disco perderebbe il livello d'album.
    playback::Intorno {
        prima: intorno.prima.and_then(|id| ospiti::riga_dietro(app, id)),
        dopo: intorno.dopo.and_then(|id| ospiti::riga_dietro(app, id)),
        ..intorno
    }
}

/// Apre la sorgente di un brano, senza tenere in mano la libreria mentre aspetta.
///
/// # Perché due metà e non una chiamata sola
///
/// Perché `playback::sorgente` fa due cose di velocità incomparabile: legge una
/// riga di database, e **apre un file**. La prima ha bisogno del lucchetto della
/// libreria e dura microsecondi; la seconda non ne ha bisogno e su una
/// condivisione di rete morta dura quaranta secondi — durante i quali, se il
/// lucchetto fosse ancora in mano, sarebbero fermi anche il filo della scansione,
/// quello dell'arricchimento e quello del backup, cioè tutta l'applicazione per
/// un cavo staccato.
///
/// `rymembra_app::playback` è già spaccato apposta nelle due metà. Qui la prima
/// gira sotto lucchetto, il lucchetto si lascia, e la seconda va su un filo suo
/// con una scadenza addosso.
///
/// # Quel che resta preso
///
/// Niente. Chi chiama ha deciso quale brano aprire sotto il lucchetto del
/// **lettore** e l'ha lasciato prima di arrivare qui — tutti e cinque, ed è una
/// condizione, non una cortesia: [`intorno_di`] lo riprende per un attimo per
/// guardare la coda, e un `Mutex` preso due volte dallo stesso filo non
/// risponde più.
///
/// # Cosa sta dentro la scadenza, e perché ci sta tutto
///
/// Aprire un brano è due letture, non una: la `File::open`, che è veloce anche
/// su una share viva, e il **riconoscimento del contenitore**, che non lo è —
/// symphonia legge tutti i blocchi di metadati prima di poter dire di che
/// formato si tratta, e su un FLAC con la copertina incorporata sono centinaia
/// di kilobyte. Finché la seconda avveniva di là dal confine, dentro il filo di
/// decodifica, la scadenza copriva la parte veloce e lasciava scoperta quella
/// lenta: una share che moriva dopo la `open` piantava il filo per i quaranta
/// secondi di Windows. Adesso `BranoAperto::apri` sta qui dentro, e la scadenza
/// vale per tutto quel che legge.
///
/// # I due modi in cui un brano si apre
///
/// Un file, oppure un **flusso**. La differenza sta tutta dentro la chiusura, e
/// non è un caso: la scadenza deve coprire tutte e due, e una richiesta HTTP che
/// non torna è la stessa attesa di una share che non risponde — chi ha premuto
/// play deve avere una risposta. Il ramo lo sceglie [`riferimento_di`], che è
/// l'unico posto che sa distinguere le due cose. La scadenza no: un flusso ha
/// la sua, [`APERTURA_FLUSSO`], perché cinque secondi sono il conto di un disco.
///
/// La rete, dentro la chiusura, si può aspettare: qui non c'è più nessun
/// lucchetto in mano, ed è la stessa proprietà per cui l'apertura di un file era
/// stata spostata qui. Vedi il preambolo del modulo.
///
/// # Errori
///
/// Quando la scadenza passa, `fs.networkUnavailable` per un file — un'apertura
/// che non finisce in cinque secondi su un percorso che il database conosce è
/// una share che non risponde, ed è ritentabile — e `net.timeout` per un
/// flusso. Fino alla 2.4.0 anche il flusso diceva `fs.networkUnavailable`, e la
/// finestra rispondeva «la cartella di rete non risponde: la VPN caduta, il
/// cavo staccato» per un brano dell'Internet Archive che di cartelle non ne ha.
/// `playback.formatUnsupported` per un contenitore che non si riconosce — o il
/// cui riconoscimento cade in panico, che non è la rete: vedi
/// [`apertura_mancata`] — e i `net.*` di [`apri_flusso`] quando la rete risponde
/// prima della scadenza per dire di no.
///
/// # Una voce di sessione
///
/// Un id negativo è una voce d'«Ascolta insieme», e l'apre [`ospiti::apri`]:
/// dalla libreria, dalla rete o da un catalogo, con la stessa regola delle
/// scadenze e nessun lucchetto in mano. È qui e non in ognuno dei cinque
/// chiamanti — la partenza, il preparatore, «Riprova», la riapertura del
/// dispositivo, l'«Annulla» — perché ognuno di loro, adesso, può avere in
/// mano una maniglia: un ramo dimenticato in uno solo vorrebbe dire una voce
/// che parte con l'id sbagliato e una coda che avanza da sola.
fn brano_di(
    app: &tauri::AppHandle,
    track_id: i64,
    formato: rymembra_play::FormatoUscita,
) -> Result<rymembra_play::BranoAperto, AppError> {
    match ospiti::chi(app, track_id) {
        Chi::Libreria(riga) => brano_di_libreria(app, riga, riga, formato),
        Chi::Voce(voce) => ospiti::apri(app, track_id, &voce, formato),
        Chi::Sparita => Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: None,
            path: None,
        })
        .with_cause(format!(
            "la voce {track_id} non c'è più: la sessione è finita o l'ha tolta"
        ))),
    }
}

/// I cataloghi del lettore, clonati fuori dallo stato: un `State` non
/// attraversa il confine di un filo, e il clone è quasi gratis — l'agente sta
/// in un `Arc` e la riserva di connessioni resta la stessa (vedi
/// [`StatoLettore::cataloghi`]).
fn cataloghi_di(app: &tauri::AppHandle, cosa: &'static str) -> Result<Cataloghi, AppError> {
    app.try_state::<StatoLettore>()
        .map(|stato| stato.cataloghi.clone())
        .ok_or_else(|| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some(cosa.to_owned()),
            })
            .with_cause("il lettore non è più fra gli stati gestiti")
        })
}

/// [`brano_di`] per una riga della libreria, annunciata dal motore come
/// `maniglia`.
///
/// Fuori sessione le due coincidono. In sessione chi ospita apre il suo file
/// sotto la maniglia della voce: il `track_id` della sorgente è quello che il
/// motore annuncia e mette nelle posizioni, e deve essere quello che la coda
/// contiene — con l'id della riga, l'annuncio d'inizio non troverebbe la
/// maniglia in coda e la farebbe avanzare di un passo a ogni brano.
fn brano_di_libreria(
    app: &tauri::AppHandle,
    track_id: i64,
    maniglia: i64,
    formato: rymembra_play::FormatoUscita,
) -> Result<rymembra_play::BranoAperto, AppError> {
    let stato = libreria_di(app, "apertura del brano")?;
    let intorno = intorno_di(app, maniglia);
    let mut scheda = con_libreria(&stato, |libreria| {
        playback::scheda_da_suonare(&libreria.connection, track_id, intorno)
    })?;
    scheda.track_id = maniglia;
    // Il riferimento si ricava **prima** della chiusura, che si porta via la
    // scheda: da qui in giù serve solo per sapere che faccia dare a una scadenza
    // passata, e chiederlo di nuovo dopo non si potrebbe.
    let remoto = riferimento_di(&scheda);
    // Il percorso si copia prima per la stessa ragione: senza di lui l'errore
    // direbbe «la rete non risponde» senza dire quale cartella andare a
    // ricollegare. È il file che si **apre** — per un brano d'immagine CD
    // l'immagine, non il percorso `…\Album.cue|03` che in tabella lo nomina e
    // che `percorso()` giustamente non dà: senza, un brano d'immagine sul NAS
    // avrebbe preso la scadenza corta di un disco locale.
    let percorso = scheda.collocazione.file_audio().map(str::to_owned);
    let cataloghi = cataloghi_di(app, "apertura del brano")?;
    let riferimento = remoto.clone();
    // Un file su una cartella di rete aspetta quanto un flusso: un NAS che
    // dorme fa girare i dischi in otto, quindici secondi, e Windows ricollega
    // una lettera mappata solo quando qualcuno ci bussa. Con cinque secondi il
    // primo play dopo l'accensione falliva quasi sempre, con un avviso che dava
    // la colpa alla rete. La risposta viene da una mappa in memoria, mai dal
    // disco: vedi `sorveglia::forse_in_rete`.
    let in_rete = percorso
        .as_deref()
        .is_some_and(|percorso| crate::sorveglia::forse_in_rete(app, percorso));
    // Un brano del cloud si apre come un flusso: la sorgente del servizio
    // dentro la cache. Aspetta quanto un flusso, per la stessa ragione.
    let del_cloud = percorso
        .as_deref()
        .is_some_and(rymembra_domain::nuvola::e_nuvola);
    let archivio = crate::nuvola_musica::archivio(app);
    // Mentre un brano del cloud suona la passata dei tag rallenta, per non
    // contendergli la linea. Il preparatore apre il brano dopo in anticipo, e
    // quindi la bandiera può cambiare un brano prima del tempo: è una
    // cortesia sulla banda, non una regola, e sbagliare di un brano costa una
    // scansione un po' più lenta o un po' più veloce.
    if let Some(archivio) = &archivio {
        archivio.suona_dal_cloud(del_cloud);
    }
    let scadenza = if remoto.is_some() || in_rete || del_cloud {
        APERTURA_FLUSSO
    } else {
        APERTURA_BRANO
    };
    let senza_durata = scheda.durata_ms <= 0;
    let esito =
        rymembra_app::scadenza::con_scadenza_o_caduta(APERTURA_BRANO_NOME, scadenza, move || {
            match riferimento {
                Some(rif) => {
                    let (flusso, da_suonare) = apri_flusso(&cataloghi, &rif)?;
                    let sorgente = sorgente_da_flusso(&scheda, &da_suonare, flusso);
                    // Il riconoscimento del contenitore legge dalla rete, e un
                    // guasto lì arriva col codice delle cartelle: vedi
                    // [`interruzione_di_flusso`].
                    rymembra_play::BranoAperto::apri(sorgente, formato)
                        .map_err(interruzione_di_flusso)
                }
                None if del_cloud => {
                    let file = scheda
                        .collocazione
                        .file_audio()
                        .unwrap_or_default()
                        .to_owned();
                    flusso::apri_da_nuvola(archivio.as_deref(), &scheda, &file, formato)
                }
                None => {
                    let sorgente =
                        playback::sorgente_da_scheda(&rymembra_app::files::LocalFiles, &scheda)?;
                    rymembra_play::BranoAperto::apri(sorgente, formato)
                }
            }
        });
    let aperto = esito.unwrap_or_else(|mancato| {
        Err(apertura_mancata(
            &mancato,
            scadenza,
            remoto.is_some() || del_cloud,
            percorso,
        ))
    });
    // Una riga del cloud entrata in libreria prima della lettura dei tag non
    // ha ancora una durata: il contenitore appena aperto la dice, e si scrive
    // adesso — la barra del brano e i conti dell'ascolto la vogliono subito,
    // non quando la passata dei tag arriverà a questo file.
    if senza_durata
        && let Ok(brano) = &aperto
        && let Some(durata) = brano.durata_dichiarata_ms()
        && let Err(guasto) = con_libreria(&stato, |libreria| {
            playback::ricorda_durata(&libreria.connection, track_id, durata)
        })
    {
        crate::nota!("[riproduzione] la durata di {track_id} non si scrive: {guasto}");
    }
    aperto
}

/// Perché un brano non si è aperto, quando l'apertura non ha nemmeno risposto.
///
/// # Le due mancate, e perché non sono la stessa
///
/// **Scaduta**: l'apertura non è tornata in tempo. Su un file è una cartella
/// di rete che non risponde, su un flusso un catalogo che non risponde: in
/// tutti e due i casi aspettare può bastare, e il codice è ritentabile —
/// `fs.networkUnavailable` o `net.timeout`, e la fascia offre «Riprova».
///
/// **Caduta**: l'apertura è tornata **subito**, ma senza un risultato, perché
/// il riconoscimento del contenitore — codice di terzi davanti a byte
/// qualunque — è andato in panico. Fino a questa versione le due erano lo
/// stesso `None`, e un file locale rovinato si presentava come «la cartella di
/// rete non risponde»: «Riprova» per un file che non si aprirà mai, a ogni
/// tentativo, per sempre. Adesso è `playback.formatUnsupported`, che non è
/// ritentabile e dice la cosa vera — il file non si legge — con il messaggio
/// del panico nella causa, per il diario. Il codice è già nel catalogo e già
/// tradotto: un codice nuovo vorrebbe dire una frase che la finestra non ha.
fn apertura_mancata(
    mancato: &rymembra_app::scadenza::Mancato,
    scadenza: Duration,
    da_un_catalogo: bool,
    percorso: Option<String>,
) -> AppError {
    match mancato {
        rymembra_app::scadenza::Mancato::Caduto { messaggio } => AppError::new(
            ErrorCode::PlaybackFormatUnsupported { format: None },
        )
        .with_cause(format!(
            "il riconoscimento del formato è caduto: {}",
            messaggio.as_deref().unwrap_or("panico senza messaggio")
        )),
        rymembra_app::scadenza::Mancato::Scaduto => {
            let causa = format!(
                "l'apertura non è finita entro {} secondi",
                scadenza.as_secs()
            );
            if da_un_catalogo {
                return AppError::new(ErrorCode::NetTimeout {
                    url: None,
                    timeout_ms: u64::try_from(scadenza.as_millis()).ok(),
                })
                .with_cause(causa);
            }
            AppError::new(ErrorCode::FsNetworkUnavailable { path: percorso }).with_cause(causa)
        }
    }
}

/// Compone lo stato e lo manda alla finestra — e al sistema operativo.
///
/// I destinatari sono qui insieme di proposito. La scheda nel riquadro del
/// volume di Windows e l'icona del vassoio dicono le stesse cose della barra in
/// fondo alla finestra, e aggiornarle da un posto loro vorrebbe dire più
/// sorgenti per la stessa verità: prima o poi una mostrerebbe il brano di
/// prima, e sarebbe il difetto che nessuno segnala perché chi lo vede pensa di
/// aver letto male.
///
/// # Cosa si fa qui e cosa dopo
///
/// Qui, con il lucchetto del lettore in mano, solo la fotografia: quel che si
/// legge dal lettore, senza database. Il resto — la scheda del brano, la
/// finestra, Windows, il vassoio — lo fa il filo del seguito, nell'ordine in cui
/// le fotografie sono state scattate. Vedi [`in_ordine`].
///
/// # E agli altri PC della sessione
///
/// Chi ospita avvisa qui anche la sessione che il lettore è cambiato: ogni
/// gesto, ogni brano nuovo, ogni pausa passa già da questa funzione, e un
/// avviso messo altrove sarebbe un posto in più dove dimenticarlo. Non blocca:
/// suona un campanello, e lo stato per gli altri lo compone il filo della
/// sessione. Vedi [`insieme::cambiato`].
pub(crate) fn manda_stato(app: &tauri::AppHandle, lettore: &Lettore) {
    let abbozzo = abbozzo(app, lettore);
    insieme::cambiato(app, lettore);
    in_ordine(app, move |app| pubblica(app, abbozzo));
}

/// Come [`manda_stato`], ma senza avvisare la sessione: per chi ridisegna
/// perché è cambiato qualcosa **fuori** dal lettore — una copertina di
/// sessione arrivata, una voce che ha trovato la sua copia — e non deve far
/// ripartire uno stato verso gli altri PC che a sua volta lo farebbe
/// ridisegnare.
pub(super) fn manda_stato_alla_finestra(app: &tauri::AppHandle, lettore: &Lettore) {
    let abbozzo = abbozzo(app, lettore);
    in_ordine(app, move |app| pubblica(app, abbozzo));
}

/// Il primo stato, a chi lo mostra fuori dalla finestra.
///
/// Il riquadro multimediale di Windows, l'icona del vassoio e i pulsanti sotto
/// la miniatura nascono in `main` **dopo** che la coda di ieri è tornata al suo
/// posto, e lo stato lo ricevono solo quando qualcosa cambia. La finestra lo
/// chiede da sé appena si apre; loro no, e restavano spenti — nessun titolo,
/// tasti grigi — fino al primo play, anche con la coda pronta. Si chiama una
/// volta, dopo che tutti e tre ci sono.
pub fn primo_stato_fuori(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return;
    };
    let _ = con_lettore(&stato, |lettore| {
        manda_stato(app, lettore);
        Ok(())
    });
}

/// Come [`manda_stato`], ma dicendo dove il brano *sarà* invece che dov'è.
///
/// I comandi al motore sono accodati: `vai_a` e `suona` tornano prima che il
/// filo di decodifica abbia mosso il cursore, e la posizione che si leggerebbe
/// un microsecondo dopo è ancora quella di prima. Mandandola così com'è, chi
/// trascina il cursore lo vede tornare al punto di partenza e poi saltare in
/// avanti un decimo di secondo più tardi — il rimbalzo che sembra un cursore
/// che non risponde.
///
/// Non è una seconda verità sulla posizione: è la stessa che il motore
/// raggiungerà, detta in anticipo. L'evento `riproduzione:tempo` la sostituisce
/// da subito con quella vera, e se il salto fallisse — un file che non si apre
/// — l'errore arriva per la sua strada.
fn manda_stato_con_posizione(app: &tauri::AppHandle, lettore: &Lettore, ms: u64) {
    let mut abbozzo = abbozzo(app, lettore);
    abbozzo.stato.posizione_ms = ms;
    insieme::cambiato(app, lettore);
    in_ordine(app, move |app| pubblica(app, abbozzo));
}

/// Quel che arriva dal motore.
///
/// Gira sul filo «rymembra-eventi» del motore — non su quello della decodifica,
/// non sulla callback audio: qui si può prendere un lucchetto e aspettare il
/// database senza che il suono ne sappia niente. Fino a ieri girava sul filo
/// della decodifica, e ogni attesa di qui era un pezzo d'anello che nessuno
/// riempiva: al cambio di brano, cioè dove il gapless doveva non avere buchi.
///
/// # I due controlli, sotto il lucchetto
///
/// Fra l'annuncio e questa funzione passa del tempo, e nel lucchetto del
/// lettore possono essere successe due cose che l'annuncio non sa. Si
/// controllano **dopo** averlo preso, perché è lì che la risposta smette di
/// poter cambiare:
///
/// - **il motore è stato sostituito** ([`Annuncio::ha_voce`]): la riapertura
///   del dispositivo cambia il motore dentro questo stesso lucchetto, e un
///   annuncio del vecchio arrivato in coda dietro di lei parlerebbe al nuovo di
///   un brano che non ha. Si tace, come faceva il motore stesso quando il
///   controllo stava soltanto di là;
/// - **qualcuno ha già voltato pagina** ([`Annuncio::superato`]): un brano
///   scelto a mano, o lo stop, dopo che l'annuncio era partito. Chi ha voltato
///   pagina ha già chiuso l'ascolto e mosso la coda; rileggere l'annuncio li
///   muoverebbe una seconda volta, oltre il brano appena scelto.
fn su_evento(app: &tauri::AppHandle, annuncio: Annuncio) {
    // `try_state` e non `state`: gli annunci arrivano anche mentre
    // l'applicazione si smonta, e uno stato che non c'è più vuol dire che non
    // c'è più nessuno a cui raccontarli.
    let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
        return;
    };
    match &annuncio.evento {
        Evento::Iniziato { track_id } => {
            let track_id = *track_id;
            let _ = con_lettore(&stato_lettore, |lettore| {
                if !annuncio.ha_voce() || annuncio.superato() {
                    return Ok(());
                }
                iniziato(app, lettore, track_id, annuncio.posizione);
                Ok(())
            });
        }
        Evento::Fermato => {
            let _ = con_lettore(&stato_lettore, |lettore| {
                if !annuncio.ha_voce() || annuncio.superato() {
                    return Ok(());
                }
                fermato(app, &stato_lettore, lettore);
                Ok(())
            });
        }
        Evento::Errore(err) => {
            // Anche un guasto passa dal lucchetto, per il primo dei due
            // controlli: quello di un motore sostituito riguarda campioni che
            // non usciranno più, e dirlo alla finestra vorrebbe dire un avviso
            // per un brano che nel frattempo è ripartito altrove.
            let Ok((ha_voce, superato)) = con_lettore(&stato_lettore, |_| {
                Ok((annuncio.ha_voce(), annuncio.superato()))
            }) else {
                return;
            };
            if !ha_voce {
                return;
            }
            nota!(
                "[riproduzione] {} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            // Un guasto di rete non è la fine del brano: è una pausa imposta da
            // un cavo. Il motore si è fermato invece di scorrere la coda (vedi
            // il commento in `rymembra_play::motore::decodifica_un_blocco`), e
            // qui si annota **dove**: lo dice la posizione che l'annuncio si
            // porta dietro, presa un istante prima che la fermata la azzerasse.
            // Finché questa funzione girava sul filo della decodifica la si
            // leggeva dal motore in tempo; adesso arriva dopo, e dal motore
            // leggerebbe il vuoto.
            //
            // Non per un guasto superato: chi ha già scelto un altro brano non
            // deve ritrovarsi un «Riprova» che lo riporta indietro.
            //
            // Vale identico per un flusso, e senza una riga in più: una
            // connessione che cade a metà canzone arriva fin qui con lo stesso
            // codice, perché `rymembra_net::flusso` la traduce in un numero di
            // sistema che `rymembra_domain::errors::rete` riconosce. La strada del
            // «Riprova» è quindi una sola per tutti e due i casi — che è il
            // punto: un brano interrotto dalla rete non deve raccontare due
            // storie diverse a seconda di dove stavano i byte.
            //
            // La strada sì, la frase no. Il codice che arriva dal motore parla
            // di una cartella di rete, e per un brano che arriva da un
            // catalogo quella frase manda a controllare un NAS che non c'è:
            // qui, dove si sa quale brano era, diventa quella giusta.
            let mut err = (**err).clone();
            if err.code().kind() == ErrorCodeKind::FsNetworkUnavailable
                && !superato
                && let Some(track_id) = annota_ripresa_da(&stato_lettore, annuncio.posizione)
                && e_un_flusso(app, track_id)
            {
                err = interruzione_di_flusso(err);
            }
            app.emetti("riproduzione:errore", crate::errore::errore(err));
        }
    }
}

/// Un brano ha cominciato a uscire dalle casse: il ramo `Iniziato` di
/// [`su_evento`], con il lucchetto del lettore già in mano.
///
/// `posizione` è quella dell'annuncio: il brano che comincia, con la sua
/// durata. Prima la si leggeva dal motore mentre il motore aspettava, e il
/// motore la scriveva **dopo** l'annuncio: quel che si leggeva era la
/// posizione del brano di prima, e la durata giusta la ripescava il database.
fn iniziato(app: &tauri::AppHandle, lettore: &mut Lettore, track_id: i64, posizione: Posizione) {
    // La voce di calibrazione non è in coda e non è un ascolto: la coda resta
    // sul brano che ripartirà dopo, e l'ascolto di quel brano resta fermo dov'è
    // (lo ha fermato `insieme::calibra`). Senza questa uscita, la coda — che
    // non la trova — avanzerebbe di un passo.
    if insieme::e_la_calibrazione(track_id) {
        lettore.annuncio_atteso = None;
        manda_stato(app, lettore);
        return;
    }

    // La riga da contare, se ce n'è una: per una voce di sessione quella che la
    // voce dichiara, o nessuna. È l'id che va all'ascolto e allo scrobble:
    // `record_play` rifiuta le maniglie, e il rifiuto finirebbe nel diario a
    // ogni voce. Vedi `ospiti::da_contare`.
    let conta = ospiti::da_contare(app, track_id);

    // Il brano rimesso nel solco dopo che il dispositivo è tornato: l'ascolto è
    // quello di prima, fermo, e riparte da dove si era fermato invece di
    // chiudersi. Vedi `riapertura::e_la_ripresa_dell_ascolto`.
    let continua = riapertura::e_la_ripresa_dell_ascolto(lettore, track_id, conta);

    // Il brano precedente è finito **adesso**, non prima: con il gapless fra
    // i due non c'è un istante di silenzio, e questo è l'unico momento in cui
    // si sa che è successo.
    if !continua {
        chiudi_ascolto(app, lettore);
    }

    // La coda può essere indietro di un passo: il motore ha attaccato il
    // brano che gli era stato preparato. Si chiede al successivo in canna e
    // non all'identificativo, che con lo stesso brano due volte di fila non
    // distingue il brano che riparte da quello che già suonava — vedi
    // `Lettore::in_canna`. Il confronto con la coda resta per quel che il
    // motore attacca senza passare dalla canna di questo lettore: un entrante
    // rimesso in canna da un salto a metà dissolvenza.
    //
    // Prima di tutto, però, il brano appena avviato: vedi
    // `Lettore::annuncio_atteso` e [`dopo_l_annuncio`].
    let atteso = lettore.annuncio_atteso.take();
    if insieme::segue(lettore) {
        // Chi segue non sceglie il brano dopo: lo decide l'ancora dell'host, e
        // la coda è lo specchio della sua. Il cursore va sulla voce che è
        // cominciata — l'ha attaccata il gapless o una riapertura del
        // regolatore — perché il brano dopo che il preparatore apre in anticipo
        // sia quello giusto: lasciato dov'era, il «dopo» sarebbe ancora questa
        // voce, che finirebbe di nuovo in canna e suonerebbe due volte. Lo
        // stato dell'host che arriva subito dopo lo conferma o lo corregge.
        insieme::cursore_sulla_voce(lettore, track_id);
        if lettore.in_canna == Some(track_id) {
            lettore.in_canna = None;
        }
    } else {
        match dopo_l_annuncio(track_id, atteso, lettore.coda.current(), lettore.in_canna) {
            DopoLAnnuncio::Resta => {}
            DopoLAnnuncio::AvanzaDallaCanna => {
                lettore.in_canna = None;
                lettore.coda.advance(false);
            }
            DopoLAnnuncio::Avanza => {
                lettore.coda.advance(false);
            }
        }
    }

    // La pausa si legge adesso e non dall'annuncio: se qualcuno ha premuto
    // pausa nel frattempo, il suo comando ha fermato l'ascolto **di prima**,
    // e quello che comincia qui deve cominciare fermo anche lui.
    let in_pausa = lettore.motore.posizione().in_pausa;
    if continua {
        if let Some(ascolto) = lettore.ascolto.as_mut()
            && !in_pausa
        {
            ascolto.resume(adesso_ms());
        }
    } else if let Some(riga) = conta {
        let durata = if posizione.track_id == Some(track_id) && posizione.durata_ms > 0 {
            posizione.durata_ms
        } else {
            durata_di(app, track_id)
        };
        let mut ascolto = ListenTracker::begin(riga, durata, adesso_ms());
        // Un brano rimesso al suo posto in pausa — l'«Annulla» di una coda
        // ferma, la riapertura di un dispositivo — comincia fermo: senza, il
        // tempo passato in pausa contava come ascoltato.
        if in_pausa {
            ascolto.pause(adesso_ms());
        }
        lettore.ascolto = Some(ascolto);
    } else {
        // Una voce che non conta — il brano di un altro PC — non ha un
        // ascolto: quel che si sente qui lo conta chi possiede la riga.
        lettore.ascolto = None;
    }

    // Il «sta ascoltando» verso i servizi di scrobbling. Parte su un filo suo:
    // qui il lucchetto del lettore è in mano, e una richiesta HTTP dentro
    // questa funzione terrebbe fermo tutto quel che lo chiede. Solo per una
    // riga che conta: lo scrobble di un brano altrui lo fa chi lo possiede.
    if let Some(riga) = conta {
        crate::scrobble::sta_suonando(app, riga);
    }

    // Il testo del **prossimo**, chiesto adesso che c'è tempo: torna subito e
    // il lavoro va su un filo suo, come la riga qui sopra e per la stessa
    // ragione. Vedi `crate::testi::precarica_prossimo`.
    crate::testi::precarica_prossimo(app, lettore.coda.peek_next());

    prepara_prossimo(app, lettore);
    salva_coda(app, lettore);
    manda_stato(app, lettore);
}

/// L'ultimo brano è uscito per intero: il ramo `Fermato` di [`su_evento`],
/// con il lucchetto del lettore già in mano.
fn fermato(app: &tauri::AppHandle, stato_lettore: &StatoLettore, lettore: &mut Lettore) {
    // Lo stop con cui si è usciti da una sessione: vedi
    // `Lettore::fermato_della_sessione`. Si ridisegna e basta.
    let della_sessione = std::mem::take(&mut lettore.fermato_della_sessione);
    let rete_caduta = stato_lettore
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some();
    let da_fare = dopo_il_fermato(
        della_sessione,
        insieme::segue(lettore),
        rete_caduta,
        lettore.coda.current(),
    );
    if !da_fare.tocca_l_utente {
        manda_stato(app, lettore);
        return;
    }
    chiudi_ascolto(app, lettore);
    // Il timer «alla fine di questo brano» ha fatto il suo lavoro: la musica si
    // è fermata dove doveva. Non ha una scadenza — `scade_il_timer` guarda solo
    // gli istanti positivi — e nessun altro lo spegneva: la finestra continuava
    // a dirlo acceso, e il giorno dopo la prima canzone si fermava di nuovo da
    // sola senza che nessuno l'avesse chiesto. Lo scambio solo se vale ancora
    // «fine del brano»: un timer a minuti acceso nel frattempo resta suo.
    let _ = stato_lettore.spegnimento.compare_exchange(
        FINE_DEL_BRANO,
        0,
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
    );
    // Il corrente è finito e in canna non c'era niente perché il brano dopo
    // non si è aperto: la coda va su di lui, così la finestra mostra quale —
    // fermo, accanto all'errore — e «riproduci» riprova quello invece di
    // ricominciare il brano appena finito.
    //
    // Non per chi segue: la sua coda è lo specchio di quella dell'host, e un
    // passo fatto qui sarebbe un passo che l'host non ha fatto.
    //
    // E da lì si riprova ad aprirlo, e se non si apre si passa oltre: vedi
    // [`riprova_o_salta`]. Su un filo suo, perché qui il lucchetto è in mano.
    if da_fare.avanza_sul_non_aperto
        && lettore.non_aperto.is_some()
        && lettore.non_aperto == lettore.coda.peek_next()
        && lettore.motore.posizione().track_id.is_none()
    {
        lettore.non_aperto = None;
        lettore.coda.advance(false);
        salva_coda(app, lettore);
        let mano = app.clone();
        let avviato = std::thread::Builder::new()
            .name("rymembra-riprova".to_owned())
            .spawn(move || riprova_o_salta(&mano));
        if avviato.is_err() {
            nota!("[riproduzione] il filo che riprova il brano dopo non è partito");
        }
    }
    // Il segno di «riprendi dov'eri» torna all'inizio del brano su cui la coda
    // si è fermata. Era quello di cinque secondi prima della fine, e
    // «Riprendi» il giorno dopo saltava lì: un brano che finiva di nuovo
    // appena ripartito. Non quando la fermata viene da una rete caduta — lì il
    // punto a metà è proprio quel che si vuole ritrovare, e `annota_ripresa`
    // l'ha appena segnato.
    //
    // La scrittura va sul filo delle scritture, come ogni altra scrittura che
    // nasce sotto questo lucchetto: vedi [`da_scrivere`]. Per quale brano lo
    // decide [`dopo_il_fermato`], dalla coda di **adesso**: quella del brano
    // non aperto, se è appena avanzata.
    if let Some(id) = dopo_il_fermato(
        della_sessione,
        insieme::segue(lettore),
        rete_caduta,
        lettore.coda.current(),
    )
    .segno_a_zero
    {
        da_scrivere(app, move |app| {
            let _ = libreria_di(app, "segno di fine brano").and_then(|stato_app| {
                con_libreria(&stato_app, |libreria| {
                    playback::save_posizione(&libreria.connection, id, 0)
                })
            });
        });
    }
    manda_stato(app, lettore);
}

/// Che cosa fa un `Fermato`, deciso senza motore: vedi [`fermato`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DopoIlFermato {
    /// Ascolto, timer «fine del brano», coda e segno: tutto quel che è
    /// dell'utente. Falso per lo stop dell'uscita da una sessione.
    tocca_l_utente: bool,
    /// La coda può avanzare sul brano dopo che non si è aperto. Mai per chi
    /// segue: la sua coda è lo specchio di quella dell'host.
    avanza_sul_non_aperto: bool,
    /// Il brano il cui segno di «riprendi dov'eri» torna a zero.
    segno_a_zero: Option<i64>,
}

/// La regola di [`fermato`]. Il segno torna a zero solo per una riga della
/// libreria — mai per una maniglia: il segno è di domani, e domani la sessione
/// non c'è più — e mai quando la fermata è di una rete caduta, dove il punto a
/// metà è proprio quel che si vuole ritrovare.
const fn dopo_il_fermato(
    della_sessione: bool,
    segue: bool,
    rete_caduta: bool,
    corrente: Option<i64>,
) -> DopoIlFermato {
    if della_sessione {
        return DopoIlFermato {
            tocca_l_utente: false,
            avanza_sul_non_aperto: false,
            segno_a_zero: None,
        };
    }
    let segno_a_zero = match corrente {
        Some(id) if ospiti::e_di_libreria(id) && !rete_caduta => Some(id),
        _ => None,
    };
    DopoIlFermato {
        tocca_l_utente: true,
        avanza_sul_non_aperto: !segue,
        segno_a_zero,
    }
}

/// Questo brano arriva dalla rete?
///
/// Una lettura di una riga, sotto il lucchetto della libreria e basta: si
/// chiede nel momento del guasto, che è raro, e la risposta non vale la pena
/// di tenerla da parte. Nel dubbio — la libreria non risponde — si dice di no,
/// cioè si lascia l'errore com'era.
///
/// Una voce di sessione lo dice da sé: i byte di un altro PC o di un catalogo
/// arrivano dalla rete, e un guasto lì è un flusso interrotto, non una
/// cartella di questo PC da ricollegare. Una voce che apre un file di questo PC
/// si chiede alla sua riga, come sempre; una voce in rete che suona dalla sua
/// copia trovata qui non è un flusso (`ospiti::e_un_flusso`), e il guasto
/// resta quello della cartella, come lo vede chi ospita.
fn e_un_flusso(app: &tauri::AppHandle, track_id: i64) -> bool {
    let track_id = match ospiti::chi(app, track_id) {
        Chi::Libreria(riga) => riga,
        Chi::Voce(voce) => match (ospiti::e_un_flusso(app, &voce), voce.apertura_di_libreria()) {
            (Some(rete), _) => return rete,
            (None, Some(riga)) => riga,
            (None, None) => return false,
        },
        Chi::Sparita => return false,
    };
    let Some(stato) = app.try_state::<Stato>() else {
        return false;
    };
    con_libreria(&stato, |libreria| {
        playback::scheda_sorgente(&libreria.connection, track_id)
    })
    .is_ok_and(|scheda| riferimento_di(&scheda).is_some())
}

/// Il guasto di rete di un brano che arriva da un catalogo, con la sua frase.
///
/// Solo il codice cambia: la causa resta, perché è quella che dice che cosa è
/// successo al socket, e il diario la vuole.
fn interruzione_di_flusso(err: AppError) -> AppError {
    if err.code().kind() != ErrorCodeKind::FsNetworkUnavailable {
        return err;
    }
    let nuovo = AppError::new(ErrorCode::PlaybackStreamInterrupted);
    match err.cause() {
        Some(causa) => nuovo.with_cause(causa.to_owned()),
        None => nuovo,
    }
}

/// Si segna brano e millisecondo, perché «Riprova» sappia dove tornare.
///
/// Il lucchetto del lettore si prende e si lascia subito: è una lettura di due
/// interi da una casella condivisa, non un'apertura di file. Se il lettore non
/// c'è — nessuna scheda audio — non c'è nemmeno un punto a cui tornare, e
/// tacere è la cosa giusta: questa strada non deve far cadere niente, come
/// tutte quelle che partono da un evento.
///
/// Restituisce il brano annotato, che a chi chiama serve per sapere di che
/// brano era il guasto.
///
/// Legge la posizione dal motore **adesso**: la chiama chi si accorge del
/// guasto guardando il motore — l'orologio, il sorvegliante dei dispositivi.
/// Chi se ne accorge da un annuncio usa [`annota_ripresa_da`] con la posizione
/// che l'annuncio si porta dietro.
fn annota_ripresa(stato: &State<'_, StatoLettore>) -> Option<i64> {
    let Ok(posizione) = con_lettore(stato, |lettore| Ok(lettore.motore.posizione())) else {
        return None;
    };
    annota_ripresa_da(stato, posizione)
}

/// Come [`annota_ripresa`], con la posizione data da chi chiama.
///
/// Serve al ramo dei guasti di [`su_evento`]: l'annuncio arriva dopo che il
/// motore si è fermato, e la posizione di quell'istante la porta lui.
fn annota_ripresa_da(stato: &StatoLettore, posizione: Posizione) -> Option<i64> {
    let track_id = posizione.track_id?;
    let mut guardia = stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Lo stesso punto segnato di nuovo — il sorvegliante lo riscrive dal motore
    // perso prima di ogni riapertura, e mentre si aspetta un'uscita fissata le
    // riaperture si provano ogni due secondi — tiene l'istante della **prima**
    // volta: è da lì che si conta quanto si è aspettato. Vedi
    // [`Ripresa::quando_ms`].
    let quando_ms = guardia
        .filter(|prima| prima.track_id == track_id && prima.ms == posizione.ms)
        .map_or_else(adesso_ms, |prima| prima.quando_ms);
    *guardia = Some(Ripresa {
        track_id,
        ms: posizione.ms,
        // Letto qui e non alla riapertura: fra i due istanti c'è un motore
        // nuovo, che di quel che il vecchio stava facendo non sa niente. È
        // questo bit a decidere se il suono ripartirà da solo.
        suonava: !posizione.in_pausa,
        quando_ms,
    });
    Some(track_id)
}

/// La durata dichiarata dal database.
///
/// Il ripiego di [`iniziato`], quando l'annuncio non la porta: con una scadenza
/// breve, perché si chiede con il lucchetto del lettore in mano. Senza risposta
/// vale zero, cioè «non si sa» — e per il conteggio dell'ascolto una durata
/// ignota è un caso che la regola del dominio conosce già.
///
/// Per una voce di sessione la durata è quella della sua scheda: il database
/// non la conosce, e chiederglielo per una maniglia sarebbe una lettura che
/// risponde sempre «non c'è».
fn durata_di(app: &tauri::AppHandle, track_id: i64) -> u64 {
    if let Some(durata) = ospiti::durata_ms(app, track_id) {
        return durata;
    }
    if !ospiti::e_di_libreria(track_id) {
        return 0;
    }
    let Some(stato) = app.try_state::<Stato>() else {
        return 0;
    };
    con_libreria_entro(&stato, ATTESA_SCHEDA, |libreria| {
        rymembra_app::library::read_summary(&libreria.connection, track_id)
    })
    .and_then(Result::ok)
    .flatten()
    .map_or(0, |b| u64::try_from(b.duration_ms).unwrap_or(0))
}

/// Scrive una preferenza del lettore, la rilegge, e la consegna al lettore.
///
/// # Il difetto che toglie
///
/// I comandi che cambiano una manopola del lettore — l'autoplay, la
/// dissolvenza, la latenza, la normalizzazione — scrivevano sul database
/// **dentro** il lucchetto del lettore. Una scansione che tiene la libreria
/// per secondi teneva così fermo anche il lettore: la pausa premuta in quel
/// momento, l'annuncio del brano dopo, l'orologio, tutti in fila dietro a una
/// preferenza da salvare.
///
/// Adesso si scrive e si rilegge **prima**, con il solo lucchetto della
/// libreria; il lettore si prende dopo, il tempo di copiare il valore riletto.
///
/// # Perché un lucchetto suo, [`StatoLettore::preferenze`]
///
/// Perché quello del lettore, tenuto attraverso la scrittura, faceva anche un
/// altro lavoro: metteva in fila due comandi ravvicinati sulla stessa manopola.
/// Senza, due cursori mossi in fretta potevano scrivere sul disco in un ordine
/// e arrivare al lettore nell'ordine opposto, e la finestra mostrare un valore
/// diverso da quello che sarebbe tornato al prossimo avvio. Questo lucchetto
/// fa solo quello: lo prendono soltanto i comandi delle preferenze, sempre per
/// primo e senza nient'altro in mano — vedi l'ordine scritto in testa al
/// modulo — e lo tengono per una scrittura e una copia.
///
/// `applica` riceve l'esito della scrittura com'è: è lei a sapere qual è il
/// ripiego giusto quando il database non ha risposto.
fn cambia_preferenza<T, R>(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    cosa: &'static str,
    scrivi: impl FnOnce(&mut crate::stato::Libreria) -> Result<T, AppError>,
    applica: impl FnOnce(&mut Lettore, Result<T, AppError>) -> Result<R, AppError>,
) -> Result<R, AppError> {
    let _in_fila = stato
        .preferenze
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scritto = libreria_di(app, cosa).and_then(|libreria| con_libreria(&libreria, scrivi));
    con_lettore(stato, |lettore| applica(lettore, scritto))
}

/// Accende o spegne la coda che non finisce.
///
/// Cambia anche quel che il motore ha già in canna: se si accende mentre
/// l'ultimo brano sta suonando, il successivo va scelto **adesso**, non al
/// prossimo cambio di traccia — che non ci sarebbe.
#[tauri::command(async)]
pub fn autoplay(app: tauri::AppHandle, stato: State<'_, StatoLettore>, attivo: bool) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Autoplay).map_err(errore)? {
        return Ok(());
    }
    // Si rilegge quel che è stato scritto, come per la normalizzazione: se il
    // salvataggio fallisce, l'interruttore non deve restare acceso su una
    // scelta che non sopravvivrà alla chiusura. La scrittura fuori dal
    // lucchetto del lettore: vedi [`cambia_preferenza`].
    cambia_preferenza(
        &app,
        &stato,
        "autoplay",
        |libreria| {
            playback::save_autoplay(&libreria.connection, attivo)?;
            playback::load_autoplay(&libreria.connection)
        },
        |lettore, salvato| {
            lettore.autoplay = salvato.unwrap_or(attivo);
            prepara_prossimo(&app, lettore);
            manda_stato(&app, lettore);
            Ok(())
        },
    )
    .map_err(errore)
}

/// Sceglie quanto due brani si sovrappongono, in secondi. `0` la spegne.
///
/// # Perché non restituisce lo stato
///
/// Perché tocca il motore, e la disciplina dell'IPC dice che chi tocca il
/// motore parla per `riproduzione:stato`: due sorgenti per lo stesso fatto
/// vorrebbero dire una finestra che mostra otto secondi mentre il motore ne fa
/// zero, il giorno in cui il salvataggio fallisse.
///
/// # Quando ha effetto
///
/// Dal **prossimo** cambio di traccia. Una dissolvenza già cominciata va
/// avanti con la durata con cui è partita: cambiarla a metà vorrebbe dire due
/// rampe che non si sommano più a uno, cioè un salto di volume proprio nel
/// punto in cui la dissolvenza esiste per non farne.
///
/// `(async)` per la ragione scritta su `suono::normalizzazione`: si scrive sul
/// database, e sul filo della finestra una libreria occupata fermava la
/// finestra. Il lucchetto del lettore, invece, non aspetta più la scrittura:
/// vedi [`cambia_preferenza`].
#[tauri::command(async)]
pub fn dissolvenza(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    secondi: u64,
) -> Esito<()> {
    // Riletta invece che ripetuta, come per l'autoplay: `save_crossfade` taglia
    // al massimo, e senza rileggere il cursore resterebbe su un valore che il
    // database ha rifiutato.
    cambia_preferenza(
        &app,
        &stato,
        "dissolvenza",
        |libreria| {
            playback::save_crossfade(&libreria.connection, secondi)?;
            playback::load_crossfade(&libreria.connection)
        },
        |lettore, salvato| {
            let salvato = salvato.unwrap_or(secondi.min(playback::CROSSFADE_MASSIMO_S));
            lettore.dissolvenza_s = salvato;
            // Quella che vale adesso: in sessione resta zero, e la scelta
            // appena scritta torna al motore all'uscita. Vedi
            // `insieme::dissolvenza_ms`.
            lettore.motore.dissolvenza(insieme::dissolvenza_ms(lettore));
            manda_stato(&app, lettore);
            Ok(())
        },
    )
    .map_err(errore)
}

/// Dichiara di quanto la catena d'uscita ritarda il suono, in millisecondi.
///
/// È il terzo dei tre ritardi che il `//!` di `rymembra_play::motore` distingue:
/// quello che nessuna misura vede — mixer di sistema, driver, DAC, e su un'uscita
/// senza fili la radio. Il motore ci somma quel che `cpal` gli riporta e toglie
/// il totale dalla posizione raccontata, così cursore, testi e pannello di
/// Windows parlano di quel che l'orecchio sta ricevendo adesso.
///
/// # Perché risponde con un numero
///
/// Perché il nucleo lo taglia a ±`LATENZA_MASSIMA_MS`, e chi ha trascinato il
/// cursore deve vederlo fermarsi dove si è fermato davvero: la stessa disciplina
/// di [`spettro_bande_scegli`] e dell'equalizzatore — si scrive, si rilegge, si
/// restituisce quel che è rimasto scritto.
///
/// # Per quale uscita
///
/// Per quella **davvero aperta**, `Motore::dispositivo()`, e non per la
/// preferenza `player.output`: quando la scheda scelta non c'è l'apertura
/// ripiega su un'altra, e chi muove il cursore lo fa ascoltando quella. Il
/// numero si scrive anche come globale, così il cursore di oggi — uno solo —
/// continua a valere per le uscite che un numero loro non l'hanno ancora: vedi
/// `playback::dichiara_latenza`. Quel che va al motore lo decide
/// `playback::latenza_effettiva` sull'uscita che c'è **quando si applica**: se
/// nel frattempo il motore è stato riaperto altrove, vale il numero di quella.
///
/// # Cosa non fa
///
/// Non tocca il suono, non sposta la fine di un brano e non rimanda il gapless:
/// la fine si dichiara quando l'ultimo campione ha lasciato l'anello del motore,
/// e il perché sta in `rymembra_play::motore::Contesto::forse_fine`.
///
/// **Niente `nuvola::se_riuscito`**, come ogni altro setter di preferenze di
/// questo modulo — e qui con una ragione in più: è un numero che descrive
/// *questo* cavo e *questa* scheda, e portarlo su un altro computer ne
/// descriverebbe un'altra. Per lo stesso motivo non entra in
/// `profilo::CATALOGO`, accanto a `player.output`.
///
/// `(async)`, come [`dissolvenza`].
#[tauri::command(async)]
pub fn latenza(app: tauri::AppHandle, stato: State<'_, StatoLettore>, ms: i64) -> Esito<i64> {
    // Il nome dell'uscita si chiede **prima** di `cambia_preferenza`, che vuole
    // il suo lucchetto per primo e senza niente in mano. Un lettore mai aperto
    // non ha un'uscita: si scrive la sola globale.
    let uscita = con_lettore(&stato, |lettore| {
        Ok(lettore.motore.dispositivo().to_owned())
    })
    .ok();
    let quando_ms = adesso_ms();
    // Riletta invece che ripetuta, come per la dissolvenza: `save_latenza`
    // taglia, e senza rileggere il cursore resterebbe su un valore che il
    // database ha rifiutato.
    cambia_preferenza(
        &app,
        &stato,
        "latenza",
        |libreria| {
            playback::dichiara_latenza(&libreria.connection, uscita.as_deref(), ms, quando_ms)
        },
        |lettore, salvato| {
            let salvato = salvato.map_or_else(
                |_| ms.clamp(-playback::LATENZA_MASSIMA_MS, playback::LATENZA_MASSIMA_MS),
                |(globale, voci)| {
                    playback::latenza_effettiva(globale, &voci, Some(lettore.motore.dispositivo()))
                },
            );
            let prima = lettore.latenza_ms;
            lettore.motore.latenza(salvato);
            lettore.latenza_ms = salvato;
            // In sessione il numero nuovo si sente subito: vedi
            // `insieme::latenza_cambiata`.
            insieme::latenza_cambiata(lettore, prima, salvato);
            manda_stato(&app, lettore);
            Ok(salvato)
        },
    )
    .map_err(errore)
}

// ── lo spettro ──────────────────────────────────────────────────────────────

/// Accende o spegne lo spettro.
///
/// Due interruttori e non uno: quello del motore ferma la scrittura
/// nell'anello dalla callback audio, questo ferma l'evento. Spegnendoli si
/// smette di pagare in tutti e due i posti, ed è la ragione per cui il comando
/// esiste invece di lasciare lo spettro sempre acceso.
#[tauri::command]
pub fn spettro(stato: State<'_, StatoLettore>, attivo: bool) -> Esito<()> {
    stato
        .spettro
        .store(attivo, std::sync::atomic::Ordering::Relaxed);
    let bande = stato
        .spettro_bande
        .load(std::sync::atomic::Ordering::Relaxed);
    con_lettore(&stato, |lettore| {
        lettore.motore.guarda_spettro(attivo);
        // La risoluzione si rimanda a ogni accensione. Costa un lucchetto già
        // preso e toglie di mezzo il caso in cui il dispositivo audio si sia
        // riaperto mentre la schermata era chiusa: là il lettore dello spettro è
        // nuovo e non sa niente della scelta di chi guarda.
        lettore.motore.spettro_dettaglio(bande);
        Ok(())
    })
    .map_err(errore)
}

/// Quante barre disegna la scena dello spettro.
#[tauri::command]
pub fn spettro_bande(stato: State<'_, Stato>) -> Esito<u16> {
    con_libreria(&stato, |libreria| {
        playback::load_spettro_bande(&libreria.connection)
    })
    .map_err(errore)
}

/// Sceglie quante barre disegna la scena dello spettro. Riporta com'è rimasta.
///
/// # Perché la risposta è un numero e non un `()`
///
/// Perché fra le potenze di due non c'è niente, e un valore che non è una di
/// quelle si porta alla più vicina invece di essere rifiutato. Chi ha premuto
/// deve vedere la linguetta che è rimasta accesa davvero — la stessa disciplina
/// dell'equalizzatore, che rilegge quel che ha scritto invece di fidarsi di
/// quel che è arrivato.
#[tauri::command]
pub fn spettro_bande_scegli(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    quante: u16,
) -> Esito<u16> {
    let stato_app = libreria_di(&app, "barre dello spettro").map_err(errore)?;
    let rimaste = con_libreria(&stato_app, |libreria| {
        playback::save_spettro_bande(&libreria.connection, quante)?;
        playback::load_spettro_bande(&libreria.connection)
    })
    .map_err(errore)?;
    stato
        .spettro_bande
        .store(rimaste, std::sync::atomic::Ordering::Relaxed);
    // Il motore può non esserci — nessuna scheda audio — e la preferenza resta
    // comunque scritta: è una scelta di disegno, e negarla perché le casse non
    // rispondono sarebbe legare due cose che non c'entrano.
    let _ = con_lettore(&stato, |lettore| {
        lettore.motore.spettro_dettaglio(rimaste);
        Ok(())
    });
    Ok(rimaste)
}

/// Se la scena dello spettro parte accesa.
#[tauri::command]
pub fn spettro_visibile(stato: State<'_, Stato>) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        playback::load_spettro_visibile(&libreria.connection)
    })
    .map_err(errore)
}

/// Sceglie se la scena dello spettro parte accesa. Riporta com'è rimasta.
///
/// Scrive, **rilegge** e restituisce quel che è rimasto scritto: la stessa
/// disciplina di [`spettro_bande_scegli`] e dell'equalizzatore. Qui il valore
/// non può cambiare passando dal database — un booleano è già ritagliato — ma
/// la rilettura non è cerimonia: è ciò che fa dipingere l'interruttore dal
/// database invece che dal click, e quindi ciò che impedisce a una scrittura
/// fallita di lasciare acceso un interruttore che domani sarà spento.
///
/// **Niente `nuvola::se_riuscito`**, come nessun altro setter di preferenze di
/// questo modulo: il backup su Drive copia due righe di `settings` (cartelle
/// sorvegliate e skin attiva) e la sincronia tre — nessuna delle quali è
/// questa. Chiamarlo qui vorrebbe dire un giro su Drive che non porterebbe via
/// niente.
#[tauri::command]
pub fn spettro_visibile_scegli(stato: State<'_, Stato>, acceso: bool) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        playback::save_spettro_visibile(&libreria.connection, acceso)?;
        playback::load_spettro_visibile(&libreria.connection)
    })
    .map_err(errore)
}

/// Quanto la scena dello spettro può costare a questa macchina.
#[tauri::command]
pub fn spettro_qualita(stato: State<'_, Stato>) -> Esito<Qualita> {
    con_libreria(&stato, |libreria| {
        playback::load_spettro_qualita(&libreria.connection)
    })
    .map_err(errore)
}

/// Sceglie il tetto di qualità della scena. Riporta quale è rimasto.
///
/// # Perché entra una `String` ed esce una [`Qualita`]
///
/// Perché l'asimmetria **è** il ritaglio, reso visibile nella firma. Dal filo
/// arriva una stringa, che può essere qualunque cosa: un `"ultra"` scritto da
/// una versione futura, o da qualcuno che prova i comandi a mano.
/// [`Qualita::da_nome`] la porta su uno dei tre livelli che esistono — ogni
/// altro nome vale `auto` — e la risposta è il livello vero, non la stringa
/// chiesta. Chi ha premuto vede accesa la linguetta che è rimasta accesa
/// davvero, che è la stessa regola di [`spettro_bande_scegli`].
///
/// Come sopra: **niente `nuvola::se_riuscito`**, questa riga non sta né nel
/// backup né nella sincronia.
#[tauri::command]
pub fn spettro_qualita_scegli(stato: State<'_, Stato>, livello: String) -> Esito<Qualita> {
    let scelta = Qualita::da_nome(&livello);
    con_libreria(&stato, |libreria| {
        playback::save_spettro_qualita(&libreria.connection, scelta)?;
        playback::load_spettro_qualita(&libreria.connection)
    })
    .map_err(errore)
}

// ── i dati tecnici del file ─────────────────────────────────────────────────

/// Se i dati tecnici del file si mostrano sotto i comandi.
///
/// Serve all'interruttore delle Impostazioni, che è l'unico a chiederlo: le due
/// viste che disegnano la riga non leggono questa preferenza, e la ragione sta
/// nella carta di [`StatoRiproduzione::formato`].
#[tauri::command]
pub fn formato_visibile(stato: State<'_, Stato>) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        playback::load_formato_visibile(&libreria.connection)
    })
    .map_err(errore)
}

/// Sceglie se i dati tecnici del file si mostrano. Riporta com'è rimasta.
///
/// Scrive, **rilegge** e restituisce quel che è rimasto scritto: la stessa
/// disciplina di [`spettro_visibile_scegli`]. Un booleano non può cambiare
/// passando dal database, ma la rilettura è ciò che fa dipingere l'interruttore
/// dal database invece che dal click — e quindi ciò che impedisce a una
/// scrittura fallita di lasciare acceso qualcosa che domani sarà spento.
///
/// # Perché questo setter manda lo stato e quello dello spettro no
///
/// Perché la preferenza dello spettro la legge la schermata, quando si apre;
/// questa la legge [`completa`], che decide se il dato viaggia. Senza
/// [`manda_stato`] l'interruttore cambierebbe il database e non la finestra: la
/// riga resterebbe disegnata — o resterebbe via — fino al prossimo cambio di
/// brano, e sarebbe un interruttore che sembra rotto. Con la chiamata il cambio
/// si vede subito **in entrambe** le viste, la colonna e lo schermo intero, che
/// sono lo stesso stato.
///
/// **Niente `nuvola::se_riuscito`**, come nessun altro setter di preferenze di
/// questo modulo: il backup su Drive copia due righe di `settings` e la
/// sincronia tre, nessuna delle quali è questa. A portarla su un altro computer
/// ci pensa il profilo, dove la chiave è in elenco.
#[tauri::command]
pub fn formato_visibile_scegli(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    acceso: bool,
) -> Esito<bool> {
    let stato_app = libreria_di(&app, "formato visibile").map_err(errore)?;
    let rimasto = con_libreria(&stato_app, |libreria| {
        playback::save_formato_visibile(&libreria.connection, acceso)?;
        playback::load_formato_visibile(&libreria.connection)
    })
    .map_err(errore)?;
    // Il lettore può non esserci — nessuna scheda audio, o nessun brano — e la
    // preferenza resta comunque scritta: è una scelta di cosa si legge, e
    // negarla perché le casse non rispondono sarebbe legare due cose che non
    // c'entrano. La stessa clemenza di `spettro_bande_scegli`.
    let _ = con_lettore(&stato, |lettore| {
        manda_stato(&app, lettore);
        Ok(())
    });
    Ok(rimasto)
}

// ── la strada dei gesti, quando c'è una sessione ────────────────────────────

/// Com'è finita la strada di un gesto, per il comando che l'ha chiesta.
///
/// La traduzione di [`Instradato`] in quel che un comando deve fare dopo: il
/// rifiuto è già diventato l'errore del `?`, e restano tre strade.
pub(super) enum Strada {
    /// Si prosegue come sempre.
    Prosegui,
    /// Si prosegue con queste maniglie al posto dei brani chiesti.
    ConVoci(Vec<i64>),
    /// Il gesto l'ha preso l'host: il comando torna `Ok(())` senza fare
    /// altro, e quel che cambia arriva col prossimo stato della sessione.
    Fatto,
}

/// La strada di un gesto, chiesta alla sessione.
///
/// # Dove si chiama, e perché lì
///
/// **In testa al corpo** di ogni comando di trasporto e di coda, prima di
/// qualunque lucchetto e prima del risveglio del flusso. In testa, perché i
/// tasti multimediali, i pulsanti della miniatura e il vassoio chiamano queste
/// funzioni direttamente (`crate::media`): una strada messa solo davanti alla
/// finestra la scavalcherebbero tutti. Prima di ogni lucchetto, perché la
/// decisione può voler dire costruire delle voci dal database — un `suona` di
/// chi ospita — e il database non si aspetta col lettore in mano. La prova
/// `ogni_comando_di_trasporto_e_di_coda_chiede_la_strada_per_primo` tiene vera
/// la regola anche per il comando che verrà aggiunto domani.
///
/// # Errori
///
/// Quello del rifiuto, così com'è: `insieme.gestoNonDisponibile`, o quel che
/// la sessione ha deciso.
pub(super) fn strada(app: &tauri::AppHandle, gesto: &Gesto) -> Result<Strada, AppError> {
    match crate::insieme::instrada(app, gesto) {
        Instradato::Locale => Ok(Strada::Prosegui),
        Instradato::ConVoci(maniglie) => Ok(Strada::ConVoci(maniglie)),
        Instradato::Inoltrato => Ok(Strada::Fatto),
        Instradato::Rifiutato(err) => Err(err),
    }
}

/// [`strada`] per un gesto che porta dei brani: `None` se il gesto è già
/// stato fatto altrove; altrimenti le maniglie delle voci che la sessione ha
/// appena creato, se ne ha create.
///
/// Le maniglie non si usano qui: se entrano in coda lo decide il corpo del
/// comando, sotto il lucchetto del lettore, guardando se il lettore ospita
/// davvero. Vedi `insieme::brani_da_mettere`.
///
/// # Errori
///
/// Quelli di [`strada`].
pub(super) fn voci_dalla_strada(
    app: &tauri::AppHandle,
    gesto: &Gesto,
) -> Result<Option<Option<Vec<i64>>>, AppError> {
    Ok(match strada(app, gesto)? {
        Strada::Prosegui => Some(None),
        Strada::ConVoci(maniglie) => Some(Some(maniglie)),
        Strada::Fatto => None,
    })
}

/// Esegue il nucleo di un comando del lettore sul pool bloccante, e ne
/// restituisce l'esito.
///
/// # Perché
///
/// Perché i comandi che fanno partire un brano o aprono un dispositivo possono
/// aspettare secondi — un file su una cartella di rete che si sveglia, i
/// quindici di [`APERTURA_FLUSSO`]; un'uscita audio, i venti
/// dell'apertura — e un `#[tauri::command(async)]` su una `fn` sincrona non
/// gira su un filo suo: `tauri-macros` lo manda a `async_runtime::spawn`, cioè
/// su un worker del runtime, uno per processore e prestati a **tutti** i
/// comandi asincroni. Quattro clic su brani diversi con il NAS addormentato
/// li fermavano tutti: la libreria non scorreva e la ricerca non rispondeva
/// fino a quindici secondi. Il pool bloccante esiste per questo, e cresce:
/// vedi [`crate::disparte`].
///
/// # Il nucleo e il comando
///
/// Il lavoro sta nel **nucleo** — `riprendi_ora`, `prossimo_ora`, … — che è il
/// comando di prima, sincrono e senza attributo: lo chiamano anche i tasti
/// multimediali (`crate::media`), l'esecutore della sessione e «Riprova». Il
/// comando è un `async fn` che passa il nucleo di qui. Lo stato del lettore si
/// prende dentro, dal registro: un prestito dello stato non entra nel pool,
/// che vuole lavori `'static`.
///
/// # Errori
///
/// Quelli del nucleo; `internal.aborted` se il lettore non è più fra gli stati
/// gestiti — l'applicazione si chiude — o se il lavoro cade.
async fn col_lettore_in_disparte<T: Send + 'static>(
    app: tauri::AppHandle,
    nome: &'static str,
    nucleo: impl for<'a> FnOnce(tauri::AppHandle, State<'a, StatoLettore>) -> Esito<T> + Send + 'static,
) -> Esito<T> {
    crate::disparte::in_disparte(nome, move || {
        let Some(stato) = app.try_state::<StatoLettore>() else {
            return Err(errore(
                AppError::new(ErrorCode::InternalAborted {
                    what: Some(nome.to_owned()),
                })
                .with_cause("il lettore non è più fra gli stati gestiti"),
            ));
        };
        nucleo(app.clone(), stato)
    })
    .await
    .map_err(errore)?
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// [`suona_ora`], sul pool bloccante: vedi [`col_lettore_in_disparte`].
#[tauri::command]
pub async fn suona(app: tauri::AppHandle, brani: Vec<i64>, indice: usize) -> Esito<()> {
    col_lettore_in_disparte(app, "suona", move |app, stato| {
        suona_ora(app, stato, brani, indice)
    })
    .await
}

/// Fa partire una coda nuova a partire dal brano indicato.
///
/// In sessione la coda ha un tetto (`MASSIMO_VOCI`): prima della strada si
/// tiene la finestra di brani attorno a quello scelto, così le voci che la
/// sessione crea sono quelle che si sentiranno. Vedi
/// [`insieme::finestra_di_sessione`].
pub fn suona_ora(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
    indice: usize,
) -> Esito<()> {
    let (brani, indice) = insieme::finestra_di_sessione(&app, brani, indice);
    let gesto = Gesto::Suona {
        ids: brani.clone(),
        da: indice,
    };
    let Some(voci) = voci_dalla_strada(&app, &gesto).map_err(errore)? else {
        return Ok(());
    };
    // La riga dietro ogni voce, per ritrovare il brano scelto fra le voci: dal
    // registro, prima del lettore.
    let voci = voci.map(|maniglie| {
        let righe = insieme::righe_delle_voci(&app, &maniglie);
        (maniglie, righe)
    });
    gesto_di_coda(&app, &stato, |lettore| {
        // Voci o righe lo decide il lettore, e un brano scelto che non è
        // diventato una voce non tocca la coda di tutti: vedi
        // `insieme::lista_da_suonare`.
        let (scelta, da_dimenticare) =
            insieme::lista_da_suonare(insieme::ospita(lettore), brani, indice, voci);
        insieme::dimentica_voci(&app, &da_dimenticare);
        let (brani, indice) = scelta?;
        ricorda_coda_di_prima(&app, lettore, Some(&brani));
        chiudi_ascolto(&app, lettore);
        // In sessione le voci della coda di prima che la nuova non nomina
        // escono dal registro, o crescerebbe di mille voci a ogni «suona»
        // fino a «Termina»: vedi `insieme::dimentica_le_uscite`. Quella che il
        // motore suona ancora e il successivo in canna restano (`tenute`).
        let prima = lettore.coda.in_play_order();
        let tenute = insieme::tenute_dal_lettore(&stato, lettore);
        lettore.coda.play_tracks(brani, indice, seme());
        lettore.coda_curata = false;
        insieme::dimentica_le_uscite(&app, &stato, lettore, &prima, tenute);
        Ok(true)
    })
    .map_err(errore)
}

/// [`radio_ora`], sul pool bloccante: vedi [`col_lettore_in_disparte`].
#[tauri::command]
pub async fn radio(app: tauri::AppHandle, brano: i64) -> Esito<()> {
    col_lettore_in_disparte(app, "radio", move |app, stato| radio_ora(app, stato, brano)).await
}

/// Fa partire una radio seminata da un brano.
///
/// # Perché non è `suona` con una lista più lunga
///
/// Perché la lista la costruisce il nucleo, e costruirla vuol dire prendere il
/// lucchetto della libreria e leggere impronte e cronologie per qualche
/// migliaio di candidati. Farlo dalla finestra vorrebbe dire un `invoke` per
/// chiedere la radio e un secondo per suonarla, con in mezzo un viaggio
/// dell'IPC per un elenco di trenta numeri che alla finestra non serve.
///
/// Il seme sta in testa alla coda perché è il brano che si è premuto: una radio
/// che comincia da un altro brano è una radio che ha ignorato il gesto.
///
/// In sessione si rifiuta: una radio cambierebbe la coda di tutti con trenta
/// brani scelti dalla cronologia di uno solo. Vedi `crate::insieme::strada`.
pub fn radio_ora(app: tauri::AppHandle, stato: State<'_, StatoLettore>, brano: i64) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Radio).map_err(errore)? {
        return Ok(());
    }
    let brano = riga_di_libreria(brano).map_err(errore)?;
    let stato_app = libreria_di(&app, "radio").map_err(errore)?;
    let coda = con_libreria(&stato_app, |libreria| {
        rymembra_app::autoplay::radio(
            &libreria.connection,
            brano,
            rymembra_app::autoplay::RADIO,
            adesso_ms(),
        )
    })
    .map_err(errore)?;

    let mut brani = Vec::with_capacity(coda.len().saturating_add(1));
    brani.push(brano);
    brani.extend(coda);

    gesto_di_coda(&app, &stato, |lettore| {
        ricorda_coda_di_prima(&app, lettore, Some(&brani));
        chiudi_ascolto(&app, lettore);
        lettore.coda.play_tracks(brani, 0, seme());
        lettore.coda_curata = false;
        Ok(true)
    })
    .map_err(errore)
}

/// Apre il brano corrente della coda e lo fa partire.
///
/// # Perché prende lo stato e non il lettore
///
/// Perché in mezzo **apre un file**, e quella è la sola cosa che non deve
/// avvenire con il lucchetto del lettore in mano: su una condivisione che non
/// risponde l'apertura arriva ai cinque secondi della scadenza, e per tutti e
/// cinque resterebbero fermi anche gli annunci del motore — l'osservatore, su
/// «rymembra-eventi», il lucchetto lo vuole a ogni cambio di traccia — e il filo
/// dell'orologio, che lo chiede quattro volte al secondo.
///
/// Quindi tre tempi: si decide sotto lucchetto, si apre **fuori**, si consegna
/// sotto lucchetto. È la stessa forma di [`riprova_corrente`], che questa
/// strada l'aveva già presa da sola.
///
/// # La finestra fra i due lucchetti
///
/// Fra il primo e il terzo tempo la coda può essere cambiata: qualcuno ha
/// premuto «prossimo» mentre il file si apriva. Si controlla, e se il corrente
/// non è più quello non si suona niente — chi ha cambiato la coda ha già
/// avviato quel che voleva, e sovrascriverlo adesso vorrebbe dire un brano che
/// riparte da solo dopo che se n'è chiesto un altro.
///
/// # Quando il brano non si apre
///
/// Il gesto ha già spostato la coda, e il motore suona ancora quel che c'era:
/// lasciarli così voleva dire il titolo nuovo nel nucleo, il brano vecchio
/// nelle casse e la coda vecchia nella finestra, che non riceveva niente. Vedi
/// [`avvio_fallito`].
///
/// `in_pausa` fa partire il brano fermo: serve a chi toglie dalla coda il brano
/// in pausa, che non ha chiesto di sentire il successivo. `da_ms` è il punto da
/// cui parte, zero quasi sempre: vedi [`riprendi`].
fn avvia_corrente(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    in_pausa: bool,
    da_ms: u64,
) -> Result<(), AppError> {
    match tenta_l_avvio(app, stato, in_pausa, da_ms)? {
        None => Ok(()),
        Some((id, err)) => avvio_fallito(app, stato, id, da_ms, err),
    }
}

/// [`avvia_corrente`] senza la sua risposta a un brano che non si apre: quel
/// brano, col suo errore, torna a chi chiama, che decide cosa farne.
/// [`avvia_corrente`] lo ferma lì; [`riprova_o_salta`] può passare oltre.
fn tenta_l_avvio(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    in_pausa: bool,
    da_ms: u64,
) -> Result<Option<(i64, AppError)>, AppError> {
    // ── 0. un brano che parte ──
    //
    // Il segno di un guasto di prima non parla più di niente: chi ha scelto un
    // brano ha voltato pagina. Restava lì, e [`fermato`] lo scambiava per una
    // rete appena caduta — il segno di «riprendi dov'eri» non tornava a zero a
    // coda finita, e il giorno dopo «Riprendi» saltava a cinque secondi dalla
    // fine di un brano finito da un pezzo.
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    // A dispositivo perso, prima un dispositivo vivo: aprire il brano col
    // formato di un motore morto e consegnarglielo vorrebbe dire un play che non
    // suona. Soltanto il dispositivo — il brano lo apre questa funzione, e
    // rimettere al suo posto quello di prima lo farebbe ripartire per un attimo.
    // Il numero dell'ultima pausa o ripresa chiesta, adesso: una chiesta da
    // qui alla partenza vince sul `in_pausa` di chi ha chiamato. Vedi
    // [`Lettore::pausa_chiesta`].
    let (perso, gesti_prima) = con_lettore(stato, |lettore| {
        Ok((lettore.motore.dispositivo_perso(), lettore.pausa_chiesta.0))
    })?;
    if perso
        && riapertura::riapri_su(app, stato, None, riapertura::Modo::SoloDispositivo)?
            == riapertura::Riaperta::Saltata
    {
        aspetta_la_riapertura(stato);
    }

    // I tre tempi qui sotto, al più due volte. Fra il primo e il terzo il
    // motore può essere stato sostituito — le cuffie tornate mentre il file si
    // apriva — e il brano aperto per il formato di prima, consegnato al motore
    // nuovo, suonerebbe alla velocità o coi canali sbagliati. Si riapre col
    // formato di adesso, una volta: due sostituzioni nei pochi secondi di
    // un'apertura non sono un caso da inseguire.
    for _ in 0..2 {
        // ── 1. chi, e con che formato ──
        let scelta = con_lettore(stato, |lettore| {
            let Some(id) = lettore.coda.current() else {
                lettore.ferma();
                salva_coda(app, lettore);
                manda_stato(app, lettore);
                return Ok(None);
            };
            Ok(Some((id, lettore.motore.formato())))
        })?;
        let Some((id, formato)) = scelta else {
            return Ok(None);
        };

        // ── 2. l'apertura, senza niente in mano ──
        //
        // Una sola per brano: vedi [`StatoLettore::aprendo`].
        let Ok(mut aprendo) = Aprendo::prendi(&stato.aprendo, id) else {
            return Ok(None);
        };
        let brano = match brano_di(app, id, formato) {
            Ok(brano) => brano,
            Err(err) => return Ok(Some((id, err))),
        };

        // ── 3. la partenza ──
        let fatto = con_lettore(stato, |lettore| {
            if lettore.coda.current() != Some(id) {
                return Ok(true);
            }
            if lettore.motore.formato() != formato {
                return Ok(false);
            }
            let da_ms = consegna(lettore, brano, in_pausa, da_ms, gesti_prima);
            // Consegnato: da qui un salto va al motore, non alla puntina. La
            // casella si libera sotto il lucchetto, nello stesso istante in
            // cui la puntina è stata letta — vedi [`vai_a`].
            drop(aprendo.take());
            prepara_prossimo(app, lettore);
            salva_coda(app, lettore);
            // Lo stato parte subito, senza aspettare che il primo campione
            // esca: il titolo nella barra deve comparire al clic, non un decimo
            // di secondo dopo. Con la posizione da cui il brano nuovo comincia:
            // quella del motore è ancora quella del brano di prima, e mandarla
            // vorrebbe dire un titolo nuovo con il cursore a metà.
            manda_stato_con_posizione(app, lettore, da_ms);
            Ok(true)
        })?;
        if fatto {
            return Ok(None);
        }
    }
    Ok(None)
}

/// Quanti brani di fila [`riprova_o_salta`] salta, al più, prima di fermarsi.
///
/// Tre: un album con un file spostato passa oltre, una cartella intera sparita
/// non scorre via tutta la coda in un istante — si ferma al quarto, con
/// l'errore in vista.
const SALTI_DI_FILA: u32 = 3;

/// Il brano non si apre per colpa **sua**: si può passare al successivo.
///
/// Il file non c'è più, il formato non si legge, la riga non esiste: il brano
/// dopo, nella stessa cartella, ha buone probabilità di aprirsi. Non quando a
/// mancare è la sorgente intera — la cartella di rete che non risponde, il
/// servizio cloud scollegato o con l'accesso scaduto, il disco staccato —:
/// lì il brano dopo fallirebbe uguale, e saltare vorrebbe dire una coda che
/// scorre via in silenzio.
///
/// Un file che non si apre arriva come `playback.sourceUnavailable` col suo
/// percorso, mancante o rotto che sia (`playback::sorgente_da_scheda`): la rete
/// e il cloud passano col loro codice, che qui non si salta. Il percorso solo
/// non dice se manca il file o tutto il disco, e allora si guarda la sua
/// cartella: se manca anche lei — una lettera staccata, una condivisione che
/// non si raggiunge — è la sorgente a mancare, e non si salta. Senza percorso
/// è la riga a non esserci più, o la voce di una sessione: si salta.
fn si_salta(err: &AppError) -> bool {
    let cartella_c_e = |percorso: &str| {
        std::path::Path::new(percorso)
            .parent()
            .is_some_and(std::path::Path::exists)
    };
    match err.code() {
        ErrorCode::FsNotFound { path } => cartella_c_e(path),
        ErrorCode::PlaybackSourceUnavailable { path, .. } => {
            path.as_deref().is_none_or(cartella_c_e)
        }
        _ => matches!(
            err.code().kind(),
            ErrorCodeKind::CloudNotFound
                | ErrorCodeKind::PlaybackFormatUnsupported
                | ErrorCodeKind::PlaybackDecodeFailed
                | ErrorCodeKind::LibraryTrackNotFound
                | ErrorCodeKind::FsPermissionDenied
                | ErrorCodeKind::FsPathInvalid
        ),
    }
}

/// Un brano saltato perché non si apriva: l'evento `riproduzione:saltato`.
///
/// Un evento suo e non un errore: un errore del nucleo non porta parametri, e
/// qui la finestra deve dire **quale** brano. Il titolo lo legge chi salta;
/// il motivo è il codice, che la finestra traduce come ogni altro.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Saltato {
    id: i64,
    titolo: Option<String>,
    codice: &'static str,
}

/// Il titolo di un brano per un avviso, senza aspettare troppo la libreria.
///
/// In sessione la coda di chi ospita è fatta di voci, non di righe: il titolo
/// sta nella scheda della voce, come per la barra.
fn titolo_di(app: &tauri::AppHandle, track_id: i64) -> Option<String> {
    let riga = match ospiti::chi(app, track_id) {
        Chi::Libreria(riga) => riga,
        Chi::Voce(voce) => {
            return Some(ospiti::scheda(track_id, &voce).title)
                .filter(|titolo| !titolo.trim().is_empty());
        }
        Chi::Sparita => return None,
    };
    let stato = app.try_state::<Stato>()?;
    con_libreria_entro(&stato, ATTESA_SCHEDA, |libreria| {
        rymembra_app::library::read_summary(&libreria.connection, riga)
    })
    .and_then(Result::ok)
    .flatten()
    .map(|brano| brano.title)
    .filter(|titolo| !titolo.trim().is_empty())
}

/// Il brano dopo non si era aperto e il corrente è finito: si riprova, e se
/// non si apre ancora si passa oltre.
///
/// Prima la coda si fermava lì per sempre, anche se nel frattempo il NAS si
/// era svegliato o la rete era tornata: il preparatore aveva provato una volta,
/// minuti prima, e nessuno riprovava più. Adesso, alla fine del corrente:
///
/// 1. si riapre il brano, ed è la strada di ogni altro avvio — in sessione,
///    chi ospita parte con l'avvio programmato, e gli altri restano a tempo;
/// 2. se non si apre per colpa sua ([`si_salta`]) si passa al successivo, e la
///    finestra lo dice con un avviso che nomina il brano;
/// 3. se non si apre perché manca la sorgente, o dopo [`SALTI_DI_FILA`]
///    salti, ci si ferma su di lui come prima ([`avvio_fallito`]), con
///    l'errore e il suo «Riprova».
///
/// Mai per chi segue una sessione: la sua coda è lo specchio di quella
/// dell'host, e un passo fatto qui sarebbe un passo che l'host non ha fatto.
/// Gira su un filo suo, perché aprire un file può metterci secondi e qui si
/// arriva dall'annuncio del motore, col lucchetto in mano.
fn riprova_o_salta(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return;
    };
    let mut saltati = 0;
    loop {
        let (id, err) = match tenta_l_avvio(app, &stato, false, 0) {
            Ok(None) => return,
            Ok(Some(fallito)) => fallito,
            Err(err) => {
                app.emetti("riproduzione:errore", errore(err));
                return;
            }
        };
        let avanzata = if saltati < SALTI_DI_FILA && si_salta(&err) {
            con_lettore(&stato, |lettore| {
                if lettore.coda.current() != Some(id) || insieme::segue(lettore) {
                    return Ok(None);
                }
                // `true`: anche con «ripeti uno» si va oltre — il brano da
                // ripetere è proprio quello che non si apre.
                let avanti = matches!(lettore.coda.advance(true), Step::Track(_));
                if avanti {
                    salva_coda(app, lettore);
                    manda_stato(app, lettore);
                }
                Ok(Some(avanti))
            })
            .unwrap_or(None)
        } else {
            Some(false)
        };
        match avanzata {
            // Qualcuno ha mosso la coda nel frattempo: ha già avviato lui.
            None => return,
            Some(true) => {
                let codice = err.code().kind().code();
                nota!("[riproduzione] brano saltato: non si apre brano={id} codice={codice}");
                app.emetti(
                    "riproduzione:saltato",
                    Saltato {
                        id,
                        titolo: titolo_di(app, id),
                        codice,
                    },
                );
                saltati += 1;
            }
            // Non si salta, o non c'è un brano dopo: fermo su di lui, come
            // prima. Un errore che non si riprova non ha un comando a cui
            // risalire: va alla fascia da qui.
            Some(false) => {
                if let Err(err) = avvio_fallito(app, &stato, id, 0, err) {
                    app.emetti("riproduzione:errore", errore(err));
                }
                return;
            }
        }
    }
}

/// Dà al motore un brano appena aperto, com'è chiesto **adesso**, e dice da che
/// millisecondo parte davvero.
///
/// Apertura e salto in un comando solo, fermo o suonando. Era `suona` +
/// `vai_a` + `pausa`, e fra il primo e gli altri due la callback poteva far
/// sentire qualche millisecondo dall'inizio del brano — il «tic» di una
/// ripresa a metà. Vedi `Motore::apri_fermo` e `Motore::suona_da`.
///
/// In sessione chi ospita non parte subito ma fra un quarto di secondo, e lo
/// annuncia: è il tempo che serve agli altri per aprire la stessa voce, e così
/// il brano nuovo comincia insieme invece di arrivare agli altri già
/// cominciato. Vedi `insieme::parti`.
///
/// Una pausa o una ripresa chiesta mentre il brano si apriva — `gesti_prima` è
/// il numero di [`Lettore::pausa_chiesta`] letto prima di aprirlo — vince su
/// quel che si era deciso prima: è più recente. Lo stesso per un salto chiesto
/// nel frattempo: vedi `Lettore::puntina_a_vuoto`.
///
/// Una sola funzione per [`avvia_corrente`], per l'«Annulla» della coda, per
/// «Riprova» e per la ripresa dopo il dispositivo ritrovato: fino alla 3.2 le
/// ultime tre facevano ancora la sequenza vecchia, col «tic», ignorando la
/// pausa premuta durante l'apertura e, in sessione, senza l'avvio programmato.
fn consegna(
    lettore: &mut Lettore,
    brano: rymembra_play::BranoAperto,
    in_pausa: bool,
    da_ms: u64,
    gesti_prima: u64,
) -> u64 {
    let id = brano.track_id();
    let in_pausa = if lettore.pausa_chiesta.0 == gesti_prima {
        in_pausa
    } else {
        lettore.pausa_chiesta.1
    };
    let da_ms = match lettore.puntina_a_vuoto {
        Some((quale, ms)) if quale == id => ms,
        _ => da_ms,
    };
    if in_pausa {
        lettore.apri_fermo(brano, da_ms);
    } else {
        insieme::parti(lettore, brano, da_ms);
    }
    da_ms
}

/// Il valore di [`StatoLettore::aprendo`] quando non si sta aprendo niente.
///
/// Zero: gli identificativi dei brani vengono da SQLite, e partono da uno.
const NESSUN_BRANO: i64 = 0;

/// Tiene [`StatoLettore::aprendo`] occupato finché vive, e lo libera comunque
/// finisca l'apertura — riuscita, fallita, o caduta a metà.
struct Aprendo<'a> {
    casella: &'a std::sync::atomic::AtomicI64,
    brano: i64,
}

impl<'a> Aprendo<'a> {
    /// Prende la casella per `brano`.
    ///
    /// `Err(())` se lo **stesso** brano lo sta già aprendo qualcun altro: chi
    /// chiama non ha niente da fare. `Ok(None)` se la casella è di un altro
    /// brano — si apre lo stesso, senza prenderla: il gesto più recente vince,
    /// e l'apertura vecchia troverà la coda già voltata.
    fn prendi(casella: &'a std::sync::atomic::AtomicI64, brano: i64) -> Result<Option<Self>, ()> {
        use std::sync::atomic::Ordering;
        match casella.compare_exchange(NESSUN_BRANO, brano, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Ok(Some(Self { casella, brano })),
            Err(occupata) if occupata == brano => Err(()),
            Err(_) => Ok(None),
        }
    }
}

impl Drop for Aprendo<'_> {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        let _ = self.casella.compare_exchange(
            self.brano,
            NESSUN_BRANO,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Quanto si aspetta una riapertura del dispositivo cominciata da qualcun altro.
///
/// Cinque secondi: un'uscita locale si riapre in decine di millisecondi, una
/// Bluetooth in qualche centinaio. Oltre si va avanti lo stesso, e il peggio che
/// può succedere è un brano consegnato al motore che sta per essere sostituito
/// — quel che succedeva sempre prima che ci fosse questa attesa.
const ATTESA_RIAPERTURA: Duration = Duration::from_secs(5);

/// Aspetta che finisca la riapertura che un altro filo ha in corso.
///
/// La chiama chi deve far partire un brano a dispositivo perso e ha trovato la
/// bandiera già alzata — di solito dal sorvegliante, che ha visto tornare le
/// cuffie nello stesso istante. Il brano va aperto col formato del motore
/// **nuovo** e consegnato a lui, non a quello che sta per essere buttato.
fn aspetta_la_riapertura(stato: &StatoLettore) {
    let fino = std::time::Instant::now() + ATTESA_RIAPERTURA;
    while stato
        .riapertura_in_corso
        .load(std::sync::atomic::Ordering::Acquire)
        && std::time::Instant::now() < fino
    {
        std::thread::sleep(PASSO_ATTESA);
    }
}

/// Il brano che la coda indica non si è aperto: si ferma il motore su di lui.
///
/// Fermo e non «si torna a com'era»: il gesto è già avvenuto — la coda nuova, la
/// coda di prima messa da parte per l'«Annulla», l'ascolto chiuso — e disfarlo a
/// metà vorrebbe dire un avviso «coda sostituita» per una coda che non lo è
/// stata. Così i tre dicono la stessa cosa: la coda è quella chiesta, la barra
/// mostra il brano che non si apre, le casse tacciono, e l'errore che risale a
/// chi ha chiamato dice perché. «Riproduci» riprova quel brano, «prossimo» va
/// oltre.
///
/// Se nel frattempo la coda ha voltato pagina, chi l'ha voltata ha già avviato
/// quel che voleva e non si tocca niente.
///
/// # Il guasto che si può riprovare
///
/// Una cartella di rete che non risponde — il NAS che si sta svegliando, la
/// lettera mappata che Windows non ha ancora ricollegato — non è un errore del
/// gesto: è un'attesa che finisce. Restituito al comando, finiva nella fascia
/// come testo e basta: la fascia mostra «Riprova» solo per i guasti che le
/// arrivano dal lettore con il loro codice, e quelli dei comandi le arrivano
/// già tradotti. Adesso un errore ritentabile su un brano che è ancora quello
/// della coda **segna dove riprendere** e passa per la stessa strada dei
/// guasti a brano avviato (`riproduzione:errore`): la fascia offre «Riprova»,
/// e [`riprova_corrente`] sa da dove ripartire. Il comando risponde che è
/// andato tutto bene, perché quel che c'era da dire l'ha già detto la fascia.
///
/// Ferma, non suonando: chi riprova dopo mezzo minuto non deve trovarsi una
/// musica partita da sola quando il NAS si è svegliato.
pub(super) fn avvio_fallito(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    id: i64,
    da_ms: u64,
    err: AppError,
) -> Result<(), AppError> {
    let ancora_lui = con_lettore(stato, |lettore| {
        if lettore.coda.current() != Some(id) {
            return Ok(false);
        }
        chiudi_ascolto(app, lettore);
        lettore.ferma();
        salva_coda(app, lettore);
        manda_stato_con_posizione(app, lettore, da_ms);
        Ok(true)
    })
    .unwrap_or(false);
    if !(ancora_lui && err.is_retryable()) {
        return Err(err);
    }
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Ripresa {
        track_id: id,
        ms: da_ms,
        suonava: false,
        quando_ms: adesso_ms(),
    });
    app.emetti("riproduzione:errore", errore(err));
    Ok(())
}

/// Mette in pausa.
///
/// In sessione, subito anche per chi ospita: gli altri si fermano quando
/// arriva l'ancora, e i pochi millisecondi di differenza li toglie la ripresa,
/// che invece è programmata. Un avvio programmato e non ancora partito si
/// butta: vedi `insieme::dopo_una_pausa`.
#[tauri::command]
pub fn pausa(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Pausa).map_err(errore)? {
        return Ok(());
    }
    con_lettore(&stato, |lettore| {
        lettore.pausa_chiesta = (lettore.pausa_chiesta.0.wrapping_add(1), true);
        lettore.motore.pausa();
        insieme::dopo_una_pausa(lettore);
        if let Some(a) = lettore.ascolto.as_mut() {
            a.pause(adesso_ms());
        }
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Riprende.
///
/// # Da dove, quando il motore non ha niente in mano
///
/// Dal segno di «riprendi dov'eri», se parla del brano che la coda indica. È il
/// caso della coda rimessa all'avvio: il «play» della barra la faceva partire
/// dall'inizio, e la riga «Riprendi» della Home saltava al segno **dalla
/// finestra**, dopo — con un `vai_a` che arrivava anche quando il brano stava
/// già suonando, e lo riportava indietro al numero di un altro brano. Il punto
/// adesso lo sceglie chi sa quale brano sta aprendo, e tutti e due i tasti
/// fanno la stessa cosa. Prima del segno viene un salto chiesto a motore vuoto:
/// vedi [`Lettore::puntina_a_vuoto`].
///
/// # A dispositivo perso
///
/// Play vuol dire prima di tutto riaprire: al motore perso `riprendi` toglieva
/// la pausa e basta, cioè niente. Si riapre **ripiegando** — chi preme play
/// mentre le cuffie fissate sono via ha chiesto di sentire adesso, da dove si
/// può; quando tornano, il sorvegliante ci riporta la musica — e il brano di
/// prima torna al suo posto **suonando**, qualunque cosa dicesse il segno
/// scritto al momento del guasto. È anche la strada del tasto multimediale,
/// che chiama questa funzione direttamente, senza passare da [`alterna`].
///
/// # In sessione
///
/// Chi ospita non riprende subito ma fra un quarto di secondo, e lo annuncia
/// con l'ancora: così gli altri ripartono nello stesso istante invece di
/// rincorrerlo. Vedi `insieme::riprendi`.
pub fn riprendi_ora(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Riprendi).map_err(errore)? {
        return Ok(());
    }
    let perso =
        con_lettore(&stato, |lettore| Ok(lettore.motore.dispositivo_perso())).map_err(errore)?;
    if perso {
        match riapertura::riapri_su(&app, &stato, None, riapertura::Modo::Suona).map_err(errore)? {
            riapertura::Riaperta::Rimessa => return Ok(()),
            riapertura::Riaperta::Saltata => {
                // Un'altra riapertura è già in corso — il sorvegliante, di
                // solito. Le si dice che adesso si vuole sentire: il segno che
                // rileggerà quando ha finito è questo.
                if let Some(ripresa) = stato
                    .ripresa
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_mut()
                {
                    ripresa.suonava = true;
                    ripresa.quando_ms = adesso_ms();
                }
                return Ok(());
            }
            // Un motore nuovo e niente da rimettere: si prosegue come a motore
            // vuoto, qui sotto.
            riapertura::Riaperta::Vuota => {}
        }
    }

    // In pausa il flusso riposa — dopo un secondo, sul Bluetooth — e chi
    // ospita una sessione sceglie l'istante della ripresa solo a flusso
    // sveglio: meglio svegliarlo qui, a lucchetto libero. Vedi
    // `insieme::regole::programma`.
    sveglia_fuori_dal_lucchetto(&stato);
    // Fuori `None` vuol dire «il motore suonava già, fatto»; dentro è il brano
    // da aprire, che può mancare anche lui — una coda vuota, che
    // `avvia_corrente` sa fermare.
    let da_aprire = con_lettore(&stato, |lettore| {
        lettore.pausa_chiesta = (lettore.pausa_chiesta.0.wrapping_add(1), false);
        if lettore.motore.posizione().track_id.is_none() {
            let corrente = lettore.coda.current();
            // Un salto chiesto a motore vuoto vince sul segno: è più recente, e
            // l'ha chiesto qualcuno. Vedi `Lettore::puntina_a_vuoto`.
            //
            // Poi il punto in cui una rete caduta ha fermato **questo** brano:
            // play invece di «Riprova» è lo stesso gesto, e deve tornare allo
            // stesso millisecondo invece che al segno salvato qualche secondo
            // prima.
            let chiesto = lettore
                .puntina_a_vuoto
                .take()
                .filter(|(id, _)| Some(*id) == corrente)
                .map(|(_, ms)| ms)
                .or_else(|| {
                    stato
                        .ripresa
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .filter(|ripresa| Some(ripresa.track_id) == corrente)
                        .map(|ripresa| ripresa.ms)
                });
            return Ok(Some((corrente, chiesto)));
        }
        insieme::riprendi(lettore);
        if let Some(a) = lettore.ascolto.as_mut() {
            a.resume(adesso_ms());
        }
        manda_stato(&app, lettore);
        Ok(None)
    })
    .map_err(errore)?;
    let Some((corrente, chiesto)) = da_aprire else {
        return Ok(());
    };
    // Il segno salvato è di una riga della libreria: una voce di sessione non
    // ne ha uno, e riparte dall'inizio o dal punto chiesto qui sopra.
    let da_ms = match (chiesto, corrente.filter(|id| ospiti::e_di_libreria(*id))) {
        (Some(ms), _) => ms,
        (None, Some(id)) => libreria_di(&app, "segno di ripresa")
            .and_then(|stato_app| {
                con_libreria(&stato_app, |libreria| {
                    playback::load_posizione(&libreria.connection, id)
                })
            })
            .unwrap_or(0),
        (None, None) => 0,
    };
    avvia_corrente(&app, &stato, false, da_ms).map_err(errore)
}

/// Alterna fra pausa e ripresa.
///
/// «In pausa» è quel che vede chi preme: un dispositivo perso conta come fermo,
/// come in [`abbozzo`]. Il motore da solo diceva «non in pausa» con le cuffie
/// staccate — nessuno aveva premuto pausa — e il tasto multimediale, sotto
/// un'icona che mostrava «riproduci», metteva in pausa: il primo tocco non
/// faceva niente, il secondo faceva il contrario di quel che diceva.
pub fn alterna_ora(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Alterna).map_err(errore)? {
        return Ok(());
    }
    let in_pausa = con_lettore(&stato, |lettore| {
        Ok(lettore.motore.posizione().in_pausa || lettore.motore.dispositivo_perso())
    })
    .map_err(errore)?;
    if in_pausa {
        riprendi_ora(app, stato)
    } else {
        pausa(app, stato)
    }
}

/// [`riprendi_ora`], sul pool bloccante: vedi [`col_lettore_in_disparte`].
#[tauri::command]
pub async fn riprendi(app: tauri::AppHandle) -> Esito<()> {
    col_lettore_in_disparte(app, "riprendi", riprendi_ora).await
}

/// [`alterna_ora`], sul pool bloccante: vedi [`col_lettore_in_disparte`].
#[tauri::command]
pub async fn alterna(app: tauri::AppHandle) -> Esito<()> {
    col_lettore_in_disparte(app, "alterna", alterna_ora).await
}

/// [`prossimo_ora`], sul pool bloccante: vedi [`col_lettore_in_disparte`].
#[tauri::command]
pub async fn prossimo(app: tauri::AppHandle) -> Esito<()> {
    col_lettore_in_disparte(app, "prossimo", prossimo_ora).await
}

/// [`precedente_ora`], sul pool bloccante: vedi [`col_lettore_in_disparte`].
#[tauri::command]
pub async fn precedente(app: tauri::AppHandle) -> Esito<()> {
    col_lettore_in_disparte(app, "precedente", precedente_ora).await
}

/// Passa al brano dopo.
pub fn prossimo_ora(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Prossimo).map_err(errore)? {
        return Ok(());
    }
    gesto_di_coda(&app, &stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        match lettore.coda.advance(true) {
            Step::Track(_) => Ok(true),
            Step::Restart => {
                insieme::salta(lettore, 0);
                manda_stato(&app, lettore);
                Ok(false)
            }
            Step::Stop => {
                lettore.ferma();
                manda_stato(&app, lettore);
                Ok(false)
            }
        }
    })
    .map_err(errore)
}

/// Torna al brano prima, o ricomincia questo.
pub fn precedente_ora(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Precedente).map_err(errore)? {
        return Ok(());
    }
    gesto_di_coda(&app, &stato, |lettore| {
        let posizione = lettore.motore.posizione();
        let corrente = lettore.coda.current();
        let si_apre = stato.aprendo.load(std::sync::atomic::Ordering::Acquire);
        let dove = dove_per_il_precedente(posizione.track_id, corrente, si_apre, posizione.ms);
        match lettore.coda.previous(dove) {
            Step::Track(_) => {
                chiudi_ascolto(&app, lettore);
                Ok(true)
            }
            // Ricominciare lo stesso brano non chiude l'ascolto: è ancora quello.
            Step::Restart => {
                insieme::salta(lettore, 0);
                manda_stato(&app, lettore);
                Ok(false)
            }
            Step::Stop => Ok(false),
        }
    })
    .map_err(errore)
}

/// Da quale punto del brano corrente [`precedente`] misura i suoi tre secondi.
///
/// Dal motore solo se il motore sta già suonando il brano che la coda indica,
/// e non lo si sta ancora aprendo. Subito dopo un cambio di brano — il NAS che
/// si sveglia, un servizio cloud, o il centinaio di millisecondi prima che il
/// motore prenda il brano consegnato — il motore dice ancora il punto del
/// brano **di prima**: oltre i tre secondi la coda rispondeva «ricomincia», si
/// riavvolgeva il brano vecchio, e poi partiva quello nuovo. «Precedente» non
/// tornava indietro. Il brano nuovo, da chi ascolta, non è ancora cominciato:
/// vale zero. È la stessa domanda che [`vai_a`] fa prima di saltare.
const fn dove_per_il_precedente(
    nel_motore: Option<i64>,
    corrente: Option<i64>,
    si_apre: i64,
    ms: u64,
) -> u64 {
    match (nel_motore, corrente) {
        (Some(motore), Some(coda)) if motore == coda && si_apre != coda => ms,
        _ => 0,
    }
}

/// Il campanello del motore di adesso, preso e lasciato col lucchetto, suonato
/// **fuori**.
///
/// # Perché
///
/// Perché i comandi del motore che svuotano l'anello — il salto, lo stop —
/// svegliano prima il flusso, e un flusso a riposo ci mette fino a mezzo
/// secondo a rispondere: lo dice `rymembra_play::uscita::Uscita::sveglia`. Due
/// di quei comandi, [`vai_a`] e `coda::coda_svuota`, girano sul filo della
/// finestra e chiedevano il risveglio **con il lucchetto del lettore in mano**:
/// per quel mezzo secondo restavano in fila gli annunci del motore, l'orologio,
/// il preparatore. Adesso il risveglio si paga qui, a lucchetto lasciato, e il
/// comando vero lo trova già fatto.
///
/// L'ordine dei comandi non cambia: tutti e due restano sincroni, sul filo
/// della finestra, e il secondo non parte prima che il primo abbia finito. Il
/// filo della finestra aspetta lo stesso il risveglio — è lui a chiederlo — ma
/// adesso aspetta da solo.
///
/// Se nel frattempo il motore è stato sostituito il campanello suona a vuoto,
/// e il comando sveglierà il motore nuovo da sé: è un caso raro, ed è il
/// comportamento di prima.
///
/// # Perché il campanello si prende con un'attesa corta
///
/// Perché il comando, subito dopo, il lucchetto lo chiede di nuovo, e sul filo
/// della finestra quella seconda presa può aspettare fino ad
/// [`ATTESA_DELLA_FINESTRA`]. Con due attese piene di fila la finestra poteva
/// restare ferma più di quattro secondi, a un passo dai cinque oltre i quali
/// Windows la dichiara morta. Qui si aspetta al più [`ATTESA_DEL_CAMPANELLO`]:
/// se il lucchetto è occupato il risveglio anticipato si salta, e il comando fa
/// quel che faceva prima — si sveglia da sé, col lucchetto in mano. Per lo
/// stesso motivo non c'è errore da restituire: un lettore che non c'è lo dirà
/// il comando.
fn sveglia_fuori_dal_lucchetto(stato: &StatoLettore) {
    let fino = std::time::Instant::now() + ATTESA_DEL_CAMPANELLO;
    let campanello = loop {
        match stato.lettore.try_lock() {
            Ok(guardia) => break guardia.as_ref().ok().map(|l| l.motore.campanello()),
            Err(std::sync::TryLockError::Poisoned(veleno)) => {
                break veleno
                    .into_inner()
                    .as_ref()
                    .ok()
                    .map(|l| l.motore.campanello());
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= fino {
                    break None;
                }
                std::thread::sleep(PASSO_ATTESA);
            }
        }
    };
    if let Some(campanello) = campanello {
        campanello.suona();
    }
}

/// Quanto [`sveglia_fuori_dal_lucchetto`] aspetta il lucchetto del lettore
/// prima di rinunciare al risveglio anticipato.
///
/// Cento millisecondi: chi tiene il lucchetto lo tiene di solito per
/// microsecondi, e oltre vuol dire che qualcuno lo sta tenendo a lungo — e
/// allora il comando, che il lucchetto lo vuole comunque, aspetterà lui.
const ATTESA_DEL_CAMPANELLO: Duration = Duration::from_millis(100);

/// Salta a un punto del brano.
///
/// In sessione, per chi ospita mentre suona, il salto è programmato: pausa,
/// salto e ripresa fra un quarto di secondo, annunciata. Vedi
/// `insieme::salta`.
#[tauri::command]
pub fn vai_a(app: tauri::AppHandle, stato: State<'_, StatoLettore>, ms: u64) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::VaiA { ms }).map_err(errore)? {
        return Ok(());
    }
    sveglia_fuori_dal_lucchetto(&stato);
    con_lettore(&stato, |lettore| {
        // A motore vuoto, o col brano della coda che si sta ancora aprendo (il
        // titolo è già quello nuovo, il motore ha ancora in mano il vecchio),
        // il salto si tiene da parte per chi aprirà il brano: vedi
        // `Lettore::puntina_a_vuoto`. Fatto al motore saltava il brano vecchio,
        // e poi l'apertura finiva e quello nuovo partiva dall'inizio — in
        // sessione, sei salti su dieci subito dopo una voce scelta dalla coda.
        //
        // «Si sta aprendo» lo dice la casella di [`avvia_corrente`], non il
        // motore: un brano appena consegnato il motore lo prende sul suo filo,
        // e per qualche millisecondo dice ancora il brano di prima — un salto
        // tenuto da parte lì non l'avrebbe raccolto più nessuno. Il motore
        // invece fa i comandi nell'ordine in cui arrivano.
        let nel_motore = lettore.motore.posizione().track_id;
        let corrente = lettore.coda.current();
        let si_apre = corrente
            .is_some_and(|id| stato.aprendo.load(std::sync::atomic::Ordering::Acquire) == id);
        if nel_motore.is_none() || si_apre {
            lettore.puntina_a_vuoto = corrente.map(|id| (id, ms));
        } else {
            insieme::salta(lettore, ms);
        }
        // Con i millisecondi richiesti: il motore li raggiunge sul suo filo, e
        // fino ad allora `posizione()` dice ancora il punto da cui si è
        // partiti. Vedi [`manda_stato_con_posizione`].
        manda_stato_con_posizione(&app, lettore, ms);
        Ok(())
    })
    .map_err(errore)
}

/// Lo stato corrente, per quando la finestra si apre.
///
/// **Non fallisce quando il motore non si è aperto**, ed è l'unico comando di
/// questo modulo a comportarsi così. Gli altri sì, e devono: chiedere di
/// suonare a un lettore che non c'è è una richiesta senza risposta. Questo
/// invece è la domanda che la finestra fa per prima, e rispondere con un errore
/// vorrebbe dire una finestra che si apre su un guasto invece che su una
/// libreria — mentre senza scheda audio Rymembra resta un catalogo consultabile,
/// che è quello che `StatoLettore` conserva l'errore per permettere.
///
/// Lo stato che torna è quello di un lettore fermo, con dentro il motivo: è la
/// stessa forma che la finestra riceve quando il dispositivo sparisce dopo, ed
/// è ciò che le permette di disegnare una fascia sola per i due casi.
///
/// Fuori dal filo della finestra: è la prima domanda della finestra, arriva
/// una volta, e l'ordine rispetto agli altri comandi non conta. Sul filo della
/// finestra dovrebbe rinunciare dopo due secondi se il lettore è occupato — e
/// una rinuncia qui vorrebbe dire una finestra che si apre dichiarando il
/// lettore indisponibile. Sul pool bloccante, con [`col_lettore_in_disparte`],
/// può aspettare quanto serve — la scheda si legge con la regola di sempre di
/// `con_libreria`, senza limite — senza tenere fermo un worker del runtime
/// che servirebbe agli altri comandi. Il lucchetto del lettore si tiene per la
/// sola fotografia; la scheda del brano si legge dopo, a lucchetto lasciato.
#[tauri::command]
pub async fn riproduzione_stato(app: tauri::AppHandle) -> Esito<StatoRiproduzione> {
    col_lettore_in_disparte(app, "stato del lettore", riproduzione_stato_ora).await
}

/// Il lavoro di [`riproduzione_stato`], sincrono.
fn riproduzione_stato_ora(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
) -> Esito<StatoRiproduzione> {
    let fotografia = {
        let guardia = prendi_il_lettore(&stato).map_err(errore)?;
        match guardia.as_ref() {
            Ok(lettore) => Ok(abbozzo(&app, lettore)),
            Err(err) => Err(err.clone()),
        }
    };
    match fotografia {
        Ok(abbozzo) => Ok(completa(&app, abbozzo, None)),
        Err(err) => Ok(StatoRiproduzione {
            brano: None,
            in_pausa: true,
            posizione_ms: 0,
            durata_ms: 0,
            shuffle: false,
            ripeti: nome_ripetizione(RepeatMode::Off).to_owned(),
            volume: Volume::default().volume,
            muto: false,
            coda: Vec::new(),
            posizione_coda: None,
            eq_attivo: false,
            eq_guadagni: Vec::new(),
            replaygain: nome_normalizzazione(Normalizzazione::default()).to_owned(),
            spegnimento_ms: None,
            autoplay: false,
            dissolvenza_s: 0,
            latenza_ms: 0,
            anticipo_ms: rymembra_domain::testo::ANTICIPO_MS,
            motivo_prossimo: None,
            audio: Some(guasto_di_apertura(&err)),
            // Nessuna uscita: non se n'è mai aperta una.
            uscita: None,
            // E nessun formato: senza motore non c'è nessun brano, e senza
            // brano non c'è nessun file di cui dire com'è fatto.
            formato: None,
            insieme: None,
        }),
    }
}

#[cfg(test)]
mod prove;
