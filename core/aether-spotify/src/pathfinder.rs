//! L'API interna del lettore web, che è quella che funziona ancora.
//!
//! # Perché non la Web API pubblica
//!
//! Perché a febbraio 2026 Spotify l'ha smontata. `GET /playlists/{id}/tracks`
//! non esiste più, e il suo sostituto `/playlists/{id}/items` restituisce il
//! contenuto **solo al proprietario autenticato**: per chiunque altro è un 403.
//! Sono spariti anche `external_ids` dall'oggetto brano — cioè l'ISRC — e
//! `GET /artists/{id}/top-tracks` per intero.
//!
//! Il vecchio albero puntava lì (`legacy/.../spotify/keyless.ts`, livello A) e
//! oggi quel livello è morto proprio sul caso d'uso principale. Pathfinder no,
//! perché non è un'API per sviluppatori a cui si applicano quelle restrizioni:
//! è il canale che il lettore web usa per disegnare sé stesso, e finché esiste
//! il lettore web esiste questo.
//!
//! # Cosa lo rende fragile, detto chiaro
//!
//! Non si può mandare il testo di una query GraphQL: si manda il nome
//! dell'operazione e l'impronta SHA-256 di un testo che il server ha già
//! («persisted query»). Quelle impronte cambiano quando Spotify ridistribuisce
//! il lettore, e stanno in [`crate::config`] per poterle correggere senza
//! ricompilare.
//!
//! Anche la **forma** delle risposte cambia. Per questo qui non ci sono
//! strutture `serde` con i campi obbligatori: si naviga un `Value` con percorsi
//! alternativi, e un campo che sparisce costa quel campo invece dell'intera
//! importazione. È lo stesso principio del lettore della pagina incorporabile
//! nel vecchio albero, e la ragione per cui quello ha continuato a funzionare
//! per mesi.
//!
//! # Una pagina persa non perde le altre
//!
//! Lo stesso principio vale sulla paginazione. Una playlist di trecento brani
//! sono tre richieste, e la terza può cadere da sola — un 500, un limite di
//! frequenza, un secondo di rete storta. Finché l'errore della terza faceva
//! fallire tutto il livello, quelle trecento righe diventavano la cinquantina
//! che dà la pagina incorporabile: si buttava via quel che era già arrivato per
//! andarne a prendere di meno.
//!
//! Adesso l'errore ferma il ciclo e basta. Il totale dichiarato è stato letto
//! dalla prima pagina, quindi `SpotifyContent::truncation()` dice «duecento di
//! trecento» e l'avviso arriva fino all'utente. Sulla **prima** pagina invece
//! l'errore continua a propagare: lì non si è letto niente, e scendere di
//! livello è esattamente la cosa giusta da fare.
//!
//! Prima di rassegnarsi, però, si **aspetta**. Il guasto tipico di una playlist
//! lunga non è un 500 ma un `429`: cento pagine di fila sono cento richieste di
//! fila, e a un certo punto Spotify chiede di rallentare. Ritentare all'istante
//! è il modo più diretto di prendersi un secondo `429`, ed è quel che [`interroga`]
//! faceva — con il risultato che una playlist arrivava monca per un guasto che
//! bastava aspettare qualche secondo per non avere. Adesso il `Retry-After` si
//! rispetta, e il troncamento resta per i guasti che lo meritano.
//!
//! # Cosa Pathfinder oggi **non** dà
//!
//! **L'ISRC.** Verificato dal vivo su tutte e tre le forme — brano, album,
//! playlist — ad agosto 2026: nelle risposte non compare né `isrc` né
//! `externalIds`. La rimozione di `external_ids` di febbraio non ha riguardato
//! solo la Web API pubblica.
//!
//! [`isrc`] resta, e non è codice morto: costa tre confronti su un `Value` che
//! si sta già navigando, e il giorno in cui un'impronta nuova riporta il campo
//! lo raccoglie senza che nessuno debba accorgersene. Ma finché è così, quel
//! campo arriva vuoto e il rapporto dell'importazione dirà zero — il che è la
//! verità, non un guasto.
//!
//! `spotify_album_id` invece c'è, e quello è il campo che conta davvero per
//! questo albero: è la chiave con cui `aether_domain::album` fonde le edizioni.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify::{SpotifyContent, SpotifyKind, SpotifySource, SpotifyTrack};
use aether_net::http::{Corpo, Metodo, Richiesta};
use serde_json::{Value, json};

use crate::config::Interrogazione;
use crate::sessione::Sessione;
use crate::url::Riferimento;

/// Il punto delle interrogazioni.
const PUNTO: &str = "https://api-partner.spotify.com/pathfinder/v2/query";

/// Quanti brani per pagina su un album.
const PAGINA_ALBUM: u32 = 50;
/// Quanti brani per pagina su una playlist.
const PAGINA_PLAYLIST: u32 = 100;
/// Quante pagine al massimo, per non girare all'infinito se `totalCount` mente.
const PAGINE_MASSIME: u32 = 200;

/// Quante volte si rimanda la stessa interrogazione prima di arrendersi.
const TENTATIVI: u32 = 3;

/// Quanto si aspetta fra due tentativi quando il servizio non l'ha detto.
///
/// Vale solo come ripiego: se la risposta porta un `Retry-After` è quello a
/// comandare, ed è il caso normale del `429` di Spotify.
const ATTESA_BASE: std::time::Duration = std::time::Duration::from_secs(2);

/// Legge un contenuto da Pathfinder.
///
/// # Errori
///
/// `spotify.tokenUnavailable` se la stretta di mano non riesce,
/// `spotify.notPublic` su un 404, `spotify.resolveFailed` se la risposta non
/// contiene quel che serve.
pub fn risolvi(
    sessione: &mut Sessione,
    riferimento: &Riferimento,
) -> Result<SpotifyContent, AppError> {
    match riferimento.genere {
        SpotifyKind::Track => brano(sessione, riferimento),
        SpotifyKind::Album => album(sessione, riferimento),
        SpotifyKind::Playlist => playlist(sessione, riferimento),
        SpotifyKind::Artist => artista(sessione, riferimento),
    }
}

/// La risposta di Pathfinder così com'è, senza interpretarla.
///
/// Non serve all'applicazione: serve a chi deve capire **perché** un campo è
/// diventato vuoto. Tutto questo modulo naviga forme che Spotify cambia senza
/// dirlo, e quando un giorno gli ISRC spariranno dal rapporto la domanda sarà
/// «dove sono finiti», che nessun messaggio d'errore può rispondere. È il
/// motivo per cui `cargo run --example spotify -- <link> --grezzo` esiste.
///
/// # Errori
///
/// Gli stessi di [`risolvi`].
pub fn grezzo(sessione: &mut Sessione, riferimento: &Riferimento) -> Result<Value, AppError> {
    let interrogazioni = &sessione.configurazione().interrogazioni;
    let interrogazione = match riferimento.genere {
        SpotifyKind::Track => interrogazioni.brano.clone(),
        SpotifyKind::Album => interrogazioni.album.clone(),
        SpotifyKind::Playlist => interrogazioni.playlist.clone(),
        SpotifyKind::Artist => interrogazioni.artista.clone(),
    };
    let variabili = match riferimento.genere {
        SpotifyKind::Track => variabili_brano(riferimento),
        SpotifyKind::Album => variabili_album(riferimento, 0),
        SpotifyKind::Playlist => variabili_playlist(riferimento, 0),
        SpotifyKind::Artist => variabili_artista(riferimento),
    };
    interroga(sessione, &interrogazione, variabili)
}

// ── le variabili ────────────────────────────────────────────────────────────
//
// In funzioni loro e non scritte sul posto perché ognuna ha **due** chiamanti —
// la lettura vera e `grezzo` — e due copie di un elenco di variabili obbligatorie
// sono due copie destinate a divergere: la seconda smetterebbe di funzionare in
// silenzio, cioè proprio nello strumento che serve a capire perché qualcosa non
// funziona.

fn variabili_brano(riferimento: &Riferimento) -> Value {
    json!({ "uri": riferimento.uri() })
}

fn variabili_album(riferimento: &Riferimento, scostamento: u32) -> Value {
    json!({
        "uri": riferimento.uri(),
        "locale": "",
        "offset": scostamento,
        "limit": PAGINA_ALBUM,
    })
}

fn variabili_playlist(riferimento: &Riferimento, scostamento: u32) -> Value {
    json!({
        "uri": riferimento.uri(),
        "offset": scostamento,
        "limit": PAGINA_PLAYLIST,
        // Dichiarata `Boolean!` — non opzionale — dall'interrogazione
        // persistita: senza, Spotify rifiuta la richiesta prima di guardarla e
        // la playlist scende al livello della pagina incorporabile, che si
        // ferma ai primi cinquanta brani e non dichiara un totale. Cioè: mezza
        // playlist, in silenzio.
        //
        // `false` perché è un pezzo di interfaccia del lettore web (la colonna
        // dei video): quel che accende non ci serve, ma la variabile va
        // dichiarata lo stesso.
        "enableWatchFeedEntrypoint": false,
    })
}

fn variabili_artista(riferimento: &Riferimento) -> Value {
    json!({
        "uri": riferimento.uri(),
        "locale": "",
        "includePrerelease": false,
    })
}

// ── le quattro letture ──────────────────────────────────────────────────────

fn brano(sessione: &mut Sessione, riferimento: &Riferimento) -> Result<SpotifyContent, AppError> {
    let interrogazione = sessione.configurazione().interrogazioni.brano.clone();
    let dati = interroga(sessione, &interrogazione, variabili_brano(riferimento))?;
    let radice = unione(&dati, &["trackUnion", "track"])
        .ok_or_else(|| mancante("il brano non è nella risposta"))?;
    let brano = brano_da_json(radice, &Contesto::vuoto())
        .ok_or_else(|| mancante("il brano non ha nemmeno un titolo"))?;

    Ok(SpotifyContent {
        kind: SpotifyKind::Track,
        id: riferimento.id.clone(),
        title: brano.title.clone(),
        author: brano.artist.clone(),
        cover_url: brano.cover_url.clone(),
        tracks: vec![brano],
        declared_total: Some(1),
        source: SpotifySource::Pathfinder,
    })
}

/// Quel che si legge una volta sola, alla prima pagina.
///
/// Le pagine successive ripetono gli stessi campi: leggerli ogni volta
/// significherebbe che l'ultima pagina può contraddire la prima, e il nome di un
/// album non cambia a metà elenco.
struct Intestazione {
    /// Il nome dell'album o della playlist.
    titolo: String,
    /// L'interprete dell'album, o chi possiede la playlist.
    autore: Option<String>,
    /// La copertina.
    copertina: Option<String>,
    /// Quanti brani Spotify dichiara: serve sia a fermare la paginazione sia a
    /// riconoscere un elenco arrivato monco.
    totale: Option<u32>,
}

fn album(sessione: &mut Sessione, riferimento: &Riferimento) -> Result<SpotifyContent, AppError> {
    let interrogazione = sessione.configurazione().interrogazioni.album.clone();
    let mut brani: Vec<SpotifyTrack> = Vec::new();
    let mut intestazione: Option<Intestazione> = None;
    let mut contesto = Contesto::vuoto();
    let mut scostamento = 0_u32;

    for pagina in 0..PAGINE_MASSIME {
        let dati = match interroga(
            sessione,
            &interrogazione,
            variabili_album(riferimento, scostamento),
        ) {
            Ok(dati) => dati,
            Err(err) => {
                if pagina == 0 {
                    return Err(err);
                }
                break;
            }
        };
        let Some(radice) = unione(&dati, &["albumUnion", "album"]) else {
            if pagina == 0 {
                return Err(mancante("l'album non è nella risposta"));
            }
            break;
        };

        if intestazione.is_none() {
            let titolo = testo(radice, "name").unwrap_or_else(|| riferimento.id.clone());
            let interprete = artisti(radice);
            let copertina = copertina(radice);
            let anno = anno(radice);
            // Sull'endpoint dell'album i brani sono in forma ridotta: non
            // portano l'album, l'anno né la copertina, che vanno presi qui e
            // applicati a ognuno. Senza, l'album importato si spacca in tanti
            // album quanti sono i brani.
            contesto = Contesto {
                album: Some(titolo.clone()),
                album_artist: interprete.clone(),
                cover_url: copertina.clone(),
                year: anno,
                album_id: Some(riferimento.id.clone()),
            };
            let totale = numero(radice, &["tracksV2", "totalCount"])
                .or_else(|| numero(radice, &["tracks", "totalCount"]));
            intestazione = Some(Intestazione {
                titolo,
                autore: interprete,
                copertina,
                totale,
            });
        }

        let voci = elenco(radice, &[&["tracksV2", "items"], &["tracks", "items"]]);
        let quante = voci.len();
        for voce in voci {
            // La voce avvolge il brano in `track`; su alcune forme è già il
            // brano.
            let corpo = voce.get("track").unwrap_or(voce);
            if let Some(b) = brano_da_json(corpo, &contesto) {
                brani.push(b);
            }
        }
        if quante == 0 {
            break;
        }
        scostamento = scostamento.saturating_add(PAGINA_ALBUM);
        if let Some(Intestazione {
            totale: Some(totale),
            ..
        }) = &intestazione
            && scostamento >= *totale
        {
            break;
        }
    }

    let testa = intestazione.ok_or_else(|| mancante("l'album non è nella risposta"))?;
    Ok(SpotifyContent {
        kind: SpotifyKind::Album,
        id: riferimento.id.clone(),
        title: testa.titolo,
        author: testa.autore,
        cover_url: testa.copertina,
        tracks: brani,
        declared_total: testa.totale,
        source: SpotifySource::Pathfinder,
    })
}

fn playlist(
    sessione: &mut Sessione,
    riferimento: &Riferimento,
) -> Result<SpotifyContent, AppError> {
    let interrogazione = sessione.configurazione().interrogazioni.playlist.clone();
    let mut brani: Vec<SpotifyTrack> = Vec::new();
    let mut intestazione: Option<Intestazione> = None;
    let mut scostamento = 0_u32;

    for pagina in 0..PAGINE_MASSIME {
        let dati = match interroga(
            sessione,
            &interrogazione,
            variabili_playlist(riferimento, scostamento),
        ) {
            Ok(dati) => dati,
            Err(err) => {
                if pagina == 0 {
                    return Err(err);
                }
                break;
            }
        };
        let Some(radice) = unione(&dati, &["playlistV2", "playlist"]) else {
            if pagina == 0 {
                return Err(mancante("la playlist non è nella risposta"));
            }
            break;
        };

        if intestazione.is_none() {
            let titolo = testo(radice, "name").unwrap_or_else(|| riferimento.id.clone());
            let proprietario = testo_a(radice, &["ownerV2", "data", "name"])
                .or_else(|| testo_a(radice, &["owner", "displayName"]));
            let copertina = copertina(radice).or_else(|| {
                elenco(radice, &[&["images", "items"]])
                    .first()
                    .and_then(|i| sorgente_immagine(i))
            });
            let totale = numero(radice, &["content", "totalCount"])
                .or_else(|| numero(radice, &["content", "pagingInfo", "totalCount"]));
            intestazione = Some(Intestazione {
                titolo,
                autore: proprietario,
                copertina,
                totale,
            });
        }

        let voci = elenco(radice, &[&["content", "items"], &["items"]]);
        let quante = voci.len();
        for voce in voci {
            // `itemV2.data` è la forma corrente; `item.data` e la voce nuda
            // sono le precedenti.
            let corpo = voce
                .get("itemV2")
                .and_then(|i| i.get("data"))
                .or_else(|| voce.get("item").and_then(|i| i.get("data")))
                .or_else(|| voce.get("track"))
                .unwrap_or(voce);
            if let Some(b) = brano_da_json(corpo, &Contesto::vuoto()) {
                brani.push(b);
            }
        }
        if quante == 0 {
            break;
        }
        scostamento = scostamento.saturating_add(PAGINA_PLAYLIST);
        if let Some(Intestazione {
            totale: Some(totale),
            ..
        }) = &intestazione
            && scostamento >= *totale
        {
            break;
        }
    }

    let testa = intestazione.ok_or_else(|| mancante("la playlist non è nella risposta"))?;
    let copertina = testa
        .copertina
        .or_else(|| brani.first().and_then(|b| b.cover_url.clone()));
    Ok(SpotifyContent {
        kind: SpotifyKind::Playlist,
        id: riferimento.id.clone(),
        title: testa.titolo,
        author: testa.autore,
        cover_url: copertina,
        tracks: brani,
        declared_total: testa.totale,
        source: SpotifySource::Pathfinder,
    })
}

/// Di un artista si prende quel che il lettore mostra in copertina.
///
/// Nel vecchio albero erano i «brani più ascoltati», presi da
/// `GET /artists/{id}/top-tracks` — endpoint **rimosso** a febbraio 2026. Qui
/// arrivano dalla stessa panoramica che il lettore disegna sulla pagina
/// dell'artista, che è l'unica fonte rimasta e dice più o meno la stessa cosa.
fn artista(sessione: &mut Sessione, riferimento: &Riferimento) -> Result<SpotifyContent, AppError> {
    let interrogazione = sessione.configurazione().interrogazioni.artista.clone();
    let dati = interroga(sessione, &interrogazione, variabili_artista(riferimento))?;
    let radice = unione(&dati, &["artistUnion", "artist"])
        .ok_or_else(|| mancante("l'artista non è nella risposta"))?;

    let nome = testo_a(radice, &["profile", "name"])
        .or_else(|| testo(radice, "name"))
        .unwrap_or_else(|| riferimento.id.clone());
    let copertina = elenco(radice, &[&["visuals", "avatarImage", "sources"]])
        .first()
        .and_then(|v| testo(v, "url"));

    let contesto = Contesto {
        album_artist: Some(nome.clone()),
        ..Contesto::vuoto()
    };
    let mut brani = Vec::new();
    for voce in elenco(
        radice,
        &[
            &["discography", "topTracks", "items"],
            &["stats", "topTracks", "items"],
        ],
    ) {
        let corpo = voce.get("track").unwrap_or(voce);
        if let Some(b) = brano_da_json(corpo, &contesto) {
            brani.push(b);
        }
    }

    let quanti = u32::try_from(brani.len()).unwrap_or(u32::MAX);
    Ok(SpotifyContent {
        kind: SpotifyKind::Artist,
        id: riferimento.id.clone(),
        title: nome.clone(),
        author: Some(nome),
        cover_url: copertina,
        tracks: brani,
        // Quel che si è letto è tutto quel che c'era da leggere: dichiarare un
        // totale diverso farebbe apparire un troncamento che non c'è.
        declared_total: Some(quanti),
        source: SpotifySource::Pathfinder,
    })
}

// ── il trasporto ────────────────────────────────────────────────────────────

/// Manda un'interrogazione e restituisce il nodo `data`.
fn interroga(
    sessione: &mut Sessione,
    interrogazione: &Interrogazione,
    variabili: Value,
) -> Result<Value, AppError> {
    let mut ultimo: Option<AppError> = None;
    // Tre tentativi. Il secondo nasce per il 401, che è il modo in cui Spotify
    // dice «quel gettone non vale più» prima della scadenza dichiarata; il terzo
    // per il `429`, che sulle playlist lunghe è il guasto vero — vedi l'attesa
    // qui sotto.
    for tentativo in 0..TENTATIVI {
        let (accesso, client_token) = sessione.gettoni()?;
        let corpo = json!({
            "operationName": interrogazione.operazione,
            "variables": variabili,
            "extensions": {
                "persistedQuery": { "version": 1, "sha256Hash": interrogazione.hash }
            }
        });
        let Ok(byte) = serde_json::to_vec(&corpo) else {
            return Err(mancante("non si è riusciti a comporre l'interrogazione"));
        };
        let autorizzazione = format!("Bearer {accesso}");

        let risposta = sessione.rete().esegui(Richiesta {
            metodo: Metodo::Post,
            url: PUNTO,
            intestazioni: &[
                ("authorization", autorizzazione.as_str()),
                ("client-token", client_token.as_str()),
                ("accept", "application/json"),
                ("app-platform", "WebPlayer"),
                ("origin", "https://open.spotify.com"),
                ("referer", "https://open.spotify.com/"),
            ],
            corpo: Corpo::Byte {
                tipo: "application/json",
                dati: &byte,
            },
        })?;

        if risposta.stato == 401 && tentativo == 0 {
            sessione.invalida();
            continue;
        }
        if risposta.stato == 404 {
            return Err(AppError::new(ErrorCode::SpotifyNotPublic));
        }
        if !risposta.e_andata() {
            let errore = sessione.rete().stato_a_errore(&risposta, PUNTO);
            if errore.is_retryable() && tentativo + 1 < TENTATIVI {
                // **Aspettando.** Senza questa riga un `429` veniva ripetuto
                // all'istante, cioè con la certezza di prenderne un secondo:
                // dopo di che il ciclo delle pagine usciva e la playlist
                // arrivava monca, in silenzio, per un guasto che bastava
                // aspettare qualche secondo per non avere. È lo stesso calcolo
                // che fa `Rete::con_tentativi` — il tempo che il servizio ha
                // chiesto vince sul nostro — e il tetto di mezzo minuto è il suo.
                std::thread::sleep(aether_net::http::quanto_aspettare(&errore, ATTESA_BASE));
                ultimo = Some(errore);
                continue;
            }
            return Err(errore);
        }

        let Ok(letto) = serde_json::from_slice::<Value>(&risposta.corpo) else {
            return Err(mancante("la risposta non è JSON").with_cause(risposta.testo()));
        };
        // GraphQL risponde 200 anche quando fallisce: l'errore sta nel corpo, e
        // ignorarlo vorrebbe dire leggere un `data` vuoto e chiamarlo «playlist
        // senza brani».
        if let Some(errori) = letto.get("errors").and_then(Value::as_array)
            && !errori.is_empty()
        {
            let dettaglio = errori
                .first()
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("errore senza messaggio");
            // Un'impronta che il server non conosce più è *il* guasto da
            // riconoscere: dice esattamente cosa correggere e dove.
            if dettaglio.contains("PersistedQueryNotFound") {
                return Err(AppError::new(ErrorCode::SpotifyResolveFailed)
                    .with_message(
                        "Spotify non riconosce più l'impronta dell'interrogazione: \
                         va aggiornata in spotify.json",
                    )
                    .with_cause(dettaglio.to_owned()));
            }
            return Err(
                mancante("Spotify ha risposto con un errore").with_cause(dettaglio.to_owned())
            );
        }

        return letto
            .get("data")
            .cloned()
            .ok_or_else(|| mancante("la risposta non contiene dati"));
    }
    Err(ultimo.unwrap_or_else(|| mancante("l'interrogazione non è riuscita")))
}

/// L'errore generico di questo modulo.
fn mancante(cosa: &str) -> AppError {
    AppError::new(ErrorCode::SpotifyResolveFailed).with_message(cosa.to_owned())
}

// ── navigazione difensiva del JSON ──────────────────────────────────────────
//
// Ogni funzione qui accetta che il campo non ci sia. È la scelta che rende
// questo modulo sopravvivibile a una ridistribuzione del lettore: un campo
// spostato costa quel campo, non l'importazione.

/// Il contesto dell'album, da applicare ai brani che non se lo portano dietro.
#[derive(Debug, Clone, Default)]
struct Contesto {
    album: Option<String>,
    album_artist: Option<String>,
    cover_url: Option<String>,
    year: Option<i32>,
    album_id: Option<String>,
}

impl Contesto {
    fn vuoto() -> Self {
        Self::default()
    }
}

/// Il primo dei nomi che esiste dentro `data`.
fn unione<'a>(dati: &'a Value, nomi: &[&str]) -> Option<&'a Value> {
    for nome in nomi {
        if let Some(v) = dati.get(nome)
            && !v.is_null()
        {
            return Some(v);
        }
    }
    // Nessuno dei nomi noti: se `data` ha un solo figlio, è quello. Copre le
    // ridenominazioni senza indovinare quando c'è ambiguità.
    dati.as_object()
        .filter(|o| o.len() == 1)
        .and_then(|o| o.values().next())
}

fn testo(v: &Value, chiave: &str) -> Option<String> {
    v.get(chiave)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn testo_a(v: &Value, percorso: &[&str]) -> Option<String> {
    let mut corrente = v;
    for passo in percorso {
        corrente = corrente.get(passo)?;
    }
    corrente
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn numero(v: &Value, percorso: &[&str]) -> Option<u32> {
    let mut corrente = v;
    for passo in percorso {
        corrente = corrente.get(passo)?;
    }
    corrente.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// Il primo dei percorsi che dà un array.
fn elenco<'a>(v: &'a Value, percorsi: &[&[&str]]) -> Vec<&'a Value> {
    for percorso in percorsi {
        let mut corrente = Some(v);
        for passo in *percorso {
            corrente = corrente.and_then(|c| c.get(passo));
        }
        if let Some(array) = corrente.and_then(Value::as_array)
            && !array.is_empty()
        {
            return array.iter().collect();
        }
    }
    Vec::new()
}

/// Gli interpreti, uniti da virgole come fa Spotify.
/// Gli interpreti, nell'ordine in cui Spotify li mette.
///
/// # Due forme, non una
///
/// Le voci di album e di playlist portano tutti gli interpreti in un elenco
/// solo, `artists.items`. Il nodo di un **brano singolo** no: lì Spotify li
/// divide in `firstArtist` e `otherArtists`, e una lettura che cercasse solo la
/// prima forma tornerebbe a mani vuote proprio sul brano singolo — che è quel
/// che succedeva, in silenzio, finché non l'ha detto una prova dal vivo.
///
/// L'ordine conta: il primo interprete è quello su cui
/// [`aether_domain::spotify_plan::primo_artista`] taglia per abbinare, e
/// invertirlo vorrebbe dire abbinare i duetti sull'ospite.
fn artisti(v: &Value) -> Option<String> {
    let mut nomi = nomi_di(&elenco(v, &[&["artists", "items"], &["artist", "items"]]));
    if nomi.is_empty() {
        nomi = nomi_di(&elenco(v, &[&["firstArtist", "items"]]));
        nomi.extend(nomi_di(&elenco(v, &[&["otherArtists", "items"]])));
    }
    (!nomi.is_empty()).then(|| nomi.join(", "))
}

/// I nomi di un elenco di nodi artista, in una forma o nell'altra.
fn nomi_di(voci: &[&Value]) -> Vec<String> {
    voci.iter()
        .filter_map(|a| testo_a(a, &["profile", "name"]).or_else(|| testo(a, "name")))
        .collect()
}

/// L'indirizzo di un'immagine, dalla sorgente più grande.
fn sorgente_immagine(v: &Value) -> Option<String> {
    let sorgenti = elenco(v, &[&["sources"]]);
    // L'ultima è la più grande nelle risposte di Spotify; se l'ordine
    // cambiasse, la prima è comunque un'immagine valida.
    sorgenti
        .last()
        .and_then(|s| testo(s, "url"))
        .or_else(|| sorgenti.first().and_then(|s| testo(s, "url")))
}

fn copertina(v: &Value) -> Option<String> {
    v.get("coverArt").and_then(sorgente_immagine)
}

/// L'identificativo in fondo a un URI `spotify:album:ID`.
fn id_da_uri(uri: &str) -> Option<String> {
    uri.rsplit(':')
        .next()
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(ToOwned::to_owned)
}

/// L'ISRC, cercato in tutte le forme in cui Pathfinder l'ha esposto.
///
/// Oggi **nessuna**: vedi la nota in testa al modulo. Resta perché costa nulla
/// su un `Value` che si sta già navigando, e perché il giorno in cui torna
/// torna da solo.
fn isrc(v: &Value) -> Option<String> {
    if let Some(diretto) = testo(v, "isrc") {
        return Some(diretto);
    }
    if let Some(dentro) = testo_a(v, &["externalIds", "isrc"]) {
        return Some(dentro);
    }
    // La forma a elenco: `externalIds.items: [{ type: "ISRC", id: "..." }]`.
    elenco(v, &[&["externalIds", "items"]])
        .into_iter()
        .find(|e| testo(e, "type").is_some_and(|t| t.eq_ignore_ascii_case("isrc")))
        .and_then(|e| testo(e, "id"))
}

/// L'anno, dalla data di pubblicazione in una qualsiasi delle sue forme.
fn anno(v: &Value) -> Option<i32> {
    if let Some(n) = v
        .get("date")
        .and_then(|d| d.get("year"))
        .and_then(Value::as_i64)
    {
        return i32::try_from(n).ok();
    }
    let iso = testo_a(v, &["date", "isoString"]).or_else(|| testo(v, "releaseDate"))?;
    iso.get(..4).and_then(|a| a.parse::<i32>().ok())
}

/// Un brano, da qualunque forma di oggetto brano.
fn brano_da_json(v: &Value, contesto: &Contesto) -> Option<SpotifyTrack> {
    let titolo = testo(v, "name").or_else(|| testo(v, "title"))?;

    let album_di = v.get("albumOfTrack").or_else(|| v.get("album"));
    let album = album_di
        .and_then(|a| testo(a, "name"))
        .or_else(|| contesto.album.clone());
    let album_artist = album_di
        .and_then(artisti)
        .or_else(|| contesto.album_artist.clone());
    let album_id = album_di
        .and_then(|a| testo(a, "uri"))
        .and_then(|uri| id_da_uri(&uri))
        .or_else(|| contesto.album_id.clone());
    let copertina_brano = album_di
        .and_then(copertina)
        .or_else(|| contesto.cover_url.clone());
    let anno_brano = album_di.and_then(anno).or(contesto.year);

    let artista = artisti(v);
    Some(SpotifyTrack {
        title: titolo,
        // Se il brano non dichiara interpreti si usa quello dell'album: è
        // meglio di niente per l'abbinamento, e su un album è quasi sempre
        // giusto.
        artist: artista.clone().or_else(|| album_artist.clone()),
        album,
        album_artist: album_artist.or(artista),
        disc_number: numero(v, &["discNumber"]).or_else(|| numero(v, &["disc_number"])),
        track_number: numero(v, &["trackNumber"]).or_else(|| numero(v, &["track_number"])),
        year: anno_brano,
        duration_ms: v
            .get("duration")
            .and_then(|d| d.get("totalMilliseconds"))
            .and_then(Value::as_u64)
            .or_else(|| v.get("duration_ms").and_then(Value::as_u64)),
        isrc: isrc(v),
        spotify_track_id: testo(v, "uri").and_then(|uri| id_da_uri(&uri)),
        spotify_album_id: album_id,
        cover_url: copertina_brano,
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_brano_completo_si_legge_tutto() {
        let v = json!({
            "uri": "spotify:track:4cOdK2wGLETKBW3PvgPWqT",
            "name": "Karma Police",
            "duration": { "totalMilliseconds": 264_066 },
            "trackNumber": 6,
            "discNumber": 1,
            "artists": { "items": [ { "profile": { "name": "Radiohead" } } ] },
            "albumOfTrack": {
                "uri": "spotify:album:6dVIqQ8qmQ5GBnJ9shOYGE",
                "name": "OK Computer",
                "date": { "year": 1997 },
                "coverArt": { "sources": [
                    { "url": "https://piccola" },
                    { "url": "https://grande" }
                ] },
                "artists": { "items": [ { "profile": { "name": "Radiohead" } } ] }
            },
            "externalIds": { "items": [ { "type": "ISRC", "id": "GBAYE9700426" } ] }
        });
        let Some(b) = brano_da_json(&v, &Contesto::vuoto()) else {
            panic!("il brano si deve leggere");
        };
        assert_eq!(b.title, "Karma Police");
        assert_eq!(b.artist.as_deref(), Some("Radiohead"));
        assert_eq!(b.album.as_deref(), Some("OK Computer"));
        assert_eq!(b.duration_ms, Some(264_066));
        assert_eq!(b.track_number, Some(6));
        assert_eq!(b.year, Some(1997));
        assert_eq!(b.isrc.as_deref(), Some("GBAYE9700426"));
        assert_eq!(
            b.spotify_track_id.as_deref(),
            Some("4cOdK2wGLETKBW3PvgPWqT")
        );
        assert_eq!(
            b.spotify_album_id.as_deref(),
            Some("6dVIqQ8qmQ5GBnJ9shOYGE"),
            "è l'aggancio che album.rs usa per fondere le edizioni"
        );
        assert_eq!(
            b.cover_url.as_deref(),
            Some("https://grande"),
            "delle sorgenti si prende la più grande"
        );
    }

    /// La forma del nodo di un **brano singolo**, com'è arrivata davvero da
    /// `trackUnion` ad agosto 2026: gli interpreti non stanno in `artists.items`
    /// ma divisi fra `firstArtist` e `otherArtists`, e `albumOfTrack` non ne
    /// porta nessuno.
    ///
    /// Questa è la forma che faceva uscire un brano **senza interprete** — cioè
    /// inabbinabile, perché la chiave della libreria comincia dall'artista. Non
    /// l'ha trovata nessuna prova: l'ha trovata un link vero. Adesso è qui.
    #[test]
    fn un_brano_singolo_ha_gli_interpreti_divisi_in_due() {
        let v = json!({
            "uri": "spotify:track:0DiWol3AO6WpXZgp0goxAV",
            "name": "One More Time",
            "duration": { "totalMilliseconds": 320_357 },
            "trackNumber": 1,
            "firstArtist": { "items": [ { "profile": { "name": "Daft Punk" } } ] },
            "otherArtists": { "items": [ { "profile": { "name": "Romanthony" } } ] },
            "albumOfTrack": {
                "uri": "spotify:album:2noRn2Aes5aoNVsU6iWThc",
                "name": "Discovery",
                "date": { "year": 2001 },
                "coverArt": { "sources": [ { "url": "https://copertina" } ] }
            }
        });
        let Some(b) = brano_da_json(&v, &Contesto::vuoto()) else {
            panic!("il brano si deve leggere");
        };
        assert_eq!(
            b.artist.as_deref(),
            Some("Daft Punk, Romanthony"),
            "il primo interprete per primo: è quello su cui si taglia per abbinare"
        );
        assert_eq!(b.album.as_deref(), Some("Discovery"));
        assert_eq!(
            b.spotify_album_id.as_deref(),
            Some("2noRn2Aes5aoNVsU6iWThc")
        );
        assert_eq!(
            b.isrc, None,
            "Pathfinder non lo manda più, e va letto come «non c'è» non come «non l'abbiamo cercato»"
        );
    }

    /// `artists.items` vince su `firstArtist` quando ci sono entrambi: è la
    /// forma completa, e mescolare le due darebbe nomi doppi.
    #[test]
    fn la_forma_completa_ha_la_precedenza() {
        let v = json!({
            "artists": { "items": [ { "profile": { "name": "Air" } } ] },
            "firstArtist": { "items": [ { "profile": { "name": "Sbagliato" } } ] }
        });
        assert_eq!(artisti(&v).as_deref(), Some("Air"));
    }

    #[test]
    fn un_brano_ridotto_eredita_il_contesto_dell_album() {
        // È la forma che arriva dall'endpoint dell'album: niente album dentro il
        // brano. Senza il contesto, un album di dodici brani diventerebbe dodici
        // album senza nome.
        let contesto = Contesto {
            album: Some("OK Computer".to_owned()),
            album_artist: Some("Radiohead".to_owned()),
            cover_url: Some("https://copertina".to_owned()),
            year: Some(1997),
            album_id: Some("6dVIqQ8qmQ5GBnJ9shOYGE".to_owned()),
        };
        let v = json!({
            "uri": "spotify:track:x1",
            "name": "Airbag",
            "duration": { "totalMilliseconds": 284_000 },
            "artists": { "items": [ { "profile": { "name": "Radiohead" } } ] }
        });
        let Some(b) = brano_da_json(&v, &contesto) else {
            panic!("il brano si deve leggere");
        };
        assert_eq!(b.album.as_deref(), Some("OK Computer"));
        assert_eq!(b.year, Some(1997));
        assert_eq!(b.cover_url.as_deref(), Some("https://copertina"));
        assert_eq!(
            b.spotify_album_id.as_deref(),
            Some("6dVIqQ8qmQ5GBnJ9shOYGE")
        );
    }

    #[test]
    fn un_brano_senza_titolo_non_e_un_brano() {
        assert!(brano_da_json(&json!({ "uri": "spotify:track:x" }), &Contesto::vuoto()).is_none());
        assert!(brano_da_json(&json!({ "name": "  " }), &Contesto::vuoto()).is_none());
    }

    #[test]
    fn i_campi_che_mancano_costano_solo_sé_stessi() {
        // Il principio del modulo, fissato in un test: una risposta ridotta
        // all'osso produce comunque un brano abbinabile.
        let v = json!({ "name": "Solo il titolo" });
        let Some(b) = brano_da_json(&v, &Contesto::vuoto()) else {
            panic!("deve restare leggibile");
        };
        assert_eq!(b.title, "Solo il titolo");
        assert_eq!(b.duration_ms, None);
        assert_eq!(b.isrc, None);
    }

    #[test]
    fn gli_interpreti_si_uniscono_come_fa_spotify() {
        let v = json!({ "artists": { "items": [
            { "profile": { "name": "Gorillaz" } },
            { "profile": { "name": "De La Soul" } }
        ] } });
        assert_eq!(artisti(&v).as_deref(), Some("Gorillaz, De La Soul"));
        assert_eq!(artisti(&json!({})), None);
    }

    #[test]
    fn lisrc_si_trova_in_tutte_e_tre_le_forme() {
        assert_eq!(
            isrc(&json!({ "isrc": "ITX123456789" })).as_deref(),
            Some("ITX123456789")
        );
        assert_eq!(
            isrc(&json!({ "externalIds": { "isrc": "ITX123456789" } })).as_deref(),
            Some("ITX123456789")
        );
        assert_eq!(
            isrc(&json!({ "externalIds": { "items": [
                { "type": "UPC", "id": "000" },
                { "type": "isrc", "id": "ITX123456789" }
            ] } }))
            .as_deref(),
            Some("ITX123456789")
        );
        assert_eq!(isrc(&json!({})), None);
    }

    #[test]
    fn lunione_sopravvive_a_una_ridenominazione() {
        let dati = json!({ "trackUnion": { "name": "x" } });
        assert!(unione(&dati, &["trackUnion"]).is_some());
        // Nome sconosciuto ma figlio unico: si prende quello.
        let rinominato = json!({ "trackUnionV3": { "name": "x" } });
        assert!(unione(&rinominato, &["trackUnion"]).is_some());
        // Due figli: non si indovina.
        let ambiguo = json!({ "a": { "name": "x" }, "b": { "name": "y" } });
        assert!(unione(&ambiguo, &["trackUnion"]).is_none());
    }

    #[test]
    fn lanno_si_legge_da_ogni_forma_di_data() {
        assert_eq!(anno(&json!({ "date": { "year": 1997 } })), Some(1997));
        assert_eq!(
            anno(&json!({ "date": { "isoString": "1997-05-21T00:00:00Z" } })),
            Some(1997)
        );
        assert_eq!(anno(&json!({ "releaseDate": "1997" })), Some(1997));
        assert_eq!(anno(&json!({})), None);
    }

    #[test]
    fn un_uri_storto_non_diventa_un_identificativo() {
        assert_eq!(
            id_da_uri("spotify:album:6dVIqQ8qmQ5GBnJ9shOYGE").as_deref(),
            Some("6dVIqQ8qmQ5GBnJ9shOYGE")
        );
        assert_eq!(id_da_uri("spotify:album:"), None);
        assert_eq!(id_da_uri("spotify:local:x:y:non-valido!"), None);
    }
}
