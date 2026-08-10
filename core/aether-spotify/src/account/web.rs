//! I lettori della Web API, uno per elenco dell'account.
//!
//! # Perché seguono `next` invece di contare le pagine
//!
//! Perché di stili di paginazione ce ne sono due e cambiano da un endpoint
//! all'altro: `/me/tracks` e `/me/playlists` vanno a `offset`, `/me/following`
//! va a cursore (`after=<ultimo id>`). Scriverli tutti e due vorrebbe dire due
//! cicli, e sbagliarne uno in un modo che si manifesta come «l'importazione ha
//! preso solo i primi cinquanta artisti» — cioè in silenzio.
//!
//! L'indirizzo della pagina dopo Spotify lo dice sempre lui, in `next`, ed è
//! già completo. Seguirlo è **un** ciclo che funziona per tutti e due, e vale
//! anche il giorno in cui uno dei due endpoint cambia stile.
//!
//! # Il rinominare del marzo 2026
//!
//! Nella migrazione le playlist hanno rinominato `tracks` → `items` e, dentro
//! ogni voce, `track` → `item`. La guida però dichiara il rinominare per gli
//! oggetti playlist e tace sugli altri elenchi — `/me/tracks` e `/me/albums`
//! restituiscono ancora voci `{added_at, track}` o forse no.
//!
//! Qui si leggono **tutti e due i nomi**, con un `or_else`. Non è tolleranza
//! decorativa: la differenza fra le due letture è una riga, e sbagliare quale
//! sia quella giusta significa importare zero brani da un account pieno, senza
//! nessun errore da nessuna parte — l'elenco arriva, è solo vuoto.
//!
//! # Quel che Spotify non dà più
//!
//! Il contenuto di una playlist si legge **solo** se l'utente la possiede o vi
//! collabora. Le playlist che segue e basta arrivano con nome e totale, e
//! niente brani: non è un guasto ed è dichiarato nel rapporto, perché
//! «importate 40 playlist su 62» è un'informazione e il silenzio no.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify::SpotifyTrack;
use aether_domain::spotify_account::{
    AlbumSpotify, ArtistaSpotify, AscoltoSpotify, PlaylistSpotify,
};
use aether_net::{Corpo, Metodo, Rete, Richiesta, Risposta};
use serde_json::Value;

/// La radice della Web API.
const API: &str = "https://api.spotify.com/v1";

/// Quante voci per pagina.
///
/// Il massimo che gli elenchi dell'account accettano. La ricerca è scesa a 10
/// nel febbraio 2026, ma questa non è la ricerca.
const PER_PAGINA: usize = 50;

/// Quante pagine al massimo si seguono, per elenco.
///
/// Un `next` che punta a se stesso — o che non finisce mai — bloccherebbe
/// l'importazione dentro un ciclo senza fine, e la finestra mostrerebbe una
/// barra che avanza per sempre.
///
/// Duecento pagine sono diecimila voci, che è **esattamente** il tetto dei
/// «Brani che ti piacciono» di Spotify: il numero non è tondo per caso, ed è
/// scelto perché nessun account vero possa toccarlo. Se lo tocca, chi ha letto
/// lo dichiara — vedi [`Letto::troncato`].
const PAGINE_MASSIME: usize = 200;

/// Quanti tentativi per richiesta.
const TENTATIVI: u32 = 3;

/// Le pagine grezze di un elenco, prima di sapere cosa contengono.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Pagine {
    /// Le voci, nell'ordine in cui Spotify le ha date.
    voci: Vec<Value>,
    /// Quante ne dichiara Spotify, quando lo dichiara.
    totale: Option<u32>,
    /// Si è smesso a [`PAGINE_MASSIME`] senza aver finito.
    troncato: bool,
}

/// Un elenco letto, e se è arrivato tutto.
///
/// # Perché il troncamento viaggia col suo elenco
///
/// Perché un elenco arrivato a metà non ha nessun altro modo di dirlo. Le
/// playlist ce l'hanno — [`PlaylistSpotify::dichiarati`] porta il totale
/// dichiarato, e la differenza coi brani letti è il numero esatto che manca —
/// ma i «Brani che ti piacciono», gli album e gli artisti no: arrivano, sono
/// meno di quelli veri, e senza questo campo la differenza la scoprirebbe
/// nessuno.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Letto<T> {
    /// Quel che si è letto.
    pub voci: Vec<T>,
    /// Si è smesso a [`PAGINE_MASSIME`] senza aver finito.
    pub troncato: bool,
}

/// Il lettore autenticato.
///
/// Tiene l'intestazione già composta invece del token nudo: un `Bearer ` in meno
/// da ricordarsi a ogni chiamata, e soprattutto un posto solo da guardare per
/// sapere dove finisce l'access token.
#[derive(Debug)]
pub struct Cliente<'a> {
    rete: &'a Rete,
    autorizzazione: String,
}

impl<'a> Cliente<'a> {
    /// Un lettore che parla a nome di questo access token.
    #[must_use]
    pub fn nuovo(rete: &'a Rete, access_token: &str) -> Self {
        Self {
            rete,
            autorizzazione: format!("Bearer {access_token}"),
        }
    }

    /// Il profilo: chi è, e se ha Premium.
    ///
    /// # Errori
    ///
    /// `spotify.accountAuthExpired`, `spotify.accountForbidden`, `net.*`.
    pub fn profilo(&self) -> Result<Profilo, AppError> {
        let corpo = self.chiedi(&format!("{API}/me"))?;
        Ok(Profilo {
            id: testo(&corpo, "id"),
            nome: testo(&corpo, "display_name"),
            premium: testo(&corpo, "product").map(|p| p == "premium"),
        })
    }

    /// I «Brani che ti piacciono».
    ///
    /// # Errori
    ///
    /// Come [`Self::profilo`].
    pub fn preferiti(&self) -> Result<Letto<SpotifyTrack>, AppError> {
        let pagine = self.elenco(&format!("{API}/me/tracks?limit={PER_PAGINA}"), None)?;
        Ok(Letto {
            voci: pagine.voci.iter().filter_map(voce_a_brano).collect(),
            troncato: pagine.troncato,
        })
    }

    /// Gli album salvati, con la prima pagina delle loro tracce.
    ///
    /// Le tracce **non si paginano**: costerebbe una richiesta in più per ogni
    /// album di più di cinquanta brani, e servono a una cosa sola — sapere quali
    /// brani di un album salvato mancano dal disco. Un cofanetto di cui si
    /// scoprono i primi cinquanta è comunque meglio che nessuno, e non cancella
    /// niente.
    ///
    /// # Errori
    ///
    /// Come [`Self::profilo`].
    pub fn album(&self) -> Result<Letto<AlbumSpotify>, AppError> {
        let pagine = self.elenco(&format!("{API}/me/albums?limit={PER_PAGINA}"), None)?;
        Ok(Letto {
            voci: pagine
                .voci
                .iter()
                .filter_map(|voce| {
                    let album = voce.get("album").filter(|v| !v.is_null()).unwrap_or(voce);
                    voce_a_album(album)
                })
                .collect(),
            troncato: pagine.troncato,
        })
    }

    /// Le playlist dell'utente, **senza** i brani.
    ///
    /// I brani si prendono una playlist alla volta con
    /// [`Self::brani_di_playlist`], perché ognuna è una paginazione a sé e
    /// perché è l'unico modo di far avanzare una barra che dica a che punto è.
    ///
    /// # Errori
    ///
    /// Come [`Self::profilo`].
    pub fn playlist(&self) -> Result<Letto<PlaylistSpotify>, AppError> {
        let pagine = self.elenco(&format!("{API}/me/playlists?limit={PER_PAGINA}"), None)?;
        Ok(Letto {
            voci: pagine.voci.iter().filter_map(voce_a_playlist).collect(),
            troncato: pagine.troncato,
        })
    }

    /// I brani di una playlist.
    ///
    /// Le puntate di podcast si contano e non entrano, come fa il lettore
    /// dell'archivio.
    ///
    /// # Errori
    ///
    /// `spotify.accountForbidden` quando la playlist non è dell'utente — dal
    /// marzo 2026 il contenuto si legge solo di quelle possedute o
    /// collaborative. Chi chiama lo tratta come «questa no», non come «basta».
    pub fn brani_di_playlist(&self, id: &str) -> Result<Contenuto, AppError> {
        let pagine = self.elenco(
            &format!("{API}/playlists/{id}/items?limit={PER_PAGINA}"),
            None,
        )?;
        let mut brani = Vec::with_capacity(pagine.voci.len());
        let mut podcast = 0_usize;
        for voce in &pagine.voci {
            match voce_a_brano(voce) {
                Some(brano) => brani.push(brano),
                None if e_podcast(voce) => podcast = podcast.saturating_add(1),
                None => {}
            }
        }
        Ok(Contenuto {
            brani,
            podcast,
            voci: pagine.voci.len(),
        })
    }

    /// Gli artisti seguiti.
    ///
    /// L'unico elenco a cursore: la pagina sta dentro `artists`, non alla
    /// radice, e la pagina dopo si chiede con `after` invece che con `offset`.
    /// Seguire `next` rende la differenza invisibile.
    ///
    /// # Errori
    ///
    /// Come [`Self::profilo`].
    pub fn artisti_seguiti(&self) -> Result<Letto<ArtistaSpotify>, AppError> {
        let pagine = self.elenco(
            &format!("{API}/me/following?type=artist&limit={PER_PAGINA}"),
            Some("artists"),
        )?;
        Ok(Letto {
            voci: pagine
                .voci
                .iter()
                .filter_map(|voce| {
                    Some(ArtistaSpotify {
                        nome: testo(voce, "name")?,
                        spotify_id: testo(voce, "id"),
                    })
                })
                .collect(),
            troncato: pagine.troncato,
        })
    }

    /// Gli ultimi ascolti che Spotify ricorda.
    ///
    /// # Cinquanta, e non una di più
    ///
    /// L'endpoint non pagina all'indietro oltre quel numero, ed è tutta la
    /// ragione per cui l'archivio esiste come seconda via:
    /// `Provenienza::Api.ha_cronologia_completa()` dice `false` proprio qui
    /// perché il rapporto possa scrivere «cinquanta ascolti» senza che sembri un
    /// guasto.
    ///
    /// # Due cose che Spotify non dice, e come si trattano
    ///
    /// **Quanto è stato ascoltato**: non c'è nessun campo. Si prende la durata
    /// del brano, perché in questo elenco Spotify mette solo ciò che è stato
    /// suonato abbastanza — e perché `AscoltoSpotify` un numero lo vuole. È una
    /// stima, e sta scritto qui che lo è.
    ///
    /// **Se `played_at` è l'inizio o la fine**: la documentazione non lo dice, e
    /// i dati veri sono stati visti fare tutti e due (issue 1083 di
    /// `spotify/web-api`, mai chiusa). Qui si legge come la **fine**, che è la
    /// stessa lettura dell'archivio — dove `ts` è documentato come «when the
    /// track stopped playing». La proprietà che si compra scegliendo così non è
    /// la precisione: è che le due vie datino **allo stesso modo** lo stesso
    /// ascolto. `play_history` deduplica su `(track_id, played_at)`, quindi due
    /// letture discordi trasformerebbero un ascolto solo in due — che è un
    /// errore peggiore di qualche minuto di scarto.
    ///
    /// # Errori
    ///
    /// Come [`Self::profilo`].
    pub fn ascolti_recenti(&self) -> Result<Vec<AscoltoSpotify>, AppError> {
        let pagine = self.elenco(
            &format!("{API}/me/player/recently-played?limit={PER_PAGINA}"),
            None,
        )?;
        Ok(pagine
            .voci
            .iter()
            .filter_map(|voce| {
                let brano = voce_a_brano(voce)?;
                Some(AscoltoSpotify {
                    finito_ms: crate::account::tempo::istante_ms(&testo(voce, "played_at")?)?,
                    ms_ascoltati: brano.duration_ms.unwrap_or(0),
                    brano,
                })
            })
            .collect())
    }

    /// Legge un elenco paginato fino in fondo.
    ///
    /// `dentro` è il campo in cui sta la pagina, quando non sta alla radice:
    /// `/me/following` la incarta in `artists`.
    fn elenco(&self, primo: &str, dentro: Option<&str>) -> Result<Pagine, AppError> {
        let mut esito = Pagine::default();
        let mut prossimo = Some(primo.to_owned());
        let mut visti = 0_usize;

        while let Some(url) = prossimo.take() {
            if visti >= PAGINE_MASSIME {
                esito.troncato = true;
                break;
            }
            visti = visti.saturating_add(1);

            let corpo = self.chiedi(&url)?;
            prossimo = assorbi(&mut esito, &corpo, dentro, &url);
        }
        Ok(esito)
    }

    /// Una richiesta autenticata, con i tentativi che hanno senso.
    fn chiedi(&self, url: &str) -> Result<Value, AppError> {
        self.rete.ritenta(TENTATIVI, || self.una_volta(url))
    }

    fn una_volta(&self, url: &str) -> Result<Value, AppError> {
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url,
            intestazioni: &[("Authorization", self.autorizzazione.as_str())],
            corpo: Corpo::Niente,
        })?;
        if risposta.e_andata() {
            return Ok(serde_json::from_slice(&risposta.corpo).unwrap_or_default());
        }
        Err(interpreta_guasto(self.rete, &risposta, url))
    }
}

/// Prende le voci di una pagina e dice dov'è la prossima.
///
/// Separata dal ciclo perché è la parte che può sbagliare in silenzio, ed è
/// l'unica di tutto il lettore che si possa provare senza una rete: un `next`
/// letto nel posto sbagliato non dà nessun errore, dà **cinquanta voci su
/// duemila** e sembra un account piccolo.
///
/// `dentro` è il campo in cui sta la pagina quando non sta alla radice.
fn assorbi(
    esito: &mut Pagine,
    corpo: &Value,
    dentro: Option<&str>,
    chiesto: &str,
) -> Option<String> {
    let pagina = match dentro {
        Some(campo) => corpo.get(campo).unwrap_or(&Value::Null),
        None => corpo,
    };
    // Il totale della prima pagina: le successive lo ripetono, e sovrascriverlo
    // costerebbe niente ma nasconderebbe una risposta incoerente.
    if esito.totale.is_none() {
        esito.totale = pagina
            .get("total")
            .and_then(Value::as_u64)
            .and_then(|t| u32::try_from(t).ok());
    }
    if let Some(voci) = pagina.get("items").and_then(Value::as_array) {
        esito.voci.extend(voci.iter().cloned());
    }
    // `next` è già l'indirizzo completo della pagina dopo. Uguale a quello
    // appena chiesto vorrebbe dire un ciclo: si smette.
    pagina
        .get("next")
        .and_then(Value::as_str)
        .filter(|dopo| !dopo.is_empty() && *dopo != chiesto)
        .map(ToOwned::to_owned)
}

/// Il contenuto di una playlist, con di che accorgersi se è arrivato tutto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contenuto {
    /// I brani, nell'ordine in cui stanno su Spotify.
    pub brani: Vec<SpotifyTrack>,
    /// Quante voci erano puntate di podcast.
    pub podcast: usize,
    /// Quante **voci** sono arrivate in tutto, musica o no.
    ///
    /// # Perché non basta `brani.len()`
    ///
    /// Perché `PlaylistSpotify::dichiarati` è il totale che Spotify dichiara, e
    /// quel totale conta **tutte** le voci: i podcast, e i brani spariti dal
    /// catalogo che arrivano come `null`. Confrontarlo con i brani tenuti
    /// direbbe «mancano 3 brani su 51» a una playlist di 50 canzoni e un
    /// podcast, arrivata intera.
    ///
    /// Non sarebbe un avviso di troppo e basta: `import_spotify::prepara_playlist`
    /// **rifiuta di sostituire** una playlist che esiste già quando l'elenco è
    /// arrivato monco — è la guardia che impedisce a trecento voci buone di
    /// essere rimpiazzate da duecento. Quella playlist diventerebbe
    /// impossibile da reimportare, per sempre, a causa di un podcast.
    ///
    /// Con questo numero il confronto torna a essere fra cose confrontabili:
    /// voci ricevute contro voci dichiarate.
    pub voci: usize,
}

impl Contenuto {
    /// Il totale da dichiarare, dato quello che Spotify ha detto.
    ///
    /// Riporta lo scarto — le voci che non sono arrivate — sul conto dei brani,
    /// così `PlaylistSpotify::troncatura()` confronta due numeri della stessa
    /// specie. Torna `None` quando è arrivato tutto: nessuna troncatura da
    /// dichiarare.
    #[must_use]
    pub fn dichiarati(&self, totale_spotify: Option<u32>) -> Option<u32> {
        let totale = totale_spotify?;
        let ricevute = u32::try_from(self.voci).unwrap_or(u32::MAX);
        let mancanti = totale.saturating_sub(ricevute);
        let tenuti = u32::try_from(self.brani.len()).unwrap_or(u32::MAX);
        Some(tenuti.saturating_add(mancanti))
    }
}

/// Chi è l'account, per il rapporto e per l'avviso.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profilo {
    /// L'identificativo dell'utente: la chiave di `spotify_account`.
    pub id: Option<String>,
    /// Il nome visualizzato.
    pub nome: Option<String>,
    /// Ha Premium?
    ///
    /// `None` quando Spotify non l'ha detto. Serve all'avviso che il piano
    /// chiede di dare **prima** del collegamento e non dopo il primo guasto:
    /// dal febbraio 2026 un'applicazione in Development Mode smette di
    /// funzionare quando il suo proprietario perde l'abbonamento, e Spotify non
    /// manda nessun avviso.
    pub premium: Option<bool>,
}

/// Trasforma una risposta di errore nel codice che le corrisponde.
///
/// I tre casi che meritano un nome sono i tre in cui l'utente può fare qualcosa
/// di diverso: ricollegarsi, sistemare la dashboard, o aspettare domani.
fn interpreta_guasto(rete: &Rete, risposta: &Risposta, url: &str) -> AppError {
    match risposta.stato {
        401 => AppError::new(ErrorCode::SpotifyAccountAuthExpired),
        403 => AppError::new(ErrorCode::SpotifyAccountForbidden)
            .with_cause(accorcia(&risposta.testo())),
        // Un `429` normale è un limite di frequenza e passa aspettando; uno con
        // `QUOTA_EXCEEDED` è la quota del giorno ed è finita. Sono lo stesso
        // stato HTTP e due situazioni opposte, e `stato_a_errore` — che non
        // legge il corpo — le farebbe ritentare tutte e due.
        429 if e_quota_esaurita(risposta) => AppError::new(ErrorCode::SpotifyQuotaExceeded {
            retry_after_ms: risposta.riprova_fra_ms,
        }),
        _ => rete.stato_a_errore(risposta, url),
    }
}

/// Il `429` è una quota esaurita e non un limite di frequenza?
fn e_quota_esaurita(risposta: &Risposta) -> bool {
    let corpo: Value = serde_json::from_slice(&risposta.corpo).unwrap_or_default();
    // Dal luglio 2026 il campo sta dentro `error`; si guarda anche alla radice
    // perché la forma vecchia gira ancora in giro.
    let motivo = corpo
        .get("error")
        .and_then(|e| e.get("reason"))
        .or_else(|| corpo.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    motivo == "QUOTA_EXCEEDED"
}

/// I primi caratteri di un corpo di errore, per la causa.
fn accorcia(testo: &str) -> String {
    testo.chars().take(300).collect()
}

/// L'oggetto brano dentro una voce di elenco.
///
/// `item` e `track`: vedi la nota sul rinominare in testa al modulo. Quando non
/// c'è né l'uno né l'altro la voce **è** il brano — è la forma delle tracce
/// dentro un album.
fn brano_di(voce: &Value) -> &Value {
    voce.get("item")
        .filter(|v| !v.is_null())
        .or_else(|| voce.get("track").filter(|v| !v.is_null()))
        .unwrap_or(voce)
}

/// Una voce di elenco è una puntata di podcast?
fn e_podcast(voce: &Value) -> bool {
    testo(brano_di(voce), "type").is_some_and(|t| t == "episode")
}

/// Da una voce di elenco a un brano di dominio.
///
/// `None` per i podcast e per le voci senza titolo — un brano senza titolo non
/// è un brano, ed è l'unico campo che ogni forma di risposta porta sempre.
fn voce_a_brano(voce: &Value) -> Option<SpotifyTrack> {
    let brano = brano_di(voce);
    if testo(brano, "type").is_some_and(|t| t == "episode") {
        return None;
    }
    let album = brano.get("album").filter(|v| !v.is_null());
    Some(SpotifyTrack {
        title: testo(brano, "name")?,
        artist: nomi(brano.get("artists")),
        album: album.and_then(|a| testo(a, "name")),
        album_artist: album.and_then(|a| nomi(a.get("artists"))),
        disc_number: numero(brano, "disc_number"),
        track_number: numero(brano, "track_number"),
        year: album.and_then(anno),
        duration_ms: brano.get("duration_ms").and_then(Value::as_u64),
        // Tolto nel febbraio 2026 e **rimesso** a marzo. È il gradino zero
        // dell'abbinamento — stessa registrazione senza guardare i nomi — e
        // questa è la prima via che glielo fa arrivare davvero: il lettore
        // keyless l'ISRC non lo vede più da nessun livello.
        isrc: brano
            .get("external_ids")
            .and_then(|e| testo(e, "isrc"))
            .map(|codice| codice.to_uppercase()),
        // I brani locali che l'utente aveva aggiunto a Spotify non hanno
        // identificativo: `is_local` è vero e `id` è nullo. Restano brani, e la
        // scala d'abbinamento li ritrova per artista e titolo — che è quel che
        // sa fare, e sono anche quelli con più probabilità di essere già sul
        // disco.
        spotify_track_id: testo(brano, "id"),
        spotify_album_id: album.and_then(|a| testo(a, "id")),
        cover_url: album.and_then(copertina),
    })
}

/// Da una voce di `/me/albums` a un album di dominio.
fn voce_a_album(album: &Value) -> Option<AlbumSpotify> {
    let titolo = testo(album, "name")?;
    let artista = nomi(album.get("artists"));
    // `items` dopo il rinominare, `tracks` prima. Vedi la nota in testa.
    let dentro = album
        .get("items")
        .filter(|v| !v.is_null())
        .or_else(|| album.get("tracks").filter(|v| !v.is_null()));
    let brani = dentro
        .and_then(|t| t.get("items"))
        .and_then(Value::as_array)
        .map(|voci| {
            voci.iter()
                .filter_map(|voce| {
                    // Le tracce dentro un album sono «semplificate»: non
                    // ripetono l'album da cui vengono, e senza reinnestarlo la
                    // chiave del brano nascerebbe con l'album vuoto — cioè non
                    // combacerebbe con nessuna riga di libreria.
                    let mut brano = voce_a_brano(voce)?;
                    brano.album = Some(titolo.clone());
                    brano.album_artist = artista.clone();
                    brano.spotify_album_id = testo(album, "id");
                    brano.year = anno(album);
                    brano.cover_url = copertina(album);
                    Some(brano)
                })
                .collect()
        })
        .unwrap_or_default();

    Some(AlbumSpotify {
        titolo,
        artista,
        spotify_id: testo(album, "id"),
        brani,
    })
}

/// Da una voce di `/me/playlists` a una playlist di dominio, senza brani.
fn voce_a_playlist(voce: &Value) -> Option<PlaylistSpotify> {
    // `items.total` dopo il rinominare del marzo 2026, `tracks.total` prima.
    let quanti = voce
        .get("items")
        .or_else(|| voce.get("tracks"))
        .and_then(|t| t.get("total"))
        .and_then(Value::as_u64)
        .and_then(|t| u32::try_from(t).ok());
    Some(PlaylistSpotify {
        nome: testo(voce, "name")?,
        descrizione: testo(voce, "description"),
        spotify_id: testo(voce, "id"),
        brani: Vec::new(),
        dichiarati: quanti,
    })
}

/// I nomi di un elenco di artisti, uniti da virgole come li dà Spotify.
fn nomi(artisti: Option<&Value>) -> Option<String> {
    let elenco: Vec<String> = artisti?
        .as_array()?
        .iter()
        .filter_map(|a| testo(a, "name"))
        .collect();
    (!elenco.is_empty()).then(|| elenco.join(", "))
}

/// L'anno dalla data di pubblicazione, che può essere `2001` o `2001-03-15`.
fn anno(album: &Value) -> Option<i32> {
    let data = testo(album, "release_date")?;
    data.get(..4)?.parse().ok()
}

/// La copertina più grande.
///
/// Spotify le dà già dalla più grande alla più piccola, e prendere la prima
/// basterebbe. Si sceglie per larghezza lo stesso perché «basterebbe» è
/// un'osservazione sul comportamento di oggi, e una copertina da sessantaquattro
/// pixel messa in una scheda album non è un guasto che qualcuno segnala: è una
/// cosa che sembra brutta e basta.
fn copertina(album: &Value) -> Option<String> {
    album
        .get("images")?
        .as_array()?
        .iter()
        .max_by_key(|i| i.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|i| testo(i, "url"))
}

/// Il campo, se c'è ed è una stringa non vuota.
fn testo(voce: &Value, nome: &str) -> Option<String> {
    voce.get(nome)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
}

/// Il campo, se c'è ed è un numero che ci sta in un `u32`.
fn numero(voce: &Value, nome: &str) -> Option<u32> {
    voce.get(nome)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

#[cfg(test)]
mod prove {
    use super::*;
    use serde_json::json;

    fn brano_completo() -> Value {
        json!({
            "name": "Song 2",
            "type": "track",
            "id": "abc123",
            "disc_number": 1,
            "track_number": 6,
            "duration_ms": 122_000,
            "external_ids": {"isrc": "gbaye9700426"},
            "artists": [{"name": "Blur", "id": "art1"}],
            "album": {
                "name": "Blur",
                "id": "alb1",
                "release_date": "1997-02-10",
                "artists": [{"name": "Blur"}],
                "images": [
                    {"url": "https://i/piccola.jpg", "width": 64},
                    {"url": "https://i/grande.jpg", "width": 640}
                ]
            }
        })
    }

    #[test]
    fn un_brano_si_legge_in_tutti_i_suoi_campi() {
        let brano = voce_a_brano(&json!({"added_at": "2020-01-01", "track": brano_completo()}))
            .expect("un brano");
        assert_eq!(brano.title, "Song 2");
        assert_eq!(brano.artist.as_deref(), Some("Blur"));
        assert_eq!(brano.album.as_deref(), Some("Blur"));
        assert_eq!(brano.album_artist.as_deref(), Some("Blur"));
        assert_eq!(brano.track_number, Some(6));
        assert_eq!(brano.disc_number, Some(1));
        assert_eq!(brano.year, Some(1997));
        assert_eq!(brano.duration_ms, Some(122_000));
        assert_eq!(brano.spotify_track_id.as_deref(), Some("abc123"));
        assert_eq!(brano.spotify_album_id.as_deref(), Some("alb1"));
    }

    #[test]
    fn lisrc_arriva_e_arriva_maiuscolo() {
        // Il gradino zero dell'abbinamento, e la prima via che glielo fa
        // arrivare: il lettore keyless quel campo non lo vede da nessun livello.
        // Maiuscolo perché è così che sta nei tag dei file, e il confronto è fra
        // stringhe.
        let brano = voce_a_brano(&brano_completo()).expect("un brano");
        assert_eq!(brano.isrc.as_deref(), Some("GBAYE9700426"));
    }

    #[test]
    fn item_e_track_si_leggono_tutti_e_due() {
        // Il rinominare del marzo 2026: le playlist dicono `item`, gli altri
        // elenchi forse ancora `track`. Sbagliare quale sia il nome giusto vuol
        // dire importare zero brani da un account pieno, senza nessun errore.
        let con_item = voce_a_brano(&json!({"item": brano_completo()})).expect("con item");
        let con_track = voce_a_brano(&json!({"track": brano_completo()})).expect("con track");
        let nudo = voce_a_brano(&brano_completo()).expect("nudo");
        assert_eq!(con_item, con_track);
        assert_eq!(con_item, nudo);
    }

    #[test]
    fn una_puntata_di_podcast_non_e_un_brano() {
        let voce = json!({"item": {"type": "episode", "name": "Una puntata", "id": "ep1"}});
        assert_eq!(voce_a_brano(&voce), None);
        assert!(e_podcast(&voce));
    }

    #[test]
    fn un_brano_locale_resta_un_brano() {
        // Non ha identificativo — `is_local` è vero e `id` è nullo — ed è quello
        // con più probabilità di essere già sul disco: la scala d'abbinamento lo
        // ritrova per artista e titolo.
        let voce = json!({"track": {
            "name": "Un mio file", "type": "track", "is_local": true, "id": null,
            "artists": [{"name": "Io"}],
            "album": {"name": "Casa", "id": null, "artists": [{"name": "Io"}]}
        }});
        let brano = voce_a_brano(&voce).expect("un brano locale è un brano");
        assert_eq!(brano.title, "Un mio file");
        assert_eq!(brano.spotify_track_id, None);
    }

    #[test]
    fn i_nomi_degli_artisti_si_uniscono_come_li_da_spotify() {
        let voce = json!({"name": "Feat", "artists": [{"name": "Uno"}, {"name": "Due"}]});
        let brano = voce_a_brano(&voce).expect("un brano");
        assert_eq!(brano.artist.as_deref(), Some("Uno, Due"));
    }

    #[test]
    fn la_copertina_e_la_piu_grande() {
        let brano = voce_a_brano(&brano_completo()).expect("un brano");
        assert_eq!(brano.cover_url.as_deref(), Some("https://i/grande.jpg"));
    }

    #[test]
    fn un_album_reinnesta_se_stesso_nelle_proprie_tracce() {
        // Le tracce dentro un album sono «semplificate» e non ripetono l'album:
        // senza reinnestarlo la chiave del brano nascerebbe con l'album vuoto,
        // e non combacerebbe con nessuna riga di libreria.
        let album = json!({
            "name": "Demon Days", "id": "alb9", "release_date": "2005",
            "artists": [{"name": "Gorillaz"}],
            "images": [{"url": "https://i/dd.jpg", "width": 640}],
            "tracks": {"items": [
                {"name": "Feel Good Inc", "type": "track", "id": "t1", "track_number": 6,
                 "duration_ms": 222_000, "artists": [{"name": "Gorillaz"}]}
            ], "total": 15}
        });
        let letto = voce_a_album(&album).expect("un album");
        assert_eq!(letto.titolo, "Demon Days");
        assert_eq!(letto.spotify_id.as_deref(), Some("alb9"));
        let brano = letto.brani.first().expect("una traccia");
        assert_eq!(brano.album.as_deref(), Some("Demon Days"));
        assert_eq!(brano.album_artist.as_deref(), Some("Gorillaz"));
        assert_eq!(brano.spotify_album_id.as_deref(), Some("alb9"));
        assert_eq!(brano.year, Some(2005));
        assert_eq!(brano.cover_url.as_deref(), Some("https://i/dd.jpg"));
    }

    #[test]
    fn un_album_con_le_tracce_sotto_items_si_legge_uguale() {
        let vecchio = json!({"name": "A", "id": "x", "artists": [{"name": "B"}],
                             "tracks": {"items": [{"name": "T", "type": "track"}]}});
        let nuovo = json!({"name": "A", "id": "x", "artists": [{"name": "B"}],
                           "items":  {"items": [{"name": "T", "type": "track"}]}});
        assert_eq!(voce_a_album(&vecchio), voce_a_album(&nuovo));
    }

    #[test]
    fn una_playlist_porta_il_totale_dichiarato() {
        // È l'unico modo di accorgersi che un elenco è arrivato monco, e
        // `prepara_playlist` rifiuta di sostituire quando lo è.
        let vecchia = json!({"name": "Corsa", "id": "p1", "tracks": {"total": 142}});
        let nuova = json!({"name": "Corsa", "id": "p1", "items": {"total": 142}});
        for voce in [&vecchia, &nuova] {
            let letta = voce_a_playlist(voce).expect("una playlist");
            assert_eq!(letta.nome, "Corsa");
            assert_eq!(letta.spotify_id.as_deref(), Some("p1"));
            assert_eq!(letta.dichiarati, Some(142));
        }
    }

    #[test]
    fn una_playlist_letta_a_meta_lo_dichiara() {
        let mut letta =
            voce_a_playlist(&json!({"name": "Lunga", "id": "p", "items": {"total": 300}}))
                .expect("una playlist");
        letta.brani = vec![SpotifyTrack::default(); 142];
        assert_eq!(letta.troncatura(), Some((142, 300)));
    }

    #[test]
    fn un_podcast_in_playlist_non_la_fa_sembrare_monca() {
        // Il difetto che questo conto impedisce, e non è un avviso di troppo:
        // `prepara_playlist` **rifiuta di sostituire** una playlist che esiste
        // già quando l'elenco è arrivato monco. Una playlist di 50 canzoni più
        // un podcast risulterebbe «50 su 51» a ogni lettura, e diventerebbe
        // impossibile da reimportare per sempre.
        let contenuto = Contenuto {
            brani: vec![SpotifyTrack::default(); 50],
            podcast: 1,
            voci: 51,
        };
        let dichiarati = contenuto.dichiarati(Some(51));
        assert_eq!(dichiarati, Some(50), "51 voci su 51: non manca niente");

        let playlist = PlaylistSpotify {
            brani: contenuto.brani.clone(),
            dichiarati,
            ..PlaylistSpotify::default()
        };
        assert_eq!(playlist.troncatura(), None);
    }

    #[test]
    fn un_elenco_arrivato_a_meta_resta_monco() {
        // L'altra metà: la guardia deve continuare a scattare quando serve.
        // Trecento voci dichiarate, duecento arrivate di cui 195 musica.
        let contenuto = Contenuto {
            brani: vec![SpotifyTrack::default(); 195],
            podcast: 5,
            voci: 200,
        };
        let playlist = PlaylistSpotify {
            brani: contenuto.brani.clone(),
            dichiarati: contenuto.dichiarati(Some(300)),
            ..PlaylistSpotify::default()
        };
        assert_eq!(
            playlist.troncatura(),
            Some((195, 295)),
            "i cento che non sono arrivati restano cento"
        );
    }

    #[test]
    fn senza_totale_dichiarato_non_si_inventa_una_troncatura() {
        let contenuto = Contenuto {
            brani: vec![SpotifyTrack::default(); 3],
            podcast: 0,
            voci: 3,
        };
        assert_eq!(contenuto.dichiarati(None), None);
    }

    #[test]
    fn le_pagine_si_seguono_fino_in_fondo() {
        let mut esito = Pagine::default();
        let prima = json!({
            "items": [{"name": "A", "type": "track"}],
            "total": 2,
            "next": "https://api.spotify.com/v1/me/tracks?offset=50&limit=50"
        });
        let dopo = assorbi(
            &mut esito,
            &prima,
            None,
            "https://api.spotify.com/v1/me/tracks",
        );
        assert_eq!(
            dopo.as_deref(),
            Some("https://api.spotify.com/v1/me/tracks?offset=50&limit=50")
        );
        assert_eq!(esito.totale, Some(2));

        let ultima = json!({"items": [{"name": "B", "type": "track"}], "total": 2, "next": null});
        let fine = assorbi(&mut esito, &ultima, None, "…");
        assert_eq!(fine, None, "un `next` nullo è la fine");
        assert_eq!(esito.voci.len(), 2, "le pagine si sommano");
    }

    #[test]
    fn la_pagina_a_cursore_sta_dentro_un_campo() {
        // `/me/following` incarta la sua pagina in `artists`, e va a cursore
        // invece che a `offset`. Cercare `items` alla radice darebbe zero
        // artisti senza nessun errore: l'elenco arriva, è solo vuoto.
        let mut esito = Pagine::default();
        let corpo = json!({"artists": {
            "items": [{"name": "Blur", "id": "a1"}],
            "total": 1,
            "cursors": {"after": "a1"},
            "next": "https://api.spotify.com/v1/me/following?type=artist&after=a1"
        }});
        let dopo = assorbi(&mut esito, &corpo, Some("artists"), "…");
        assert_eq!(esito.voci.len(), 1);
        assert_eq!(esito.totale, Some(1));
        assert!(dopo.is_some(), "il cursore è un `next` come gli altri");

        // E cercandola alla radice non si troverebbe: è il difetto che questa
        // prova esiste per prendere.
        let mut sbagliato = Pagine::default();
        assert_eq!(assorbi(&mut sbagliato, &corpo, None, "…"), None);
        assert!(sbagliato.voci.is_empty());
    }

    #[test]
    fn un_next_che_punta_a_se_stesso_non_gira_per_sempre() {
        // Una barra che avanza all'infinito è peggio di un errore: nessuno sa
        // se aspettare.
        let mut esito = Pagine::default();
        let corpo = json!({"items": [], "next": "https://uguale"});
        assert_eq!(assorbi(&mut esito, &corpo, None, "https://uguale"), None);
    }

    #[test]
    fn una_pagina_vuota_non_e_un_guasto() {
        // Chi non ha nessun album salvato ha un elenco vuoto, e non è successo
        // niente di male.
        let mut esito = Pagine::default();
        assert_eq!(assorbi(&mut esito, &json!({}), None, "…"), None);
        assert!(esito.voci.is_empty());
        assert_eq!(esito.totale, None);
    }

    #[test]
    fn una_quota_esaurita_non_e_un_limite_di_frequenza() {
        // Stesso stato HTTP, due situazioni opposte: la prima passa aspettando
        // qualche secondo, la seconda no. `stato_a_errore` non legge il corpo e
        // le farebbe ritentare tutte e due.
        let quota = Risposta {
            stato: 429,
            corpo: br#"{"error":{"status":429,"reason":"QUOTA_EXCEEDED"}}"#.to_vec(),
            riprova_fra_ms: Some(1000),
            posizione: None,
            url_finale: String::new(),
        };
        assert!(e_quota_esaurita(&quota));

        let frequenza = Risposta {
            corpo: br#"{"error":{"status":429,"message":"rate limit"}}"#.to_vec(),
            ..quota.clone()
        };
        assert!(!e_quota_esaurita(&frequenza));

        let rete = Rete::nuova("spotify", std::time::Duration::from_secs(1));
        let err = interpreta_guasto(&rete, &quota, "https://api.spotify.com/v1/me");
        assert_eq!(err.code().kind().code(), "spotify.quotaExceeded");
        assert!(!err.is_retryable(), "una quota finita non passa riprovando");
        assert!(
            interpreta_guasto(&rete, &frequenza, "https://api.spotify.com/v1/me").is_retryable(),
            "un limite di frequenza sì"
        );
    }

    #[test]
    fn i_due_no_di_spotify_hanno_due_risposte_diverse() {
        // 401: ricollegati. 403: c'è da sistemare qualcosa nella dashboard —
        // l'utente non è fra i cinque, o il proprietario ha perso Premium.
        let rete = Rete::nuova("spotify", std::time::Duration::from_secs(1));
        let vuota = Risposta {
            stato: 401,
            corpo: Vec::new(),
            riprova_fra_ms: None,
            posizione: None,
            url_finale: String::new(),
        };
        assert_eq!(
            interpreta_guasto(&rete, &vuota, "u").code().kind().code(),
            "spotify.accountAuthExpired"
        );
        let negato = Risposta {
            stato: 403,
            ..vuota
        };
        let err = interpreta_guasto(&rete, &negato, "u");
        assert_eq!(err.code().kind().code(), "spotify.accountForbidden");
        assert!(
            !err.is_retryable(),
            "riprovare non aggiunge nessuno ai cinque"
        );
    }

    #[test]
    fn un_profilo_dice_se_ha_premium() {
        // È l'avviso che il piano chiede di dare **prima** del collegamento:
        // un'app in Development Mode smette di funzionare quando il proprietario
        // perde l'abbonamento, e Spotify non manda niente.
        let corpo = json!({"id": "utente123", "display_name": "Tizio", "product": "premium"});
        let profilo = Profilo {
            id: testo(&corpo, "id"),
            nome: testo(&corpo, "display_name"),
            premium: testo(&corpo, "product").map(|p| p == "premium"),
        };
        assert_eq!(profilo.id.as_deref(), Some("utente123"));
        assert_eq!(profilo.premium, Some(true));

        let gratis = json!({"id": "x", "product": "free"});
        assert_eq!(
            testo(&gratis, "product").map(|p| p == "premium"),
            Some(false)
        );
        // Non detto è diverso da «no»: senza lo scope il campo non arriva, e
        // dire «non hai Premium» a chi ce l'ha sarebbe un allarme falso.
        assert_eq!(
            testo(&json!({"id": "x"}), "product").map(|p| p == "premium"),
            None
        );
    }
}
