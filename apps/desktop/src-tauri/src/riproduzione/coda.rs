//! La coda, e i gesti che la cambiano.
//!
//! Accodare, mettere subito dopo, saltare, togliere, riordinare, svuotare,
//! ripetere, mescolare — più la coda di ieri che torna all'avvio e la scelta
//! che l'autoplay fa da solo quando dopo non c'è più niente.
//!
//! Fa parte di [`crate::riproduzione`]: la regola dei due lucchetti — prima il
//! lettore, poi la libreria — è scritta là e vale anche qui. Qui si vede in
//! [`gesto_di_coda`], che tiene il gesto e la partenza una fuori dall'altra
//! proprio per non aprire un file con il lucchetto del lettore in mano.

use crate::nucleo::Nucleo;
use std::sync::atomic::{AtomicU64, Ordering};

use rymembra_app::library::TrackSummary;
use rymembra_app::playback;
use rymembra_domain::errors::AppError;
use rymembra_domain::queue::{Queue, QueueSnapshot, RepeatMode};
use serde::Serialize;

use crate::disparte::leggi_la_libreria;
use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria};

use super::fili::prepara_prossimo;
use super::{
    Lettore, StatoLettore, Strada, Verso, avvia_corrente, avvia_o_salta, avvio_fallito, brano_di,
    chiudi_ascolto, con_lettore, insieme, manda_stato, manda_stato_con_posizione, ospiti, seme,
    strada, voci_dalla_strada,
};
use crate::insieme::Gesto;

// ── la coda di prima ────────────────────────────────────────────────────────

/// Da quanti brani una coda che nessuno ha toccato merita un «Annulla».
///
/// Un gesto che sostituisce la coda è quasi sempre voluto: si è finito un
/// disco e se ne mette un altro, e un avviso a ogni album sarebbe rumore che
/// insegna a non leggere gli avvisi. Quel che si perde **senza volerlo** è una
/// coda lunga — la cartella da duecento brani sostituita da un Invio sulla
/// cartella accanto — o una coda costruita a mano, che ha un valore suo
/// qualunque sia la lunghezza (per questa vale [`Lettore::coda_curata`]).
///
/// Venticinque sta sopra quasi ogni album e sotto quasi ogni cartella, playlist
/// o «tutti i brani»: il confine fra «un disco» e «una sessione».
const SOGLIA_DI_UNA_SESSIONE: usize = 25;

/// Il numero dell'ultima sostituzione, per riconoscere un «Annulla» vecchio.
///
/// L'avviso resta a schermo qualche secondo, e in quei secondi la coda può
/// essere sostituita di nuovo — anche da un gesto che un avviso non lo merita.
/// Senza un numero, premere «Annulla» sul primo avviso rimetterebbe la coda
/// di prima del **secondo** gesto: un annulla che non annulla quel che dice.
/// Statico e non dentro [`Lettore`] perché deve crescere anche attraverso una
/// riapertura del dispositivo, che il lettore lo ricostruisce.
static SOSTITUZIONI: AtomicU64 = AtomicU64::new(0);

/// La coda com'era prima che un gesto la sostituisse.
pub(super) struct CodaDiPrima {
    /// Quale sostituzione l'ha messa da parte; vedi [`SOSTITUZIONI`].
    numero: u64,
    /// I brani, l'ordine e il punto in cui si era.
    istantanea: QueueSnapshot,
    /// Se era stata costruita a mano: torna com'era insieme alla coda.
    curata: bool,
    /// A che punto del brano si era, in millisecondi.
    ms: u64,
    /// Il motore aveva un brano in mano: se no, la coda torna e la musica no.
    avviata: bool,
    /// La musica stava andando, non era in pausa.
    suonava: bool,
}

/// Quel che la finestra riceve quando c'è una coda da poter rimettere.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodaSostituita {
    /// Da ripassare a [`coda_ripristina`].
    numero: u64,
    /// Quanti brani aveva la coda che se n'è andata.
    brani: usize,
    /// Il gesto era «Svuota», non una coda nuova.
    svuotata: bool,
}

/// Mette da parte la coda di adesso, prima che un gesto la sostituisca.
///
/// Si chiama **dentro** il gesto, con il lucchetto del lettore in mano e prima
/// di toccare la coda. `nuovi` sono i brani che la sostituiranno; `None` vuol
/// dire «Svuota».
///
/// Una coda vuota non ha niente da rimettere, e la stessa lista riavviata da un
/// altro brano — il doppio clic su una riga dell'elenco che sta già suonando —
/// non ha perso niente: in quei due casi l'eventuale coda messa da parte prima
/// resta dov'è, perché l'avviso che la offre può essere ancora a schermo.
pub(super) fn ricorda_coda_di_prima(app: &Nucleo, lettore: &mut Lettore, nuovi: Option<&[i64]>) {
    // In sessione no: l'«Annulla» rimetterebbe una coda di prima della
    // sessione, o di voci che gli altri non hanno più, e la strada lo
    // rifiuta (`Gesto::CodaRipristina`). Offrirlo nell'avviso vorrebbe dire un
    // tasto che risponde con un errore. La coda dell'utente da rimettere
    // all'uscita la tiene la sessione: vedi `insieme::Sessione`.
    if lettore.sessione.is_some() {
        return;
    }
    let istantanea = lettore.coda.snapshot();
    if !c_e_da_rimettere(&istantanea.tracks, nuovi) {
        return;
    }
    // La coda che il giro guidato ha messo lì e nessuno ha toccato: vedi
    // `Lettore::coda_del_giro`. Si consuma solo qui, dopo il primo controllo:
    // la stessa lista riavviata non deve far dimenticare al giro la sua coda.
    let del_giro = lettore.coda_del_giro.take();
    if e_la_coda_del_giro(&istantanea.tracks, del_giro.as_deref()) {
        return;
    }
    let posizione = lettore.motore.posizione();
    let numero = SOSTITUZIONI.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let brani = istantanea.tracks.len();
    let curata = lettore.coda_curata;
    lettore.coda_di_prima = Some(CodaDiPrima {
        numero,
        istantanea,
        curata,
        ms: posizione.ms,
        avviata: posizione.track_id.is_some(),
        suonava: posizione.track_id.is_some() && !posizione.in_pausa,
    });
    if merita_l_avviso(nuovi.is_none(), curata, brani) {
        app.emetti(
            "coda:sostituita",
            CodaSostituita {
                numero,
                brani,
                svuotata: nuovi.is_none(),
            },
        );
    }
}

/// C'è una coda da poter rimettere? Non se è vuota, e non se i brani nuovi
/// sono gli stessi — la lista che suona già riavviata da un altro brano.
/// `nuovi` è `None` per «Svuota».
///
/// Le regole di [`ricorda_coda_di_prima`] stanno in tre funzioni pure perché si
/// possano provare senza un motore: sbagliate non si vedono — una coda curata
/// che sparisce senza avviso, o un «Annulla» a ogni doppio clic — finché non
/// costano qualcosa a qualcuno.
fn c_e_da_rimettere(coda: &[i64], nuovi: Option<&[i64]>) -> bool {
    !coda.is_empty() && nuovi != Some(coda)
}

/// La coda è quella che il giro guidato ha messo e nessuno ha toccato? Vedi
/// `Lettore::coda_del_giro`.
fn e_la_coda_del_giro(coda: &[i64], del_giro: Option<&[i64]>) -> bool {
    del_giro == Some(coda)
}

/// La coda sostituita merita l'avviso con l'«Annulla»?
///
/// «Svuota» l'avviso lo ha sempre: è il solo gesto il cui scopo è perdere la
/// coda, ed è anche quello che si preme per sbaglio accanto a «Mescola». Poi
/// la coda costruita a mano, qualunque sia la lunghezza, e quella lunga almeno
/// [`SOGLIA_DI_UNA_SESSIONE`]: un disco al posto di un altro no.
fn merita_l_avviso(svuota: bool, curata: bool, brani: usize) -> bool {
    svuota || curata || brani >= SOGLIA_DI_UNA_SESSIONE
}

/// Il brano che continua la sessione, secondo la libreria.
///
/// Riceve il brano corrente e quel che è già in coda, e **non** il lettore:
/// scegliere vuol dire leggere impronte e cronologie, e si fa a lucchetto del
/// lettore lasciato. Prima si faceva con il lucchetto in mano, e per tutta la
/// scelta la pausa premuta dalla finestra restava in attesa. Chi chiama
/// fotografa la coda sotto il lucchetto, sceglie fuori, e rientra per
/// accodare solo se nel frattempo la coda non è cambiata — vedi
/// `fili::prepara_prossimo_adesso`.
///
/// `esclusi` è tutto quel che è già in coda, così l'autoplay non ripropone
/// quel che si è appena sentito.
///
/// La libreria si presta **un gradino alla volta** della cascata (album,
/// contesto, artista, genere, preferito, uno qualunque), con
/// [`crate::stato::DepositoStato`]: in una presa sola la discesa fino in fondo
/// la teneva chiusa per tutto il tempo, e un cuore premuto nella finestra in
/// quel momento aspettava. L'attesa di ogni presa è quella di `con_libreria`
/// fuori dal filo della finestra — il deposito passa dalla stessa strada — e
/// una presa lunga finisce nel diario col suo numero, come quelle della
/// scansione.
pub(super) fn scegli_da_solo(
    app: &Nucleo,
    corrente: i64,
    esclusi: &[i64],
) -> Option<rymembra_app::autoplay::Scelta> {
    // `try_state`: chi chiede è il filo del preparatore, che può ritrovarsi
    // vivo mentre gli stati se ne vanno.
    let stato = app.try_state::<Stato>()?;
    rymembra_app::autoplay::prossimo_con(
        &mut crate::stato::DepositoStato::nuovo(&stato),
        corrente,
        esclusi,
        adesso_ms(),
        &crate::lontano::origine_che_suona(app),
    )
    .ok()
    .flatten()
}

/// Conserva la coda — fra un attimo.
///
/// Alza la bandiera [`StatoLettore::coda_da_salvare`], e la scrittura la fa il
/// filo dell'orologio entro un quarto di secondo, con una scadenza: vedi
/// `fili::salva_quel_che_manca`. Qui si passava dal database con il lucchetto
/// del lettore in mano, a ogni gesto e a ogni cambio di brano — e un database
/// occupato teneva fermo il lettore per tutto il tempo della scrittura.
///
/// Il lettore resta nella firma perché la regola resta quella di prima: si
/// chiama dopo aver cambiato la coda, con il lucchetto in mano. Chi scrive
/// rilegge la coda da lì, e scrive l'ultima — che è la sola che interessi.
pub(super) fn salva_coda(app: &Nucleo, _lettore: &Lettore) {
    if let Some(stato) = app.try_state::<StatoLettore>() {
        stato.coda_da_salvare.store(true, Ordering::Relaxed);
    }
}

/// Scrive la coda sul disco adesso, con una scadenza. Chiamata dall'orologio e
/// dall'uscita, **senza** il lucchetto del lettore in mano: se lo prende da sé
/// per la fotografia, e lo lascia prima di toccare il database.
///
/// `None` vuol dire «non scritta, la libreria era occupata»; chi chiama rialza
/// la bandiera.
pub(super) fn scrivi_coda(
    app: &Nucleo,
    stato: &StatoLettore,
    entro: std::time::Duration,
) -> Option<Result<(), AppError>> {
    // In sessione la coda non si scrive: è fatta di maniglie, che domani non
    // vorranno dire niente, e quella sul disco deve restare la coda
    // dell'utente — se l'app cadesse a metà sessione, al prossimo avvio
    // tornerebbe quella. Detto «scritta» perché non c'è niente da riprovare:
    // all'uscita la bandiera si rialza con la coda di prima. Vedi
    // `insieme::coda_si_scrive`.
    let istantanea = con_lettore(stato, |lettore| {
        Ok(insieme::coda_si_scrive(lettore).then(|| lettore.coda.snapshot()))
    })
    .ok()?;
    let Some(istantanea) = istantanea else {
        return Some(Ok(()));
    };
    // Un brano di un altro dispositivo è una riga della libreria unica, e sta in
    // coda come ogni altra: non c'è più una coda intera accanto.
    let stato_app = app.try_state::<Stato>()?;
    crate::stato::con_libreria_entro(&stato_app, entro, |libreria| {
        playback::save_queue(&libreria.connection, &istantanea)
    })
}

/// Riprende la coda conservata, senza far partire niente.
///
/// La coda torna, la musica no: un'applicazione che comincia a suonare da sola
/// all'avvio è un'applicazione che fa saltare sulla sedia chi l'ha aperta per
/// cercare una canzone.
///
/// # Prima si legge, poi si prende il lettore
///
/// Fino a questa versione le nove letture dal database stavano **dentro** il
/// lucchetto del lettore, ognuna con la sua presa della libreria. All'avvio
/// non costava niente a nessuno; con una scansione in corso, chi chiamava
/// teneva chiuso il lettore per tutta l'attesa della libreria. Adesso si legge
/// tutto con una presa sola della libreria e a lettore lasciato — vedi
/// [`leggi_il_salvato`] — e il lettore si prende dopo, il tempo di copiare: vedi
/// [`rimetti_il_salvato`].
///
/// Il motore aperto per la prima volta a programma già avviato — le cuffie
/// attaccate dopo — non passa di qui: lì il lettore nasce già con quel che c'è
/// scritto, e non vuoto per essere riempito un attimo dopo. Vedi
/// `riapertura::riapri_davvero`.
pub fn riprendi_coda(app: &Nucleo) {
    // `try_state`, come in tutto il modulo: uno stato che non c'è più vuol dire
    // un'applicazione che si chiude, non un panico.
    let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
        return;
    };
    let salvato = leggi_il_salvato(app);
    // Prima di consegnare il salvato al lettore: se la coda di «Sfoglia» aspetta
    // il catalogo di un dispositivo, serve la coda com'è stata ripresa per
    // riconoscere, dopo, che nessuno l'ha toccata.
    let in_attesa = salvato.coda_lontana_in_attesa;
    let ripresa = salvato.coda.as_ref().map(Queue::snapshot);
    let _ = con_lettore(&stato_lettore, |lettore| {
        rimetti_il_salvato(&stato_lettore, lettore, salvato);
        Ok(())
    });
    if in_attesa && let Some(ripresa) = ripresa {
        aspetta_la_coda_lontana(app.clone(), ripresa);
    }
}

/// Ogni quanto si riguarda se il catalogo di un dispositivo è arrivato.
const OGNI_QUANTO_SI_RIGUARDA: std::time::Duration = std::time::Duration::from_secs(3);

/// Quanto si aspettano i cataloghi, al più, per portare la coda di «Sfoglia»:
/// due minuti coprono il primo giro di un catalogo grande via relay (il collaudo
/// del 18/11: da dieci a trenta secondi per diecimila righe). Poi si porta quel
/// che c'è, e la voce di un dispositivo che non ha risposto si scarta.
const ATTESA_DELLA_CODA_LONTANA: std::time::Duration = std::time::Duration::from_secs(120);

/// La coda con le voci lontane rimesse prende il posto della coda ripresa solo se
/// nessuno l'ha toccata: il motore non ha un brano in mano, non si è in
/// sessione, e la coda di adesso è quella di quando si è ripresa. Pura apposta,
/// perché la scelta si prova senza un lettore.
fn la_coda_si_puo_sostituire(
    adesso: &QueueSnapshot,
    ripresa: &QueueSnapshot,
    brano_in_motore: bool,
    in_sessione: bool,
) -> bool {
    !in_sessione && !brano_in_motore && adesso == ripresa
}

/// La coda di «Sfoglia» che all'avvio ha trovato il catalogo di un dispositivo
/// ancora da leggere (D6 del collaudo del 18/11): si riprova ogni pochi secondi
/// su un filo suo, finché il catalogo c'è o il tempo scade
/// ([`ATTESA_DELLA_CODA_LONTANA`]). Quando si porta, la coda nuova prende il
/// posto di quella ripresa **solo se nessuno l'ha toccata**
/// ([`la_coda_si_puo_sostituire`]); se la persona ha già fatto qualcosa la coda
/// intera «rimane indietro», e la chiave si toglie da sola senza scrivere niente.
fn aspetta_la_coda_lontana(app: Nucleo, ripresa: QueueSnapshot) {
    let partito = std::thread::Builder::new()
        .name("rymembra-coda-lontana".to_owned())
        .spawn(move || {
            let inizio = std::time::Instant::now();
            loop {
                std::thread::sleep(OGNI_QUANTO_SI_RIGUARDA);
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let Some(stato) = app.try_state::<Stato>() else {
                    return;
                };
                let aspetta = inizio.elapsed() < ATTESA_DELLA_CODA_LONTANA;
                let esito = con_libreria(&stato, |libreria| {
                    let righe = playback::load_queue(&libreria.connection)?;
                    playback::porta_la_coda_lontana(&libreria.connection, righe, aspetta)
                });
                match esito {
                    Ok(playback::CodaLontana::Rimandata) => {}
                    Ok(playback::CodaLontana::Fatta(migrata)) => {
                        metti_la_coda_lontana(&app, &ripresa, migrata);
                        return;
                    }
                    Ok(playback::CodaLontana::Niente(_)) => return,
                    Err(err) => {
                        nota!(
                            "[riproduzione] la coda di «Sfoglia» non passa alla libreria unica codice={}",
                            err.code().kind().code()
                        );
                        return;
                    }
                }
            }
        });
    if let Err(err) = partito {
        nota!("[riproduzione] il filo della coda di «Sfoglia» non parte: {err}");
    }
}

/// Mette nel lettore la coda con le voci lontane, se si può: vedi
/// [`la_coda_si_puo_sostituire`].
fn metti_la_coda_lontana(app: &Nucleo, ripresa: &QueueSnapshot, migrata: QueueSnapshot) {
    let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
        return;
    };
    let _ = con_lettore(&stato_lettore, |lettore| {
        if la_coda_si_puo_sostituire(
            &lettore.coda.snapshot(),
            ripresa,
            lettore.motore.posizione().track_id.is_some(),
            lettore.sessione.is_some(),
        ) {
            lettore.coda = Queue::restore(migrata);
            salva_coda(app, lettore);
            manda_stato(app, lettore);
        }
        Ok(())
    });
}

/// Quel che [`riprendi_coda`] rimette al suo posto, letto dal database.
pub(super) struct Salvato {
    /// La coda, se si è letta. Il guasto, se no, l'ha già scritto nel diario
    /// [`leggi_il_salvato`].
    coda: Option<Queue>,
    volume: playback::Volume,
    eq: playback::Equalizzazione,
    normalizzazione: playback::Normalizzazione,
    autoplay: bool,
    dissolvenza_s: u64,
    /// La latenza **globale**: il ripiego delle uscite senza un numero loro.
    /// Quella che va al motore la sceglie [`rimetti_il_salvato`], che sa su
    /// quale uscita il motore si è aperto.
    latenza_ms: i64,
    /// Le latenze delle uscite che ne hanno una. Vedi
    /// `playback::CHIAVE_LATENZA_PER_USCITA`.
    latenze: playback::LatenzePerUscita,
    bande: u16,
    /// Nella coda di «Sfoglia» c'è una voce di un dispositivo il cui catalogo
    /// non è ancora arrivato: la coda con le voci lontane si porta dopo
    /// ([`aspetta_la_coda_lontana`]).
    coda_lontana_in_attesa: bool,
}

/// Legge tutto quel che [`riprendi_coda`] rimette, con una presa sola della
/// libreria e senza il lettore in mano.
///
/// Ogni lettura ha il suo ripiego, com'era quando erano nove prese diverse:
/// una preferenza illeggibile vale quella di serie, e non porta via le altre.
/// Una libreria che non risponde affatto — o che non c'è più — le fa valere
/// tutte di serie, e la coda resta vuota.
///
/// Una coda salvata che non si legge non è una ragione per saltare tutto il
/// resto: col `?` di una volta, un guasto lì lasciava il motore col volume di
/// serie, l'equalizzatore spento e la normalizzazione di fabbrica — cioè il
/// primo brano a tutto volume per chi teneva il cursore basso. Il guasto si
/// scrive nel diario **qui**, dove nessun lucchetto è in mano.
pub(super) fn leggi_il_salvato(app: &Nucleo) -> Salvato {
    let letto = super::libreria_di(app, "coda salvata").and_then(|stato| {
        con_libreria(&stato, |libreria| {
            let c = &libreria.connection;
            let latenze = playback::load_latenze_per_uscita(c).unwrap_or_default();
            let guasto_latenze = riga_delle_latenze(&latenze);
            // Chi aggiorna da una versione con «Sfoglia» può avere in coda voci
            // remote salvate a parte: si portano sulle righe specchio con lo
            // stesso (dispositivo, id), quelle che non ci sono si scartano
            // senza errore, e la chiave vecchia se ne va (`migra_coda_lontana`).
            // Un guasto della migrazione lascia la coda di righe com'era.
            // Il catalogo di un dispositivo arriva dopo l'avvio: una voce lontana
            // che lo aspetta lascia la coda di righe com'è, e la porta
            // `aspetta_la_coda_lontana` quando c'è.
            let mut coda_lontana_in_attesa = false;
            let righe = playback::load_queue(c).map(|righe| {
                match playback::porta_la_coda_lontana(c, righe.clone(), true) {
                    Ok(playback::CodaLontana::Niente(coda) | playback::CodaLontana::Fatta(coda)) => {
                        coda
                    }
                    Ok(playback::CodaLontana::Rimandata) => {
                        coda_lontana_in_attesa = true;
                        righe
                    }
                    Err(err) => {
                        nota!(
                            "[riproduzione] la coda di «Sfoglia» non passa alla libreria unica codice={}",
                            err.code().kind().code()
                        );
                        righe
                    }
                }
            });
            Ok((
                righe,
                Salvato {
                    coda: None,
                    volume: playback::load_volume(c).unwrap_or_default(),
                    eq: playback::load_eq(c).unwrap_or_default(),
                    normalizzazione: playback::load_replaygain(c).unwrap_or_default(),
                    autoplay: playback::load_autoplay(c).unwrap_or(false),
                    dissolvenza_s: playback::load_crossfade(c).unwrap_or(0),
                    latenza_ms: playback::load_latenza(c).unwrap_or(0),
                    latenze: latenze.voci,
                    bande: playback::load_spettro_bande(c)
                        .unwrap_or(rymembra_play::RISOLUZIONE_DI_SERIE),
                    coda_lontana_in_attesa,
                },
                guasto_latenze,
            ))
        })
    });
    let (coda, mut salvato, guasto_latenze) = letto.unwrap_or_else(|err| {
        (
            Err(err),
            Salvato {
                coda: None,
                volume: playback::Volume::default(),
                eq: playback::Equalizzazione::default(),
                normalizzazione: playback::Normalizzazione::default(),
                autoplay: false,
                dissolvenza_s: 0,
                latenza_ms: 0,
                latenze: playback::LatenzePerUscita::new(),
                bande: rymembra_play::RISOLUZIONE_DI_SERIE,
                coda_lontana_in_attesa: false,
            },
            None,
        )
    });
    // Anche questa qui, a lucchetti lasciati, come la coda qui sotto.
    if let Some(riga) = guasto_latenze {
        nota!("{riga}");
    }
    match coda.map(Queue::restore) {
        Ok(coda) => salvato.coda = Some(coda),
        Err(err) => nota!(
            "[riproduzione] coda salvata non riletta codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ),
    }
    salvato
}

/// Rimette nel lettore — e nel suo motore — quel che [`leggi_il_salvato`] ha
/// letto. Si chiama con il lucchetto del lettore in mano, o su un lettore che
/// nessun altro vede ancora; non aspetta niente.
pub(super) fn rimetti_il_salvato(
    stato_lettore: &StatoLettore,
    lettore: &mut Lettore,
    salvato: Salvato,
) {
    if let Some(coda) = salvato.coda {
        lettore.coda = coda;
    }
    lettore.volume = salvato.volume;
    lettore
        .motore
        .volume(salvato.volume.volume, salvato.volume.muto);
    lettore
        .motore
        .equalizzatore(&salvato.eq.guadagni, salvato.eq.attivo);
    lettore.eq = salvato.eq;
    // La normalizzazione si rimanda al motore anche quando coincide con il suo
    // valore di serie: costa un comando su una coda che è già lì, e toglie di
    // mezzo la domanda «chi dei due ha ragione» il giorno in cui uno dei due
    // valori cambiasse.
    lettore.motore.replaygain(
        salvato.normalizzazione.attivo,
        salvato.normalizzazione.bersaglio_db,
    );
    lettore.normalizzazione = salvato.normalizzazione;

    // L'autoplay non si manda al motore — il motore non sa cosa sia una
    // libreria, ed è giusto così. Serve solo a `prepara_prossimo`, che è di qui.
    lettore.autoplay = salvato.autoplay;

    // La dissolvenza invece sì: è il motore a doverla fare, ed è l'unico che sa
    // quando un brano sta per finire. Si manda anche quando vale zero, per la
    // stessa ragione della normalizzazione qui sopra.
    lettore.dissolvenza_s = salvato.dissolvenza_s;
    lettore.motore.dissolvenza(insieme::dissolvenza_ms(lettore));

    // E la latenza d'uscita dichiarata a mano, per la stessa ragione: la
    // compensazione la fa il motore, e un motore appena aperto non sa niente di
    // quel che l'utente aveva dichiarato. Senza questa riga il cursore delle
    // impostazioni mostrerebbe il numero scritto e la posizione non ne terrebbe
    // conto, che è la peggiore delle due bugie possibili.
    //
    // Quella dell'uscita **davvero aperta**, se ne ha una sua, altrimenti la
    // globale: il nome è quello di `dispositivo()` e non della preferenza,
    // perché quando la scheda scelta non c'è l'apertura ripiega su un'altra, e
    // il numero va a chi suona. La copia nel lettore è la stessa, ed è quel che
    // il cursore mostra: il numero dell'uscita che si sta ascoltando.
    let latenza = playback::latenza_effettiva(
        salvato.latenza_ms,
        &salvato.latenze,
        Some(lettore.motore.dispositivo()),
    );
    lettore.motore.latenza(latenza);
    lettore.latenza_ms = latenza;

    // Quante barre vuole vedere chi guarda. Si applica all'avvio anche se la
    // schermata è chiusa: costa un messaggio e vuol dire che la prima apertura
    // mostra la scena giusta invece di quella di serie per un fotogramma. Anche
    // nella copia fuori dal motore, che sopravvive a una riapertura: vedi
    // `StatoLettore::spettro_bande`.
    stato_lettore
        .spettro_bande
        .store(salvato.bande, std::sync::atomic::Ordering::Relaxed);
    lettore.motore.spettro_dettaglio(salvato.bande);
}

/// La riga del diario per le latenze per uscita che non si sono lette tutte,
/// o `None` se non c'è niente da dire.
///
/// Solo una bandiera e un numero. I nomi delle uscite non ci vanno — sono le
/// schede, le cuffie e le casse di qualcuno — e nemmeno il messaggio di
/// `serde`, che per dire cosa non andava ripete il valore che ha trovato, cioè
/// spesso proprio un nome. La riga serve a capire perché una taratura è
/// tornata a quella globale; a quale uscita fosse, lo sa chi la sta guardando.
///
/// La chiamano la lettura all'avvio, qui, e quella della riapertura: tutte e
/// due a lucchetti lasciati, perché il diario scrive su disco.
pub(super) fn riga_delle_latenze(lette: &playback::LatenzeLette) -> Option<String> {
    if lette.illeggibile {
        return Some(
            "[riproduzione] latenze per uscita illeggibili: vale la globale per tutte".to_owned(),
        );
    }
    (lette.scartate > 0).then(|| {
        format!(
            "[riproduzione] latenze per uscita: {} voci illeggibili lasciate fuori, per loro vale \
             la globale",
            lette.scartate
        )
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
pub(super) fn gesto_di_coda(
    app: &Nucleo,
    stato: &StatoLettore,
    gesto: impl FnOnce(&mut Lettore) -> Result<bool, AppError>,
) -> Result<(), AppError> {
    gesto_di_coda_con(stato, gesto, |gesti| {
        avvia_corrente(app, stato, false, 0, gesti)
    })
}

/// [`gesto_di_coda`] per «avanti» e «indietro»: se il brano che parte non si apre
/// per colpa sua e nessuno può rispondere a «Usare i dati mobili?», si passa a
/// quello oltre nel `verso` invece di fermarsi su di lui. Vedi
/// [`avvia_o_salta`](super::avvia_o_salta).
pub(super) fn gesto_di_coda_saltando(
    app: &Nucleo,
    stato: &StatoLettore,
    verso: Verso,
    gesto: impl FnOnce(&mut Lettore) -> Result<bool, AppError>,
) -> Result<(), AppError> {
    gesto_di_coda_con(stato, gesto, |gesti| {
        avvia_o_salta(app, stato, verso, gesti)
    })
}

/// Il gesto sulla coda, e poi la partenza che `avvia` sceglie: la stessa presa
/// del lettore, e lo stesso numero dei gesti, per tutte le forme di
/// [`gesto_di_coda`].
fn gesto_di_coda_con(
    stato: &StatoLettore,
    gesto: impl FnOnce(&mut Lettore) -> Result<bool, AppError>,
    avvia: impl FnOnce(u64) -> Result<(), AppError>,
) -> Result<(), AppError> {
    // Il numero dei gesti si legge nella stessa presa che decide: una pausa
    // arrivata fra qui e la consegna alza il numero, e il brano parte fermo
    // (S7-1).
    let (parte, gesti) = con_lettore(stato, |lettore| {
        Ok((gesto(lettore)?, lettore.pausa_chiesta.0))
    })?;
    if parte {
        avvia(gesti)?;
    }
    Ok(())
}

/// Nella posizione `indice` della coda come si vede c'è ancora il brano che la
/// finestra ci vedeva?
///
/// # Posizione e brano
///
/// La posizione da sola non basta: è quella della coda disegnata, e il nucleo
/// la applica alla sua, che nel frattempo può essere cambiata — la prima × di
/// un doppio clic già arrivata, il «Riproduci dopo» di chi ascolta insieme.
/// Si toglieva, si faceva partire o si spostava allora un altro brano, in
/// silenzio. Con il brano accanto, se a quella posizione non c'è più lui, il
/// gesto non tocca niente: lo stato che torna ridisegna la coda vera, e chi
/// voleva quella riga la ritrova e riprova. È la regola che `playlists`
/// segue già per le righe di una playlist.
///
/// `None` vale «non lo so, fai come prima»: è chi non ha una coda disegnata
/// davanti — l'auto, o il gesto di un membro che l'host ha già risolto per
/// voce.
pub(super) fn al_suo_posto(coda: &Queue, indice: usize, atteso: Option<i64>) -> bool {
    atteso.is_none_or(|brano| coda.in_play_order().get(indice) == Some(&brano))
}

// ── i comandi della coda ────────────────────────────────────────────────────

/// [`coda_accoda_ora`], sul pool bloccante: vedi
/// `super::col_lettore_in_disparte`.
#[tauri::command]
pub async fn coda_accoda(app: Nucleo, brani: Vec<i64>) -> Esito<()> {
    super::col_lettore_in_disparte(app, "accoda", move |app, stato| {
        coda_accoda_ora(app, stato, brani)
    })
    .await
}

/// Accoda in fondo.
pub fn coda_accoda_ora(app: Nucleo, stato: StatoLettore, brani: Vec<i64>) -> Esito<()> {
    let gesto = Gesto::Accoda { ids: brani.clone() };
    let Some(voci) = voci_dalla_strada(&app, &gesto).map_err(errore)? else {
        return Ok(());
    };
    gesto_di_coda(&app, &stato, |lettore| {
        // Voci o righe lo decide il lettore: vedi `insieme::brani_da_mettere`.
        let (brani, da_dimenticare) =
            insieme::brani_da_mettere(insieme::ospita(lettore), brani, voci);
        insieme::dimentica_voci(&app, &da_dimenticare);
        let era_vuota = lettore.coda.is_empty();
        // In sessione la coda ha un tetto: vedi `insieme::entrano`.
        let brani = insieme::entrano(&app, lettore, &brani);
        lettore.coda.enqueue(brani);
        lettore.coda_curata = true;
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

/// [`coda_dopo_ora`], sul pool bloccante: vedi
/// `super::col_lettore_in_disparte`.
#[tauri::command]
pub async fn coda_dopo(app: Nucleo, brani: Vec<i64>) -> Esito<()> {
    super::col_lettore_in_disparte(app, "metti dopo", move |app, stato| {
        coda_dopo_ora(app, stato, brani)
    })
    .await
}

/// Mette subito dopo il brano corrente.
pub fn coda_dopo_ora(app: Nucleo, stato: StatoLettore, brani: Vec<i64>) -> Esito<()> {
    let gesto = Gesto::Dopo { ids: brani.clone() };
    let Some(voci) = voci_dalla_strada(&app, &gesto).map_err(errore)? else {
        return Ok(());
    };
    gesto_di_coda(&app, &stato, |lettore| {
        let (brani, da_dimenticare) =
            insieme::brani_da_mettere(insieme::ospita(lettore), brani, voci);
        insieme::dimentica_voci(&app, &da_dimenticare);
        let era_vuota = lettore.coda.is_empty();
        let brani = insieme::entrano(&app, lettore, &brani);
        lettore.coda.play_next(brani);
        lettore.coda_curata = true;
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

/// [`coda_vai_ora`], sul pool bloccante: vedi
/// `super::col_lettore_in_disparte`.
///
/// `brano` è quello che la finestra vedeva in `indice`: vedi
/// [`al_suo_posto`].
#[tauri::command]
pub async fn coda_vai(app: Nucleo, indice: usize, brano: i64) -> Esito<()> {
    super::col_lettore_chiedendo(app, "salto nella coda", move |app, stato| {
        coda_vai_ora(app, stato, indice, Some(brano))
    })
    .await
}

/// Salta a una posizione della coda.
///
/// Con `atteso`, solo se in quella posizione c'è ancora quel brano: vedi
/// [`al_suo_posto`].
pub fn coda_vai_ora(
    app: Nucleo,
    stato: StatoLettore,
    indice: usize,
    atteso: Option<i64>,
) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::CodaVai { indice, atteso }).map_err(errore)? {
        return Ok(());
    }
    gesto_di_coda(&app, &stato, |lettore| {
        if !al_suo_posto(&lettore.coda, indice, atteso) {
            manda_stato(&app, lettore);
            return Ok(false);
        }
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

/// [`coda_togli_ora`], sul pool bloccante: vedi
/// `super::col_lettore_in_disparte`.
///
/// `brano` è quello che la finestra vedeva in `indice`: vedi
/// [`al_suo_posto`].
#[tauri::command]
pub async fn coda_togli(app: Nucleo, indice: usize, brano: i64) -> Esito<()> {
    super::col_lettore_in_disparte(app, "togli dalla coda", move |app, stato| {
        coda_togli_ora(app, stato, indice, Some(brano))
    })
    .await
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
///
/// # Togliere quel che è in pausa
///
/// Il successivo arriva **fermo**, al suo inizio: chi ha messo in pausa non ha
/// chiesto musica, e togliere una riga dalla coda non è premere play. Prima
/// partiva. E se il motore non aveva ancora niente in mano — la coda di ieri,
/// appena riaperta — non si apre niente: si sposta la coda, come per ogni brano
/// che non sta suonando.
///
/// # Se lì c'è un altro brano
///
/// Con `atteso`, la riga si toglie solo se in quella posizione c'è ancora quel
/// brano: vedi [`al_suo_posto`]. Altrimenti non si tocca niente e la finestra
/// riceve la coda vera.
pub fn coda_togli_ora(
    app: Nucleo,
    stato: StatoLettore,
    indice: usize,
    atteso: Option<i64>,
) -> Esito<()> {
    let gesto = Gesto::CodaTogli {
        indici: vec![indice],
        attesi: atteso.into_iter().collect(),
    };
    if let Strada::Fatto = strada(&app, &gesto).map_err(errore)? {
        return Ok(());
    }
    let partenza = con_lettore(&stato, |lettore| {
        if !al_suo_posto(&lettore.coda, indice, atteso) {
            manda_stato(&app, lettore);
            return Ok(None);
        }
        // Stesso spazio di coordinate di `coda_vai`: l'ordine di riproduzione.
        let suonava = lettore.coda.position() == Some(indice);
        let era_ultimo = indice.saturating_add(1) >= lettore.coda.len();
        let motore = lettore.motore.posizione();
        let prima = lettore.coda.in_play_order();
        let tenute = insieme::tenute_dal_lettore(&stato, lettore);
        lettore.coda.remove_at(indice);
        lettore.coda_curata = true;
        // In sessione la voce tolta esce anche dal registro, se il lettore
        // non la tiene ancora: vedi `insieme::dimentica_le_uscite`. Qui, per
        // tutte le strade qui sotto.
        insieme::dimentica_le_uscite(&app, &stato, lettore, &prima, tenute);
        if !suonava || motore.track_id.is_none() {
            prepara_prossimo(&app, lettore);
            salva_coda(&app, lettore);
            manda_stato(&app, lettore);
            return Ok(None);
        }
        chiudi_ascolto(&app, lettore);
        if !era_ultimo {
            return Ok(Some((motore.in_pausa, lettore.pausa_chiesta.0)));
        }
        if lettore.coda.repeat() == RepeatMode::All && lettore.coda.play_at(0).is_some() {
            return Ok(Some((motore.in_pausa, lettore.pausa_chiesta.0)));
        }
        lettore.ferma();
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(None)
    })
    .map_err(errore)?;
    if let Some((in_pausa, gesti)) = partenza {
        avvia_corrente(&app, &stato, in_pausa, 0, gesti).map_err(errore)?;
    }
    Ok(())
}

/// La fotografia di una coda senza questi brani, se ne nominava qualcuno.
///
/// Per la coda che l'«Annulla» rimetterebbe ([`CodaDiPrima`]): `None` se non
/// cambia niente. Con `Queue::remove_tracks`, la regola della coda vera.
fn senza_i_brani(istantanea: &QueueSnapshot, brani: &[i64]) -> Option<QueueSnapshot> {
    let mut coda = Queue::restore(istantanea.clone());
    (coda.remove_tracks(brani).quante > 0).then(|| coda.snapshot())
}

/// Toglie dalla coda dei brani che la libreria sta per non avere più.
///
/// # Perché la coda va sgomberata a mano
///
/// Perché non è una tabella: è un JSON di identificativi dentro `settings`, e
/// nessuna chiave esterna la ripulisce quando la riga del brano se ne va. Un
/// identificativo rimasto lì è un numero che non apre più niente, e chi lo
/// incontra è il motore alla fine del brano in corso — con un
/// `playback.sourceUnavailable` al posto della canzone dopo, che è il modo
/// peggiore di scoprire di aver cancellato qualcosa venti minuti fa.
///
/// # I due modi, e cosa li distingue
///
/// `ferma` è «il file sta per sparire dal disco». Senza, vale il comportamento
/// di [`coda_togli`]: il brano seguente scivola sotto il cursore e parte, perché
/// togliere dalla coda vuol dire «non risuonarlo», non «zitto adesso». Con,
/// invece, il motore si ferma e basta — non perché sia più prudente, ma perché
/// su Windows un file aperto non si può mandare nel Cestino, e finché il motore
/// ha in mano quei campioni l'eliminazione fallirebbe con `fs.inUse`.
///
/// Anche il brano **in canna** tiene aperto il suo file: `prepara_prossimo` lo
/// lascia andare da sé appena non è più il brano dopo, ed è per questo che si
/// chiama anche quando il corrente non è fra i condannati.
///
/// # Perché si guarda se c'è un «dopo» **prima** di togliere
///
/// Perché dopo non si può più: [`Queue::remove_tracks`] lascia il cursore dove
/// è scivolato il seguente, e da lì «c'era ancora qualcosa da sentire?» e «il
/// cursore si è aggrappato al brano di prima, che è già stato ascoltato?» sono
/// indistinguibili. È la stessa distinzione che in [`coda_togli`] si legge
/// dall'indice, detta per un insieme di brani invece che per uno solo.
pub(crate) fn sgombra_dalla_coda(
    app: &Nucleo,
    stato: &StatoLettore,
    brani: &[i64],
    ferma: bool,
) -> Result<(), AppError> {
    if brani.is_empty() {
        return Ok(());
    }
    // In sessione la coda di chi ospita ha le maniglie delle voci, non le
    // righe: si tolgono anche le voci che aprono quelle righe. Senza, il file
    // che sta per finire nel Cestino resterebbe aperto dal motore sotto la sua
    // maniglia, e l'eliminazione fallirebbe con `fs.inUse`. Il registro si
    // chiede prima del lettore: è un lucchetto foglia, e non si prende mai
    // dopo averlo in mano.
    let mut brani = brani.to_vec();
    brani.extend(ospiti::maniglie_di_righe(app, &brani));
    let brani = brani.as_slice();
    let partenza = con_lettore(stato, |lettore| {
        // La coda che l'«Annulla» rimetterebbe nomina gli stessi identificativi:
        // rimessa dopo che il brano non c'è più — e il suo id è di un altro —,
        // suonerebbe quello (S11-1). Prima del resto: il ritorno anticipato più
        // sotto parla della coda di adesso, non di quella di prima.
        if let Some(prima) = lettore.coda_di_prima.as_mut()
            && let Some(senza) = senza_i_brani(&prima.istantanea, brani)
        {
            prima.istantanea = senza;
        }
        let c_e_un_dopo = lettore.coda.position().is_some_and(|posizione| {
            lettore
                .coda
                .in_play_order()
                .into_iter()
                .skip(posizione.saturating_add(1))
                .any(|id| !brani.contains(&id))
        });
        let motore = lettore.motore.posizione();
        let prima = lettore.coda.in_play_order();
        let tenute = insieme::tenute_dal_lettore(stato, lettore);
        let tolti = lettore.coda.remove_tracks(brani);
        if tolti.quante == 0 {
            return Ok(None);
        }
        // In sessione le voci tolte escono anche dal registro, se il lettore
        // non le tiene ancora: vedi `insieme::dimentica_le_uscite`. Qui, per
        // tutte le strade qui sotto.
        insieme::dimentica_le_uscite(app, stato, lettore, &prima, tenute);
        // La coda resta «curata»: toglierne un brano non è sostituirla, e chi
        // l'aveva costruita a mano ha ancora diritto all'«Annulla» se qualcuno
        // gliela sostituisce dopo.
        lettore.coda_curata = true;
        if !tolti.cera_il_corrente || motore.track_id.is_none() {
            prepara_prossimo(app, lettore);
            salva_coda(app, lettore);
            manda_stato(app, lettore);
            return Ok(None);
        }
        // L'ascolto di quel che suonava si chiude qui, prima che il motore
        // attacchi qualunque altra cosa: è finito adesso.
        chiudi_ascolto(app, lettore);
        if !ferma {
            if c_e_un_dopo {
                return Ok(Some((motore.in_pausa, lettore.pausa_chiesta.0)));
            }
            // Con la ripetizione accesa un dopo c'è comunque, ed è il primo
            // della coda. Senza, non resta che fermarsi.
            if lettore.coda.repeat() == RepeatMode::All && lettore.coda.play_at(0).is_some() {
                return Ok(Some((motore.in_pausa, lettore.pausa_chiesta.0)));
            }
        }
        lettore.ferma();
        salva_coda(app, lettore);
        manda_stato(app, lettore);
        Ok(None)
    })?;
    if let Some((in_pausa, gesti)) = partenza {
        avvia_corrente(app, stato, in_pausa, 0, gesti)?;
    }
    Ok(())
}

/// [`coda_riordina_ora`], dalla finestra.
///
/// `brano` è quello che la finestra vedeva in `da`: vedi [`al_suo_posto`].
#[tauri::command]
pub async fn coda_riordina(app: Nucleo, da: usize, a: usize, brano: i64) -> Esito<()> {
    super::col_lettore_in_disparte(app, "riordino della coda", move |app, stato| {
        coda_riordina_ora(app, stato, da, a, Some(brano))
    })
    .await
}

/// Sposta un brano dentro la coda.
///
/// Con `atteso`, solo se in `da` c'è ancora quel brano: vedi [`al_suo_posto`].
/// Senza, è il gesto di un membro che l'host ha già risolto per voce.
pub fn coda_riordina_ora(
    app: Nucleo,
    stato: StatoLettore,
    da: usize,
    a: usize,
    atteso: Option<i64>,
) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::CodaRiordina { da, a, atteso }).map_err(errore)? {
        return Ok(());
    }
    con_lettore(&stato, |lettore| {
        if !al_suo_posto(&lettore.coda, da, atteso) {
            manda_stato(&app, lettore);
            return Ok(());
        }
        lettore.coda.reorder(da, a);
        lettore.coda_curata = true;
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Svuota la coda e ferma tutto.
///
/// Il flusso si sveglia **prima**, a lucchetto lasciato: fermare il motore
/// svuota l'anello, e svuotarlo vuole il flusso sveglio. Vedi
/// `super::sveglia_fuori_dal_lucchetto`.
#[tauri::command]
pub async fn coda_svuota(app: Nucleo) -> Esito<()> {
    super::col_lettore_in_fila(app, "svuota la coda", coda_svuota_ora).await
}

/// [`coda_svuota`], sul posto: il lavoro sta qui, e il comando lo mette in
/// fila sulla corsia del lettore (`crate::corsia`).
pub fn coda_svuota_ora(app: Nucleo, stato: StatoLettore) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::CodaSvuota).map_err(errore)? {
        return Ok(());
    }
    super::sveglia_fuori_dal_lucchetto(&stato);
    con_lettore(&stato, |lettore| {
        ricorda_coda_di_prima(&app, lettore, None);
        chiudi_ascolto(&app, lettore);
        let prima = lettore.coda.in_play_order();
        let tenute = insieme::tenute_dal_lettore(&stato, lettore);
        lettore.coda.clear();
        lettore.coda_curata = false;
        lettore.ferma();
        // In sessione le voci della coda svuotata escono dal registro: vedi
        // `insieme::dimentica_le_uscite`.
        insieme::dimentica_le_uscite(&app, &stato, lettore, &prima, tenute);
        salva_coda(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Mette dei brani in una coda **vuota**, senza farli partire.
///
/// Serve al giro guidato: quattro dei suoi passi — il trasporto, il giudizio,
/// la colonna e lo schermo intero — illuminano quel che si vede **quando c'è un
/// brano**, e al primo avvio non ce n'è nessuno. Si saltavano tutti, cioè il giro
/// spiegava la barra laterale e le impostazioni e taceva proprio sul lettore.
///
/// Con una coda che c'è già non fa niente, e restituisce `false`: il giro non
/// deve toccare quel che qualcuno ha messo in coda. La musica non parte — un
/// giro che comincia a suonare da solo fa saltare sulla sedia chi lo sta
/// leggendo — e «riproduci» da lì funziona come per la coda ripresa all'avvio.
#[tauri::command]
pub async fn coda_prepara(app: Nucleo, brani: Vec<i64>) -> Esito<bool> {
    super::col_lettore_in_disparte(app, "coda del giro", move |app, stato| {
        con_lettore(&stato, |lettore| {
            // In sessione no: la coda è di voci, e i brani del giro sono righe
            // della libreria che gli altri non hanno. Il giro trova la coda «già
            // piena», come quando qualcuno ci ha messo qualcosa.
            if !lettore.coda.is_empty() || brani.is_empty() || lettore.sessione.is_some() {
                return Ok(false);
            }
            lettore.coda.play_tracks(brani, 0, seme());
            lettore.coda_curata = false;
            lettore.coda_del_giro = Some(lettore.coda.snapshot().tracks);
            salva_coda(&app, lettore);
            manda_stato(&app, lettore);
            Ok(true)
        })
        .map_err(errore)
    })
    .await
}

/// [`coda_ripristina_ora`], sul pool bloccante: vedi
/// `super::col_lettore_in_disparte`.
#[tauri::command]
pub async fn coda_ripristina(app: Nucleo, numero: u64) -> Esito<bool> {
    super::col_lettore_in_disparte(app, "annulla della coda", move |app, stato| {
        coda_ripristina_ora(app, stato, numero)
    })
    .await
}

/// Rimette la coda che l'ultimo gesto aveva sostituito: l'«Annulla» dell'avviso.
///
/// Torna com'era in tutto quel che si può: i brani, l'ordine, il punto della
/// coda, il punto del brano, e se suonava o era in pausa. L'ascolto chiuso dal
/// gesto resta chiuso — è stato un ascolto vero, e ritrattarlo vorrebbe dire
/// togliere dalla cronologia secondi che si sono sentiti.
///
/// Restituisce `false` quando non c'è niente da rimettere: l'avviso era di una
/// sostituzione che nel frattempo ne ha avuta un'altra dopo, o l'«Annulla» è
/// già stato premuto. La finestra chiude l'avviso in tutti e due i casi.
///
/// # I tre tempi
///
/// Gli stessi di [`avvia_corrente`], per la stessa ragione: il brano si apre
/// **fuori** dal lucchetto del lettore. In più, nel terzo tempo, il brano
/// riparte dov'era, fermo se era in pausa — con la stessa consegna con cui
/// `riapertura` rimette il brano dopo un dispositivo sparito (`consegna`).
pub fn coda_ripristina_ora(app: Nucleo, stato: StatoLettore, numero: u64) -> Esito<bool> {
    if let Strada::Fatto = strada(&app, &Gesto::CodaRipristina).map_err(errore)? {
        return Ok(false);
    }
    // ── 1. la coda torna, e si decide se torna anche la musica ──
    // Due `Option` una dentro l'altra: fuori «c'era qualcosa da rimettere»,
    // dentro «e c'è un brano da riaprire».
    let scelta = con_lettore(&stato, |lettore| {
        let Some(prima) = lettore.coda_di_prima.take_if(|p| p.numero == numero) else {
            return Ok(None);
        };
        chiudi_ascolto(&app, lettore);
        lettore.coda = Queue::restore(prima.istantanea);
        lettore.coda_curata = prima.curata;
        let corrente = lettore.coda.current().filter(|_| prima.avviata);
        let Some(id) = corrente else {
            lettore.ferma();
            salva_coda(&app, lettore);
            manda_stato(&app, lettore);
            return Ok(Some(None));
        };
        Ok(Some(Some((
            id,
            lettore.motore.formato(),
            prima.ms,
            prima.suonava,
            lettore.pausa_chiesta.0,
        ))))
    })
    .map_err(errore)?;
    let Some(partenza) = scelta else {
        return Ok(false);
    };
    let Some((id, formato, ms, suonava, gesti_prima)) = partenza else {
        return Ok(true);
    };

    // Una sola apertura per brano, come in `avvia_corrente`: è anche la
    // casella che dice a [`super::vai_a`] di tenere da parte un salto chiesto
    // mentre il file si apre. Presa da un altro per lo stesso brano: chi l'ha
    // presa lo farà partire, e la coda è già tornata.
    let Ok(mut aprendo) = super::Aprendo::prendi(&stato.aprendo, id) else {
        return Ok(true);
    };

    // ── 2. l'apertura, senza niente in mano ──
    // Fallita, la coda è già tornata: il motore si ferma su di lei invece di
    // continuare il brano che la sostituzione aveva avviato. Vedi
    // [`avvio_fallito`].
    let brano = match brano_di(&app, id, formato) {
        Ok(brano) => brano,
        // La coda è tornata comunque: per la finestra l'«Annulla» ha fatto il
        // suo, e un guasto ritentabile lo dice la fascia. Vedi
        // [`avvio_fallito`].
        Err(err) => {
            return avvio_fallito(&app, &stato, id, ms, err)
                .map(|()| true)
                .map_err(errore);
        }
    };

    // ── 3. la puntina dov'era ──
    let consegnato = con_lettore(&stato, |lettore| {
        if lettore.coda.current() != Some(id) {
            return Ok(true);
        }
        // Il motore è stato sostituito mentre il file si apriva: il brano è
        // per il formato di prima. Vedi la stessa guardia in `avvia_corrente`.
        if lettore.motore.formato() != formato {
            return Ok(false);
        }
        // Fermo o suonando com'era, dal punto in cui era: vedi
        // [`super::consegna`].
        let da_ms = super::consegna(lettore, brano, !suonava, ms, gesti_prima);
        drop(aprendo.take());
        prepara_prossimo(&app, lettore);
        salva_coda(&app, lettore);
        manda_stato_con_posizione(&app, lettore, da_ms);
        Ok(true)
    })
    .map_err(errore)?;
    if !consegnato {
        // La strada di tutti gli altri avvii, che col formato nuovo sa
        // riaprire da sé: stesso brano, stesso punto, stessa pausa. La
        // casella va lasciata prima: `avvia_corrente` la vuole per lo stesso
        // brano.
        drop(aprendo);
        avvia_corrente(&app, &stato, !suonava, ms, gesti_prima).map_err(errore)?;
    }
    Ok(true)
}

/// Passa al modo di ripetizione successivo.
#[tauri::command]
pub async fn ripeti(app: Nucleo) -> Esito<()> {
    super::col_lettore_in_disparte(app, "ripeti", ripeti_ora).await
}

/// [`ripeti`], sul posto: lo chiama anche il gesto dell'auto (`auto::esegui`),
/// dal filo dei gesti.
pub fn ripeti_ora(app: Nucleo, stato: StatoLettore) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Ripeti).map_err(errore)? {
        return Ok(());
    }
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

/// Impone un modo di ripetizione.
///
/// La strada dei gesti dei membri, sull'host: arrivano con un modo
/// (`Azione::Ripeti{modo}`) e non con un «il prossimo», perché fra il gesto e
/// il suo arrivo il modo dell'host può essere già cambiato, e ciclare da lì
/// darebbe un modo che nessuno ha scelto. Per il resto è [`ripeti`]: la strada
/// in testa, il brano dopo rimesso in canna, lo stato.
///
/// Non è un comando della finestra: la finestra cicla, con [`ripeti`].
///
/// # Errori
///
/// Quelli della strada del gesto; `internal.aborted` se il lettore non c'è.
pub fn imposta_ripeti(app: &Nucleo, modo: RepeatMode) -> Result<(), AppError> {
    if let Strada::Fatto = strada(app, &Gesto::Ripeti)? {
        return Ok(());
    }
    let stato = app.try_state::<StatoLettore>().ok_or_else(|| {
        AppError::new(rymembra_domain::errors::ErrorCode::InternalAborted {
            what: Some("modo di ripetizione".to_owned()),
        })
    })?;
    con_lettore(&stato, |lettore| {
        if lettore.coda.repeat() == modo {
            return Ok(());
        }
        lettore.coda.set_repeat(modo);
        prepara_prossimo(app, lettore);
        salva_coda(app, lettore);
        manda_stato(app, lettore);
        Ok(())
    })
}

/// Accende o spegne lo shuffle.
#[tauri::command]
pub async fn mescola(app: Nucleo) -> Esito<()> {
    super::col_lettore_in_disparte(app, "mescola", mescola_ora).await
}

/// [`mescola`], sul posto: vedi [`ripeti_ora`].
pub fn mescola_ora(app: Nucleo, stato: StatoLettore) -> Esito<()> {
    if let Strada::Fatto = strada(&app, &Gesto::Shuffle).map_err(errore)? {
        return Ok(());
    }
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
///
/// # In sessione
///
/// La coda è di maniglie, e una maniglia il database non la conosce: la sua
/// scheda la dà la voce ([`ospiti::scheda`]), senza toccare la libreria. Le
/// righe vere si leggono come sempre, tutte insieme; poi si rimette l'ordine
/// chiesto, doppioni compresi, e quel che non c'è più — una riga cancellata,
/// una voce tolta — si salta come prima.
///
/// # Perché `async`
///
/// Perché una coda da «Riproduci tutto» sono tutte le righe della libreria:
/// venticinquemila schede, una quindicina di megabyte di JSON, chieste di nuovo
/// a ogni avvio e a ogni uscita dallo schermo intero, che rimonta la colonna
/// con la mappa vuota. Da comando sincrono era tutto lavoro del filo della
/// finestra. Adesso la lettura va sul pool bloccante con
/// [`crate::disparte::leggi_la_libreria`], come gli altri elenchi; le maniglie
/// si risolvono dopo, a lettura finita, perché `ospiti` non tocca la libreria.
#[tauri::command]
pub async fn brani_per_id(app: Nucleo, brani: Vec<i64>) -> Esito<Vec<TrackSummary>> {
    let righe: Vec<i64> = brani
        .iter()
        .copied()
        .filter(|id| ospiti::e_di_libreria(*id))
        .collect();
    // Tutte righe — fuori sessione, sempre — e la strada di sempre, intera.
    if righe.len() == brani.len() {
        return leggi_la_libreria(app, "righe della coda", move |libreria| {
            rymembra_app::library::summaries_by_id(&libreria.connection, &brani)
        })
        .await
        .map_err(errore);
    }
    let lette = if righe.is_empty() {
        Vec::new()
    } else {
        leggi_la_libreria(app.clone(), "righe della coda", move |libreria| {
            rymembra_app::library::summaries_by_id(&libreria.connection, &righe)
        })
        .await
        .map_err(errore)?
    };
    let per_id: std::collections::HashMap<i64, TrackSummary> = lette
        .into_iter()
        .map(|scheda| (scheda.id, scheda))
        .collect();
    Ok(brani
        .iter()
        .filter_map(|&id| match ospiti::chi(&app, id) {
            ospiti::Chi::Libreria(riga) => per_id.get(&riga).cloned(),
            ospiti::Chi::Voce(voce) => Some(ospiti::scheda(id, &voce)),
            ospiti::Chi::Sparita => None,
        })
        .collect())
}

/// [`brani_per_id`] per un filo che può aspettare: le righe di un tratto di
/// coda, al loro posto, con le voci di sessione risolte dalla loro scheda.
/// `None` dove la riga non c'è più: il posto resta, perché è il posto nella
/// coda che l'auto rimanda indietro per saltarci.
///
/// Lo usa la coda che l'auto mostra, che si compone su un filo suo e non sul
/// pool dei comandi: una cinquantina di righe, non venticinquemila.
///
/// # Errori
///
/// Quelli della libreria.
#[cfg(mobile)]
pub fn righe_della_coda(
    app: &Nucleo,
    brani: &[i64],
) -> Result<Vec<Option<TrackSummary>>, rymembra_domain::AppError> {
    let righe: Vec<i64> = brani
        .iter()
        .copied()
        .filter(|id| ospiti::e_di_libreria(*id))
        .collect();
    let lette = match app.try_state::<crate::stato::Stato>() {
        Some(stato) if !righe.is_empty() => crate::stato::con_libreria(&stato, |libreria| {
            rymembra_app::library::summaries_by_id(&libreria.connection, &righe)
        })?,
        _ => Vec::new(),
    };
    let per_id: std::collections::HashMap<i64, TrackSummary> = lette
        .into_iter()
        .map(|scheda| (scheda.id, scheda))
        .collect();
    Ok(brani
        .iter()
        .map(|&id| match ospiti::chi(app, id) {
            ospiti::Chi::Libreria(riga) => per_id.get(&riga).cloned(),
            ospiti::Chi::Voce(voce) => Some(ospiti::scheda(id, &voce)),
            ospiti::Chi::Sparita => None,
        })
        .collect())
}

#[cfg(test)]
mod prove {
    use super::*;

    // ── la coda di «Sfoglia» che arriva dopo il catalogo (D6) ──

    fn coda_di(tracks: Vec<i64>) -> QueueSnapshot {
        let order = (0..tracks.len()).collect();
        QueueSnapshot {
            order,
            tracks,
            order_pos: Some(0),
            shuffle: false,
            repeat: RepeatMode::Off,
        }
    }

    /// La coda con le voci lontane rimesse prende il posto di quella di
    /// all'avvio solo se nessuno l'ha toccata: nessun brano in mano al motore,
    /// nessuna sessione, e la coda uguale a quella che si era ripresa.
    #[test]
    fn la_coda_con_le_voci_lontane_sostituisce_solo_una_coda_che_nessuno_ha_toccato() {
        let ripresa = coda_di(vec![1, 2]);
        assert!(la_coda_si_puo_sostituire(&ripresa, &ripresa, false, false));
        // La persona ha già fatto partire un brano: la coda è sua.
        assert!(!la_coda_si_puo_sostituire(&ripresa, &ripresa, true, false));
        // Ha accodato qualcosa, o ha cambiato lista.
        assert!(!la_coda_si_puo_sostituire(
            &coda_di(vec![1, 2, 3]),
            &ripresa,
            false,
            false
        ));
        // In sessione la coda è fatta di maniglie e non si tocca.
        assert!(!la_coda_si_puo_sostituire(&ripresa, &ripresa, false, true));
    }

    // ── la coda di prima: vedi `ricorda_coda_di_prima` ──

    /// L'«Annulla» non rimette nella coda un brano che la libreria non ha più:
    /// `tracks.id` non è `AUTOINCREMENT`, e il suo id lo prende un brano nuovo
    /// (S11-1).
    #[test]
    fn la_coda_di_prima_non_nomina_i_brani_tolti() {
        let prima = QueueSnapshot {
            tracks: vec![1, 30_001, 2],
            order: vec![2, 0, 1],
            order_pos: Some(1),
            shuffle: true,
            repeat: RepeatMode::All,
        };
        let senza = senza_i_brani(&prima, &[30_001]).expect("ne nominava uno");
        assert_eq!(senza.tracks, vec![1, 2]);
        assert_eq!(senza.repeat, RepeatMode::All, "il resto non cambia");
        assert!(senza.shuffle);
        let mut ordine = senza.order.clone();
        ordine.sort_unstable();
        assert_eq!(ordine, vec![0, 1], "l'ordine resta una permutazione");
        // Una coda che non li nominava resta com'è: niente da riscrivere.
        assert_eq!(senza_i_brani(&prima, &[99]), None);
    }

    #[test]
    fn svuota_ha_sempre_l_avviso() {
        assert!(merita_l_avviso(true, false, 1));
    }

    /// Una coda costruita a mano vale qualunque sia la lunghezza: sostituita
    /// da un doppio clic su un album, senza avviso non tornerebbe più.
    #[test]
    fn una_coda_curata_corta_ha_l_avviso() {
        assert!(merita_l_avviso(false, true, 3));
    }

    /// Un disco al posto di un altro non è un avviso; una sessione lunga sì.
    #[test]
    fn un_album_al_posto_di_un_altro_no() {
        assert!(!merita_l_avviso(false, false, SOGLIA_DI_UNA_SESSIONE - 1));
        assert!(merita_l_avviso(false, false, SOGLIA_DI_UNA_SESSIONE));
    }

    /// La lista che suona già, riavviata da un altro brano, non ha perso
    /// niente; una coda vuota non ha niente da rimettere. «Svuota» e una lista
    /// diversa sì.
    #[test]
    fn la_stessa_lista_riavviata_non_si_mette_da_parte() {
        assert!(!c_e_da_rimettere(&[1, 2], Some(&[1, 2])));
        assert!(!c_e_da_rimettere(&[], Some(&[1, 2])));
        assert!(!c_e_da_rimettere(&[], None));
        assert!(c_e_da_rimettere(&[1, 2], Some(&[3])));
        assert!(c_e_da_rimettere(&[1, 2], None));
    }

    /// La coda del giro guidato, intatta, non è una coda di qualcuno.
    #[test]
    fn la_coda_del_giro_intatta_non_si_mette_da_parte() {
        assert!(e_la_coda_del_giro(&[1, 2], Some(&[1, 2])));
        assert!(!e_la_coda_del_giro(&[1, 2, 3], Some(&[1, 2])));
        assert!(!e_la_coda_del_giro(&[1, 2], None));
    }

    /// Le righe della coda si leggono sul pool bloccante, non sul filo della
    /// finestra: con una coda da «Riproduci tutto» sono venticinquemila schede
    /// e una quindicina di megabyte di JSON, chieste di nuovo a ogni avvio e a
    /// ogni uscita dallo schermo intero. È la regola di `crate::disparte`: un
    /// comando che legge più di una riga per chiave non è sincrono.
    #[test]
    fn le_righe_della_coda_non_si_leggono_sul_filo_della_finestra() {
        let codice = include_str!("coda.rs");
        let codice = codice.split("\n#[cfg(test)]").next().unwrap_or(codice);
        let inizio = codice
            .find("pub async fn brani_per_id(")
            .expect("`brani_per_id` dev'essere un `async fn`");
        let corpo = &codice[inizio..];
        let corpo = &corpo[..corpo.find("\n}\n").unwrap_or(corpo.len())];
        assert!(
            corpo.contains("leggi_la_libreria("),
            "le righe si leggono con `leggi_la_libreria`, sul pool bloccante"
        );
        assert!(
            !corpo.contains("con_libreria("),
            "una presa diretta della libreria girerebbe su un worker del runtime"
        );
    }

    #[test]
    fn le_latenze_lette_per_intero_non_scrivono_niente() {
        let mut voci = playback::LatenzePerUscita::new();
        voci.insert(
            "Cuffie di Mario".to_owned(),
            playback::LatenzaUscita {
                ms: 180,
                fonte: playback::FonteLatenza::Microfono,
                verificata: true,
                quando_ms: 0,
            },
        );
        let lette = playback::LatenzeLette {
            voci,
            illeggibile: false,
            scartate: 0,
        };
        assert_eq!(riga_delle_latenze(&lette), None);
    }

    #[test]
    fn la_riga_delle_latenze_non_nomina_nessuna_uscita() {
        // La riga si costruisce da un `bool` e da un numero: la prova tiene
        // vero che resti così anche il giorno in cui qualcuno vorrà
        // «aggiungere il nome, per capire meglio».
        let mut voci = playback::LatenzePerUscita::new();
        voci.insert(
            "Cuffie di Mario".to_owned(),
            playback::LatenzaUscita {
                ms: 180,
                fonte: playback::FonteLatenza::Mano,
                verificata: false,
                quando_ms: 0,
            },
        );
        for lette in [
            playback::LatenzeLette {
                voci,
                illeggibile: false,
                scartate: 2,
            },
            playback::LatenzeLette {
                voci: playback::LatenzePerUscita::new(),
                illeggibile: true,
                scartate: 0,
            },
        ] {
            let riga = riga_delle_latenze(&lette).expect("c'è qualcosa da dire");
            assert!(riga.starts_with("[riproduzione] "), "{riga}");
            assert!(!riga.contains("Mario"), "{riga}");
        }
    }
}
