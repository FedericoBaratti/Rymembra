//! Jamendo: musica sotto Creative Commons, che si ascolta e non si tiene.
//!
//! # La differenza che decide tutto il modulo
//!
//! [`Fonte::puo_consegnare`] dice **`false`** per Jamendo, e non è una
//! limitazione tecnica: è quel che c'è scritto nei loro termini. Vietano
//! espressamente la cache e l'accesso fuori linea, e `TERMS.md` § 2 lo riporta.
//! Da qui ogni brano esce [`Disponibilita::SoloAscolto`] — non «di solito», ma
//! **sempre**, perché è `Disponibilita::decidi` a stabilirlo un gradino sotto e
//! questo modulo non ha modo di scavalcarla nemmeno volendo.
//!
//! La conseguenza pratica è che l'indirizzo che si porta a casa è `audio`, il
//! flusso, e mai `audiodownload`. Quel secondo campo esiste nella loro risposta
//! e per certi brani è pieno: leggerlo sarebbe scoprire se il server ce li
//! lascerebbe prendere, che è una domanda diversa da «si può».
//!
//! # Perché sta dietro una feature, e perché è spenta
//!
//! L'API di Jamendo è gratuita **per i soli usi non commerciali**, e i loro
//! termini definiscono l'uso commerciale come «any monetary compensation».
//! Aether si sostiene con le donazioni: se contino o no come compenso è una
//! domanda a cui deve rispondere Jamendo, non questo file, e finché non ha
//! risposto la feature `jamendo` di `Cargo.toml` resta **spenta di serie**.
//!
//! Non è prudenza cerimoniale. Il codice c'è ed è provato; quel che manca è un
//! permesso, e spedire un installer che usa un'API a condizioni che non si
//! sanno sarebbe esattamente il genere di cosa che questo programma è stato
//! riscritto per smettere di fare. `README.md` e `TERMS.md` § 2 dicono a chi va
//! scritto — `licensing@jamendo.com` — e il giorno della risposta questa
//! feature si accende con una riga.
//!
//! # Il `client_id`, e perché non è un segreto
//!
//! Perché non lo è: Jamendo lo consegna a chiunque apra un account
//! sviluppatore, viaggia in chiaro dentro ogni indirizzo, e non autorizza
//! niente che riguardi l'utente. Sta quindi in `settings` come le altre
//! preferenze e non nel portachiavi di sistema, che è per i segreti veri —
//! `PRIVACY.md` § 1 lo dice già: «più il `client_id` che avrai configurato tu».
//!
//! Si può cambiare a programma acceso, ed è il motivo per cui sta dietro un
//! lucchetto invece che in un campo immutabile: chi lo incolla in Impostazioni
//! deve poter provare subito, non riavviare.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{
    BranoEsterno, ContenutoEsterno, Disponibilita, Fonte, GenereContenuto, Licenza, Livello,
};
use aether_domain::scelta::{Candidato, Natura, natura_dal_titolo, query_larga, query_stretta};
use aether_net::{Corpo, Metodo, Rete, Richiesta, percento};
use serde_json::Value;

use crate::riferimento::Riferimento;

/// La radice dell'API.
const BASE: &str = "https://api.jamendo.com/v3.0";

/// Quanto si aspetta una risposta.
const SCADENZA: Duration = Duration::from_secs(30);

/// Quanti risultati chiede una ricerca.
const RISULTATI: u32 = 8;

/// Quanti brani si leggono al massimo da un album, una playlist o un artista.
///
/// Il tetto dell'API è 200 per pagina, e coincide con quello che Aether si dà
/// altrove: prendere l'uno per l'altro sarebbe un caso, ma il numero è giusto
/// per la stessa ragione — un elenco senza fondo, letto tutto, è la libreria di
/// qualcun altro tenuta in memoria.
const TETTO_BRANI: u32 = 200;

/// Un client di Jamendo.
#[derive(Debug, Clone)]
pub struct Jamendo {
    rete: Rete,
    /// Il `client_id`, che arriva dalle impostazioni e può cambiare a programma
    /// acceso.
    chiave: Arc<Mutex<Option<String>>>,
}

impl Default for Jamendo {
    fn default() -> Self {
        Self::nuovo()
    }
}

impl Jamendo {
    /// Un client senza chiave: finché non ne ha una non chiede niente.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            rete: Rete::nuova("jamendo.com", SCADENZA),
            chiave: Arc::new(Mutex::new(None)),
        }
    }

    /// La rete che usa, per chi deve leggere il flusso con la stessa riserva di
    /// connessioni.
    #[must_use]
    pub const fn rete(&self) -> &Rete {
        &self.rete
    }

    /// Cambia la chiave, o la toglie.
    ///
    /// Una stringa vuota vale come assente: un campo svuotato in Impostazioni
    /// deve spegnere il catalogo, non mandare richieste con `client_id=`.
    pub fn imposta_chiave(&self, nuova: Option<String>) {
        let pulita = nuova.map(|c| c.trim().to_owned()).filter(|c| !c.is_empty());
        *self.chiave.lock().unwrap_or_else(PoisonError::into_inner) = pulita;
    }

    /// C'è una chiave, quindi si può chiedere qualcosa.
    #[must_use]
    pub fn configurato(&self) -> bool {
        self.chiave
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Il catalogo risponde.
    ///
    /// `false` anche quando manca la chiave, ed è la risposta giusta: dal punto
    /// di vista di chi guarda la diagnostica, un catalogo che non si può
    /// interrogare e uno che non risponde sono la stessa cosa — la riga accanto
    /// dirà quale dei due, ma questa colonna è «adesso funziona».
    pub fn risponde(&self) -> bool {
        self.chiedi("/tracks", &[("limit", "1")]).is_ok()
    }

    /// I brani di questo catalogo che potrebbero essere quello chiesto.
    ///
    /// # Errori
    ///
    /// `catalogo.notAvailable` senza chiave, `download.externalSearchFailed`
    /// quando la ricerca non risponde, `net.*` per il trasporto.
    pub fn cerca(
        &self,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        if annullato() {
            return Ok(Vec::new());
        }
        let mut trovati = self.cerca_con(&query_stretta(brano), brano)?;
        if trovati.is_empty()
            && !annullato()
            && let Some(larga) = query_larga(brano)
        {
            trovati = self.cerca_con(&larga, brano)?;
        }
        Ok(trovati)
    }

    /// Una passata di ricerca con una stringa sola.
    fn cerca_con(&self, testo: &str, brano: &BranoEsterno) -> Result<Vec<Candidato>, AppError> {
        if testo.trim().is_empty() {
            return Ok(Vec::new());
        }
        let limite = RISULTATI.to_string();
        let corpo = self
            .chiedi(
                "/tracks",
                &[
                    ("limit", &limite),
                    // `search` e non `namesearch`: il primo guarda titolo,
                    // artista e album insieme, che è la forma delle due query
                    // che `scelta` costruisce — «artista titolo». Il secondo
                    // cerca solo nel nome del brano e non troverebbe mai
                    // niente con quelle stringhe.
                    ("search", testo),
                ],
            )
            .map_err(|err| {
                if err.code().kind().code() == "catalogo.notAvailable" {
                    return err;
                }
                AppError::new(ErrorCode::DownloadExternalSearchFailed).with_cause(
                    err.cause()
                        .unwrap_or("la ricerca di Jamendo non ha risposto")
                        .to_owned(),
                )
            })?;

        Ok(risultati(&corpo)
            .iter()
            .filter_map(|t| candidato_da(t, Some(&brano.title)))
            .collect())
    }

    /// Un brano, un album, una playlist o un artista, come contenuto
    /// importabile.
    ///
    /// # Errori
    ///
    /// `catalogo.notAvailable` senza chiave, `catalogo.notPublic` se il link
    /// non porta a niente, `catalogo.resolveFailed` se non se ne cava niente di
    /// ascoltabile.
    pub fn risolvi(&self, riferimento: &Riferimento) -> Result<ContenutoEsterno, AppError> {
        let (percorso, dichiarati) = match riferimento.genere {
            GenereContenuto::Track => ("/tracks", Some(1_u32)),
            GenereContenuto::Album => ("/albums/tracks", None),
            GenereContenuto::Playlist | GenereContenuto::Collezione => ("/playlists/tracks", None),
            GenereContenuto::Artist => ("/artists/tracks", None),
        };
        let limite = TETTO_BRANI.to_string();
        let corpo = self.chiedi(percorso, &[("id", &riferimento.id), ("limit", &limite)])?;
        let voci = risultati(&corpo);
        let Some(testa) = voci.first() else {
            return Err(
                AppError::new(ErrorCode::CatalogoNotPublic).with_cause(format!(
                    "Jamendo non conosce «{}»",
                    riferimento.url_pubblico()
                )),
            );
        };

        // I punti `/*/tracks` restituiscono il contenitore con dentro `tracks`;
        // `/tracks` restituisce i brani direttamente. Due forme, e leggerne una
        // sola vorrebbe dire che tre link su quattro tornano vuoti.
        let (brani, titolo, autore, copertina) = match riferimento.genere {
            GenereContenuto::Track => (
                voci.clone(),
                testo(testa, "name").unwrap_or_else(|| riferimento.id.clone()),
                testo(testa, "artist_name"),
                testo(testa, "album_image").or_else(|| testo(testa, "image")),
            ),
            _ => (
                testa
                    .get("tracks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                testo(testa, "name")
                    .or_else(|| testo(testa, "artist_name"))
                    .unwrap_or_else(|| riferimento.id.clone()),
                testo(testa, "artist_name"),
                testo(testa, "image").or_else(|| testo(testa, "album_image")),
            ),
        };

        let tracce: Vec<BranoEsterno> = brani
            .iter()
            .filter_map(|t| candidato_da(t, None))
            .enumerate()
            .map(|(n, c)| brano_da(c, n))
            .collect();
        if tracce.is_empty() {
            return Err(
                AppError::new(ErrorCode::CatalogoResolveFailed).with_cause(format!(
                    "«{}» non ha nessun brano ascoltabile",
                    riferimento.url_pubblico()
                )),
            );
        }

        Ok(ContenutoEsterno {
            fonte: Fonte::Jamendo,
            kind: riferimento.genere,
            id: riferimento.id.clone(),
            title: titolo,
            author: autore,
            cover_url: copertina,
            declared_total: dichiarati,
            tracks: tracce,
            source: Livello::Jamendo,
        })
    }

    /// Una richiesta all'API, con la chiave e il formato.
    fn chiedi(&self, percorso: &str, parametri: &[(&str, &str)]) -> Result<Value, AppError> {
        let Some(chiave) = self
            .chiave
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            return Err(AppError::new(ErrorCode::CatalogoNotAvailable)
                .with_cause("manca il client id di Jamendo: si mette in Impostazioni".to_owned()));
        };
        let mut url = format!(
            "{BASE}{percorso}/?client_id={}&format=json",
            percento(&chiave)
        );
        for (nome, valore) in parametri {
            url.push('&');
            url.push_str(nome);
            url.push('=');
            url.push_str(&percento(valore));
        }
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: &url,
            intestazioni: &[("accept", "application/json")],
            corpo: Corpo::Niente,
        })?;
        if !risposta.e_andata() {
            return Err(self.rete.stato_a_errore(&risposta, &url));
        }
        let corpo: Value = serde_json::from_slice(&risposta.corpo).map_err(|err| {
            AppError::new(ErrorCode::DownloadBadResponse)
                .with_cause(format!("la risposta di {percorso} non è JSON: {err}"))
        })?;

        // Jamendo risponde `200` anche quando rifiuta: il verdetto sta in
        // `headers.status`, e leggere solo il codice HTTP vorrebbe dire trattare
        // «client id non valido» come un elenco vuoto — cioè dire a chi ha
        // sbagliato a incollare la chiave che il catalogo non ha quel brano.
        if let Some(intestazioni) = corpo.get("headers")
            && testo(intestazioni, "status").as_deref() == Some("failed")
        {
            let perche = testo(intestazioni, "error_message")
                .unwrap_or_else(|| "Jamendo ha rifiutato la richiesta".to_owned());
            return Err(AppError::new(ErrorCode::CatalogoNotAvailable).with_cause(perche));
        }
        Ok(corpo)
    }
}

/// L'array che sta in `results`, o niente.
fn risultati(corpo: &Value) -> Vec<Value> {
    corpo
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Un campo di testo, non vuoto.
fn testo(valore: &Value, campo: &str) -> Option<String> {
    let grezzo = valore.get(campo)?.as_str()?.trim();
    (!grezzo.is_empty()).then(|| grezzo.to_owned())
}

/// La licenza, dall'indirizzo Creative Commons che Jamendo dichiara.
///
/// La stessa lettura che `archivio_org` fa di `licenseurl`, e per la stessa
/// ragione: l'indirizzo canonico di una CC porta il codice nel percorso, ed è
/// un dato più solido di qualunque etichetta.
///
/// Senza l'indirizzo si resta a [`Licenza::Sconosciuta`]. Su Jamendo cambia
/// poco — da lì non si tiene niente comunque — ma la licenza si **mostra**
/// accanto al brano e si scrive nei desiderati, e dichiararne una che non si è
/// letta sarebbe inventarsela.
fn licenza_da(traccia: &Value) -> Licenza {
    let Some(url) = testo(traccia, "license_ccurl") else {
        return Licenza::Sconosciuta;
    };
    let piatto = url.to_ascii_lowercase();
    if piatto.contains("publicdomain") || piatto.contains("/zero/") {
        return Licenza::PubblicoDominio;
    }
    if let Some(coda) = piatto.split("/licenses/").nth(1)
        && let Some(codice) = coda.split('/').next()
        && !codice.is_empty()
    {
        return Licenza::CreativeCommons(codice.to_owned());
    }
    Licenza::Sconosciuta
}

/// Da una traccia dell'API a un candidato.
///
/// `None` quando manca l'indirizzo del flusso: un brano che non si può sentire
/// non è un candidato, e metterlo in elenco darebbe una playlist con dentro
/// qualcosa che non parte.
fn candidato_da(traccia: &Value, atteso: Option<&str>) -> Option<Candidato> {
    let titolo = testo(traccia, "name")?;
    // `audio` e **mai** `audiodownload`: vedi la testa del modulo. Il secondo
    // campo esiste e per certi brani è pieno; leggerlo sarebbe scoprire se il
    // server ce li lascerebbe prendere, che non è la stessa domanda di «si può».
    let flusso = testo(traccia, "audio")?;

    let licenza = licenza_da(traccia);
    // Non si guarda niente per decidere: la fonte non consegna, punto. La
    // chiamata resta esplicita perché la regola sta in un posto solo, e perché
    // il giorno in cui qualcuno cambiasse `puo_consegnare` questa riga
    // seguirebbe invece di restare indietro.
    let disponibilita = Disponibilita::decidi(Fonte::Jamendo, &licenza, false);

    Some(Candidato {
        url: flusso,
        natura: atteso.map_or_else(Natura::default, |a| natura_dal_titolo(&titolo, a)),
        titolo,
        autore: testo(traccia, "artist_name"),
        // Jamendo non ha una verifica degli artisti: chi pubblica è chi ha
        // caricato, e dichiararlo «verificato» sarebbe un peso inventato dentro
        // un confronto che decide quale registrazione vince.
        autore_verificato: false,
        durata_sec: traccia
            .get("duration")
            .and_then(Value::as_u64)
            .and_then(|d| u32::try_from(d).ok()),
        fonte: Fonte::Jamendo,
        licenza,
        disponibilita,
        // Il flusso di Jamendo è sempre mp3: `format=mp31` sta nell'indirizzo
        // che loro stessi costruiscono. Serve al decodificatore come
        // suggerimento — dal percorso non si ricava, perché l'estensione non
        // c'è.
        estensione: Some("mp3".to_owned()),
        pagina: testo(traccia, "shareurl").or_else(|| testo(traccia, "shorturl")),
    })
}

/// Da un candidato a un brano, col suo posto nell'elenco.
fn brano_da(candidato: Candidato, posizione: usize) -> BranoEsterno {
    BranoEsterno {
        title: candidato.titolo,
        artist: candidato.autore,
        duration_ms: candidato
            .durata_sec
            .map(|s| u64::from(s).saturating_mul(1_000)),
        fonte_url: Some(candidato.url),
        licenza: candidato.licenza,
        disponibilita: candidato.disponibilita,
        pagina_url: candidato.pagina,
        track_number: u32::try_from(posizione.saturating_add(1)).ok(),
        ..BranoEsterno::default()
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn traccia(json: &str) -> Value {
        serde_json::from_str(json).unwrap_or(Value::Null)
    }

    #[test]
    fn senza_chiave_non_parte_nessuna_richiesta() {
        // Le prove girano senza rete: se `chiedi` provasse a uscire prima di
        // guardare la chiave, qui uscirebbe un errore di trasporto invece di
        // questo. Il codice che torna è la dimostrazione.
        let jamendo = Jamendo::nuovo();
        assert!(!jamendo.configurato());
        let esito = jamendo.risolvi(&Riferimento {
            fonte: Fonte::Jamendo,
            genere: GenereContenuto::Track,
            id: "1532771".to_owned(),
        });
        assert!(matches!(
            esito.as_ref().map_err(AppError::code),
            Err(ErrorCode::CatalogoNotAvailable)
        ));
        assert!(!jamendo.risponde());
    }

    #[test]
    fn un_campo_svuotato_spegne_il_catalogo() {
        // Chi cancella la chiave in Impostazioni sta spegnendo Jamendo, non
        // chiedendo di mandare `client_id=` a vuoto.
        let jamendo = Jamendo::nuovo();
        jamendo.imposta_chiave(Some("abc123".to_owned()));
        assert!(jamendo.configurato());
        jamendo.imposta_chiave(Some("   ".to_owned()));
        assert!(!jamendo.configurato(), "gli spazi non sono una chiave");
        jamendo.imposta_chiave(Some("abc123".to_owned()));
        jamendo.imposta_chiave(None);
        assert!(!jamendo.configurato());
    }

    #[test]
    fn da_jamendo_non_esce_mai_qualcosa_di_scaricabile() {
        // È il patto del modulo, e vale **per ogni licenza**: anche un brano in
        // pubblico dominio, da lì, si ascolta soltanto. Non lo decide questo
        // file — lo decide `Fonte::puo_consegnare` — e questa prova è la rete
        // che lo verifica dal di fuori.
        for licenza in [
            r#""license_ccurl":"http://creativecommons.org/publicdomain/zero/1.0/""#,
            r#""license_ccurl":"http://creativecommons.org/licenses/by/3.0/""#,
            r#""license_ccurl":"http://creativecommons.org/licenses/by-nc-nd/3.0/""#,
        ] {
            let json = format!(r#"{{"name":"t","audio":"https://p/x.mp3",{licenza}}}"#);
            let candidato = candidato_da(&traccia(&json), None).unwrap_or_default();
            assert_eq!(
                candidato.disponibilita,
                Disponibilita::SoloAscolto,
                "con {licenza}"
            );
            assert!(!candidato.si_puo_tenere());
        }
    }

    #[test]
    fn si_prende_il_flusso_e_non_lo_scarico() {
        // `audiodownload` è pieno e va ignorato: i loro termini vietano la
        // copia, e il fatto che l'indirizzo risponda non è un permesso.
        let con_tutti_e_due = traccia(
            r#"{"name":"t","audio":"https://p/flusso.mp3","audiodownload":"https://p/scarico.mp3","audiodownload_allowed":true}"#,
        );
        let candidato = candidato_da(&con_tutti_e_due, None).unwrap_or_default();
        assert_eq!(candidato.url, "https://p/flusso.mp3");
        assert!(!candidato.url.contains("scarico"));
    }

    #[test]
    fn senza_flusso_non_e_un_candidato() {
        let muto = traccia(r#"{"name":"t","audiodownload":"https://p/x.mp3"}"#);
        assert!(candidato_da(&muto, None).is_none());
    }

    #[test]
    fn la_licenza_si_legge_dallindirizzo_canonico() {
        for (url, atteso) in [
            (
                "http://creativecommons.org/licenses/by-nc-nd/3.0/",
                Licenza::CreativeCommons("by-nc-nd".to_owned()),
            ),
            (
                "https://creativecommons.org/licenses/by-sa/4.0/",
                Licenza::CreativeCommons("by-sa".to_owned()),
            ),
            (
                "http://creativecommons.org/publicdomain/zero/1.0/",
                Licenza::PubblicoDominio,
            ),
        ] {
            let json = format!(r#"{{"license_ccurl":"{url}"}}"#);
            assert_eq!(licenza_da(&traccia(&json)), atteso, "{url}");
        }
        assert_eq!(licenza_da(&traccia("{}")), Licenza::Sconosciuta);
    }

    #[test]
    fn un_rifiuto_di_jamendo_non_e_un_elenco_vuoto() {
        // Rispondono `200` anche quando dicono di no: il verdetto sta in
        // `headers.status`. Leggere solo il codice HTTP vorrebbe dire dire a
        // chi ha sbagliato la chiave che il catalogo non ha quel brano.
        let corpo = traccia(
            r#"{"headers":{"status":"failed","error_message":"Your credential is not valid"},"results":[]}"#,
        );
        assert_eq!(
            testo(corpo.get("headers").unwrap_or(&Value::Null), "status").as_deref(),
            Some("failed")
        );
        assert!(risultati(&corpo).is_empty());
    }

    #[test]
    fn il_mp3_si_dichiara_perche_dallindirizzo_non_si_ricava() {
        // Il loro flusso è `.../?trackid=…&format=mp31`: nessuna estensione nel
        // percorso, e il decodificatore ha bisogno del suggerimento.
        let t = traccia(
            r#"{"name":"t","audio":"https://prod-1.storage.jamendo.com/?trackid=1&format=mp31"}"#,
        );
        assert_eq!(
            candidato_da(&t, None).and_then(|c| c.estensione),
            Some("mp3".to_owned())
        );
    }
}
