//! I comandi dello scrobbling, e il filo che svuota la coda.
//!
//! Involucri, come tutto il resto di questo crate: i due protocolli stanno in
//! `aether-scrobble`, la coda in `aether_app::scrobble`, la regola di cosa conta
//! in `aether_domain::listen`. Qui c'è chi tiene le credenziali, chi decide
//! quando è il momento di mandare, e chi lo racconta alla finestra.
//!
//! # I lucchetti, nell'ordine che conta
//!
//! Le credenziali si leggono **prima**, fuori dal lucchetto della libreria; la
//! rete non lo vede mai. Una passata di svuotamento è un `con_libreria` per
//! leggere un blocco, poi minuti di HTTP con il lucchetto libero, poi un
//! `con_libreria` per chiudere le righe. Farla in un blocco solo sarebbe più
//! corta da scrivere e terrebbe ferma la riproduzione per tutto il tempo — che
//! è esattamente il difetto che la regola «chi parla col mondo non vede
//! `rusqlite`» rende impossibile scrivere di là e possibilissimo scrivere qui.
//!
//! Il «sta ascoltando adesso» parte da `su_evento`, che tiene in mano il
//! lucchetto del **lettore**: quella richiesta va quindi su un filo suo, o la
//! riproduzione aspetterebbe una risposta da Last.fm prima di far partire il
//! brano successivo.
//!
//! # Perché un blocco rifiutato si divide invece di essere buttato
//!
//! ListenBrainz valuta il documento intero: un solo ascolto con una data
//! impossibile fa rispondere `400` a tutti e mille. Segnare l'intero blocco come
//! definitivamente fallito butterebbe novecentonovantanove ascolti buoni per
//! colpa di uno — in silenzio, perché dal punto di vista della coda sarebbe
//! andato tutto secondo le regole. [`manda_bisecando`] dimezza finché non resta
//! il colpevole, e paga qualche richiesta in più solo nel caso in cui c'è
//! davvero qualcosa da salvare.
//!
//! # Cosa non finisce mai in un log
//!
//! Il token di ListenBrainz, il segreto e la chiave di sessione di Last.fm, il
//! token in attesa del consenso. Le cause degli errori sì: sono i codici che i
//! servizi restituiscono, e servono a capire cosa è successo.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::spegnimento::Emette as _;
use aether_app::scrobble::{self as coda, Voce};
use aether_app::settings;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::listen::Listen;
use aether_domain::scrobble::{Ascolto, Servizio};
use aether_oauth::portachiavi::{DiSistema, Portachiavi};
use aether_scrobble::{LastFm, ListenBrainz, per_richiesta};
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, Turno, con_libreria};

/// Il token di ListenBrainz, nel portachiavi.
const CHIAVE_LB_TOKEN: &str = "listenbrainz.token";

/// Il segreto dell'applicazione Last.fm, nel portachiavi.
const CHIAVE_LFM_SEGRETO: &str = "lastfm.segreto";

/// La chiave dell'applicazione Last.fm, nel portachiavi.
///
/// # Perché ci si è spostata, e perché il vecchio commento era sbagliato
///
/// Fino alla 2.3.0 stava in `settings`, con scritto accanto che «non è un
/// segreto»: viaggia in chiaro nell'indirizzo del consenso, quindi chiunque
/// guardi la barra del browser la vede. La frase è vera e la conclusione era
/// sbagliata, perché la domanda giusta non è se sia segreta ma **di chi sia**.
///
/// È una credenziale **personale**: la registra una persona a suo nome, e chi
/// la ottiene manda scrobble come lei finché non gliela si revoca a mano.
/// Finché stava soltanto nel database di questa macchina era una questione fra
/// l'utente e il proprio disco; dalla 2.3.1 il profilo esporta la libreria e si
/// mette su una chiavetta che si passa in giro, e una credenziale personale
/// dentro un file che si passa in giro è la definizione del guasto.
///
/// Il travaso lo fa [`travasa_chiave_lastfm`] una volta sola all'avvio. Il
/// profilo la lascia comunque fuori — `aether_app::profilo::CATALOGO` non la
/// contiene, e una prova lo tiene vero — quindi anche una macchina su cui il
/// portachiavi non risponde non la esporta.
const CHIAVE_LFM_CHIAVE: &str = "lastfm.api_key";

/// La chiave di sessione di Last.fm, nel portachiavi. Non scade mai.
const CHIAVE_LFM_SESSIONE: &str = "lastfm.sessione";

/// Ogni quanto il filo si sveglia da solo, se nessuno lo chiama.
///
/// Serve a una cosa sola: riprovare quel che è fallito perché non c'era rete.
/// Cinque minuti è la scala di quel guasto — un tunnel, un riavvio del router —
/// e sotto quella soglia si spenderebbero richieste per niente.
const RIPOSO: Duration = Duration::from_secs(300);

/// Lo stato dello scrobbling.
pub struct StatoScrobble {
    /// Il token Last.fm che aspetta il consenso dell'utente.
    ///
    /// In memoria e mai su disco: vale un'ora, si usa una volta sola, e da solo
    /// non fa niente. Scriverlo moltiplicherebbe i posti in cui una credenziale
    /// resta dopo aver smesso di servire — e il codice che dice «non c'è nessuna
    /// richiesta in attesa» esiste apposta per il caso in cui l'applicazione è
    /// stata chiusa in mezzo.
    attesa_lastfm: Mutex<Option<String>>,
    /// Il filo deve fare un giro.
    sveglia: (Mutex<bool>, Condvar),
    /// Un giro è in corso.
    in_corso: AtomicBool,
}

impl StatoScrobble {
    /// Lo stato, vuoto. Non tocca né rete né disco.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            attesa_lastfm: Mutex::new(None),
            sveglia: (Mutex::new(false), Condvar::new()),
            in_corso: AtomicBool::new(false),
        }
    }
}

impl Default for StatoScrobble {
    fn default() -> Self {
        Self::nuovo()
    }
}

// ── quel che la finestra riceve ─────────────────────────────────────────────

/// Com'è messo un servizio.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collegamento {
    /// Ha le credenziali per parlare col servizio (per Last.fm: chiave e
    /// segreto). Non vuol dire che l'utente abbia dato il consenso.
    pub configurato: bool,
    /// C'è una sessione o un token utente: si può mandare.
    pub collegato: bool,
    /// Come si chiama l'utente lassù.
    pub utente: Option<String>,
    /// Quanti ascolti aspettano di partire verso questo servizio.
    pub in_attesa: i64,
    /// Quanti hanno finito i tentativi.
    pub abbandonati: i64,
}

/// Lo stato completo.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoScrobbleIpc {
    /// Mandare quel che si ascolta è acceso.
    pub attivo: bool,
    /// ListenBrainz.
    pub listenbrainz: Collegamento,
    /// Last.fm.
    pub lastfm: Collegamento,
    /// C'è un consenso Last.fm cominciato e non finito.
    pub attesa_lastfm: bool,
    /// C'è una passata in corso.
    pub in_corso: bool,
}

/// Com'è andata una passata.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoInvio {
    /// Quanti ascolti sono usciti.
    pub mandati: u32,
    /// Quanti il servizio ha ricevuto e scartato.
    pub ignorati: u32,
    /// Perché li ha scartati, un motivo per riga.
    pub motivi: Vec<String>,
    /// Quanti restano in coda.
    pub in_attesa: i64,
    /// Quanti hanno finito i tentativi.
    pub abbandonati: i64,
    /// Il codice del guasto che ha fermato la passata, quando ce n'è stato uno.
    pub guasto: Option<String>,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Lo stato dello scrobbling. Istantaneo: non tocca la rete.
#[tauri::command]
pub fn scrobble_stato(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
) -> Esito<StatoScrobbleIpc> {
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Accende o spegne lo scrobbling, senza toccare le credenziali.
///
/// Spento, la coda **non si riempie più** e quel che c'è dentro resta fermo:
/// mettere in pausa lo scrobbling e poi trovarsi tre giorni di ascolti spediti
/// tutti insieme alla riaccensione sarebbe una sorpresa, non una comodità.
#[tauri::command]
pub fn scrobble_attivo(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
    attivo: bool,
) -> Esito<StatoScrobbleIpc> {
    con_libreria(&stato, |libreria| {
        settings::write(
            &libreria.connection,
            coda::CHIAVE_ATTIVO,
            if attivo { "1" } else { "0" },
        )
    })
    .map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Collega ListenBrainz con un token utente.
///
/// Il token si **verifica prima di salvarlo**: uno sbagliato incollato qui non
/// darebbe nessun sintomo finché il primo ascolto non fallisce, cioè ore dopo e
/// in un posto diverso da dove è stato commesso l'errore.
///
/// `(async)` perché va in rete: vedi la nota in `account.rs` sul perché un
/// comando bloccante non deve girare sul filo della finestra.
#[tauri::command(async)]
pub fn scrobble_listenbrainz_collega(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
    token: String,
) -> Esito<StatoScrobbleIpc> {
    let esito = (|| {
        let token = token.trim().to_owned();
        if token.is_empty() {
            return Err(AppError::new(ErrorCode::SettingsListenbrainzNotConfigured));
        }
        let proprietario = ListenBrainz::nuovo(&token).valida()?;
        portachiavi().scrivi(CHIAVE_LB_TOKEN, &token)?;
        con_libreria(&stato, |libreria| {
            settings::write(
                &libreria.connection,
                coda::CHIAVE_LB_UTENTE,
                &proprietario.utente,
            )
        })
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Scollega ListenBrainz.
///
/// Toglie il token e butta la sua coda. La coda va buttata e non tenuta: chi
/// ricollega fra un mese non vuole veder partire gli ascolti di oggi con la data
/// di oggi — che sul servizio comparirebbero come ascolti di un mese fa mai
/// avvenuti, ma che nel frattempo l'utente ha già ascoltato altrove.
#[tauri::command]
pub fn scrobble_listenbrainz_scollega(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
) -> Esito<StatoScrobbleIpc> {
    let esito = (|| {
        portachiavi().cancella(CHIAVE_LB_TOKEN)?;
        con_libreria(&stato, |libreria| {
            settings::forget(&libreria.connection, coda::CHIAVE_LB_UTENTE)?;
            coda::dimentica(&libreria.connection, Some(Servizio::ListenBrainz), false)
        })
        .map(|_| ())
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Scrive chiave e segreto dell'applicazione Last.fm.
///
/// Vuoti li cancellano, e scollegano: una sessione ottenuta con un'altra chiave
/// continuerebbe a funzionare, e nascondere che le credenziali nuove non vanno è
/// il modo di far cercare il guasto nel posto sbagliato. È la stessa regola di
/// `account_credenziali`.
#[tauri::command]
pub fn scrobble_lastfm_credenziali(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
    api_key: String,
    segreto: String,
) -> Esito<StatoScrobbleIpc> {
    let esito = (|| {
        let api_key = api_key.trim();
        let segreto = segreto.trim();
        // Nel portachiavi, accanto al segreto. E in `settings` si cancella
        // comunque, anche quando la chiave nuova è vuota: è la riga che chiude
        // il travaso per chi arriva qui prima che l'avvio l'abbia fatto.
        if api_key.is_empty() {
            portachiavi().cancella(CHIAVE_LFM_CHIAVE)?;
        } else {
            portachiavi().scrivi(CHIAVE_LFM_CHIAVE, api_key)?;
        }
        con_libreria(&stato, |libreria| {
            settings::forget(&libreria.connection, coda::CHIAVE_LFM_API_KEY)?;
            Ok(())
        })?;
        if segreto.is_empty() {
            portachiavi().cancella(CHIAVE_LFM_SEGRETO)?;
        } else {
            portachiavi().scrivi(CHIAVE_LFM_SEGRETO, segreto)?;
        }
        scollega_lastfm(&stato, &scrobble)
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Primo tempo del consenso: chiede un token e apre il browser.
///
/// Il token resta in memoria finché l'utente non torna a dire che ha approvato.
/// Non c'è nessun modo di accorgersene da qui: Last.fm non richiama nessuno.
#[tauri::command(async)]
pub fn scrobble_lastfm_collega(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
) -> Esito<StatoScrobbleIpc> {
    let esito = (|| {
        let client = client_lastfm(&stato)?;
        let token = client.chiedi_token()?;
        let url = client.url_consenso(&token);
        tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|err| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura del browser".to_owned()),
            })
            .with_cause(err.to_string())
        })?;
        if let Ok(mut attesa) = scrobble.attesa_lastfm.lock() {
            *attesa = Some(token);
        }
        Ok(())
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Secondo tempo: scambia il token approvato per una sessione.
#[tauri::command(async)]
pub fn scrobble_lastfm_completa(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
) -> Esito<StatoScrobbleIpc> {
    let esito = (|| {
        let token = scrobble
            .attesa_lastfm
            .lock()
            .ok()
            .and_then(|attesa| attesa.clone())
            .ok_or_else(|| AppError::new(ErrorCode::SettingsLastfmNoPendingToken))?;

        let sessione = client_lastfm(&stato)?.sessione(&token)?;
        portachiavi().scrivi(CHIAVE_LFM_SESSIONE, &sessione.chiave)?;
        con_libreria(&stato, |libreria| {
            settings::write(
                &libreria.connection,
                coda::CHIAVE_LFM_UTENTE,
                &sessione.utente,
            )
        })?;
        // Il token è servito: usarlo due volte dà un errore che non spiega
        // niente, e tenerlo in giro non serve a nessuno.
        if let Ok(mut attesa) = scrobble.attesa_lastfm.lock() {
            *attesa = None;
        }
        Ok(())
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Scollega Last.fm.
#[tauri::command]
pub fn scrobble_lastfm_scollega(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
) -> Esito<StatoScrobbleIpc> {
    scollega_lastfm(&stato, &scrobble).map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Svuota la coda adesso, senza aspettare il filo.
#[tauri::command(async)]
pub fn scrobble_invia(app: AppHandle) -> Esito<EsitoInvio> {
    let scrobble = app.state::<StatoScrobble>();
    let _turno = Turno::prendi(&scrobble.in_corso).ok_or_else(|| {
        errore(
            AppError::new(ErrorCode::SyncBusy)
                .with_cause("c'è già una passata di scrobbling in corso"),
        )
    })?;
    Ok(passata(&app))
}

/// Rimette in fila gli ascolti abbandonati.
#[tauri::command]
pub fn scrobble_riprova(stato: State<'_, Stato>, app: AppHandle) -> Esito<StatoScrobbleIpc> {
    con_libreria(&stato, |libreria| {
        coda::rimetti_in_fila(&libreria.connection, None)
    })
    .map_err(errore)?;
    sveglia(&app);
    stato_ipc(&stato, &app.state::<StatoScrobble>()).map_err(errore)
}

/// Butta gli ascolti abbandonati.
#[tauri::command]
pub fn scrobble_dimentica(
    stato: State<'_, Stato>,
    scrobble: State<'_, StatoScrobble>,
) -> Esito<StatoScrobbleIpc> {
    con_libreria(&stato, |libreria| {
        coda::dimentica(&libreria.connection, None, true)
    })
    .map_err(errore)?;
    stato_ipc(&stato, &scrobble).map_err(errore)
}

/// Accoda tutta la cronologia già in casa verso ListenBrainz.
///
/// È il gesto che chiude il cerchio dell'importazione da Spotify: gli anni di
/// ascolti arrivati nell'archivio diventano la propria cronologia su un servizio
/// che non appartiene a nessuna piattaforma.
///
/// **Solo ListenBrainz**, e non è una scelta di gusto: Last.fm rifiuta gli
/// ascolti con una data vecchia e ha un tetto giornaliero, quindi là quelle
/// righe non entrerebbero — comparirebbero come «ignorate» a decine di
/// migliaia.
#[tauri::command(async)]
pub fn scrobble_importa_cronologia(
    stato: State<'_, Stato>,
    app: AppHandle,
    solo_importati: bool,
) -> Esito<i64> {
    let sorgente = if solo_importati {
        Some("spotify")
    } else {
        None
    };
    let accodati = con_libreria(&stato, |libreria| {
        coda::accoda_cronologia(&libreria.connection, Servizio::ListenBrainz, sorgente, 0)
    })
    .map_err(errore)?;
    sveglia(&app);
    Ok(i64::try_from(accodati).unwrap_or(i64::MAX))
}

// ── i ganci della riproduzione ──────────────────────────────────────────────

/// Un ascolto è finito e conta: mettilo in coda.
///
/// La chiama `riproduzione::chiudi_ascolto` dopo `record_play`, e solo quando
/// quello ha scritto davvero: la coda deve contenere le stesse righe della
/// cronologia, non un insieme suo.
pub fn dopo_un_ascolto(app: &AppHandle, ascolto: &Listen) {
    let stato = app.state::<Stato>();
    let servizi = match collegati(&stato) {
        Ok(servizi) if !servizi.is_empty() => servizi,
        _ => return,
    };

    let esito = con_libreria(&stato, |libreria| {
        let Some(da_mandare) =
            coda::ascolto_di(&libreria.connection, ascolto.track_id, ascolto.started_at)?
        else {
            return Ok(0);
        };
        coda::accoda(&libreria.connection, &servizi, &da_mandare)
    });
    match esito {
        Ok(0) => {}
        Ok(_) => sveglia(app),
        Err(err) => lamenta("ascolto non accodato", &err),
    }
}

/// Dichiara ai servizi cosa sta suonando adesso.
///
/// Su un filo suo: la chiama `su_evento`, che tiene in mano il lucchetto del
/// lettore, e una richiesta HTTP lì dentro fermerebbe il brano successivo.
///
/// Non entra mai in coda. Un «sta ascoltando» rimasto indietro perché non c'era
/// rete descriverebbe un brano finito venti minuti fa: si manda subito o non si
/// manda.
pub fn sta_suonando(app: &AppHandle, track_id: i64) {
    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-scrobble-adesso".to_owned())
        .spawn(move || {
            let stato = manico.state::<Stato>();
            let Ok(servizi) = collegati(&stato) else {
                return;
            };
            if servizi.is_empty() {
                return;
            }
            let letto = con_libreria(&stato, |libreria| {
                coda::ascolto_di(&libreria.connection, track_id, 0)
            });
            let Ok(Some(ascolto)) = letto else {
                return;
            };
            if !ascolto.valido() {
                return;
            }

            for servizio in servizi {
                // Un guasto qui non si ritenta e non si registra da nessuna
                // parte: è un'informazione che vale finché dura il brano, e fra
                // tre minuti sarà comunque vecchia.
                let esito = match servizio {
                    Servizio::ListenBrainz => listenbrainz(&stato).and_then(|c| c.adesso(&ascolto)),
                    Servizio::LastFm => lastfm(&stato)
                        .and_then(|(client, sessione)| client.adesso(&sessione, &ascolto)),
                };
                if let Err(err) = esito {
                    lamenta(&format!("«sta suonando» verso {}", servizio.nome()), &err);
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[scrobble] il filo del «sta suonando» non è partito: {err}");
    }
}

// ── il filo ─────────────────────────────────────────────────────────────────

/// Avvia il filo che svuota la coda. Uno solo, per tutta la vita del processo.
pub fn avvia(app: &AppHandle) {
    // Prima di tutto il resto, e una volta sola: la chiave Last.fm passa da
    // `settings` al portachiavi. Sta qui e non in `main` perché è materia di
    // questo modulo — è lui a sapere dove la chiave sta e chi la legge — e
    // perché è già il posto in cui lo scrobbling si sveglia all'apertura.
    //
    // Un guasto **non ferma niente**: la chiave resta dov'è, Last.fm continua
    // a funzionare, e l'unica conseguenza è una riga di diario. Vale la regola
    // di `travasa_chiave_lastfm`: non si perde una credenziale per proteggerla.
    if let Some(stato) = app.try_state::<Stato>() {
        let esito = con_libreria(&stato, |libreria| {
            travasa_chiave_lastfm(&portachiavi(), &libreria.connection)
        });
        match esito {
            Ok(true) => nota!("[scrobble] la chiave Last.fm è passata nel portachiavi"),
            Ok(false) => {}
            Err(err) => lamenta("la chiave Last.fm non è passata nel portachiavi", &err),
        }
    }

    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-scrobble".to_owned())
        .spawn(move || {
            loop {
                {
                    let scrobble = manico.state::<StatoScrobble>();
                    let Some(_turno) = Turno::prendi(&scrobble.in_corso) else {
                        // Una passata a mano è già in corso: questo giro non
                        // serve, e la coda che resta la prende lei.
                        aspetta(&manico);
                        continue;
                    };
                    let esito = passata(&manico);
                    if esito.mandati > 0 || esito.guasto.is_some() {
                        manico.emetti("scrobble:passata", &esito);
                    }
                }
                aspetta(&manico);
            }
        });
    if let Err(err) = avviato {
        nota!("[scrobble] il filo della coda non è partito: {err}");
        return;
    }
    // Un giro subito: alla chiusura precedente può essere rimasto qualcosa, e
    // aspettare cinque minuti per accorgersene sarebbe cinque minuti di
    // «perché non è arrivato niente».
    sveglia(app);
}

/// Dorme finché qualcuno non chiama, o al massimo [`RIPOSO`].
fn aspetta(app: &AppHandle) {
    let scrobble = app.state::<StatoScrobble>();
    let (serrata, condizione) = &scrobble.sveglia;
    let Ok(guardia) = serrata.lock() else {
        // Il mutex è avvelenato: c'è poco da fare se non non girare a vuoto.
        std::thread::sleep(RIPOSO);
        return;
    };
    let esito = condizione.wait_timeout_while(guardia, RIPOSO, |chiamato| !*chiamato);
    if let Ok((mut chiamato, _)) = esito {
        *chiamato = false;
    }
}

/// Sveglia il filo: c'è qualcosa da mandare.
pub fn sveglia(app: &AppHandle) {
    let scrobble = app.state::<StatoScrobble>();
    let (serrata, condizione) = &scrobble.sveglia;
    if let Ok(mut chiamato) = serrata.lock() {
        *chiamato = true;
        condizione.notify_all();
    }
}

/// Una passata completa: per ogni servizio collegato, finché c'è coda.
fn passata(app: &AppHandle) -> EsitoInvio {
    let stato = app.state::<Stato>();
    let mut esito = EsitoInvio::default();

    if !acceso(&stato) {
        aggiorna_conteggi(&stato, &mut esito);
        return esito;
    }

    for servizio in Servizio::tutti() {
        let mandatore = match mandatore(&stato, servizio) {
            Ok(Some(mandatore)) => mandatore,
            Ok(None) => continue,
            Err(err) => {
                lamenta(&format!("credenziali di {}", servizio.nome()), &err);
                esito.guasto = Some(err.code().kind().code().to_owned());
                continue;
            }
        };

        loop {
            let lotto = con_libreria(&stato, |libreria| {
                coda::da_mandare(&libreria.connection, servizio, per_richiesta(servizio))
            });
            let voci = match lotto {
                Ok(voci) if voci.is_empty() => break,
                Ok(voci) => voci,
                Err(err) => {
                    lamenta("lettura della coda", &err);
                    esito.guasto = Some(err.code().kind().code().to_owned());
                    break;
                }
            };

            let (riuscite, fallite, fermato) = manda_bisecando(&mandatore, &voci, &mut esito);
            chiudi(&stato, &riuscite, &fallite);
            if fermato.is_some() {
                esito.guasto = fermato;
                break;
            }
        }
    }

    aggiorna_conteggi(&stato, &mut esito);
    esito
}

/// Chi manda, già costruito con le sue credenziali.
enum Mandatore {
    /// ListenBrainz, con il suo token dentro.
    ListenBrainz(Box<ListenBrainz>),
    /// Last.fm, con la chiave di sessione accanto.
    LastFm(Box<LastFm>, String),
}

impl Mandatore {
    /// Chi è.
    const fn servizio(&self) -> Servizio {
        match self {
            Self::ListenBrainz(_) => Servizio::ListenBrainz,
            Self::LastFm(_, _) => Servizio::LastFm,
        }
    }

    /// Manda un blocco, e dice quanti ne ha scartati il servizio.
    fn invia(&self, ascolti: &[Ascolto]) -> Result<(u32, Vec<String>), AppError> {
        match self {
            Self::ListenBrainz(client) => {
                client.invia(ascolti)?;
                // ListenBrainz non scarta: o prende il documento intero o lo
                // rifiuta. Non c'è una terza risposta da leggere.
                Ok((0, Vec::new()))
            }
            Self::LastFm(client, sessione) => {
                let esito = client.invia(sessione, ascolti)?;
                Ok((esito.ignorati, esito.motivi))
            }
        }
    }
}

/// Manda un blocco; se viene rifiutato in blocco, lo dimezza per trovare chi.
///
/// Restituisce (riuscite, fallite definitivamente, guasto che ferma la passata).
/// Un guasto **ritentabile** — non c'è rete, il servizio è giù — ferma tutto e
/// non consuma i tentativi delle righe che non sono nemmeno partite; uno non
/// ritentabile che riguarda un solo ascolto lo isola e lascia passare gli altri.
fn manda_bisecando(
    mandatore: &Mandatore,
    voci: &[Voce],
    esito: &mut EsitoInvio,
) -> (Vec<i64>, Vec<(i64, String)>, Option<String>) {
    let mut riuscite = Vec::new();
    let mut fallite = Vec::new();
    let mut fermato = None;
    bisezione(
        mandatore,
        voci,
        esito,
        &mut riuscite,
        &mut fallite,
        &mut fermato,
    );
    (riuscite, fallite, fermato)
}

/// Il passo ricorsivo di [`manda_bisecando`].
fn bisezione(
    mandatore: &Mandatore,
    voci: &[Voce],
    esito: &mut EsitoInvio,
    riuscite: &mut Vec<i64>,
    fallite: &mut Vec<(i64, String)>,
    fermato: &mut Option<String>,
) {
    if voci.is_empty() || fermato.is_some() {
        return;
    }
    let ascolti: Vec<Ascolto> = voci.iter().map(|v| v.ascolto.clone()).collect();

    match mandatore.invia(&ascolti) {
        Ok((ignorati, motivi)) => {
            let quanti = u32::try_from(voci.len()).unwrap_or(u32::MAX);
            esito.mandati = esito
                .mandati
                .saturating_add(quanti.saturating_sub(ignorati));
            esito.ignorati = esito.ignorati.saturating_add(ignorati);
            for motivo in motivi {
                if !esito.motivi.contains(&motivo) {
                    esito.motivi.push(motivo);
                }
            }
            riuscite.extend(voci.iter().map(|v| v.id));
        }
        Err(err) if err.is_retryable() => {
            // Non è colpa di questi ascolti. Si ferma tutto: consumare un
            // tentativo a mille righe perché il servizio è in manutenzione
            // vorrebbe dire abbandonarle dopo dieci manutenzioni.
            lamenta(
                &format!("invio verso {}", mandatore.servizio().nome()),
                &err,
            );
            *fermato = Some(err.code().kind().code().to_owned());
        }
        Err(err) => {
            let causa = format!(
                "{}: {}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            match voci.split_at_checked(meta(voci.len())) {
                // Uno solo, ed è stato rifiutato: il colpevole è lui.
                Some((prima, dopo)) if prima.is_empty() || dopo.is_empty() => {
                    fallite.extend(voci.iter().map(|v| (v.id, causa.clone())));
                }
                Some((prima, dopo)) => {
                    // Il rifiuto riguarda il documento, non tutti gli ascolti
                    // che contiene. Si divide finché non resta chi.
                    bisezione(mandatore, prima, esito, riuscite, fallite, fermato);
                    bisezione(mandatore, dopo, esito, riuscite, fallite, fermato);
                }
                None => fallite.extend(voci.iter().map(|v| (v.id, causa.clone()))),
            }
        }
    }
}

/// Metà di un numero di voci, per la bisezione.
#[expect(
    clippy::integer_division,
    reason = "dimezzare un blocco: il resto finisce nella metà di destra, ed è quel che si vuole"
)]
const fn meta(quante: usize) -> usize {
    quante / 2
}

/// Chiude le righe: via quelle arrivate, contate quelle rifiutate.
///
/// I fallimenti si raggruppano per causa invece di chiudersi uno per uno. La
/// bisezione produce la **stessa** causa per tutte le voci di un ramo — è la
/// stessa stringa clonata — quindi un lotto rifiutato in blocco è un gruppo solo,
/// e una chiamata sola: [`coda::fallite`] apre una transazione per invocazione, e
/// una per riga vorrebbe dire tornare ai commit contati che questa correzione
/// toglie di mezzo.
fn chiudi(stato: &State<'_, Stato>, riuscite: &[i64], fallite: &[(i64, String)]) {
    let mut per_causa: std::collections::HashMap<&str, Vec<i64>> = std::collections::HashMap::new();
    for (id, causa) in fallite {
        per_causa.entry(causa.as_str()).or_default().push(*id);
    }
    let esito = con_libreria(stato, |libreria| {
        coda::fatte(&libreria.connection, riuscite)?;
        for (causa, id) in &per_causa {
            coda::fallite(&libreria.connection, id, causa, true)?;
        }
        Ok(())
    });
    if let Err(err) = esito {
        lamenta("chiusura della coda", &err);
    }
}

/// Rilegge i conteggi dentro l'esito.
fn aggiorna_conteggi(stato: &State<'_, Stato>, esito: &mut EsitoInvio) {
    if let Ok(conteggi) = con_libreria(stato, |libreria| coda::conteggi(&libreria.connection, None))
    {
        esito.in_attesa = conteggi.in_attesa;
        esito.abbandonati = conteggi.abbandonati;
    }
}

// ── le credenziali ──────────────────────────────────────────────────────────

/// Il portachiavi di sistema.
const fn portachiavi() -> DiSistema {
    DiSistema
}

/// Porta la chiave Last.fm da `settings` al portachiavi, una volta sola.
///
/// Restituisce `true` se ha spostato qualcosa. Una chiave che non c'è, o che è
/// già nel portachiavi, dà `false` e non è un guasto: è la condizione normale
/// di ogni avvio dopo il primo.
///
/// # Se il portachiavi non risponde, non si declassa
///
/// La chiave **resta dov'è** e l'errore torna al chiamante, che lo annota. Non
/// si cancella da `settings` prima di aver visto la scrittura riuscire, o un
/// portachiavi momentaneamente irraggiungibile all'avvio scollegherebbe
/// Last.fm senza dire niente: l'utente si ritroverebbe da rifare il consenso e
/// nessuno collegherebbe la cosa a un travaso.
///
/// È la stessa regola scritta in testa a `aether_oauth::portachiavi` — un
/// declassamento silenzioso di una proprietà di sicurezza è peggio di una
/// funzione che dice di non poter partire — applicata nel verso in cui qui
/// serve: non si perde la credenziale per proteggerla.
///
/// # Errori
///
/// `settings.secretUnavailable` se il portachiavi non risponde;
/// `db.queryFailed` se `settings` non si legge o non si scrive.
pub fn travasa_chiave_lastfm(
    portachiavi: &dyn Portachiavi,
    connection: &rusqlite::Connection,
) -> Result<bool, AppError> {
    let Some(chiave) = settings::read(connection, coda::CHIAVE_LFM_API_KEY)?
        .map(|chiave| chiave.trim().to_owned())
        .filter(|chiave| !chiave.is_empty())
    else {
        // Niente in `settings`: o non è mai stata scritta, o il travaso è già
        // avvenuto. In tutti e due i casi non c'è niente da fare, e la riga
        // costa una lettura sulla chiave primaria.
        return Ok(false);
    };
    portachiavi.scrivi(CHIAVE_LFM_CHIAVE, &chiave)?;
    // E solo adesso, con la copia buona già al sicuro.
    settings::forget(connection, coda::CHIAVE_LFM_API_KEY)?;
    Ok(true)
}

/// La chiave dell'applicazione Last.fm: prima il portachiavi, poi `settings`.
///
/// Il ripiego su `settings` non è un declassamento — non si *scrive* mai là —
/// ma la lettura della coda del travaso: su una macchina il cui portachiavi non
/// ha risposto all'avvio la chiave è ancora nel database, e rifiutarsi di
/// leggerla vorrebbe dire scollegare Last.fm a chi non ha fatto niente.
fn chiave_lastfm(stato: &State<'_, Stato>) -> Result<Option<String>, AppError> {
    if let Some(chiave) = leggi_segreto(CHIAVE_LFM_CHIAVE)
        .unwrap_or_default()
        .filter(|chiave| !chiave.trim().is_empty())
    {
        return Ok(Some(chiave));
    }
    con_libreria(stato, |libreria| {
        settings::read(&libreria.connection, coda::CHIAVE_LFM_API_KEY)
    })
    .map(|chiave| chiave.filter(|chiave| !chiave.trim().is_empty()))
}

/// Lo scrobbling è acceso? Acceso di serie: chi collega un servizio vuole che
/// mandi, e un interruttore che nasce spento sembra un guasto.
fn acceso(stato: &State<'_, Stato>) -> bool {
    con_libreria(stato, |libreria| {
        settings::read(&libreria.connection, coda::CHIAVE_ATTIVO)
    })
    .ok()
    .flatten()
    .is_none_or(|valore| valore != "0")
}

/// Quali servizi hanno di che mandare, adesso.
fn collegati(stato: &State<'_, Stato>) -> Result<Vec<Servizio>, AppError> {
    if !acceso(stato) {
        return Ok(Vec::new());
    }
    let mut servizi = Vec::new();
    if !leggi_segreto(CHIAVE_LB_TOKEN)?
        .unwrap_or_default()
        .is_empty()
    {
        servizi.push(Servizio::ListenBrainz);
    }
    if !leggi_segreto(CHIAVE_LFM_SESSIONE)?
        .unwrap_or_default()
        .is_empty()
    {
        servizi.push(Servizio::LastFm);
    }
    Ok(servizi)
}

/// Un segreto dal portachiavi, senza far cadere niente se il portachiavi non
/// c'è.
///
/// Un portachiavi che non risponde **non** è «non collegato»: lo si dice, e non
/// si prova a mandare. Ripiegare su `settings` sarebbe il declassamento
/// silenzioso che `aether_oauth::portachiavi` vieta in testa al modulo.
fn leggi_segreto(chiave: &str) -> Result<Option<String>, AppError> {
    portachiavi().leggi(chiave)
}

/// Il client di ListenBrainz, se c'è un token.
fn listenbrainz(stato: &State<'_, Stato>) -> Result<ListenBrainz, AppError> {
    let _ = stato;
    let token = leggi_segreto(CHIAVE_LB_TOKEN)?
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::SettingsListenbrainzNotConfigured))?;
    Ok(ListenBrainz::nuovo(token))
}

/// Il client di Last.fm, senza la sessione: serve al consenso, che la sessione
/// non ce l'ha ancora.
fn client_lastfm(stato: &State<'_, Stato>) -> Result<LastFm, AppError> {
    let api_key = chiave_lastfm(stato)?
        .ok_or_else(|| AppError::new(ErrorCode::SettingsLastfmNotConfigured))?;
    let segreto = leggi_segreto(CHIAVE_LFM_SEGRETO)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::SettingsLastfmNotConfigured))?;
    Ok(LastFm::nuovo(api_key, segreto))
}

/// Il client di Last.fm con la sua sessione.
fn lastfm(stato: &State<'_, Stato>) -> Result<(LastFm, String), AppError> {
    let client = client_lastfm(stato)?;
    let sessione = leggi_segreto(CHIAVE_LFM_SESSIONE)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::new(ErrorCode::SettingsScrobbleAuthRejected {
                service: Servizio::LastFm.chiave().to_owned(),
            })
            .with_cause("nessuna sessione: il consenso non è mai stato completato")
        })?;
    Ok((client, sessione))
}

/// Chi manda per questo servizio, o `None` se non è collegato.
fn mandatore(stato: &State<'_, Stato>, servizio: Servizio) -> Result<Option<Mandatore>, AppError> {
    let collegati = collegati(stato)?;
    if !collegati.contains(&servizio) {
        return Ok(None);
    }
    match servizio {
        Servizio::ListenBrainz => listenbrainz(stato)
            .map(|c| Some(Mandatore::ListenBrainz(Box::new(c))))
            // Le credenziali sono sparite fra il controllo e la costruzione:
            // non è un guasto da raccontare, è un servizio non collegato.
            .or(Ok(None)),
        Servizio::LastFm => lastfm(stato)
            .map(|(c, s)| Some(Mandatore::LastFm(Box::new(c), s)))
            .or(Ok(None)),
    }
}

/// Scollega Last.fm: sessione, nome, coda e token in attesa.
fn scollega_lastfm(
    stato: &State<'_, Stato>,
    scrobble: &State<'_, StatoScrobble>,
) -> Result<(), AppError> {
    portachiavi().cancella(CHIAVE_LFM_SESSIONE)?;
    if let Ok(mut attesa) = scrobble.attesa_lastfm.lock() {
        *attesa = None;
    }
    con_libreria(stato, |libreria| {
        settings::forget(&libreria.connection, coda::CHIAVE_LFM_UTENTE)?;
        coda::dimentica(&libreria.connection, Some(Servizio::LastFm), false)
    })
    .map(|_| ())
}

/// Lo stato completo, senza toccare la rete.
fn stato_ipc(
    stato: &State<'_, Stato>,
    scrobble: &State<'_, StatoScrobble>,
) -> Result<StatoScrobbleIpc, AppError> {
    let (utente_lb, utente_lfm, attivo, conta_lb, conta_lfm) = con_libreria(stato, |libreria| {
        let c = &libreria.connection;
        Ok((
            settings::read(c, coda::CHIAVE_LB_UTENTE)?,
            settings::read(c, coda::CHIAVE_LFM_UTENTE)?,
            settings::read(c, coda::CHIAVE_ATTIVO)?,
            coda::conteggi(c, Some(Servizio::ListenBrainz))?,
            coda::conteggi(c, Some(Servizio::LastFm))?,
        ))
    })?;
    // La chiave sta nel portachiavi dalla 2.3.1, con il ripiego su `settings`
    // per chi non ha ancora travasato: `chiave_lastfm` conosce tutti e due i
    // posti, e qui non se ne deve sapere nessuno.
    let api_key = chiave_lastfm(stato).unwrap_or_default();

    // Un portachiavi che non risponde vale «non collegato» **solo qui**, dove
    // si sta disegnando una schermata: far fallire la lettura dello stato
    // renderebbe la sezione delle impostazioni inapribile. Chi prova a mandare
    // invece l'errore lo riceve intero.
    let token_lb = leggi_segreto(CHIAVE_LB_TOKEN).unwrap_or_default();
    let sessione_lfm = leggi_segreto(CHIAVE_LFM_SESSIONE).unwrap_or_default();
    let segreto_lfm = leggi_segreto(CHIAVE_LFM_SEGRETO).unwrap_or_default();

    Ok(StatoScrobbleIpc {
        attivo: attivo.is_none_or(|valore| valore != "0"),
        listenbrainz: Collegamento {
            configurato: true,
            collegato: token_lb.is_some_and(|t| !t.is_empty()),
            utente: utente_lb,
            in_attesa: conta_lb.in_attesa,
            abbandonati: conta_lb.abbandonati,
        },
        lastfm: Collegamento {
            configurato: api_key.is_some_and(|k| !k.trim().is_empty())
                && segreto_lfm.is_some_and(|s| !s.is_empty()),
            collegato: sessione_lfm.is_some_and(|s| !s.is_empty()),
            utente: utente_lfm,
            in_attesa: conta_lfm.in_attesa,
            abbandonati: conta_lfm.abbandonati,
        },
        attesa_lastfm: scrobble
            .attesa_lastfm
            .lock()
            .is_ok_and(|attesa| attesa.is_some()),
        in_corso: scrobble.in_corso.load(Ordering::Relaxed),
    })
}

/// Un guasto che non ferma niente, ma che non va perso.
fn lamenta(cosa: &str, err: &AppError) {
    nota!(
        "[scrobble] {cosa} codice={} causa={}",
        err.code().kind().code(),
        err.cause().unwrap_or("—")
    );
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_oauth::portachiavi::{Guasto, InMemoria};

    /// Una libreria vuota in memoria.
    fn libreria() -> rusqlite::Connection {
        aether_app::db::open_in_memory()
            .expect("database")
            .connection
    }

    #[test]
    fn la_chiave_lastfm_migra_nel_portachiavi_e_sparisce_da_settings() {
        let c = libreria();
        settings::write(&c, coda::CHIAVE_LFM_API_KEY, "0123456789abcdef").expect("scrittura");

        let anello = InMemoria::nuovo();
        assert_eq!(travasa_chiave_lastfm(&anello, &c), Ok(true));
        assert_eq!(
            anello.leggi(CHIAVE_LFM_CHIAVE),
            Ok(Some("0123456789abcdef".to_owned())),
            "la chiave è nel portachiavi"
        );
        assert_eq!(
            settings::read(&c, coda::CHIAVE_LFM_API_KEY),
            Ok(None),
            "e non è più nel database, che è il file che un profilo porterebbe in giro"
        );

        // Un secondo avvio non ha niente da fare, e non deve cancellare quel
        // che ha appena messo al sicuro.
        assert_eq!(travasa_chiave_lastfm(&anello, &c), Ok(false));
        assert_eq!(
            anello.leggi(CHIAVE_LFM_CHIAVE),
            Ok(Some("0123456789abcdef".to_owned()))
        );
    }

    #[test]
    fn se_il_portachiavi_non_risponde_la_chiave_resta_dov_e() {
        // La strada che conta: non si perde una credenziale per proteggerla.
        // Senza questa prova, un portachiavi momentaneamente irraggiungibile
        // all'avvio scollegherebbe Last.fm in silenzio, e nessuno collegherebbe
        // la cosa al travaso.
        let c = libreria();
        settings::write(&c, coda::CHIAVE_LFM_API_KEY, "0123456789abcdef").expect("scrittura");

        let err = travasa_chiave_lastfm(&Guasto, &c).expect_err("il portachiavi non risponde");
        assert_eq!(err.code().kind().code(), "settings.secretUnavailable");
        assert_eq!(
            settings::read(&c, coda::CHIAVE_LFM_API_KEY),
            Ok(Some("0123456789abcdef".to_owned())),
            "la chiave resta dov'è, e il profilo la lascia comunque fuori"
        );
    }

    #[test]
    fn una_libreria_senza_chiave_non_ha_niente_da_travasare() {
        let c = libreria();
        assert_eq!(travasa_chiave_lastfm(&InMemoria::nuovo(), &c), Ok(false));
        // Nemmeno una scritta a spazi: è il residuo di un campo svuotato a
        // mano, e scriverla nel portachiavi vorrebbe dire far credere a
        // `stato_ipc` che Last.fm è configurato.
        settings::write(&c, coda::CHIAVE_LFM_API_KEY, "   ").expect("scrittura");
        assert_eq!(travasa_chiave_lastfm(&InMemoria::nuovo(), &c), Ok(false));
    }
}
