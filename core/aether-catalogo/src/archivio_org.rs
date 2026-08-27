//! L'Internet Archive: cercare che cosa c'è, e prenderne i byte.
//!
//! È l'unico catalogo di Aether da cui si **tiene** qualcosa, e la ragione non è
//! tecnica: è che qui il permesso è scritto. Il Live Music Archive esiste perché
//! gli artisti che ci stano dentro hanno acconsentito allo scambio non
//! commerciale delle loro registrazioni dal vivo; le netlabel pubblicano sotto
//! Creative Commons; il pubblico dominio è pubblico dominio. Nessuna di queste
//! tre cose richiede a nessuno di fidarsi di noi.
//!
//! # La cosa da sapere prima di leggere il codice
//!
//! **La ricerca dell'Internet Archive indicizza gli *item*, non i file.** Un
//! item è un concerto intero o un disco intero; il brano singolo è un file
//! dentro l'item, e nell'indice non compare affatto. Cercare «Grateful Dead
//! Sugaree» quindi non restituisce mai un brano: restituisce i concerti in cui
//! quel brano potrebbe esserci.
//!
//! Da lì la forma di questo modulo, che è la cosa che lo distingue da un client
//! HTTP qualunque: si cerca l'item, si chiede la sua scheda, e si guarda **nei
//! file** se il brano c'è. Due richieste invece di una, moltiplicate per quanti
//! item vale la pena aprire — ed è il motivo per cui [`ITEM_DA_APRIRE`] è un
//! numero piccolo e [`primo_sicuro`] esiste per smettere presto.
//!
//! # Che cosa non si prende, e non è una svista
//!
//! Un item marcato come consultabile e basta (`access-restricted-item`, o il
//! vecchio `stream_only` dei soundboard) resta [`Disponibilita::SoloAscolto`]
//! anche se i suoi file si scaricherebbero benissimo. Il fatto che un indirizzo
//! risponda non è un permesso.

use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{
    BranoEsterno, ContenutoEsterno, Disponibilita, Fonte, GenereContenuto, Licenza, Livello,
};
use aether_domain::scelta::{Candidato, Natura, natura_dal_titolo, query_larga, query_stretta};
use aether_domain::text::fold_text;
use aether_domain::titolo::spezza;
use aether_net::{Corpo, Metodo, Rete, Richiesta, percento};
use serde_json::Value;

/// La radice di tutto.
const BASE: &str = "https://archive.org";

/// Le collezioni in cui si cerca.
///
/// Non «tutto l'audio»: l'Internet Archive ospita anche molto materiale caricato
/// da chi non aveva il diritto di caricarlo, e una ricerca senza questo filtro
/// lo troverebbe insieme al resto. Queste tre hanno ciascuna un patto scritto
/// dietro — il consenso dei tapers, la licenza della netlabel, la scadenza del
/// diritto d'autore.
const COLLEZIONI: &str = "collection:(etree OR netlabels OR audio_music OR 78rpm)";

/// Quanti item la ricerca restituisce.
const ITEM_TROVATI: u32 = 8;

/// Di quanti item si apre la scheda.
///
/// Quattro e non otto: ogni scheda è una richiesta, e una richiesta a un
/// servizio pubblico gratuito che regge il mondo si paga in cortesia. Con
/// [`primo_sicuro`] la maggior parte delle volte se ne aprono una o due.
const ITEM_DA_APRIRE: usize = 4;

/// Quanto si aspetta una risposta.
const SCADENZA: Duration = Duration::from_secs(30);

/// I formati che il motore audio sa aprire, dal migliore al peggiore.
///
/// L'ordine è quello di preferenza e conta: l'Internet Archive tiene l'originale
/// senza perdita **e** i derivati compressi che genera lui, e per una libreria
/// che si tiene sul disco l'originale è quel che si vuole. Non c'è `shn` né
/// `ogg`: symphonia non decodifica il primo, e il secondo lo decodifica ma i
/// derivati Ogg dell'Archive sono a bitrate basso, quindi non c'è mai un caso in
/// cui sia la scelta migliore fra quelle presenti.
const FORMATI: [(&str, &str); 4] = [
    ("flac", "flac"),
    ("24bit flac", "flac"),
    ("vbr mp3", "mp3"),
    ("mp3", "mp3"),
];

/// Un client dell'Internet Archive.
#[derive(Debug, Clone)]
pub struct ArchivioOrg {
    rete: Rete,
}

impl Default for ArchivioOrg {
    fn default() -> Self {
        Self::nuovo()
    }
}

impl ArchivioOrg {
    /// Un client con la scadenza di serie.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            rete: Rete::nuova("archive.org", SCADENZA),
        }
    }

    /// La rete che usa, per chi deve prelevare con la stessa riserva di
    /// connessioni invece di aprirne una seconda.
    #[must_use]
    pub const fn rete(&self) -> &Rete {
        &self.rete
    }

    /// Il catalogo risponde.
    ///
    /// Una richiesta vera e piccolissima: la ricerca di un item solo. Serve alla
    /// diagnostica, che deve poter distinguere «non l'ho trovato» da «non ci
    /// arrivo».
    pub fn risponde(&self) -> bool {
        self.cerca_item("mediatype:audio", 1).is_ok()
    }

    /// I brani di questo catalogo che potrebbero essere quello chiesto.
    ///
    /// Prima con la stringa stretta, e solo se non ha dato niente con quella
    /// larga: la seconda richiesta si paga, e nella maggior parte dei casi la
    /// prima basta.
    ///
    /// # Errori
    ///
    /// `download.externalSearchFailed` quando la ricerca non risponde,
    /// `net.*` per i guasti di trasporto.
    pub fn cerca(
        &self,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        let mut trovati = self.cerca_con(&query_stretta(brano), brano, annullato)?;
        if trovati.is_empty()
            && let Some(larga) = query_larga(brano)
        {
            trovati = self.cerca_con(&larga, brano, annullato)?;
        }
        Ok(trovati)
    }

    /// Una passata di ricerca con una stringa sola.
    fn cerca_con(
        &self,
        testo: &str,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        if testo.trim().is_empty() {
            return Ok(Vec::new());
        }
        let query = format!("mediatype:audio AND {COLLEZIONI} AND ({})", frase(testo));
        let item = self.cerca_item(&query, ITEM_TROVATI)?;

        let mut candidati = Vec::new();
        for doc in item.iter().take(ITEM_DA_APRIRE) {
            if annullato() {
                return Ok(candidati);
            }
            let Some(identificativo) = testo_di(doc, "identifier") else {
                continue;
            };
            let scheda = self.scheda(&identificativo)?;
            candidati.extend(brani_dalla_scheda(&identificativo, &scheda, Some(brano)));
            if primo_sicuro(&candidati, brano) {
                break;
            }
        }
        Ok(candidati)
    }

    /// Un item intero, come contenuto importabile.
    ///
    /// È la strada di chi incolla il link di un concerto: non si cerca niente,
    /// si legge quel che c'è dentro nell'ordine in cui sta.
    ///
    /// # Errori
    ///
    /// `catalogo.notPublic` se l'item non esiste, `catalogo.resolveFailed` se
    /// non ha nessun file leggibile.
    pub fn risolvi(&self, identificativo: &str) -> Result<ContenutoEsterno, AppError> {
        let scheda = self.scheda(identificativo)?;
        let metadati = scheda.get("metadata").unwrap_or(&Value::Null);
        if metadati.is_null() {
            return Err(AppError::new(ErrorCode::CatalogoNotPublic)
                .with_cause(format!("l'item «{identificativo}» non esiste")));
        }

        let candidati = brani_dalla_scheda(identificativo, &scheda, None);
        if candidati.is_empty() {
            return Err(
                AppError::new(ErrorCode::CatalogoResolveFailed).with_cause(format!(
                    "l'item «{identificativo}» non ha nessun file in un formato leggibile"
                )),
            );
        }

        let autore = testo_di(metadati, "creator");
        let licenza = licenza_da(metadati);
        let disponibilita = disponibilita_da(metadati, &licenza);
        let tracce: Vec<BranoEsterno> = candidati
            .into_iter()
            .enumerate()
            .map(|(n, c)| brano_da_candidato(c, autore.as_deref(), n))
            .collect();

        Ok(ContenutoEsterno {
            fonte: Fonte::InternetArchive,
            kind: GenereContenuto::Collezione,
            id: identificativo.to_owned(),
            title: testo_di(metadati, "title").unwrap_or_else(|| identificativo.to_owned()),
            author: autore,
            cover_url: Some(format!("{BASE}/services/img/{}", percento(identificativo))),
            // Il numero di brani è quello che si è letto: la scheda arriva tutta
            // in una risposta, non c'è una pagina dopo, e dichiarare un totale
            // diverso da quello letto sarebbe inventarsi un ammanco.
            declared_total: None,
            tracks: {
                let mut t = tracce;
                for (n, brano) in t.iter_mut().enumerate() {
                    brano.disponibilita = disponibilita;
                    brano.licenza = licenza.clone();
                    brano.track_number = brano
                        .track_number
                        .or_else(|| u32::try_from(n.saturating_add(1)).ok());
                }
                t
            },
            source: Livello::ArchivioOrg,
        })
    }

    /// La ricerca avanzata: da una query a un elenco di item.
    fn cerca_item(&self, query: &str, righe: u32) -> Result<Vec<Value>, AppError> {
        let url = format!(
            "{BASE}/advancedsearch.php?q={}&fl%5B%5D=identifier&rows={righe}&page=1&output=json",
            percento(query)
        );
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: &url,
            intestazioni: &[("accept", "application/json")],
            corpo: Corpo::Niente,
        })?;
        if !risposta.e_andata() {
            return Err(AppError::new(ErrorCode::DownloadExternalSearchFailed)
                .with_cause(format!("la ricerca ha risposto {}", risposta.stato)));
        }
        let corpo: Value = serde_json::from_slice(&risposta.corpo).map_err(|err| {
            AppError::new(ErrorCode::DownloadBadResponse)
                .with_cause(format!("la ricerca non è JSON: {err}"))
        })?;
        Ok(corpo
            .get("response")
            .and_then(|r| r.get("docs"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// La scheda di un item: i metadati e l'elenco dei file.
    fn scheda(&self, identificativo: &str) -> Result<Value, AppError> {
        let url = format!("{BASE}/metadata/{}", percento(identificativo));
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: &url,
            intestazioni: &[("accept", "application/json")],
            corpo: Corpo::Niente,
        })?;
        if !risposta.e_andata() {
            return Err(self.rete.stato_a_errore(&risposta, &url));
        }
        serde_json::from_slice(&risposta.corpo).map_err(|err| {
            AppError::new(ErrorCode::DownloadBadResponse)
                .with_cause(format!("la scheda di «{identificativo}» non è JSON: {err}"))
        })
    }
}

/// Il testo di un campo, che l'Archive dà come stringa o come elenco.
///
/// `creator` e `collection` sono l'uno o l'altro a seconda dell'item, e non c'è
/// modo di saperlo prima: un item con due autori li dà come array, uno con un
/// autore solo come stringa. Trattarne uno solo dei due casi vuol dire perdere
/// l'autore su metà del catalogo, in silenzio.
fn testo_di(valore: &Value, campo: &str) -> Option<String> {
    match valore.get(campo)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Array(elenco) => {
            let uniti: Vec<&str> = elenco.iter().filter_map(Value::as_str).collect();
            (!uniti.is_empty()).then(|| uniti.join(", "))
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Le collezioni di un item, in minuscolo.
fn collezioni_di(metadati: &Value) -> Vec<String> {
    testo_di(metadati, "collection")
        .map(|c| {
            c.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

/// Un campo che vale «sì» in una delle forme che l'Archive usa.
fn vero(metadati: &Value, campo: &str) -> bool {
    match metadati.get(campo) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
        }
        Some(Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

/// Sotto che licenza sta un item.
///
/// L'ordine dei tentativi è quello dell'autorevolezza: `licenseurl` è una
/// dichiarazione esplicita di chi ha caricato, la collezione è un'inferenza dal
/// patto che quella collezione ha con i suoi artisti. La seconda si usa solo
/// quando la prima tace, e mai per contraddirla.
fn licenza_da(metadati: &Value) -> Licenza {
    if let Some(url) = testo_di(metadati, "licenseurl") {
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
    }

    let collezioni = collezioni_di(metadati);
    if collezioni.iter().any(|c| c == "etree") {
        // Il patto del Live Music Archive: gli artisti *trade-friendly*
        // acconsentono allo scambio non commerciale, e a quello soltanto.
        return Licenza::LiberaNonCommerciale;
    }
    if collezioni.iter().any(|c| c == "78rpm") {
        return Licenza::PubblicoDominio;
    }
    Licenza::Sconosciuta
}

/// Che cosa si può fare di un item.
fn disponibilita_da(metadati: &Value, licenza: &Licenza) -> Disponibilita {
    // Il fatto che un indirizzo risponda non è un permesso: un item marcato
    // come consultabile e basta resta tale anche se i byte si prenderebbero.
    let solo_ascolto = vero(metadati, "access-restricted-item")
        || vero(metadati, "access-restricted")
        || vero(metadati, "stream_only");
    Disponibilita::decidi(Fonte::InternetArchive, licenza, !solo_ascolto)
}

/// Il formato di un file, tradotto in estensione, se lo sappiamo aprire.
///
/// Restituisce anche quanto è preferito, perché la scelta fra due file dello
/// stesso brano nello stesso item è esattamente questa: il FLAC originale batte
/// l'MP3 che l'Archive ha generato da lui.
fn formato_di(file: &Value) -> Option<(usize, &'static str)> {
    let dichiarato = file.get("format")?.as_str()?.to_ascii_lowercase();
    FORMATI
        .iter()
        .enumerate()
        .find(|(_, (nome, _))| dichiarato == *nome)
        .map(|(rango, (_, estensione))| (rango, *estensione))
}

/// La durata di un file in secondi, dalle due forme che l'Archive usa.
///
/// `"312.45"` e `"5:12"` compaiono tutte e due, e nello stesso item: la prima
/// sui derivati che genera lui, la seconda su quel che ha scritto chi ha
/// caricato.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "i secondi sono già vincolati a finiti e non negativi, e un brano               che durasse più di 4 miliardi di secondi non è un brano"
)]
fn durata_di(file: &Value) -> Option<u32> {
    let grezza = file.get("length")?.as_str()?.trim();
    if grezza.is_empty() {
        return None;
    }
    if let Some((minuti, secondi)) = grezza.split_once(':') {
        let m: u32 = minuti.trim().parse().ok()?;
        let s: f64 = secondi.trim().parse().ok()?;
        return Some(m.saturating_mul(60).saturating_add(s.round() as u32));
    }
    let secondi: f64 = grezza.parse().ok()?;
    (secondi.is_finite() && secondi >= 0.0).then(|| secondi.round() as u32)
}

/// I brani di un item, come candidati.
///
/// Con `atteso` a `Some`, tiene solo quelli che potrebbero essere quel brano;
/// con `None` li tiene tutti, che è il caso di chi ha incollato il link
/// dell'item e vuole quel che c'è dentro.
fn brani_dalla_scheda(
    identificativo: &str,
    scheda: &Value,
    atteso: Option<&BranoEsterno>,
) -> Vec<Candidato> {
    let metadati = scheda.get("metadata").unwrap_or(&Value::Null);
    let licenza = licenza_da(metadati);
    let disponibilita = disponibilita_da(metadati, &licenza);
    let autore = testo_di(metadati, "creator");
    let album = testo_di(metadati, "title");
    let dal_vivo = collezioni_di(metadati).iter().any(|c| c == "etree");

    let Some(file) = scheda.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };

    // Un brano può stare nello stesso item in FLAC e in MP3. Si tiene il
    // migliore per titolo, non tutti e due: due candidati identici tranne che
    // per il formato farebbero perdere tempo alla scelta e, a pari merito,
    // deciderebbe l'ordine di arrivo, cioè il caso.
    let mut migliori: Vec<(String, usize, Candidato)> = Vec::new();

    for voce in file {
        let Some((rango, estensione)) = formato_di(voce) else {
            continue;
        };
        let Some(nome) = voce.get("name").and_then(Value::as_str) else {
            continue;
        };
        let titolo_grezzo = testo_di(voce, "title").unwrap_or_else(|| nome_pulito(nome));
        let (interprete, titolo) = spezza(&titolo_grezzo, autore.as_deref());
        if titolo.trim().is_empty() {
            continue;
        }

        if let Some(atteso) = atteso
            && !somiglia(&titolo, &atteso.title)
        {
            continue;
        }

        let chiave = fold_text(&titolo);
        let natura = if dal_vivo {
            Natura::DalVivo
        } else {
            atteso.map_or(Natura::Studio, |a| natura_dal_titolo(&titolo, &a.title))
        };

        let candidato = Candidato {
            url: format!(
                "{BASE}/download/{}/{}",
                percento(identificativo),
                percento(nome)
            ),
            titolo,
            autore: testo_di(voce, "artist").or_else(|| interprete.clone()),
            // L'Archive non ha una spunta blu. Quel che ha è l'appartenenza a
            // una collezione curata, che è la cosa più vicina a «qualcuno ha
            // guardato»: `etree` accetta solo artisti che hanno acconsentito.
            autore_verificato: dal_vivo,
            durata_sec: durata_di(voce),
            natura,
            fonte: Fonte::InternetArchive,
            licenza: licenza.clone(),
            disponibilita,
            estensione: Some(estensione.to_owned()),
            pagina: Some(format!("{BASE}/details/{}", percento(identificativo))),
        };

        match migliori.iter_mut().find(|(c, _, _)| *c == chiave) {
            Some((_, rango_ora, ferma)) if rango < *rango_ora => {
                *rango_ora = rango;
                *ferma = candidato;
            }
            Some(_) => {}
            None => migliori.push((chiave, rango, candidato)),
        }
    }

    let _ = album;
    migliori.into_iter().map(|(_, _, c)| c).collect()
}

/// Il nome del file senza cartella e senza estensione, come ripiego di un titolo.
fn nome_pulito(nome: &str) -> String {
    let ultimo = nome.rsplit('/').next().unwrap_or(nome);
    ultimo
        .rsplit_once('.')
        .map_or(ultimo, |(radice, _)| radice)
        .replace(['_', '-'], " ")
        .trim()
        .to_owned()
}

/// I due titoli potrebbero essere lo stesso brano.
///
/// Deliberatamente largo: qui non si decide niente, si sfoltisce. La decisione è
/// di [`aether_domain::scelta::scegli_candidato`], che guarda anche la durata, e
/// togliere qui un candidato che quella avrebbe scelto è un brano perduto in
/// silenzio.
fn somiglia(titolo: &str, atteso: &str) -> bool {
    let a = fold_text(titolo);
    let b = fold_text(atteso);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

/// Fra i candidati ce n'è uno che chiude la questione.
///
/// Serve a smettere di aprire schede: ogni scheda è una richiesta a un servizio
/// pubblico gratuito, e continuare a chiederne dopo aver già trovato il brano
/// giusto è tempo di chi aspetta e banda di chi ospita, per niente.
fn primo_sicuro(candidati: &[Candidato], brano: &BranoEsterno) -> bool {
    let Some(attesa) = brano.duration_ms else {
        // Senza una durata da confrontare non si può essere sicuri di niente:
        // si aprono tutte le schede previste e decide chi sceglie.
        return false;
    };
    candidati.iter().any(|c| {
        c.durata_sec
            .is_some_and(|sec| attesa.abs_diff(u64::from(sec).saturating_mul(1000)) <= 3_000)
            && fold_text(&c.titolo) == fold_text(&brano.title)
    })
}

/// Da un candidato al brano che finirà in libreria.
fn brano_da_candidato(
    candidato: Candidato,
    autore: Option<&str>,
    posizione: usize,
) -> BranoEsterno {
    BranoEsterno {
        title: candidato.titolo,
        artist: candidato.autore.or_else(|| autore.map(ToOwned::to_owned)),
        album_artist: autore.map(ToOwned::to_owned),
        track_number: u32::try_from(posizione.saturating_add(1)).ok(),
        duration_ms: candidato
            .durata_sec
            .map(|s| u64::from(s).saturating_mul(1000)),
        fonte_url: Some(candidato.url),
        licenza: candidato.licenza,
        disponibilita: candidato.disponibilita,
        pagina_url: candidato.pagina,
        ..BranoEsterno::default()
    }
}

/// La frase da mettere nella query, con le virgolette al posto giusto.
///
/// Senza virgolette, «Karma Police» cerca i due termini separatamente e trova
/// ogni item che contenga «police». Con le virgolette cerca la frase.
fn frase(testo: &str) -> String {
    let ripulito: String = testo
        .chars()
        .map(|c| if c == '"' || c == '\\' { ' ' } else { c })
        .collect();
    format!("\"{}\"", ripulito.trim())
}

#[cfg(test)]
mod prove {
    use super::*;

    fn scheda(json: &str) -> Value {
        serde_json::from_str(json).unwrap_or(Value::Null)
    }

    #[test]
    fn un_campo_e_stringa_o_elenco_e_va_letto_in_tutti_e_due_i_modi() {
        let uno = scheda(r#"{"creator":"Grateful Dead"}"#);
        let due = scheda(r#"{"creator":["Jerry Garcia","Bob Weir"]}"#);
        assert_eq!(testo_di(&uno, "creator").as_deref(), Some("Grateful Dead"));
        assert_eq!(
            testo_di(&due, "creator").as_deref(),
            Some("Jerry Garcia, Bob Weir")
        );
        assert_eq!(testo_di(&uno, "assente"), None);
    }

    #[test]
    fn la_licenza_dichiarata_batte_quella_dedotta_dalla_collezione() {
        let m = scheda(
            r#"{"collection":["etree"],
                "licenseurl":"https://creativecommons.org/licenses/by-nc-sa/3.0/"}"#,
        );
        assert_eq!(
            licenza_da(&m),
            Licenza::CreativeCommons("by-nc-sa".to_owned())
        );
    }

    #[test]
    fn senza_licenza_dichiarata_il_patto_dei_tapers_vale_come_licenza() {
        let m = scheda(r#"{"collection":["etree","GratefulDead"]}"#);
        assert_eq!(licenza_da(&m), Licenza::LiberaNonCommerciale);
    }

    #[test]
    fn il_pubblico_dominio_si_riconosce_in_tutte_e_due_le_forme() {
        assert_eq!(
            licenza_da(&scheda(
                r#"{"licenseurl":"http://creativecommons.org/publicdomain/mark/1.0/"}"#
            )),
            Licenza::PubblicoDominio
        );
        assert_eq!(
            licenza_da(&scheda(
                r#"{"licenseurl":"https://creativecommons.org/publicdomain/zero/1.0/"}"#
            )),
            Licenza::PubblicoDominio
        );
        assert_eq!(
            licenza_da(&scheda(r#"{"collection":"78rpm"}"#)),
            Licenza::PubblicoDominio
        );
    }

    #[test]
    fn una_licenza_sconosciuta_non_diventa_scaricabile() {
        let m = scheda(r#"{"collection":["opensource_audio"]}"#);
        assert_eq!(licenza_da(&m), Licenza::Sconosciuta);
        assert_eq!(
            disponibilita_da(&m, &licenza_da(&m)),
            Disponibilita::SoloAscolto
        );
    }

    #[test]
    fn un_item_consultabile_e_basta_resta_solo_ascolto() {
        // Il fatto che i byte si prenderebbero non è un permesso.
        let m = scheda(r#"{"collection":["etree"],"access-restricted-item":"true"}"#);
        assert_eq!(
            disponibilita_da(&m, &licenza_da(&m)),
            Disponibilita::SoloAscolto
        );
        let libero = scheda(r#"{"collection":["etree"]}"#);
        assert_eq!(
            disponibilita_da(&libero, &licenza_da(&libero)),
            Disponibilita::Scaricabile
        );
    }

    #[test]
    fn la_durata_si_legge_nelle_due_forme_che_larchive_usa() {
        assert_eq!(durata_di(&scheda(r#"{"length":"312.45"}"#)), Some(312));
        assert_eq!(durata_di(&scheda(r#"{"length":"5:12"}"#)), Some(312));
        assert_eq!(durata_di(&scheda(r#"{"length":""}"#)), None);
        assert_eq!(durata_di(&scheda("{}")), None);
    }

    #[test]
    fn dei_due_formati_dello_stesso_brano_resta_il_migliore() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [
                {"name":"gd77t01.mp3","format":"VBR MP3","title":"Sugaree","length":"300"},
                {"name":"gd77t01.flac","format":"Flac","title":"Sugaree","length":"300"}
              ]
            }"#,
        );
        let trovati = brani_dalla_scheda("gd77", &s, None);
        assert_eq!(trovati.len(), 1);
        assert!(trovati.first().is_some_and(|c| c.url.ends_with(".flac")));
        assert_eq!(
            trovati.first().and_then(|c| c.estensione.clone()),
            Some("flac".to_owned())
        );
    }

    #[test]
    fn i_file_che_non_sappiamo_aprire_non_diventano_candidati() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"x","collection":["etree"]},
              "files": [
                {"name":"cover.jpg","format":"JPEG","title":"copertina"},
                {"name":"x_meta.xml","format":"Metadata"},
                {"name":"t01.shn","format":"Shorten","title":"Sugaree"}
              ]
            }"#,
        );
        assert!(brani_dalla_scheda("x", &s, None).is_empty());
    }

    #[test]
    fn dentro_una_collezione_di_concerti_tutto_e_dal_vivo() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [{"name":"t01.flac","format":"Flac","title":"Sugaree","length":"300"}]
            }"#,
        );
        let trovati = brani_dalla_scheda("gd77", &s, None);
        assert_eq!(trovati.first().map(|c| c.natura), Some(Natura::DalVivo));
    }

    #[test]
    fn cercando_un_brano_gli_altri_dellitem_non_arrivano() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [
                {"name":"t01.flac","format":"Flac","title":"Sugaree","length":"300"},
                {"name":"t02.flac","format":"Flac","title":"Jack Straw","length":"280"}
              ]
            }"#,
        );
        let atteso = BranoEsterno {
            title: "Sugaree".to_owned(),
            ..BranoEsterno::default()
        };
        let trovati = brani_dalla_scheda("gd77", &s, Some(&atteso));
        assert_eq!(trovati.len(), 1);
        assert_eq!(trovati.first().map(|c| c.titolo.as_str()), Some("Sugaree"));
    }

    #[test]
    fn senza_titolo_si_ripiega_sul_nome_del_file() {
        assert_eq!(
            nome_pulito("dir/gd1977-05-08_d1t04.flac"),
            "gd1977 05 08 d1t04"
        );
        assert_eq!(nome_pulito("Sugaree.mp3"), "Sugaree");
    }

    #[test]
    fn la_frase_non_puo_rompere_la_query() {
        assert_eq!(frase(r#"Say "Hello""#), "\"Say  Hello\"");
        assert_eq!(frase("Karma Police"), "\"Karma Police\"");
    }

    #[test]
    fn smettere_presto_vuole_una_durata_da_confrontare() {
        let senza = BranoEsterno {
            title: "Sugaree".to_owned(),
            ..BranoEsterno::default()
        };
        let con = BranoEsterno {
            duration_ms: Some(300_000),
            ..senza.clone()
        };
        let candidati = [Candidato {
            titolo: "Sugaree".to_owned(),
            durata_sec: Some(300),
            ..Candidato::default()
        }];
        assert!(!primo_sicuro(&candidati, &senza));
        assert!(primo_sicuro(&candidati, &con));
    }
}
