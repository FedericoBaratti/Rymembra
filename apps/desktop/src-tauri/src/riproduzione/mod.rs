//! La riproduzione, dal lato della finestra.
//!
//! Tiene insieme tre cose che vivono in tre posti diversi: la **coda**
//! (`aether_domain::queue`, pura), il **motore** (`aether-play`, che non sa
//! cosa sia una libreria) e il **database** (`aether_app::playback`). Nessuno
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
//! `aether_domain::listen::counts_as_play` — è del dominio, e la scrittura è di
//! `aether-app`. Qui c'è solo il filo che le mette in fila.
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
//! - [`coda`]: la coda e i gesti che la cambiano.
//!
//! Qui restano i tipi che attraversano tutto, i due lucchetti che li tengono,
//! l'apertura di un brano e i comandi che fanno partire e fermare la musica.

mod coda;
mod fili;
mod flusso;
mod riapertura;
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

use std::sync::Mutex;
use std::time::Duration;

use crate::spegnimento::Emette as _;
use aether_app::library::TrackSummary;
use aether_app::playback::{self, Equalizzazione, Normalizzazione, Qualita, Volume};
use aether_catalogo::Cataloghi;
use aether_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use aether_domain::listen::ListenTracker;
use aether_domain::queue::{Queue, RepeatMode, Step};
use aether_play::{Evento, Motore};
use serde::Serialize;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria};

use self::coda::{gesto_di_coda, salva_coda};
use self::fili::{prepara_prossimo, quanto_manca};
use self::flusso::{apri_flusso, riferimento_di, sorgente_da_flusso};
use self::suono::nome_normalizzazione;

/// Ogni quanto la finestra riceve la posizione.
///
/// Quattro volte al secondo, non sessanta. Il cursore si muove liscio lo stesso
/// perché la finestra interpola fra un colpo e l'altro; sessanta eventi al
/// secondo attraverso l'IPC sarebbero sessanta serializzazioni JSON al secondo
/// per spostare un pixel, e si vedrebbero nel consumo della batteria molto prima
/// che nella fluidità.
const PASSO_TEMPO: Duration = Duration::from_millis(250);

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
/// sposteranno: sono `aether_play::spettro::FONDO_DB` e il fondo scala, cioè la
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BandeIpc {
    /// Le fini, per la scena: quante ne ha chieste chi guarda.
    fini: Vec<u8>,
}

impl BandeIpc {
    /// Quantizza quel che il lettore ha appena misurato.
    fn da(bande: &aether_play::Bande) -> Self {
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
    /// l'interfaccia in inglese. Vedi [`aether_app::autoplay::Motivo::codice`].
    ///
    /// `None` quando il brano dopo l'hai messo tu, e allora non c'è niente da
    /// spiegare.
    pub motivo_prossimo: Option<String>,
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
    /// Il canale con cui si sveglia il filo che apre il brano successivo.
    ///
    /// # Perché una spinta e non una chiamata
    ///
    /// Perché chi la manda, la metà delle volte, è il filo della decodifica:
    /// l'osservatore degli eventi gira lì sopra, e a ogni cambio di traccia
    /// chiede di preparare il successivo. Aprire un file su quel filo vuol dire
    /// smettere di riempire l'anello per tutta la durata dell'apertura — cioè
    /// silenzio, appena la riserva si esaurisce. Vedi [`prepara_prossimo`].
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
    /// Su quale uscita si vuole sentire Aether. `None`: quella di sistema.
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
}

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
}

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
fn apri_motore(app: &tauri::AppHandle, voluto: Option<String>) -> Result<Motore, AppError> {
    let manico = app.clone();
    aether_play::avvia(move |evento| su_evento(&manico, evento), voluto)
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
            motivo_prossimo: None,
        });
        let (prepara, orecchio) = std::sync::mpsc::channel();
        let (sveglia_dispositivi, orecchio_dispositivi) = std::sync::mpsc::channel();
        (
            Self {
                lettore: Mutex::new(lettore),
                spettro: std::sync::atomic::AtomicBool::new(false),
                spettro_bande: std::sync::atomic::AtomicU16::new(aether_play::RISOLUZIONE_DI_SERIE),
                spegnimento: std::sync::atomic::AtomicI64::new(0),
                ripresa: Mutex::new(None),
                prepara,
                volume_da_salvare: std::sync::atomic::AtomicBool::new(false),
                eq_da_salvare: std::sync::atomic::AtomicBool::new(false),
                cataloghi: Cataloghi::nuovi(),
                uscita_voluta: Mutex::new(voluta),
                riapertura_in_corso: std::sync::atomic::AtomicBool::new(false),
                sveglia_dispositivi,
            },
            orecchio,
            orecchio_dispositivi,
        )
    }
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
    let stato = app.state::<Stato>();
    con_libreria(&stato, |libreria| {
        aether_app::playback::load_uscita(&libreria.connection)
    })
    .ok()
    .flatten()
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
            nota!(
                "[avvio] audio aperto uscita={} frequenza={} canali={}",
                lettore.motore.dispositivo(),
                f.frequenza,
                f.canali
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
fn con_lettore<T>(
    stato: &StatoLettore,
    azione: impl FnOnce(&mut Lettore) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_mut() {
        Ok(lettore) => azione(lettore),
        Err(err) => Err(err.clone()),
    }
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
/// Prende il lucchetto della libreria: va chiamata quando quello del lettore è
/// già in mano, mai al contrario.
fn chiudi_ascolto(app: &tauri::AppHandle, lettore: &mut Lettore) {
    let Some(tracker) = lettore.ascolto.take() else {
        return;
    };
    let ascolto = tracker.finish(adesso_ms());
    if !ascolto.counts {
        return;
    }
    let stato = app.state::<Stato>();
    let scritto = con_libreria(&stato, |libreria| {
        // L'ascolto si intesta a questo computer. Non è contabilità: il conteggio
        // è la somma dei contatori di tutti i dispositivi, e un ascolto senza
        // mittente non saprebbe in quale sommarsi.
        let dispositivo = crate::nuvola::dispositivo(&libreria.connection)?;
        playback::record_play(&mut libreria.connection, &ascolto, &dispositivo)
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
            crate::scrobble::dopo_un_ascolto(app, &ascolto);
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

/// Il nome che il filo dell'apertura porta nel diario dei panici.
const APERTURA_BRANO_NOME: &str = "apertura-brano";

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
/// `aether_app::playback` è già spaccato apposta nelle due metà. Qui la prima
/// gira sotto lucchetto, il lucchetto si lascia, e la seconda va su un filo suo
/// con una scadenza addosso.
///
/// # Quel che resta preso
///
/// Il lucchetto del **lettore**, perché chi chiama sta dentro `con_lettore` e
/// deve sapere quale brano aprire. Cinque secondi e non quaranta, quindi, ed è
/// la differenza fra un tasto che risponde tardi e una finestra che il sistema
/// dichiara morta.
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
/// non torna è la stessa attesa di una share che non risponde — cinque secondi
/// dopo, chi ha premuto play deve avere una risposta. Il ramo lo sceglie
/// [`riferimento_di`], che è l'unico posto che sa distinguere le due cose.
///
/// La rete, dentro la chiusura, si può aspettare: qui non c'è più nessun
/// lucchetto in mano, ed è la stessa proprietà per cui l'apertura di un file era
/// stata spostata qui. Vedi il preambolo del modulo.
///
/// # Errori
///
/// `fs.networkUnavailable` quando la scadenza passa — ed è il codice giusto e
/// non un ripiego: un'apertura che non finisce in cinque secondi su un percorso
/// che il database conosce è una share che non risponde, ed è ritentabile. Vale
/// identico per un flusso: un catalogo che non consegna 256 kilobyte in cinque
/// secondi è, dal punto di vista di chi ascolta, la stessa cosa — e la stessa
/// cosa deve avere lo stesso tasto «Riprova» accanto.
/// `playback.formatUnsupported` per un contenitore che non si riconosce, e i
/// `net.*` di [`apri_flusso`] quando la rete risponde prima della scadenza per
/// dire di no.
fn brano_di(
    app: &tauri::AppHandle,
    track_id: i64,
    formato: aether_play::FormatoUscita,
) -> Result<aether_play::BranoAperto, AppError> {
    let stato = app.state::<Stato>();
    let scheda = con_libreria(&stato, |libreria| {
        playback::scheda_sorgente(&libreria.connection, track_id)
    })?;
    // Il riferimento si ricava **prima** della chiusura, che si porta via la
    // scheda: da qui in giù serve solo per sapere che faccia dare a una scadenza
    // passata, e chiederlo di nuovo dopo non si potrebbe.
    let remoto = riferimento_di(&scheda);
    // Il percorso si copia prima per la stessa ragione: senza di lui l'errore
    // direbbe «la rete non risponde» senza dire quale cartella andare a
    // ricollegare.
    let percorso = scheda.path.clone();
    // I cataloghi si clonano fuori dallo stato: un `State` non attraversa il
    // confine di un filo, e il clone è quasi gratis — l'agente sta in un `Arc` e
    // la riserva di connessioni resta la stessa (vedi [`StatoLettore::cataloghi`]).
    let cataloghi = app.state::<StatoLettore>().cataloghi.clone();
    let riferimento = remoto.clone();
    aether_app::scadenza::con_scadenza(APERTURA_BRANO_NOME, APERTURA_BRANO, move || {
        let sorgente = match riferimento {
            Some(rif) => {
                let flusso = apri_flusso(&cataloghi, &rif)?;
                sorgente_da_flusso(&scheda, &rif.url, flusso)
            }
            None => playback::sorgente_da_scheda(&aether_app::files::LocalFiles, &scheda)?,
        };
        aether_play::BranoAperto::apri(sorgente, formato)
    })
    .unwrap_or_else(|| {
        Err(AppError::new(ErrorCode::FsNetworkUnavailable {
            // Per un flusso il campo resta vuoto, e non per pudore: la finestra
            // lo mostra per dire *quale cartella* andare a ricollegare, e una
            // cartella non c'è. Un indirizzo al posto di un percorso sarebbe un
            // consiglio che non si può seguire.
            path: remoto.is_none().then_some(percorso),
        })
        .with_cause(format!(
            "l'apertura non è finita entro {} secondi",
            APERTURA_BRANO.as_secs()
        )))
    })
}

/// Compone lo stato e lo manda alla finestra — e al sistema operativo.
///
/// I due destinatari sono qui insieme di proposito. La scheda nel riquadro del
/// volume di Windows dice le stesse cose della barra in fondo alla finestra, e
/// aggiornarla da un posto suo vorrebbe dire due sorgenti per la stessa
/// verità: prima o poi una delle due mostrerebbe il brano di prima, e sarebbe
/// il difetto che nessuno segnala perché chi lo vede pensa di aver letto male.
pub(crate) fn manda_stato(app: &tauri::AppHandle, lettore: &Lettore) {
    let stato = costruisci_stato(app, lettore);
    crate::media::aggiorna(app, &stato);
    app.emetti("riproduzione:stato", stato);
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
    let mut stato = costruisci_stato(app, lettore);
    stato.posizione_ms = ms;
    crate::media::aggiorna(app, &stato);
    app.emetti("riproduzione:stato", stato);
}

fn costruisci_stato(app: &tauri::AppHandle, lettore: &Lettore) -> StatoRiproduzione {
    let posizione = lettore.motore.posizione();
    let brano = lettore.coda.current().and_then(|id| {
        let stato = app.state::<Stato>();
        con_libreria(&stato, |libreria| {
            aether_app::library::read_summary(&libreria.connection, id)
        })
        .ok()
        .flatten()
    });
    // Un dispositivo perso è fermo, comunque la pensi il motore.
    //
    // `Condiviso::in_pausa` lo alzano solo pausa, stop e fine del brano: la
    // callback di `cpal` che segnala il guasto non lo tocca, e non deve — gira
    // dentro il backend audio. Il risultato era uno stato che diceva
    // `inPausa: false` con le cuffie staccate, e la finestra ci si ancorava: il
    // cursore continuava a interpolare da solo, oltre la fine del brano, mentre
    // il pulsante accanto mostrava «riprendi». Cioè esattamente la bugia che
    // `GuastoAudio` è stato aggiunto per togliere.
    //
    // Si corregge qui e non nella finestra perché di stato ce n'è uno: le
    // schermate che leggono `inPausa` sono tre, e ognuna che se lo aggiusti per
    // conto suo è un'occasione di non farlo.
    let guasto = guasto_di(&lettore.motore);
    StatoRiproduzione {
        durata_ms: brano.as_ref().map_or(posizione.durata_ms, |b| {
            u64::try_from(b.duration_ms).unwrap_or(0)
        }),
        brano,
        in_pausa: posizione.in_pausa || guasto.is_some(),
        posizione_ms: posizione.ms,
        shuffle: lettore.coda.shuffle(),
        ripeti: nome_ripetizione(lettore.coda.repeat()).to_owned(),
        volume: lettore.volume.volume,
        muto: lettore.volume.muto,
        coda: lettore.coda.in_play_order(),
        posizione_coda: lettore.coda.position(),
        eq_attivo: lettore.eq.attivo,
        eq_guadagni: lettore.eq.guadagni.clone(),
        replaygain: nome_normalizzazione(lettore.normalizzazione).to_owned(),
        // Solo una lettura atomica: `state` restituisce il registro, non
        // prende il lucchetto — che del resto è già in mano a chi ci ha
        // chiamati.
        spegnimento_ms: quanto_manca(&app.state::<StatoLettore>()),
        autoplay: lettore.autoplay,
        dissolvenza_s: lettore.dissolvenza_s,
        // Il motivo vale per il brano che **verrà**, non per uno qualunque: se
        // nel frattempo la coda è cambiata, la frase parlerebbe di un brano che
        // non c'è più. Confrontare costa un `Option<i64>` e toglie l'unico modo
        // in cui questa riga poteva mentire.
        motivo_prossimo: lettore
            .motivo_prossimo
            .filter(|(id, _)| Some(*id) == lettore.coda.peek_next())
            .map(|(_, codice)| codice.to_owned()),
        audio: guasto,
        uscita: Some(lettore.motore.dispositivo().to_owned()),
    }
}

/// Il guasto del motore, se ce n'è uno.
///
/// # Perché il codice non è sempre quello del dispositivo perso
///
/// Perché `aether_play` alza una bandiera sola per due guasti diversi — la
/// ragione sta in `Condiviso::perso`, e in breve è che la cura è la stessa: un
/// motore nuovo. Al banner però la differenza serve, perché le due frasi
/// mandano l'utente in due posti opposti: `playback.deviceLost` dice che
/// l'audio se n'è andato, cioè «guarda i cavi», e sarebbe il consiglio
/// sbagliato quando il dispositivo è al suo posto e a essersi interrotto è il
/// decodificatore. `playback.stalled` dice invece proprio quello, ed è già nel
/// catalogo e già tradotto.
///
/// La causa in chiaro resta quella del motore in tutti e due i casi: è la
/// stessa tabella, letta due volte, che dice «dispositivo non più disponibile»
/// o «il decodificatore si è interrotto».
fn guasto_di(motore: &Motore) -> Option<GuastoAudio> {
    motore.causa_perdita().map(|causa| GuastoAudio {
        codice: if motore.filo_caduto() {
            ErrorCodeKind::PlaybackStalled
        } else {
            ErrorCodeKind::PlaybackDeviceLost
        }
        .code()
        .to_owned(),
        causa: causa.to_owned(),
        // Lo stesso bit letto una seconda volta, non una seconda verità: i
        // rami di `codice_perdita` sono quelli di `causa_perdita`, detti in
        // una parola che si può cercare in un dizionario invece che in una
        // frase che si può solo stampare.
        causa_codice: motore.codice_perdita().map(str::to_owned),
        riapribile: true,
    })
}

/// Il guasto di un motore che non si è proprio aperto.
fn guasto_di_apertura(err: &AppError) -> GuastoAudio {
    GuastoAudio {
        codice: err.code().kind().code().to_owned(),
        causa: err
            .cause()
            .unwrap_or("il motore audio non si è aperto")
            .to_owned(),
        // Niente codice della causa: qui la causa **è** un `AppError`, e il
        // suo codice sta già nel campo qui sopra. La finestra lo traduce da
        // lì, come traduce ogni altro errore del nucleo.
        causa_codice: None,
        riapribile: true,
    }
}

const fn nome_ripetizione(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "off",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    }
}

/// Quel che arriva dal motore.
///
/// Gira su un filo del motore, mai su quello della callback audio: qui si può
/// prendere un lucchetto e scrivere sul database senza rischiare di spezzare il
/// suono.
fn su_evento(app: &tauri::AppHandle, evento: Evento) {
    let stato_lettore = app.state::<StatoLettore>();
    match evento {
        Evento::Iniziato { track_id } => {
            let _ = con_lettore(&stato_lettore, |lettore| {
                // Il brano precedente è finito **adesso**, non prima: con il
                // gapless fra i due non c'è un istante di silenzio, e questo è
                // l'unico momento in cui si sa che è successo.
                chiudi_ascolto(app, lettore);

                // La coda può essere indietro di un passo: il motore ha
                // attaccato il brano che gli era stato preparato.
                if lettore.coda.current() != Some(track_id) {
                    lettore.coda.advance(false);
                }

                let durata = lettore
                    .motore
                    .posizione()
                    .durata_ms
                    .max(durata_di(app, track_id));
                lettore.ascolto = Some(ListenTracker::begin(track_id, durata, adesso_ms()));

                // Il «sta ascoltando» verso i servizi di scrobbling. Parte su un
                // filo suo: qui il lucchetto del lettore è in mano, e una
                // richiesta HTTP dentro questa chiusura terrebbe fermo il brano
                // successivo per il tempo di una risposta da Last.fm.
                crate::scrobble::sta_suonando(app, track_id);

                prepara_prossimo(app, lettore);
                salva_coda(app, lettore);
                manda_stato(app, lettore);
                Ok(())
            });
        }
        Evento::Fermato => {
            let _ = con_lettore(&stato_lettore, |lettore| {
                chiudi_ascolto(app, lettore);
                manda_stato(app, lettore);
                Ok(())
            });
        }
        Evento::Errore(err) => {
            nota!(
                "[riproduzione] {} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            // Un guasto di rete non è la fine del brano: è una pausa imposta da
            // un cavo. Il motore si è fermato invece di scorrere la coda (vedi
            // il commento in `aether_play::motore::decodifica_un_blocco`), e
            // qui si annota **dove**, finché quel dove esiste ancora: la
            // fermata azzera la posizione subito dopo, e questa chiusura gira
            // sul filo della decodifica un istante prima che succeda.
            //
            // Vale identico per un flusso, e senza una riga in più: una
            // connessione che cade a metà canzone arriva fin qui con lo stesso
            // codice, perché `aether_net::flusso` la traduce in un numero di
            // sistema che `aether_domain::errors::rete` riconosce. La strada del
            // «Riprova» è quindi una sola per tutti e due i casi — che è il
            // punto: un brano interrotto dalla rete non deve raccontare due
            // storie diverse a seconda di dove stavano i byte.
            if err.code().kind() == ErrorCodeKind::FsNetworkUnavailable {
                annota_ripresa(&stato_lettore);
            }
            app.emetti("riproduzione:errore", crate::errore::errore(*err));
        }
    }
}

/// Si segna brano e millisecondo, perché «Riprova» sappia dove tornare.
///
/// Il lucchetto del lettore si prende e si lascia subito: è una lettura di due
/// interi da una casella condivisa, non un'apertura di file. Se il lettore non
/// c'è — nessuna scheda audio — non c'è nemmeno un punto a cui tornare, e
/// tacere è la cosa giusta: questa strada non deve far cadere niente, come
/// tutte quelle che partono da un evento.
fn annota_ripresa(stato: &State<'_, StatoLettore>) {
    let Ok(posizione) = con_lettore(stato, |lettore| Ok(lettore.motore.posizione())) else {
        return;
    };
    let Some(track_id) = posizione.track_id else {
        return;
    };
    let mut guardia = stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guardia = Some(Ripresa {
        track_id,
        ms: posizione.ms,
        // Letto qui e non alla riapertura: fra i due istanti c'è un motore
        // nuovo, che di quel che il vecchio stava facendo non sa niente. È
        // questo bit a decidere se il suono ripartirà da solo.
        suonava: !posizione.in_pausa,
    });
}

/// La durata dichiarata dal database.
fn durata_di(app: &tauri::AppHandle, track_id: i64) -> u64 {
    let stato = app.state::<Stato>();
    con_libreria(&stato, |libreria| {
        aether_app::library::read_summary(&libreria.connection, track_id)
    })
    .ok()
    .flatten()
    .map_or(0, |b| u64::try_from(b.duration_ms).unwrap_or(0))
}

/// Accende o spegne la coda che non finisce.
///
/// Cambia anche quel che il motore ha già in canna: se si accende mentre
/// l'ultimo brano sta suonando, il successivo va scelto **adesso**, non al
/// prossimo cambio di traccia — che non ci sarebbe.
#[tauri::command(async)]
pub fn autoplay(app: tauri::AppHandle, stato: State<'_, StatoLettore>, attivo: bool) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        let stato_app = app.state::<Stato>();
        // Si rilegge quel che è stato scritto, come per la normalizzazione: se
        // il salvataggio fallisce, l'interruttore non deve restare acceso su
        // una scelta che non sopravvivrà alla chiusura.
        let salvato = con_libreria(&stato_app, |libreria| {
            playback::save_autoplay(&libreria.connection, attivo)?;
            playback::load_autoplay(&libreria.connection)
        })
        .unwrap_or(attivo);
        lettore.autoplay = salvato;
        prepara_prossimo(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
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
#[tauri::command]
pub fn dissolvenza(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    secondi: u64,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        let stato_app = app.state::<Stato>();
        // Riletta invece che ripetuta, come per l'autoplay: `save_crossfade`
        // taglia al massimo, e senza rileggere il cursore resterebbe su un
        // valore che il database ha rifiutato.
        let salvato = con_libreria(&stato_app, |libreria| {
            playback::save_crossfade(&libreria.connection, secondi)?;
            playback::load_crossfade(&libreria.connection)
        })
        .unwrap_or(secondi.min(playback::CROSSFADE_MASSIMO_S));
        lettore.motore.dissolvenza(salvato.saturating_mul(1000));
        lettore.dissolvenza_s = salvato;
        manda_stato(&app, lettore);
        Ok(())
    })
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
    let stato_app = app.state::<Stato>();
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

// ── i comandi ───────────────────────────────────────────────────────────────

/// Fa partire una coda nuova a partire dal brano indicato.
#[tauri::command(async)]
pub fn suona(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
    indice: usize,
) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        lettore.coda.play_tracks(brani, indice, seme());
        Ok(true)
    })
    .map_err(errore)
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
#[tauri::command(async)]
pub fn radio(app: tauri::AppHandle, stato: State<'_, StatoLettore>, brano: i64) -> Esito<()> {
    let stato_app = app.state::<Stato>();
    let coda = con_libreria(&stato_app, |libreria| {
        aether_app::autoplay::radio(
            &libreria.connection,
            brano,
            aether_app::autoplay::RADIO,
            adesso_ms(),
        )
    })
    .map_err(errore)?;

    let mut brani = Vec::with_capacity(coda.len().saturating_add(1));
    brani.push(brano);
    brani.extend(coda);

    gesto_di_coda(&app, &stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        lettore.coda.play_tracks(brani, 0, seme());
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
/// cinque resterebbero fermi anche il filo della decodifica — che il lucchetto
/// lo vuole a ogni cambio di traccia — e il filo dell'orologio, che lo chiede
/// quattro volte al secondo.
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
fn avvia_corrente(app: &tauri::AppHandle, stato: &StatoLettore) -> Result<(), AppError> {
    // ── 1. chi, e con che formato ──
    let scelta = con_lettore(stato, |lettore| {
        let Some(id) = lettore.coda.current() else {
            lettore.motore.ferma();
            salva_coda(app, lettore);
            manda_stato(app, lettore);
            return Ok(None);
        };
        Ok(Some((id, lettore.motore.formato())))
    })?;
    let Some((id, formato)) = scelta else {
        return Ok(());
    };

    // ── 2. l'apertura, senza niente in mano ──
    let brano = brano_di(app, id, formato)?;

    // ── 3. la partenza ──
    con_lettore(stato, |lettore| {
        if lettore.coda.current() != Some(id) {
            return Ok(());
        }
        lettore.motore.suona(brano);
        prepara_prossimo(app, lettore);
        salva_coda(app, lettore);
        // Lo stato parte subito, senza aspettare che il primo campione esca: il
        // titolo nella barra deve comparire al clic, non un decimo di secondo
        // dopo. Con la posizione a zero, che è dove il brano nuovo comincia:
        // quella del motore è ancora quella del brano di prima, e mandarla
        // vorrebbe dire un titolo nuovo con il cursore a metà.
        manda_stato_con_posizione(app, lettore, 0);
        Ok(())
    })
}

/// Mette in pausa.
#[tauri::command]
pub fn pausa(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.motore.pausa();
        if let Some(a) = lettore.ascolto.as_mut() {
            a.pause(adesso_ms());
        }
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Riprende.
#[tauri::command(async)]
pub fn riprendi(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        // Coda ripresa dall'avvio: il motore non ha ancora niente in mano, e
        // «riprendi» deve voler dire «comincia».
        if lettore.motore.posizione().track_id.is_none() {
            return Ok(true);
        }
        lettore.motore.riprendi();
        if let Some(a) = lettore.ascolto.as_mut() {
            a.resume(adesso_ms());
        }
        manda_stato(&app, lettore);
        Ok(false)
    })
    .map_err(errore)
}

/// Alterna fra pausa e ripresa.
#[tauri::command(async)]
pub fn alterna(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    let in_pausa =
        con_lettore(&stato, |lettore| Ok(lettore.motore.posizione().in_pausa)).map_err(errore)?;
    if in_pausa {
        riprendi(app, stato)
    } else {
        pausa(app, stato)
    }
}

/// Passa al brano dopo.
#[tauri::command(async)]
pub fn prossimo(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        match lettore.coda.advance(true) {
            Step::Track(_) => Ok(true),
            Step::Restart => {
                lettore.motore.vai_a(0);
                manda_stato(&app, lettore);
                Ok(false)
            }
            Step::Stop => {
                lettore.motore.ferma();
                manda_stato(&app, lettore);
                Ok(false)
            }
        }
    })
    .map_err(errore)
}

/// Torna al brano prima, o ricomincia questo.
#[tauri::command(async)]
pub fn precedente(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        let dove = lettore.motore.posizione().ms;
        match lettore.coda.previous(dove) {
            Step::Track(_) => {
                chiudi_ascolto(&app, lettore);
                Ok(true)
            }
            // Ricominciare lo stesso brano non chiude l'ascolto: è ancora quello.
            Step::Restart => {
                lettore.motore.vai_a(0);
                manda_stato(&app, lettore);
                Ok(false)
            }
            Step::Stop => Ok(false),
        }
    })
    .map_err(errore)
}

/// Salta a un punto del brano.
#[tauri::command]
pub fn vai_a(app: tauri::AppHandle, stato: State<'_, StatoLettore>, ms: u64) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.motore.vai_a(ms);
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
/// libreria — mentre senza scheda audio Aether resta un catalogo consultabile,
/// che è quello che `StatoLettore` conserva l'errore per permettere.
///
/// Lo stato che torna è quello di un lettore fermo, con dentro il motivo: è la
/// stessa forma che la finestra riceve quando il dispositivo sparisce dopo, ed
/// è ciò che le permette di disegnare una fascia sola per i due casi.
#[tauri::command]
pub fn riproduzione_stato(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
) -> Esito<StatoRiproduzione> {
    let guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match guardia.as_ref() {
        Ok(lettore) => Ok(costruisci_stato(&app, lettore)),
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
            motivo_prossimo: None,
            audio: Some(guasto_di_apertura(err)),
            // Nessuna uscita: non se n'è mai aperta una.
            uscita: None,
        }),
    }
}
