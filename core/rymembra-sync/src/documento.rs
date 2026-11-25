//! Il documento di un dispositivo: cosa contiene, come si scrive, come si rilegge.
//!
//! # La busta sta fuori dal contenuto
//!
//! `device` e `generatedAt` non stanno dentro `content`, ed è la stessa scelta —
//! per la stessa ragione — di `rymembra_app::backup::Salvataggio`: l'impronta si
//! calcola sul solo contenuto, così due passate che non hanno cambiato niente
//! producono la stessa impronta e la seconda non carica niente. Se la data
//! stesse dentro, ogni passata riscriverebbe il file solo per aver guardato
//! l'orologio.
//!
//! # La versione sta nel nome del file
//!
//! `rymembra-<id>.v1.json.gz`, non solo `version: 1` dentro. Il giorno in cui il
//! formato cambiasse in modo non leggibile all'indietro, una versione vecchia di
//! Rymembra continuerebbe a trovare e a scrivere il **suo** `.v1` invece di
//! litigare con un file che non capisce, e i due Rymembra convivrebbero sulla
//! stessa cartella senza rovinarsi a vicenda. È la stessa scelta di
//! `NOME_LIBRERIA` in `rymembra-cloud`.
//!
//! # Il nome di prima si legge, ma non si scrive
//!
//! I documenti scritti prima del cambio di nome al programma hanno il prefisso
//! di allora ([`eredita::PREFISSO_DISPOSITIVO`]). [`dispositivo_da_nome`] li
//! riconosce ancora, perché un dispositivo che li ignorasse non vedrebbe più gli
//! ascolti degli altri; [`nome_file`] invece scrive sempre quello nuovo. Il
//! motore porta il **proprio** documento dal nome di prima a quello di adesso;
//! quelli altrui li rinominano i loro dispositivi, quando si aggiornano.
//!
//! # Indulgente sui record, intransigente sulla versione
//!
//! Un voto illeggibile fa sparire quel voto, non il documento: `filter_map`
//! invece di `collect` su un `Result`, esattamente come in `backup::interpreta`.
//! Una versione **maggiore** invece è un rifiuto netto, perché è l'unico caso in
//! cui leggere male porterebbe a scrivere sopra il lavoro di un Rymembra più
//! nuovo — cioè la sola perdita di dati vera che questo modulo può causare.
//!
//! I record leggibili ma **fuori dominio** — sei stelle, un conteggio presso
//! `i64::MAX`, una data fra dieci anni — non si scartano qui ma in
//! [`crate::dominio::nel_dominio`], che chi fonde chiama con il proprio
//! «adesso»: il futuro dipende da quando lo si guarda, e un documento letto una
//! volta resta in memoria per molte passate.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};

use rymembra_domain::eredita;
use rymembra_domain::errors::{AppError, ErrorCode};

use crate::contatore::Contatore;
use crate::registro::{Interruttore, Momento, Registro, Scelta, Voto, fondi_istanti, fondi_mappa};
use crate::sequenza::Sequenza;

/// La versione del formato scritta nei documenti nuovi.
pub const VERSIONE: u32 = 1;

/// Il segno con cui un dispositivo dichiara, nel proprio documento, che **sa
/// lasciare le voci di playlist dei brani che qui mancano**: se una playlist ha
/// brani che la sua libreria non ha, non li seppellisce con una lapide.
///
/// # Perché esiste
///
/// La 3.2.0 quei brani li seppelliva, e il formato non dice con che versione
/// è stato scritto un documento: `version` vale 1 per tutte e due, e alzarla
/// non serve, perché la 3.2.0 rifiuterebbe di *leggere* i documenti nuovi ma
/// continuerebbe a *scrivere* i suoi. Senza un segno nessuna 3.3.x potrebbe
/// mai distinguere un pari 3.3.0 da un PC rimasto sulla 3.2.0, e un filtro
/// futuro delle lapidi dovrebbe trattare tutti come vecchi. Il segno è
/// irreversibile — dopo il rilascio non si può più mettere — e per questo c'è
/// già, anche se oggi nessun codice lo legge: chi lo guarderà sarà una 3.3.x.
///
/// # Dove sta, e perché
///
/// **Dentro `content`**, non nella busta: l'impronta si calcola sul solo
/// contenuto ([`impronta`]), e un segno nella busta lascerebbe un dispositivo
/// appena aggiornato e fermo a pubblicare un documento senza segno finché non
/// cambia un ascolto. Dentro, l'impronta cambia, e ogni dispositivo riscrive il
/// proprio una volta sola al primo giro dopo l'aggiornamento.
///
/// La 3.2.0 lo ignora: [`interpreta`] legge per puntatore, campo per campo, e
/// non conosce questo.
pub const CAPACITA_VOCI_ASSENTI: &str = "voci-assenti-1";

/// Quanti segni si leggono da un documento altrui: sedici.
///
/// I documenti degli altri non sono fidati, e questo campo si fonde (si
/// uniscono gli insiemi) e si tiene in memoria per ogni pari: senza un tetto
/// un documento potrebbe riempirlo di milioni di stringhe dentro i limiti di
/// [`MASSIMO_VALORI`].
const MASSIMO_CAPACITA: usize = 16;

/// Quanto può essere lungo un segno che si legge: sessantaquattro byte.
const LUNGHEZZA_MASSIMA_CAPACITA: usize = 64;

/// I segni che questa versione scrive nel proprio documento.
#[must_use]
pub fn capacita_di_questa_versione() -> BTreeSet<String> {
    BTreeSet::from([CAPACITA_VOCI_ASSENTI.to_owned()])
}

/// Il nome del servizio nei codici d'errore.
const SERVIZIO: &str = "sincronia";

/// La cartella, dentro la radice, in cui stanno i documenti.
pub const CARTELLA: &str = "dispositivi";

/// Il prefisso del nome di un documento.
const PREFISSO: &str = "rymembra-";

/// Il suffisso del nome di un documento.
const SUFFISSO: &str = ".v1.json.gz";

/// Quanto si accetta che un documento compresso diventi una volta aperto.
///
/// Sessantaquattro mebibyte: molto più di qualunque libreria vera, molto meno di
/// quel che serve a una bomba di decompressione per riempire la memoria. Il file
/// viene da una cartella condivisa, e «l'abbiamo scritto noi» non è una garanzia
/// su cosa ci sia dentro adesso.
const LIMITE_DECOMPRESSO: u64 = 64 * 1024 * 1024;

/// Quanti valori JSON si accetta che un documento contenga: quattro milioni.
///
/// Il tetto sui byte aperti non basta, perché la memoria che conta è quella
/// dell'albero: un `serde_json::Value` costa una trentina di byte per valore,
/// più la crescita a raddoppi dei suoi vettori, e `[0,0,…]` sono due byte di
/// testo per valore. Misurato: quaranta volte il testo, cioè ~2,6 GB al tetto
/// dei 64 MiB, da 64 KB di gzip. I valori si contano **prima** di costruire
/// l'albero ([`valori_al_piu`]), e oltre il tetto il documento si rifiuta.
///
/// Quattro milioni sono qualche centinaio di megabyte nel caso peggiore, e
/// stanno larghi attorno a una libreria vera: centomila brani con ascolti,
/// ultimi ascolti e voti fanno mezzo milione di valori, e anche le sequenze di
/// cento playlist da duemila brani con le loro lapidi restano sotto i due
/// milioni.
const MASSIMO_VALORI: usize = 4_000_000;

/// Il nome del documento di un dispositivo.
///
/// Sempre col prefisso di adesso: il nome di prima si legge ancora
/// ([`dispositivo_da_nome`]) ma non si scrive più.
#[must_use]
pub fn nome_file(dispositivo: &str) -> String {
    format!("{PREFISSO}{dispositivo}{SUFFISSO}")
}

/// Di quale dispositivo è questo file, se è un documento.
///
/// Riconosce il prefisso di adesso e anche quello di prima
/// ([`eredita::PREFISSO_DISPOSITIVO`]): i documenti scritti prima del cambio di
/// nome stanno ancora nella cartella condivisa, e un dispositivo che non li
/// vedesse smetterebbe di contare gli ascolti di tutti gli altri. L'identificativo
/// del dispositivo non contiene il marchio, quindi è lo stesso sotto i due nomi.
///
/// L'identificativo [`RISERVATO`] non è di nessun dispositivo: è il file della
/// libreria del backup su Drive, che con la sincronia su Drive sta nella
/// stessa cartella dell'app e ha lo stesso prefisso e lo stesso suffisso.
#[must_use]
pub fn dispositivo_da_nome(nome: &str) -> Option<&str> {
    let resto = nome
        .strip_prefix(PREFISSO)
        .or_else(|| nome.strip_prefix(eredita::PREFISSO_DISPOSITIVO))?;
    resto.strip_suffix(SUFFISSO).filter(|id| *id != RISERVATO)
}

/// L'identificativo che nessun dispositivo può avere.
///
/// `rymembra-libreria.v1.json.gz` (e `aether-libreria.v1.json.gz`, il nome di
/// prima) è il file della libreria del backup, non un documento della
/// sincronia: letto come tale, a ogni passata dopo un backup cambiato ogni
/// dispositivo lo scaricava e lo interpretava come il documento del
/// dispositivo «libreria» (R08-12). Oggi non sporcava la fusione — il
/// contenuto risultava vuoto — ma bastava un campo in comune fra i due formati
/// perché il backup entrasse nella sincronia. Gli identificativi veri sono
/// casuali e non lo incontrano mai.
pub const RISERVATO: &str = "libreria";

/// Il file ha ancora il nome di prima?
///
/// Serve al motore per riconoscere, fra i documenti, quelli che andrebbero
/// rinominati: un file che [`dispositivo_da_nome`] riconosce ma che non ha il
/// prefisso di adesso.
#[must_use]
pub(crate) fn ha_il_nome_di_prima(nome: &str) -> bool {
    nome.starts_with(eredita::PREFISSO_DISPOSITIVO)
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
    /// I byte stanno in un file a parte: un `.ryskin` pesa fino a venti megabyte e
    /// non cambia quasi mai, mentre questo documento cambia a ogni cuoricino.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skin: BTreeMap<String, String>,
    /// Le bozze dello Studio: identificativo → impronta del `skin.json`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bozze: BTreeMap<String, String>,
    /// La skin attiva.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin_attiva: Option<Scelta>,
    /// Quel che il dispositivo che l'ha scritto sa fare: vedi
    /// [`CAPACITA_VOCI_ASSENTI`].
    ///
    /// Un documento senza questo campo — tutti quelli della 3.2.0 — lo porta
    /// vuoto. Non è un dato dell'utente e nessuna fusione lo usa: lo mette
    /// [`crate::Motore::passata`] sul documento che pubblica, e chi legge gli
    /// altri lo trova qui.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub capacita: BTreeSet<String>,
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
            capacita: self.capacita.union(&altro.capacita).cloned().collect(),
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

/// Unisce due inventari: l'unione degli identificativi, e a parità di
/// identificativo l'impronta più grande, come stringa.
///
/// Un inventario dice «ho questa skin, e i suoi byte hanno quest'impronta». Due
/// impronte diverse per lo stesso identificativo vogliono dire che i pacchetti
/// sono davvero due, e chi scarica se ne accorgerà. Quale tenere qui non lo
/// può decidere «la propria»: in [`crate::fusione::fondi`] questa funzione
/// riceve l'accumulato della piega e il documento dopo, non il proprio e
/// l'altrui, e una regola «tieni il primo» faceva vincere il documento che
/// l'elenco della cartella metteva in testa — un risultato che cambiava con
/// l'ordine. Il massimo è commutativo, associativo e idempotente come le
/// altre fusioni di questo crate, ed è la stessa regola con cui [`Scelta`]
/// rompe la parità.
fn unisci_testi(
    a: &BTreeMap<String, String>,
    b: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut unito = a.clone();
    for (chiave, valore) in b {
        unito
            .entry(chiave.clone())
            .and_modify(|gia| {
                if valore > gia {
                    gia.clone_from(valore);
                }
            })
            .or_insert_with(|| valore.clone());
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
/// La stessa funzione di `rymembra_app::backup::impronta`, e per la stessa ragione:
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
/// - `sync.remoteCorrupt` se il gzip o il JSON non si interpretano, se non c'è
///   un `content` dentro, o se porterebbe più di [`MASSIMO_VALORI`] valori:
///   non c'è niente da leggere, e chi chiama può mettere il file da parte.
/// - `net.badSchema` se la versione è **più alta** di [`VERSIONE`]. Un documento
///   scritto da un Rymembra più nuovo non è corrotto, e chi chiama non deve
///   sovrascriverlo.
pub fn interpreta(compresso: &[u8]) -> Result<Documento, AppError> {
    let crudo = decomprimi(compresso)?;
    let valori = valori_al_piu(&crudo);
    if valori > MASSIMO_VALORI {
        return Err(
            AppError::new(ErrorCode::SyncRemoteCorrupt).with_cause(format!(
                "il documento porterebbe fino a {valori} valori, oltre i {MASSIMO_VALORI} che si \
             accettano: aperto in memoria peserebbe troppo"
            )),
        );
    }
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
            capacita: segni(contenuto.get("capacita")),
        },
    })
}

/// I segni di capacità di un documento, quelli che sono testo ragionevole.
///
/// Si leggono anche se questa versione non li conosce: servono a chi guarda un
/// pari e vuole sapere cosa sa fare, e un segno nuovo di una 3.3.x non deve
/// sparire passando di qui. Ma con un tetto di numero e di lunghezza: vedi
/// [`MASSIMO_CAPACITA`].
fn segni(valore: Option<&serde_json::Value>) -> BTreeSet<String> {
    valore
        .and_then(serde_json::Value::as_array)
        .map(|elenco| {
            elenco
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|segno| !segno.is_empty() && segno.len() <= LUNGHEZZA_MASSIMA_CAPACITA)
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(MASSIMO_CAPACITA)
                .collect()
        })
        .unwrap_or_default()
}

/// Quanti valori JSON al più stanno in questo testo, senza costruire niente.
///
/// Ogni elemento di un vettore o di un oggetto, tranne il primo, è preceduto
/// da una virgola; i primi sono al più uno per contenitore; più la radice.
/// Quindi virgole + contenitori + 1, contati **fuori** dalle stringhe — una
/// virgola dentro un nome di brano non è un valore. Un conto per eccesso, in
/// un solo passaggio e senza allocare: su un testo che non è JSON sbaglia, ma
/// lì il parser fallisce comunque, e una stringa costa quanto i suoi byte.
fn valori_al_piu(testo: &[u8]) -> usize {
    let mut valori: usize = 1;
    let mut in_stringa = false;
    let mut dopo_barra = false;
    for &byte in testo {
        if in_stringa {
            if dopo_barra {
                dopo_barra = false;
            } else if byte == b'\\' {
                dopo_barra = true;
            } else if byte == b'"' {
                in_stringa = false;
            }
            continue;
        }
        match byte {
            b'"' => in_stringa = true,
            b',' | b'[' | b'{' => valori = valori.saturating_add(1),
            _ => {}
        }
    }
    valori
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
        assert_eq!(nome_file("aBc-123_"), "rymembra-aBc-123_.v1.json.gz");
        assert_eq!(
            dispositivo_da_nome(&nome_file("aBc-123_")),
            Some("aBc-123_")
        );
        assert_eq!(dispositivo_da_nome("rymembra-skin-notte.ryskin"), None);
        assert_eq!(dispositivo_da_nome("qualcosa.txt"), None);
    }

    #[test]
    fn nome_file_emette_il_nuovo_prefisso() {
        // Mai il nome di prima, qualunque sia l'identificativo: chi scrive con
        // quello lascia la cartella mezza sotto un nome e mezza sotto l'altro.
        for dispositivo in ["aBc-123_", "portatile", "x"] {
            let nome = nome_file(dispositivo);
            assert!(nome.starts_with("rymembra-"), "{nome}");
            assert!(!nome.starts_with(eredita::PREFISSO_DISPOSITIVO), "{nome}");
            assert!(!ha_il_nome_di_prima(&nome));
        }
        assert_eq!(Documento::vuoto("uno").nome_file(), nome_file("uno"));
    }

    #[test]
    fn dispositivo_da_nome_accetta_entrambi() {
        // Il nome di prima è scritto per esteso, e non preso dalla costante:
        // se un giorno la costante cambiasse per sbaglio, la prova se ne
        // accorgerebbe.
        assert_eq!(
            dispositivo_da_nome("aether-aBc-123_.v1.json.gz"),
            Some("aBc-123_")
        );
        assert_eq!(
            dispositivo_da_nome("rymembra-aBc-123_.v1.json.gz"),
            Some("aBc-123_")
        );
        let di_prima = format!("{}portatile{SUFFISSO}", eredita::PREFISSO_DISPOSITIVO);
        assert_eq!(dispositivo_da_nome(&di_prima), Some("portatile"));
        assert!(ha_il_nome_di_prima(&di_prima));
        assert!(!ha_il_nome_di_prima(&nome_file("portatile")));
    }

    #[test]
    fn nomi_non_documento_restano_none() {
        // Né le skin e le bozze di Drive, né i file di passaggio, né un nome
        // col suffisso giusto e il prefisso sbagliato.
        for nome in [
            "rymembra-skin-notte.ryskin",
            "aether-skin-notte.aeskin",
            "rymembra-libreria.v1.json.gz.corrotto-1",
            // R08-12: il file della libreria del backup, sotto i due nomi, sta
            // nella stessa cartella dell'app quando la sincronia è su Drive.
            "rymembra-libreria.v1.json.gz",
            "aether-libreria.v1.json.gz",
            "rymembra-uno.v1.json.gz.in-corso",
            "rymembra-uno.v2.json.gz",
            "aether-uno.v2.json.gz",
            "altro-uno.v1.json.gz",
            "uno.v1.json.gz",
            "rymembra-",
            "",
        ] {
            let letto = dispositivo_da_nome(nome);
            // Gli ultimi due casi sono vuoti ma non hanno il suffisso.
            assert_eq!(letto, None, "«{nome}» non è un documento");
        }
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

    fn gzip(crudo: &str) -> Vec<u8> {
        let mut compressore =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        compressore.write_all(crudo.as_bytes()).expect("comprime");
        compressore.finish().expect("chiude")
    }

    #[test]
    fn un_documento_della_3_2_0_senza_il_segno_si_legge_come_prima() {
        // Scritto a mano, com'è nella 3.2.0: nessun campo `capacita`. Si legge
        // tutto quel che c'era, il segno resta vuoto, e il contenuto riserializzato
        // non ne porta (`skip_serializing_if`): l'impronta di chi non ha segno è
        // quella di sempre.
        let documento = interpreta(&gzip(
            r#"{"version":1,"device":"vecchio","generatedAt":7,"content":{
                "ascolti":{"a":3},"ultimo":{"a":500},
                "voti":{"a":{"v":4,"at":10}},
                "preferiti":{"a":{"on":true,"at":11}},
                "lapidi":{"brani":{"x":5},"playlist":{"p":6}},
                "cartelle":{"D:/Musica":{"on":true,"at":1}},
                "skin":{"nera":"abc"},"skin_attiva":{"id":"nera","at":2}
            }}"#,
        ))
        .expect("si legge");
        assert!(documento.content.capacita.is_empty());
        assert_eq!(documento.content.ascolti.get("a"), Some(&3));
        assert_eq!(documento.content.voti.get("a").map(|v| v.v), Some(4));
        assert_eq!(documento.content.lapidi.brani.get("x"), Some(&5));
        assert_eq!(documento.content.cartelle.len(), 1);
        assert_eq!(
            documento
                .content
                .skin_attiva
                .as_ref()
                .map(|s| s.id.as_str()),
            Some("nera")
        );
        let canonico =
            String::from_utf8(canonico(&documento.content).expect("canonico")).expect("utf8");
        assert!(
            !canonico.contains("capacita"),
            "senza segno il contenuto non ne scrive: {canonico}"
        );
    }

    #[test]
    fn il_segno_si_rilegge_e_cambia_l_impronta_solo_di_chi_lo_porta() {
        let senza = Contenuto::default();
        let con = Contenuto {
            capacita: capacita_di_questa_versione(),
            ..Contenuto::default()
        };
        assert_ne!(
            impronta(&senza).expect("impronta"),
            impronta(&con).expect("impronta")
        );

        // Scritto e riletto, il documento ha lo stesso contenuto e la stessa
        // impronta: è ciò che permette a `Motore::passata` di non riscriverlo.
        let scritto = serializza(&Documento::nuovo("uno", con.clone(), 1)).expect("serializza");
        let letto = interpreta(&scritto).expect("si legge");
        assert_eq!(letto.content, con);
        assert_eq!(
            impronta(&letto.content).expect("impronta"),
            impronta(&con).expect("impronta")
        );
    }

    #[test]
    fn i_segni_di_un_documento_altrui_hanno_un_tetto() {
        // Il campo si fonde (unione) e si tiene in memoria per ogni pari: un
        // documento ostile non deve poterlo gonfiare dentro i limiti generali.
        let molti: Vec<String> = (0..1_000).map(|n| format!("\"s{n:04}\"")).collect();
        let lungo = "x".repeat(10_000);
        let documento = interpreta(&gzip(&format!(
            r#"{{"version":1,"device":"ostile","content":{{"capacita":[{},"{lungo}",7,null,""]}}}}"#,
            molti.join(",")
        )))
        .expect("si legge");
        assert!(
            documento.content.capacita.len() <= 16,
            "{}",
            documento.content.capacita.len()
        );
        assert!(
            documento
                .content
                .capacita
                .iter()
                .all(|segno| !segno.is_empty() && segno.len() <= 64)
        );
        // Un segno sconosciuto di una 3.3.x, invece, passa.
        let futuro = interpreta(&gzip(
            r#"{"version":1,"device":"nuovo","content":{"capacita":["voci-assenti-1","altro-2"]}}"#,
        ))
        .expect("si legge");
        assert!(futuro.content.capacita.contains("altro-2"));
        assert!(futuro.content.capacita.contains(CAPACITA_VOCI_ASSENTI));
    }

    #[test]
    fn i_segni_si_uniscono_e_la_fusione_resta_commutativa() {
        let mut uno = Contenuto::default();
        uno.capacita.insert("a-1".to_owned());
        let mut due = Contenuto::default();
        due.capacita.insert("b-1".to_owned());
        assert_eq!(uno.fondi(&due), due.fondi(&uno));
        assert_eq!(uno.fondi(&due).capacita.len(), 2);
        assert_eq!(uno.fondi(&uno), uno);
        assert_eq!(uno.fondi(&Contenuto::default()), uno);
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
    fn un_documento_piccolo_non_si_apre_in_un_albero_enorme() {
        // Dieci megabyte di `[0,0,…]` dentro un record, una decina di
        // kilobyte compressi: come `serde_json::Value` pesano ~400 MB
        // (misurato, quaranta volte il testo), e al tetto di 64 MiB si arriva a
        // gigabyte. Il documento si legge perché il record storto cade da sé:
        // oggi il conto lo paga la memoria, prima di qualunque controllo.
        let crudo = format!(
            r#"{{"version":1,"device":"uno","generatedAt":5,"content":{{"voti":{{"a":[{}0]}}}}}}"#,
            "0,".repeat(5_000_000)
        );
        let mut compressore =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        compressore.write_all(crudo.as_bytes()).expect("comprime");
        let byte = compressore.finish().expect("chiude");
        drop(crudo);

        let errore = interpreta(&byte).expect_err("troppi valori per un documento vero");
        assert_eq!(*errore.code(), ErrorCode::SyncRemoteCorrupt);
        assert!(
            errore.cause().is_some_and(|causa| causa.contains("valori")),
            "{errore:?}"
        );
    }

    #[test]
    fn un_documento_vero_e_grande_si_legge_ancora() {
        // Il tetto è sui valori, non sui record: una libreria da centomila
        // brani con ascolti, ultimi ascolti e voti sta molto sotto.
        let mut contenuto = Contenuto::default();
        for n in 0..100_000 {
            let chiave = format!("artista {n}|titolo {n}|album");
            contenuto.ascolti.insert(chiave.clone(), 3);
            contenuto.ultimo.insert(chiave.clone(), 1_700_000_000_000);
            contenuto.voti.insert(chiave, Voto { v: 4, at: 10 });
        }
        let documento = Documento::nuovo("uno", contenuto, 5);
        let riletto = interpreta(&serializza(&documento).expect("scrive")).expect("si legge");
        assert_eq!(riletto, documento);
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
