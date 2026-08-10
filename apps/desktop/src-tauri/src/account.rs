//! I comandi dell'importazione di un account Spotify intero.
//!
//! Involucri, come tutto il resto di questo crate: qui non si decide niente. Il
//! consenso e la lettura stanno in `aether-spotify`, la lettura dell'archivio in
//! `aether-archivio`, l'abbinamento in `aether-domain`, la scrittura in
//! `aether-app`.
//!
//! # Due vie, un solo insieme di comandi
//!
//! `account_collega` + `account_leggi` riempiono la cella dalla Web API;
//! `archivio_apri` la riempie da uno zip. Da lì in poi `account_piano` e
//! `account_importa` sono **gli stessi due comandi** per tutte e due le vie,
//! perché dentro la cella c'è lo stesso `AccountSnapshot`. È il perno deciso in
//! `aether_domain::spotify_account`, e questo modulo è il posto in cui si vede a
//! occhio nudo: la finestra ha una schermata sola.
//!
//! # Perché lo snapshot resta di qua
//!
//! Stessa ragione scritta in testa a [`crate::spotify`], moltiplicata: un
//! account è decine di migliaia di brani più anni di cronologia. Farlo viaggiare
//! fino alla finestra e indietro a ogni passo vorrebbe dire serializzarlo tre
//! volte, e soprattutto fidarsi che quel che torna sia quel che era partito.
//! Resta qui in una cella; la finestra riceve solo i conteggi.
//!
//! # E perché la rete non tocca mai il lucchetto della libreria
//!
//! `con_libreria` tiene il mutex per tutta la durata della chiamata. Leggere un
//! account da duecento playlist sono duecento richieste HTTP, cioè minuti:
//! farle lì dentro bloccherebbe la riproduzione, la ricerca e la scansione per
//! tutto il tempo. Ogni comando di questo modulo legge **prima**, fuori dal
//! lucchetto, e lo prende solo per scrivere.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use aether_app::import_account::{self, AccountImportReport};
use aether_app::settings;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify_account::{AccountSnapshot, Scelte};
use aether_net::Rete;
use aether_oauth::ancora_valido;
use aether_oauth::portachiavi::{DiSistema, Portachiavi as _};
use aether_spotify::account::portachiavi::SPOTIFY_REFRESH_TOKEN;
use aether_spotify::account::{self, Token, oauth};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, Turno, con_libreria};

/// L'identificativo del client, scritto dall'utente.
///
/// In `settings` e non nel portachiavi: con PKCE non è un segreto, e la nota in
/// testa a `aether_spotify::account::oauth` argomenta perché. Metterlo fra i
/// segreti costerebbe una finestra di sblocco del portachiavi per leggere una
/// cosa che sta scritta in chiaro nell'indirizzo del consenso.
const CHIAVE_CLIENT_ID: &str = "spotify.client_id";

/// Quel che è stato letto per ultimo, da una delle due vie.
struct Caricato {
    snapshot: AccountSnapshot,
    anteprima: Anteprima,
}

/// Lo stato dell'importazione dell'account.
pub struct StatoAccount {
    /// L'ultimo account letto, da qualunque via.
    caricato: Mutex<Option<Caricato>>,
    /// L'access token in corso, finché dura.
    ///
    /// In memoria e mai su disco: dura un'ora e si riottiene dal refresh token.
    /// Scriverlo moltiplicherebbe i posti in cui un segreto resta dopo aver
    /// smesso di servire.
    token: Mutex<Option<Token>>,
    /// C'è già un'operazione lunga in corso.
    in_corso: AtomicBool,
}

impl StatoAccount {
    /// Lo stato, vuoto. Non tocca né rete né disco.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            caricato: Mutex::new(None),
            token: Mutex::new(None),
            in_corso: AtomicBool::new(false),
        }
    }
}

impl Default for StatoAccount {
    fn default() -> Self {
        Self::nuovo()
    }
}

/// Cosa si è letto, prima di guardare la libreria.
///
/// Una sola forma per tutte e due le vie, con i campi che riguardano solo
/// l'archivio lasciati vuoti quando si viene dall'API. L'alternativa —
/// due tipi e due schermate — costerebbe alla finestra di sapere da dove viene
/// quel che sta mostrando, che è precisamente la cosa che il perno del dominio
/// esiste per rendere irrilevante.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anteprima {
    /// `api` o `archivio`.
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

    // ── solo dalla Web API ──
    /// L'account ha Premium? `None` quando Spotify non l'ha detto.
    pub premium: Option<bool>,
    /// Le playlist di cui Spotify non dà più il contenuto.
    pub senza_contenuto: Vec<String>,
    /// Gli elenchi arrivati a metà.
    pub troncati: Vec<String>,

    // ── solo dall'archivio ──
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

/// Che aria tira, senza toccare la rete.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoAccountIpc {
    /// C'è un identificativo di applicazione configurato.
    pub configurato: bool,
    /// L'identificativo, per rimetterlo nella casella.
    pub client_id: Option<String>,
    /// C'è un token di aggiornamento nel portachiavi.
    pub collegato: bool,
    /// L'identificativo dell'ultimo account importato.
    pub spotify_user_id: Option<String>,
    /// Il suo nome.
    pub display_name: Option<String>,
    /// Quando è finita l'ultima importazione, in millisecondi.
    pub ultimo_ms: Option<i64>,
    /// Da quale via: `api` o `archivio`.
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

/// Lo stato dell'account. Istantaneo: non tocca la rete.
#[tauri::command]
pub fn account_stato(
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
) -> Esito<StatoAccountIpc> {
    stato_ipc(&stato, &account).map_err(errore)
}

/// Scrive l'identificativo dell'applicazione Spotify.
///
/// Vuoto lo cancella, e scollega: un token emesso per un altro client
/// continuerebbe a funzionare fino alla scadenza, e nascondere per un'ora il
/// fatto che le credenziali nuove non vanno è il modo di far cercare il guasto
/// nel posto sbagliato.
#[tauri::command]
pub fn account_credenziali(
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
    client_id: String,
) -> Esito<StatoAccountIpc> {
    let esito = (|| {
        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_CLIENT_ID, client_id.trim())
        })?;
        dimentica_token(&account);
        Ok(())
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &account).map_err(errore)
}

/// Collega un account Spotify.
///
/// Apre la schermata di consenso nel **browser di sistema** e aspetta la
/// risposta su un socket loopback effimero, per non più di tre minuti.
///
/// # Perché `(async)` su una funzione che non è `async`
///
/// Perché un `#[tauri::command]` normale gira sul filo dell'anello degli eventi
/// della finestra, e questa aspetta che una persona legga una schermata in un
/// browser: fino a tre minuti in cui l'applicazione sarebbe congelata, compreso
/// il tasto che l'utente premerebbe per annullare. `(async)` su una funzione
/// sincrona la manda sulla riserva di fili di Tauri; il corpo resta bloccante e
/// ordinario. Vale per questo comando e per i tre qui sotto che vanno in rete.
#[tauri::command(async)]
pub fn account_collega(
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
) -> Esito<StatoAccountIpc> {
    let esito = (|| {
        let _turno = Turno::prendi(&account.in_corso).ok_or_else(occupato)?;
        let client_id = client_id(&stato)?;
        let rete = Rete::nuova("spotify", oauth::SCADENZA);

        let token = oauth::collega(&rete, &client_id, |url| {
            tauri_plugin_opener::open_url(url, None::<&str>).map_err(|err| {
                AppError::new(ErrorCode::InternalAborted {
                    what: Some("apertura del browser".to_owned()),
                })
                .with_cause(err.to_string())
            })
        })?;

        let Some(refresh) = token.refresh_token.as_deref() else {
            // Spotify lo manda sempre al primo consenso. Senza, il collegamento
            // morirebbe fra un'ora in silenzio: meglio non collegarsi.
            return Err(AppError::new(ErrorCode::SpotifyAccountAuthExpired)
                .with_cause("Spotify non ha mandato un token di aggiornamento"));
        };
        DiSistema.scrivi(SPOTIFY_REFRESH_TOKEN, refresh)?;
        ricorda_token(&account, token);
        Ok(())
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &account).map_err(errore)
}

/// Scollega l'account.
///
/// Pulisce il portachiavi e dimentica chi era. **Non** tocca niente di quel che
/// è già stato importato: le playlist restano, i preferiti restano, la
/// cronologia resta. Scollegarsi è smettere di parlare con Spotify, non
/// disfare quello che si è portato dentro — per quello c'è
/// [`cronologia_dimentica_importati`], che dice cosa cancella.
///
/// Spotify non ha un punto di revoca come Google: si revoca dalla propria
/// pagina delle applicazioni, e non c'è niente da chiedere alla rete.
#[tauri::command]
pub fn account_scollega(
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
) -> Esito<StatoAccountIpc> {
    let esito = (|| {
        DiSistema.cancella(SPOTIFY_REFRESH_TOKEN)?;
        dimentica_token(&account);
        svuota_cella(&account);
        con_libreria(&stato, |libreria| {
            import_account::scollega(&libreria.connection)
        })
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &account).map_err(errore)
}

/// Legge tutto l'account dalla Web API e lo mette in cella.
///
/// Emette `account:avanzamento` a ogni passo. `(async)`: sono minuti di rete.
#[tauri::command(async)]
pub fn account_leggi(
    app: AppHandle,
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
) -> Esito<Anteprima> {
    let esito = (|| {
        let _turno = Turno::prendi(&account.in_corso).ok_or_else(occupato)?;
        let rete = Rete::nuova("spotify", oauth::SCADENZA);
        let token = access_token(&stato, &account, &rete)?;

        let lettura = account::leggi(&rete, &token, |passo| {
            // L'evento si manda e non si controlla: una finestra che si è
            // chiusa a metà lettura non è una ragione per far fallire la
            // lettura. È lo stesso trattamento che riceve `nuvola:stato`.
            drop(app.emit("account:avanzamento", AvanzamentoIpc::da(passo)));
        })?;

        let anteprima = Anteprima {
            premium: lettura.premium,
            senza_contenuto: lettura.senza_contenuto,
            troncati: lettura.troncati.iter().map(|t| (*t).to_owned()).collect(),
            podcast: lettura.podcast,
            ..da_snapshot(&lettura.snapshot)
        };
        metti_in_cella(&account, lettura.snapshot, anteprima.clone());
        Ok(anteprima)
    })();
    esito.map_err(errore)
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
/// # E poi scarica
///
/// I brani che in libreria non ci sono finiscono in `spotify_wanted`, e da lì
/// parte la coda — subito, senza che l'utente debba premere altro. È la stessa
/// scelta di [`crate::spotify::spotify_importa`]: chi importa il proprio account
/// vuole ascoltarlo, non ottenere un elenco di cose che gli mancano.
#[tauri::command(async)]
pub fn account_importa(
    app: AppHandle,
    stato: State<'_, Stato>,
    account: State<'_, StatoAccount>,
    scelte: ScelteIpc,
) -> Esito<AccountImportReport> {
    let esito = crate::nuvola::se_riuscito(&app, esegui(&stato, &account, scelte, true));
    if esito.as_ref().is_ok_and(|rapporto| rapporto.missing() > 0) {
        crate::scarica::avvia(&app);
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

/// A che punto è la lettura, nella forma che la finestra riceve.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AvanzamentoIpc {
    fase: String,
    nome: Option<String>,
    fatti: usize,
    totali: Option<usize>,
}

impl AvanzamentoIpc {
    fn da(passo: &account::Avanzamento) -> Self {
        Self {
            fase: passo.fase.to_owned(),
            nome: passo.nome.clone(),
            fatti: passo.fatti,
            totali: passo.totali,
        }
    }
}

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

/// I conteggi che valgono per tutte e due le vie.
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

/// L'access token da usare adesso, rinfrescandolo se è ora.
///
/// # Il refresh token che ruota, e dove si paga
///
/// Spotify ne restituisce uno nuovo a (quasi) ogni rinfresco, e quello di prima
/// smette di valere. Riscriverlo nel portachiavi **qui**, subito, è l'unico
/// punto in cui si può fare: se lo si dimenticasse, il collegamento continuerebbe
/// a funzionare per l'ora dell'access token in corso e poi morirebbe al riavvio
/// successivo, quando nessuno collega più il guasto a questa funzione.
fn access_token(stato: &Stato, account: &StatoAccount, rete: &Rete) -> Result<String, AppError> {
    {
        let cache = account
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = cache.as_ref()
            && ancora_valido(token.scade_ms)
        {
            return Ok(token.access_token.clone());
        }
    }

    let Some(refresh) = DiSistema.leggi(SPOTIFY_REFRESH_TOKEN)? else {
        return Err(AppError::new(ErrorCode::SpotifyAccountAuthExpired)
            .with_cause("nessun account collegato"));
    };
    let client_id = client_id(stato)?;
    let token = oauth::rinfresca(rete, &client_id, &refresh)?;
    if let Some(nuovo) = token.refresh_token.as_deref() {
        DiSistema.scrivi(SPOTIFY_REFRESH_TOKEN, nuovo)?;
    }
    let access = token.access_token.clone();
    ricorda_token(account, token);
    Ok(access)
}

/// L'identificativo dell'applicazione, o l'errore che dice cosa manca.
fn client_id(stato: &Stato) -> Result<String, AppError> {
    let letto = con_libreria(stato, |libreria| {
        settings::read(&libreria.connection, CHIAVE_CLIENT_ID)
    })?;
    letto
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::SpotifyAccountNotConfigured))
}

fn ricorda_token(account: &StatoAccount, token: Token) {
    let mut cache = account
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cache = Some(token);
}

fn dimentica_token(account: &StatoAccount) {
    let mut cache = account
        .token
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cache = None;
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
        AppError::new(ErrorCode::SpotifyResolveFailed)
            .with_message("non c'è nessun account letto da importare".to_owned())
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
    // Un portachiavi che non risponde vale «non collegato»: è la direzione che
    // non promette niente, e la stessa che prende `nuvola`.
    let collegato = DiSistema
        .leggi(SPOTIFY_REFRESH_TOKEN)
        .ok()
        .flatten()
        .is_some();

    let (client_id, riga, ascolti) = con_libreria(stato, |libreria| {
        let client_id = settings::read(&libreria.connection, CHIAVE_CLIENT_ID)?
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
        Ok((
            client_id,
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
        configurato: client_id.is_some(),
        client_id,
        collegato,
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
    fn la_provenienza_dice_alla_finestra_cosa_aspettarsi() {
        // «3 ascolti importati» dall'API è il limite di un endpoint, non un
        // guasto: la finestra deve poterlo scrivere, e questo è il bit con cui.
        let dall_api = da_snapshot(&AccountSnapshot::vuoto(Provenienza::Api));
        assert_eq!(dall_api.provenienza, "api");
        assert!(!dall_api.cronologia_completa);
    }

    #[test]
    fn la_chiave_del_client_id_non_e_quella_di_google() {
        // Due chiavi che si somigliano in `settings` sono due chiavi che prima o
        // poi qualcuno scambia, e il sintomo sarebbe un collegamento a Drive che
        // smette di funzionare quando si configura Spotify.
        assert_eq!(CHIAVE_CLIENT_ID, "spotify.client_id");
        assert_ne!(CHIAVE_CLIENT_ID, "nuvola.client_id");
    }

    #[test]
    fn una_cella_vuota_non_e_un_panico() {
        let account = StatoAccount::nuovo();
        let esito = con_cella(&account, |_| Ok(()));
        assert!(esito.is_err(), "non c'è niente da importare");
    }

    #[test]
    fn il_turno_impedisce_due_letture_insieme() {
        // Due `account_leggi` in parallelo sarebbero due letture dello stesso
        // account che si scrivono a vicenda nella cella, e chi importa
        // otterrebbe quella che ha finito per seconda.
        let account = StatoAccount::nuovo();
        let primo = Turno::prendi(&account.in_corso);
        assert!(primo.is_some());
        assert!(Turno::prendi(&account.in_corso).is_none());
        drop(primo);
        assert!(Turno::prendi(&account.in_corso).is_some());
    }
}
