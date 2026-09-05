//! La riproduzione, dal lato della finestra.
//!
//! Tiene insieme tre cose che vivono in tre posti diversi: la **coda**
//! (`aether_domain::queue`, pura), il **motore** (`aether-play`, che non sa cosa
//! sia una libreria) e il **database** (`aether_app::playback`). Nessuno dei tre
//! conosce gli altri due, ed è voluto — ma qualcuno deve pur presentarli, e quel
//! qualcuno è questo file.
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
//! libreria, mai il contrario**. Ogni funzione di questo file la rispetta.
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
//! il lucchetto della libreria, lo si lascia, e l'apertura va su un filo suo con
//! i cinque secondi di [`APERTURA_BRANO`] addosso — e che il ramo nuovo sta
//! **dentro** quella scadenza, non accanto. Vale anche per il gapless: il
//! [`filo del preparatore`](avvia_preparatore) apre il brano successivo in
//! anticipo, e da oggi «in anticipo» può voler dire una connessione aperta
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

use std::sync::Mutex;
use std::time::Duration;

use crate::spegnimento::Emette as _;
use aether_app::library::TrackSummary;
use aether_app::playback::{self, Equalizzazione, Normalizzazione, SchedaSorgente, Volume};
use aether_catalogo::Cataloghi;
use aether_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use aether_domain::esterno::Fonte;
use aether_domain::listen::ListenTracker;
use aether_domain::queue::{Queue, RepeatMode, Step};
use aether_domain::scelta::Candidato;
use aether_net::FlussoHttp;
use aether_play::{Evento, Motore, PRESET_DI_SERIE};
use serde::Serialize;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria};

/// Ogni quanto la finestra riceve la posizione.
///
/// Quattro volte al secondo, non sessanta. Il cursore si muove liscio lo stesso
/// perché la finestra interpola fra un colpo e l'altro; sessanta eventi al
/// secondo attraverso l'IPC sarebbero sessanta serializzazioni JSON al secondo
/// per spostare un pixel, e si vedrebbero nel consumo della batteria molto prima
/// che nella fluidità.
const PASSO_TEMPO: Duration = Duration::from_millis(250);

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
/// # Perché le ottave non ci sono
///
/// Il lettore le calcola dalla stessa trasformata — sono quelle
/// dell'equalizzatore, e le sue prove le controllano — ma nella finestra non le
/// legge più nessuno: le disegnava la striscia a dieci barre sotto la copertina,
/// e quella striscia non c'è più. Dieci `f32` per trenta eventi al secondo che
/// nessuno guarda sono dieci `f32` di troppo.
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
    pub causa: String,
    /// Riaprire ha senso provarlo.
    ///
    /// Sempre vero, oggi, e resta un campo perché la finestra non deve saperlo:
    /// il giorno in cui una causa non fosse più ritentabile, il tasto sparisce
    /// senza toccare la finestra.
    pub riapribile: bool,
}

/// La curva dell'equalizzatore da sola.
///
/// Un evento suo invece di [`StatoRiproduzione`]: comporre lo stato intero
/// richiede una lettura del brano corrente dal database, e durante un
/// trascinamento questa roba parte una dozzina di volte al secondo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoEq {
    /// I filtri sono accesi.
    pub attivo: bool,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
}

/// Una curva che si può scegliere dall'elenco.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocePreset {
    /// Come si chiama.
    pub nome: String,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
    /// Viene con l'applicazione, quindi non si può cancellare.
    pub di_serie: bool,
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
fn apri_motore(app: &tauri::AppHandle) -> Result<Motore, AppError> {
    let manico = app.clone();
    aether_play::avvia(move |evento| su_evento(&manico, evento))
}

impl StatoLettore {
    /// Avvia il motore e riprende la coda di ieri.
    ///
    /// Restituisce anche l'estremo da ascoltare del canale del preparatore, che
    /// va passato ad [`avvia_preparatore`]: la stessa forma di
    /// `nuvola::StatoNuvola::nuovo`, e per la stessa ragione — il canale nasce
    /// insieme allo stato che ne tiene l'estremo da cui si manda.
    pub fn avvia(app: &tauri::AppHandle) -> (Self, std::sync::mpsc::Receiver<()>) {
        let motore = apri_motore(app);
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
            },
            orecchio,
        )
    }
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
                "[avvio] audio aperto frequenza={} canali={}",
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

/// Il riferimento a un brano che non è un file.
///
/// Due campi e nient'altro, perché due sono le domande: **quale catalogo** — che
/// decide da quale [`aether_net::Rete`] escono i byte, con le sue scadenze, il
/// suo modo di trattare un `429` e la sua riserva di connessioni — e **a che
/// indirizzo**.
///
/// Non c'è la licenza e non c'è la disponibilità, e non è una dimenticanza: qui
/// non si scrive niente sul disco. Il cancello che le guarda è quello di
/// `aether_catalogo::prelievo`, e sta prima della rete perché prima della rete
/// deve stare; ascoltare mentre arriva è la cosa che [`Disponibilita::SoloAscolto`]
/// **permette**, e chiederne il permesso una seconda volta qui vorrebbe dire
/// due implementazioni della stessa regola.
///
/// [`Disponibilita::SoloAscolto`]: aether_domain::esterno::Disponibilita::SoloAscolto
#[derive(Debug, Clone, PartialEq, Eq)]
struct RiferimentoFlusso {
    /// Da quale catalogo, cioè con quale rete si apre.
    fonte: Fonte,
    /// L'indirizzo dei byte. Per Audius un percorso, che vuole ancora un nodo
    /// davanti: vedi [`apri_flusso`].
    url: String,
}

/// Il percorso con cui Audius conserva un brano invece di un indirizzo.
///
/// `aether_catalogo::audius` scrive `/v1/tracks/<id>/stream` e non un URL
/// intero, e la ragione sta scritta là: Audius non ha un server ma una rete di
/// nodi che entrano ed escono, e un indirizzo con dentro il nodo di oggi fra un
/// mese punta a una macchina che non c'è più. Il nodo lo rimette
/// `Audius::prepara` al momento di andare a prendere i byte.
const PERCORSO_AUDIUS: &str = "/v1/tracks/";

/// Questo brano è un flusso? Allora ecco da dove.
///
/// # Quale campo si legge, e quale si leggerà
///
/// Oggi `path`, perché in [`SchedaSorgente`] non c'è altro: `tracks.path` è
/// `NOT NULL UNIQUE` e la libreria non ha ancora un posto per una traccia che
/// non è un file — è il buco dichiarato fra i limiti noti del README, e
/// chiuderlo vuole una migrazione, quindi `aether-app` e una minor in più.
///
/// Quando quella migrazione arriverà, i campi da guardare sono già scritti nella
/// migrazione 10 e vivono su `desiderati`: `fonte_url` per l'indirizzo,
/// `source_service` per la fonte, `disponibilita` per il resto. **Cambia solo
/// questa funzione**: tutto quel che sta a valle — [`apri_flusso`],
/// [`sorgente_da_flusso`], il ramo dentro [`brano_di`] — riceve un
/// [`RiferimentoFlusso`] e non sa da quale colonna sia uscito.
///
/// # Perché il riconoscimento passa dal catalogo e non da un elenco di qui
///
/// Perché un elenco di domini scritto qui sarebbe il **secondo**:
/// `aether_catalogo::riconosci` è una allowlist rigida, e il suo modulo dice
/// perché — «un link che non si riconosce è un link a cui non si va, ed è la
/// garanzia che Aether non vada mai a bussare dove non è invitato». Due copie di
/// quella regola sono due copie che un giorno diranno cose diverse, e il giorno
/// in cui succede si va a bussare da qualche parte per sbaglio.
///
/// Di quel che `riconosci` restituisce serve solo la **fonte**: l'indirizzo
/// resta quello che c'era scritto, perché quello sono i byte, mentre l'`id` che
/// il catalogo ne ricava nomina il concerto e non il file. È un cancello, non un
/// traduttore.
fn riferimento_di(scheda: &SchedaSorgente) -> Option<RiferimentoFlusso> {
    riferimento_da_testo(&scheda.path)
}

/// Il riconoscimento vero e proprio, su una stringa sola.
///
/// Separata da [`riferimento_di`] per poterla provare senza un database e senza
/// una finestra: è una decisione su del testo, e le decisioni su del testo si
/// provano come chiamate di funzione.
fn riferimento_da_testo(testo: &str) -> Option<RiferimentoFlusso> {
    let pulito = testo.trim();
    // Il percorso di Audius prima di tutto, perché non è un indirizzo e
    // `riconosci` — che di indirizzi si occupa — non lo vedrebbe. Non si
    // confonde con un percorso di disco: su Windows un percorso comincia con
    // una lettera di unità o con due barre rovesce, mai con `/v1/`.
    if pulito.starts_with(PERCORSO_AUDIUS) {
        return Some(RiferimentoFlusso {
            fonte: Fonte::Audius,
            url: pulito.to_owned(),
        });
    }
    // `https://` e non anche `http://`: `Rete` nasce con `https_only`, quindi un
    // indirizzo in chiaro non partirebbe comunque, e riconoscerlo qui vorrebbe
    // dire promettere un'apertura che fallirà più in là con un errore che parla
    // d'altro.
    if !pulito.starts_with("https://") {
        return None;
    }
    Some(RiferimentoFlusso {
        fonte: aether_catalogo::riconosci(pulito)?.fonte,
        url: pulito.to_owned(),
    })
}

/// Apre il flusso di un riferimento, con la rete del catalogo che lo ha dato.
///
/// **Va in rete**, e quindi va chiamata dove la rete si può aspettare: dentro la
/// scadenza di [`brano_di`], senza nessun lucchetto in mano. Costa una richiesta
/// `Range` da [`aether_net::flusso::FINESTRA`] byte — l'inizio del brano, che è
/// la prima cosa che il decodificatore chiederà — più, su Audius, la scelta del
/// nodo.
///
/// # Perché la rete è quella del catalogo e non una nuova
///
/// Perché in quella `Rete` ci sono lo `User-Agent` con cui Aether si presenta,
/// la scadenza che quel servizio merita, il modo di leggere un `Retry-After` e
/// la riserva di connessioni già calda. Una `Rete` nuova per brano sarebbe un
/// saluto TLS per canzone e un limite di frequenza contato da capo ogni volta,
/// contro archivi pubblici che ci ospitano gratis — cioè il modo di farsi
/// bloccare per maleducazione.
///
/// # Errori
///
/// Quelli di `Rete::intervallo` (`net.*`, `download.*`) e, per Audius,
/// `catalogo.notAvailable` se non risponde nessun nodo.
/// `playback.sourceUnavailable` per una fonte che di byte non ne dà: è il caso
/// dell'archivio di Spotify e di un file di playlist, che nominano un brano e
/// non lo consegnano.
fn apri_flusso(cataloghi: &Cataloghi, rif: &RiferimentoFlusso) -> Result<FlussoHttp, AppError> {
    match rif.fonte {
        Fonte::InternetArchive => {
            FlussoHttp::nuovo(cataloghi.archivio().rete().clone(), rif.url.clone())
        }
        // Il nodo si sceglie **adesso**: è lo stesso passo che fa il prelievo, e
        // per la stessa ragione. Un `Candidato` con il solo indirizzo dentro
        // perché è tutto quel che `prepara` guarda, e perché costruirne uno
        // finto completo vorrebbe dire inventare una licenza per una funzione
        // che non la legge.
        Fonte::Audius => {
            let pronto = cataloghi.audius().prepara(&Candidato {
                url: rif.url.clone(),
                ..Candidato::default()
            })?;
            FlussoHttp::nuovo(cataloghi.audius().rete().clone(), pronto.url)
        }
        // Il catalogo per cui `FlussoHttp` è nato: da Jamendo non si tiene
        // niente, e ascoltare mentre arriva è l'unico modo lecito che c'è. Senza
        // la feature il ramo non esiste e la fonte cade nel caso generale, che è
        // la verità: questa copia di Aether Jamendo non ce l'ha.
        #[cfg(feature = "jamendo")]
        Fonte::Jamendo => FlussoHttp::nuovo(cataloghi.jamendo().rete().clone(), rif.url.clone()),
        altra => Err(AppError::new(ErrorCode::PlaybackSourceUnavailable {
            track_id: None,
            path: Some(rif.url.clone()),
        })
        .with_cause(format!(
            "da {} non arrivano byte da suonare",
            altra.etichetta()
        ))),
    }
}

/// Un flusso di rete, vestito da [`aether_play::Flusso`].
///
/// # Perché serve un involucro
///
/// Perché il tratto è di `aether-play` e il tipo è di `aether-net`, e nessuno
/// dei due è di qui: la regola dell'orfano vieta di scrivere quell'`impl` in un
/// terzo crate. È la stessa ragione per cui `aether-app` ne ha uno suo
/// (`playback::Adattatore`) per i file, e la stessa forma.
///
/// Non è però solo una formalità del compilatore. Questo modulo è **l'unico
/// posto dell'albero che conosce tutti e due i lati**: `aether-app` non vede la
/// rete e `aether-catalogo` non vede il motore. Il punto in cui un flusso HTTP
/// diventa qualcosa che si può suonare non poteva stare altrove.
struct FlussoDiRete(FlussoHttp);

impl std::io::Read for FlussoDiRete {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Seek for FlussoDiRete {
    fn seek(&mut self, verso: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(verso)
    }
}

impl aether_play::Flusso for FlussoDiRete {
    /// Quanto è lungo, se il servizio lo ha dichiarato.
    ///
    /// Non è un di più: `MediaSource::is_seekable` di symphonia risponde
    /// guardando questo, e un `None` qui vuol dire il cursore che sparisce dalla
    /// barra prima ancora che il brano cominci.
    fn lunghezza(&self) -> Option<u64> {
        self.0.lunghezza()
    }
}

/// Da un flusso aperto alla sorgente che il motore sa suonare.
///
/// Quel che il motore riceve è indistinguibile da un file: stesso `track_id`,
/// stessa durata dichiarata dal database, stesso guadagno ReplayGain. È il
/// motivo per cui gapless, dissolvenza, equalizzatore e spettro funzionano senza
/// una riga in più — il motore non ha mai saputo cosa ci fosse dietro i byte.
fn sorgente_da_flusso(
    scheda: &SchedaSorgente,
    url: &str,
    flusso: FlussoHttp,
) -> aether_play::Sorgente {
    aether_play::Sorgente {
        track_id: scheda.track_id,
        media: Box::new(FlussoDiRete(flusso)),
        estensione: estensione_da_url(url),
        durata_ms: u64::try_from(scheda.durata_ms).unwrap_or(0),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "sono decibel: la precisione di un f32 è un milionesimo di dB"
        )]
        replaygain_db: scheda.replaygain_db.map(|db| db as f32),
    }
}

/// Il suggerimento di formato ricavato da un indirizzo.
///
/// # Perché la query si butta prima di guardare il punto
///
/// Perché su Audius la query **mente, apposta**: `/v1/tracks/<id>/stream?ext=wav`
/// dice l'estensione del file che l'artista ha caricato, e serve al prelievo per
/// non chiamare `.mp3` un wav. Ma il punto di ascolto restituisce un mp3
/// transcodificato, non quel wav: passare `wav` a symphonia come suggerimento
/// vuol dire farle provare per primo il lettore sbagliato. Un suggerimento
/// assente è meglio di uno falso — symphonia riconosce comunque il contenitore
/// dai marcatori, ed è la strada che prende ogni volta che l'estensione non c'è.
///
/// Le stesse regole di `aether_app::playback::estensione_di`, che per i file fa
/// questo identico mestiere: l'ultimo segmento, dopo l'ultimo punto, in
/// minuscolo, e niente se è vuoto o più lungo di cinque caratteri.
fn estensione_da_url(url: &str) -> Option<String> {
    let senza_query = url.split(['?', '#']).next().unwrap_or(url);
    // Il nome del file sta nel **percorso**, e un URL senza percorso non ne ha
    // nessuno. Senza questi due passaggi `https://archive.org` avrebbe come
    // estensione il proprio dominio di primo livello — e il motore si vedrebbe
    // suggerire un contenitore «org» che non esiste.
    let dopo_schema = senza_query
        .split_once("://")
        .map_or(senza_query, |(_, resto)| resto);
    let (_, percorso) = dopo_schema.split_once('/')?;
    let ultimo = percorso.rsplit('/').next().unwrap_or(percorso);
    let (_, ext) = ultimo.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
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

/// Dice al motore quale sarà il brano dopo.
///
/// È tutto il gapless: il file successivo viene aperto **mentre** il corrente
/// suona ancora, così quando tocca a lui i suoi campioni sono già pronti. Un
/// brano che non si apre non è un guasto da mostrare adesso: lo si scoprirà
/// quando toccherà a lui, e nel frattempo quello che suona non va interrotto.
///
/// # Perché questa funzione non apre più niente
///
/// Perché viene chiamata anche **dall'osservatore**, cioè dal filo della
/// decodifica: il ramo `Evento::Iniziato` di [`su_evento`] la invoca a ogni
/// cambio di traccia. Aprire là dentro voleva dire fermare il filo che riempie
/// l'anello per tutta la durata dell'apertura — fino ai cinque secondi della
/// scadenza, su una share che risponde male — contro una riserva che a 48 kHz
/// stereo vale poco più di tre secondi. Cioè un buco udibile, a ogni cambio di
/// traccia, proprio quando la rete è già in difficoltà.
///
/// Adesso è una **spinta**: si manda un colpetto sul canale e si torna subito.
/// Il lavoro vero lo fa il filo di [`avvia_preparatore`].
fn prepara_prossimo(app: &tauri::AppHandle, _lettore: &mut Lettore) {
    // `_lettore` resta nella firma apposta: dice che chi chiama ha il lucchetto
    // in mano, ed è mentre ce l'ha che il colpetto va mandato — così il filo che
    // si sveglia trova la coda già nello stato nuovo e non in quello di un
    // istante prima.
    sveglia_preparatore(&app.state::<StatoLettore>());
}

/// Il colpetto sul canale del preparatore.
///
/// Non blocca, e l'unico errore possibile non è interessante: il canale è senza
/// limite, quindi una `send` fallisce solo se il filo è morto — cioè se
/// l'applicazione sta uscendo, quando non c'è più nessun brano dopo da
/// preparare.
fn sveglia_preparatore(stato: &StatoLettore) {
    let _ = stato.prepara.send(());
}

/// Quanto si aspetta che una raffica di spinte si calmi.
///
/// Riordinare una playlist a trascinamenti manda una spinta per movimento, e
/// preparare il successivo dieci volte di fila vuol dire aprire dieci file per
/// buttarne nove. Quindici millisecondi non si sentono — il gapless comincia a
/// contare secondi prima della fine del brano — e tolgono di mezzo la raffica.
const RAFFICA_PREPARA: Duration = Duration::from_millis(15);

/// Il filo che apre il brano successivo.
///
/// Esiste per una ragione sola: **togliere l'apertura di un file dal filo della
/// decodifica**, dove stava perché l'osservatore ci gira sopra. Il perché per
/// esteso è in [`prepara_prossimo`].
///
/// Modellato sugli altri fili di questo albero — `avvia_orologio`,
/// `nuvola::avvia_filo`: un thread nominato, un canale, nessun runtime
/// asincrono.
pub fn avvia_preparatore(app: tauri::AppHandle, orecchio: std::sync::mpsc::Receiver<()>) {
    let avviato = std::thread::Builder::new()
        .name("aether-preparatore".to_owned())
        .spawn(move || {
            loop {
                // Canale chiuso: l'applicazione sta uscendo.
                if orecchio.recv().is_err() {
                    return;
                }
                // Si lascia finire la raffica: `recv_timeout` fa da
                // antirimbalzo, e si esce quando per quindici millisecondi non
                // arriva più niente — o subito, se il canale si è chiuso.
                while orecchio.recv_timeout(RAFFICA_PREPARA).is_ok() {}
                prepara_prossimo_adesso(&app);
            }
        });
    if avviato.is_err() {
        // Senza il filo si perde il gapless, non la riproduzione: ogni brano
        // verrà aperto quando tocca a lui. Vale una riga nel diario, non un
        // avvio fallito.
        nota!("[riproduzione] il filo del preparatore non è partito: niente gapless");
    }
}

/// Sceglie, apre e consegna il brano successivo. Gira sul filo del preparatore.
///
/// Tre tempi, e la divisione è tutto il punto: **decidere** vuole il lucchetto e
/// dura microsecondi, **aprire** non lo vuole e può durare secondi, **consegnare**
/// lo rivuole e dura di nuovo microsecondi. Tenerli insieme vorrebbe dire il
/// lucchetto del lettore in mano per tutta l'apertura — e allora tanto varrebbe
/// essere rimasti sul filo della decodifica.
///
/// # Quando il brano dopo è un flusso
///
/// Il secondo tempo diventa una **connessione aperta in anticipo**: 256 kilobyte
/// chiesti a un catalogo mentre il brano corrente suona ancora. La divisione in
/// tre tempi, che era nata per le share lente, è quel che rende la cosa
/// sostenibile — l'attesa avviene qui, su questo filo, con le mani vuote.
///
/// L'antirimbalzo di [`RAFFICA_PREPARA`] conta il doppio in questo caso: dieci
/// file aperti per buttarne nove erano dieci `open` sul disco di chi ascolta,
/// dieci flussi aperti per buttarne nove sono dieci richieste a un archivio
/// pubblico che nessuno paga. Il terzo tempo lascia comunque cadere quel che non
/// serve più — un successivo sbagliato attaccato in gapless si sente — ma la
/// richiesta, a quel punto, è già partita: è la raffica a doverla evitare, non
/// il controllo finale.
fn prepara_prossimo_adesso(app: &tauri::AppHandle) {
    let stato = app.state::<StatoLettore>();

    // ── 1. la decisione, sotto lucchetto ──
    let scelta = con_lettore(&stato, |lettore| {
        // Il timer «fine del brano» si fa qui, e non con una sveglia: non è un
        // istante da aspettare ma un successivo che non deve esserci. Detto
        // così, la musica finisce dove sarebbe finita comunque — senza
        // dissolvenze, senza tagli, senza un secondo di silenzio prima del
        // previsto.
        let fine = stato.spegnimento.load(std::sync::atomic::Ordering::Relaxed) == FINE_DEL_BRANO;
        if fine {
            lettore.motore.prepara(None);
            return Ok(None);
        }

        // La coda non ha un dopo: è qui che l'autoplay entra, e **qui** e non
        // sull'evento `Fermato`. Accodando adesso — mentre il brano corrente
        // suona ancora — quel che si sceglie passa dalla stessa strada di tutti
        // gli altri: viene aperto in anticipo, attacca senza stacco, e non c'è
        // nessun istante in cui l'applicazione si sia fermata. Reagire a
        // `Fermato` vorrebbe dire ripartire *dopo* il silenzio.
        if lettore.coda.peek_next().is_none() && lettore.autoplay {
            if let Some(scelta) = scegli_da_solo(app, lettore) {
                lettore.coda.enqueue(&[scelta.id]);
                lettore.motivo_prossimo = Some((scelta.id, scelta.motivo.codice()));
            }
        }

        let formato = lettore.motore.formato();
        match lettore.coda.peek_next() {
            // Nessun successivo: azzerare è l'unica cosa da fare, dura quanto
            // una scrittura su un canale, e si fa subito qui.
            None => {
                lettore.motore.prepara(None);
                Ok(None)
            }
            Some(id) => Ok(Some((id, formato))),
        }
    });
    let Ok(Some((id, formato))) = scelta else {
        return;
    };

    // ── 2. l'apertura, senza lucchetti in mano ──
    let brano = match brano_di(app, id, formato) {
        Ok(brano) => brano,
        Err(err) => {
            // Si dice adesso, mentre il corrente suona ancora, invece di
            // scoprirlo nel silenzio fra i due.
            //
            // **Senza** annotare una ripresa, al contrario di quel che si fa
            // quando la rete cade a brano avviato: qui non si è interrotto
            // niente — quel che si sente continua — e segnarsi un punto a cui
            // tornare vorrebbe dire offrire un «Riprova» che riavvolge una
            // canzone che sta suonando bene.
            nota!(
                "[riproduzione] il brano dopo non si apre: {} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            app.emetti("riproduzione:errore", crate::errore::errore(err));
            return;
        }
    };

    // ── 3. la consegna, di nuovo sotto lucchetto ──
    let _ = con_lettore(&stato, |lettore| {
        // Nel frattempo la coda può essere cambiata, e chi l'ha cambiata ha
        // mandato la sua spinta: questo brano non serve più a nessuno. Si lascia
        // cadere invece di metterlo in canna — un successivo sbagliato attaccato
        // in gapless si sente, ed è peggio di un gapless mancato.
        if lettore.coda.peek_next() != Some(id) {
            return Ok(());
        }
        lettore.motore.prepara(Some(brano));
        Ok(())
    });
}

/// Il brano che continua la sessione, secondo la libreria.
///
/// Fuori da [`prepara_prossimo`] perché prende il lucchetto della libreria, e
/// la regola di questo file è che lo si prenda **dopo** quello del lettore —
/// che a questo punto è già in mano a chi ci ha chiamati. Tenerla separata
/// rende la sequenza leggibile invece che implicita.
fn scegli_da_solo(
    app: &tauri::AppHandle,
    lettore: &Lettore,
) -> Option<aether_app::autoplay::Scelta> {
    let corrente = lettore.coda.current()?;
    // Tutto quel che è già in coda, così l'autoplay non ripropone quel che si
    // è appena sentito.
    let esclusi = lettore.coda.in_play_order();
    let stato = app.state::<Stato>();
    con_libreria(&stato, |libreria| {
        aether_app::autoplay::prossimo(&libreria.connection, corrente, &esclusi, adesso_ms())
    })
    .ok()
    .flatten()
}

/// Conserva la coda.
fn salva_coda(app: &tauri::AppHandle, lettore: &Lettore) {
    let istantanea = lettore.coda.snapshot();
    let stato = app.state::<Stato>();
    let _ = con_libreria(&stato, |libreria| {
        playback::save_queue(&libreria.connection, &istantanea)
    });
}

/// Compone lo stato e lo manda alla finestra — e al sistema operativo.
///
/// I due destinatari sono qui insieme di proposito. La scheda nel riquadro del
/// volume di Windows dice le stesse cose della barra in fondo alla finestra, e
/// aggiornarla da un posto suo vorrebbe dire due sorgenti per la stessa
/// verità: prima o poi una delle due mostrerebbe il brano di prima, e sarebbe
/// il difetto che nessuno segnala perché chi lo vede pensa di aver letto male.
fn manda_stato(app: &tauri::AppHandle, lettore: &Lettore) {
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
    }
}

/// Il guasto del motore, se ce n'è uno.
fn guasto_di(motore: &Motore) -> Option<GuastoAudio> {
    motore.causa_perdita().map(|causa| GuastoAudio {
        codice: ErrorCode::PlaybackDeviceLost.kind().code().to_owned(),
        causa: causa.to_owned(),
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

/// Il livello di normalizzazione che porta questo nome.
///
/// # Perché un nome e non un numero di decibel
///
/// Il modello sotto è ed era un bersaglio in decibel, e resta quello: qui non
/// si aggiunge niente al motore, che sapeva già portare tutto a un livello
/// scelto. Quel che mancava era un modo di dirglielo.
///
/// Ai decibel non si dà però accesso dalla finestra. Un cursore da −30 a −6
/// chiederebbe a chi ascolta di sapere cos'è un LUFS per decidere, e la
/// risposta giusta per quasi tutti è una di tre. Il nome viene tradotto qui in
/// un `match` chiuso, come `ordine` in [`crate::comandi::brani`]: quel che
/// arriva dalla finestra non raggiunge mai un valore che il motore userebbe
/// senza guardarlo.
///
/// Un nome sconosciuto vale «normale», che è il riferimento dei tag.
fn normalizzazione_da_nome(nome: &str) -> Normalizzazione {
    match nome {
        // Spento conserva il bersaglio invece di azzerarlo: chi rispegne e
        // riaccende ritrova il livello che aveva scelto, non quello di serie.
        "spento" => Normalizzazione {
            attivo: false,
            bersaglio_db: playback::BERSAGLIO_PREDEFINITO_DB,
        },
        "basso" => Normalizzazione {
            attivo: true,
            bersaglio_db: playback::BERSAGLIO_BASSO_DB,
        },
        "alto" => Normalizzazione {
            attivo: true,
            bersaglio_db: playback::BERSAGLIO_ALTO_DB,
        },
        _ => Normalizzazione {
            attivo: true,
            bersaglio_db: playback::BERSAGLIO_PREDEFINITO_DB,
        },
    }
}

/// Come si chiama il livello in cui si trova la normalizzazione.
///
/// Il verso opposto di [`normalizzazione_da_nome`], e non è un `match` perché
/// il bersaglio su disco è un `f32` che passa da un taglio: confrontarlo con
/// `==` vorrebbe dire che un valore scritto da una versione precedente, o
/// limitato da `sana`, non corrisponde a nessun nome e la finestra non
/// evidenzia niente. Si prende il più vicino dei tre, che per i valori scritti
/// da qui è sempre quello esatto.
fn nome_normalizzazione(normalizzazione: Normalizzazione) -> &'static str {
    if !normalizzazione.attivo {
        return "spento";
    }
    let scarto = |bersaglio: f32| (normalizzazione.bersaglio_db - bersaglio).abs();
    let mut nome = "normale";
    let mut minimo = scarto(playback::BERSAGLIO_PREDEFINITO_DB);
    if scarto(playback::BERSAGLIO_BASSO_DB) < minimo {
        nome = "basso";
        minimo = scarto(playback::BERSAGLIO_BASSO_DB);
    }
    if scarto(playback::BERSAGLIO_ALTO_DB) < minimo {
        nome = "alto";
    }
    nome
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

/// Quanto manca allo spegnimento, come lo legge la finestra.
///
/// `None` se non c'è nessun timer, `Some(0)` per «alla fine di questo brano»,
/// che non ha una durata da mostrare. Il conto non scende mai sotto uno: un
/// timer scaduto ma non ancora raccolto dall'orologio — c'è un quarto di
/// secondo in cui può succedere — mostrerebbe altrimenti un numero negativo, e
/// azzerarlo e basta lo farebbe passare per l'altro `Some(0)`, cioè per «alla
/// fine di questo brano»: la finestra scriverebbe che il timer aspetta la fine
/// del brano proprio nell'istante in cui invece sta per spegnere tutto.
fn quanto_manca(stato_lettore: &StatoLettore) -> Option<i64> {
    let quando = stato_lettore
        .spegnimento
        .load(std::sync::atomic::Ordering::Relaxed);
    match quando {
        0 => None,
        FINE_DEL_BRANO => Some(0),
        scadenza => Some((scadenza - adesso_ms()).max(1)),
    }
}

/// Accende, cambia o spegne il timer di spegnimento.
///
/// `minuti` a zero spegne il timer; [`FINE_DEL_BRANO`] chiede di fermarsi dove
/// finisce quel che sta suonando; qualunque altro numero positivo sono i
/// minuti da adesso.
///
/// # Perché i minuti e non un istante
///
/// Perché «fra mezz'ora» è quel che si intende, e un istante calcolato dalla
/// finestra sarebbe calcolato con l'orologio della finestra. Sono lo stesso
/// orologio finché nessuno cambia fuso, e «finché nessuno» non è una
/// garanzia.
#[tauri::command(async)]
pub fn spegnimento(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    minuti: i64,
) -> Esito<()> {
    use std::sync::atomic::Ordering;

    let quando = match minuti {
        0 => 0,
        FINE_DEL_BRANO => FINE_DEL_BRANO,
        // Un tetto a ventiquattro ore: oltre non è più un timer per
        // addormentarsi, e `adesso_ms` più un numero arbitrario è il modo di
        // farlo traboccare.
        minuti => adesso_ms().saturating_add(minuti.clamp(1, 24 * 60).saturating_mul(60_000)),
    };
    stato.spegnimento.store(quando, Ordering::Relaxed);

    // «Fine del brano» cambia quel che il motore ha già in canna: il brano
    // successivo era stato preparato quando questo è cominciato, e senza
    // questa riga suonerebbe lo stesso.
    con_lettore(&stato, |lettore| {
        prepara_prossimo(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Mette in pausa se il timer di spegnimento è scaduto.
///
/// Pausa e non `ferma`: chi si addormenta con la musica accesa, al risveglio,
/// vuole ritrovare il segno dov'era. `ferma` butterebbe la posizione, e il
/// mattino dopo il brano ripartirebbe da capo senza che nessuno capisca
/// perché.
///
/// Il modo «fine del brano» non passa di qui: quello non è una scadenza ma un
/// brano successivo che non viene preparato, e lo decide
/// [`prepara_prossimo`].
fn scade_il_timer(app: &tauri::AppHandle, stato_lettore: &StatoLettore) {
    use std::sync::atomic::Ordering;

    let quando = stato_lettore.spegnimento.load(Ordering::Relaxed);
    if quando <= 0 || adesso_ms() < quando {
        return;
    }
    // Si azzera **prima** di agire, e solo se nel frattempo nessuno l'ha
    // cambiato: uno scambio secco metterebbe a zero anche una scadenza nuova —
    // o un «fine del brano» — arrivata fra la lettura qui sopra e questa riga,
    // spegnendo un timer che qualcuno aveva appena acceso.
    if stato_lettore
        .spegnimento
        .compare_exchange(quando, 0, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let _ = con_lettore(stato_lettore, |lettore| {
        lettore.motore.pausa();
        if let Some(ascolto) = lettore.ascolto.as_mut() {
            ascolto.pause(adesso_ms());
        }
        manda_stato(app, lettore);
        Ok(())
    });
}

/// Scrive sul disco volume e curva, se sono cambiati da quando li si è scritti.
///
/// # Chi la chiama, e perché non il comando
///
/// La chiama il filo dell'orologio a ogni giro, e il `RunEvent` d'uscita in
/// `main` un'ultima volta. I comandi `volume` ed `equalizzatore` alzano soltanto
/// una bandiera: la ragione per esteso sta su
/// [`StatoLettore::volume_da_salvare`], e in breve è che quei due comandi
/// arrivano una dozzina di volte al secondo da un cursore sotto un dito, e che
/// aprire una transazione su SQLite dodici volte al secondo dal filo che disegna
/// la finestra è il modo di far sembrare morta la finestra.
///
/// # Perché prende le bandiere prima del lucchetto
///
/// Perché `swap` le abbassa e dice com'erano in un colpo solo: se un comando
/// arriva mentre questa scrive, rialza la sua e il giro dopo la riscrive. Il
/// contrario — leggere, scrivere, poi abbassare — perderebbe quel cambio.
///
/// Quattro volte al secondo si legge una coppia di atomici rilassati e quasi
/// sempre si scopre che non c'è niente da fare, che è il costo che questo
/// modulo paga già per `spettro` e per `spegnimento`.
fn salva_quel_che_manca(app: &tauri::AppHandle, stato: &StatoLettore) {
    use std::sync::atomic::Ordering::Relaxed;
    let volume = stato.volume_da_salvare.swap(false, Relaxed);
    let eq = stato.eq_da_salvare.swap(false, Relaxed);
    if !volume && !eq {
        return;
    }
    // Una lettura sola sotto il lucchetto del lettore, e poi lo si molla: il
    // database si tocca **fuori**, che è la regola di tutto questo modulo.
    let valori = con_lettore(stato, |lettore| Ok((lettore.volume, lettore.eq.clone())));
    let Ok((salva_volume, salva_eq)) = valori else {
        return;
    };
    let Some(stato_app) = app.try_state::<Stato>() else {
        return;
    };
    let esito = con_libreria(&stato_app, |libreria| {
        if volume {
            playback::save_volume(&libreria.connection, salva_volume)?;
        }
        if eq {
            playback::save_eq(&libreria.connection, &salva_eq)?;
        }
        Ok(())
    });
    if let Err(err) = esito {
        nota!(
            "[riproduzione] preferenze audio non salvate codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
    }
}

/// L'ultimo giro di [`salva_quel_che_manca`], mentre si chiude.
///
/// Il filo dell'orologio esce appena la bandiera dello spegnimento si alza, e
/// senza questa chiamata l'ultima posizione di un cursore mosso un attimo prima
/// di chiudere sarebbe l'unica a non arrivare mai sul disco. Qui lo stato c'è
/// ancora: il `RunEvent` d'uscita arriva mentre la finestra esiste.
pub fn salva_uscendo(app: &tauri::AppHandle) {
    if let Some(stato) = app.try_state::<StatoLettore>() {
        salva_quel_che_manca(app, &stato);
    }
}

/// Avvia il filo che manda la posizione alla finestra.
///
/// Manda solo mentre suona: un'applicazione ferma in secondo piano non deve
/// svegliare il webview quattro volte al secondo per dirgli che non è cambiato
/// niente.
pub fn avvia_orologio(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("aether-orologio".to_owned())
        .spawn(move || {
            let mut ultimo_fermo = true;
            // Quanti giri d'orologio sono passati, per il segno di «riprendi
            // dov'eri». Un contatore locale al filo: non lo guarda nessun
            // altro, e non merita né un atomico né un lucchetto.
            let mut battiti: u32 = 0;
            // Il dispositivo era già sparito all'ultimo giro: serve perché
            // l'annuncio parta **una volta**, non quattro al secondo per tutto
            // il tempo in cui le cuffie restano staccate.
            let mut gia_perso = false;
            loop {
                std::thread::sleep(PASSO_TEMPO);
                // Prima di qualunque cosa che tocchi l'`AppHandle`: se la
                // finestra si sta chiudendo, un `emit` da qui fa panicare il
                // ciclo degli eventi. Vedi `crate::spegnimento`.
                if crate::spegnimento::in_uscita() {
                    return;
                }
                // `try_state` e non `state`: `state` panica se lo stato non
                // c'è, e questo filo gira anche mentre lo stato viene lasciato
                // cadere. Nessuno stato vuol dire che non c'è più niente da
                // raccontare a nessuno.
                let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
                    return;
                };

                // Le preferenze audio che i cursori hanno lasciato in sospeso.
                // Quasi sempre non c'è niente da fare: due letture rilassate.
                salva_quel_che_manca(&app, &stato_lettore);

                // Prima della posizione, perché è la ragione per cui la
                // posizione ha smesso di muoversi. Un dispositivo sparito
                // lascia la callback senza nessuno che la chiami: i fotogrammi
                // non avanzano più, il cursore resta fermo, e senza questo
                // controllo la finestra continua a dire che sta suonando.
                let perso = con_lettore(&stato_lettore, |lettore| Ok(guasto_di(&lettore.motore)))
                    .unwrap_or(None);
                match (&perso, gia_perso) {
                    (Some(guasto), false) => {
                        nota!("[riproduzione] dispositivo audio perso: {}", guasto.causa);
                        app.emetti("riproduzione:audio", guasto.clone());
                        // E lo stato intero, perché chi non stava ascoltando
                        // l'evento — una schermata aperta dopo — lo trovi lì.
                        let _ = con_lettore(&stato_lettore, |lettore| {
                            manda_stato(&app, lettore);
                            Ok(())
                        });
                        gia_perso = true;
                    }
                    (None, true) => gia_perso = false,
                    _ => {}
                }

                // Il timer di spegnimento. Prima della posizione perché se
                // scade adesso, la posizione che manderemmo fra due righe
                // sarebbe già quella di un lettore in pausa.
                scade_il_timer(&app, &stato_lettore);

                let tempo = con_lettore(&stato_lettore, |lettore| {
                    let p = lettore.motore.posizione();
                    Ok(Tempo {
                        posizione_ms: p.ms,
                        durata_ms: p.durata_ms,
                        in_pausa: p.in_pausa,
                    })
                });
                let Ok(tempo) = tempo else { continue };
                // Il filo dell'analisi sonora legge da qui se può leggere dal
                // disco. Passa da un'atomica e non da `con_lettore` perché
                // chiedere al lettore come sta, per sapere se disturbarlo,
                // sarebbe gia` disturbarlo: la ragione per esteso sta in testa
                // a `analisi.rs`.
                crate::analisi::segna_riproduzione(!tempo.in_pausa && perso.is_none());
                // Anche al sistema operativo, che dei salti relativi delle
                // cuffie sa solo il «di quanto» e mai il «da dove»: senza
                // questa riga il suo «da dove» resterebbe quello dell'ultimo
                // `manda_stato`, cioè quasi sempre l'inizio del brano.
                crate::media::segna_posizione(&app, tempo.posizione_ms);
                // Con il dispositivo perso la posizione è ferma per definizione:
                // mandarla quattro volte al secondo direbbe «sta suonando» a chi
                // interpola, che è la bugia che questo giro è venuto a togliere.
                if perso.is_some() {
                    continue;
                }
                // Un colpo anche quando si è appena fermato, per non lasciare il
                // cursore a interpolare nel vuoto.
                if !tempo.in_pausa || !ultimo_fermo {
                    app.emetti("riproduzione:tempo", tempo);
                }
                ultimo_fermo = tempo.in_pausa;

                // Il segno per «riprendi dov'eri», ogni tanto.
                //
                // Non a ogni giro: sarebbero quattro scritture al secondo sul
                // database per un numero che serve una volta sola, alla
                // prossima apertura. Ogni venti giri sono cinque secondi, che
                // è la peggior imprecisione possibile su una cosa che si
                // riprende a mano — e chi chiude a metà brano ritrova il segno
                // a cinque secondi da dove l'aveva lasciato, non all'inizio.
                battiti = battiti.wrapping_add(1);
                if !tempo.in_pausa && battiti % BATTITI_PER_SEGNO == 0 {
                    let stato_app = app.state::<Stato>();
                    let _ = con_libreria(&stato_app, |libreria| {
                        playback::save_posizione(&libreria.connection, tempo.posizione_ms)
                    });
                }
            }
        })
        .ok();
}

/// Avvia il filo che manda le bande dello spettro.
///
/// Separato dall'orologio e non un ramo dentro di esso: sono due cadenze
/// diverse per due ragioni diverse — vedi [`PASSO_SPETTRO`] — e infilarle nello
/// stesso ciclo avrebbe voluto dire mandare la posizione trenta volte al
/// secondo o le bande quattro.
///
/// Quando nessuno guarda dorme e non prende nessun lucchetto: la schermata
/// chiusa non costa niente né qui né nella callback audio.
pub fn avvia_spettro(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("aether-spettro".to_owned())
        .spawn(move || {
            loop {
                // Trenta giri al secondo: è il filo che ha più probabilità di
                // tutti di avere un messaggio in volo nell'istante in cui la
                // finestra si smonta, ed è per questo che il controllo sta in
                // cima. Vedi `crate::spegnimento`.
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
                    return;
                };
                if !stato_lettore
                    .spettro
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    std::thread::sleep(PASSO_SPETTRO_FERMO);
                    continue;
                }
                // La quantizzazione avviene **dentro** il lucchetto, e non è
                // una distrazione: è l'unico modo di leggere fino a 1024 bande
                // senza copiarle prima in un vettore da buttare via subito dopo.
                let bande = con_lettore(&stato_lettore, |lettore| {
                    Ok(lettore.motore.spettro(BandeIpc::da))
                });
                if let Ok(Some(bande)) = bande {
                    app.emetti("riproduzione:spettro", bande);
                }
                std::thread::sleep(PASSO_SPETTRO);
            }
        })
        .ok();
}

/// Riprende la coda conservata, senza far partire niente.
///
/// La coda torna, la musica no: un'applicazione che comincia a suonare da sola
/// all'avvio è un'applicazione che fa saltare sulla sedia chi l'ha aperta per
/// cercare una canzone.
pub fn riprendi_coda(app: &tauri::AppHandle) {
    let stato_lettore = app.state::<StatoLettore>();
    let _ = con_lettore(&stato_lettore, |lettore| {
        let stato = app.state::<Stato>();
        let istantanea = con_libreria(&stato, |libreria| {
            playback::load_queue(&libreria.connection)
        })?;
        lettore.coda = Queue::restore(istantanea);
        let volume = con_libreria(&stato, |libreria| {
            playback::load_volume(&libreria.connection)
        })
        .unwrap_or_default();
        lettore.volume = volume;
        lettore.motore.volume(volume.volume, volume.muto);
        let eq = con_libreria(&stato, |libreria| playback::load_eq(&libreria.connection))
            .unwrap_or_default();
        lettore.motore.equalizzatore(&eq.guadagni, eq.attivo);
        lettore.eq = eq;
        // La normalizzazione si rimanda al motore anche quando coincide con il
        // suo valore di serie: costa un comando su una coda che è già lì, e
        // toglie di mezzo la domanda «chi dei due ha ragione» il giorno in cui
        // uno dei due valori cambiasse.
        let normalizzazione = con_libreria(&stato, |libreria| {
            playback::load_replaygain(&libreria.connection)
        })
        .unwrap_or_default();
        lettore
            .motore
            .replaygain(normalizzazione.attivo, normalizzazione.bersaglio_db);
        lettore.normalizzazione = normalizzazione;

        // L'autoplay non si manda al motore — il motore non sa cosa sia una
        // libreria, ed è giusto così. Serve solo a `prepara_prossimo`, che è
        // di qui.
        lettore.autoplay = con_libreria(&stato, |libreria| {
            playback::load_autoplay(&libreria.connection)
        })
        .unwrap_or(false);

        // La dissolvenza invece sì: è il motore a doverla fare, ed è l'unico
        // che sa quando un brano sta per finire. Si manda anche quando vale
        // zero, per la stessa ragione della normalizzazione qui sopra.
        let dissolvenza_s = con_libreria(&stato, |libreria| {
            playback::load_crossfade(&libreria.connection)
        })
        .unwrap_or(0);
        lettore
            .motore
            .dissolvenza(dissolvenza_s.saturating_mul(1000));
        lettore.dissolvenza_s = dissolvenza_s;

        // Quante barre vuole vedere chi guarda. Si applica all'avvio anche se
        // la schermata è chiusa: costa un messaggio e vuol dire che la prima
        // apertura mostra la scena giusta invece di quella di serie per un
        // fotogramma.
        let bande = con_libreria(&stato, |libreria| {
            playback::load_spettro_bande(&libreria.connection)
        })
        .unwrap_or(aether_play::RISOLUZIONE_DI_SERIE);
        app.state::<StatoLettore>()
            .spettro_bande
            .store(bande, std::sync::atomic::Ordering::Relaxed);
        lettore.motore.spettro_dettaglio(bande);
        Ok(())
    });
}

// ── i comandi ───────────────────────────────────────────────────────────────

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

/// Un gesto sulla coda, e poi — se il gesto lo chiede — la partenza.
///
/// Esiste per tenere le due cose **una fuori dall'altra**: il gesto vuole il
/// lucchetto del lettore e dura microsecondi, la partenza apre un file e non
/// deve averlo in mano. Il `bool` che il gesto restituisce è «adesso fai
/// partire il corrente», e girare quella decisione qui invece di chiamare
/// [`avvia_corrente`] da dentro la chiusura è ciò che impedisce di riprendere un
/// `Mutex` che è già preso — cioè un blocco irreversibile della finestra.
fn gesto_di_coda(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    gesto: impl FnOnce(&mut Lettore) -> Result<bool, AppError>,
) -> Result<(), AppError> {
    if con_lettore(stato, gesto)? {
        avvia_corrente(app, stato)?;
    }
    Ok(())
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

/// Cambia volume e silenziamento.
#[tauri::command]
pub fn volume(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    volume: f32,
    muto: bool,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.volume = Volume {
            volume: volume.clamp(0.0, 1.0),
            muto,
        };
        lettore
            .motore
            .volume(lettore.volume.volume, lettore.volume.muto);
        // Il disco lo aggiorna il filo dell'orologio, entro un quarto di
        // secondo. Vedi `StatoLettore::volume_da_salvare`.
        stato
            .volume_da_salvare
            .store(true, std::sync::atomic::Ordering::Relaxed);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Cambia la curva dell'equalizzatore.
///
/// **Non manda [`StatoRiproduzione`]**, e non è una svista: comporre quello
/// stato richiede una lettura del brano corrente dal database, e questo comando
/// arriva una dozzina di volte al secondo finché un cursore è sotto il dito.
/// Parte invece `riproduzione:eq`, che sono due campi e nessuna query — quanto
/// basta perché il pannello nella barra e la pagina delle impostazioni restino
/// d'accordo fra loro.
#[tauri::command]
pub fn equalizzatore(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    guadagni: Vec<f32>,
    attivo: bool,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.motore.equalizzatore(&guadagni, attivo);
        // `playback::normalizza` fa qui il taglio che prima si andava a
        // rileggere dal database: è **la stessa** funzione che usa `save_eq`,
        // chiamata dove il valore nasce invece che dopo un giro su SQLite. La
        // finestra vede subito quel che rivedrà alla prossima apertura, e non
        // c'è più una scrittura più una rilettura per ogni pixel di cursore.
        lettore.eq = Equalizzazione {
            attivo,
            guadagni: playback::normalizza(&guadagni),
        };
        stato
            .eq_da_salvare
            .store(true, std::sync::atomic::Ordering::Relaxed);
        app.emetti(
            "riproduzione:eq",
            StatoEq {
                attivo: lettore.eq.attivo,
                guadagni: lettore.eq.guadagni.clone(),
            },
        );
        Ok(())
    })
    .map_err(errore)
}

/// Riapre il dispositivo audio.
///
/// # Perché è un comando e non un tentativo automatico
///
/// Perché riaprire non è gratis e non è sempre giusto. `cpal` apre il
/// dispositivo **predefinito di sistema**: se le cuffie si sono staccate e il
/// predefinito è tornato a essere gli altoparlanti del portatile, riaprire da
/// solo vuol dire far uscire la musica dagli altoparlanti — in un ufficio, di
/// notte, in una riunione. Chi ha staccato le cuffie sa se vuole che continui;
/// l'applicazione no.
///
/// C'è anche la ragione tecnica, ed è quella scritta in `uscita.rs`: riaprire
/// può bloccarsi, e non si può fare dal filo che segnala il guasto — quello è
/// un `StreamError` che arriva da dentro `cpal`.
///
/// # Cosa sopravvive
///
/// La coda, il volume, la curva dell'equalizzatore e la normalizzazione: sono
/// tutte cose del lettore, non del dispositivo, e ricostruirle dal database a
/// ogni riapertura le farebbe divergere da quel che l'utente ha davanti se una
/// scrittura fosse fallita. **La posizione no**: il motore nuovo nasce senza
/// niente aperto, e il brano riparte da capo — dirlo è meglio che far ripartire
/// una canzone di venti minuti dall'inizio senza avvisare.
///
/// Funziona anche quando il motore non si è **mai** aperto, che è il caso più
/// utile: l'applicazione avviata senza scheda audio è un catalogo consultabile
/// finché qualcuno non attacca le cuffie, e prima di oggi l'unico modo di
/// accorgersene era riavviare.
///
/// `(async)`: aprire un dispositivo audio parla con il sistema e può prendersi
/// il suo tempo. Sul filo principale sarebbe la finestra ferma.
#[tauri::command(async)]
pub fn riapri_audio(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    // Il motore nuovo si apre **prima** di buttare via quello vecchio: se
    // l'apertura fallisce, quel che c'era resta dov'è. Un riaprire fallito che
    // lascia il lettore peggio di come l'ha trovato è la cosa che un tasto
    // «riprova» non deve mai fare.
    //
    // E si apre **prima di chiedere il lucchetto**, non dopo. Aprire un
    // dispositivo audio parla con il sistema e può prendersi secondi: con il
    // lucchetto già in mano, pausa, volume e «prossimo» resterebbero tutti in
    // coda dietro la riapertura. `(async)` toglie di mezzo il filo principale,
    // non il lucchetto.
    let motore = apri_motore(&app).map_err(errore)?;

    let mut guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    match guardia.as_mut() {
        Ok(lettore) => {
            lettore.ascolto = None; // La posizione è persa: l'ascolto in corso non si può chiudere onestamente.
            lettore.motore = motore;
            lettore
                .motore
                .volume(lettore.volume.volume, lettore.volume.muto);
            lettore
                .motore
                .equalizzatore(&lettore.eq.guadagni, lettore.eq.attivo);
            lettore.motore.replaygain(
                lettore.normalizzazione.attivo,
                lettore.normalizzazione.bersaglio_db,
            );
            // Anche lo spettro: il lettore dentro il motore nuovo è nuovo pure
            // lui, e senza queste due righe chi stava guardando le barre le
            // vedrebbe fermarsi a zero dopo aver riaperto il dispositivo.
            lettore
                .motore
                .guarda_spettro(stato.spettro.load(std::sync::atomic::Ordering::Relaxed));
            lettore.motore.spettro_dettaglio(
                stato
                    .spettro_bande
                    .load(std::sync::atomic::Ordering::Relaxed),
            );
        }
        Err(_) => {
            // Il caso del motore mai aperto: si costruisce il lettore adesso, e
            // la coda di ieri la rimette `riprendi_coda` — che è la stessa
            // funzione dell'avvio, non una sua copia.
            *guardia = Ok(Lettore {
                motore,
                coda: Queue::new(),
                ascolto: None,
                volume: Volume::default(),
                eq: Equalizzazione::default(),
                normalizzazione: Normalizzazione::default(),
                // Come volume, curva e normalizzazione qui sopra: valori di
                // partenza che `riprendi_coda`, due righe più giù, rimpiazza
                // con quel che c'è scritto in `settings`.
                autoplay: false,
                dissolvenza_s: 0,
                motivo_prossimo: None,
            });
            drop(guardia);
            riprendi_coda(&app);
            let stato_lettore = app.state::<StatoLettore>();
            return con_lettore(&stato_lettore, |lettore| {
                manda_stato(&app, lettore);
                Ok(())
            })
            .map_err(errore);
        }
    }

    if let Ok(lettore) = guardia.as_mut() {
        manda_stato(&app, lettore);
    }
    Ok(())
}

/// Riprende il brano che una cartella di rete aveva interrotto.
///
/// È il tasto «Riprova» dell'avviso di rete. Riapre **lo stesso** brano e
/// torna al millisecondo a cui la musica si era fermata, che è tutto il punto:
/// una condivisione che sparisce a metà del secondo movimento non deve
/// costare il secondo movimento.
///
/// # Cosa succede se la rete è ancora giù
///
/// L'apertura fallisce di nuovo, con lo stesso errore ritentabile, e
/// l'annotazione **resta**: si può premere «Riprova» quante volte si vuole,
/// fino a quando il NAS si riaccende. Si cancella solo quando ha funzionato.
///
/// # Cosa succede se nel frattempo si è ascoltato altro
///
/// Niente. Se la coda non punta più a quel brano, chi ascolta ha già voltato
/// pagina e riportarcelo di forza sarebbe una musica che riparte da sola.
/// L'annotazione si butta e basta.
///
/// `(async)`: riapre un file che può stare su una cartella di rete, e quella è
/// esattamente l'attesa da cui la finestra va tenuta fuori.
#[tauri::command(async)]
pub fn riprova_corrente(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    let annotato = *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(ripresa) = annotato else {
        return Ok(());
    };

    // Il formato dell'uscita si legge sotto lucchetto e il lucchetto si lascia
    // subito: sono due interi da una struttura `Copy`, non un'apertura di file.
    let formato = con_lettore(&stato, |lettore| Ok(lettore.motore.formato())).map_err(errore)?;

    // Il brano si apre **prima di chiedere il lucchetto**, per la stessa
    // ragione scritta in [`riapri_audio`]: fin qui l'apertura ha già mostrato
    // di poterci mettere dei secondi, ed è la ragione per cui esiste questo
    // comando.
    let brano = brano_di(&app, ripresa.track_id, formato).map_err(errore)?;

    con_lettore(&stato, |lettore| {
        if lettore.coda.current() != Some(ripresa.track_id) {
            return Ok(());
        }
        lettore.motore.suona(brano);
        lettore.motore.vai_a(ripresa.ms);
        prepara_prossimo(&app, lettore);
        // Con la posizione richiesta e non con quella del motore: il salto lo
        // fa il filo della decodifica, e fino ad allora `posizione()`
        // direbbe zero. Stessa ragione di [`vai_a`].
        manda_stato_con_posizione(&app, lettore, ripresa.ms);
        Ok(())
    })
    .map_err(errore)?;

    // Si cancella qui e non prima: se l'apertura fosse fallita saremmo usciti
    // sopra con il `?`, e l'annotazione sarebbe rimasta per il tentativo dopo.
    // Arrivare fin qui vuol dire o che la musica è ripartita, o che la coda ha
    // voltato pagina: in tutti e due i casi non c'è più niente da riprendere.
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    Ok(())
}

/// Accende o spegne la normalizzazione ReplayGain.
///
/// # Cosa cambia davvero, e cosa no
///
/// Il guadagno lo applica `aether_play::guadagno` allo stadio del volume, e la
/// correzione esiste **solo per i brani che portano il tag**: su un file senza
/// `replaygain_track_gain` questo interruttore non sposta niente, in nessuna
/// delle due posizioni. È il motivo per cui il valore di serie è «acceso» —
/// sulla libreria misurata di questo progetto nessun file ha il tag, quindi
/// acceso e spento suonano identici finché non arriva un disco che ce l'ha, e
/// allora la cosa giusta da fare è rispettarlo.
///
/// Manda [`StatoRiproduzione`] e non un evento suo, al contrario
/// dell'equalizzatore: questo comando arriva quando un dito preme un
/// interruttore, cioè una volta ogni tanto, non dodici volte al secondo.
#[tauri::command]
pub fn normalizzazione(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    livello: String,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        let voluta = normalizzazione_da_nome(&livello);
        let stato_app = app.state::<Stato>();
        // Si rilegge quel che è stato scritto invece di fidarsi di quel che è
        // arrivato: è la disciplina dell'equalizzatore, e serve perché il
        // bersaglio passa per un taglio.
        let salvata = con_libreria(&stato_app, |libreria| {
            playback::save_replaygain(&libreria.connection, voluta)?;
            playback::load_replaygain(&libreria.connection)
        })
        .unwrap_or(voluta);
        lettore
            .motore
            .replaygain(salvata.attivo, salvata.bersaglio_db);
        lettore.normalizzazione = salvata;
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Le curve fra cui si può scegliere: prima quelle di serie, poi le proprie.
///
/// In quest'ordine perché è quello in cui si guardano: chi apre l'elenco la
/// prima volta non ha curve sue, e chi ne ha vuole trovarle in fondo, sempre
/// nello stesso posto, invece che mescolate alfabeticamente alle altre.
#[tauri::command]
pub fn eq_preset_elenco(stato: State<'_, Stato>) -> Esito<Vec<VocePreset>> {
    let mut elenco: Vec<VocePreset> = PRESET_DI_SERIE
        .iter()
        .map(|(nome, guadagni)| VocePreset {
            nome: (*nome).to_owned(),
            guadagni: guadagni.to_vec(),
            di_serie: true,
        })
        .collect();
    let miei = con_libreria(&stato, |libreria| {
        playback::load_preset_eq(&libreria.connection)
    })
    .map_err(errore)?;
    elenco.extend(miei.into_iter().map(|p| VocePreset {
        nome: p.nome,
        guadagni: p.guadagni,
        di_serie: false,
    }));
    Ok(elenco)
}

/// Salva la curva corrente sotto un nome.
///
/// `false` se il nome era vuoto. Un nome che c'è già sostituisce quella curva:
/// la regola sta in `aether_app::playback::salva_preset`, con la sua ragione.
#[tauri::command]
pub fn eq_preset_salva(
    stato: State<'_, Stato>,
    stato_lettore: State<'_, StatoLettore>,
    nome: String,
) -> Esito<bool> {
    let guadagni =
        con_lettore(&stato_lettore, |lettore| Ok(lettore.eq.guadagni.clone())).map_err(errore)?;
    con_libreria(&stato, |libreria| {
        playback::salva_preset(&libreria.connection, &nome, &guadagni)
    })
    .map_err(errore)
}

/// Toglie una curva salvata. `false` se non ce n'era una con quel nome.
///
/// Quelle di serie non passano di qui: non stanno nel database, quindi non c'è
/// niente da togliere e la finestra non ne offre il comando.
#[tauri::command]
pub fn eq_preset_cancella(stato: State<'_, Stato>, nome: String) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        playback::cancella_preset(&libreria.connection, &nome)
    })
    .map_err(errore)
}

/// Lo stato corrente, per quando la finestra si apre.
///
/// **Non fallisce quando il motore non si è aperto**, ed è l'unico comando di
/// questo file a comportarsi così. Gli altri sì, e devono: chiedere di suonare
/// a un lettore che non c'è è una richiesta senza risposta. Questo invece è la
/// domanda che la finestra fa per prima, e rispondere con un errore vorrebbe
/// dire una finestra che si apre su un guasto invece che su una libreria —
/// mentre senza scheda audio Aether resta un catalogo consultabile, che è
/// quello che `StatoLettore` conserva l'errore per permettere.
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
        }),
    }
}

/// Accoda in fondo.
#[tauri::command(async)]
pub fn coda_accoda(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        let era_vuota = lettore.coda.is_empty();
        lettore.coda.enqueue(&brani);
        if era_vuota {
            return Ok(true);
        }
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(false)
    })
    .map_err(errore)
}

/// Mette subito dopo il brano corrente.
#[tauri::command(async)]
pub fn coda_dopo(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    brani: Vec<i64>,
) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        let era_vuota = lettore.coda.is_empty();
        lettore.coda.play_next(&brani);
        if era_vuota {
            return Ok(true);
        }
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(false)
    })
    .map_err(errore)
}

/// Salta a una posizione della coda.
#[tauri::command(async)]
pub fn coda_vai(app: tauri::AppHandle, stato: State<'_, StatoLettore>, indice: usize) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        // L'indice arriva dalla coda **come si vede**, cioè nell'ordine di
        // riproduzione: con lo shuffle acceso non è quello dell'elenco interno.
        if lettore.coda.play_at(indice).is_none() {
            return Ok(false);
        }
        chiudi_ascolto(&app, lettore);
        Ok(true)
    })
    .map_err(errore)
}

/// Toglie dalla coda.
///
/// # Togliere quel che sta suonando
///
/// `Queue::remove` lascia il cursore dov'era — «non risuonarlo, non zitto
/// adesso» — e lì è scivolato il brano seguente: la finestra mostra quindi
/// subito il titolo del successivo. Senza le righe qui sotto, però, il motore
/// continuerebbe a far uscire i campioni del brano appena tolto: la barra
/// direbbe una cosa e le casse un'altra, e alla fine di quel brano la coda
/// avanzerebbe ancora, saltando il titolo che era stato annunciato. Si fa
/// quindi partire il nuovo corrente, come in [`coda_vai`], e l'ascolto del
/// brano tolto si chiude qui — prima che il motore attacchi il successivo —
/// perché è finito adesso.
///
/// Se però era l'**ultimo**, dietro di lui non è scivolato nessuno: il cursore
/// si aggrappa al brano di prima, che è già stato ascoltato, e farlo ripartire
/// sarebbe la sorpresa peggiore delle due. Con la ripetizione accesa un dopo
/// c'è comunque, ed è il primo della coda; senza, non resta che fermarsi.
#[tauri::command(async)]
pub fn coda_togli(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    indice: usize,
) -> Esito<()> {
    gesto_di_coda(&app, &stato, |lettore| {
        // Stesso spazio di coordinate di `coda_vai`: l'ordine di riproduzione.
        let suonava = lettore.coda.position() == Some(indice);
        let era_ultimo = indice.saturating_add(1) >= lettore.coda.len();
        lettore.coda.remove_at(indice);
        if !suonava {
            prepara_prossimo(&app, lettore);
            salva_coda(&app, lettore);
            manda_stato(&app, lettore);
            return Ok(false);
        }
        chiudi_ascolto(&app, lettore);
        if !era_ultimo {
            return Ok(true);
        }
        if lettore.coda.repeat() == RepeatMode::All && lettore.coda.play_at(0).is_some() {
            return Ok(true);
        }
        lettore.motore.ferma();
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(false)
    })
    .map_err(errore)
}

/// Sposta un brano dentro la coda.
#[tauri::command(async)]
pub fn coda_riordina(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    da: usize,
    a: usize,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.coda.reorder(da, a);
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Svuota la coda e ferma tutto.
#[tauri::command]
pub fn coda_svuota(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        chiudi_ascolto(&app, lettore);
        lettore.coda.clear();
        lettore.motore.ferma();
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Passa al modo di ripetizione successivo.
#[tauri::command(async)]
pub fn ripeti(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.coda.cycle_repeat();
        // Il prossimo cambia: con «ripeti uno» è di nuovo questo.
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Accende o spegne lo shuffle.
#[tauri::command(async)]
pub fn mescola(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.coda.toggle_shuffle(seme());
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Le righe di una lista di identificativi, per il pannello della coda.
///
/// L'ordine chiesto si conserva: la coda non è ordinata come il database, e
/// restituirla ordinata per `id` vorrebbe dire mostrare all'utente un elenco che
/// non è quello che sentirà.
#[tauri::command]
pub fn brani_per_id(stato: State<'_, Stato>, brani: Vec<i64>) -> Esito<Vec<TrackSummary>> {
    con_libreria(&stato, |libreria| {
        aether_app::library::summaries_by_id(&libreria.connection, &brani)
    })
    .map_err(errore)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// I quattro nomi vanno e tornano.
    ///
    /// La prova che conta davvero è il ritorno: `nome_normalizzazione` non fa
    /// un confronto esatto ma prende il più vicino, e un bersaglio nuovo
    /// aggiunto in mezzo agli altri potrebbe rubare il nome a uno dei tre
    /// senza che niente smetta di compilare.
    #[test]
    fn i_livelli_di_normalizzazione_vanno_e_tornano() {
        for nome in ["spento", "basso", "normale", "alto"] {
            let livello = normalizzazione_da_nome(nome);
            assert_eq!(
                nome_normalizzazione(livello),
                nome,
                "andata e ritorno di «{nome}»"
            );
        }
    }

    /// Un nome che non conosciamo vale «normale», non un guasto.
    #[test]
    fn un_livello_sconosciuto_vale_il_riferimento() {
        let livello = normalizzazione_da_nome("fortissimo");
        assert!(livello.attivo);
        assert_eq!(nome_normalizzazione(livello), "normale");
    }

    /// Spento conserva un bersaglio valido a cui tornare.
    ///
    /// Se spegnere scrivesse uno zero, riaccendere porterebbe tutto a 0 dB —
    /// diciotto decibel sopra il riferimento, cioè un salto di volume che
    /// nessuno ha chiesto.
    #[test]
    fn spento_non_perde_il_bersaglio() {
        let spento = normalizzazione_da_nome("spento");
        assert!(!spento.attivo);
        assert!(
            spento.bersaglio_db.is_finite() && spento.bersaglio_db < 0.0,
            "spento ha lasciato un bersaglio insensato: {}",
            spento.bersaglio_db
        );
    }

    // ── il brano che non è un file ──────────────────────────────────────────

    /// Un percorso resta un percorso.
    ///
    /// La prova che conta di più delle altre: qui dentro passa **ogni** brano
    /// della libreria, e un riconoscimento troppo largo vorrebbe dire un file
    /// del disco mandato a cercare in rete. Le forme sono quelle vere di
    /// Windows, più le due che somigliano di più a un indirizzo.
    #[test]
    fn un_percorso_di_disco_non_diventa_mai_un_flusso() {
        for percorso in [
            r"C:\Musica\Pink Floyd\Animals\01 - Pigs on the Wing.flac",
            r"\\nas\musica\raccolta\02 - Dogs.mp3",
            r"D:\archive.org\download\gd77\t01.flac",
            "C:/Musica/https/brano.mp3",
            "",
            "   ",
        ] {
            assert_eq!(
                riferimento_da_testo(percorso),
                None,
                "«{percorso}» è un file, non un flusso"
            );
        }
    }

    /// Un indirizzo di catalogo sì, e con l'indirizzo intatto.
    ///
    /// L'indirizzo **non** si riscrive: `riconosci` sa ricavare da un link
    /// l'identificativo del concerto, ma i byte da suonare sono quel file lì, e
    /// sostituirlo con la pagina dell'item vorrebbe dire suonare dell'HTML.
    #[test]
    fn un_indirizzo_di_catalogo_diventa_un_flusso_senza_essere_riscritto() {
        let file = "https://archive.org/download/gd1977-05-08/gd77-05-08d1t01.flac";
        assert_eq!(
            riferimento_da_testo(file),
            Some(RiferimentoFlusso {
                fonte: Fonte::InternetArchive,
                url: file.to_owned(),
            })
        );

        // Audius conserva un percorso e non un indirizzo, apposta: il nodo di
        // oggi fra un mese non c'è più. Vedi [`PERCORSO_AUDIUS`].
        let percorso = "/v1/tracks/aB3dE/stream?ext=wav";
        assert_eq!(
            riferimento_da_testo(percorso),
            Some(RiferimentoFlusso {
                fonte: Fonte::Audius,
                url: percorso.to_owned(),
            })
        );
    }

    /// Dove non si è invitati non si bussa.
    ///
    /// Compreso il suffisso che somiglia: `evilarchive.org` finisce per
    /// `archive.org`, e la ragione per cui non passa sta in
    /// `aether_catalogo::riferimento`. Questa prova esiste per accorgersi il
    /// giorno in cui questo modulo smettesse di passare da quel cancello.
    #[test]
    fn un_dominio_che_non_ci_ha_invitati_non_diventa_un_flusso() {
        for indirizzo in [
            "https://esempio.invalido/musica/brano.mp3",
            "https://evilarchive.org/download/gd77/t01.flac",
            "https://archive.org/",
            // In chiaro non si esce: `Rete` nasce `https_only`, e riconoscerlo
            // qui vorrebbe dire promettere un'apertura che fallirebbe più in là
            // dicendo un'altra cosa.
            "http://archive.org/download/gd77/t01.flac",
        ] {
            assert_eq!(
                riferimento_da_testo(indirizzo),
                None,
                "«{indirizzo}» non è un posto dove Aether sia invitato"
            );
        }
    }

    /// Il suggerimento di formato viene dal percorso, mai dalla query.
    ///
    /// Il caso che conta è il terzo: su Audius `?ext=wav` dice il formato del
    /// file **originale**, mentre dal punto di ascolto arriva un mp3
    /// transcodificato. Un suggerimento falso è peggio di nessun suggerimento.
    #[test]
    fn il_formato_si_indovina_dal_percorso_e_non_dalla_query() {
        assert_eq!(
            estensione_da_url("https://archive.org/download/gd77/t01.FLAC").as_deref(),
            Some("flac")
        );
        assert_eq!(
            estensione_da_url("https://archive.org/download/gd77/t01.mp3?x=1").as_deref(),
            Some("mp3")
        );
        assert_eq!(
            estensione_da_url("https://nodo.esempio/v1/tracks/aB3/stream?ext=wav"),
            None
        );
        assert_eq!(estensione_da_url("https://archive.org/download/gd77"), None);
        // Un «punto» che è in mezzo al dominio e non nel nome del file.
        assert_eq!(estensione_da_url("https://archive.org"), None);
    }

    /// Una fonte che non consegna byte lo dice, senza chiedere niente a nessuno.
    ///
    /// Le prove girano senza rete, ed è ciò che rende questa prova capace di
    /// dire qualcosa: se il rifiuto arrivasse *dopo* la richiesta, qui uscirebbe
    /// un errore di trasporto invece di `playback.sourceUnavailable`.
    #[test]
    fn da_una_fonte_che_non_consegna_non_parte_nessuna_richiesta() {
        let cataloghi = Cataloghi::nuovi();
        let esito = apri_flusso(
            &cataloghi,
            &RiferimentoFlusso {
                fonte: Fonte::ArchivioSpotify,
                url: "https://archivio.esempio/brano".to_owned(),
            },
        );
        assert!(
            matches!(
                esito.as_ref().map_err(AppError::code),
                Err(ErrorCode::PlaybackSourceUnavailable { .. })
            ),
            "invece di rifiutare ha risposto: {:?}",
            esito.map(|_| ()).map_err(|e| e.code().kind().code())
        );
    }

    /// Un file finto in memoria, al posto di un catalogo.
    ///
    /// Passa dal tratto `Sorgente` di `aether-net`, che esiste esattamente per
    /// questo: provare il cablaggio contro un servizio vero vorrebbe dire non
    /// provarlo.
    #[derive(Debug)]
    struct FintaRete(Vec<u8>);

    impl aether_net::flusso::Sorgente for FintaRete {
        fn pezzo(&self, da: u64, quanti: u64) -> Result<aether_net::Pezzo, AppError> {
            let inizio = usize::try_from(da).unwrap_or(usize::MAX);
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap_or(0))
                .min(self.0.len());
            Ok(aether_net::Pezzo {
                byte: self.0.get(inizio..fine).unwrap_or(&[]).to_vec(),
                totale: Some(u64::try_from(self.0.len()).unwrap_or(0)),
            })
        }
    }

    /// Quel che arriva al motore è indistinguibile da un file.
    ///
    /// È la prova del cablaggio intero, meno la rete: identificativo, durata dal
    /// database, guadagno, suggerimento di formato, lunghezza dichiarata — e i
    /// byte, che devono essere quelli e nell'ordine giusto. La lunghezza in
    /// particolare non è un di più: `MediaSource::is_seekable` di symphonia
    /// risponde guardando quella, e senza il cursore sparisce dalla barra.
    #[test]
    fn un_flusso_diventa_una_sorgente_che_il_motore_sa_suonare() {
        let byte: Vec<u8> = (0..3000_u32)
            .map(|n| u8::try_from(n % 251).unwrap_or(0))
            .collect();
        let Ok(flusso) = FlussoHttp::da(Box::new(FintaRete(byte.clone()))) else {
            panic!("la finta non fallisce mai");
        };
        let scheda = SchedaSorgente {
            track_id: 4242,
            path: "https://archive.org/download/gd1977-05-08/gd77d1t01.flac".to_owned(),
            durata_ms: 754_000,
            replaygain_db: Some(-7.5),
        };

        let mut sorgente = sorgente_da_flusso(&scheda, &scheda.path, flusso);
        assert_eq!(sorgente.track_id, 4242, "il brano ha perso il suo nome");
        assert_eq!(
            sorgente.durata_ms, 754_000,
            "la durata la dice il database, non il flusso"
        );
        assert_eq!(sorgente.estensione.as_deref(), Some("flac"));
        assert!(
            sorgente
                .replaygain_db
                .is_some_and(|db| (db + 7.5).abs() < 0.001),
            "il guadagno non è arrivato: {:?}",
            sorgente.replaygain_db
        );
        assert_eq!(
            sorgente.media.lunghezza(),
            Some(3000),
            "senza lunghezza il cursore sparisce dalla barra"
        );

        let mut letti = Vec::new();
        let quanti = std::io::Read::read_to_end(&mut sorgente.media, &mut letti).unwrap_or(0);
        assert_eq!(quanti, 3000);
        assert_eq!(letti, byte, "i byte non sono quelli, o sono spostati");
    }
}
