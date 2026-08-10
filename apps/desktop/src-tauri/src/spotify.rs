//! I comandi dell'importazione da Spotify.
//!
//! Involucri, come tutto il resto di questo crate: qui non si decide niente. La
//! lettura sta in `aether-spotify`, l'abbinamento in `aether-domain`, la
//! scrittura in `aether-app`.
//!
//! # Perché il contenuto risolto resta di qua
//!
//! Il flusso è a tre passi — anteprima, piano, conferma — e tutti e tre parlano
//! dello stesso contenuto. Farlo viaggiare fino alla finestra e indietro
//! vorrebbe dire serializzare trecento brani due volte, e soprattutto vorrebbe
//! dire fidarsi che quel che torna sia quel che era partito. Resta qui, in una
//! cella, indicizzato dall'URI: la finestra manda sempre e solo il link.
//!
//! # E perché la rete non tocca mai il lucchetto della libreria
//!
//! `con_libreria` tiene il mutex per tutta la durata della chiamata. Risolvere
//! una playlist di trecento brani sono tre richieste HTTP: farle lì dentro
//! bloccherebbe la riproduzione, la ricerca e la scansione per tutto il tempo.
//! Ogni comando di questo modulo risolve **prima**, fuori dal lucchetto, e lo
//! prende solo per scrivere.

use std::path::PathBuf;
use std::sync::Mutex;

use aether_app::import_spotify::{self, SpotifyImportReport};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify::SpotifyContent;
use aether_spotify::config::{EsitoFile, NOME_FILE};
use aether_spotify::{Lettore, riconosci};
use serde::Serialize;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Il contenuto letto per ultimo, con l'URI da cui è arrivato.
struct Risolto {
    uri: String,
    contenuto: SpotifyContent,
    /// La copertina già portata dentro, come `data:` URI. Vedi
    /// [`aether_spotify::copertina`] sul perché non è l'indirizzo di Spotify.
    copertina: Option<String>,
    falliti: Vec<LivelloFallito>,
}

/// Lo stato dell'importazione da Spotify.
pub struct StatoSpotify {
    /// Dove sta il file che sovrascrive le costanti volatili.
    percorso_config: PathBuf,
    /// Il lettore, creato alla prima richiesta e poi tenuto: si porta dietro i
    /// gettoni, e importare tre playlist di fila fa una stretta di mano sola.
    lettore: Mutex<Option<Lettore>>,
    /// L'ultimo contenuto risolto.
    ultimo: Mutex<Option<Risolto>>,
}

impl StatoSpotify {
    /// Lo stato, con la cartella dati in cui cercare `spotify.json`.
    #[must_use]
    pub fn nuovo(data_dir: &std::path::Path) -> Self {
        Self {
            percorso_config: data_dir.join(NOME_FILE),
            lettore: Mutex::new(None),
            ultimo: Mutex::new(None),
        }
    }
}

/// Un livello che non ha risposto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivelloFallito {
    /// Quale.
    pub livello: String,
    /// Perché.
    pub perche: String,
}

/// Un elenco arrivato più corto di quanto Spotify dichiari.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Troncatura {
    /// Quanti brani sono arrivati.
    pub letti: u32,
    /// Quanti ne dichiara Spotify.
    pub attesi: u32,
}

/// Cosa si è letto da Spotify, prima di guardare la libreria.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anteprima {
    /// `brano`, `album`, `playlist` o `artista`.
    pub genere: String,
    /// L'identificativo Spotify.
    pub id: String,
    /// Il nome.
    pub titolo: String,
    /// Chi lo firma.
    pub autore: Option<String>,
    /// La copertina come `data:` URI, già portata dentro.
    ///
    /// **Non** l'indirizzo di Spotify: la politica dei contenuti della finestra
    /// non ammette domini esterni fra le immagini, e allargarla per sempre per
    /// una miniatura è esattamente quel che il resto dell'applicazione si
    /// rifiuta di fare. Vedi [`aether_spotify::copertina`].
    pub copertina: Option<String>,
    /// Quanti brani sono arrivati.
    pub brani: usize,
    /// Da quale livello.
    pub sorgente: String,
    /// Se l'elenco è monco.
    pub troncato: Option<Troncatura>,
    /// I livelli provati prima, che non hanno risposto.
    pub falliti: Vec<LivelloFallito>,
}

/// Che aria tira.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostica {
    /// `null` se la stretta di mano riesce, altrimenti perché no.
    pub stretta_di_mano: Option<String>,
    /// Quanti cifrari TOTP sono disponibili.
    pub cifrari: usize,
    /// Le loro versioni, in ordine di tentativo.
    pub versioni_cifrari: Vec<u32>,
    /// Dove va messo il file che corregge le costanti scadute.
    pub percorso_config: String,
    /// `assente`, `letto` o `illeggibile`.
    pub file_config: String,
    /// Il dettaglio, quando il file c'è ma non si è capito.
    pub file_config_errore: Option<String>,
}

/// Legge il contenuto, riusando quello già in cella se è lo stesso link.
///
/// `forza` salta il riuso: vedi [`spotify_anteprima`] sul perché una lettura
/// riuscita male deve poter essere rifatta.
///
/// **Non** prende il lucchetto della libreria: vedi la nota in testa al modulo.
fn risolvi(spotify: &State<'_, StatoSpotify>, url: &str, forza: bool) -> Result<(), AppError> {
    // Il riconoscimento a secco per primo, fuori da ogni lucchetto: copre tutte
    // le forme di link che si leggono da sole, cioè tutte tranne una.
    if let Some(riferimento) = riconosci(url) {
        let uri = riferimento.uri();
        let Ok(cella) = spotify.ultimo.lock() else {
            return Err(avvelenato());
        };
        if !forza && cella.as_ref().is_some_and(|r| r.uri == uri) {
            return Ok(());
        }
    }

    let Ok(mut custodia) = spotify.lettore.lock() else {
        return Err(avvelenato());
    };
    let lettore = custodia.get_or_insert_with(|| Lettore::dal_file(&spotify.percorso_config));
    // Qui dentro perché un link corto del telefono va chiesto alla rete per
    // sapere che cosa nomina, e la rete di questo crate è quella del lettore.
    let riferimento = lettore.riferimento(url).ok_or_else(|| {
        AppError::new(ErrorCode::DownloadUnrecognizedUrl)
            .with_message("non è un link di Spotify".to_owned())
    })?;
    let uri = riferimento.uri();

    // Il riuso si controlla di nuovo: un link corto porta a un contenuto che può
    // benissimo essere quello già in cella, e senza questo secondo controllo
    // ognuno dei tre passi del flusso lo rileggerebbe da capo.
    {
        let Ok(cella) = spotify.ultimo.lock() else {
            return Err(avvelenato());
        };
        if !forza && cella.as_ref().is_some_and(|r| r.uri == uri) {
            return Ok(());
        }
    }

    let risultato = lettore.risolvi(&riferimento)?;
    // Qui e non alla prima anteprima: siamo già dentro il lucchetto del lettore
    // e già in rete, e scaricarla adesso vuol dire che i tre passi del flusso
    // costano una richiesta in più in tutto, non una a testa.
    let copertina = risultato
        .contenuto
        .cover_url
        .as_deref()
        .and_then(|indirizzo| lettore.copertina(indirizzo));

    let Ok(mut cella) = spotify.ultimo.lock() else {
        return Err(avvelenato());
    };
    *cella = Some(Risolto {
        uri,
        copertina,
        falliti: risultato
            .falliti
            .iter()
            .map(|f| LivelloFallito {
                livello: f.livello.nome().to_owned(),
                perche: f.perche.clone(),
            })
            .collect(),
        contenuto: risultato.contenuto,
    });
    Ok(())
}

/// Fa qualcosa con il contenuto in cella.
fn con_contenuto<T>(
    spotify: &State<'_, StatoSpotify>,
    cosa: impl FnOnce(&Risolto) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let Ok(cella) = spotify.ultimo.lock() else {
        return Err(avvelenato());
    };
    let risolto = cella
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::SpotifyResolveFailed))?;
    cosa(risolto)
}

/// Un mutex avvelenato da un panico altrove.
fn avvelenato() -> AppError {
    AppError::new(ErrorCode::InternalUnexpected {
        detail: Some("lo stato di Spotify non è più leggibile".to_owned()),
    })
}

/// Legge un link e dice cosa c'è dentro, senza toccare la libreria.
///
/// # Perché esiste `forza`
///
/// Perché il contenuto letto resta in una cella indicizzata dall'URI, e senza
/// una via per svuotarla una lettura andata male sarebbe **definitiva**: se il
/// primo tentativo è caduto al terzo livello — solo titolo e copertina, perché
/// in quel momento la rete singhiozzava — lo stesso link darebbe quella stessa
/// risposta magra per tutto il tempo in cui l'applicazione resta aperta, e
/// «Riprova» non riproverebbe niente.
#[tauri::command]
pub fn spotify_anteprima(
    spotify: State<'_, StatoSpotify>,
    url: String,
    forza: bool,
) -> Esito<Anteprima> {
    risolvi(&spotify, &url, forza).map_err(errore)?;
    con_contenuto(&spotify, |risolto| {
        let c = &risolto.contenuto;
        Ok(Anteprima {
            genere: c.kind.nome().to_owned(),
            id: c.id.clone(),
            titolo: c.title.clone(),
            autore: c.author.clone(),
            copertina: risolto.copertina.clone(),
            brani: c.tracks.len(),
            sorgente: c.source.nome().to_owned(),
            troncato: c
                .truncation()
                .map(|(letti, attesi)| Troncatura { letti, attesi }),
            falliti: risolto.falliti.clone(),
        })
    })
    .map_err(errore)
}

/// Cosa porterebbe l'importazione, senza scrivere niente.
#[tauri::command]
pub fn spotify_piano(
    stato: State<'_, Stato>,
    spotify: State<'_, StatoSpotify>,
    url: String,
    crea_playlist: bool,
) -> Esito<SpotifyImportReport> {
    esegui(&stato, &spotify, &url, crea_playlist, false)
}

/// Importa per davvero.
///
/// # E poi scarica
///
/// I brani che in libreria non ci sono finiscono in `spotify_wanted`, e da lì
/// parte la coda — subito, senza che l'utente debba premere altro. È l'«in modo
/// automatico»: chi importa una playlist vuole ascoltarla, non ottenere un
/// elenco di cose che gli mancano.
///
/// Non è bloccante: [`crate::scarica::avvia`] mette in piedi un filo e torna, e
/// questo comando risponde con il rapporto dell'importazione mentre il primo
/// brano sta già scendendo. Se una coda sta già girando non ne parte una
/// seconda: quella in corso rilegge la tabella a ogni lotto, quindi i brani
/// appena aggiunti li prende comunque.
#[tauri::command]
pub fn spotify_importa(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    spotify: State<'_, StatoSpotify>,
    url: String,
    crea_playlist: bool,
) -> Esito<SpotifyImportReport> {
    let esito =
        crate::nuvola::se_riuscito(&app, esegui(&stato, &spotify, &url, crea_playlist, true));
    if esito.as_ref().is_ok_and(|rapporto| rapporto.missing > 0) {
        crate::scarica::avvia(&app);
    }
    esito
}

fn esegui(
    stato: &State<'_, Stato>,
    spotify: &State<'_, StatoSpotify>,
    url: &str,
    crea_playlist: bool,
    conferma: bool,
) -> Esito<SpotifyImportReport> {
    // Prima la rete, fuori da ogni lucchetto… e senza `forza`: il piano e la
    // conferma parlano del contenuto che l'utente ha appena visto, e rileggerlo
    // vorrebbe dire poter importare qualcosa di diverso da quel che mostrava
    // l'anteprima.
    risolvi(spotify, url, false).map_err(errore)?;
    // …poi una copia del contenuto, così la cella non resta presa mentre si
    // scrive nel database: sono due lucchetti diversi, e prenderli in ordine
    // fisso è quel che rende impossibile incrociarli.
    let contenuto =
        con_contenuto(spotify, |risolto| Ok(risolto.contenuto.clone())).map_err(errore)?;

    con_libreria(stato, |libreria| {
        if conferma {
            import_spotify::import(&mut libreria.connection, &contenuto, crea_playlist)
        } else {
            import_spotify::plan(&mut libreria.connection, &contenuto, crea_playlist)
        }
    })
    .map_err(errore)
}

/// Prova la stretta di mano e riporta lo stato della configurazione.
///
/// Esiste perché questo sottosistema dipende da punti interni che Spotify
/// cambia: senza, un guasto si presenta come «non funziona» e non c'è modo di
/// distinguere una rotazione dei cifrari da un computer offline.
#[tauri::command]
pub fn spotify_diagnostica(spotify: State<'_, StatoSpotify>) -> Esito<Diagnostica> {
    let Ok(mut custodia) = spotify.lettore.lock() else {
        return Err(errore(avvelenato()));
    };
    let lettore = custodia.get_or_insert_with(|| Lettore::dal_file(&spotify.percorso_config));
    let d = lettore.diagnostica();
    let (file_config, file_config_errore) = match &d.file_configurazione {
        EsitoFile::Assente => ("assente", None),
        EsitoFile::Letto => ("letto", None),
        EsitoFile::Illeggibile(perche) => ("illeggibile", Some(perche.clone())),
    };
    Ok(Diagnostica {
        stretta_di_mano: d.stretta_di_mano,
        cifrari: d.cifrari,
        versioni_cifrari: d.versioni_cifrari,
        percorso_config: spotify.percorso_config.display().to_string(),
        file_config: file_config.to_owned(),
        file_config_errore,
    })
}
