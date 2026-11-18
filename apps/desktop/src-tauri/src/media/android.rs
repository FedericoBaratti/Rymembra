//! La sessione multimediale di Android.
//!
//! È la metà di [`super`] che su Android parla col sistema: la notifica del
//! lettore, la schermata di blocco, i tasti delle cuffie, e Android Auto. Il
//! lato Kotlin — un `MediaLibraryService` di Media3 con un giocatore che non
//! suona niente, e riflette soltanto — sta nel progetto Gradle; da qui ci si
//! arriva con [`rymembra_android::sessione`] e [`rymembra_android::auto`].
//!
//! # Perché conta più che su Windows
//!
//! Sul computer la scheda nel riquadro del volume è una comodità. Su Android il
//! servizio in primo piano che regge la notifica è **la ragione per cui la
//! musica continua** a schermo spento: senza, il sistema considera l'app ferma
//! appena la si lascia, e prima o poi la chiude. Ed è da lì che passa l'auto,
//! che sfoglia la libreria e sceglie che cosa suonare.
//!
//! # I tre fili
//!
//! Kotlin non chiama Rust: mette in coda, e un filo di qui prende. Ce ne sono
//! tre, perché le tre code non devono aspettarsi l'una con l'altra:
//!
//! - **i gesti** ([`ascolta_i_gesti`]): i tasti delle cuffie e della notifica,
//!   e quel che si sceglie sull'auto. Li consegna a [`super::premi`], la
//!   strada dei tasti della scheda di Windows e del vassoio;
//! - **le domande dell'auto** ([`rispondi_all_auto`]): che cosa c'è dentro un
//!   album, una playlist, una ricerca. Una lettura della libreria ciascuna, e
//!   un «pausa» premuto intanto non deve aspettarla;
//! - **la coda** ([`componi_la_coda`]): il tratto di coda che l'auto mostra,
//!   riletto dalla libreria quando cambia. Dal seguito del lettore parte solo
//!   un avviso: la lettura la fa questo filo, lontano dallo stato che si sta
//!   pubblicando.

use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use rymembra_android::auto::Richiesta;
use rymembra_android::sessione::{self, Coda, Gesto as GestoAndroid, Scheda, VoceCoda};
use rymembra_app::covers::CoverStore;
use tauri::{Listener as _, Manager as _};

use super::{Gesto, StatoMedia, avanti_entro, premi};
use crate::auto::{self, GestoAuto, Nodo, RicercaVocale};
use crate::nota;
use crate::riproduzione::StatoRiproduzione;
use crate::stato::Stato;

/// Quanto aspetta un filo prima di riguardare se l'app sta uscendo.
///
/// Un tasto arriva subito, comunque: è solo il tetto dell'attesa a vuoto.
const ATTESA: Duration = Duration::from_secs(5);

/// Quanti brani della coda prima del corrente si mostrano all'auto.
const CODA_PRIMA: usize = 10;

/// Quanti dopo.
const CODA_DOPO: usize = 40;

/// Un tratto di coda da comporre: da quale posto parte, e i brani.
type Tratto = (usize, Vec<i64>);

/// Quel che serve a riempire la sessione, e quel che le si è già mandato.
pub(super) struct Controlli {
    /// Dove stanno le copertine su disco.
    copertine: CoverStore,
    /// L'ultima scheda consegnata, senza la posizione. Vedi [`aggiorna`].
    mostrata: Option<Scheda>,
    /// L'ultimo tratto di coda chiesto al filo della coda.
    tratto: Option<Tratto>,
    /// Il filo della coda. Vedi [`componi_la_coda`].
    coda: Option<Sender<Tratto>>,
}

/// Prepara la sessione e fa partire i fili.
///
/// La sessione vera — il servizio, la notifica — la crea Kotlin alla prima
/// scheda, o l'ha già creata se a svegliare l'app è stata l'auto: qui le si
/// dice soltanto che il nucleo c'è ([`sessione::pronta`]), con una chiamata
/// che non aspetta niente. Questa funzione gira nel `setup`, sul filo
/// principale, e quel filo non deve aspettare.
pub fn avvia(app: &tauri::AppHandle) {
    let stato_app = app.state::<Stato>();
    let Ok(copertine) = stato_app.copertine() else {
        nota!("[media] la libreria non è aperta: niente sessione multimediale");
        return;
    };
    let radice = copertine.radice().to_path_buf();

    let coda = parti(
        "rymembra-coda-auto",
        app,
        |mano, ricevi: Receiver<Tratto>| {
            componi_la_coda(&mano, &ricevi);
        },
    );
    *app.state::<StatoMedia>()
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(Controlli {
        copertine,
        mostrata: None,
        tratto: None,
        coda,
    });

    let mano = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("rymembra-gesti".to_owned())
        .spawn(move || ascolta_i_gesti(&mano))
    {
        nota!("[media] il filo dei gesti non è partito: la notifica non comanda niente: {err}");
    }
    let mano = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("rymembra-auto".to_owned())
        .spawn(move || rispondi_all_auto(&mano))
    {
        nota!("[media] il filo dell'auto non è partito: l'auto non vede la libreria: {err}");
    }

    if let Err(err) = sessione::pronta(&radice) {
        nota!("[media] la sessione non sa che il nucleo è partito: {err}");
    }

    // Una scansione finita è una libreria nuova anche per l'auto: le schede
    // che ha aperte si rileggono, invece di restare quelle di prima finché
    // qualcuno non torna indietro e rientra.
    app.listen_any("scansione:finita", |_| {
        if let Err(err) = sessione::libreria_cambiata() {
            nota!("[media] l'auto non sa che la libreria è cambiata: {err}");
        }
    });
}

/// Fa partire un filo che lavora su un canale, e ne dà il lato che manda.
fn parti<T: Send + 'static>(
    nome: &str,
    app: &tauri::AppHandle,
    lavoro: impl FnOnce(tauri::AppHandle, Receiver<T>) + Send + 'static,
) -> Option<Sender<T>> {
    let (manda, ricevi) = std::sync::mpsc::channel();
    let mano = app.clone();
    match std::thread::Builder::new()
        .name(nome.to_owned())
        .spawn(move || lavoro(mano, ricevi))
    {
        Ok(_) => Some(manda),
        Err(err) => {
            nota!("[media] il filo {nome} non è partito: {err}");
            None
        }
    }
}

/// Il filo dei gesti: aspetta un tasto, lo consegna, ricomincia.
///
/// Un guasto del ponte non lo ferma — si scrive e si riprova dopo una pausa —
/// perché un filo dei gesti morto vuol dire una notifica che mostra i tasti e
/// non risponde, e quello non lo racconta nessuno.
fn ascolta_i_gesti(app: &tauri::AppHandle) {
    let mut ultimo_guasto: Option<String> = None;
    while !crate::spegnimento::in_uscita() {
        match sessione::aspetta_gesto(ATTESA) {
            Ok(Some(gesto)) => {
                ultimo_guasto = None;
                // Dentro `sorvegliato`: un panico nella traduzione lasciava la
                // notifica con i tasti che non rispondevano più (R23-10).
                let _ = crate::stato::sorvegliato("rymembra-gesti", || {
                    if let Some(gesto) = traduci(app, gesto) {
                        premi(app, gesto);
                    }
                });
            }
            Ok(None) => {}
            Err(err) => guasto(&mut ultimo_guasto, "i gesti non arrivano", &err),
        }
    }
}

/// Scrive un guasto del ponte una volta sola finché resta lo stesso, e lascia
/// passare un po' prima di riprovare.
fn guasto(ultimo: &mut Option<String>, cosa: &str, err: &rymembra_android::Errore) {
    let testo = err.to_string();
    if ultimo.as_ref() != Some(&testo) {
        nota!("[media] {cosa}: {testo}");
    }
    *ultimo = Some(testo);
    std::thread::sleep(ATTESA);
}

/// Il gesto di Android, nel gesto di Rymembra.
///
/// I salti relativi passano dallo stesso tetto della scheda di Windows: un
/// «avanti di dieci secondi» premuto a tre dalla fine non cambia brano. Vedi
/// [`avanti_entro`].
fn traduci(app: &tauri::AppHandle, gesto: GestoAndroid) -> Option<Gesto> {
    let media = app.try_state::<StatoMedia>()?;
    let adesso = media.posizione_ms.load(Ordering::Relaxed);
    let durata = media.durata_ms.load(Ordering::Relaxed);
    Some(match gesto {
        GestoAndroid::Riprendi => Gesto::Riprendi,
        GestoAndroid::Pausa => Gesto::Pausa,
        GestoAndroid::Prossimo => Gesto::Prossimo,
        GestoAndroid::Precedente => Gesto::Precedente,
        GestoAndroid::VaiA(ms) => Gesto::VaiA(super::dentro_il_brano(app, ms)),
        GestoAndroid::Avanti(quanto) => Gesto::VaiA(avanti_entro(adesso, quanto, durata)?),
        GestoAndroid::Indietro(quanto) => Gesto::VaiA(adesso.saturating_sub(quanto)),
        GestoAndroid::Suona(voce) => Gesto::Auto(GestoAuto::Suona(voce)),
        GestoAndroid::Cerca(ricerca) => Gesto::Auto(GestoAuto::Cerca(RicercaVocale {
            testo: ricerca.testo,
            artista: ricerca.artista,
            album: ricerca.album,
            titolo: ricerca.titolo,
        })),
        GestoAndroid::Coda(posto) => Gesto::Auto(GestoAuto::Coda(posto)),
        GestoAndroid::Collega(link) => {
            crate::lontano::link_arrivato(app, &link);
            return None;
        }
        GestoAndroid::CambioDiRete => {
            crate::lontano::cambio_di_rete(app);
            return None;
        }
        GestoAndroid::Mescola => Gesto::Auto(GestoAuto::Mescola),
        GestoAndroid::Ripeti => Gesto::Auto(GestoAuto::Ripeti),
        GestoAndroid::Piace => {
            // Il brano e il cuore di **quel che l'auto vede**: l'ultima scheda
            // consegnata. Un «mi piace» premuto mentre il brano cambia vale
            // per quello che c'era sullo schermo quando lo si è premuto.
            let dentro = media
                .controlli
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let scheda = dentro.as_ref()?.mostrata.as_ref()?;
            if !scheda.brano || crate::riproduzione::riga_di_libreria(scheda.id).is_err() {
                return None;
            }
            Gesto::Auto(GestoAuto::Piace {
                id: scheda.id,
                valore: !scheda.piace,
            })
        }
    })
}

/// Il filo delle domande dell'auto: aspetta una domanda, legge, risponde.
fn rispondi_all_auto(app: &tauri::AppHandle) {
    let mut ultimo_guasto: Option<String> = None;
    while !crate::spegnimento::in_uscita() {
        match rymembra_android::auto::aspetta_richiesta(ATTESA) {
            Ok(Some(domanda)) => {
                ultimo_guasto = None;
                let id = domanda.id();
                // Dentro `sorvegliato`: un panico nella risposta lasciava
                // l'auto senza libreria per il resto del processo (R23-10).
                let risposto =
                    crate::stato::sorvegliato("rymembra-auto", || risposta(app, domanda))
                        .unwrap_or_else(|| {
                            Err(rymembra_domain::AppError::new(
                                rymembra_domain::ErrorCode::InternalAborted {
                                    what: Some("risposta all'auto".to_owned()),
                                },
                            )
                            .with_cause("la risposta è caduta"))
                        });
                let esito = match risposto {
                    Ok(voci) => rymembra_android::auto::rispondi(id, &voci),
                    Err(err) => {
                        let codice = err.code().kind().code();
                        nota!("[auto] {codice} causa={}", err.cause().unwrap_or_default());
                        rymembra_android::auto::rifiuta(id, codice)
                    }
                };
                if let Err(err) = esito {
                    nota!("[auto] la risposta non arriva: {err}");
                }
            }
            Ok(None) => {}
            Err(err) => guasto(
                &mut ultimo_guasto,
                "le domande dell'auto non arrivano",
                &err,
            ),
        }
    }
}

/// La risposta a una domanda dell'auto: delle voci.
fn risposta(
    app: &tauri::AppHandle,
    domanda: Richiesta,
) -> Result<Vec<auto::Voce>, rymembra_domain::AppError> {
    match domanda {
        Richiesta::Figli {
            genitore,
            pagina,
            quante,
            ..
        } => {
            let nodo = Nodo::da_testo(&genitore).ok_or_else(|| sconosciuta(&genitore))?;
            auto::leggi(app, |connessione, origine, store| {
                auto::figli(connessione, origine, store, &nodo, pagina, quante)
            })
        }
        Richiesta::Voce { voce, .. } => {
            let nodo = Nodo::da_testo(&voce).ok_or_else(|| sconosciuta(&voce))?;
            auto::leggi(app, |connessione, origine, store| {
                auto::voce(connessione, origine, store, &nodo)
                    .map(|trovata| trovata.into_iter().collect())
            })
        }
        Richiesta::Cerca {
            testo,
            pagina,
            quante,
            ..
        } => auto::leggi(app, |connessione, origine, store| {
            auto::figli(
                connessione,
                origine,
                store,
                &Nodo::Cerca(testo),
                pagina,
                quante,
            )
        }),
    }
}

fn sconosciuta(voce: &str) -> rymembra_domain::AppError {
    rymembra_domain::AppError::new(rymembra_domain::ErrorCode::LibraryTrackNotFound {
        track_id: None,
    })
    .with_cause(format!("voce sconosciuta: «{voce}»"))
}

/// Il filo della coda: a ogni tratto nuovo, le sue righe, e alla sessione.
///
/// Se ne arrivano più d'uno mentre si legge, vale l'ultimo: quelli in mezzo
/// non li vedrebbe nessuno.
fn componi_la_coda(app: &tauri::AppHandle, ricevi: &Receiver<Tratto>) {
    while let Ok(mut tratto) = ricevi.recv() {
        while let Ok(dopo) = ricevi.try_recv() {
            tratto = dopo;
        }
        // Dentro `sorvegliato`: un panico qui lasciava l'auto senza coda per
        // il resto del processo (R23-10).
        let _ = crate::stato::sorvegliato("rymembra-coda-auto", || componi(app, tratto));
    }
}

/// Un tratto di coda per l'auto: le sue righe, alla sessione.
fn componi(app: &tauri::AppHandle, tratto: Tratto) {
    let (inizio, ids) = tratto;
    let righe = match crate::riproduzione::righe_della_coda(app, &ids) {
        Ok(righe) => righe,
        Err(err) => {
            nota!(
                "[media] la coda per l'auto non si legge: {} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or_default()
            );
            return;
        }
    };
    let copertine = app
        .try_state::<Stato>()
        .and_then(|stato| stato.copertine().ok());
    let voci = righe
        .into_iter()
        .enumerate()
        .filter_map(|(dove, riga)| {
            let riga = riga?;
            Some(VoceCoda {
                posto: inizio.saturating_add(dove),
                id: riga.id,
                titolo: riga.title,
                artista: riga.artist,
                album: riga.album,
                copertina: copertine
                    .as_ref()
                    .and_then(|store| auto::copertina(store, riga.cover_art_hash.as_deref())),
                durata_ms: u64::try_from(riga.duration_ms).unwrap_or(0),
            })
        })
        .collect();
    if let Err(err) = sessione::coda(&Coda { voci }) {
        nota!("[media] la sessione non accetta la coda: {err}");
    }
}

/// Toglie la notifica e ferma il servizio, all'uscita.
pub fn stacca(app: &tauri::AppHandle) {
    if let Some(stato) = app.try_state::<StatoMedia>() {
        drop(
            stato
                .controlli
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }
    if let Err(err) = sessione::stacca() {
        nota!("[media] la sessione non si stacca: {err}");
    }
}

/// Dice all'auto che l'ultimo gesto non ha portato a niente. Vedi
/// [`sessione::errore`].
pub(super) fn non_riuscito(codice: &str) {
    if let Err(err) = sessione::errore(codice) {
        nota!("[media] la sessione non riceve l'avviso: {err}");
    }
}

/// La parte di [`super::aggiorna`] che tocca Android.
///
/// La scheda parte a ogni stato **con** la posizione: Media3 la estrapola da
/// sé finché la musica suona, e la riallinea a ogni scheda — un salto, una
/// pausa. Si confronta però senza la posizione, per scrivere nel diario una
/// volta sola quando il ponte rifiuta lo stesso brano.
///
/// Se è cambiato il tratto di coda attorno al brano, lo si manda a comporre al
/// filo della coda: qui si confrontano numeri e basta.
pub(super) fn aggiorna(media: &StatoMedia, stato: &StatoRiproduzione) {
    let mut dentro = media
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(controlli) = dentro.as_mut() else {
        return;
    };
    let scheda = scheda_di(stato, &controlli.copertine);
    let senza_posizione = Scheda {
        posizione_ms: 0,
        ..scheda.clone()
    };
    match sessione::aggiorna(&scheda) {
        Ok(()) => controlli.mostrata = Some(senza_posizione),
        Err(err) => {
            if controlli.mostrata.as_ref() != Some(&senza_posizione) {
                nota!(
                    "[media] la sessione non accetta il brano {:?}: {err}",
                    stato.brano.as_ref().map(|brano| brano.id)
                );
            }
            controlli.mostrata = Some(senza_posizione);
        }
    }

    let tratto = tratto_di(&stato.coda, stato.posizione_coda);
    if controlli.tratto.as_ref() != Some(&tratto) {
        if let Some(coda) = controlli.coda.as_ref() {
            // Un filo che non c'è più: l'auto resta con la coda di prima, che
            // è meno grave di uno stato che non parte.
            let _ = coda.send(tratto.clone());
        }
        controlli.tratto = Some(tratto);
    }
}

/// Il tratto di coda attorno a `posto`: [`CODA_PRIMA`] brani prima,
/// [`CODA_DOPO`] dopo. Senza un posto, l'inizio della coda.
fn tratto_di(coda: &[i64], posto: Option<usize>) -> Tratto {
    let centro = posto.unwrap_or(0);
    let inizio = centro.saturating_sub(CODA_PRIMA).min(coda.len());
    let fine = centro
        .saturating_add(CODA_DOPO)
        .saturating_add(1)
        .min(coda.len());
    (inizio, coda.get(inizio..fine).unwrap_or_default().to_vec())
}

/// Quel che la notifica deve mostrare per questo stato.
fn scheda_di(stato: &StatoRiproduzione, copertine: &CoverStore) -> Scheda {
    let dopo = stato
        .posizione_coda
        .is_some_and(|dove| dove.saturating_add(1) < stato.coda.len())
        || stato.ripeti == "all";
    let Some(brano) = stato.brano.as_ref() else {
        return Scheda {
            id: 0,
            titolo: String::new(),
            artista: String::new(),
            album: String::new(),
            copertina: None,
            durata_ms: 0,
            posizione_ms: 0,
            suona: false,
            brano: false,
            dopo,
            piace: false,
            mescola: stato.shuffle,
            ripeti: stato.ripeti.clone(),
            indice_coda: stato.posizione_coda,
        };
    };
    Scheda {
        id: brano.id,
        titolo: brano.title.clone(),
        artista: brano.artist.clone(),
        album: brano.album.clone(),
        copertina: auto::copertina_grande(copertine, brano.cover_art_hash.as_deref()),
        durata_ms: stato.durata_ms,
        posizione_ms: stato.posizione_ms,
        suona: !stato.in_pausa,
        brano: true,
        dopo,
        piace: brano.liked,
        mescola: stato.shuffle,
        ripeti: stato.ripeti.clone(),
        indice_coda: stato.posizione_coda,
    }
}
