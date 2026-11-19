//! I controlli multimediali del sistema operativo.
//!
//! # Cosa mancava
//!
//! Rymembra non parlava in nessun modo con Windows. Fra i comandi registrati in
//! [`crate::run`] non ce n'era uno che uscisse dalla finestra: il tasto
//! play/pausa della tastiera non faceva niente, nel riquadro del volume di
//! Windows 11 non compariva nessuna scheda, la barra delle applicazioni non
//! mostrava anteprima né comandi, e le cuffie Bluetooth non comandavano nulla.
//! Un lettore musicale sta in secondo piano quasi tutto il tempo che è acceso,
//! e per tutto quel tempo era irraggiungibile se non tornandoci sopra col
//! mouse.
//!
//! Questo modulo chiude la scheda nel riquadro del volume e i tasti della
//! tastiera e delle cuffie. I tre pulsanti sotto l'anteprima nella barra delle
//! applicazioni sono un'altra cosa — le SMTC non li danno — e li mette
//! `crate::barra`, con gli stessi gesti di qui: vedi [`premi`].
//!
//! # Una registrazione sola per due cose
//!
//! Le `SystemMediaTransportControls` non sono soltanto la scheda che si vede:
//! registrandole è **Windows** a instradare i tasti multimediali
//! all'applicazione che sta suonando. Per questo qui non c'è nessuna
//! scorciatoia globale — una scorciatoia registrata su `MediaPlayPause`
//! ruberebbe quel tasto a ogni altro programma, per sempre, anche a lettore
//! fermo, ed è esattamente il difetto che rende insopportabili i lettori che
//! lo fanno.
//!
//! # Perché una dipendenza
//!
//! Il perché sta per esteso in `Cargo.toml`, accanto alla riga: la radice del
//! workspace vieta `unsafe` con `forbid`, che non si scavalca con un
//! `#[allow]`, e parlare di COM col sistema operativo è `unsafe` per
//! definizione. Il puntatore della finestra lo si **passa** e basta — passare
//! un puntatore grezzo non è `unsafe`, lo è dereferenziarlo — e a
//! dereferenziarlo ci pensa `souvlaki`.
//!
//! # Dove si aggancia
//!
//! In un punto solo: [`crate::riproduzione::manda_stato`], che è già l'unica
//! sorgente dello stato verso la finestra. Se la scheda del sistema si
//! aggiornasse da un posto suo, prima o poi mostrerebbe un brano diverso da
//! quello che suona — ed è il tipo di difetto che nessuno segnala, perché chi
//! lo vede pensa di aver letto male.
//!
//! # Tre difetti che la 3.0 ha chiuso
//!
//! **La scheda restava vuota per ogni brano con la copertina.** Il percorso
//! della copertina partiva come `file:///C:/…`; `souvlaki` toglie soltanto
//! `file://` e passa a Windows `/C:/…`, che `StorageFile::GetFileFromPathAsync`
//! rifiuta. L'errore usciva col `?` **prima** di `DisplayUpdater::Update`, cioè
//! prima che titolo, artista, album e durata venissero consegnati, e qui lo si
//! scartava in silenzio. Adesso l'indirizzo è quello che `souvlaki` sa
//! trasformare in un percorso vero: vedi [`url_di_file`].
//!
//! **A coda finita restava l'ultimo brano.** `souvlaki` scrive solo i campi che
//! riceve: `MediaMetadata::default()` non azzerava niente. Adesso si mandano
//! stringhe vuote.
//!
//! **La copertina si ricaricava a ogni pausa.** Ogni stato passava la
//! copertina, e ogni copertina è un file aperto e un'attesa del sistema: a ogni
//! pausa, salto o ritocco della coda. Adesso la scheda si riscrive solo quando
//! cambia, e la copertina solo quando cambia lei.
//!
//! E i tasti non fanno più aspettare il sistema: arrivano su un filo di
//! Windows, e un «prossimo» su un brano che arriva dalla rete può durare
//! quindici secondi. Vedi [`premi`].

//!
//! # Due sistemi, un modulo
//!
//! Quel che vale ovunque sta qui: lo stato che i gesti leggono (dove sta il
//! brano, quanto dura), il filo che li esegue in ordine, e la regola dei salti
//! che non escono dal brano. Quel che parla col sistema operativo sta in
//! [`sistema`], che su Windows è `smtc.rs` — le SMTC via `souvlaki` — e su
//! Android `android.rs`, la sessione multimediale. Tutte e due consegnano i
//! tasti a [`premi`], e tutte e due ricevono lo stato da [`aggiorna`].

#[cfg_attr(desktop, path = "smtc.rs")]
#[cfg_attr(mobile, path = "android.rs")]
mod sistema;

pub(crate) mod lontane;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use tauri::Manager as _;

use crate::nota;
use crate::riproduzione::{StatoLettore, StatoRiproduzione};
use crate::spegnimento::Emette as _;

pub use sistema::{avvia, stacca};

/// Di quanto si sposta un `Seek` senza durata.
///
/// Gli stessi cinque secondi delle frecce in [`crate::riproduzione`]: il gesto
/// è lo stesso, e due passi diversi per la stessa intenzione si notano.
#[cfg_attr(
    mobile,
    allow(dead_code, reason = "Android manda i salti già misurati")
)]
const PASSO_SALTO_MS: u64 = 5_000;

/// I controlli del sistema, e quel che serve per riempirli.
///
/// # Perché la copertina sta qui dentro
///
/// SMTC vuole un **file vero**, non lo schema `rymembra-cover` con cui la
/// finestra chiede le copertine: quel protocollo lo serve l'applicazione, e il
/// sistema operativo non lo sa parlare. Il percorso si ricava da
/// [`CoverStore::path_for`], che è deterministico, e lo `CoverStore` è
/// `Clone` — quindi se ne tiene una copia presa una volta all'avvio.
///
/// L'alternativa era prendere il lucchetto della libreria a ogni cambio di
/// stato solo per leggere una cartella che non cambia mai. Con l'ordine dei
/// lucchetti che vige qui — prima il lettore, poi la libreria — sarebbe stata
/// una presa in più dentro una sezione già critica, per un valore costante.
pub struct CorpoMedia {
    /// I controlli, se il sistema li ha concessi.
    ///
    /// `Option` e non `Result`: che i controlli non ci siano non è un guasto da
    /// mostrare. Su una macchina che non li offre il lettore funziona come
    /// prima, e l'unica differenza è che la scheda nel riquadro del volume non
    /// compare.
    controlli: Mutex<Option<sistema::Controlli>>,
    /// L'ultima posizione nota, in millisecondi.
    ///
    /// Serve solo ai salti relativi, che il sistema manda senza dire da dove.
    /// In un atomico e non dentro il mutex perché chi lo legge è la chiusura
    /// degli eventi, che gira su un filo suo mentre lo scrivono [`aggiorna`] e
    /// [`segna_posizione`] — e prendere un lucchetto per leggere un intero
    /// significherebbe che un tasto premuto aspetta il disegno di uno stato.
    ///
    /// Lo scrivono in due, e non è la doppia sorgente che il modulo evita
    /// altrove: la scheda che si **vede** la compone solo [`aggiorna`]. Questo
    /// numero è un'altra cosa — dove sta il brano adesso — e lo stato completo
    /// parte a ogni cambio, non mentre la musica scorre. Con il solo
    /// [`aggiorna`] restava quindi fermo al valore dell'ultimo cambio, di
    /// solito lo zero dell'inizio del brano, e ogni «avanti di dieci secondi»
    /// dalle cuffie riportava a dieci secondi dall'inizio invece che dieci
    /// secondi più avanti.
    posizione_ms: AtomicU64,
    /// Quanto dura il brano corrente, in millisecondi.
    ///
    /// Accanto alla posizione, e per la stessa ragione: la legge la chiusura
    /// degli eventi, sul filo del sistema operativo, e serve a non far uscire
    /// dal brano un gesto che voleva restarci. «Avanti di cinque secondi» a
    /// tre secondi dalla fine chiedeva un salto oltre l'ultimo campione, e un
    /// salto oltre la fine — o **sulla** fine — è la fine: il gesto diventava
    /// «brano successivo» senza che nessuno l'avesse chiesto. Per questo il
    /// tetto dei salti relativi sta un margine prima: vedi [`avanti_entro`].
    ///
    /// `0` vuol dire **non si sa** — niente è ancora partito — e non «lungo
    /// zero»: con un tetto a zero ogni salto in avanti tornerebbe all'inizio,
    /// che è un difetto peggiore di quello che il tetto viene a togliere.
    durata_ms: AtomicU64,
    /// Le corsie dei fili che eseguono i tasti, o `None` se non sono partiti.
    /// Vedi [`premi`].
    tasti: OnceLock<Option<Corsie>>,
}

crate::nucleo::condiviso! {
    /// I controlli del sistema: un handle `Clone` sul [`CorpoMedia`], perché il nucleo e Tauri
    /// lo possiedono insieme (vedi `nucleo`).
    pub struct StatoMedia(CorpoMedia);
}

impl StatoMedia {
    /// Uno stato spento, da riempire con [`avvia`].
    #[must_use]
    pub fn nuovo() -> Self {
        Self::da_corpo(CorpoMedia {
            controlli: Mutex::new(None),
            posizione_ms: AtomicU64::new(0),
            durata_ms: AtomicU64::new(0),
            tasti: OnceLock::new(),
        })
    }
}

/// Rispecchia lo stato del lettore nella scheda del sistema.
///
/// Chiamata da [`crate::riproduzione::manda_stato`] con lo stato **già
/// composto**: comporlo una seconda volta vorrebbe dire una seconda lettura
/// del brano dal database a ogni cambio, e soprattutto due risposte possibili
/// alla stessa domanda.
///
/// Gira sul filo del seguito del lettore, non dentro il suo lucchetto: la
/// consegna della copertina aspetta il sistema, e prima si aspettava con il
/// lettore chiuso. Vedi `crate::riproduzione::in_ordine`.
pub fn aggiorna(app: &tauri::AppHandle, stato: &StatoRiproduzione) {
    // `try_state` e non `state`: quest'ultima **va in panico** se lo stato non
    // è registrato, e chi chiama è anche il filo dell'orologio, che parte
    // insieme al lettore. L'ordine in `main` è già quello giusto, ma un ordine
    // giusto è una cosa che si può cambiare per sbaglio fra un anno; una
    // riproduzione che si interrompe perché la scheda del volume non era
    // pronta, no.
    let Some(media) = app.try_state::<StatoMedia>() else {
        return;
    };
    media
        .posizione_ms
        .store(stato.posizione_ms, Ordering::Relaxed);
    // Da qui e non dalla scheda del sistema: la scheda la si riempie qualche
    // riga più giù e non la si può rileggere — `souvlaki` prende e non
    // racconta. Questo è lo stesso numero, tenuto dove serve ai salti.
    media.durata_ms.store(stato.durata_ms, Ordering::Relaxed);

    sistema::aggiorna(app, &media, stato);
}

/// Aggiorna dove sta il brano, senza toccare la scheda.
///
/// La chiama l'orologio di [`crate::riproduzione`], lo stesso giro che manda
/// `riproduzione:tempo` alla finestra: quattro volte al secondo, e senza
/// ricomporre niente. Serve ai salti relativi — vedi
/// [`StatoMedia::posizione_ms`] — e non ridisegna la scheda del sistema, che
/// interpola per conto suo dall'ultimo [`MediaPlayback`] ricevuto.
pub fn segna_posizione(app: &tauri::AppHandle, ms: u64) {
    // `try_state` per la stessa ragione di [`aggiorna`]: l'orologio parte
    // insieme al lettore, e questo modulo può non essere ancora registrato.
    if let Some(media) = app.try_state::<StatoMedia>() {
        media.posizione_ms.store(ms, Ordering::Relaxed);
    }
}

/// Un gesto sulla musica, da eseguire lontano da chi l'ha ricevuto.
///
/// Li mandano i tasti della scheda di Windows, il menù dell'icona nel vassoio
/// e i pulsanti nell'anteprima della barra: vedi [`premi`].
///
/// Su Android anche quelli che partono dall'auto ([`Gesto::Auto`]): «suona questo
/// album» deve stare in fila con il «riprendi» che l'auto manda subito dopo.
#[derive(Debug, Clone)]
#[cfg_attr(desktop, derive(Copy))]
pub(crate) enum Gesto {
    /// Riprendi.
    Riprendi,
    /// Pausa.
    Pausa,
    /// Pausa se suona, riprendi se è fermo.
    #[cfg_attr(
        mobile,
        allow(
            dead_code,
            reason = "Media3 manda «riproduci» e «pausa», mai «alterna»"
        )
    )]
    Alterna,
    /// Il brano dopo.
    Prossimo,
    /// Il brano prima, o da capo.
    Precedente,
    /// A questo millisecondo del brano.
    VaiA(u64),
    /// Un gesto dello schermo dell'auto: suona una voce, cerca, salta nella
    /// coda, shuffle, ripeti, mi piace. Vedi [`crate::auto`].
    #[cfg(mobile)]
    Auto(crate::auto::GestoAuto),
}

/// Esegue un gesto sul filo «rymembra-tasti», dietro a quelli arrivati prima.
///
/// # Perché un filo
///
/// Perché chi riceve il gesto non può aspettare. I tasti della scheda di
/// Windows arrivano su un filo del sistema operativo, dentro il gestore che
/// `souvlaki` gli ha registrato; il menù del vassoio arriva sul filo della
/// finestra. E i comandi che eseguono il gesto possono aspettare parecchio:
/// «prossimo» apre un file, e un brano che arriva dalla rete ha quindici
/// secondi di scadenza. Per tutto quel tempo il sistema restava con un tasto
/// in mano, o la finestra ferma.
///
/// Due fili, e non uno per gesto: due tasti premuti di fila — pausa e subito
/// play — devono valere nell'ordine in cui sono stati premuti. Uno per i
/// gesti in fila e uno per la pausa, che non aspetta: vedi [`avvia_corsie`].
///
/// # Sorvegliati
///
/// Ogni gesto gira dentro `crate::stato::sorvegliato`, come ogni altro filo
/// del lettore: un panico in un gesto si portava via il filo in silenzio, e
/// da lì ogni tasto si eseguiva sul filo che l'aveva ricevuto — il callback
/// delle SMTC, dove un secondo panico abortisce il processo, o la finestra,
/// ferma su un «successivo» dalla rete (R23-10). Senza fili il gesto si
/// perde con una riga nel diario.
///
/// Parte al primo gesto, non all'avvio: chi non tocca mai un tasto non paga un
/// filo.
pub(crate) fn premi(app: &tauri::AppHandle, gesto: Gesto) {
    let Some(media) = app.try_state::<StatoMedia>() else {
        return;
    };
    let corsie = media.tasti.get_or_init(|| {
        let mano = app.clone();
        let chi_suona = app.clone();
        let chi_aspetta = app.clone();
        let corsie = avvia_corsie(
            move |gesto| {
                let _ = crate::stato::sorvegliato(FILO_TASTI, || esegui(&mano, gesto));
            },
            move || {
                chi_suona
                    .try_state::<StatoLettore>()
                    .is_some_and(|stato| crate::riproduzione::suona_adesso(&stato))
            },
            move |gesto| {
                if !vuole_il_motore(gesto) {
                    return;
                }
                let Some(stato) = chi_aspetta.try_state::<StatoLettore>() else {
                    return;
                };
                // Il motore non si è aperto entro l'attesa: il gesto prosegue
                // lo stesso, e dice lui l'errore vero. Una riga, perché sullo
                // schermo dell'auto l'errore è una frase sola.
                if !crate::riproduzione::aspetta_il_primo_motore(&stato) {
                    nota!("[media] un gesto ha aspettato il primo motore e non si è aperto");
                }
            },
        );
        if corsie.is_none() {
            nota!("[media] i fili dei tasti non sono partiti: i tasti non comandano niente");
        }
        corsie
    });
    // Senza fili il gesto si perde, con una riga: eseguirlo qui voleva dire
    // eseguirlo sul filo del sistema — il callback delle SMTC, dove un panico
    // abortisce il processo — o su quello della finestra, fermo fino a
    // quindici secondi su un «successivo» dalla rete (R23-10).
    if !corsie.as_ref().is_some_and(|corsie| corsie.manda(gesto)) {
        nota!("[media] un tasto si è perso: i fili dei tasti non ci sono");
    }
}

/// Il gesto ha bisogno del motore audio: se si sta ancora aprendo, aspetta.
///
/// Tutti tranne la pausa — che non ha niente da fermare, e sta nella corsia che
/// non aspetta — e il «mi piace» dell'auto, che scrive nella libreria e non
/// tocca il lettore. Vedi [`crate::riproduzione::aspetta_il_primo_motore`].
fn vuole_il_motore(gesto: &Gesto) -> bool {
    match gesto {
        Gesto::Pausa => false,
        #[cfg(mobile)]
        Gesto::Auto(crate::auto::GestoAuto::Piace { .. }) => false,
        _ => true,
    }
}

/// Il nome del filo che esegue i gesti in fila: nel diario dei panici e nella
/// salute dei fili.
const FILO_TASTI: &str = "rymembra-tasti";

/// Il nome del filo della corsia che non aspetta: vedi [`Corsie`].
const FILO_PAUSA: &str = "rymembra-tasti-pausa";

/// Da quale corsia passa un gesto: vedi [`Corsie`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corsia {
    /// Non aspetta niente: la pausa, e «alterna» quando la musica va.
    Subito,
    /// Dietro ai gesti arrivati prima, nell'ordine in cui sono stati premuti.
    InFila,
}

/// La corsia di un gesto.
const fn corsia(gesto: &Gesto) -> Corsia {
    match gesto {
        Gesto::Pausa | Gesto::Alterna => Corsia::Subito,
        _ => Corsia::InFila,
    }
}

/// Le due corsie dei gesti, e il numero che le tiene in ordine.
pub(crate) struct Corsie {
    /// Il numero dell'ultimo gesto mandato: cresce a ogni tasto, in tutte e
    /// due le corsie.
    numero: AtomicU64,
    fila: Sender<(u64, Gesto)>,
    subito: Sender<(u64, Gesto)>,
}

impl Corsie {
    /// Manda un gesto nella sua corsia. `false` se il filo non c'è più.
    fn manda(&self, gesto: Gesto) -> bool {
        let numero = self.numero.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        let verso = match corsia(&gesto) {
            Corsia::Subito => &self.subito,
            Corsia::InFila => &self.fila,
        };
        verso.send((numero, gesto)).is_ok()
    }
}

/// Fa partire i fili delle corsie. `esegui` fa il gesto; `suona` dice se la
/// musica sta andando, per «alterna»; `aspetta` è l'attesa del motore audio che
/// si sta ancora aprendo, che ogni gesto in fila fa **prima** del suo turno.
/// `None` se un filo non parte.
///
/// # Perché l'attesa sta fuori dal turno
///
/// Un «riprendi» che arriva mentre il primo motore si apre — l'auto che si
/// collega a nucleo appena partito — aspetta fino a dieci secondi invece di
/// fallire (vedi [`crate::riproduzione::aspetta_il_primo_motore`]). Se
/// aspettasse col turno in mano, la pausa — che sta nella corsia veloce e prende
/// lo stesso turno — resterebbe ferma lo stesso tempo: le cuffie staccate, o il
/// tasto pausa dell'auto, partirebbero dopo la ripresa, e la musica comincerebbe
/// proprio mentre li si è fermati (R23-3). Aspettando fuori, una pausa arrivata
/// nel frattempo alza il numero del turno, e la ripresa — già in fila con un
/// numero più basso — non si esegue: vince l'ultimo tasto premuto.
///
/// # Perché due corsie
///
/// Perché la pausa non ha niente da aspettare, e in fila aspettava: dietro un
/// «successivo» che apre un brano da un NAS che dorme, da un catalogo o dal
/// cloud — fino a quindici secondi, quaranta con un'uscita da riaprire — il
/// brano di prima continuava a suonare. Sul telefono anche la pausa di
/// `BECOMING_NOISY`, cioè le cuffie staccate e la musica dall'altoparlante
/// (R23-3). L'ordine fra una pausa e un brano che si sta aprendo lo tiene già
/// `Lettore::pausa_chiesta` alla consegna: il brano nuovo parte fermo.
///
/// # L'ordine fra le corsie
///
/// Ogni gesto porta un numero, e la pausa vince sulle riprese premute
/// **prima** di lei: una ripresa rimasta in fila dietro al gesto lento, e
/// arrivata al suo turno dopo la pausa, non la disfa. Pausa e riprese si
/// eseguono anche una alla volta — sotto `ultima_pausa` — così una ripresa
/// già cominciata finisce prima che la pausa arrivata dopo la fermi. Gli
/// altri gesti in fila — «successivo», «precedente», un salto — non
/// aspettano la pausa né la pausa aspetta loro.
///
/// «Alterna» passa dalla corsia veloce: se la musica va è una pausa, e si fa
/// lì; se no è una ripresa, e torna in fila col suo numero.
fn avvia_corsie(
    esegui: impl Fn(Gesto) + Send + Sync + 'static,
    suona: impl Fn() -> bool + Send + 'static,
    aspetta: impl Fn(&Gesto) + Send + 'static,
) -> Option<Corsie> {
    let esegui = Arc::new(esegui);
    // Il numero dell'ultima pausa fatta dalla corsia veloce, e il turno fra
    // pause e riprese.
    let ultima_pausa = Arc::new(Mutex::new(0_u64));
    let (fila, in_fila) = std::sync::mpsc::channel::<(u64, Gesto)>();
    let (subito, di_corsa) = std::sync::mpsc::channel::<(u64, Gesto)>();
    {
        let esegui = Arc::clone(&esegui);
        let ultima_pausa = Arc::clone(&ultima_pausa);
        std::thread::Builder::new()
            .name(FILO_TASTI.to_owned())
            .spawn(move || {
                for (numero, gesto) in in_fila {
                    // Fuori dal turno: vedi sopra, «Perché l'attesa sta fuori».
                    aspetta(&gesto);
                    if !matches!(gesto, Gesto::Riprendi | Gesto::Alterna) {
                        esegui(gesto);
                        continue;
                    }
                    let turno = ultima_pausa.lock().unwrap_or_else(PoisonError::into_inner);
                    if numero < *turno {
                        continue;
                    }
                    esegui(gesto);
                    drop(turno);
                }
            })
            .ok()?;
    }
    let in_fila = fila.clone();
    std::thread::Builder::new()
        .name(FILO_PAUSA.to_owned())
        .spawn(move || {
            for (numero, gesto) in di_corsa {
                let mut turno = ultima_pausa.lock().unwrap_or_else(PoisonError::into_inner);
                if matches!(gesto, Gesto::Pausa) || suona() {
                    *turno = (*turno).max(numero);
                    esegui(gesto);
                } else {
                    drop(turno);
                    let _ = in_fila.send((numero, gesto));
                }
            }
        })
        .ok()?;
    Some(Corsie {
        numero: AtomicU64::new(0),
        fila,
        subito,
    })
}

/// Il gesto, fatto con i comandi che esistono già.
///
/// Ogni ramo passa dai comandi della finestra invece di toccare il lettore:
/// così un tasto della tastiera, una voce del vassoio e il pulsante nella
/// finestra fanno **letteralmente** la stessa cosa, salvataggio della coda e
/// invio dello stato compresi.
fn esegui(app: &tauri::AppHandle, gesto: Gesto) {
    let Some(lettore) = app.try_state::<StatoLettore>() else {
        return;
    };
    let esito = match gesto {
        Gesto::Riprendi => crate::riproduzione::riprendi_ora(app.clone(), lettore),
        Gesto::Pausa => crate::riproduzione::pausa_ora(app.clone(), lettore),
        Gesto::Alterna => crate::riproduzione::alterna_ora(app.clone(), lettore),
        Gesto::Prossimo => crate::riproduzione::prossimo_ora(app.clone(), lettore),
        Gesto::Precedente => crate::riproduzione::precedente_ora(app.clone(), lettore),
        Gesto::VaiA(ms) => crate::riproduzione::vai_a_ora(app.clone(), lettore, ms),
        #[cfg(mobile)]
        Gesto::Auto(gesto) => crate::auto::esegui(app, gesto),
    };
    if let Err(err) = esito {
        // Chi ha chiesto dallo schermo dell'auto non vede la fascia della
        // finestra: glielo si dice anche là.
        #[cfg(mobile)]
        sistema::non_riuscito(&err.code);
        nota!(
            "[media] {} causa={}",
            err.code,
            err.cause.as_deref().unwrap_or_default()
        );
        // Anche alla finestra, come quando il gesto parte da lì: un «prossimo»
        // dal vassoio su un file sparito fermava tutto, e il perché finiva
        // solo nel diario. Chi ha premuto un tasto multimediale, una voce del
        // vassoio o un pulsante della miniatura non ha un comando a cui
        // l'errore risalga: la fascia è l'unico posto in cui leggerlo.
        app.emetti("riproduzione:errore", err);
    }
}

/// Lo stesso millisecondo, ma non oltre la fine del brano.
///
/// # Perché un tetto anche qui
///
/// Il tetto vero sta nel decodificatore e vale per tutti; questo sta prima, e
/// serve a non chiedergli un numero che sta fuori dalla barra del brano: una
/// posizione assoluta mandata dal sistema operativo — la barra trascinata nel
/// riquadro del volume, un'auto collegata in Bluetooth — può parlare di una
/// durata vecchia, quella del brano di prima. Non trattiene nel brano: una
/// posizione sulla fine è la fine, e il brano dopo parte — che è quel che
/// chiede chi trascina la barra fino in fondo. A trattenere i salti relativi è
/// [`avanti_entro`], con il suo margine.
///
/// Con durata sconosciuta non si limita niente: `0` significa che nessun brano
/// è ancora partito, e trattarlo come una lunghezza vera manderebbe ogni salto
/// all'inizio.
fn dentro_il_brano(app: &tauri::AppHandle, ms: u64) -> u64 {
    let durata = app.state::<StatoMedia>().durata_ms.load(Ordering::Relaxed);
    entro_la_durata(ms, durata)
}

/// La regola di [`dentro_il_brano`], senza il posto da cui viene la durata.
///
/// Una funzione a sé unicamente perché si possa provare: leggere `durata_ms`
/// vuole un `AppHandle`, e un `AppHandle` vuole un ciclo degli eventi. La riga
/// è la stessa di prima, spostata di un livello.
fn entro_la_durata(ms: u64, durata: u64) -> u64 {
    if durata == 0 { ms } else { ms.min(durata) }
}

/// Quanto prima della fine si ferma un «avanti» del sistema.
///
/// Un salto sulla fine è la fine: il decodificatore lo ferma poco prima
/// dell'ultimo fotogramma (`rymembra_play`, `MARGINE_FINE_MS` del
/// decodificatore: centoventi millisecondi), e da lì il brano finisce e la coda
/// avanza. Un secondo e mezzo lascia sentire che si è arrivati in fondo — e
/// basta un altro «avanti», o il brano che finisce da sé, per passare oltre.
const MARGINE_FINE_MS: u64 = 1_500;

/// Dove finisce un «avanti» di `quanto` da `adesso`, in un brano lungo
/// `durata`: mai oltre [`MARGINE_FINE_MS`] dalla fine, e mai indietro.
///
/// `None` vuol dire **nessun salto**: si è già dentro l'ultimo tratto, e un
/// salto sul punto in cui si è già non è gratis — azzera il decodificatore e,
/// su una condivisione, rilegge. Con la durata sconosciuta (`0`, vedi
/// [`StatoMedia::durata_ms`]) non si limita niente.
fn avanti_entro(adesso: u64, quanto: u64, durata: u64) -> Option<u64> {
    let voluto = adesso.saturating_add(quanto);
    if durata == 0 {
        return Some(voluto);
    }
    let tetto = durata.saturating_sub(MARGINE_FINE_MS);
    if adesso >= tetto {
        return None;
    }
    Some(voluto.min(tetto))
}

/// I millisecondi di una durata, senza traboccare.
#[cfg_attr(
    mobile,
    allow(dead_code, reason = "le durate di `souvlaki`: solo sul computer")
)]
fn ms_di(durata: Duration) -> u64 {
    u64::try_from(durata.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod prove {
    use super::*;

    use std::sync::mpsc::Receiver;

    /// Quanto una prova aspetta un gesto: largo, perché un gesto che arriva
    /// lo fa in microsecondi e uno che non arriva non arriva mai.
    const ENTRO: Duration = Duration::from_secs(5);

    /// Le corsie con un esecutore finto: annota i gesti, e su «prossimo» resta
    /// fermo — un brano che si apre da un NAS che dorme — finché la prova non
    /// lo lascia andare.
    fn banco(suona: bool) -> (Corsie, Receiver<&'static str>, Sender<()>) {
        banco_con(suona, |_| {})
    }

    /// Come [`banco`], con l'attesa del motore che l'avvio vero fa prima del
    /// turno di ogni gesto in fila.
    fn banco_con(
        suona: bool,
        aspetta_il_motore: impl Fn(&Gesto) + Send + 'static,
    ) -> (Corsie, Receiver<&'static str>, Sender<()>) {
        let (annota, annotati) = std::sync::mpsc::channel();
        let (lascia, aspetta) = std::sync::mpsc::channel::<()>();
        let aspetta = Mutex::new(aspetta);
        let corsie = avvia_corsie(
            move |gesto| {
                let nome = match gesto {
                    Gesto::Prossimo => {
                        let _ = annota.send("prossimo:comincia");
                        let _ = aspetta
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .recv_timeout(ENTRO);
                        "prossimo:finisce"
                    }
                    Gesto::Pausa => "pausa",
                    Gesto::Riprendi => "riprendi",
                    Gesto::Alterna => "alterna",
                    Gesto::Precedente => "precedente",
                    _ => "altro",
                };
                let _ = annota.send(nome);
            },
            move || suona,
            aspetta_il_motore,
        )
        .expect("i fili delle corsie partono");
        (corsie, annotati, lascia)
    }

    /// L'attesa del motore di una prova: sul «riprendi» annota che comincia e
    /// resta ferma finché la prova non la lascia andare; ogni altro gesto passa.
    fn attesa_sul_riprendi() -> (
        impl Fn(&Gesto) + Send + 'static,
        Receiver<&'static str>,
        Sender<()>,
    ) {
        let (annota, annotati) = std::sync::mpsc::channel();
        let (lascia, aspetta) = std::sync::mpsc::channel::<()>();
        let aspetta = Mutex::new(aspetta);
        let attesa = move |gesto: &Gesto| {
            if matches!(gesto, Gesto::Riprendi) {
                let _ = annota.send("motore:aspetta");
                let _ = aspetta
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .recv_timeout(ENTRO);
                let _ = annota.send("motore:aperto");
            }
        };
        (attesa, annotati, lascia)
    }

    /// Il difetto di Android Auto: un «riprendi» che arriva mentre il primo
    /// motore si apre falliva. Adesso aspetta l'apertura, e solo dopo si
    /// esegue: mai prima.
    #[test]
    fn un_riprendi_aspetta_il_motore_che_si_apre_e_poi_parte() {
        let (attesa, motore, apri) = attesa_sul_riprendi();
        let (corsie, annotati, _) = banco_con(false, attesa);
        assert!(corsie.manda(Gesto::Riprendi));
        assert_eq!(motore.recv_timeout(ENTRO), Ok("motore:aspetta"));
        assert!(
            annotati.try_recv().is_err(),
            "il «riprendi» si è eseguito mentre il motore si apriva"
        );
        let _ = apri.send(());
        assert_eq!(motore.recv_timeout(ENTRO), Ok("motore:aperto"));
        assert_eq!(
            annotati.recv_timeout(ENTRO),
            Ok("riprendi"),
            "il «riprendi» non è ripartito all'apertura"
        );
    }

    /// L'attesa del motore non tiene il turno: una pausa premuta mentre il
    /// «riprendi» aspetta passa subito — il filo della pausa non resta ad
    /// aspettare —, e il «riprendi» superato non parte quando il motore si
    /// apre: vince l'ultimo tasto premuto (R23-3).
    #[test]
    fn una_pausa_durante_l_attesa_del_motore_annulla_la_ripresa() {
        let (attesa, motore, apri) = attesa_sul_riprendi();
        let (corsie, annotati, _) = banco_con(true, attesa);
        assert!(corsie.manda(Gesto::Riprendi));
        assert_eq!(motore.recv_timeout(ENTRO), Ok("motore:aspetta"));
        assert!(corsie.manda(Gesto::Pausa));
        assert_eq!(
            annotati.recv_timeout(ENTRO),
            Ok("pausa"),
            "la pausa ha aspettato che il motore si aprisse"
        );
        let _ = apri.send(());
        assert_eq!(motore.recv_timeout(ENTRO), Ok("motore:aperto"));
        // Un gesto dopo, per sapere che il filo in fila ha finito col
        // «riprendi»: se fosse stato eseguito, starebbe prima di questo.
        assert!(corsie.manda(Gesto::Precedente));
        assert_eq!(
            annotati.recv_timeout(ENTRO),
            Ok("precedente"),
            "il «riprendi» superato dalla pausa è partito all'apertura del motore"
        );
    }

    /// Chi aspetta il motore è chi lo vuole: la pausa e il cuore dell'auto no.
    #[test]
    fn solo_i_gesti_che_toccano_il_motore_lo_aspettano() {
        assert!(!vuole_il_motore(&Gesto::Pausa));
        for gesto in [
            Gesto::Riprendi,
            Gesto::Alterna,
            Gesto::Prossimo,
            Gesto::Precedente,
            Gesto::VaiA(1_000),
        ] {
            assert!(vuole_il_motore(&gesto), "{gesto:?}");
        }
    }

    /// L'ordine dentro `avvia_corsie`: l'attesa viene **prima** del turno. Se
    /// scivolasse dopo, la pausa resterebbe ferma per dieci secondi.
    #[test]
    fn l_attesa_del_motore_sta_prima_del_turno() {
        let sorgente = include_str!("mod.rs");
        let sorgente = sorgente
            .split("#[cfg(test)]\nmod prove")
            .next()
            .unwrap_or(sorgente);
        let corpo = sorgente
            .split("fn avvia_corsie(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let attesa = corpo.find("aspetta(&gesto)");
        let turno = corpo.find("ultima_pausa.lock()");
        assert!(
            attesa.is_some() && attesa < turno,
            "avvia_corsie non aspetta il motore prima di prendere il turno"
        );
        // E il filo che chiama `premi` lo collega davvero all'attesa vera.
        let premi = sorgente
            .split("pub(crate) fn premi(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert!(
            premi.contains("aspetta_il_primo_motore(&stato)"),
            "premi non collega i gesti all'attesa del primo motore"
        );
    }

    /// R23-3: una pausa da un tasto non aspetta un «successivo» lento.
    /// Restava in fila dietro di lui, e il brano di prima continuava a
    /// suonare fino a quindici secondi — sul telefono anche la pausa delle
    /// cuffie staccate, dall'altoparlante.
    #[test]
    fn la_pausa_non_aspetta_un_gesto_lento() {
        let (corsie, annotati, lascia) = banco(true);
        assert!(corsie.manda(Gesto::Prossimo));
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:comincia"));
        assert!(corsie.manda(Gesto::Pausa));
        assert_eq!(
            annotati.recv_timeout(ENTRO),
            Ok("pausa"),
            "la pausa ha aspettato il «successivo»"
        );
        let _ = lascia.send(());
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:finisce"));
    }

    /// Una ripresa premuta prima della pausa, e rimasta in fila dietro al
    /// gesto lento, non la disfa quando il gesto lento finisce: vince l'ultimo
    /// tasto premuto.
    #[test]
    fn una_ripresa_superata_da_una_pausa_non_si_esegue() {
        let (corsie, annotati, lascia) = banco(true);
        assert!(corsie.manda(Gesto::Prossimo));
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:comincia"));
        assert!(corsie.manda(Gesto::Riprendi));
        assert!(corsie.manda(Gesto::Pausa));
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("pausa"));
        let _ = lascia.send(());
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:finisce"));
        assert!(corsie.manda(Gesto::Precedente));
        assert_eq!(
            annotati.recv_timeout(ENTRO),
            Ok("precedente"),
            "la ripresa premuta prima della pausa è stata eseguita dopo"
        );
    }

    /// «Alterna» mentre suona è una pausa, e non aspetta; a musica ferma è
    /// una ripresa, e sta in fila con gli altri.
    #[test]
    fn alterna_non_aspetta_solo_quando_e_una_pausa() {
        let (corsie, annotati, lascia) = banco(true);
        assert!(corsie.manda(Gesto::Prossimo));
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:comincia"));
        assert!(corsie.manda(Gesto::Alterna));
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("alterna"));
        let _ = lascia.send(());
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:finisce"));

        let (corsie, annotati, lascia) = banco(false);
        assert!(corsie.manda(Gesto::Prossimo));
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:comincia"));
        assert!(corsie.manda(Gesto::Alterna));
        let _ = lascia.send(());
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("prossimo:finisce"));
        assert_eq!(annotati.recv_timeout(ENTRO), Ok("alterna"));
    }

    /// R23-10: ogni filo dei gesti esegue dentro `sorvegliato`, e senza fili
    /// un gesto non si esegue sul filo che l'ha ricevuto.
    #[test]
    fn i_fili_dei_gesti_sono_sorvegliati() {
        let senza_prove = |testo: &'static str| {
            testo
                .find("#[cfg(test)]\nmod prove")
                .map_or(testo, |fine| &testo[..fine])
        };
        let corpo = |testo: &'static str, firma: &str| {
            let testo = senza_prove(testo);
            let inizio = testo.find(firma).unwrap_or_else(|| panic!("manca {firma}"));
            let resto = &testo[inizio..];
            resto[..resto.find("\n}\n").unwrap_or(resto.len())].to_owned()
        };
        let premi = corpo(include_str!("mod.rs"), "pub(crate) fn premi(");
        assert!(
            premi.contains("sorvegliato(FILO_TASTI"),
            "i tasti si eseguono senza sorveglianza"
        );
        assert!(
            !premi.contains("esegui(app"),
            "senza filo i tasti si eseguono sul posto"
        );
        for firma in [
            "fn ascolta_i_gesti(",
            "fn rispondi_all_auto(",
            "fn componi_la_coda(",
        ] {
            assert!(
                corpo(include_str!("android.rs"), firma).contains("sorvegliato("),
                "{firma} non è sorvegliato"
            );
        }
    }
    #[test]
    fn una_durata_sconosciuta_non_mette_nessun_tetto() {
        // `0` vuol dire «non si sa», non «lungo zero»: con un tetto a zero ogni
        // salto in avanti tornerebbe all'inizio del brano.
        assert_eq!(entro_la_durata(93_000, 0), 93_000);
        assert_eq!(entro_la_durata(0, 0), 0);
    }

    #[test]
    fn un_salto_oltre_la_fine_si_ferma_alla_fine() {
        // Una posizione assoluta del sistema non chiede al motore un numero
        // fuori dalla barra: si ferma alla fine. Non trattiene nel brano — una
        // posizione sulla fine è la fine, e il brano dopo parte — e per gli
        // «avanti» relativi il margine lo mette `avanti_entro`.
        assert_eq!(entro_la_durata(200_000, 187_000), 187_000);
        assert_eq!(entro_la_durata(187_000, 187_000), 187_000);
        // Dentro al brano non si tocca niente.
        assert_eq!(entro_la_durata(12_500, 187_000), 12_500);
    }

    /// «Avanti» vicino alla fine resta nel brano: atterra prima dell'ultimo
    /// tratto, mai sulla fine — che è il brano dopo — e mai indietro.
    #[test]
    fn un_avanti_vicino_alla_fine_resta_nel_brano() {
        let dove = avanti_entro(184_000, 5_000, 187_000).expect("c'è spazio per saltare");
        assert!(
            (184_000..=187_000 - MARGINE_FINE_MS).contains(&dove),
            "il salto è finito sulla fine del brano, o indietro: {dove}"
        );
        // Già dentro il margine: nessun salto, che azzererebbe il
        // decodificatore per tornare dov'era.
        assert_eq!(avanti_entro(186_500, 5_000, 187_000), None);
        // Lontano dalla fine non cambia niente.
        assert_eq!(avanti_entro(12_000, 5_000, 187_000), Some(17_000));
        // Con la durata sconosciuta non si limita niente.
        assert_eq!(avanti_entro(93_000, 5_000, 0), Some(98_000));
    }

    #[test]
    fn una_durata_troppo_grande_satura_invece_di_traboccare() {
        assert_eq!(ms_di(Duration::from_millis(0)), 0);
        assert_eq!(ms_di(Duration::from_secs(5)), 5_000);
        // `Duration::as_millis` è un `u128`: il massimo non ci sta in un `u64`,
        // e la conversione deve fermarsi lì invece di girare intorno.
        assert_eq!(ms_di(Duration::MAX), u64::MAX);
    }
}
