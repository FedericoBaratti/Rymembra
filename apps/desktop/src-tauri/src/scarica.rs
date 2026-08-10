//! La coda che scarica i desiderati, dal lato della finestra.
//!
//! Tre comandi, due eventi e un filo di sottofondo. Le decisioni stanno in
//! `aether-domain` (quale video), il dialogo con yt-dlp in `aether-yt`, il
//! database in `aether-app`: qui si tiene il tempo e si prende il lucchetto —
//! due cose che nessuno di quei tre sa fare, e che sono anche le due che possono
//! far impuntare la riproduzione.
//!
//! # L'ordine dei lucchetti
//!
//! Come `nuvola` dichiara il suo, questo modulo dichiara il proprio:
//!
//! > **Il filo della coda prende il lucchetto della libreria solo in finestre
//! > brevi, e mai mentre yt-dlp gira.**
//!
//! Il ciclo alterna in modo stretto:
//!
//! ```text
//! con_libreria → leggi un lotto di desiderati  → rilascia
//! (senza lucchetto) cerca su YouTube, scarica, scrivi i tag
//! con_libreria → segna l'esito                 → rilascia
//! ```
//!
//! Qui la regola pesa più che altrove. Una richiesta di rete dura un secondo;
//! **uno scaricamento dura un minuto**, e la coda ne fa cento di fila. Tenere il
//! lucchetto per tutta la coda vorrebbe dire un'applicazione che per un'ora non
//! risponde: niente riproduzione, niente ricerca, niente scansione.
//!
//! Che sia vero non è affidato all'attenzione di chi legge: `aether-yt` non
//! riceve mai una `rusqlite::Connection`, quindi il codice che terrebbe il
//! lucchetto durante uno scaricamento non si può nemmeno scrivere.
//!
//! # Perché tre fili e non uno, e non dieci
//!
//! Tre come `SPOTIFY_TRACK_CONCURRENCY` del vecchio albero. Uno solo lascia il
//! collegamento vuoto — YouTube strozza per connessione, non per client. Dieci
//! si prendono un `429`, e a quel punto tutti e dieci aspettano.
//!
//! # Perché un `AtomicBool` e non un canale
//!
//! Stessa ragione scritta per esteso in [`crate::stato`] a proposito di
//! `annulla_scansione`: chi chiede di fermarsi non deve mettersi in coda dietro
//! ciò che vuole fermare.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use aether_app::desiderati::{self, Desiderato};
use aether_app::settings;
use aether_app::tag_scrittura::scrivi_tag;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::paths::PathRules;
use aether_domain::yt_match::{destinazione, scegli_candidato};
use aether_net::http::Rete;
use aether_yt::binario::Binari;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Quanti brani si scaricano insieme.
const FILI: usize = 3;

/// Quanti desiderati si leggono per volta.
///
/// Un lotto e non tutta la tabella: la coda può essere di migliaia di righe, e
/// tenerle in memoria per rileggerne lo stato subito dopo non serve a niente.
/// Un lotto per giro tiene la finestra del lucchetto corta e permette che un
/// «riprova i falliti» arrivato a metà entri nel giro successivo.
const LOTTO: usize = 24;

/// Ogni quanti punti percentuali si manda un evento per il brano in corso.
///
/// yt-dlp annuncia l'avanzamento decine di volte al secondo. Mandarne uno per
/// annuncio inonderebbe il canale IPC per muovere una barra di meno di un pixel
/// — è la stessa strozzatura di `riordino` e `comandi`, con la stessa ragione.
const PASSO_FRAZIONE: u32 = 2;

// ── lo stato condiviso ──────────────────────────────────────────────────────

/// Lo stato della coda di scaricamento.
pub struct StatoScarico {
    /// Una coda sta girando. Impedisce che due partano insieme.
    ///
    /// Non è pignoleria: due code che leggono lo stesso lotto scaricherebbero
    /// gli stessi brani due volte, sullo stesso percorso, con yt-dlp che scrive
    /// lo stesso file da due processi.
    in_corso: AtomicBool,
    /// Qualcuno ha chiesto di fermarsi.
    da_fermare: AtomicBool,
    /// Quanti ne restano, per chi chiede lo stato senza aspettare un evento.
    rimasti: AtomicU32,
    /// Quanti ne sono stati presi in questa passata.
    fatti: AtomicU32,
    /// Dove stanno i binari esterni.
    cartella_binari: PathBuf,
}

impl StatoScarico {
    /// Lo stato, con la cartella in cui cercare yt-dlp.
    #[must_use]
    pub fn nuovo(cartella_binari: PathBuf) -> Self {
        Self {
            in_corso: AtomicBool::new(false),
            da_fermare: AtomicBool::new(false),
            rimasti: AtomicU32::new(0),
            fatti: AtomicU32::new(0),
            cartella_binari,
        }
    }

    /// Prova a prendere il posto della coda. `false` se ce n'è già una.
    fn prendi_posto(&self) -> bool {
        self.in_corso
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Lascia il posto.
    fn lascia_posto(&self) {
        self.in_corso.store(false, Ordering::Release);
    }

    /// Chiede alla coda in corso di fermarsi.
    pub fn ferma(&self) {
        self.da_fermare.store(true, Ordering::Relaxed);
    }

    /// Se qualcuno ha chiesto di fermarsi.
    fn fermata(&self) -> bool {
        self.da_fermare.load(Ordering::Relaxed)
    }
}

// ── quel che attraversa l'IPC ───────────────────────────────────────────────

/// Come sta la coda.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoCoda {
    /// Sta girando.
    pub attiva: bool,
    /// Quanti sono stati presi in questa passata.
    pub fatti: u32,
    /// Quanti ne restano da prendere.
    pub rimasti: u32,
    /// I conteggi per stato, dal database.
    pub conteggi: desiderati::Conteggi,
    /// Gli stessi conteggi, ma una riga per importazione.
    ///
    /// Il totale della coda non basta a chi ha importato due playlist: dice
    /// «31 su 74» e nasconde quale delle due sta scendendo. Vedi
    /// [`desiderati::per_sorgente`].
    pub sorgenti: Vec<desiderati::Sorgente>,
    /// yt-dlp è al suo posto.
    pub ytdlp: bool,
}

/// L'avanzamento della coda nel suo insieme, e di ogni importazione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Avanzamento {
    fatti: u32,
    totale: u32,
    sorgenti: Vec<desiderati::Sorgente>,
}

/// Cosa sta succedendo a un brano.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventoBrano {
    /// Il titolo secondo Spotify.
    titolo: String,
    /// L'interprete.
    artista: Option<String>,
    /// Da 0 a 1 mentre scende; `None` quando sta ancora cercando.
    frazione: Option<f32>,
    /// `cerco`, `scarico`, `fatto`, `fallito`, `introvabile`.
    esito: &'static str,
    /// Il codice del catalogo, quando è andata male.
    codice: Option<String>,
    /// Da quale importazione viene: l'identificativo del contenitore.
    ///
    /// Senza, un elenco di importazioni non saprebbe sotto quale riga mettere
    /// «sto cercando su YouTube…», e lo metterebbe sotto tutte.
    sorgente_id: String,
    /// Il nome di quel contenitore.
    provenienza: String,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Avvia la coda di scaricamento.
///
/// Torna subito: la coda gira su un filo suo. Chi la mostra segue gli eventi
/// `scarico:avanzamento` e `scarico:brano`.
#[tauri::command]
pub fn scarica_desiderati(app: AppHandle) -> Esito<StatoCoda> {
    avvia(&app);
    scarico_stato(app.state(), app.state())
}

/// Chiede alla coda in corso di fermarsi.
///
/// Non chiede il lucchetto della libreria — e non può: quel lucchetto ce l'ha
/// la coda, a intermittenza, per tutta la sua durata. Alza un bit, e la coda lo
/// legge fra un brano e l'altro e mentre yt-dlp gira.
#[tauri::command]
pub fn annulla_scarico(scarico: State<'_, StatoScarico>) -> Esito<()> {
    scarico.ferma();
    Ok(())
}

/// Come sta la coda.
#[tauri::command]
pub fn scarico_stato(
    stato: State<'_, Stato>,
    scarico: State<'_, StatoScarico>,
) -> Esito<StatoCoda> {
    // Le due letture sotto lo **stesso** lucchetto: prenderlo due volte
    // lascerebbe passare in mezzo un brano che si conclude, e i totali
    // direbbero una cosa mentre le righe per importazione ne dicono un'altra.
    let (conteggi, sorgenti) = con_libreria(&stato, |libreria| {
        Ok((
            desiderati::conteggi(&libreria.connection)?,
            desiderati::per_sorgente(&libreria.connection)?,
        ))
    })
    .map_err(errore)?;
    Ok(StatoCoda {
        attiva: scarico.in_corso.load(Ordering::Acquire),
        fatti: scarico.fatti.load(Ordering::Relaxed),
        rimasti: scarico.rimasti.load(Ordering::Relaxed),
        conteggi,
        sorgenti,
        ytdlp: Binari::risolvi(scarico.cartella_binari.clone())
            .ytdlp()
            .is_some(),
    })
}

/// Rimette in coda i falliti e riavvia.
#[tauri::command]
pub fn riprova_falliti(app: AppHandle, stato: State<'_, Stato>) -> Esito<u32> {
    let quanti = con_libreria(&stato, |libreria| {
        desiderati::riprova_falliti(&libreria.connection)
    })
    .map_err(errore)?;
    if quanti > 0 {
        avvia(&app);
    }
    Ok(quanti)
}

// ── il filo ─────────────────────────────────────────────────────────────────

/// Avvia la coda, se non ce n'è già una e se c'è qualcosa da fare.
///
/// Pubblico perché lo chiama anche [`crate::spotify::spotify_importa`]: è
/// l'«in modo automatico» chiesto: confermata un'importazione, i brani mancanti
/// cominciano a scendere da soli.
pub fn avvia(app: &AppHandle) {
    let scarico = app.state::<StatoScarico>();
    if !scarico.prendi_posto() {
        // Ce n'è già una. I brani appena importati li prenderà lei al prossimo
        // lotto: la coda rilegge la tabella a ogni giro, quindi non si perde
        // niente e non serve accodare una seconda passata.
        //
        // Si annuncia lo stesso, però: chi ha appena confermato la seconda
        // importazione deve vederla comparire adesso, non fra un minuto quando
        // finisce il brano che era in corso. Un elenco che non nomina quel che
        // hai appena fatto assomiglia a un'importazione non riuscita.
        annuncia(app);
        return;
    }
    // Prima di partire: un annullamento arrivato dopo la fine della coda
    // precedente fermerebbe questa al primo brano.
    scarico.da_fermare.store(false, Ordering::Relaxed);
    scarico.fatti.store(0, Ordering::Relaxed);

    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-scarico".to_owned())
        .spawn(move || {
            passata(&manico);
            manico.state::<StatoScarico>().lascia_posto();
            let _ = manico.emit("scarico:finito", ());
        });
    if let Err(err) = avviato {
        eprintln!("[scarico] il filo della coda non è partito: {err}");
        scarico.lascia_posto();
    }
}

/// Una passata completa: legge lotti finché ce n'è, o finché non si ferma.
fn passata(app: &AppHandle) {
    let scarico = app.state::<StatoScarico>();
    let binari = Binari::risolvi(scarico.cartella_binari.clone());

    // Il binario si controlla **una volta**, prima di toccare la tabella. Senza,
    // una coda di cento brani segnerebbe cento fallimenti identici e
    // brucerebbe i tentativi di tutti per un guasto che non c'entra con loro.
    if let Err(mancante) = binari.richiedi_ytdlp() {
        segnala_guasto(app, &mancante);
        return;
    }

    let Some(cartella) = cartella_download(app) else {
        segnala_guasto(
            app,
            &AppError::new(ErrorCode::FsNotFound {
                path: "cartella di scaricamento".to_owned(),
            })
            .with_cause(
                "nessuna cartella sorvegliata: i file scaricati non sarebbero trovati da nessuna scansione"
                    .to_owned(),
            ),
        );
        return;
    };
    let temporanea = cartella_temporanea(app);

    // Un annuncio prima di cominciare. Serve a chi apre l'elenco subito dopo un
    // riavvio con una coda rimasta in sospeso: senza, i numeri resterebbero
    // quelli dell'interrogazione iniziale finché non finisce il primo brano.
    annuncia(app);

    let mut presi_qualcosa = false;
    loop {
        if scarico.fermata() {
            break;
        }
        let lotto = match con_libreria(&app.state::<Stato>(), |libreria| {
            desiderati::leggi_da_scaricare(&libreria.connection, LOTTO)
        }) {
            Ok(lotto) => lotto,
            Err(guasto) => {
                segnala_guasto(app, &guasto);
                break;
            }
        };
        if lotto.is_empty() {
            break;
        }
        presi_qualcosa = true;
        esegui_lotto(app, &binari, &cartella, &temporanea, lotto);
    }

    // La scansione finale, e solo se qualcosa è arrivato: è ciò che fa entrare i
    // file in libreria da soli. Senza, l'utente vedrebbe la coda finire e la
    // libreria ferma, e dovrebbe indovinare che gli tocca premere «Scansiona».
    if presi_qualcosa && scarico.fatti.load(Ordering::Relaxed) > 0 {
        rientra_in_libreria(app);
    }
}

/// Scarica un lotto, con [`FILI`] brani insieme.
fn esegui_lotto(
    app: &AppHandle,
    binari: &Binari,
    cartella: &Path,
    temporanea: &Path,
    lotto: Vec<Desiderato>,
) {
    let scarico = app.state::<StatoScarico>();
    let totale = totale_da_fare(app);
    scarico.rimasti.store(totale, Ordering::Relaxed);

    // Una coda condivisa e non una fetta per filo: i brani non durano uguale, e
    // dividerli in tre parti uguali lascerebbe due fili fermi ad aspettare il
    // terzo che è capitato sui video lunghi.
    let coda = Arc::new(Mutex::new(lotto.into_iter().collect::<Vec<_>>()));
    let quanti = FILI.min(coda.lock().map_or(1, |c| c.len()).max(1));

    std::thread::scope(|ambito| {
        for _ in 0..quanti {
            let coda = Arc::clone(&coda);
            ambito.spawn(move || {
                loop {
                    if app.state::<StatoScarico>().fermata() {
                        break;
                    }
                    let Some(desiderato) = coda.lock().ok().and_then(|mut c| c.pop()) else {
                        break;
                    };
                    un_brano(app, binari, cartella, temporanea, &desiderato);
                }
            });
        }
    });
}

/// Un brano solo, dall'inizio alla fine.
///
/// Ogni esito passa da uno dei tre `segna_*`: una riga che resta in `attesa`
/// senza che nessuno l'abbia toccata tornerebbe nel lotto successivo, e la coda
/// girerebbe su di lei per sempre.
fn un_brano(
    app: &AppHandle,
    binari: &Binari,
    cartella: &Path,
    temporanea: &Path,
    desiderato: &Desiderato,
) {
    let scarico = app.state::<StatoScarico>();
    let fermare = || app.state::<StatoScarico>().fermata();
    emetti_brano(app, desiderato, None, "cerco", None);

    // ── la ricerca, fuori da ogni lucchetto ─────────────────────────────────
    let candidati = match aether_yt::cerca(binari, &desiderato.brano, &fermare) {
        Ok(candidati) => candidati,
        Err(guasto) => return chiudi_male(app, desiderato, &guasto),
    };
    let Some(scelto) = scegli_candidato(&candidati, &desiderato.brano) else {
        // «Ho cercato e non c'è» è terminale, e va distinto da «non sono
        // riuscito a cercare», che è passeggero: vedi `aether_yt::ricerca`.
        let motivo = AppError::new(ErrorCode::DownloadNoResults);
        segna(app, desiderato, |connection| {
            desiderati::segna_introvabile(connection, desiderato.id, &codice_e_causa(&motivo))
        });
        emetti_brano(app, desiderato, None, "introvabile", Some(&motivo));
        return;
    };
    let url = scelto.url.clone();

    // ── lo scaricamento, fuori da ogni lucchetto ────────────────────────────
    // La posizione si conta da 1: `posizione` è l'indice nel contenitore.
    let numero = desiderato.posizione.saturating_add(1);
    let dove = destinazione(&desiderato.brano, numero);
    let mut ultima = 0_u32;
    let percorso = aether_yt::scarica(
        binari,
        &aether_yt::Richiesta {
            url: &url,
            cartella_download: cartella,
            cartella_temporanea: temporanea,
            base_relativa: &dove.relativo(),
        },
        &fermare,
        &mut |frazione| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "la frazione arriva già limitata fra 0 e 1"
            )]
            let percento = (frazione * 100.0).clamp(0.0, 100.0) as u32;
            if percento >= ultima.saturating_add(PASSO_FRAZIONE) || percento >= 100 {
                ultima = percento;
                emetti_brano(app, desiderato, Some(frazione), "scarico", None);
            }
        },
    );
    let percorso = match percorso {
        Ok(percorso) => percorso,
        Err(guasto) => return chiudi_male(app, desiderato, &guasto),
    };

    // ── i tag di Spotify sopra quelli di YouTube ────────────────────────────
    // Un guasto qui **non** annulla lo scaricamento: il file c'è ed è ascoltabile,
    // e rimetterlo in coda vorrebbe dire riscaricarlo per riscrivere un campo.
    // Si annota e si prosegue.
    let copertina = copertina(desiderato);
    if let Err(guasto) = scrivi_tag(&percorso, &desiderato.brano, numero, copertina.as_deref()) {
        eprintln!(
            "[scarico] tag non scritti su {}: {guasto}",
            percorso.display()
        );
    }

    segna(app, desiderato, |connection| {
        desiderati::segna_fatto(
            connection,
            desiderato.id,
            &percorso.display().to_string(),
            &url,
        )
    });
    scarico.fatti.fetch_add(1, Ordering::Relaxed);
    emetti_brano(app, desiderato, Some(1.0), "fatto", None);
    annuncia(app);
}

/// Chiude una riga andata male, distinguendo i due modi.
fn chiudi_male(app: &AppHandle, desiderato: &Desiderato, guasto: &AppError) {
    // L'annullamento non è un guasto e non consuma tentativi: chi ha premuto
    // «Annulla» si aspetta di ritrovare la coda dov'era, non accorciata di tre
    // tentativi per ogni brano che stava scendendo.
    if matches!(guasto.code(), ErrorCode::InternalAborted { .. }) {
        return;
    }

    let dettaglio = codice_e_causa(guasto);
    let ritentabile = guasto.is_retryable();
    segna(app, desiderato, |connection| {
        desiderati::segna_fallito(connection, desiderato.id, &dettaglio, ritentabile)
    });
    emetti_brano(app, desiderato, None, "fallito", Some(guasto));
    annuncia(app);
}

/// Prende il lucchetto per il tempo di una `UPDATE`.
fn segna(
    app: &AppHandle,
    desiderato: &Desiderato,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<(), AppError>,
) {
    let esito = con_libreria(&app.state::<Stato>(), |libreria| {
        azione(&libreria.connection)
    });
    if let Err(guasto) = esito {
        // Non si può fare altro che dirlo: se il database non risponde, il
        // brano resterà in `attesa` e verrà ritentato al prossimo giro — che è
        // il comportamento meno sbagliato fra quelli disponibili.
        eprintln!(
            "[scarico] esito non registrato per «{}»: {guasto}",
            desiderato.brano.title
        );
    }
}

/// Porta dentro la copertina di Spotify, se c'è.
///
/// Fuori da ogni lucchetto, come tutto il resto della rete. `None` per qualunque
/// intoppo: un brano senza copertina è un brano, e far fallire uno scaricamento
/// riuscito perché una miniatura non si scarica sarebbe assurdo.
fn copertina(desiderato: &Desiderato) -> Option<Vec<u8>> {
    let url = desiderato.brano.cover_url.as_deref()?;
    // Trenta secondi: una copertina è piccola, ma un CDN può impuntarsi, e la
    // coda non deve fermarsi per un'immagine.
    let rete = Rete::nuova("copertine", std::time::Duration::from_secs(30));
    aether_spotify::copertina::scarica_byte(&rete, url)
}

// ── gli eventi ──────────────────────────────────────────────────────────────

/// Manda lo stato di un brano alla finestra.
fn emetti_brano(
    app: &AppHandle,
    desiderato: &Desiderato,
    frazione: Option<f32>,
    esito: &'static str,
    guasto: Option<&AppError>,
) {
    let _ = app.emit(
        "scarico:brano",
        EventoBrano {
            titolo: desiderato.brano.title.clone(),
            artista: desiderato.brano.artist.clone(),
            frazione,
            esito,
            codice: guasto.map(|g| g.code().kind().code().to_owned()),
            sorgente_id: desiderato.sorgente_id.clone(),
            provenienza: desiderato.provenienza.clone(),
        },
    );
}

/// Manda l'avanzamento complessivo e quello di ogni importazione.
///
/// Pubblica perché la chiama anche [`avvia`] quando il posto della coda è già
/// preso: è il solo modo perché un'importazione confermata a coda in corsa
/// compaia subito nell'elenco invece che alla fine del brano in corso.
pub fn annuncia(app: &AppHandle) {
    let scarico = app.state::<StatoScarico>();
    let (rimasti, sorgenti) = situazione(app);
    scarico.rimasti.store(rimasti, Ordering::Relaxed);
    let fatti = scarico.fatti.load(Ordering::Relaxed);
    let _ = app.emit(
        "scarico:avanzamento",
        Avanzamento {
            fatti,
            totale: fatti.saturating_add(rimasti),
            sorgenti,
        },
    );
}

/// Fa sapere alla finestra che la coda non può partire.
fn segnala_guasto(app: &AppHandle, guasto: &AppError) {
    eprintln!("[scarico] la coda non parte: {guasto}");
    let _ = app.emit("scarico:guasto", crate::errore::errore(guasto.clone()));
}

/// Quanti ne restano in tabella.
fn totale_da_fare(app: &AppHandle) -> u32 {
    con_libreria(&app.state::<Stato>(), |libreria| {
        desiderati::conta_in_attesa(&libreria.connection)
    })
    .unwrap_or(0)
}

/// Quanti ne restano e come stanno le importazioni, con un lucchetto solo.
///
/// Due prese separate lascerebbero passare in mezzo un brano concluso da un
/// altro filo, e i due numeri racconterebbero due momenti diversi.
fn situazione(app: &AppHandle) -> (u32, Vec<desiderati::Sorgente>) {
    con_libreria(&app.state::<Stato>(), |libreria| {
        Ok((
            desiderati::conta_in_attesa(&libreria.connection)?,
            desiderati::per_sorgente(&libreria.connection)?,
        ))
    })
    .unwrap_or_default()
}

/// Il codice del catalogo più la causa, per la colonna `download_error`.
///
/// Il codice e non la sola frase: è quello che la finestra sa tradurre, e senza
/// di lui un errore salvato oggi diventerebbe testo opaco al prossimo cambio di
/// lingua.
fn codice_e_causa(guasto: &AppError) -> String {
    match guasto.cause() {
        Some(causa) => format!("{}: {causa}", guasto.code().kind().code()),
        None => guasto.code().kind().code().to_owned(),
    }
}

// ── le cartelle ─────────────────────────────────────────────────────────────

/// Dove finiscono i brani scaricati.
///
/// L'impostazione se c'è, altrimenti la **prima cartella sorvegliata**. `None`
/// quando non ce n'è nessuna: scaricare in una cartella non sorvegliata
/// produrrebbe file che nessuna scansione trova mai, cioè uno scaricamento
/// riuscito che l'utente non vede comparire — indistinguibile, per lui, da uno
/// fallito. Meglio non partire e dirlo.
fn cartella_download(app: &AppHandle) -> Option<PathBuf> {
    con_libreria(&app.state::<Stato>(), |libreria| {
        let scelta: Option<String> =
            settings::read(&libreria.connection, settings::CHIAVE_CARTELLA_DOWNLOAD)?;
        if let Some(scelta) = scelta.filter(|s| !s.trim().is_empty()) {
            return Ok(Some(PathBuf::from(scelta)));
        }
        let cartelle: Vec<String> =
            settings::read_json(&libreria.connection, settings::CHIAVE_CARTELLE)?
                .unwrap_or_default();
        Ok(cartelle.into_iter().next().map(PathBuf::from))
    })
    .ok()
    .flatten()
}

/// Dove tengono i `.part` e i frammenti.
///
/// Nella cartella dati e **non** dentro quella sorvegliata: un file a metà che
/// la scansione ingoiasse entrerebbe in libreria come un brano normale che non
/// si sente. È la stessa ragione per cui `MIN_TRACK_BYTES` esiste, presa
/// dall'altro capo.
fn cartella_temporanea(app: &AppHandle) -> PathBuf {
    let dati = con_libreria(&app.state::<Stato>(), |libreria| {
        Ok(libreria.data_dir.clone())
    })
    .unwrap_or_else(|_| std::env::temp_dir());
    dati.join("scarico")
}

/// Una scansione delle cartelle sorvegliate, così i file entrano in libreria.
///
/// E subito dopo il **viaggio di ritorno**: i brani appena entrati vanno rimessi
/// nelle playlist da cui mancavano. Qui e non altrove perché è l'unico momento in
/// cui esistono tutti e due i capi — la riga di `spotify_wanted` che dice «questa
/// playlist, questo posto» e la riga di `tracks` che il file ha appena creato.
/// Senza questo passo la coda finisce, i file ci sono, e la playlist è ancora
/// quella con i soli brani che c'erano già.
fn rientra_in_libreria(app: &AppHandle) {
    let esito = con_libreria(&app.state::<Stato>(), |libreria| {
        let roots: Vec<String> =
            settings::read_json(&libreria.connection, settings::CHIAVE_CARTELLE)?
                .unwrap_or_default();
        let scan = aether_app::library::Scan {
            files: &aether_app::files::LocalFiles,
            covers: &libreria.covers,
            roots: &roots,
            rules: PathRules::for_current_platform(),
        };
        scan.run(&mut libreria.connection, |_, _| {
            std::ops::ControlFlow::Continue(())
        })
    });
    match esito {
        Ok(report) => {
            let _ = app.emit("scarico:in_libreria", report.inserted);
        }
        Err(guasto) => eprintln!("[scarico] la scansione finale è fallita: {guasto}"),
    }

    // `None`: la scansione ha guardato tutte le cartelle, quindi possono essere
    // arrivati brani di qualunque importazione — anche di una vecchia, se
    // l'utente ha messo i file a mano nel frattempo. Un guasto qui si annota e
    // basta: i file sono al loro posto, e la passata successiva riprova.
    let ritorno = con_libreria(&app.state::<Stato>(), |libreria| {
        aether_app::desiderati::riconcilia(&libreria.connection, None)
    });
    match ritorno {
        Ok(fatto) if fatto.voci_rimesse > 0 || fatto.righe_chiuse > 0 => {
            let _ = app.emit("scarico:riconciliato", fatto);
        }
        Ok(_) => {}
        Err(guasto) => eprintln!("[scarico] il ritorno nelle playlist è fallito: {guasto}"),
    }
    // Brani nuovi sono statistiche nuove da ancorare, per la stessa ragione
    // scritta in `comandi::scansiona`.
    let _ = crate::nuvola::se_riuscito(app, Ok::<(), ()>(()));
    // E sono anche i brani che hanno più bisogno di essere arricchiti: quel che
    // scende da YouTube entra con titolo «… (Official Video)», interprete
    // «… - Topic» e nessuna copertina.
    crate::arricchimento::sporca(app);
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn una_coda_alla_volta() {
        // Due code insieme leggerebbero lo stesso lotto e scriverebbero lo
        // stesso file da due processi yt-dlp.
        let stato = StatoScarico::nuovo(PathBuf::from("C:/bin"));
        assert!(stato.prendi_posto());
        assert!(!stato.prendi_posto(), "la seconda non deve entrare");
        stato.lascia_posto();
        assert!(stato.prendi_posto(), "finita la prima, si riparte");
    }

    #[test]
    fn lannullamento_si_alza_e_si_legge() {
        let stato = StatoScarico::nuovo(PathBuf::from("C:/bin"));
        assert!(!stato.fermata());
        stato.ferma();
        assert!(stato.fermata());
    }

    #[test]
    fn il_codice_derrore_sopravvive_alla_scrittura() {
        // È quel che la finestra traduce: senza il codice, `download_error`
        // conterrebbe una frase in italiano che resta in italiano per sempre.
        let guasto = AppError::new(ErrorCode::DownloadUnavailable).with_cause("Video unavailable");
        assert_eq!(
            codice_e_causa(&guasto),
            "download.unavailable: Video unavailable"
        );
        assert_eq!(
            codice_e_causa(&AppError::new(ErrorCode::DownloadNoResults)),
            "download.noResults"
        );
    }
}
