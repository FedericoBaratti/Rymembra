//! Il documento di un dispositivo: cosa contiene, come si scrive, come si rilegge.
//!
//! # La busta sta fuori dal contenuto
//!
//! `device` e `generatedAt` non stanno dentro `content`, ed è la stessa scelta —
//! per la stessa ragione — di `aether_app::backup::Salvataggio`: l'impronta si
//! calcola sul solo contenuto, così due passate che non hanno cambiato niente
//! producono la stessa impronta e la seconda non carica niente. Se la data
//! stesse dentro, ogni passata riscriverebbe il file solo per aver guardato
//! l'orologio.
//!
//! # La versione sta nel nome del file
//!
//! `aether-<id>.v1.json.gz`, non solo `version: 1` dentro. Il giorno in cui il
//! formato cambiasse in modo non leggibile all'indietro, una versione vecchia di
//! Aether continuerebbe a trovare e a scrivere il **suo** `.v1` invece di
//! litigare con un file che non capisce, e i due Aether convivrebbero sulla
//! stessa cartella senza rovinarsi a vicenda. È la stessa scelta di
//! `NOME_LIBRERIA` in `aether-cloud`.
//!
//! # Indulgente sui record, intransigente sulla versione
//!
//! Un voto illeggibile fa sparire quel voto, non il documento: `filter_map`
//! invece di `collect` su un `Result`, esattamente come in `backup::interpreta`.
//! Una versione **maggiore** invece è un rifiuto netto, perché è l'unico caso in
//! cui leggere male porterebbe a scrivere sopra il lavoro di un Aether più
//! nuovo — cioè la sola perdita di dati vera che questo modulo può causare.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};

use aether_domain::errors::{AppError, ErrorCode};

use crate::contatore::Contatore;
use crate::registro::{Interruttore, Momento, Registro, Scelta, Voto, fondi_istanti, fondi_mappa};
use crate::sequenza::Sequenza;

/// La versione del formato scritta nei documenti nuovi.
pub const VERSIONE: u32 = 1;

/// Il nome del servizio nei codici d'errore.
const SERVIZIO: &str = "sincronia";

/// La cartella, dentro la radice, in cui stanno i documenti.
pub const CARTELLA: &str = "dispositivi";

/// Il prefisso del nome di un documento.
const PREFISSO: &str = "aether-";

/// Il suffisso del nome di un documento.
const SUFFISSO: &str = ".v1.json.gz";

/// Quanto si accetta che un documento compresso diventi una volta aperto.
///
/// Sessantaquattro mebibyte: molto più di qualunque libreria vera, molto meno di
/// quel che serve a una bomba di decompressione per riempire la memoria. Il file
/// viene da una cartella condivisa, e «l'abbiamo scritto noi» non è una garanzia
/// su cosa ci sia dentro adesso.
const LIMITE_DECOMPRESSO: u64 = 64 * 1024 * 1024;

/// Il nome del documento di un dispositivo.
#[must_use]
pub fn nome_file(dispositivo: &str) -> String {
    format!("{PREFISSO}{dispositivo}{SUFFISSO}")
}

/// Di quale dispositivo è questo file, se è un documento.
#[must_use]
pub fn dispositivo_da_nome(nome: &str) -> Option<&str> {
    nome.strip_prefix(PREFISSO)?.strip_suffix(SUFFISSO)
}

/// Le cancellazioni, perché possano viaggiare.
///
/// Senza, l'altro dispositivo vedrebbe soltanto «a me manca una playlist» e la
/// rimanderebbe indietro: la cosa cancellata tornerebbe da sola, che è il modo
/// più rapido di far perdere fiducia in una sincronia.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Lapidi {
    /// I brani tolti dalla libreria.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub brani: BTreeMap<String, i64>,
    /// Le playlist cancellate.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub playlist: BTreeMap<String, i64>,
}

impl Registro for Lapidi {
    fn fondi(&self, altro: &Self) -> Self {
        Self {
            brani: fondi_istanti(&self.brani, &altro.brani),
            playlist: fondi_istanti(&self.playlist, &altro.playlist),
        }
    }
}

/// Una playlist come la vede la sincronia.
///
/// I dati descrittivi si risolvono scegliendo un vincitore — `at` decide, come già
/// fa `updated_at` oggi — mentre i membri e il loro ordine si **fondono**, perché
/// per quelli un vincitore solo perde il lavoro dell'altro. È l'unica cosa che
/// questa struttura fa diversamente da `PlaylistState`, ed è tutto il punto.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaylistSincronizzata {
    /// Il nome come si scrive.
    pub nome: String,
    /// La descrizione, se c'è.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descrizione: Option<String>,
    /// Quando è stata creata, in millisecondi.
    #[serde(default)]
    pub creata_at: i64,
    /// Quando i dati descrittivi sono stati toccati l'ultima volta.
    #[serde(default)]
    pub at: i64,
    /// È automatica: l'appartenenza la decidono le regole.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub smart: bool,
    /// Le regole, per una playlist automatica.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regole: Option<String>,
    /// I membri e il loro ordine.
    ///
    /// Vuota per una playlist automatica: ogni dispositivo la ricalcola dalle
    /// regole, ed è per questo che resta vera anche sui brani che l'altro non ha.
    #[serde(default, skip_serializing_if = "Sequenza::e_vuota")]
    pub sequenza: Sequenza,
}

impl Registro for PlaylistSincronizzata {
    fn fondi(&self, altro: &Self) -> Self {
        // I membri si fondono sempre, chiunque abbia toccato i dati per ultimo.
        let sequenza = self.sequenza.fondi(&altro.sequenza);
        // La creazione è un fatto: la più antica delle due è quella vera. Uno zero
        // vuol dire «non si sa» e non «all'inizio dei tempi».
        let creata_at = match (self.creata_at, altro.creata_at) {
            (0, altra) | (altra, 0) => altra,
            (mia, altra) => mia.min(altra),
        };
        let mut vincitore = match self.at.cmp(&altro.at) {
            std::cmp::Ordering::Greater => self.clone(),
            std::cmp::Ordering::Less => altro.clone(),
            // A parità esatta serve una regola qualsiasi purché sia la stessa su
            // tutti i dispositivi: l'ordine di due nomi non dipende da chi guarda.
            std::cmp::Ordering::Equal => {
                if self.nome >= altro.nome {
                    self.clone()
                } else {
                    altro.clone()
                }
            }
        };
        vincitore.sequenza = sequenza;
        vincitore.creata_at = creata_at;
        vincitore
    }
}

/// Quel che un dispositivo ha da dire, senza la busta.
///
/// Ogni mappa è una [`BTreeMap`]: non è pignoleria, è ciò che rende `serde_json`
/// deterministico per costruzione e quindi [`impronta`] confrontabile fra due
/// dispositivi. Un passaggio a `HashMap` romperebbe in silenzio la scorciatoia
/// che evita di riscrivere il documento a ogni passata — un test lo sorveglia.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Contenuto {
    /// Quante volte **questo** dispositivo ha ascoltato ciascun brano.
    ///
    /// Solo il proprio: il totale lo fa la fusione sommando quelli di tutti. Vedi
    /// [`crate::contatore`] sul perché non è un numero solo.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ascolti: BTreeMap<String, i64>,
    /// Lo storico ereditato: quel che era già contato prima che ci fosse una
    /// sincronia.
    ///
    /// Sta a parte dagli ascolti propri perché non appartiene a questo
    /// dispositivo: appartiene allo pseudo-dispositivo `importazione`, che è lo
    /// stesso su tutti. Ne segue la proprietà che serve — due dispositivi che
    /// hanno importato **lo stesso** vecchio database convergono sul massimo,
    /// non sulla somma. Metterlo negli ascolti propri raddoppierebbe la storia di
    /// ogni brano al primo incontro fra i due, e nessuno saprebbe più da dove sia
    /// venuto il numero.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ereditati: BTreeMap<String, i64>,
    /// L'ultimo ascolto di ciascun brano, in millisecondi.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ultimo: BTreeMap<String, i64>,
    /// I voti.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub voti: BTreeMap<String, Voto>,
    /// I preferiti.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preferiti: BTreeMap<String, Interruttore>,
    /// Dove si era arrivati, per i brani lasciati a metà.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub posizioni: BTreeMap<String, Momento>,
    /// Le playlist.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub playlist: BTreeMap<String, PlaylistSincronizzata>,
    /// Le cancellazioni.
    #[serde(default)]
    pub lapidi: Lapidi,
    /// Le cartelle sorvegliate.
    ///
    /// Una mappa e non un elenco: una cartella tolta su un dispositivo deve
    /// restare tolta, e un elenco non ha modo di dirlo — l'unione di due elenchi
    /// rimette dentro tutto quel che qualcuno ha mai sorvegliato.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cartelle: BTreeMap<String, Interruttore>,
    /// Le skin installate: identificativo → impronta del pacchetto.
    ///
    /// I byte stanno in un file a parte: un `.aeskin` pesa fino a venti megabyte e
    /// non cambia quasi mai, mentre questo documento cambia a ogni cuoricino.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skin: BTreeMap<String, String>,
    /// Le bozze dello Studio: identificativo → impronta del `skin.json`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bozze: BTreeMap<String, String>,
    /// La skin attiva.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin_attiva: Option<Scelta>,
}

impl Registro for Contenuto {
    fn fondi(&self, altro: &Self) -> Self {
        Self {
            // Gli ascolti no: quelli di due dispositivi diversi non si fondono a
            // due a due, si sommano tutti insieme. Vedi `fusione::fondi`, che è
            // l'unico posto che li sa mettere insieme senza sbagliare. Qui si
            // tiene il proprio, che è l'unico che questo documento possiede.
            ascolti: self.ascolti.clone(),
            // Lo storico ereditato invece sì, e col massimo: è di un dispositivo
            // solo — `importazione` — e il massimo è il join giusto per un
            // contatore che non deve raddoppiare quando la stessa importazione si
            // rifà. È la regola che `merge_stats` applica da sempre.
            ereditati: massimi(&self.ereditati, &altro.ereditati),
            ultimo: fondi_istanti(&self.ultimo, &altro.ultimo),
            voti: fondi_mappa(&self.voti, &altro.voti),
            preferiti: fondi_mappa(&self.preferiti, &altro.preferiti),
            posizioni: fondi_mappa(&self.posizioni, &altro.posizioni),
            playlist: fondi_mappa(&self.playlist, &altro.playlist),
            lapidi: self.lapidi.fondi(&altro.lapidi),
            cartelle: fondi_mappa(&self.cartelle, &altro.cartelle),
            skin: unisci_testi(&self.skin, &altro.skin),
            bozze: unisci_testi(&self.bozze, &altro.bozze),
            skin_attiva: match (&self.skin_attiva, &altro.skin_attiva) {
                (Some(mia), Some(altra)) => Some(mia.fondi(altra)),
                (Some(una), None) | (None, Some(una)) => Some(una.clone()),
                (None, None) => None,
            },
        }
    }
}

/// Unisce due mappe di conteggi tenendo il massimo di ciascuna chiave.
fn massimi(a: &BTreeMap<String, i64>, b: &BTreeMap<String, i64>) -> BTreeMap<String, i64> {
    let mut unito = a.clone();
    for (chiave, quanti) in b {
        unito
            .entry(chiave.clone())
            .and_modify(|gia| *gia = (*gia).max(*quanti))
            .or_insert(*quanti);
    }
    unito
}

/// Unisce due inventari, tenendo l'impronta locale quando c'è.
///
/// Un inventario dice «ho questa skin, e i suoi byte hanno quest'impronta». Due
/// impronte diverse per lo stesso identificativo vogliono dire che i pacchetti
/// sono davvero due, e chi scarica se ne accorgerà; qui si tiene la propria,
/// perché è l'unica di cui si sa che corrisponde a un file che c'è.
fn unisci_testi(
    a: &BTreeMap<String, String>,
    b: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut unito = b.clone();
    for (chiave, valore) in a {
        unito.insert(chiave.clone(), valore.clone());
    }
    unito
}

/// Il documento intero: la busta e il contenuto.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Documento {
    /// La versione del formato. Vedi [`VERSIONE`].
    pub version: u32,
    /// Di quale dispositivo è.
    pub device: String,
    /// Quando è stato scritto, in millisecondi.
    #[serde(rename = "generatedAt")]
    pub generated_at: i64,
    /// Il contenuto.
    pub content: Contenuto,
}

impl Documento {
    /// Un documento vuoto per un dispositivo.
    #[must_use]
    pub fn vuoto(dispositivo: &str) -> Self {
        Self {
            version: VERSIONE,
            device: dispositivo.to_owned(),
            generated_at: 0,
            content: Contenuto::default(),
        }
    }

    /// Mette un contenuto nella busta.
    #[must_use]
    pub fn nuovo(dispositivo: &str, content: Contenuto, generated_at: i64) -> Self {
        Self {
            version: VERSIONE,
            device: dispositivo.to_owned(),
            generated_at,
            content,
        }
    }

    /// Il nome del file di questo documento.
    #[must_use]
    pub fn nome_file(&self) -> String {
        nome_file(&self.device)
    }
}

/// I byte canonici del solo contenuto: è su questi che si calcola l'impronta.
///
/// # Errori
///
/// `internal.unexpected` se la serializzazione fallisce, cosa che per questi tipi
/// non può accadere — ma un'impronta sbagliata farebbe **saltare** una scrittura,
/// cioè perdere in silenzio quel che si era appena ascoltato, e non è un rischio
/// da nascondere dietro un valore di ripiego.
pub fn canonico(contenuto: &Contenuto) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec(contenuto).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione del contenuto della sincronia".to_owned()),
        })
        .with_cause(err.to_string())
    })
}

/// L'impronta di un contenuto.
///
/// La stessa funzione di `aether_app::backup::impronta`, e per la stessa ragione:
/// la domanda è «questi byte sono già lassù?», e due risposte diverse alla stessa
/// domanda sullo stesso file sarebbero un file che si ricarica per sempre.
///
/// # Errori
///
/// Quelli di [`canonico`].
pub fn impronta(contenuto: &Contenuto) -> Result<String, AppError> {
    Ok(blake3::hash(&canonico(contenuto)?).to_hex().to_string())
}

/// Il documento pronto da scrivere: JSON compresso.
///
/// # Errori
///
/// `internal.unexpected` se la serializzazione o la compressione falliscono.
pub fn serializza(documento: &Documento) -> Result<Vec<u8>, AppError> {
    let crudo = serde_json::to_vec(documento).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione del documento di sincronia".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    let mut compressore = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    compressore
        .write_all(&crudo)
        .and_then(|()| compressore.finish())
        .map_err(|err| {
            AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("compressione del documento di sincronia".to_owned()),
            })
            .with_cause(err.to_string())
        })
}

/// Apre un documento compresso.
///
/// # Errori
///
/// `sync.remoteCorrupt` se i byte non sono un gzip leggibile, o se una volta
/// aperti superano [`LIMITE_DECOMPRESSO`].
fn decomprimi(compresso: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut aperto = Vec::new();
    flate2::read::GzDecoder::new(compresso)
        .take(LIMITE_DECOMPRESSO)
        .read_to_end(&mut aperto)
        .map_err(|err| {
            AppError::new(ErrorCode::SyncRemoteCorrupt)
                .with_cause(format!("documento non decomprimibile: {err}"))
        })?;
    if u64::try_from(aperto.len()).unwrap_or(u64::MAX) >= LIMITE_DECOMPRESSO {
        return Err(AppError::new(ErrorCode::SyncRemoteCorrupt)
            .with_cause("documento troppo grande una volta aperto"));
    }
    Ok(aperto)
}

/// Legge un documento, buttando via i singoli record illeggibili.
///
/// # Errori
///
/// - `sync.remoteCorrupt` se il gzip o il JSON non si interpretano, o se non c'è
///   un `content` dentro: non c'è niente da leggere, e chi chiama può mettere il
///   file da parte.
/// - `net.badSchema` se la versione è **più alta** di [`VERSIONE`]. Un documento
///   scritto da un Aether più nuovo non è corrotto, e chi chiama non deve
///   sovrascriverlo.
pub fn interpreta(compresso: &[u8]) -> Result<Documento, AppError> {
    let crudo = decomprimi(compresso)?;
    let radice: serde_json::Value = serde_json::from_slice(&crudo).map_err(|err| {
        AppError::new(ErrorCode::SyncRemoteCorrupt).with_cause(format!("json illeggibile: {err}"))
    })?;

    let version = radice
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::from(VERSIONE));
    if version > u64::from(VERSIONE) {
        return Err(AppError::new(ErrorCode::NetBadSchema {
            service: Some(SERVIZIO.to_owned()),
            detail: Some(format!(
                "il documento è di versione {version}, questa applicazione legge la {VERSIONE}"
            )),
        }));
    }

    let Some(contenuto) = radice.get("content").filter(|valore| valore.is_object()) else {
        return Err(AppError::new(ErrorCode::SyncRemoteCorrupt)
            .with_cause("il documento non ha un contenuto leggibile"));
    };

    Ok(Documento {
        version: u32::try_from(version).unwrap_or(VERSIONE),
        device: radice
            .get("device")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        generated_at: radice
            .get("generatedAt")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        content: Contenuto {
            ascolti: istanti(contenuto.get("ascolti")),
            ereditati: istanti(contenuto.get("ereditati")),
            ultimo: istanti(contenuto.get("ultimo")),
            voti: mappa(contenuto.get("voti")),
            preferiti: mappa(contenuto.get("preferiti")),
            posizioni: mappa(contenuto.get("posizioni")),
            playlist: mappa(contenuto.get("playlist")),
            lapidi: Lapidi {
                brani: istanti(contenuto.pointer("/lapidi/brani")),
                playlist: istanti(contenuto.pointer("/lapidi/playlist")),
            },
            cartelle: mappa(contenuto.get("cartelle")),
            skin: testi(contenuto.get("skin")),
            bozze: testi(contenuto.get("bozze")),
            skin_attiva: contenuto
                .get("skin_attiva")
                .and_then(|valore| serde_json::from_value(valore.clone()).ok()),
        },
    })
}

/// Una mappa di record, saltando quelli che non si interpretano.
///
/// È la regola d'indulgenza in una riga: `filter_map` invece di `collect` su un
/// `Result`. Un campo mancante fa sparire **quella** voce, non il documento.
fn mappa<V: serde::de::DeserializeOwned>(
    valore: Option<&serde_json::Value>,
) -> BTreeMap<String, V> {
    valore
        .and_then(serde_json::Value::as_object)
        .map(|oggetto| {
            oggetto
                .iter()
                .filter_map(|(chiave, voce)| {
                    serde_json::from_value(voce.clone())
                        .ok()
                        .map(|valore| (chiave.clone(), valore))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Una mappa da chiave a millisecondi, saltando le voci che non lo sono.
fn istanti(valore: Option<&serde_json::Value>) -> BTreeMap<String, i64> {
    valore
        .and_then(serde_json::Value::as_object)
        .map(|oggetto| {
            oggetto
                .iter()
                .filter_map(|(chiave, quando)| {
                    quando.as_i64().map(|quando| (chiave.clone(), quando))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Una mappa da chiave a testo, saltando le voci che non lo sono.
fn testi(valore: Option<&serde_json::Value>) -> BTreeMap<String, String> {
    valore
        .and_then(serde_json::Value::as_object)
        .map(|oggetto| {
            oggetto
                .iter()
                .filter_map(|(chiave, testo)| {
                    testo
                        .as_str()
                        .map(|testo| (chiave.clone(), testo.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Il contatore d'ascolti di un brano, ricavato da un documento solo.
///
/// Serve alla fusione, che li mette insieme; da qui esce sempre un contatore con
/// un dispositivo dentro, o vuoto.
#[must_use]
pub fn contatore_di(documento: &Documento, brano: &str) -> Contatore {
    let mut contatore = Contatore::di_uno(
        &documento.device,
        documento.content.ascolti.get(brano).copied().unwrap_or(0),
    );
    if let Some(ereditati) = documento.content.ereditati.get(brano) {
        contatore.segna(crate::contatore::IMPORTAZIONE, *ereditati);
    }
    contatore
}

#[cfg(test)]
mod prove {
    use super::*;

    fn con_un_voto(dispositivo: &str, brano: &str, voto: Voto) -> Documento {
        let mut contenuto = Contenuto::default();
        contenuto.voti.insert(brano.to_owned(), voto);
        Documento::nuovo(dispositivo, contenuto, 1_000)
    }

    #[test]
    fn un_documento_fa_il_giro() {
        let documento = con_un_voto(
            "portatile",
            "miles|so what|kind of blue",
            Voto { v: 5, at: 100 },
        );
        let byte = serializza(&documento).expect("serializza");
        let riletto = interpreta(&byte).expect("interpreta");
        assert_eq!(riletto, documento);
    }

    #[test]
    fn il_nome_del_file_fa_il_giro() {
        assert_eq!(nome_file("aBc-123_"), "aether-aBc-123_.v1.json.gz");
        assert_eq!(
            dispositivo_da_nome(&nome_file("aBc-123_")),
            Some("aBc-123_")
        );
        assert_eq!(dispositivo_da_nome("aether-skin-notte.aeskin"), None);
        assert_eq!(dispositivo_da_nome("qualcosa.txt"), None);
    }

    #[test]
    fn l_impronta_non_dipende_dalla_busta() {
        // È tutto il punto: due passate che non hanno cambiato niente devono dare
        // la stessa impronta, o si riscrive il file solo per aver guardato
        // l'orologio.
        let uno = con_un_voto("portatile", "a", Voto { v: 3, at: 10 });
        let mut due = con_un_voto("telefono", "a", Voto { v: 3, at: 10 });
        due.generated_at = 999_999;
        assert_eq!(
            impronta(&uno.content).expect("impronta"),
            impronta(&due.content).expect("impronta")
        );
    }

    #[test]
    fn una_voce_illeggibile_non_porta_via_il_documento() {
        let crudo = br#"{
            "version": 1, "device": "uno", "generatedAt": 5,
            "content": {
                "voti": { "buono": {"v": 4, "at": 10}, "rotto": "non un voto" },
                "ultimo": { "buono": 500, "rotto": "nemmeno questo" }
            }
        }"#;
        let mut compressore =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        compressore.write_all(crudo).expect("comprime");
        let byte = compressore.finish().expect("chiude");

        let documento = interpreta(&byte).expect("interpreta");
        assert_eq!(documento.content.voti.len(), 1, "il voto buono resta");
        assert_eq!(documento.content.ultimo.len(), 1);
    }

    #[test]
    fn una_versione_dal_futuro_si_rifiuta_invece_di_sovrascriverla() {
        let crudo = br#"{"version": 99, "device": "uno", "content": {}}"#;
        let mut compressore =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        compressore.write_all(crudo).expect("comprime");
        let byte = compressore.finish().expect("chiude");

        let errore = interpreta(&byte).expect_err("deve rifiutare");
        assert!(
            matches!(errore.code(), ErrorCode::NetBadSchema { .. }),
            "non è corrotto: è più nuovo di noi, e non va sovrascritto"
        );
    }

    #[test]
    fn byte_che_non_sono_un_gzip_sono_un_guasto_dichiarato() {
        let errore = interpreta(b"non sono un gzip").expect_err("deve rifiutare");
        assert_eq!(*errore.code(), ErrorCode::SyncRemoteCorrupt);
    }

    #[test]
    fn i_dati_descrittivi_di_una_playlist_li_decide_l_orologio_ma_i_membri_si_fondono() {
        let base = Sequenza::dalla_lista("comune", &["a".to_owned(), "b".to_owned()]);
        let mut mia = PlaylistSincronizzata {
            nome: "Vecchio nome".to_owned(),
            at: 10,
            creata_at: 5,
            sequenza: base.clone(),
            ..PlaylistSincronizzata::default()
        };
        mia.sequenza.accoda("portatile", "so-what");

        let mut sua = PlaylistSincronizzata {
            nome: "Nome nuovo".to_owned(),
            at: 20,
            creata_at: 5,
            sequenza: base,
            ..PlaylistSincronizzata::default()
        };
        sua.sequenza.accoda("telefono", "blue-in-green");

        let fusa = mia.fondi(&sua);
        assert_eq!(fusa.nome, "Nome nuovo", "il nome lo decide l'orologio");
        let ordine = fusa.sequenza.ordine();
        assert!(ordine.contains(&"so-what"), "i membri si fondono");
        assert!(ordine.contains(&"blue-in-green"));
        assert_eq!(sua.fondi(&mia).sequenza.ordine(), ordine);
    }

    #[test]
    fn la_creazione_piu_antica_e_quella_vera() {
        let vecchia = PlaylistSincronizzata {
            creata_at: 100,
            at: 10,
            ..PlaylistSincronizzata::default()
        };
        let senza_data = PlaylistSincronizzata {
            creata_at: 0,
            at: 20,
            ..PlaylistSincronizzata::default()
        };
        assert_eq!(vecchia.fondi(&senza_data).creata_at, 100);
        assert_eq!(senza_data.fondi(&vecchia).creata_at, 100);
    }

    #[test]
    fn una_cartella_tolta_resta_tolta() {
        // Con un elenco al posto di una mappa, l'unione rimetterebbe dentro tutto
        // quel che qualcuno ha mai sorvegliato.
        let mut mio = Contenuto::default();
        mio.cartelle
            .insert("D:/Musica".to_owned(), Interruttore { on: true, at: 10 });
        let mut suo = Contenuto::default();
        suo.cartelle
            .insert("D:/Musica".to_owned(), Interruttore { on: false, at: 20 });
        let fuso = mio.fondi(&suo);
        assert_eq!(fuso.cartelle.get("D:/Musica").map(|c| c.on), Some(false));
    }

    #[test]
    fn le_mappe_restano_ordinate_e_l_impronta_e_stabile() {
        let mut contenuto = Contenuto::default();
        for chiave in ["zeta", "alfa", "mu"] {
            contenuto
                .voti
                .insert(chiave.to_owned(), Voto { v: 1, at: 1 });
        }
        let prima = impronta(&contenuto).expect("impronta");
        let testo = String::from_utf8(canonico(&contenuto).expect("canonico")).expect("utf8");
        assert!(
            testo.find("alfa") < testo.find("mu") && testo.find("mu") < testo.find("zeta"),
            "le chiavi devono uscire ordinate, o l'impronta cambia da sola: {testo}"
        );
        assert_eq!(prima, impronta(&contenuto).expect("impronta"));
    }
}
