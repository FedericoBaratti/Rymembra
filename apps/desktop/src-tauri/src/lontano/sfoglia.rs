//! «Sfoglia <dispositivo>»: la libreria di un altro dispositivo collegato,
//! vista e ascoltata da qui.
//!
//! # Gli elenchi
//!
//! Album, artisti, brani e ricerca si chiedono all'altro a pagine, e
//! arrivano nella forma che la finestra disegna già per la libreria di qui:
//! [`AlbumSummary`], [`ArtistSummary`], e per i brani [`BranoRemoto`], che è
//! una scheda di brano senza il percorso. Le copertine cambiano nome per la
//! strada: l'impronta dell'altro diventa `lontano-<dispositivo>-<impronta>`,
//! che il protocollo `rymembra-cover` riconosce e serve da [`copertina`].
//!
//! # L'ascolto
//!
//! Un brano dell'altro diventa una voce «lontana» del registro del lettore
//! (`riproduzione::registra_lontana`), che si apre in rete con [`DaLontano`]:
//! una sorgente a blocchi, come quella d'«Ascolta insieme», sotto la stessa
//! finestra scorrevole dei cataloghi. Niente cache dei brani: si è scelto
//! così, e un brano riascoltato si richiede.
//!
//! La finestra rimanda indietro le schede che ha ricevuto: qui non si tiene
//! nessun elenco, e il comando registra quel che gli arriva. Una scheda
//! alterata non fa danni — l'altro serve solo i suoi file condivisi, per id.
//!
//! # Gli ascolti
//!
//! Misurati qui, contati là: [`ascoltato_qui`] li mette in fila nella
//! libreria, e [`manda_gli_ascolti`] li manda quando l'altro risponde. La
//! chiave di ogni ascolto rende innocuo un invio ripetuto.
//!
//! # «Solo Wi-Fi»
//!
//! Su una rete a consumo — i dati mobili del telefono — un brano dell'altro
//! si apre solo con «Usa i dati mobili» acceso. Con «Chiedi», il comando
//! risponde `lontano.reteAConsumo` e la finestra chiede; la risposta diventa
//! l'impostazione. Gli elenchi e le copertine passano sempre: pesano poco, e
//! senza non si saprebbe cosa scegliere. Su Windows la rete non è mai a
//! consumo. La stessa regola vale per «Ascolta insieme» via internet
//! (`lontano::insieme::dati_permessi`): entrare, accettare, invitare e
//! ammettere su una connessione del collegamento.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use rymembra_app::dispositivi::{self, AscoltoLontano, DatiMobili};
use rymembra_app::library::condivisa::BranoRemoto;
use rymembra_app::library::{AlbumSummary, ArtistSummary};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::listen::Listen;
use rymembra_lontano::protocollo::MASSIMO_PEZZO;
use rymembra_lontano::{Domanda, Risposta};
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::Pezzo;
use tauri::AppHandle;

use super::filo::{ATTESA_LIBRERIA, Comune, Posta, in_libreria, prendi};
use super::{comune, in_disparte};
use crate::errore::Esito;
use crate::nota;
use crate::riproduzione::insieme::{Apertura, ApriInRete, SchedaVoce, Voce};

/// Quanto si chiede all'altro per volta, e quanto si aspetta un salto: vedi
/// `crate::insieme::flusso`, che fa lo stesso per la stessa ragione.
const BLOCCO: u64 = 256 * 1024;

/// Quanto si aspetta un blocco. Via relay, sui dati mobili, un quarto di
/// mebibyte può chiedere qualche secondo; oltre, la rete non c'è.
const ATTESA_BLOCCO: Duration = Duration::from_secs(20);

/// Quanto si aspetta una pagina di un elenco.
const ATTESA_ELENCO: Duration = Duration::from_secs(20);

/// Quanto si aspetta una copertina.
const ATTESA_COPERTINA: Duration = Duration::from_secs(15);

/// Quanto si aspetta l'altro per un ascolto.
const ATTESA_ASCOLTO: Duration = Duration::from_secs(10);

/// Quanti brani entrano in coda con un gesto solo: una coda di sessantamila
/// voci lontane non la vuole nessuno, e il registro le terrebbe tutte.
const MASSIMI_BRANI: usize = 5000;

/// Quante copertine si chiedono all'altro insieme: una griglia ne chiede
/// trenta per schermata, e trenta richieste insieme su un relay rallentano il
/// brano che suona.
const COPERTINE_INSIEME: usize = 6;

/// Quanto può pesare una copertina dell'altro: la media è di qualche
/// centinaio di KB.
const MASSIMA_COPERTINA: usize = 4 * 1024 * 1024;

/// La cache delle copertine lontane: oltre [`CACHE_MASSIMA`] si scende a
/// [`CACHE_DOPO`], togliendo le meno usate.
const CACHE_MASSIMA: u64 = 50 * 1024 * 1024;
const CACHE_DOPO: u64 = 40 * 1024 * 1024;

/// Ogni quante copertine scritte si guarda quanto pesa la cache.
const POTA_OGNI: usize = 32;

/// Il prefisso delle chiavi di copertina lontane.
const PREFISSO_COPERTINA: &str = "lontano-";

// ── la sorgente ─────────────────────────────────────────────────────────────

/// Chi legge i pezzi di un brano dell'altro: il nodo; nelle prove, un file in
/// memoria. Restituisce i byte, la dimensione del file e la sua versione.
pub(super) trait Pezzi: Send + Sync + std::fmt::Debug + 'static {
    fn pezzo(&self, brano: i64, da: u64, quanti: u64) -> Result<(Vec<u8>, u64, String), AppError>;
}

/// Il nodo, preso a ogni blocco e lasciato subito: spento «Raggiungibile da
/// internet», il filo lo deve poter fermare davvero, e una sorgente aperta
/// che lo tenesse lo terrebbe acceso finché il brano non finisce.
pub(super) struct ViaNodo {
    pub(super) comune: Arc<Comune>,
    pub(super) chiave: String,
}

impl std::fmt::Debug for ViaNodo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViaNodo").finish_non_exhaustive()
    }
}

impl Pezzi for ViaNodo {
    fn pezzo(&self, brano: i64, da: u64, quanti: u64) -> Result<(Vec<u8>, u64, String), AppError> {
        let nodo = self
            .comune
            .nodo()
            .ok_or_else(|| AppError::new(ErrorCode::LontanoSpento))?;
        match nodo.chiedi(
            &self.chiave,
            Domanda::Leggi { brano, da, quanti },
            ATTESA_BLOCCO,
        )? {
            Risposta::Byte {
                byte,
                totale,
                versione,
            } => Ok((byte, totale, versione)),
            _ => Err(AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause("a una lettura non si risponde con dei byte")),
        }
    }
}

/// Finché c'è, il nodo del telefono resta acceso anche a schermo spento: vedi
/// `Comune::in_volo`.
pub(super) struct InVolo(Arc<Comune>);

impl InVolo {
    pub(super) fn nuovo(comune: Arc<Comune>) -> Self {
        comune.in_volo.fetch_add(1, Ordering::Relaxed);
        tieni_il_wifi(&comune);
        Self(comune)
    }
}

impl Drop for InVolo {
    fn drop(&mut self) {
        self.0.in_volo.fetch_sub(1, Ordering::Relaxed);
        tieni_il_wifi(&self.0);
    }
}

/// Sul telefono, finché arriva qualcosa dall'altro il Wi-Fi resta sveglio
/// (`rymembra_android::sistema::wifi`). Sotto un lucchetto che rilegge il
/// conto: due fili che aprono e chiudono insieme non lasciano il Wi-Fi nello
/// stato del primo dei due.
#[cfg(target_os = "android")]
fn tieni_il_wifi(comune: &Comune) {
    static PRESO: Mutex<bool> = Mutex::new(false);
    let mut preso = prendi(&PRESO);
    let voluto = comune.in_volo();
    if *preso == voluto {
        return;
    }
    match rymembra_android::sistema::wifi("lontano", voluto) {
        Ok(()) => *preso = voluto,
        Err(err) => nota!("[lontano] il lucchetto del Wi-Fi non si cambia: {err}"),
    }
}

#[cfg(not(target_os = "android"))]
const fn tieni_il_wifi(_: &Comune) {}

impl std::fmt::Debug for InVolo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InVolo")
    }
}

/// I byte di un brano dell'altro, come [`Sorgente`]: vedi il `//!`.
#[derive(Debug)]
struct DaLontano<P: Pezzi = ViaNodo> {
    pezzi: P,
    brano: i64,
    /// La versione del file, fissata dalla prima risposta e comune a tutte le
    /// aperture della stessa voce: un file cambiato nel frattempo non si cuce
    /// con quello di prima.
    ///
    /// Non quella dell'elenco ([`BranoRemoto::versione`]): viene dal database
    /// dell'altro, che può essere indietro rispetto al disco — dopo «Rileggi
    /// tutti i tag» (`date_modified = -1`), o con un file ritoccato da fuori
    /// dopo l'ultima scansione — e ogni lettura sarebbe `fileCambiato`.
    versione: Arc<Mutex<Option<String>>>,
    _in_volo: Option<InVolo>,
}

impl<P: Pezzi> DaLontano<P> {
    /// Come `crate::insieme::flusso::DaPari::leggi`: a blocchi, chiedendo
    /// `avanti()` fra l'uno e l'altro. Gli errori passano senza causa: il
    /// lettore li scrive nel diario, e una causa di rete può dire un indirizzo.
    fn leggi(&self, da: u64, quanti: u64, avanti: &dyn Fn() -> bool) -> Result<Pezzo, AppError> {
        let capienza = usize::try_from(quanti.min(MASSIMO_PEZZO)).unwrap_or(0);
        let mut byte = Vec::with_capacity(capienza);
        let mut totale = None;
        let mut letti = 0_u64;
        while letti < quanti {
            if !avanti() {
                return Err(AppError::new(ErrorCode::InternalAborted {
                    what: Some("lettura in rete".to_owned()),
                }));
            }
            let chiesti = quanti.saturating_sub(letti).min(BLOCCO);
            let (mut blocco, dimensione, versione) = self
                .pezzi
                .pezzo(self.brano, da.saturating_add(letti), chiesti)
                .map_err(|err| AppError::new(err.code().clone()))?;
            {
                let mut fissata = prendi(&self.versione);
                match fissata.as_deref() {
                    Some(prima) if prima != versione => {
                        return Err(AppError::new(ErrorCode::LontanoFileCambiato));
                    }
                    Some(_) => {}
                    None => *fissata = Some(versione),
                }
            }
            totale = Some(dimensione);
            // Mai più di quanto chiesto: l'altro non decide quanto si
            // alloca qui.
            blocco.truncate(usize::try_from(chiesti).unwrap_or(usize::MAX));
            let arrivati = u64::try_from(blocco.len()).unwrap_or(u64::MAX);
            byte.extend_from_slice(&blocco);
            letti = letti.saturating_add(arrivati);
            // Meno di quanti chiesti è la fine del file.
            if arrivati < chiesti {
                break;
            }
        }
        Ok(Pezzo {
            byte,
            totale,
            url_finale: None,
        })
    }
}

impl<P: Pezzi> Sorgente for DaLontano<P> {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.leggi(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        self.leggi(da, quanti, avanti)
    }
}

// ── «Solo Wi-Fi» ────────────────────────────────────────────────────────────

/// La rete di adesso è a consumo. Se il telefono non risponde si dice di no:
/// un ponte rotto non deve togliere la musica, e il sistema stesso avvisa dei
/// dati che corrono.
#[cfg(target_os = "android")]
fn rete_a_consumo() -> bool {
    rymembra_android::sistema::rete_a_consumo().unwrap_or_else(|err| {
        nota!("[lontano] la rete a consumo non si sa: {err}");
        false
    })
}

#[cfg(not(target_os = "android"))]
const fn rete_a_consumo() -> bool {
    false
}

/// Un brano dell'altro si può aprire su questa rete: vedi il `//!`.
pub(super) fn dati_permessi(comune: &Comune) -> Result<(), AppError> {
    if !rete_a_consumo() {
        return Ok(());
    }
    match prendi(&comune.vetrina).dati_mobili {
        DatiMobili::Si => Ok(()),
        DatiMobili::Chiedi | DatiMobili::No => Err(AppError::new(ErrorCode::LontanoReteAConsumo)),
    }
}

// ── le voci ─────────────────────────────────────────────────────────────────

/// I millisecondi di un fotogramma del CD (1/75 s), arrotondati: lo stesso
/// giro che `riproduzione::ospiti::tratto` fa all'indietro.
fn ms_da_cd(cd: i64) -> u64 {
    let cd = u64::try_from(cd).unwrap_or(0);
    cd.saturating_mul(40)
        .saturating_add(1)
        .checked_div(3)
        .unwrap_or(0)
}

/// L'estensione di un brano dell'altro, se ha la forma di un'estensione.
pub(super) fn estensione_buona(estensione: &str) -> Option<String> {
    let estensione = estensione
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    (!estensione.is_empty()
        && estensione.len() <= 8
        && estensione.bytes().all(|b| b.is_ascii_alphanumeric()))
    .then_some(estensione)
}

/// Un'impronta di copertina dell'altro: le stesse regole di chi la serve.
fn e_un_impronta(impronta: &str) -> bool {
    (16..=128).contains(&impronta.len())
        && impronta
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Una chiave di dispositivo: sessantaquattro cifre esadecimali minuscole.
fn e_una_chiave(chiave: &str) -> bool {
    chiave.len() == 64
        && chiave
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// La chiave di copertina per la finestra: `lontano-<dispositivo>-<impronta>`.
/// Accetta l'impronta dell'altro, o una chiave già tradotta per lo stesso
/// dispositivo (la scheda che la finestra rimanda indietro).
fn chiave_copertina(dispositivo: &str, impronta: Option<&str>) -> Option<String> {
    let impronta = impronta?;
    let impronta = impronta
        .strip_prefix(PREFISSO_COPERTINA)
        .and_then(|resto| resto.strip_prefix(dispositivo))
        .and_then(|resto| resto.strip_prefix('-'))
        .unwrap_or(impronta);
    (e_una_chiave(dispositivo) && e_un_impronta(impronta))
        .then(|| format!("{PREFISSO_COPERTINA}{dispositivo}-{impronta}"))
}

/// Il dispositivo e l'impronta di una chiave di copertina lontana.
fn parti_della_copertina(chiave: &str) -> Option<(&str, &str)> {
    let resto = chiave.strip_prefix(PREFISSO_COPERTINA)?;
    let (dispositivo, impronta) = resto.split_once('-')?;
    (e_una_chiave(dispositivo) && e_un_impronta(impronta)).then_some((dispositivo, impronta))
}

/// È una chiave di copertina lontana: per `crate::copertine`.
#[must_use]
pub(crate) fn e_una_copertina(chiave: &str) -> bool {
    parti_della_copertina(chiave).is_some()
}

/// La voce di un brano dell'altro, con l'`uid` che le dà il registro.
fn voce_da(
    comune: &Arc<Comune>,
    dispositivo: &str,
    nome: &str,
    brano: &BranoRemoto,
    uid: u64,
) -> Voce {
    let per_aprire = Arc::clone(comune);
    let chiave = dispositivo.to_owned();
    let id = brano.id;
    // Fissata alla prima apertura: vedi `DaLontano::versione`.
    let versione = Arc::new(Mutex::new(None::<String>));
    let apri: ApriInRete = Arc::new(move || {
        // Si guarda adesso e non solo al gesto: la coda può arrivare a questo
        // brano dopo che il Wi-Fi se n'è andato.
        dati_permessi(&per_aprire)?;
        Ok(Box::new(DaLontano {
            pezzi: ViaNodo {
                comune: Arc::clone(&per_aprire),
                chiave: chiave.clone(),
            },
            brano: id,
            versione: Arc::clone(&versione),
            _in_volo: Some(InVolo::nuovo(Arc::clone(&per_aprire))),
        }) as Box<dyn Sorgente>)
    });
    Voce {
        uid,
        proprietario: Some(nome.to_owned()),
        scheda: SchedaVoce {
            titolo: brano.title.clone(),
            artista: brano.artist.clone(),
            album: brano.album.clone(),
            durata_ms: u64::try_from(brano.duration_ms).unwrap_or(0),
            numero: brano.track_number.and_then(|n| u32::try_from(n).ok()),
            anno: brano.year.and_then(|a| i32::try_from(a).ok()),
            copertina: chiave_copertina(dispositivo, brano.cover_art_hash.as_deref()),
            replaygain_db: None,
            picco: None,
        },
        apertura: Apertura::Rete {
            apri,
            estensione: estensione_buona(&brano.estensione),
            segmento: brano
                .segmento
                .map(|s| (ms_da_cd(s.inizio_cd), s.fine_cd.map(ms_da_cd))),
            // Due brani della stessa immagine CD devono portare la stessa
            // stringa, perché il motore li riconosca contigui: l'album e la
            // versione del file la danno.
            immagine: brano.segmento.map(|_| {
                format!(
                    "lontano:{dispositivo}:{}:{}",
                    brano.album_key.as_deref().unwrap_or(""),
                    brano.versione
                )
            }),
            copia: None,
        },
        da_contare: None,
    }
}

/// Le maniglie dei brani dell'altro, registrati come voci lontane.
///
/// # Errori
///
/// `insieme.gestoNonDisponibile` durante una sessione (una voce lontana non
/// entra in una sessione), `lontano.nonAssociato`, `lontano.spento`,
/// `lontano.reteAConsumo` (vedi il `//!`), `ipc.payloadInvalid` per una lista
/// troppo lunga.
fn voci_lontane(
    app: &AppHandle,
    chiave: &str,
    brani: &[BranoRemoto],
) -> Result<Vec<i64>, AppError> {
    if !matches!(crate::insieme::ruolo(app), crate::insieme::Ruolo::Fuori) {
        return Err(AppError::new(ErrorCode::InsiemeGestoNonDisponibile {
            gesto: "lontano".to_owned(),
        }));
    }
    if brani.len() > MASSIMI_BRANI {
        return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "lontano".to_owned(),
            detail: Some(format!("al più {MASSIMI_BRANI} brani per volta")),
        }));
    }
    let comune = comune(app)?;
    if !comune.associato(chiave) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    if comune.nodo().is_none() {
        return Err(AppError::new(ErrorCode::LontanoSpento));
    }
    dati_permessi(&comune)?;
    let nome = prendi(&comune.vetrina)
        .dispositivi
        .iter()
        .find(|d| d.chiave == chiave)
        .map_or_else(|| "?".to_owned(), |d| d.nome.clone());
    brani
        .iter()
        .map(|brano| {
            let descrittore = serde_json::to_value(VoceSalvata {
                dispositivo: chiave.to_owned(),
                nome: nome.clone(),
                brano: brano.clone(),
            })
            .unwrap_or_default();
            crate::riproduzione::registra_lontana(app, chiave, brano.id, descrittore, |uid| {
                voce_da(&comune, chiave, &nome, brano, uid)
            })
            .ok_or_else(|| {
                AppError::new(ErrorCode::InternalAborted {
                    what: Some("lontano".to_owned()),
                })
            })
        })
        .collect()
}

/// Come si conserva una voce lontana nella coda (`playback::CHIAVE_CODA_LONTANA`):
/// di chi è, come si chiama il proprietario, e la scheda del brano — quel che
/// serve a rifarne la voce senza chiedere niente all'altro.
#[derive(serde::Serialize, serde::Deserialize)]
struct VoceSalvata {
    dispositivo: String,
    nome: String,
    brano: BranoRemoto,
}

/// Una voce lontana conservata nella coda, riaperta all'avvio: la sua maniglia
/// nuova, o `None` se il dispositivo non è più fra gli `associati` (presi dal
/// database: il filo del collegamento all'avvio non li ha ancora letti) o il
/// descrittore non si legge. Non guarda il nodo né «Solo Wi-Fi»: si guardano
/// quando la voce si apre, come per una messa in coda adesso.
pub(crate) fn voce_salvata(
    app: &AppHandle,
    descrittore: &serde_json::Value,
    associati: &std::collections::HashSet<String>,
) -> Option<i64> {
    let salvata = serde_json::from_value::<VoceSalvata>(descrittore.clone()).ok()?;
    if !e_una_chiave(&salvata.dispositivo) || !associati.contains(&salvata.dispositivo) {
        return None;
    }
    let comune = comune(app).ok()?;
    crate::riproduzione::registra_lontana(
        app,
        &salvata.dispositivo,
        salvata.brano.id,
        descrittore.clone(),
        |uid| {
            voce_da(
                &comune,
                &salvata.dispositivo,
                &salvata.nome,
                &salvata.brano,
                uid,
            )
        },
    )
}

// ── gli elenchi ─────────────────────────────────────────────────────────────

/// Una domanda di dati all'altro, letta nella forma `T`.
fn elenco<T: serde::de::DeserializeOwned>(
    app: &AppHandle,
    chiave: &str,
    domanda: Domanda,
) -> Result<T, AppError> {
    let comune = comune(app)?;
    if !comune.associato(chiave) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    let nodo = comune
        .nodo()
        .ok_or_else(|| AppError::new(ErrorCode::LontanoSpento))?;
    match nodo.chiedi(chiave, domanda, ATTESA_ELENCO)? {
        Risposta::Dati(dati) => serde_json::from_value(dati)
            .map_err(|err| AppError::new(ErrorCode::LontanoProtocollo).with_cause(err.to_string())),
        _ => Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("a un elenco non si risponde con dei dati")),
    }
}

fn album_tradotti(chiave: &str, mut album: Vec<AlbumSummary>) -> Vec<AlbumSummary> {
    for a in &mut album {
        a.cover_art_hash = chiave_copertina(chiave, a.cover_art_hash.as_deref());
    }
    album
}

fn brani_tradotti(chiave: &str, mut brani: Vec<BranoRemoto>) -> Vec<BranoRemoto> {
    for b in &mut brani {
        b.cover_art_hash = chiave_copertina(chiave, b.cover_art_hash.as_deref());
    }
    brani
}

/// Gli album dell'altro, a pagine.
#[tauri::command]
pub async fn lontano_album(
    app: AppHandle,
    chiave: String,
    da: u32,
    quanti: u32,
) -> Esito<Vec<AlbumSummary>> {
    in_disparte(move || {
        let album = elenco(&app, &chiave, Domanda::Album { da, quanti })?;
        Ok(album_tradotti(&chiave, album))
    })
    .await
}

/// Gli album di un artista dell'altro.
#[tauri::command]
pub async fn lontano_album_artista(
    app: AppHandle,
    chiave: String,
    artista: String,
) -> Esito<Vec<AlbumSummary>> {
    in_disparte(move || {
        let album = elenco(&app, &chiave, Domanda::AlbumArtista { artista })?;
        Ok(album_tradotti(&chiave, album))
    })
    .await
}

/// Gli artisti dell'altro, a pagine.
#[tauri::command]
pub async fn lontano_artisti(
    app: AppHandle,
    chiave: String,
    da: u32,
    quanti: u32,
) -> Esito<Vec<ArtistSummary>> {
    in_disparte(move || {
        let mut artisti: Vec<ArtistSummary> =
            elenco(&app, &chiave, Domanda::Artisti { da, quanti })?;
        for a in &mut artisti {
            a.covers = a
                .covers
                .iter()
                .filter_map(|c| chiave_copertina(&chiave, Some(c)))
                .collect();
        }
        Ok(artisti)
    })
    .await
}

/// I brani dell'altro, a pagine, in un ordine (`recenti`, `ascoltati`,
/// `titolo`, o lo scaffale).
#[tauri::command]
pub async fn lontano_brani(
    app: AppHandle,
    chiave: String,
    ordine: String,
    da: u32,
    quanti: u32,
) -> Esito<Vec<BranoRemoto>> {
    in_disparte(move || {
        let brani = elenco(&app, &chiave, Domanda::Brani { ordine, da, quanti })?;
        Ok(brani_tradotti(&chiave, brani))
    })
    .await
}

/// I brani di un album dell'altro, dalla sua impronta.
#[tauri::command]
pub async fn lontano_brani_album(
    app: AppHandle,
    chiave: String,
    album: String,
) -> Esito<Vec<BranoRemoto>> {
    in_disparte(move || {
        let brani = elenco(&app, &chiave, Domanda::BraniAlbum { album })?;
        Ok(brani_tradotti(&chiave, brani))
    })
    .await
}

/// La ricerca nella libreria dell'altro.
#[tauri::command]
pub async fn lontano_cerca(
    app: AppHandle,
    chiave: String,
    testo: String,
    da: u32,
    quanti: u32,
) -> Esito<Vec<BranoRemoto>> {
    in_disparte(move || {
        let brani = elenco(&app, &chiave, Domanda::Cerca { testo, da, quanti })?;
        Ok(brani_tradotti(&chiave, brani))
    })
    .await
}

/// Quanti risultati ha una ricerca nella libreria dell'altro.
#[tauri::command]
pub async fn lontano_cerca_conteggio(app: AppHandle, chiave: String, testo: String) -> Esito<i64> {
    in_disparte(move || elenco(&app, &chiave, Domanda::CercaConteggio { testo })).await
}

// ── i gesti ─────────────────────────────────────────────────────────────────

/// Suona i brani dell'altro, a partire da `indice`.
#[tauri::command]
pub async fn lontano_suona(
    app: AppHandle,
    chiave: String,
    brani: Vec<BranoRemoto>,
    indice: usize,
) -> Esito<()> {
    let maniglie = {
        let app = app.clone();
        in_disparte(move || voci_lontane(&app, &chiave, &brani)).await?
    };
    crate::riproduzione::suona(app, maniglie, indice).await
}

/// Accoda i brani dell'altro in fondo.
#[tauri::command]
pub async fn lontano_accoda(app: AppHandle, chiave: String, brani: Vec<BranoRemoto>) -> Esito<()> {
    let maniglie = {
        let app = app.clone();
        in_disparte(move || voci_lontane(&app, &chiave, &brani)).await?
    };
    crate::riproduzione::coda_accoda(app, maniglie).await
}

/// Mette i brani dell'altro subito dopo il corrente.
#[tauri::command]
pub async fn lontano_dopo(app: AppHandle, chiave: String, brani: Vec<BranoRemoto>) -> Esito<()> {
    let maniglie = {
        let app = app.clone();
        in_disparte(move || voci_lontane(&app, &chiave, &brani)).await?
    };
    crate::riproduzione::coda_dopo(app, maniglie).await
}

// ── gli ascolti ─────────────────────────────────────────────────────────────

/// Un ascolto fatto qui di un brano dell'altro: in fila, da mandargli.
/// Gira sul filo delle scritture del lettore.
pub fn ascoltato_qui(app: &AppHandle, dispositivo: &str, brano: i64, ascolto: &Listen) {
    let lontano = AscoltoLontano {
        brano,
        quando_ms: ascolto.started_at,
        ascoltato_ms: i64::try_from(ascolto.listened_ms).unwrap_or(i64::MAX),
        // Un brano non comincia due volte nello stesso millisecondo: l'inizio
        // e il brano bastano a distinguere ogni ascolto di qui, e chi lo riceve
        // ci mette davanti la chiave di questo dispositivo.
        chiave: format!("{}-{brano}", ascolto.started_at),
    };
    match in_libreria(app, ATTESA_LIBRERIA, |c| {
        dispositivi::ricorda_ascolto(c, dispositivo, &lontano)
    }) {
        Ok(()) => {
            if let Ok(comune) = comune(app) {
                comune.sveglia(Posta::Ascolti);
            }
        }
        Err(err) => nota!(
            "[lontano] un ascolto di un brano di un altro dispositivo non si ricorda codice={}",
            err.code().kind().code()
        ),
    }
}

/// C'è un giro di invii in corso.
static POSTINO: AtomicBool = AtomicBool::new(false);

/// Un invio non è riuscito, o un giro non è partito: il filo riprova al
/// prossimo giro.
static DA_RIPROVARE: AtomicBool = AtomicBool::new(false);

/// Il filo deve riprovare gli invii; lo dice una volta sola.
pub(super) fn da_riprovare() -> bool {
    DA_RIPROVARE.swap(false, Ordering::AcqRel)
}

/// Manda gli ascolti in fila ai dispositivi che rispondono adesso — a `solo`,
/// se è detto. Su un filo suo: il filo del collegamento non aspetta la rete.
pub(super) fn manda_gli_ascolti(app: &AppHandle, comune: &Arc<Comune>, solo: Option<&str>) {
    if comune.nodo().is_none() {
        return;
    }
    let presenti: Vec<String> = match solo {
        Some(chiave) => vec![chiave.to_owned()],
        None => prendi(&comune.presenze).keys().cloned().collect(),
    };
    if presenti.is_empty() {
        return;
    }
    if POSTINO.swap(true, Ordering::AcqRel) {
        DA_RIPROVARE.store(true, Ordering::Release);
        return;
    }
    let app = app.clone();
    let comune = Arc::clone(comune);
    let partito = std::thread::Builder::new()
        .name("rymembra-lontano-ascolti".to_owned())
        .spawn(move || {
            for chiave in presenti {
                if !manda_a(&app, &comune, &chiave) {
                    DA_RIPROVARE.store(true, Ordering::Release);
                }
            }
            POSTINO.store(false, Ordering::Release);
        });
    if let Err(err) = partito {
        nota!("[lontano] il filo degli ascolti non parte: {err}");
        POSTINO.store(false, Ordering::Release);
        DA_RIPROVARE.store(true, Ordering::Release);
    }
}

/// Manda a `chiave` i suoi ascolti in fila, dal più vecchio. `false` se si è
/// fermato prima della fine.
fn manda_a(app: &AppHandle, comune: &Comune, chiave: &str) -> bool {
    let da_mandare = match in_libreria(app, ATTESA_LIBRERIA, |c| {
        dispositivi::ascolti_da_mandare(c, chiave)
    }) {
        Ok(da_mandare) => da_mandare,
        Err(_) => return false,
    };
    for ascolto in da_mandare {
        let Some(nodo) = comune.nodo() else {
            return false;
        };
        let esito = nodo.chiedi(
            chiave,
            Domanda::Ascoltato {
                brano: ascolto.brano,
                quando_ms: ascolto.quando_ms,
                ascoltato_ms: ascolto.ascoltato_ms,
                chiave: ascolto.chiave.clone(),
            },
            ATTESA_ASCOLTO,
        );
        drop(nodo);
        if let Err(err) = esito {
            nota!(
                "[lontano] un ascolto non è arrivato codice={}: si riprova",
                err.code().kind().code()
            );
            return false;
        }
        if let Err(err) = in_libreria(app, ATTESA_LIBRERIA, |c| {
            dispositivi::ascolto_arrivato(c, &ascolto.chiave)
        }) {
            // Resta in fila: rimandato, l'altro lo riconosce dalla chiave.
            nota!(
                "[lontano] un ascolto arrivato non si toglie codice={}",
                err.code().kind().code()
            );
            return false;
        }
    }
    true
}

// ── le copertine ────────────────────────────────────────────────────────────

/// Quante copertine si stanno chiedendo, e chi aspetta un posto.
static IN_CORSO: Mutex<usize> = Mutex::new(0);
static UN_POSTO: Condvar = Condvar::new();

/// Un posto fra le [`COPERTINE_INSIEME`]: lasciato, sveglia chi aspetta.
struct Posto;

impl Posto {
    /// Un posto, aspettato al più `entro`.
    fn prendi(entro: Duration) -> Option<Self> {
        let in_corso = IN_CORSO.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut in_corso, _) = UN_POSTO
            .wait_timeout_while(in_corso, entro, |n| *n >= COPERTINE_INSIEME)
            .unwrap_or_else(PoisonError::into_inner);
        if *in_corso >= COPERTINE_INSIEME {
            return None;
        }
        *in_corso = in_corso.saturating_add(1);
        Some(Self)
    }
}

impl Drop for Posto {
    fn drop(&mut self) {
        let mut in_corso = IN_CORSO.lock().unwrap_or_else(PoisonError::into_inner);
        *in_corso = in_corso.saturating_sub(1);
        UN_POSTO.notify_one();
    }
}

/// Quante copertine si sono scritte nella cache: vedi [`POTA_OGNI`].
static SCRITTE: AtomicUsize = AtomicUsize::new(0);

/// Dove stanno le copertine lontane: accanto alla cache dei brani del cloud,
/// e per la stessa ragione fuori dai dati che vanno in giro coi profili. La
/// regola è quella di [`crate::cartella_di_cache`]; `None` vuol dire nessuna
/// cache.
fn cartella_copertine(app: &AppHandle) -> Option<PathBuf> {
    crate::cartella_di_cache(app, "copertine-lontane")
}

/// Una copertina dell'altro, per il protocollo `rymembra-cover`: dalla cache,
/// o chiesta all'altro e messa in cache. `Ok(None)` se l'altro non ce l'ha.
/// Si chiama su un filo del pool bloccante.
///
/// # Errori
///
/// `lontano.spento`, `lontano.nonAssociato`, quelli della rete, e
/// `net.timeout` se i posti sono tutti presi troppo a lungo.
pub(crate) fn copertina(app: &AppHandle, chiave: &str) -> Result<Option<Vec<u8>>, AppError> {
    let Some((dispositivo, impronta)) = parti_della_copertina(chiave) else {
        return Ok(None);
    };
    // Senza una cartella per la cache non c'è dove tenerla, né dove cercarla:
    // vedi `crate::cartella_di_cache`.
    let Some(cartella) = cartella_copertine(app) else {
        return Ok(None);
    };
    let file = cartella.join(format!("{impronta}.jpg"));
    if let Some(byte) = dalla_cache(&file) {
        return Ok(Some(byte));
    }
    let comune = comune(app)?;
    if !comune.associato(dispositivo) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    // Un dispositivo che non risponde non tiene un posto per quindici
    // secondi a copertina: una griglia intera aspetterebbe minuti. La
    // finestra la richiede quando torna (503 non resta in cache).
    if !prendi(&comune.presenze).contains_key(dispositivo) {
        return Err(AppError::new(ErrorCode::LontanoIrraggiungibile));
    }
    let _posto = Posto::prendi(ATTESA_COPERTINA).ok_or_else(|| {
        AppError::new(ErrorCode::NetTimeout {
            url: None,
            timeout_ms: u64::try_from(ATTESA_COPERTINA.as_millis()).ok(),
        })
    })?;
    // Mentre si aspettava il posto, un'altra richiesta può averla portata.
    if let Some(byte) = dalla_cache(&file) {
        return Ok(Some(byte));
    }
    let nodo = comune
        .nodo()
        .ok_or_else(|| AppError::new(ErrorCode::LontanoSpento))?;
    let risposta = nodo.chiedi(
        dispositivo,
        Domanda::Copertina {
            impronta: impronta.to_owned(),
        },
        ATTESA_COPERTINA,
    );
    drop(nodo);
    match risposta? {
        Risposta::Byte { byte, .. } if !byte.is_empty() && byte.len() <= MASSIMA_COPERTINA => {
            in_cache(&cartella, &file, &byte);
            Ok(Some(byte))
        }
        Risposta::Byte { .. } | Risposta::Fatto => Ok(None),
        _ => Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("a una copertina non si risponde con dei byte")),
    }
}

/// Una copertina dalla cache, segnata come usata adesso.
fn dalla_cache(file: &Path) -> Option<Vec<u8>> {
    let byte = std::fs::read(file).ok()?;
    if let Ok(aperto) = std::fs::File::options().write(true).open(file) {
        let _ = aperto.set_modified(SystemTime::now());
    }
    Some(byte)
}

/// Mette una copertina in cache: un file temporaneo rinominato, così una
/// lettura non trova mai un file a metà. Ogni tanto pota.
fn in_cache(cartella: &Path, file: &Path, byte: &[u8]) {
    if std::fs::create_dir_all(cartella).is_err() {
        return;
    }
    let provvisorio = file.with_extension("part");
    let scritto =
        std::fs::write(&provvisorio, byte).and_then(|()| std::fs::rename(&provvisorio, file));
    if scritto.is_err() {
        let _ = std::fs::remove_file(&provvisorio);
        return;
    }
    if SCRITTE
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(POTA_OGNI)
    {
        pota(cartella);
    }
}

/// Toglie le copertine meno usate finché la cache non scende a
/// [`CACHE_DOPO`], se ha passato [`CACHE_MASSIMA`].
fn pota(cartella: &Path) {
    let Ok(voci) = std::fs::read_dir(cartella) else {
        return;
    };
    let mut file: Vec<(SystemTime, u64, PathBuf)> = voci
        .filter_map(Result::ok)
        .filter_map(|voce| {
            let metadati = voce.metadata().ok()?;
            metadati.is_file().then(|| {
                (
                    metadati.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    metadati.len(),
                    voce.path(),
                )
            })
        })
        .collect();
    let mut totale: u64 = file.iter().map(|(_, peso, _)| *peso).sum();
    if totale <= CACHE_MASSIMA {
        return;
    }
    file.sort_by_key(|(quando, _, _)| *quando);
    for (_, peso, percorso) in file {
        if totale <= CACHE_DOPO {
            break;
        }
        if std::fs::remove_file(&percorso).is_ok() {
            totale = totale.saturating_sub(peso);
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un file finto in memoria, con la sua versione.
    #[derive(Debug)]
    struct Finto {
        byte: Vec<u8>,
        versione: Mutex<String>,
        richieste: Mutex<Vec<(u64, u64)>>,
    }

    impl Finto {
        fn di(n: usize) -> Self {
            Self {
                byte: (0..n).map(|i| u8::try_from(i % 251).unwrap()).collect(),
                versione: Mutex::new("v1".to_owned()),
                richieste: Mutex::new(Vec::new()),
            }
        }
    }

    impl Pezzi for Arc<Finto> {
        fn pezzo(
            &self,
            _brano: i64,
            da: u64,
            quanti: u64,
        ) -> Result<(Vec<u8>, u64, String), AppError> {
            self.richieste.lock().unwrap().push((da, quanti));
            let inizio = usize::try_from(da).unwrap().min(self.byte.len());
            let fine = inizio
                .saturating_add(usize::try_from(quanti).unwrap())
                .min(self.byte.len());
            Ok((
                self.byte[inizio..fine].to_vec(),
                u64::try_from(self.byte.len()).unwrap(),
                self.versione.lock().unwrap().clone(),
            ))
        }
    }

    fn sorgente(finto: &Arc<Finto>) -> DaLontano<Arc<Finto>> {
        DaLontano {
            pezzi: Arc::clone(finto),
            brano: 7,
            versione: Arc::new(Mutex::new(Some("v1".to_owned()))),
            _in_volo: None,
        }
    }

    /// Una lettura grande va a blocchi, e torna intera.
    #[test]
    fn si_legge_a_blocchi() {
        let finto = Arc::new(Finto::di(700 * 1024));
        let pezzo = sorgente(&finto).pezzo(10, 600 * 1024).unwrap();
        assert_eq!(pezzo.byte.len(), 600 * 1024);
        assert_eq!(pezzo.byte.as_slice(), &finto.byte[10..10 + 600 * 1024]);
        assert_eq!(pezzo.totale, Some(700 * 1024));
        let richieste = finto.richieste.lock().unwrap().clone();
        assert_eq!(richieste.len(), 3);
        assert!(richieste.iter().all(|(_, quanti)| *quanti <= BLOCCO));
    }

    /// La fine del file ferma la lettura senza una richiesta vuota.
    #[test]
    fn la_fine_del_file_non_e_un_guasto() {
        let finto = Arc::new(Finto::di(1000));
        let pezzo = sorgente(&finto).pezzo(900, 1024 * 1024).unwrap();
        assert_eq!(pezzo.byte.len(), 100);
        assert_eq!(finto.richieste.lock().unwrap().len(), 1);
    }

    /// Chi ha smesso di volere i byte non aspetta il blocco dopo.
    #[test]
    fn un_salto_ferma_la_lettura() {
        let finto = Arc::new(Finto::di(2 * 1024 * 1024));
        let chiesti = AtomicUsize::new(0);
        let esito = sorgente(&finto).pezzo_finche(0, 2 * 1024 * 1024, &|| {
            chiesti.fetch_add(1, Ordering::Relaxed) < 1
        });
        assert!(esito.is_err());
        assert_eq!(finto.richieste.lock().unwrap().len(), 1);
    }

    /// Un file cambiato fra un blocco e l'altro non si cuce.
    #[test]
    fn un_file_cambiato_si_dice() {
        let finto = Arc::new(Finto::di(1000));
        *finto.versione.lock().unwrap() = "v2".to_owned();
        let errore = sorgente(&finto).pezzo(0, 100).unwrap_err();
        assert_eq!(
            errore.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoFileCambiato
        );
    }

    /// La versione dell'elenco può essere vecchia: conta quella del disco,
    /// fissata alla prima lettura e comune alle aperture della voce.
    #[test]
    fn la_versione_si_fissa_alla_prima_lettura() {
        let finto = Arc::new(Finto::di(1000));
        *finto.versione.lock().unwrap() = "v9".to_owned();
        let fissata = Arc::new(Mutex::new(None));
        let apri = || DaLontano {
            pezzi: Arc::clone(&finto),
            brano: 7,
            versione: Arc::clone(&fissata),
            _in_volo: None,
        };
        assert_eq!(apri().pezzo(0, 100).unwrap().byte.len(), 100);
        assert_eq!(fissata.lock().unwrap().as_deref(), Some("v9"));
        // Un'altra apertura della stessa voce, dopo un salto: stessa versione.
        assert_eq!(apri().pezzo(500, 100).unwrap().byte.len(), 100);
        // Il file cambia sotto: non si cuce.
        *finto.versione.lock().unwrap() = "v10".to_owned();
        assert_eq!(
            apri().pezzo(600, 100).unwrap_err().code().kind(),
            rymembra_domain::errors::ErrorCodeKind::LontanoFileCambiato
        );
    }

    #[test]
    fn i_fotogrammi_del_cd_tornano_indietro() {
        assert_eq!(ms_da_cd(0), 0);
        assert_eq!(ms_da_cd(75), 1000);
        assert_eq!(ms_da_cd(-5), 0);
        // Andata e ritorno come in `ospiti::tratto`.
        for cd in [1_i64, 2, 3, 74, 76, 13_337, 330_000] {
            let ms = ms_da_cd(cd);
            let indietro = ms
                .saturating_mul(3)
                .saturating_add(20)
                .checked_div(40)
                .unwrap();
            assert_eq!(indietro, u64::try_from(cd).unwrap(), "fotogramma {cd}");
        }
    }

    #[test]
    fn le_chiavi_di_copertina() {
        let dispositivo = "a".repeat(64);
        let impronta = "0123456789abcdef";
        let chiave = chiave_copertina(&dispositivo, Some(impronta)).unwrap();
        assert_eq!(chiave, format!("lontano-{dispositivo}-{impronta}"));
        // Già tradotta: resta com'è.
        assert_eq!(
            chiave_copertina(&dispositivo, Some(&chiave)),
            Some(chiave.clone())
        );
        assert!(e_una_copertina(&chiave));
        assert_eq!(
            parti_della_copertina(&chiave),
            Some((dispositivo.as_str(), impronta))
        );
        // Quel che non è un'impronta non passa.
        for brutta in ["", "abc", "../../x", "0123456789ABCDEF", "0123456789abcdeg"] {
            assert_eq!(
                chiave_copertina(&dispositivo, Some(brutta)),
                None,
                "{brutta}"
            );
        }
        assert_eq!(chiave_copertina("corta", Some(impronta)), None);
        assert!(!e_una_copertina("lontano-x-0123456789abcdef"));
        assert!(!e_una_copertina(&format!(
            "lontano-{dispositivo}-{impronta}/../x"
        )));
        assert!(!e_una_copertina(&format!(
            "insieme-{dispositivo}-{impronta}"
        )));
    }

    #[test]
    fn le_estensioni() {
        assert_eq!(estensione_buona("FLAC").as_deref(), Some("flac"));
        assert_eq!(estensione_buona(".mp3").as_deref(), Some("mp3"));
        assert_eq!(estensione_buona(""), None);
        assert_eq!(estensione_buona("a/b"), None);
        assert_eq!(estensione_buona("lunghissima"), None);
    }

    /// La cache scende sotto la soglia togliendo le meno usate.
    #[test]
    fn la_cache_si_pota_dalle_piu_vecchie() {
        let cartella =
            std::env::temp_dir().join(format!("rymembra-prova-copertine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cartella);
        std::fs::create_dir_all(&cartella).unwrap();
        let mega = vec![0_u8; 1024 * 1024];
        let adesso = SystemTime::now();
        for i in 0..55_u64 {
            let file = cartella.join(format!("{i:016x}.jpg"));
            std::fs::write(&file, &mega).unwrap();
            let aperto = std::fs::File::options().write(true).open(&file).unwrap();
            aperto
                .set_modified(adesso - Duration::from_secs(1000 - i))
                .unwrap();
        }
        pota(&cartella);
        let rimasti: Vec<String> = std::fs::read_dir(&cartella)
            .unwrap()
            .map(|v| v.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(rimasti.len(), 40);
        assert!(!rimasti.contains(&format!("{:016x}.jpg", 0)));
        assert!(rimasti.contains(&format!("{:016x}.jpg", 54)));
        let _ = std::fs::remove_dir_all(&cartella);
    }
}
