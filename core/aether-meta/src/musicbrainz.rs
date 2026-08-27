//! MusicBrainz: il catalogo, e gli identificativi che ne discendono.
//!
//! # Perché è la fonte principale e non una delle tre
//!
//! Perché è l'unica che dà un **nome stabile** alle cose. Un titolo corretto lo
//! sanno anche iTunes e Deezer; un `mb_release_group_id` no, ed è quello che
//! permette a [`aether_domain::album::build_album_groups`] di riconoscere che
//! due cartelle sono lo stesso disco, e al Cover Art Archive di sapere quale
//! immagine appartiene a quale pubblicazione. Senza MusicBrainz l'arricchimento
//! sistemerebbe dei testi; con MusicBrainz mette in ordine la libreria.
//!
//! # Come si spendono le richieste
//!
//! Una al secondo, e sono poche. Il modo di sprecarle è chiedere la
//! pubblicazione per intero a ogni risultato di ricerca: cinque candidati sono
//! cinque letture, cioè sei secondi per album.
//!
//! La ricerca però restituisce già il **numero di tracce** di ogni candidato, e
//! un disco con un numero di tracce diverso dal nostro non verrà mai applicato
//! ([`aether_domain::enrich::decide_album`] lo pretende esatto). Quel campo fa da
//! setaccio prima della spesa: si leggono solo i candidati che potrebbero
//! vincere, e su una libreria vera sono uno o due invece di cinque.
//!
//! # La ricerca a scaglioni
//!
//! I tag di un file scaricato raramente combaciano con una frase esatta, e una
//! frase esatta è quel che serve perché il punteggio di MusicBrainz voglia dire
//! qualcosa. Si allarga per gradi e ci si ferma al primo scaglione che risponde
//! — titolo ripulito più interprete, poi i due grezzi, poi il solo titolo — così
//! il caso normale costa una richiesta e quello difficile ne costa tre invece di
//! restituire spazzatura.

use aether_domain::enrich::{Candidate, FonteMeta, RemoteRelease, RemoteTrack, titolo_da_cercare};
use aether_domain::errors::{AppError, ErrorCode};
use aether_net::percento;
use serde_json::Value;

use crate::deposito::{VIVE_PUBBLICAZIONE_MS, VIVE_RICERCA_MS};
use crate::{Fornitori, termine_lucene};

/// Il punto delle API.
const BASE: &str = "https://musicbrainz.org/ws/2";

/// Quante pubblicazioni si leggono per intero, al massimo.
///
/// Tre. Il setaccio sul numero di tracce ne lascia passare poche, e oltre la
/// terza si stanno leggendo ristampe che differiscono per il paese di
/// pubblicazione — cioè per un campo che non partecipa a nessuna decisione.
const LETTURE_MASSIME: usize = 3;

/// Sotto questo punteggio MusicBrainz stesso non ci crede.
///
/// È il cancello di sanità di `pickBestRecording` del vecchio albero, e resta
/// **fuori** dal punteggio composito: è un'opinione del motore di ricerca sulla
/// propria risposta, non una misura di quanto quel candidato somigli al nostro
/// file. Mescolarla al punteggio vorrebbe dire far pesare due volte la stessa
/// somiglianza testuale.
const PUNTEGGIO_MINIMO: u64 = 50;

/// Un risultato di ricerca, prima di spendere una lettura per averlo intero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stub {
    /// L'identificativo della pubblicazione.
    pub id: String,
    /// Il titolo.
    pub titolo: String,
    /// L'interprete.
    pub artista: String,
    /// Quante tracce dichiara, sommate su tutti i dischi.
    pub tracce: Option<usize>,
    /// Il punteggio che MusicBrainz dà alla propria risposta.
    pub punteggio: Option<u64>,
}

/// Il guasto di MusicBrainz, con la causa vera in coda.
///
/// Un codice solo per chi lo mostra — «i metadati non si possono cercare
/// adesso» — e la distinzione fra un timeout e un `503` conservata nella causa,
/// che è dove serve a chi legge un registro.
fn indisponibile(err: &AppError) -> AppError {
    AppError::new(ErrorCode::MetadataMusicbrainzUnavailable).with_cause(format!(
        "{} {}",
        err.code().kind().code(),
        err.cause().unwrap_or("—")
    ))
}

/// Il nome che l'artist-credit compone.
///
/// MusicBrainz spezza un interprete composito in più voci con la loro
/// congiunzione (`joinphrase`): `[{name:"A", joinphrase:" feat. "}, {name:"B"}]`
/// è «A feat. B». Ricomporlo con la congiunzione e non con una virgola è quel
/// che permette al confronto di ritrovare la forma che sta nei tag, dove la
/// congiunzione è scritta per esteso.
fn artista_composto(credito: Option<&Value>) -> String {
    let Some(voci) = credito.and_then(Value::as_array) else {
        return String::new();
    };
    let mut fuori = String::new();
    for voce in voci {
        if let Some(nome) = voce.get("name").and_then(Value::as_str) {
            fuori.push_str(nome);
        }
        if let Some(giunzione) = voce.get("joinphrase").and_then(Value::as_str) {
            fuori.push_str(giunzione);
        }
    }
    fuori.trim().to_owned()
}

/// L'anno da una data MusicBrainz, che può essere `1997`, `1997-03` o completa.
fn anno(data: Option<&str>) -> Option<i32> {
    data?.get(..4)?.parse::<i32>().ok().filter(|a| *a > 0)
}

/// Interpreta la risposta di una ricerca di pubblicazioni. Funzione pura.
#[must_use]
pub fn interpreta_ricerca(corpo: &[u8]) -> Vec<Stub> {
    let Ok(letto) = serde_json::from_slice::<Value>(corpo) else {
        return Vec::new();
    };
    let Some(elenco) = letto.get("releases").and_then(Value::as_array) else {
        return Vec::new();
    };
    elenco
        .iter()
        .filter_map(|voce| {
            let id = voce.get("id").and_then(Value::as_str)?;
            let titolo = voce.get("title").and_then(Value::as_str)?;
            // `track-count` in cima è la somma su tutti i dischi; quando manca
            // si somma da `media`, che è la forma che le risposte più vecchie
            // usano. Senza il totale il setaccio non si può applicare, e il
            // candidato passa: un dato che manca non deve escludere niente.
            let tracce = voce
                .get("track-count")
                .and_then(Value::as_u64)
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
                .or_else(|| somma_tracce(voce.get("media")));
            Some(Stub {
                id: id.to_owned(),
                titolo: titolo.to_owned(),
                artista: artista_composto(voce.get("artist-credit")),
                tracce,
                punteggio: voce.get("score").and_then(Value::as_u64),
            })
        })
        .collect()
}

/// Somma le tracce dichiarate dai dischi di una pubblicazione.
fn somma_tracce(media: Option<&Value>) -> Option<usize> {
    let dischi = media?.as_array()?;
    if dischi.is_empty() {
        return None;
    }
    let mut totale = 0_usize;
    for disco in dischi {
        let quante = disco.get("track-count").and_then(Value::as_u64)?;
        totale = totale.saturating_add(usize::try_from(quante).unwrap_or(0));
    }
    Some(totale)
}

/// Interpreta una pubblicazione letta per intero. Funzione pura.
///
/// Le tracce si appiattiscono su tutti i dischi conservando il numero di disco:
/// un doppio album in libreria sta in una cartella sola — `album_folder`
/// risolve `CD1` nella cartella superiore — quindi il gruppo da abbinare è uno
/// e le tracce sono tutte.
#[must_use]
pub fn interpreta_pubblicazione(corpo: &[u8]) -> Option<RemoteRelease> {
    let letto: Value = serde_json::from_slice(corpo).ok()?;
    let titolo = letto.get("title").and_then(Value::as_str)?;

    let mut tracce = Vec::new();
    if let Some(dischi) = letto.get("media").and_then(Value::as_array) {
        for (indice, disco) in dischi.iter().enumerate() {
            let numero_disco = disco
                .get("position")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .or_else(|| u32::try_from(indice.saturating_add(1)).ok());
            let Some(elenco) = disco.get("tracks").and_then(Value::as_array) else {
                continue;
            };
            for traccia in elenco {
                let registrazione = traccia.get("recording");
                // Il titolo della **traccia** e non quello della registrazione:
                // sono quasi sempre uguali, e quando differiscono è la traccia
                // ad avere il nome con cui quel disco la chiama — che è quello
                // stampato sulla copertina e quello che l'utente si aspetta.
                let Some(nome) = traccia
                    .get("title")
                    .and_then(Value::as_str)
                    .or_else(|| registrazione?.get("title")?.as_str())
                else {
                    continue;
                };
                tracce.push(RemoteTrack {
                    title: nome.to_owned(),
                    artist: Some(artista_composto(traccia.get("artist-credit")))
                        .filter(|a| !a.is_empty()),
                    duration_ms: traccia
                        .get("length")
                        .and_then(Value::as_u64)
                        .or_else(|| registrazione?.get("length")?.as_u64())
                        .filter(|d| *d > 0),
                    track_number: traccia
                        .get("position")
                        .and_then(Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                    disc_number: numero_disco,
                    mb_recording_id: registrazione
                        .and_then(|r| r.get("id"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                });
            }
        }
    }

    Some(RemoteRelease {
        title: titolo.to_owned(),
        artist: artista_composto(letto.get("artist-credit")),
        year: anno(letto.get("date").and_then(Value::as_str)).or_else(|| {
            // La data del **gruppo** di pubblicazione quando la ristampa non ne
            // ha una: è l'anno in cui il disco è uscito, che è quello che chi
            // guarda una libreria si aspetta di leggere.
            anno(
                letto
                    .get("release-group")?
                    .get("first-release-date")?
                    .as_str(),
            )
        }),
        tracks: tracce,
        mb_release_id: letto
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        mb_release_group_id: letto
            .get("release-group")
            .and_then(|g| g.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

/// Interpreta una ricerca di registrazioni. Funzione pura.
#[must_use]
pub fn interpreta_registrazioni(corpo: &[u8]) -> Vec<Candidate> {
    let Ok(letto) = serde_json::from_slice::<Value>(corpo) else {
        return Vec::new();
    };
    let Some(elenco) = letto.get("recordings").and_then(Value::as_array) else {
        return Vec::new();
    };
    elenco
        .iter()
        .filter(|voce| {
            voce.get("score")
                .and_then(Value::as_u64)
                .is_none_or(|p| p >= PUNTEGGIO_MINIMO)
        })
        .filter_map(|voce| {
            let titolo = voce.get("title").and_then(Value::as_str)?;
            let pubblicazione = scegli_pubblicazione(voce.get("releases"));
            Some(Candidate {
                fonte: Some(FonteMeta::MusicBrainz),
                title: titolo.to_owned(),
                artist: artista_composto(voce.get("artist-credit")),
                album: pubblicazione
                    .and_then(|p| p.get("title"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                year: anno(
                    pubblicazione
                        .and_then(|p| p.get("date"))
                        .and_then(Value::as_str),
                ),
                genre: None,
                duration_ms: voce
                    .get("length")
                    .and_then(Value::as_u64)
                    .filter(|d| *d > 0),
                cover_url: None,
                mb_recording_id: voce
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                mb_release_id: pubblicazione
                    .and_then(|p| p.get("id"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                mb_release_group_id: pubblicazione
                    .and_then(|p| p.get("release-group"))
                    .and_then(|g| g.get("id"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            })
        })
        .collect()
}

/// La pubblicazione da cui prendere album e copertina, fra quelle elencate.
///
/// Si preferisce un album ufficiale. Prendere la prima significa prendere quella
/// che il catalogo restituisce per prima, che è spesso una raccolta o un singolo
/// promozionale — e da lì discende la copertina, che finirebbe in libreria a
/// rappresentare un disco che l'utente non ha.
fn scegli_pubblicazione(elenco: Option<&Value>) -> Option<&Value> {
    let voci = elenco?.as_array()?;
    let rango = |voce: &Value| -> u8 {
        let mut punti = 0_u8;
        if voce
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("official"))
        {
            punti = punti.saturating_add(2);
        }
        if voce
            .get("release-group")
            .and_then(|g| g.get("primary-type"))
            .and_then(Value::as_str)
            .is_some_and(|t| t.eq_ignore_ascii_case("album"))
        {
            punti = punti.saturating_add(1);
        }
        punti
    };
    // `max_by_key` restituisce l'ultimo massimo: si scorre al contrario perché a
    // pari rango vinca la prima, che è quella che MusicBrainz ritiene più
    // rilevante.
    voci.iter().rev().max_by_key(|voce| rango(voce))
}

/// Le pubblicazioni candidate per un gruppo d'album, già lette per intero.
///
/// # Errori
///
/// `metadata.musicbrainzUnavailable` quando il servizio non risponde. Un elenco
/// vuoto vuol dire invece che ha risposto e non ha quel disco.
pub fn pubblicazioni(
    fornitori: &Fornitori,
    titolo: &str,
    artista: &str,
    tracce: usize,
) -> Result<Vec<RemoteRelease>, AppError> {
    let titolo_pulito = termine_lucene(&titolo_da_cercare(titolo));
    let artista_pulito = termine_lucene(artista);
    if titolo_pulito.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = format!("release:\"{titolo_pulito}\"");
    if !artista_pulito.is_empty() {
        query.push_str(&format!(" AND artist:\"{artista_pulito}\""));
    }
    // Il numero di tracce **nella query** e non solo nel setaccio: restringe già
    // dalla parte del servizio, quindi i venticinque risultati che tornano sono
    // venticinque candidati veri invece di venticinque ristampe da scartare.
    if tracce > 0 {
        query.push_str(&format!(" AND tracks:{tracce}"));
    }

    let url = format!(
        "{BASE}/release?query={}&limit=10&fmt=json",
        percento(&query)
    );
    let corpo = fornitori
        .json(
            &fornitori.musicbrainz,
            "mb-ricerca-pubblicazione",
            &query.to_lowercase(),
            &url,
            VIVE_RICERCA_MS,
        )
        .map_err(|err| indisponibile(&err))?;
    let Some(corpo) = corpo else {
        return Ok(Vec::new());
    };

    let mut stub = interpreta_ricerca(&corpo);
    // Il setaccio: un disco con un numero di tracce diverso non verrà mai
    // applicato, quindi leggerlo per intero è una richiesta buttata. Chi non
    // dichiara il totale passa — un dato che manca non deve escludere niente.
    stub.retain(|s| {
        s.punteggio.is_none_or(|p| p >= PUNTEGGIO_MINIMO)
            && s.tracce
                .is_none_or(|quante| quante == tracce || tracce == 0)
    });
    stub.truncate(LETTURE_MASSIME);

    let mut fuori = Vec::with_capacity(stub.len());
    for candidato in stub {
        match pubblicazione(fornitori, &candidato.id) {
            Ok(Some(release)) => fuori.push(release),
            Ok(None) => {}
            // Una lettura caduta a metà elenco non butta via le precedenti: si
            // decide su quel che si è riusciti a leggere, e il resto lo trova
            // la passata dopo.
            Err(err) if fuori.is_empty() => return Err(err),
            Err(_) => break,
        }
    }
    Ok(fuori)
}

/// Una pubblicazione con le sue tracce.
///
/// # Errori
///
/// `metadata.musicbrainzUnavailable` quando il servizio non risponde.
pub fn pubblicazione(fornitori: &Fornitori, id: &str) -> Result<Option<RemoteRelease>, AppError> {
    let url = format!(
        "{BASE}/release/{}?inc=recordings+artist-credits+release-groups&fmt=json",
        percento(id)
    );
    let corpo = fornitori
        .json(
            &fornitori.musicbrainz,
            "mb-pubblicazione",
            id,
            &url,
            VIVE_PUBBLICAZIONE_MS,
        )
        .map_err(|err| indisponibile(&err))?;
    Ok(corpo.as_deref().and_then(interpreta_pubblicazione))
}

/// Le registrazioni che potrebbero essere questo brano.
///
/// Cerca a scaglioni e si ferma al primo che risponde: vedi la nota in testa al
/// modulo.
///
/// # Errori
///
/// `metadata.musicbrainzUnavailable` quando il servizio non risponde.
pub fn registrazioni(
    fornitori: &Fornitori,
    titolo: &str,
    artista: &str,
) -> Result<Vec<Candidate>, AppError> {
    for query in scaglioni(titolo, artista) {
        let url = format!(
            "{BASE}/recording?query={}&limit=25&fmt=json",
            percento(&query)
        );
        let corpo = fornitori
            .json(
                &fornitori.musicbrainz,
                "mb-ricerca-registrazione",
                &query.to_lowercase(),
                &url,
                VIVE_RICERCA_MS,
            )
            .map_err(|err| indisponibile(&err))?;
        let Some(corpo) = corpo else { continue };
        let candidati = interpreta_registrazioni(&corpo);
        if !candidati.is_empty() {
            return Ok(candidati);
        }
    }
    Ok(Vec::new())
}

/// Le query da provare, dalla più stretta alla più larga, senza ripetizioni.
///
/// Funzione pura, esposta perché è la parte che decide **quanto** si allarga —
/// cioè quante richieste costa un file taggato male — e si prova senza rete.
#[must_use]
pub fn scaglioni(titolo: &str, artista: &str) -> Vec<String> {
    let grezzo_titolo = termine_lucene(titolo);
    let pulito_titolo = {
        let pulito = termine_lucene(&titolo_da_cercare(titolo));
        if pulito.is_empty() {
            grezzo_titolo.clone()
        } else {
            pulito
        }
    };
    let noto = !artista.trim().is_empty() && artista != aether_domain::album::UNKNOWN_ARTIST;
    let grezzo_artista = if noto {
        termine_lucene(artista)
    } else {
        String::new()
    };
    let pulito_artista = if noto {
        let pulito = termine_lucene(&titolo_da_cercare(artista));
        if pulito.is_empty() {
            grezzo_artista.clone()
        } else {
            pulito
        }
    } else {
        String::new()
    };

    let mut fuori: Vec<String> = Vec::new();
    let mut aggiungi = |query: String| {
        if !query.is_empty() && !fuori.contains(&query) {
            fuori.push(query);
        }
    };
    if !pulito_titolo.is_empty() && !pulito_artista.is_empty() {
        aggiungi(format!(
            "recording:\"{pulito_titolo}\" AND artist:\"{pulito_artista}\""
        ));
    }
    if !grezzo_titolo.is_empty() && !grezzo_artista.is_empty() {
        aggiungi(format!(
            "recording:\"{grezzo_titolo}\" AND artist:\"{grezzo_artista}\""
        ));
    }
    if !pulito_titolo.is_empty() {
        aggiungi(format!("recording:\"{pulito_titolo}\""));
    }
    fuori
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_artista_composito_si_ricompone_con_la_sua_congiunzione() {
        // Con una virgola al posto di « feat. » il confronto non ritroverebbe
        // la forma che sta nei tag, dove la congiunzione è scritta per esteso.
        let credito = serde_json::json!([
            {"name": "Calcutta", "joinphrase": " feat. "},
            {"name": "Tommaso Paradiso"}
        ]);
        assert_eq!(
            artista_composto(Some(&credito)),
            "Calcutta feat. Tommaso Paradiso"
        );
        assert_eq!(artista_composto(None), "");
        assert_eq!(artista_composto(Some(&serde_json::json!([]))), "");
    }

    #[test]
    fn una_data_incompleta_da_comunque_l_anno() {
        // MusicBrainz scrive `1997`, `1997-03` o la data intera a seconda di
        // quanto se ne sa: prenderne i primi quattro caratteri è l'unica forma
        // che le regge tutte e tre.
        assert_eq!(anno(Some("1997-09-29")), Some(1997));
        assert_eq!(anno(Some("1997-03")), Some(1997));
        assert_eq!(anno(Some("1997")), Some(1997));
        assert_eq!(anno(Some("")), None);
        assert_eq!(anno(Some("boh")), None);
        assert_eq!(anno(None), None);
    }

    #[test]
    fn una_ricerca_da_gli_stub_col_conteggio() {
        let corpo = br#"{"releases":[
            {"id":"R1","title":"OK Computer","score":100,"track-count":12,
             "artist-credit":[{"name":"Radiohead"}]},
            {"id":"R2","title":"OK Computer OKNOTOK","score":90,
             "media":[{"track-count":12},{"track-count":11}],
             "artist-credit":[{"name":"Radiohead"}]}
        ]}"#;
        let stub = interpreta_ricerca(corpo);
        assert_eq!(stub.len(), 2);
        assert_eq!(stub.first().and_then(|s| s.tracce), Some(12));
        // Il secondo somma i due dischi: è il totale che il setaccio confronta.
        assert_eq!(stub.get(1).and_then(|s| s.tracce), Some(23));
        assert_eq!(stub.first().map(|s| s.artista.as_str()), Some("Radiohead"));
    }

    #[test]
    fn uno_stub_senza_conteggio_non_si_esclude() {
        // Un dato che manca non deve escludere niente: passa al setaccio e sarà
        // la distanza a dire se è quello giusto.
        let corpo = br#"{"releases":[{"id":"R1","title":"X"}]}"#;
        assert_eq!(
            interpreta_ricerca(corpo).first().and_then(|s| s.tracce),
            None
        );
    }

    #[test]
    fn una_pubblicazione_appiattisce_i_dischi_tenendo_il_numero() {
        // Un doppio album in libreria sta in una cartella sola — `album_folder`
        // risolve `CD1` nella superiore — quindi il gruppo è uno e le tracce
        // sono tutte, ma ognuna sa da che disco viene.
        let corpo = br#"{
            "id":"R1","title":"Il Disco","date":"2001-05-04",
            "artist-credit":[{"name":"Artista"}],
            "release-group":{"id":"RG1","primary-type":"Album"},
            "media":[
              {"position":1,"tracks":[
                {"position":1,"title":"Uno","length":180000,
                 "recording":{"id":"REC1","title":"Uno","length":180000}}
              ]},
              {"position":2,"tracks":[
                {"position":1,"title":"Due","length":200000,
                 "recording":{"id":"REC2","title":"Due"}}
              ]}
            ]}"#;
        let Some(release) = interpreta_pubblicazione(corpo) else {
            panic!("si deve leggere");
        };
        assert_eq!(release.title, "Il Disco");
        assert_eq!(release.artist, "Artista");
        assert_eq!(release.year, Some(2001));
        assert_eq!(release.mb_release_group_id.as_deref(), Some("RG1"));
        assert_eq!(release.tracks.len(), 2);
        let primo = release.tracks.first().expect("prima traccia");
        assert_eq!(primo.disc_number, Some(1));
        assert_eq!(primo.mb_recording_id.as_deref(), Some("REC1"));
        let secondo = release.tracks.get(1).expect("seconda traccia");
        assert_eq!(secondo.disc_number, Some(2));
        assert_eq!(secondo.track_number, Some(1));
    }

    #[test]
    fn senza_data_sulla_ristampa_vale_quella_del_gruppo() {
        // È l'anno in cui il disco è uscito, che è quello che chi guarda una
        // libreria si aspetta di leggere — non quello della ristampa del 2015.
        let corpo = br#"{"id":"R1","title":"X","media":[],
            "release-group":{"id":"RG1","first-release-date":"1997-06-16"}}"#;
        assert_eq!(
            interpreta_pubblicazione(corpo).and_then(|r| r.year),
            Some(1997)
        );
    }

    #[test]
    fn una_pubblicazione_senza_tracce_e_comunque_una_pubblicazione() {
        // Non si restituisce `None`: il conteggio a zero è quel che farà
        // fallire il confronto, e farlo fallire dicendo perché è meglio che far
        // sparire il candidato.
        let corpo = br#"{"id":"R1","title":"X"}"#;
        let Some(release) = interpreta_pubblicazione(corpo) else {
            panic!("si deve leggere");
        };
        assert!(release.tracks.is_empty());
    }

    #[test]
    fn un_corpo_storto_non_fa_cadere_niente() {
        assert!(interpreta_ricerca(b"non json").is_empty());
        assert!(interpreta_ricerca(b"{}").is_empty());
        assert!(interpreta_pubblicazione(b"non json").is_none());
        assert!(interpreta_pubblicazione(b"{}").is_none());
        assert!(interpreta_registrazioni(b"non json").is_empty());
    }

    #[test]
    fn le_registrazioni_scartano_i_punteggi_bassi() {
        let corpo = br#"{"recordings":[
            {"id":"A","title":"Poetica","score":95,"length":297000,
             "artist-credit":[{"name":"Cesare Cremonini"}],
             "releases":[{"id":"R1","title":"Possibili scenari","date":"2017-11-24",
                          "status":"Official","release-group":{"id":"RG1","primary-type":"Album"}}]},
            {"id":"B","title":"Poetica","score":20,"artist-credit":[{"name":"Altro"}]}
        ]}"#;
        let candidati = interpreta_registrazioni(corpo);
        assert_eq!(candidati.len(), 1);
        let primo = candidati.first().expect("un candidato");
        assert_eq!(primo.mb_recording_id.as_deref(), Some("A"));
        assert_eq!(primo.album.as_deref(), Some("Possibili scenari"));
        assert_eq!(primo.year, Some(2017));
        assert_eq!(primo.mb_release_group_id.as_deref(), Some("RG1"));
    }

    #[test]
    fn si_preferisce_l_album_ufficiale_al_singolo_promozionale() {
        // Da qui discende la copertina: prendere la prima metterebbe in
        // libreria l'immagine di un disco che l'utente non ha.
        let corpo = br#"{"recordings":[{"id":"A","title":"X","score":100,
            "artist-credit":[{"name":"Y"}],
            "releases":[
              {"id":"P1","title":"Promo","status":"Promotion",
               "release-group":{"id":"G1","primary-type":"Single"}},
              {"id":"P2","title":"L'Album","status":"Official",
               "release-group":{"id":"G2","primary-type":"Album"}}
            ]}]}"#;
        let candidati = interpreta_registrazioni(corpo);
        assert_eq!(
            candidati.first().and_then(|c| c.album.as_deref()),
            Some("L'Album")
        );
    }

    #[test]
    fn gli_scaglioni_si_allargano_e_non_si_ripetono() {
        let query = scaglioni("Poetica (Official Video)", "Cesare Cremonini");
        assert_eq!(query.len(), 3);
        assert!(
            query.first().is_some_and(|q| q.contains("\"Poetica\"")),
            "il primo scaglione cerca il titolo ripulito: {query:?}"
        );
        assert!(
            query.get(1).is_some_and(|q| q.contains("Official Video")),
            "il secondo prova i termini grezzi: {query:?}"
        );
        assert!(
            query.get(2).is_some_and(|q| !q.contains("artist:")),
            "l'ultimo lascia cadere l'interprete: {query:?}"
        );

        // Un titolo già pulito non produce due scaglioni identici: sarebbe una
        // richiesta pagata per avere due volte la stessa risposta.
        let semplice = scaglioni("Poetica", "Cesare Cremonini");
        assert_eq!(semplice.len(), 2);
    }

    #[test]
    fn senza_interprete_resta_solo_il_titolo() {
        let query = scaglioni("Poetica", aether_domain::album::UNKNOWN_ARTIST);
        assert_eq!(query.len(), 1);
        assert!(query.first().is_some_and(|q| !q.contains("artist:")));
    }

    #[test]
    fn un_titolo_vuoto_non_produce_nessuna_query() {
        // Cercare la stringa vuota su MusicBrainz restituisce il catalogo.
        assert!(scaglioni("   ", "Artista").is_empty());
    }
}
