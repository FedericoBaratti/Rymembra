//! I comandi dell'importazione di un account Spotify intero.
//!
//! Involucri, come tutto il resto di questo crate: qui non si decide niente. La
//! lettura dell'archivio sta in `aether-archivio`, l'abbinamento in
//! `aether-domain`, la scrittura in `aether-app`.
//!
//! # Una via sola, e perché è quella giusta
//!
//! C'erano due strade: l'archivio GDPR e la Web API di Spotify col consenso
//! OAuth. È rimasta la prima.
//!
//! Non per un guasto: la Web API funzionava. È che il *Spotify Developer
//! Policy* (III.5, III.9) limita cosa si può fare dei dati che restituisce, e
//! una libreria musicale locale che tiene per anni le playlist di qualcuno non
//! sta dentro quei limiti — mentre l'archivio che Spotify consegna all'utente
//! su richiesta è **dell'utente**, per legge (GDPR art. 20, diritto alla
//! portabilità), e portarselo dove si vuole è precisamente il diritto che
//! quell'articolo riconosce.
//!
//! La differenza pratica per chi usa Aether è una sola: l'archivio va chiesto a
//! Spotify e arriva in qualche giorno. In cambio, contiene la cronologia
//! **completa** invece delle ultime cinquanta righe che l'API concedeva.
//!
//! # Perché lo snapshot resta di qua
//!
//! Stessa ragione scritta in testa a [`crate::importa`], moltiplicata: un
//! account è decine di migliaia di brani più anni di cronologia. Farlo viaggiare
//! fino alla finestra e indietro a ogni passo vorrebbe dire serializzarlo tre
//! volte, e soprattutto fidarsi che quel che torna sia quel che era partito.
//! Resta qui in una cella; la finestra riceve solo i conteggi.
//!
//! # E perché la lettura non tocca mai il lucchetto della libreria
//!
//! `con_libreria` tiene il mutex per tutta la durata della chiamata. Scompattare
//! uno zip da trecento megabyte sono decine di secondi: farlo lì dentro
//! bloccherebbe la riproduzione, la ricerca e la scansione per tutto il tempo.
//! Ogni comando di questo modulo legge **prima**, fuori dal lucchetto, e lo
//! prende solo per scrivere.

use std::sync::Mutex;

use aether_app::import_account::{self, AccountImportReport};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify_account::{AccountSnapshot, Scelte};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, Turno, con_libreria};

/// Quel che è stato letto per ultimo.
struct Caricato {
    snapshot: AccountSnapshot,
    anteprima: Anteprima,
}

/// Lo stato dell'importazione dell'account.
pub struct StatoAccount {
    /// L'ultimo archivio letto.
    caricato: Mutex<Option<Caricato>>,
    /// C'è già un'operazione lunga in corso.
    in_corso: std::sync::atomic::AtomicBool,
}

impl StatoAccount {
    /// Lo stato, vuoto. Non tocca né rete né disco.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            caricato: Mutex::new(None),
            in_corso: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl Default for StatoAccount {
    fn default() -> Self {
        Self::nuovo()
    }
}

/// Cosa si è letto, prima di guardare la libreria.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anteprima {
    /// `archivio`. Resta un campo e non una costante perché
    /// [`aether_domain::spotify_account::Provenienza`] ha ancora il valore
    /// `api`: un database scritto da una versione precedente lo contiene, e
    /// leggerlo non deve diventare un errore.
    pub provenienza: String,
    /// Il nome visualizzato dell'account.
    pub profilo: Option<String>,
    /// L'identificativo dell'utente su Spotify.
    pub spotify_user_id: Option<String>,
    /// Questa via porta una cronologia degna di quel nome?
    pub cronologia_completa: bool,
    /// Quante playlist.
    pub playlist: usize,
    /// Quanti brani in playlist, in tutto.
    pub brani_in_playlist: usize,
    /// Quanti «Brani che ti piacciono».
    pub preferiti: usize,
    /// Quanti album salvati.
    pub album: usize,
    /// Quanti artisti seguiti.
    pub artisti: usize,
    /// Quante righe di cronologia.
    pub cronologia: usize,
    /// Quante voci erano podcast.
    pub podcast: usize,
    /// I file dello zip che si sono letti.
    pub letti: Vec<String>,
    /// Quelli riconosciuti e lasciati stare.
    pub ignorati: Vec<String>,
    /// Quelli che non si sono aperti, col perché.
    pub illeggibili: Vec<FileIlleggibile>,
    /// Quante righe singole non si sono capite.
    pub righe_illeggibili: usize,
    /// Quante voci non erano musica.
    pub non_musica: usize,
}

/// Un file dell'archivio che non si è aperto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIlleggibile {
    /// Quale.
    pub nome: String,
    /// Perché.
    pub perche: String,
}

/// Cosa portarsi dietro.
///
/// Il gemello deserializzabile di [`Scelte`]: il dominio non ha serde fra le
/// dipendenze, e non deve averlo.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScelteIpc {
    /// Creare una playlist per ognuna di quelle su Spotify.
    pub playlist: bool,
    /// Segnare i preferiti.
    pub preferiti: bool,
    /// Registrare gli album salvati.
    pub album: bool,
    /// Registrare gli artisti seguiti.
    pub artisti: bool,
    /// Portare dentro la cronologia.
    pub cronologia: bool,
}

impl From<ScelteIpc> for Scelte {
    fn from(scelte: ScelteIpc) -> Self {
        Self {
            playlist: scelte.playlist,
            preferiti: scelte.preferiti,
            album: scelte.album,
            artisti: scelte.artisti,
            cronologia: scelte.cronologia,
        }
    }
}

/// Che aria tira. Istantaneo: legge solo il database.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoAccountIpc {
    /// L'identificativo dell'ultimo account importato.
    pub spotify_user_id: Option<String>,
    /// Il suo nome.
    pub display_name: Option<String>,
    /// Quando è finita l'ultima importazione, in millisecondi.
    pub ultimo_ms: Option<i64>,
    /// Da quale via. Oggi sempre `archivio`; `api` compare solo nei database
    /// scritti prima che quella strada venisse tolta.
    pub ultima_via: Option<String>,
    /// Quanti ascolti importati ci sono adesso in `play_history`.
    ///
    /// È il numero che rende «dimentica gli ascolti importati» un tasto che dice
    /// quel che sta per cancellare invece di uno che chiede di fidarsi.
    pub ascolti_importati: i64,
    /// C'è già un'operazione lunga in corso.
    pub in_corso: bool,
    /// C'è qualcosa in cella, pronto da importare.
    pub caricato: Option<Anteprima>,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Lo stato dell'account. Istantaneo: non tocca né rete né portachiavi.
#[tauri::command]
pub fn account_stato(
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
) -> Esito<StatoAccountIpc> {
    stato_ipc(&stato, &account).map_err(errore)
}

/// Apre un archivio di Spotify e lo mette in cella.
///
/// `(async)`: scompattare uno zip da trecento megabyte e leggere quarantamila
/// righe di JSON non è un'operazione da filo dell'interfaccia.
#[tauri::command(async)]
pub fn archivio_apri(account: State<'_, StatoAccount>, percorso: String) -> Esito<Anteprima> {
    let esito = (|| {
        let _turno = Turno::prendi(&account.in_corso).ok_or_else(occupato)?;
        let lettura = aether_archivio::leggi(std::path::Path::new(&percorso))?;

        let anteprima = Anteprima {
            letti: lettura.letti,
            ignorati: lettura.ignorati,
            illeggibili: lettura
                .illeggibili
                .into_iter()
                .map(|g| FileIlleggibile {
                    nome: g.nome,
                    perche: g.perche,
                })
                .collect(),
            righe_illeggibili: lettura.righe_illeggibili,
            non_musica: lettura.non_musica.totale(),
            podcast: lettura.podcast,
            ..da_snapshot(&lettura.snapshot)
        };
        metti_in_cella(&account, lettura.snapshot, anteprima.clone());
        Ok(anteprima)
    })();
    esito.map_err(errore)
}

/// Butta via quel che è in cella.
///
/// # Perché è un comando e non succede da solo dopo l'importazione
///
/// Perché importare due volte con scelte diverse è una cosa che si fa: la prima
/// senza la cronologia, poi ci si ripensa. Se la cella si svuotasse da sola
/// dopo `account_importa`, il secondo giro vorrebbe dire riaprire lo zip da
/// capo — decine di secondi per una cosa che è già in memoria.
///
/// Ma in memoria ci resta, e un archivio di dieci anni sono centinaia di
/// megabyte: quando si è finito, questo è il tasto che li restituisce.
#[tauri::command]
pub fn archivio_dimentica(
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
) -> Esito<StatoAccountIpc> {
    svuota_cella(&account);
    stato_ipc(&stato, &account).map_err(errore)
}

/// Cosa porterebbe l'importazione di quel che è in cella, senza scrivere niente.
///
/// `(async)`: il piano è l'importazione vera dentro una transazione abbandonata,
/// e su un archivio di dieci anni sono decine di migliaia di righe.
#[tauri::command(async)]
pub fn account_piano(
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
    scelte: ScelteIpc,
) -> Esito<AccountImportReport> {
    esegui(&stato, &account, scelte, false)
}

/// Importa per davvero.
///
/// # E poi cerca
///
/// I brani che in libreria non ci sono finiscono in `desiderati`, e da lì parte
/// la coda — subito, senza che l'utente debba premere altro. È la stessa scelta
/// di [`crate::importa::import_esegui`]: chi importa il proprio account vuole
/// ascoltarlo, non ottenere un elenco di cose che gli mancano.
///
/// Quel che la coda troverà nei cataloghi liberi è un'altra questione, e la
/// risposta onesta è «una parte». Il resto finisce in «Da comprare», che è la
/// cosa che questa applicazione può fare e che dice la verità.
#[tauri::command(async)]
pub fn account_importa(
    app: AppHandle,
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
    scelte: ScelteIpc,
) -> Esito<AccountImportReport> {
    let esito = crate::nuvola::se_riuscito(&app, esegui(&stato, &account, scelte, true));
    if esito.as_ref().is_ok_and(|rapporto| rapporto.missing() > 0) {
        crate::procura::avvia(&app);
    }
    esito
}

/// Dimentica gli ascolti importati da Spotify.
///
/// L'operazione che `play_history.source` esiste per rendere possibile.
/// Restituisce quante righe se ne sono andate.
#[tauri::command(async)]
pub fn cronologia_dimentica_importati(app: AppHandle, stato: State<'_, Stato>) -> Esito<usize> {
    let esito = con_libreria(&stato, |libreria| {
        import_account::dimentica_importati(&mut libreria.connection)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

// ── il lavoro sotto ─────────────────────────────────────────────────────────

fn esegui(
    stato: &State<'_, Stato>,
    account: &State<'_, StatoAccount>,
    scelte: ScelteIpc,
    conferma: bool,
) -> Esito<AccountImportReport> {
    // Una copia dello snapshot, così la cella non resta presa mentre si scrive
    // nel database: sono due lucchetti diversi, e prenderli in ordine fisso è
    // quel che rende impossibile incrociarli.
    let snapshot = con_cella(account, |caricato| Ok(caricato.snapshot.clone())).map_err(errore)?;
    let scelte = Scelte::from(scelte);

    con_libreria(stato, |libreria| {
        if conferma {
            import_account::import(&mut libreria.connection, &snapshot, &scelte)
        } else {
            import_account::plan(&mut libreria.connection, &snapshot, &scelte)
        }
    })
    .map_err(errore)
}

/// I conteggi che si leggono da uno snapshot, qualunque sia la sua provenienza.
fn da_snapshot(snapshot: &AccountSnapshot) -> Anteprima {
    Anteprima {
        provenienza: snapshot.provenienza.nome().to_owned(),
        profilo: snapshot.profilo.clone(),
        spotify_user_id: snapshot.spotify_user_id.clone(),
        cronologia_completa: snapshot.provenienza.ha_cronologia_completa(),
        playlist: snapshot.playlist.len(),
        brani_in_playlist: snapshot.playlist.iter().map(|p| p.brani.len()).sum(),
        preferiti: snapshot.preferiti.len(),
        album: snapshot.album.len(),
        artisti: snapshot.artisti.len(),
        cronologia: snapshot.cronologia.len(),
        ..Anteprima::default()
    }
}

fn metti_in_cella(account: &StatoAccount, snapshot: AccountSnapshot, anteprima: Anteprima) {
    let mut cella = account
        .caricato
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cella = Some(Caricato {
        snapshot,
        anteprima,
    });
}

fn svuota_cella(account: &StatoAccount) {
    let mut cella = account
        .caricato
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cella = None;
}

/// Fa qualcosa con quel che è in cella.
fn con_cella<T>(
    account: &StatoAccount,
    cosa: impl FnOnce(&Caricato) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let cella = account
        .caricato
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let caricato = cella.as_ref().ok_or_else(|| {
        AppError::new(ErrorCode::SpotifyArchiveUnreadable {
            path: String::new(),
            detail: None,
        })
        .with_message("non c'è nessun archivio aperto da importare".to_owned())
    })?;
    cosa(caricato)
}

/// Un'operazione lunga è già in corso.
fn occupato() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("un'altra operazione sull'account è in corso".to_owned()),
    })
}

fn stato_ipc(stato: &Stato, account: &StatoAccount) -> Result<StatoAccountIpc, AppError> {
    let (riga, ascolti) = con_libreria(stato, |libreria| {
        Ok((
            import_account::registrato(&libreria.connection)?,
            import_account::quanti_importati(&libreria.connection)?,
        ))
    })?;

    let caricato = {
        let cella = account
            .caricato
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cella.as_ref().map(|c| c.anteprima.clone())
    };

    Ok(StatoAccountIpc {
        spotify_user_id: riga.as_ref().map(|r| r.spotify_user_id.clone()),
        display_name: riga.as_ref().and_then(|r| r.display_name.clone()),
        ultimo_ms: riga.as_ref().and_then(|r| r.last_sync_at),
        ultima_via: riga.map(|r| r.last_source),
        ascolti_importati: ascolti,
        in_corso: account.in_corso.load(std::sync::atomic::Ordering::Relaxed),
        caricato,
    })
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::spotify_account::{PlaylistSpotify, Provenienza};

    #[test]
    fn le_scelte_attraversano_il_confine_senza_scambiarsi() {
        // Cinque booleani di fila sono cinque occasioni di invertirne due senza
        // che il compilatore dica niente, e il sintomo sarebbe «ho spento la
        // cronologia e mi ha importato la cronologia».
        let ipc = ScelteIpc {
            playlist: true,
            preferiti: false,
            album: true,
            artisti: false,
            cronologia: true,
        };
        let scelte = Scelte::from(ipc);
        assert!(scelte.playlist);
        assert!(!scelte.preferiti);
        assert!(scelte.album);
        assert!(!scelte.artisti);
        assert!(scelte.cronologia);
    }

    #[test]
    fn l_anteprima_conta_i_brani_di_tutte_le_playlist() {
        let snapshot = AccountSnapshot {
            playlist: vec![
                PlaylistSpotify {
                    nome: "Una".to_owned(),
                    brani: vec![Default::default(); 3],
                    ..PlaylistSpotify::default()
                },
                PlaylistSpotify {
                    nome: "Due".to_owned(),
                    brani: vec![Default::default(); 4],
                    ..PlaylistSpotify::default()
                },
            ],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };
        let anteprima = da_snapshot(&snapshot);
        assert_eq!(anteprima.playlist, 2);
        assert_eq!(anteprima.brani_in_playlist, 7);
        assert_eq!(anteprima.provenienza, "archivio");
        assert!(anteprima.cronologia_completa);
    }

    #[test]
    fn un_archivio_vecchio_scritto_dall_api_si_legge_ancora() {
        // La strada dell'API non c'è più, ma un database che l'ha usata sì. Se
        // `da_snapshot` inciampasse sulla provenienza `api`, chi aggiorna
        // Aether vedrebbe la schermata dell'account rompersi senza spiegazione.
        let dall_api = da_snapshot(&AccountSnapshot::vuoto(Provenienza::Api));
        assert_eq!(dall_api.provenienza, "api");
        assert!(!dall_api.cronologia_completa);
    }

    #[test]
    fn una_cella_vuota_non_e_un_panico() {
        let account = StatoAccount::nuovo();
        let esito = con_cella(&account, |_| Ok(()));
        assert!(esito.is_err(), "non c'è niente da importare");
    }

    #[test]
    fn dimenticare_l_archivio_lo_toglie_davvero_dalla_memoria() {
        // Il tasto esiste per restituire centinaia di megabyte: se svuotasse
        // solo l'anteprima e lasciasse lo snapshot, non servirebbe a niente e
        // nessuno se ne accorgerebbe guardando la finestra.
        let account = StatoAccount::nuovo();
        metti_in_cella(
            &account,
            AccountSnapshot::vuoto(Provenienza::Archivio),
            Anteprima::default(),
        );
        assert!(con_cella(&account, |_| Ok(())).is_ok());
        svuota_cella(&account);
        assert!(con_cella(&account, |_| Ok(())).is_err());
    }

    #[test]
    fn il_turno_impedisce_due_letture_insieme() {
        // Due `archivio_apri` in parallelo sarebbero due letture che si
        // scrivono a vicenda nella cella, e chi importa otterrebbe quella che
        // ha finito per seconda.
        let account = StatoAccount::nuovo();
        let primo = Turno::prendi(&account.in_corso);
        assert!(primo.is_some());
        assert!(Turno::prendi(&account.in_corso).is_none());
        drop(primo);
        assert!(Turno::prendi(&account.in_corso).is_some());
    }
}
