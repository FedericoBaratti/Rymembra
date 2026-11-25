//! Lo store delle copertine: indirizzato dal contenuto, e ricodificato.
//!
//! # Perché non si salvano gli originali
//!
//! Misurato sulla libreria vera: le copertine incorporate nei file sono PNG da
//! cinquecento-seicento kilobyte l'una. Millequattrocento brani vorrebbero dire
//! **circa ottocento megabyte** di sole immagini, per mostrare quadratini da
//! duecento pixel in una griglia.
//!
//! Ricodificate a 640 pixel in JPEG stanno in una frazione di quello spazio, e
//! la differenza non si vede: nessuna interfaccia mostra una copertina d'album
//! a seicento kilobyte di dettaglio.
//!
//! # Perché indirizzato dal contenuto
//!
//! Il nome del file è l'impronta dei byte **originali**. Ne discendono tre cose
//! che valgono più della semplicità di un id progressivo:
//!
//! - la stessa copertina condivisa da dodici brani sta su disco una volta sola;
//! - cambiare i tag di un brano non orfana la sua immagine, perché l'immagine
//!   non appartiene al brano ma al suo contenuto;
//! - riscansionare non ricodifica niente: se l'impronta c'è già, il lavoro è
//!   fatto. È ciò che rende una riscansione veloce quanto deve essere.
//!
//! L'impronta è quella dell'originale e non del ricodificato: così cambiare
//! qualità o dimensione domani non fa perdere l'aggancio a ciò che è già
//! salvato, e permette di ricodificare senza rileggere i file musicali.
//!
//! # Perché lo store va anche svuotato
//!
//! Indirizzato dal contenuto vuol dire che una copertina non appartiene a
//! nessuno, e quindi che nessuno la butta quando smette di servire. Fino alla
//! 2.4 non la buttava nessuno davvero: i brani tolti dalla libreria, i file
//! ritaggati con un'immagine nuova e le copertine dell'arricchimento sostituite
//! da una migliore lasciavano la loro riga in `cover_art` e i loro due file sul
//! disco **per sempre**. [`dimentica_orfane`] e [`CoverStore::togli_tutte`]
//! sono la pulizia a conteggio di riferimenti: una copertina se ne va quando
//! nessuna riga del database la nomina più — e chi la nomina lo dice lo schema,
//! non un elenco scritto qui (vedi [`riferimenti`]).
//!
//! # Perché la decodifica ha dei limiti
//!
//! Un PNG da qualche kilobyte può dichiarare diecimila pixel per lato, e un
//! `deflate` ben costruito li riempie davvero: è la «bomba di decompressione».
//! Dentro un tag, arriva alla scansione come una copertina qualunque. Con i
//! limiti di serie di `image` — nessun tetto sui lati, 512 MiB di pixel — la
//! decodifica ne chiedeva mezzo gigabyte, più il ridimensionamento in virgola
//! mobile, per produrre un quadratino da 640 pixel.
//! [`LATO_MASSIMO_DECODIFICA`] e [`MEMORIA_MASSIMA_DECODIFICA`] chiudono la
//! porta prima che l'allocazione avvenga: `image` legge l'intestazione, fa i
//! conti e rifiuta. Il brano entra in libreria senza copertina, e l'esito della
//! scansione dice perché.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use image::imageops::FilterType;
use image::{ImageError, ImageReader, Limits};
use rusqlite::Connection;
use rymembra_domain::errors::{AppError, ErrorCode};

use crate::files::io_error;
use crate::scrittura::{e_temporaneo, scrivi_atomico};

/// Il lato massimo della copertina salvata.
///
/// 640 copre uno schermo ad alta densità che ne mostri una a tutta larghezza in
/// una colonna; oltre, si pagherebbero byte per pixel che nessuno guarda.
pub const MAX_LATO: u32 = 640;

/// Il lato massimo della miniatura, quella delle righe degli elenchi.
pub const MAX_LATO_MINIATURA: u32 = 160;

/// Il lato massimo della copertina media, quella delle schede delle griglie.
///
/// Una scheda è larga dai 164 pixel in su: la miniatura da 160 le stava
/// appena sotto a 100%, e a 200% era ingrandita più del doppio. La media la
/// copre a 100%; per il doppio dei pixel la finestra sceglie la piena (`srcset`
/// in `Copertina.tsx`).
///
/// # Perché si fa alla prima richiesta
///
/// Gli originali non si tengono (vedi la nota del modulo), quindi la media non
/// può nascere da loro per le copertine già nello store: nasce dalla piena, la
/// prima volta che qualcuno la chiede ([`CoverStore::media`]). Così le
/// copertine di prima restano valide così come sono, la scansione non paga una
/// terza ricodifica, e una copertina che nessuno guarda in una griglia non
/// occupa un terzo file.
pub const MAX_LATO_MEDIA: u32 = 320;

/// Qualità JPEG della copertina piena.
const QUALITA: u8 = 85;
/// Qualità della media: quella della piena, perché a 320 pixel in una scheda si
/// guarda ancora come un'immagine e non come un'etichetta.
const QUALITA_MEDIA: u8 = 85;
/// Qualità della miniatura: più bassa, perché a 160 pixel non si distingue.
const QUALITA_MINIATURA: u8 = 78;

/// Il lato più lungo che si accetta di decodificare, in pixel.
///
/// Vale 8192, e il perché: le copertine più grandi che circolano davvero — le
/// scansioni dei negozi «ad alta risoluzione» — stanno fra i tremila e i
/// seimila pixel; sopra c'è soltanto chi ha incollato una locandina, o chi sta
/// provando a far cadere il programma. Oltre questo lato l'immagine non si decodifica e il
/// brano resta senza copertina.
///
/// Il lato vale **anche da solo**, non solo dentro il conto della memoria: una
/// striscia da 1×100 000 pixel pesa poco, ma il ridimensionamento la tratta
/// come un'immagine di centomila righe.
pub const LATO_MASSIMO_DECODIFICA: u32 = 8192;

/// Quanta memoria può chiedere una decodifica, in byte: 192 MiB.
///
/// È il conto dei pixel decodificati, prima del ridimensionamento: un'immagine
/// RGB da 8000×8000 ci sta (183 MiB), una RGBA da 8192×8192 no (256 MiB). Il
/// valore di serie di `image` è 512 MiB, e non è tutto il conto: il filtro di
/// Lanczos lavora su una copia intermedia in virgola mobile, quattro byte per
/// canale, che ai limiti di serie aggiungeva centinaia di megabyte a quel mezzo
/// gigabyte — per ogni copertina, sul filo della scansione, mentre la musica
/// suona.
pub const MEMORIA_MASSIMA_DECODIFICA: u64 = 192 * 1024 * 1024;

/// Il lato più lungo, in pixel, di una copertina presa da un altro dispositivo:
/// 1024.
///
/// Un dispositivo vero serve la «media» da [`MAX_LATO_MEDIA`] pixel — o, per
/// «Ascolta insieme», la piena da [`MAX_LATO`] —: nessuna copertina vera ha
/// bisogno di più. I limiti dei tag ([`LATO_MASSIMO_DECODIFICA`], 8192) sono
/// pensati per le copertine dei file di chi ascolta; per i byte di un altro
/// dispositivo, che non si è scelto e che si decodificano a ogni riga che scorre,
/// erano una porta aperta: una JPEG progressiva 8192×8192 sta in 0,4 MiB e
/// costava ~580 MiB (reperto S8-2, misurato). Il lato vale anche da solo.
pub const LATO_MASSIMO_ESTRANEA: u32 = 1024;

/// Quanta memoria può chiedere la decodifica di una copertina di un altro
/// dispositivo, in byte: 32 MiB. Una 1024×1024 a quattro canali ne vuole 4: il
/// resto è margine. Il conto dei pixel non vede i buffer interni dei decoder:
/// per quelli vale il lato.
const MEMORIA_MASSIMA_ESTRANEA: u64 = 32 * 1024 * 1024;

/// Quante decodifiche di byte altrui possono andare insieme, in tutto il
/// processo. Una riga che scorre, una scheda del sistema e «Ascolta insieme»
/// ne chiedevano una ciascuna — tre per dispositivo e sei dalla finestra — tutte
/// sul pool bloccante; ora si mettono in fila.
const DECODIFICHE_ESTRANEE_INSIEME: usize = 2;

/// Le decodifiche di byte altrui in corso e il massimo visto: solo per le prove.
#[cfg(test)]
static DECODIFICHE_ESTRANEE: (
    std::sync::atomic::AtomicUsize,
    std::sync::atomic::AtomicUsize,
) = (
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
);

/// Quante decodifiche di byte altrui sono in corso adesso, e il lucchetto per
/// aspettare un posto.
static POSTI_DI_DECODIFICA: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// Un posto fra le [`DECODIFICHE_ESTRANEE_INSIEME`]: lo si tiene finché si lavora
/// sull'immagine, e si rende da solo — anche se il decoder cade.
struct PostoDiDecodifica;

impl PostoDiDecodifica {
    /// Aspetta un posto libero, e lo prende.
    fn prendi() -> Self {
        let (in_corso, avviso) = &POSTI_DI_DECODIFICA;
        let mut conto = in_corso.lock().unwrap_or_else(PoisonError::into_inner);
        while *conto >= DECODIFICHE_ESTRANEE_INSIEME {
            conto = avviso.wait(conto).unwrap_or_else(PoisonError::into_inner);
        }
        *conto += 1;
        Self
    }
}

impl Drop for PostoDiDecodifica {
    fn drop(&mut self) {
        let (in_corso, avviso) = &POSTI_DI_DECODIFICA;
        let mut conto = in_corso.lock().unwrap_or_else(PoisonError::into_inner);
        *conto = conto.saturating_sub(1);
        drop(conto);
        avviso.notify_one();
    }
}

/// Quanto spazio, al più, occupano nello store le copertine prese da altri
/// dispositivi: 256 MiB.
///
/// Una copertina di un altro dispositivo pesa 30–60 KB (la media da 320 pixel,
/// ricodificata a 640, più la miniatura): 256 MiB sono **qualche migliaio** di
/// dischi, più di quanti ne scorra chi guarda la libreria di un altro; con
/// immagini rumorose da 400 KB — il peggio che un catalogo ostile può servire — ne
/// bastano seicento, ma non di più: prima, con duecentomila righe e
/// nessun tetto, riempivano il disco man mano che la libreria scorreva, e niente
/// le toglieva finché la riga esisteva (reperto S8-9). Oltre il tetto le copertine
/// nuove **non si salvano**: la riga resta senza immagine finché non si fa posto
/// (si dimentica un dispositivo, che porta via le sue). La media di una copertina
/// già salvata si aggiunge, circa un decimo.
pub const SPAZIO_MASSIMO_ESTRANEE: u64 = 256 * 1024 * 1024;

/// Il tipo di un'immagine dai suoi primi byte: JPEG, PNG o WebP, le sole che una
/// copertina di un altro dispositivo può essere (decisione D-32). `None` per tutto
/// il resto: `image` apre anche GIF e BMP, due decoder in più sui byte di un altro.
#[must_use]
pub fn tipo_immagine(byte: &[u8]) -> Option<&'static str> {
    if byte.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if byte.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if byte.starts_with(b"RIFF") && byte.get(8..12) == Some(b"WEBP".as_slice()) {
        Some("image/webp")
    } else {
        None
    }
}

/// I limiti con cui ogni immagine di questo modulo si decodifica.
///
/// `Limits` è `#[non_exhaustive]`: si parte da quello senza limiti e si
/// riempiono i tre campi, così un campo aggiunto domani da `image` resta
/// spento invece di prendere un valore che nessuno qui ha scelto.
fn limiti() -> Limits {
    let mut limiti = Limits::no_limits();
    limiti.max_image_width = Some(LATO_MASSIMO_DECODIFICA);
    limiti.max_image_height = Some(LATO_MASSIMO_DECODIFICA);
    limiti.max_alloc = Some(MEMORIA_MASSIMA_DECODIFICA);
    limiti
}

/// I limiti con cui si decodifica una copertina di un altro dispositivo: i lati e
/// la memoria stretti di [`LATO_MASSIMO_ESTRANEA`] e [`MEMORIA_MASSIMA_ESTRANEA`].
fn limiti_estranee() -> Limits {
    let mut limiti = Limits::no_limits();
    limiti.max_image_width = Some(LATO_MASSIMO_ESTRANEA);
    limiti.max_image_height = Some(LATO_MASSIMO_ESTRANEA);
    limiti.max_alloc = Some(MEMORIA_MASSIMA_ESTRANEA);
    limiti
}

/// Decodifica un'immagine dentro i [`limiti`].
///
/// Una funzione sola per i due posti che decodificano — la ricodifica di
/// [`CoverStore::store`] e la lettura della tinta — perché due letture con due
/// politiche sono una porta chiusa e una aperta.
fn decodifica(byte: &[u8], hash: &str) -> Result<image::DynamicImage, AppError> {
    decodifica_con(byte, hash, limiti())
}

/// [`decodifica`] con i limiti che si danno: quelli dei tag, o quelli stretti dei
/// byte di un altro dispositivo ([`limiti_estranee`]).
fn decodifica_con(
    byte: &[u8],
    hash: &str,
    limiti: Limits,
) -> Result<image::DynamicImage, AppError> {
    let mut lettore = ImageReader::new(std::io::Cursor::new(byte))
        .with_guessed_format()
        .map_err(|err| decode_error(hash, &err.to_string()))?;
    lettore.limits(limiti);
    lettore.decode().map_err(|err| match err {
        // Un'immagine troppo grande non è un'immagine rotta, e il diario deve
        // poterle distinguere: la prima vuol dire «questo file ha una copertina
        // da cinquanta megapixel», la seconda «questo file ha dentro
        // spazzatura». Per il brano la conseguenza è la stessa — nessuna
        // copertina — ed è voluta.
        ImageError::Limits(limite) => {
            AppError::new(ErrorCode::MetadataTagReadFailed { path: None })
                .with_detail(format!(
                    "copertina {hash} non decodificabile: oltre i limiti di decodifica"
                ))
                .with_cause(limite.to_string())
        }
        altro => decode_error(hash, &altro.to_string()),
    })
}

/// Quanti byte può pesare una copertina arrivata da un altro PC: 1 MiB.
///
/// È il tetto con cui il proprietario la serve (la «piena» del suo archivio,
/// che a 640 pixel in JPEG sta molto sotto), ripetuto qui perché è qui che si
/// decide cosa entra: un PC fidato per il codice non è fidato per i byte, e
/// un'immagine da cinquanta megabyte non diventa una copertina solo perché è
/// arrivata cifrata.
pub const MASSIMO_ESTRANEA: usize = 1024 * 1024;

/// Una copertina arrivata da un altro PC, ricodificata da Rymembra.
///
/// Per «Ascolta insieme»: il membro riceve la copertina di una voce dal PC che
/// la possiede, e quel che arriva alla finestra non devono essere quei byte
/// così come sono. Passano da [`con_immagine_estranea`] — solo JPEG, PNG o WebP,
/// al più [`LATO_MASSIMO_ESTRANEA`] pixel letti dall'intestazione, due decodifiche
/// insieme — e ne esce una JPEG nostra, ridotta a [`MAX_LATO`] con la qualità
/// della piena:
/// un formato solo, che il visualizzatore della finestra apre senza sorprese,
/// e senza i metadati dell'originale (EXIF, profili, commenti), che non sono
/// affari di chi ascolta.
///
/// # Niente disco, niente store
///
/// Non scrive niente e non tocca [`CoverStore`]. Chi chiama la tiene in
/// memoria per la durata della sessione: nello store la pulizia delle orfane
/// la cancellerebbe al primo giro — nessuna riga del database la nomina — e
/// finché c'era sporcherebbe la libreria con l'immagine di un disco che qui
/// non esiste.
///
/// # Errori
///
/// Lo stesso codice di ogni copertina illeggibile (`metadata.tagReadFailed`):
/// per byte che non sono un'immagine, per un'immagine che non è JPEG, PNG o
/// WebP, per un'immagine oltre i limiti di decodifica, e per più di
/// [`MASSIMO_ESTRANEA`] byte in ingresso — rifiutati prima di guardarli. Il
/// messaggio dice «copertina estranea» e non un'impronta: un'impronta di byte
/// altrui nel diario non aiuta nessuno.
pub fn ricodifica_estranea(byte: &[u8]) -> Result<Vec<u8>, AppError> {
    const ETICHETTA: &str = "estranea";
    con_immagine_estranea(byte, ETICHETTA, |immagine| {
        encode_jpeg(&fit(&immagine, MAX_LATO), QUALITA, ETICHETTA)
    })
}

/// Lavora su un'immagine presa da un altro dispositivo, **decodificata** con i
/// limiti stretti e con un posto fra le [`DECODIFICHE_ESTRANEE_INSIEME`].
///
/// L'unica porta da cui i byte di un altro dispositivo arrivano a un decoder, per
/// [`ricodifica_estranea`] e per [`CoverStore::salva_estranea`]. In ordine, dal
/// più economico: il peso ([`MASSIMO_ESTRANEA`]), il formato ([`tipo_immagine`]:
/// solo JPEG, PNG o WebP), i lati dall'**intestazione**, senza decodificare
/// ([`LATO_MASSIMO_ESTRANEA`]); poi un posto, e la decodifica dentro
/// [`limiti_estranee`]. Il posto si tiene finché dura `lavoro`, che è la
/// riduzione e la ricodifica: è lì che sta la memoria.
fn con_immagine_estranea<T>(
    byte: &[u8],
    etichetta: &str,
    lavoro: impl FnOnce(image::DynamicImage) -> Result<T, AppError>,
) -> Result<T, AppError> {
    if byte.len() > MASSIMO_ESTRANEA {
        return Err(decode_error(
            etichetta,
            &format!(
                "{} byte in ingresso, il tetto è {MASSIMO_ESTRANEA}",
                byte.len()
            ),
        ));
    }
    if tipo_immagine(byte).is_none() {
        return Err(decode_error(
            etichetta,
            "una copertina di un altro dispositivo è un'immagine JPEG, PNG o WebP",
        ));
    }
    let lettore = ImageReader::new(std::io::Cursor::new(byte))
        .with_guessed_format()
        .map_err(|err| decode_error(etichetta, &err.to_string()))?;
    let (larghezza, altezza) = lettore
        .into_dimensions()
        .map_err(|err| decode_error(etichetta, &err.to_string()))?;
    if larghezza > LATO_MASSIMO_ESTRANEA || altezza > LATO_MASSIMO_ESTRANEA {
        return Err(oltre_i_limiti(
            etichetta,
            &format!("{larghezza}×{altezza} pixel, il lato massimo è {LATO_MASSIMO_ESTRANEA}"),
        ));
    }
    let _posto = PostoDiDecodifica::prendi();
    #[cfg(test)]
    let _conta = ContaDecodifiche::nuova();
    // Un decoder che cade su un input ostile non porta con sé chi lo chiama: il
    // filo delle copertine di «Ascolta insieme», per esempio, ne serve una per
    // voce, e uno solo che cadesse lascerebbe senza copertina tutte le voci dopo
    // (reperto S8-7). Il panico si dice come ogni altra copertina illeggibile, e il
    // posto si rende comunque.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let immagine = decodifica_con(byte, etichetta, limiti_estranee())?;
        lavoro(immagine)
    }))
    .unwrap_or_else(|_| {
        Err(decode_error(
            etichetta,
            "il decoder è caduto su questi byte",
        ))
    })
}

/// Un'immagine troppo grande, detta come la dice [`decodifica`]: non è un'immagine
/// rotta, e il diario deve poterle distinguere.
fn oltre_i_limiti(hash: &str, causa: &str) -> AppError {
    AppError::new(ErrorCode::MetadataTagReadFailed { path: None })
        .with_detail(format!(
            "copertina {hash} non decodificabile: oltre i limiti di decodifica"
        ))
        .with_cause(causa.to_owned())
}

#[cfg(test)]
struct ContaDecodifiche;

#[cfg(test)]
impl ContaDecodifiche {
    fn nuova() -> Self {
        use std::sync::atomic::Ordering;
        let adesso = DECODIFICHE_ESTRANEE.0.fetch_add(1, Ordering::SeqCst) + 1;
        DECODIFICHE_ESTRANEE.1.fetch_max(adesso, Ordering::SeqCst);
        Self
    }
}

#[cfg(test)]
impl Drop for ContaDecodifiche {
    fn drop(&mut self) {
        DECODIFICHE_ESTRANEE
            .0
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Un'impronta è esadecimale **minuscola**, da 2 a 128 cifre, e nient'altro.
///
/// È l'invariante su cui lo store costruisce i suoi percorsi, e sta qui perché
/// è lo store che lo possiede: chi gli passa un'impronta non deve poterlo
/// portare fuori dalla sua cartella, qualunque cosa ci sia scritto. Cifre
/// esadecimali e basta non compongono né `..` né barre né i due punti di
/// un'unità, e nemmeno un nome riservato di Windows.
///
/// Minuscola, e non per pignoleria: su NTFS `AB/ABCD….jpg` e `ab/abcd….jpg`
/// sono lo stesso file, mentre per SQLite sono due righe. Un gemello maiuscolo
/// orfano in `cover_art` avrebbe fatto togliere alla pulizia la copertina di
/// chi la usa ancora. Le impronte che Rymembra scrive — blake3 in
/// esadecimale — sono minuscole per costruzione.
///
/// Lo usano anche la lettura dei profili (i nomi delle voci e le righe della
/// biblioteca): un criterio scritto in due posti è due criteri.
#[must_use]
pub fn e_un_impronta(hash: &str) -> bool {
    (2..=128).contains(&hash.len())
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// La cartella e il nome, dentro lo store, di ciò che non è un'impronta: vedi
/// [`CoverStore::percorso`]. Non è esadecimale, quindi lo store non lo scrive e
/// [`CoverStore::spazza_avanzi`] non lo tocca.
const NON_UN_IMPRONTA: &str = "__";

/// Da dove viene una copertina. Serve a decidere se una trovata online debba
/// sostituirne una già presente, o lasciarla stare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverSource {
    /// Incorporata nei tag del file.
    Tag,
    /// Da un servizio di metadati.
    Provider,
    /// Da Spotify.
    Spotify,
    /// Dal Cover Art Archive.
    CoverArtArchive,
    /// Scelta a mano dall'utente.
    Manual,
}

impl CoverSource {
    /// Il nome che finisce nel database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::Provider => "provider",
            Self::Spotify => "spotify",
            Self::CoverArtArchive => "caa",
            Self::Manual => "manual",
        }
    }
}

/// Una copertina salvata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCover {
    /// L'impronta dei byte originali: il nome del file e la chiave nel database.
    pub hash: String,
    /// Larghezza dell'immagine salvata.
    pub width: u32,
    /// Altezza dell'immagine salvata.
    pub height: u32,
    /// Quanto occupa su disco la versione piena.
    pub byte_size: u64,
    /// Il tipo dell'immagine salvata.
    pub mime_type: &'static str,
    /// Era già presente: non è stata ricodificata.
    pub already_present: bool,
}

/// Lo store su disco.
#[derive(Debug, Clone)]
pub struct CoverStore {
    root: PathBuf,
    /// Quanti byte occupano le copertine prese da altri dispositivi; `None`
    /// finché non si è guardato il disco. Condiviso fra le copie dello store: è
    /// una sola cosa.
    estranee: Arc<Mutex<Option<u64>>>,
}

impl CoverStore {
    /// Apre (o crea) lo store nella cartella data.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, AppError> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|err| io_error(&root.display().to_string(), &err))?;
        Ok(Self {
            root,
            estranee: Arc::new(Mutex::new(None)),
        })
    }

    /// La cartella dello store.
    ///
    /// Serve a chi deve dire **dove dentro** lo store sta una copertina: su
    /// Android le copertine le serve all'auto un `ContentProvider`, che accetta
    /// solo percorsi relativi a questa cartella e non esce mai da lì.
    #[must_use]
    pub fn radice(&self) -> &std::path::Path {
        &self.root
    }

    /// Il percorso della copertina piena.
    ///
    /// I file sono divisi in sottocartelle per i primi due caratteri
    /// dell'impronta. Non è eleganza: una cartella con decine di migliaia di
    /// voci rallenta ogni `readdir`, e su qualche filesystem molto più che
    /// linearmente.
    ///
    /// Sempre **dentro** lo store, qualunque cosa riceva: vedi
    /// [`CoverStore::percorso`].
    #[must_use]
    pub fn path_for(&self, hash: &str) -> PathBuf {
        self.percorso(hash, "jpg")
    }

    /// Il percorso della miniatura.
    #[must_use]
    pub fn thumbnail_path_for(&self, hash: &str) -> PathBuf {
        self.percorso(hash, "t.jpg")
    }

    /// Il percorso della copertina media: `<impronta>.m.jpg`, accanto alle altre
    /// due e con lo stesso schema, così la pulizia la riconosce come loro.
    #[must_use]
    pub fn percorso_media(&self, hash: &str) -> PathBuf {
        self.percorso(hash, "m.jpg")
    }

    /// Il segno di una copertina presa da un altro dispositivo:
    /// `<impronta>.lontano`, un file vuoto accanto alla piena. Dice che la
    /// «piena» è la media che l'altro serve (320 pixel, ricodificata) e non la
    /// copertina di questa macchina: finché c'è, [`CoverStore::store`] non si
    /// fida del file e lo rifà dall'originale.
    fn percorso_segno(&self, hash: &str) -> PathBuf {
        self.percorso(hash, "lontano")
    }

    /// L'unico posto in cui un'impronta diventa un percorso.
    ///
    /// Un'impronta che non è un'impronta ([`e_un_impronta`]) arriva da fuori —
    /// la riga di un profilo costruito a mano, un indirizzo chiesto dalla
    /// finestra o da un altro PC — e unita così com'è alla radice usciva dallo
    /// store: `..` risaliva, un assoluto o un prefisso d'unità la sostituiva, un
    /// `\\host` apriva una sessione di rete. Per quella si restituisce un nome
    /// **dentro** lo store che lo store non scrive mai e che la pulizia non
    /// riconosce come suo (`__/__.<coda>`): chi legge trova una copertina che
    /// manca, che è la verità. Chi scrive o cancella non arriva fin qui —
    /// [`CoverStore::togli_tutte`] e [`CoverStore::media`] la scartano prima.
    ///
    /// Un nome e non un `Option`: i chiamanti sono una dozzina, in tre crate, e
    /// trattano già «il file non c'è» come una copertina che manca.
    fn percorso(&self, hash: &str, coda: &str) -> PathBuf {
        if !e_un_impronta(hash) {
            return self
                .root
                .join(NON_UN_IMPRONTA)
                .join(format!("{NON_UN_IMPRONTA}.{coda}"));
        }
        // Le impronte sono ASCII di almeno due cifre: il prefisso c'è sempre.
        let prefisso = hash.get(..2).unwrap_or(NON_UN_IMPRONTA);
        self.root.join(prefisso).join(format!("{hash}.{coda}"))
    }

    /// I byte della copertina media, facendola dalla piena se non c'è ancora.
    ///
    /// `None` se non c'è nemmeno la piena: una copertina che manca, che per chi
    /// la chiede è un disco senza immagine e non un guasto. La media scritta
    /// passa da [`scrivi_atomico`] come le altre: due richieste della stessa
    /// copertina nello stesso momento la fanno tutte e due, e l'ultima
    /// rinomina vince su un file che è comunque lo stesso.
    ///
    /// Vedi [`MAX_LATO_MEDIA`] per perché nasce qui e non nella scansione.
    ///
    /// # Errori
    ///
    /// `metadata.tagReadFailed` se la piena sul disco non si decodifica; `fs.*`
    /// se la media non si scrive.
    pub fn media(&self, hash: &str) -> Result<Option<Vec<u8>>, AppError> {
        // Una che non è un'impronta non ha una piena da cui nascere, e la media
        // non si scrive nemmeno nel nome di ripiego di [`CoverStore::percorso`].
        if !e_un_impronta(hash) {
            return Ok(None);
        }
        let percorso = self.percorso_media(hash);
        if let Ok(byte) = std::fs::read(&percorso) {
            return Ok(Some(byte));
        }
        let Ok(piena) = std::fs::read(self.path_for(hash)) else {
            return Ok(None);
        };
        // Con i limiti anche qui, come per la tinta: la piena l'ha scritta
        // Rymembra, ma la cartella dati è una cartella come un'altra.
        let immagine = decodifica(&piena, hash)?;
        let byte = encode_jpeg(&fit(&immagine, MAX_LATO_MEDIA), QUALITA_MEDIA, hash)?;
        scrivi_atomico(&percorso, &byte)?;
        Ok(Some(byte))
    }

    /// La tinta dominante di una copertina, dalla sua miniatura.
    ///
    /// Dalla miniatura e non dalla piena: sono gli stessi colori nelle stesse
    /// proporzioni — è una riduzione della stessa immagine — e sono sedici volte
    /// meno pixel da decodificare. La scelta di quale colore vince sta in
    /// [`crate::tinta::dominante`]; qui c'è solo la lettura.
    ///
    /// `None` per ogni motivo per cui potrebbe non esserci: file assente,
    /// illeggibile, o una copertina in bianco e nero. Nessuno di questi è un
    /// guasto da riportare — è un disco che non tinge l'interfaccia, e
    /// l'interfaccia ha già il suo accento.
    #[must_use]
    pub fn tinta(&self, hash: &str) -> Option<[u8; 3]> {
        if !e_un_impronta(hash) {
            return None;
        }
        let bytes = std::fs::read(self.thumbnail_path_for(hash)).ok()?;
        // Con i limiti anche qui, benché la miniatura l'abbia scritta Rymembra:
        // la cartella dati è una cartella come un'altra, e un file messo al
        // posto di una miniatura non deve valere più di una copertina in un tag.
        let immagine = decodifica(&bytes, hash).ok()?;
        crate::tinta::dominante(&immagine.to_rgb8())
    }

    /// Salva una copertina, ricodificandola. Se c'è già, non fa niente.
    pub fn store(&self, original: &[u8], source: CoverSource) -> Result<StoredCover, AppError> {
        let _ = source; // la provenienza la registra il database, non lo store
        let hash = blake3::hash(original).to_hex().to_string();
        let full = self.path_for(&hash);
        let segno = self.percorso_segno(&hash);

        // Una «piena» presa da un altro dispositivo (vedi `salva_estranea`) non
        // vale come quella già salvata: la scansione dell'originale la rifà.
        if let Ok(meta) = std::fs::metadata(&full)
            && !segno.exists()
        {
            // Già presente: si legge la dimensione senza decodificare. È il
            // caso normale di una riscansione, e deve costare quanto uno `stat`
            // — due, con quello della miniatura.
            //
            // La miniatura si guarda anche lei: l'ordine di scrittura qui sotto
            // tiene «piena ⇒ miniatura», ma un programma di pulizia che toglie
            // i `*.t.jpg`, o una cancellazione riuscita a metà, la rompono, e
            // da lì la copertina sarebbe un segnaposto negli elenchi per
            // sempre: questo ramo era l'unico che poteva rifarla, e non la
            // guardava. Si rifà dall'originale, come la prima volta; succede
            // di rado, e allora si paga una decodifica.
            let miniatura = self.thumbnail_path_for(&hash);
            if !miniatura.exists() {
                let piena = fit(&decodifica(original, &hash)?, MAX_LATO);
                let byte = encode_jpeg(&fit(&piena, MAX_LATO_MINIATURA), QUALITA_MINIATURA, &hash)?;
                scrivi_atomico(&miniatura, &byte)?;
            }
            let (width, height) = imagesize::blob_size(original)
                .map(|s| {
                    (
                        u32::try_from(s.width).unwrap_or(0),
                        u32::try_from(s.height).unwrap_or(0),
                    )
                })
                .unwrap_or((0, 0));
            // Le dimensioni da riportare sono quelle del file sul disco —
            // passato da `fit`, che scala in proporzione — non quelle
            // dell'originale schiacciate per asse: un originale 2000×1000 sta
            // sul disco come 640×320, e dirlo 640×640 falserebbe lo spareggio
            // con cui `pick_album_cover` sceglie la copertina di un album.
            let (width, height) = dimensioni_ridotte(width, height, MAX_LATO);
            return Ok(StoredCover {
                hash,
                width,
                height,
                byte_size: meta.len(),
                mime_type: "image/jpeg",
                already_present: true,
            });
        }

        // Se questa «piena» era quella di un altro dispositivo, ne esce dal conto
        // delle estranee: i suoi file (e la media di prima) sono ormai di qui.
        let della_estranea = if segno.exists() {
            self.byte_dei_file_di(&hash)
        } else {
            0
        };
        let decoded = decodifica(original, &hash)?;

        let piena = fit(&decoded, MAX_LATO);
        // La miniatura dalla piena e non dall'originale: Lanczos3 su una
        // copertina da tremila pixel costa quanto la decodifica, e dalla 640 il
        // conto si fa su un ventesimo dei pixel. A 160 pixel la differenza fra
        // le due strade non si vede, e una scansione di dischi nuovi ne paga
        // una per copertina.
        let miniatura = fit(&piena, MAX_LATO_MINIATURA);
        let width = piena.width();
        let height = piena.height();

        let bytes_piena = encode_jpeg(&piena, QUALITA, &hash)?;
        let bytes_miniatura = encode_jpeg(&miniatura, QUALITA_MINIATURA, &hash)?;

        let byte_size = bytes_piena.len() as u64;
        // Una scrittura interrotta a metà lascerebbe nello store un file
        // parziale col nome giusto — e siccome lo store si fida del nome, quel
        // file verrebbe servito per sempre come se fosse valido. Vedi
        // [`crate::scrittura`].
        //
        // La miniatura **prima** della piena, ed è l'ordine che conta: il ramo
        // «già presente» decide guardando la piena, quindi una piena scritta e
        // una miniatura persa a metà sarebbero una copertina senza miniatura
        // finché quel ramo non la rifà (costa una decodifica). Al contrario,
        // una miniatura senza la piena si riscrive da sé alla prossima
        // scansione di quel file.
        scrivi_atomico(&self.thumbnail_path_for(&hash), &bytes_miniatura)?;
        scrivi_atomico(&full, &bytes_piena)?;
        // Per ultimo: se si cade prima, il segno resta e la prossima volta si
        // rifà.
        if std::fs::remove_file(&segno).is_ok() {
            self.aggiorna_le_estranee(|usato| usato.saturating_sub(della_estranea));
        }

        Ok(StoredCover {
            hash,
            width,
            height,
            byte_size,
            mime_type: "image/jpeg",
            already_present: false,
        })
    }

    /// Salva la copertina che un altro dispositivo ha servito, **sotto
    /// l'impronta che lui dice**.
    ///
    /// # Perché non [`CoverStore::store`]
    ///
    /// Perché `Domanda::Copertina` non restituisce l'originale: restituisce la
    /// «media» (320 pixel, ricodificata, [`CoverStore::media`]). L'impronta di
    /// quei byte non è quella dell'originale, che è il nome con cui la
    /// libreria dell'altro — e, per lo stesso disco, quella di qui — chiama la
    /// copertina; e `store` darebbe un nome diverso da quello che la riga
    /// specchio porta. Qui il nome lo dà chi chiama, **dopo** aver controllato
    /// che sia un'impronta ([`e_un_impronta`]).
    ///
    /// # Cosa si fida
    ///
    /// Niente dei byte: passano da [`con_immagine_estranea`] — il tetto di
    /// [`MASSIMO_ESTRANEA`], solo JPEG, PNG o WebP, al più
    /// [`LATO_MASSIMO_ESTRANEA`] pixel dall'intestazione, due decodifiche insieme —
    /// e si ricodificano come JPEG nostra. Se
    /// qui c'è già una copertina con quell'impronta — locale, o presa prima —
    /// non si tocca: un dispositivo collegato non decide cosa c'è sullo store
    /// di chi l'ha collegato.
    ///
    /// # Il segno
    ///
    /// La piena che nasce da qui è la media di un altro, non l'originale: ne
    /// resta un segno ([`CoverStore::percorso_segno`]), scritto **per
    /// primo**, e la scansione di un file con quello stesso originale la rifà
    /// a 640 pixel invece di fidarsi di lei.
    ///
    /// # Errori
    ///
    /// `metadata.tagReadFailed` per un'impronta che non lo è, per byte che non
    /// sono un'immagine nei limiti, o più di [`MASSIMO_ESTRANEA`]; `fs.*` se
    /// non si scrive.
    pub fn salva_estranea(&self, hash: &str, byte: &[u8]) -> Result<StoredCover, AppError> {
        const ETICHETTA: &str = "estranea";
        if !e_un_impronta(hash) {
            return Err(decode_error(ETICHETTA, "un'impronta che non lo è"));
        }
        let piena = self.path_for(hash);
        if let Ok(meta) = std::fs::metadata(&piena) {
            let (width, height) = imagesize::size(&piena)
                .map(|s| {
                    (
                        u32::try_from(s.width).unwrap_or(0),
                        u32::try_from(s.height).unwrap_or(0),
                    )
                })
                .unwrap_or((0, 0));
            return Ok(StoredCover {
                hash: hash.to_owned(),
                width,
                height,
                byte_size: meta.len(),
                mime_type: "image/jpeg",
                already_present: true,
            });
        }
        // Lo spazio prima di decodificare: una copertina che non si può tenere
        // non vale una decodifica.
        if !self.ha_spazio_per_estranee(u64::try_from(byte.len()).unwrap_or(u64::MAX)) {
            return Err(decode_error(
                ETICHETTA,
                "lo spazio per le copertine degli altri dispositivi è finito",
            ));
        }
        let (grande, bytes_piena, bytes_miniatura) =
            con_immagine_estranea(byte, ETICHETTA, |immagine| {
                let grande = fit(&immagine, MAX_LATO);
                let miniatura = fit(&grande, MAX_LATO_MINIATURA);
                let bytes_piena = encode_jpeg(&grande, QUALITA, ETICHETTA)?;
                let bytes_miniatura = encode_jpeg(&miniatura, QUALITA_MINIATURA, ETICHETTA)?;
                Ok((grande, bytes_piena, bytes_miniatura))
            })?;
        // Lo spazio si guarda anche su quel che si scrive davvero: la piena ricodificata e
        // la miniatura possono pesare molto più dei byte arrivati — una PNG di 60 KB di
        // rumore che si ripete diventa una JPEG di 400 KB —, e il controllo di sopra vede
        // solo quelli: con un margine più largo dell'ingresso il tetto restava superato.
        let scritti = u64::try_from(bytes_piena.len() + bytes_miniatura.len()).unwrap_or(u64::MAX);
        if !self.ha_spazio_per_estranee(scritti) {
            return Err(decode_error(
                ETICHETTA,
                "lo spazio per le copertine degli altri dispositivi è finito",
            ));
        }
        // Il segno prima, la miniatura poi, la piena per ultima: la stessa
        // regola di `store` («piena ⇒ miniatura»), con il segno davanti a
        // tutto, perché una piena senza segno sembrerebbe quella vera.
        scrivi_atomico(&self.percorso_segno(hash), b"")?;
        scrivi_atomico(&self.thumbnail_path_for(hash), &bytes_miniatura)?;
        scrivi_atomico(&piena, &bytes_piena)?;
        self.aggiorna_le_estranee(|usato| usato.saturating_add(scritti));
        Ok(StoredCover {
            hash: hash.to_owned(),
            width: grande.width(),
            height: grande.height(),
            byte_size: bytes_piena.len() as u64,
            mime_type: "image/jpeg",
            already_present: false,
        })
    }

    /// C'è posto, sotto [`SPAZIO_MASSIMO_ESTRANEE`], per altri `byte` di copertine
    /// prese da altri dispositivi?
    ///
    /// La prima volta si guarda il disco — i segni (`<impronta>.lontano`) dicono
    /// quali copertine vengono da fuori, e se ne sommano i file —; poi il conto
    /// lo tengono [`CoverStore::salva_estranea`], [`CoverStore::togli_tutte`] e
    /// [`CoverStore::store`]. Chi vuole chiedere una copertina a un altro
    /// dispositivo lo chiede prima: è inutile scaricare quel che non si può tenere.
    #[must_use]
    pub fn ha_spazio_per_estranee(&self, byte: u64) -> bool {
        let mut conto = self.estranee.lock().unwrap_or_else(PoisonError::into_inner);
        let usato = *conto.get_or_insert_with(|| self.byte_delle_estranee_sul_disco());
        usato.saturating_add(byte) <= SPAZIO_MASSIMO_ESTRANEE
    }

    /// Cambia il conto delle estranee, se si è già guardato il disco: altrimenti
    /// la prima volta lo vedrà com'è.
    fn aggiorna_le_estranee(&self, cambia: impl FnOnce(u64) -> u64) {
        let mut conto = self.estranee.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(usato) = conto.as_mut() {
            *usato = cambia(*usato);
        }
    }

    /// Quanto pesano sul disco la piena, la media e la miniatura di `hash`.
    fn byte_dei_file_di(&self, hash: &str) -> u64 {
        [
            self.path_for(hash),
            self.percorso_media(hash),
            self.thumbnail_path_for(hash),
        ]
        .iter()
        .filter_map(|percorso| std::fs::metadata(percorso).ok())
        .fold(0, |somma, dati| somma.saturating_add(dati.len()))
    }

    /// Quanto pesano sul disco le copertine che hanno il segno di quelle prese da
    /// un altro dispositivo: piena, miniatura e media.
    fn byte_delle_estranee_sul_disco(&self) -> u64 {
        let file = self.file_dello_store();
        let estranee: HashSet<&str> = file
            .iter()
            .filter_map(|(percorso, _)| {
                percorso
                    .file_name()?
                    .to_str()?
                    .strip_suffix(".lontano")
                    .filter(|hash| e_un_impronta(hash))
            })
            .collect();
        file.iter()
            .filter(|(percorso, _)| {
                percorso
                    .file_name()
                    .and_then(|nome| nome.to_str())
                    .and_then(|nome| {
                        nome.strip_suffix(".t.jpg")
                            .or_else(|| nome.strip_suffix(".m.jpg"))
                            .or_else(|| nome.strip_suffix(".jpg"))
                    })
                    .is_some_and(|hash| estranee.contains(hash))
            })
            .map(|(_, dati)| dati.len())
            .fold(0, u64::saturating_add)
    }

    /// Quanto occupa lo store sul disco: file e byte, miniature comprese.
    ///
    /// Si cammina la cartella invece di sommare `cover_art.byte_size`, per due
    /// ragioni: quella colonna conta solo la copertina piena e non la
    /// miniatura, e non vede quel che sta sul disco senza una riga — gli avanzi
    /// che la pulizia esiste per togliere. Il numero che si mostra all'utente
    /// dev'essere quello che il disco libererebbe, non quello che il database
    /// crede.
    ///
    /// Costa una lettura di cartella per ognuna delle (al più) 256
    /// sottocartelle: qualche decina di millisecondi su una libreria grande, e
    /// per questo chi la chiama lo fa lontano dal filo della finestra. Una
    /// cartella che non si legge conta zero invece di far fallire tutto: è una
    /// misura, non un inventario.
    #[must_use]
    pub fn occupazione(&self) -> Occupazione {
        let mut occupazione = Occupazione::default();
        for (_, dati) in self.file_dello_store() {
            occupazione.file = occupazione.file.saturating_add(1);
            occupazione.byte = occupazione.byte.saturating_add(dati.len());
        }
        occupazione
    }

    /// Toglie dal disco la copertina piena, la media e la miniatura di ogni
    /// impronta.
    ///
    /// Un file che non c'è già non è un guasto: è il risultato che si voleva.
    /// Uno che non si lascia cancellare — aperto da un altro processo senza
    /// condivisione, una cartella diventata di sola lettura — resta dov'è e
    /// semplicemente non si conta: la sua riga è già andata, e
    /// [`CoverStore::spazza_avanzi`] lo ritroverà alla pulizia dopo.
    ///
    /// La piena per prima, e se resta, restano anche media e miniatura: è
    /// l'invariante «piena ⇒ miniatura» su cui conta il ramo «già presente» di
    /// [`CoverStore::store`], se la stessa copertina torna in libreria prima
    /// della pulizia dopo.
    ///
    /// Un'impronta che non è un'impronta ([`e_un_impronta`]) si salta, senza
    /// chiedere niente al disco. Le impronte arrivano da `cover_art`, e una
    /// riga scritta da un profilo costruito a mano poteva nominare `..`, un
    /// assoluto o il gemello maiuscolo di una copertina in uso: la sua riga se
    /// n'è già andata, e un file suo nello store non c'è mai stato.
    #[must_use]
    pub fn togli_tutte(&self, hashes: &[String]) -> Liberato {
        let mut liberato = Liberato::default();
        for hash in hashes.iter().filter(|hash| e_un_impronta(hash)) {
            let piena = self.path_for(hash);
            let tolta = togli_file(&piena);
            if tolta.is_none() && piena.exists() {
                continue;
            }
            liberato.aggiungi(tolta);
            let mut tolti = tolta.unwrap_or(0);
            for percorso in [self.percorso_media(hash), self.thumbnail_path_for(hash)] {
                let tolto = togli_file(&percorso);
                tolti = tolti.saturating_add(tolto.unwrap_or(0));
                liberato.aggiungi(tolto);
            }
            // Il segno di una copertina presa da fuori non ha peso e non si
            // conta, ma non deve restare; e il suo spazio torna a chi ne ha.
            if std::fs::remove_file(self.percorso_segno(hash)).is_ok() {
                self.aggiorna_le_estranee(|usato| usato.saturating_sub(tolti));
            }
        }
        liberato
    }

    /// Toglie quel che sta nello store senza stare nel database.
    ///
    /// Due generi di avanzi, e nient'altro:
    ///
    /// - i temporanei di una scrittura interrotta (vedi
    ///   [`crate::scrittura::e_temporaneo`]), e i provvisori di un'estrazione
    ///   da profilo interrotta (`<impronta>.parziale`);
    /// - le copertine e le miniature la cui impronta **non ha una riga** in
    ///   `cover_art` (`registrate` è l'elenco delle impronte che ce l'hanno).
    ///   Nascono quando una scansione annullata ha salvato le immagini dei file
    ///   letti ma non ha fatto in tempo a registrarli, o quando una pulizia
    ///   precedente ha tolto la riga e non è riuscita a togliere il file.
    ///
    /// # Perché l'età
    ///
    /// Tutti e due si toccano solo se più vecchi di `eta_minima`. Un
    /// temporaneo giovane può essere la scrittura in corso di un altro filo, e
    /// una copertina giovane senza riga è quasi sempre una copertina appena
    /// salvata da una scansione che la registrerà fra un istante: la riga
    /// arriva **dopo** il file, per costruzione, perché il file si scrive
    /// fuori dal lucchetto della libreria.
    ///
    /// Un file con un nome che questo modulo non ha scritto non si tocca, a
    /// nessuna età: la cartella è di Rymembra, ma non abbastanza da cancellare
    /// quel che non si riconosce.
    #[must_use]
    pub fn spazza_avanzi(&self, registrate: &HashSet<String>, eta_minima: Duration) -> Liberato {
        let mut liberato = Liberato::default();
        let adesso = std::time::SystemTime::now();
        for (percorso, dati) in self.file_dello_store() {
            // Un orologio che non si legge, o un file «dal futuro» su una
            // macchina con la data sbagliata, vale giovane: nel dubbio si lascia.
            let vecchio = dati
                .modified()
                .ok()
                .and_then(|quando| adesso.duration_since(quando).ok())
                .is_some_and(|eta| eta >= eta_minima);
            if !vecchio {
                continue;
            }
            let avanzo = e_temporaneo(&percorso)
                || provvisorio_di_estrazione(&percorso)
                || impronta_del_file(&percorso).is_some_and(|hash| !registrate.contains(hash));
            if avanzo {
                liberato.aggiungi(togli_file(&percorso));
            }
        }
        liberato
    }

    /// Tutti i file dello store, con i loro metadati.
    ///
    /// Solo le sottocartelle a due caratteri e i file lì dentro: è la forma
    /// che [`CoverStore::path_for`] produce, e quel che sta fuori da quella
    /// forma non è dello store. Le cartelle e i file che non si leggono si
    /// saltano.
    fn file_dello_store(&self) -> Vec<(PathBuf, std::fs::Metadata)> {
        let mut file = Vec::new();
        let Ok(cartelle) = std::fs::read_dir(&self.root) else {
            return file;
        };
        for cartella in cartelle.flatten() {
            let nome = cartella.file_name();
            let di_due = nome.to_str().is_some_and(|n| n.chars().count() == 2);
            if !di_due || !cartella.file_type().is_ok_and(|tipo| tipo.is_dir()) {
                continue;
            }
            let Ok(voci) = std::fs::read_dir(cartella.path()) else {
                continue;
            };
            for voce in voci.flatten() {
                if let Ok(dati) = voce.metadata()
                    && dati.is_file()
                {
                    file.push((voce.path(), dati));
                }
            }
        }
        file
    }
}

/// Quanto occupa lo store: vedi [`CoverStore::occupazione`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Occupazione {
    /// Quanti file, copertine piene e miniature insieme.
    pub file: u64,
    /// Quanti byte.
    pub byte: u64,
}

/// Quanto ha liberato un giro di cancellazioni.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Liberato {
    /// Quanti file se ne sono andati.
    pub file: u64,
    /// Quanti byte occupavano.
    pub byte: u64,
}

impl Liberato {
    /// Somma l'esito di una cancellazione: `None` vuol dire «non c'era», o
    /// «non si è lasciato togliere», e non conta.
    fn aggiungi(&mut self, tolto: Option<u64>) {
        if let Some(byte) = tolto {
            self.file = self.file.saturating_add(1);
            self.byte = self.byte.saturating_add(byte);
        }
    }

    /// La somma di due giri.
    #[must_use]
    pub fn piu(self, altro: Self) -> Self {
        Self {
            file: self.file.saturating_add(altro.file),
            byte: self.byte.saturating_add(altro.byte),
        }
    }
}

/// Toglie un file e dice quanto pesava, o `None` se non c'era o non si è
/// lasciato togliere.
fn togli_file(percorso: &Path) -> Option<u64> {
    let peso = std::fs::metadata(percorso).ok()?.len();
    std::fs::remove_file(percorso).ok()?;
    Some(peso)
}

/// L'impronta di un file dello store, se il nome ha la forma che lo store
/// scrive: `<impronta>.jpg`, `<impronta>.m.jpg` o `<impronta>.t.jpg`, con
/// l'impronta di blake3 in esadecimale minuscolo. `None` per tutto il resto,
/// che non si tocca.
fn impronta_del_file(percorso: &Path) -> Option<&str> {
    let nome = percorso.file_name()?.to_str()?;
    let hash = nome
        .strip_suffix(".t.jpg")
        .or_else(|| nome.strip_suffix(".m.jpg"))
        .or_else(|| nome.strip_suffix(".jpg"))?;
    e_di_blake3(hash).then_some(hash)
}

/// Il provvisorio con cui l'estrazione di un profilo scrive una copertina
/// prima di rinominarla (`profilo::archivio::Lettore::estrai`):
/// `<impronta>.parziale` o `<impronta>.t.parziale`. Un'estrazione fallita lo
/// toglie da sé; questo è per quella interrotta dalla morte del processo, che
/// lo lasciava nello store per sempre.
fn provvisorio_di_estrazione(percorso: &Path) -> bool {
    percorso
        .file_name()
        .and_then(|nome| nome.to_str())
        .and_then(|nome| {
            nome.strip_suffix(".t.parziale")
                .or_else(|| nome.strip_suffix(".parziale"))
        })
        .is_some_and(e_di_blake3)
}

/// Un'impronta come la scrive lo store: blake3, 64 cifre esadecimali minuscole.
fn e_di_blake3(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Le colonne che nominano un'impronta di `cover_art`, come `(tabella,
/// colonna)`.
///
/// # Chi lo dice
///
/// Lo **schema**, non un elenco scritto qui. Ogni colonna che punta a una
/// copertina è dichiarata `REFERENCES cover_art(hash)`, e SQLite sa elencarle:
/// `pragma_foreign_key_list` su ogni tabella. Così una tabella aggiunta da una
/// migrazione futura che dichiari la sua chiave esterna entra nel conto da
/// sola, senza che nessuno si ricordi di passare di qui — e dimenticarsene
/// vorrebbe dire cancellare copertine ancora in uso.
///
/// # E la rete sotto
///
/// Le quattro che ci sono dalla prima migrazione — `tracks.cover_art_hash`,
/// `albums.cover_art_hash`, `artists.image_hash`, `playlists.cover_art_hash` —
/// **devono** esserci. Se il catalogo ne restituisce di meno qualcosa non va
/// nello schema, o nella lettura del catalogo, e la pulizia si rifiuta invece
/// di cancellare sulla base di un elenco monco.
///
/// # Errori
///
/// `db.*` se il catalogo non si legge; `db.queryFailed` se manca una delle
/// quattro.
pub fn riferimenti(connection: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let fallita = |err: &rusqlite::Error| {
        crate::db::codice_da_sqlite("elenco delle colonne che nominano una copertina", err)
    };
    let mut statement = connection
        .prepare(
            "SELECT m.name, f.\"from\"
               FROM sqlite_master AS m, pragma_foreign_key_list(m.name) AS f
              WHERE m.type = 'table'
                AND lower(f.\"table\") = 'cover_art'
                AND (f.\"to\" IS NULL OR lower(f.\"to\") = 'hash')
              ORDER BY m.name, f.\"from\"",
        )
        .map_err(|err| fallita(&err))?;
    let colonne = statement
        .query_map([], |riga| {
            Ok((riga.get::<_, String>(0)?, riga.get::<_, String>(1)?))
        })
        .map_err(|err| fallita(&err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| fallita(&err))?;

    const ATTESE: [(&str, &str); 4] = [
        ("albums", "cover_art_hash"),
        ("artists", "image_hash"),
        ("playlists", "cover_art_hash"),
        ("tracks", "cover_art_hash"),
    ];
    for (tabella, colonna) in ATTESE {
        let trovata = colonne
            .iter()
            .any(|(t, c)| t.eq_ignore_ascii_case(tabella) && c.eq_ignore_ascii_case(colonna));
        if !trovata {
            return Err(AppError::new(ErrorCode::DbQueryFailed {
                detail: Some("elenco delle colonne che nominano una copertina".to_owned()),
            })
            .with_cause(format!(
                "manca {tabella}.{colonna} fra i riferimenti a cover_art: la pulizia \
                 delle copertine non procede su un elenco monco"
            )));
        }
    }
    Ok(colonne)
}

/// La condizione «nessuno la nomina», per le colonne date.
///
/// `NOT IN` su un'unione e non un `NOT EXISTS` per colonna: le colonne delle
/// copertine non hanno indici, e un `NOT EXISTS` correlato scorrerebbe tutti i
/// brani **per ogni** copertina — su cinquantamila brani e cinquemila
/// copertine, duecentocinquanta milioni di confronti. La sottoquery non
/// correlata SQLite la materializza una volta sola in un indice temporaneo.
///
/// Il `IS NOT NULL` in ogni ramo non è pignoleria: un solo `NULL` dentro un
/// `NOT IN` rende la condizione `NULL` per **ogni** riga, e la pulizia non
/// toglierebbe mai niente — in silenzio.
fn nessuno_la_nomina(colonne: &[(String, String)]) -> String {
    let virgolette = |nome: &str| format!("\"{}\"", nome.replace('"', "\"\""));
    let rami: Vec<String> = colonne
        .iter()
        .map(|(tabella, colonna)| {
            let colonna = virgolette(colonna);
            format!(
                "SELECT {colonna} FROM {} WHERE {colonna} IS NOT NULL",
                virgolette(tabella)
            )
        })
        .collect();
    format!("hash NOT IN ({})", rami.join(" UNION "))
}

/// Quante righe di `cover_art` non nomina più nessuno.
///
/// È la domanda che il filo di manutenzione si fa prima di decidere se una
/// pulizia vale la pena: costa la stessa unione di [`dimentica_orfane`], ma
/// non scrive niente.
///
/// # Errori
///
/// Quelli di [`riferimenti`], e `db.*` dalla lettura.
pub fn quante_orfane(connection: &Connection) -> Result<u64, AppError> {
    let colonne = riferimenti(connection)?;
    let quante: i64 = connection
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM cover_art WHERE {}",
                nessuno_la_nomina(&colonne)
            ),
            [],
            |riga| riga.get(0),
        )
        .map_err(|err| crate::db::codice_da_sqlite("conto delle copertine orfane", &err))?;
    Ok(u64::try_from(quante).unwrap_or(0))
}

/// Cancella le righe di `cover_art` che nessuno nomina più, e ne restituisce
/// le impronte.
///
/// **Non tocca i file**, e non per dimenticanza: si chiama col lucchetto della
/// libreria in mano, e cancellare migliaia di file dentro quel lucchetto
/// fermerebbe la finestra per tutto il tempo. Chi chiama passa le impronte a
/// [`CoverStore::togli_tutte`] dopo averlo lasciato. Se il processo muore fra
/// le due cose, i file restano senza riga, e li trova
/// [`CoverStore::spazza_avanzi`] alla pulizia successiva.
///
/// Un'istruzione sola, `DELETE … RETURNING`: la scelta di cosa togliere e la
/// cancellazione sono lo stesso passo, quindi nessuna scrittura può infilarsi
/// fra le due e far togliere una copertina appena tornata in uso.
///
/// # Errori
///
/// Quelli di [`riferimenti`], e `db.*` dalla cancellazione.
pub fn dimentica_orfane(connection: &Connection) -> Result<Vec<String>, AppError> {
    let colonne = riferimenti(connection)?;
    let fallita =
        |err: &rusqlite::Error| crate::db::codice_da_sqlite("pulizia delle copertine orfane", err);
    let mut statement = connection
        .prepare(&format!(
            "DELETE FROM cover_art WHERE {} RETURNING hash",
            nessuno_la_nomina(&colonne)
        ))
        .map_err(|err| fallita(&err))?;
    statement
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| fallita(&err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| fallita(&err))
}

/// Le impronte che hanno una riga in `cover_art`, per
/// [`CoverStore::spazza_avanzi`].
///
/// # Errori
///
/// `db.*` dalla lettura.
pub fn registrate(connection: &Connection) -> Result<HashSet<String>, AppError> {
    let fallita =
        |err: &rusqlite::Error| crate::db::codice_da_sqlite("elenco delle copertine", err);
    let mut statement = connection
        .prepare("SELECT hash FROM cover_art")
        .map_err(|err| fallita(&err))?;
    statement
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| fallita(&err))?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|err| fallita(&err))
}

/// Com'è andata una pulizia intera.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pulizia {
    /// Quante righe di `cover_art` se ne sono andate.
    pub righe: usize,
    /// Quanto si è liberato sul disco: copertine, miniature e avanzi.
    pub liberato: Liberato,
}

/// La pulizia intera in un colpo: righe orfane, i loro file, e gli avanzi.
///
/// È la composizione dei tre passi per chi non ha un lucchetto da lasciare fra
/// l'uno e l'altro — le prove, gli esempi. L'applicazione li chiama separati,
/// per cancellare i file **fuori** dal lucchetto della libreria.
///
/// # Errori
///
/// Quelli di [`dimentica_orfane`] e di [`registrate`]: se il database non
/// risponde non si cancella nessun file.
pub fn pulisci(
    connection: &Connection,
    store: &CoverStore,
    eta_avanzi: Duration,
) -> Result<Pulizia, AppError> {
    let tolte = dimentica_orfane(connection)?;
    let rimaste = registrate(connection)?;
    let liberato = store
        .togli_tutte(&tolte)
        .piu(store.spazza_avanzi(&rimaste, eta_avanzi));
    Ok(Pulizia {
        righe: tolte.len(),
        liberato,
    })
}

/// Un'immagine illeggibile non è un guasto della scansione: è un file con
/// dentro qualcosa che non è un'immagine, e va saltato riportando il perché.
fn decode_error(hash: &str, detail: &str) -> AppError {
    AppError::new(ErrorCode::MetadataTagReadFailed { path: None })
        .with_detail(format!("copertina {hash} non decodificabile"))
        .with_cause(detail.to_owned())
}

/// Le dimensioni che [`fit`] produrrebbe, senza decodificare niente.
///
/// Stessi conti di `image::DynamicImage::resize`: rapporto minimo fra i due
/// assi, arrotondamento, mai sotto il pixel. Serve al ramo «già presente» di
/// [`CoverStore::store`], che deve dichiarare le dimensioni del file già sul
/// disco potendo leggere solo quelle dell'originale.
fn dimensioni_ridotte(width: u32, height: u32, lato: u32) -> (u32, u32) {
    if (width <= lato && height <= lato) || width == 0 || height == 0 {
        return (width, height);
    }
    let rapporto = f64::from(lato) / f64::from(width.max(height));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "il prodotto è al più `lato`, finito e non negativo per costruzione"
    )]
    let ridotta = |dim: u32| ((f64::from(dim) * rapporto).round() as u32).max(1);
    (ridotta(width), ridotta(height))
}

/// Riduce l'immagine perché stia in un quadrato di `lato`, senza deformarla.
///
/// Non ingrandisce mai: una copertina da 300 pixel resta di 300. Ingrandirla
/// occuperebbe più spazio per mostrare gli stessi dettagli, più sfocati.
fn fit(source: &image::DynamicImage, lato: u32) -> image::DynamicImage {
    if source.width() <= lato && source.height() <= lato {
        return source.clone();
    }
    // Lanczos3 e non il filtro veloce: la differenza si vede proprio alle
    // dimensioni di una miniatura, dove il ridimensionamento è aggressivo.
    source.resize(lato, lato, FilterType::Lanczos3)
}

fn encode_jpeg(image: &image::DynamicImage, quality: u8, hash: &str) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::new();
    // In RGB8: il JPEG non ha canale alfa, e una PNG con trasparenza
    // convertita senza questo passaggio esce con i bordi anneriti.
    let rgb = image.to_rgb8();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(
            &rgb,
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|err| decode_error(hash, &err.to_string()))?;
    Ok(out)
}

#[cfg(test)]
// La divisione fra interi qui e deliberata: sono rapporti fra dimensioni in
// pixel e in byte, dove il resto non interessa. Il lint serve nel codice di
// produzione, dove una divisione troncata puo essere un calcolo sbagliato.
#[expect(
    clippy::integer_division,
    reason = "rapporti fra pixel e byte: il resto non interessa, come sopra"
)]
mod tests {
    use super::*;

    mod prove_estranee;

    /// Un PNG vero, generato al volo: provare la ricodifica richiede
    /// un'immagine vera, non byte a caso.
    ///
    /// Sfumatura continua e non `x % 256`: il ritorno a zero ogni 256 pixel
    /// crea bordi netti che, ridimensionati, diventano alias ad alta frequenza —
    /// cioè esattamente il contenuto che il JPEG comprime peggio. Un'immagine di
    /// prova così direbbe cose sul proprio disegno, non sul codice.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut buffer = image::RgbaImage::new(width, height);
        let (w, h) = (width.max(1), height.max(1));
        for (x, y, pixel) in buffer.enumerate_pixels_mut() {
            let r = u8::try_from(x * 255 / w).unwrap_or(255);
            let g = u8::try_from(y * 255 / h).unwrap_or(255);
            *pixel = image::Rgba([r, g, 128, 255]);
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("codifica di prova");
        out
    }

    #[test]
    fn ricodifica_e_rimpicciolisce() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let originale = png(2000, 2000);

        let salvata = store
            .store(&originale, CoverSource::Tag)
            .expect("salvataggio");

        assert_eq!(salvata.width, MAX_LATO);
        assert_eq!(salvata.height, MAX_LATO);
        assert!(!salvata.already_present);
        assert!(store.path_for(&salvata.hash).exists());
        assert!(store.thumbnail_path_for(&salvata.hash).exists());

        // Non si afferma qui che la ricodifica RIMPICCIOLISCA: su un'immagine
        // sintetica dipende da quanto bene il PNG comprimeva quel disegno, e
        // sarebbe un'affermazione sul disegno del test. Il risparmio si misura
        // sulla libreria vera, dove le copertine sono fotografie.
        // Quello che qui si può affermare: la miniatura pesa molto meno.
        let miniatura = std::fs::metadata(store.thumbnail_path_for(&salvata.hash))
            .expect("miniatura")
            .len();
        assert!(
            miniatura < salvata.byte_size / 2,
            "miniatura {miniatura} contro piena {}",
            salvata.byte_size
        );
    }

    #[test]
    fn la_stessa_immagine_si_salva_una_volta_sola() {
        // È ciò che rende veloce una riscansione: se l'impronta c'è già, non si
        // decodifica niente.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let originale = png(400, 400);

        let prima = store.store(&originale, CoverSource::Tag).expect("prima");
        let seconda = store.store(&originale, CoverSource::Tag).expect("seconda");

        assert_eq!(prima.hash, seconda.hash);
        assert!(!prima.already_present);
        assert!(seconda.already_present);
    }

    #[test]
    fn non_ingrandisce_le_copertine_piccole() {
        // Ingrandire occuperebbe piu' spazio per mostrare gli stessi dettagli,
        // piu' sfocati.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let salvata = store
            .store(&png(120, 120), CoverSource::Tag)
            .expect("salvataggio");
        assert_eq!((salvata.width, salvata.height), (120, 120));
    }

    #[test]
    fn le_proporzioni_non_cambiano() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let salvata = store
            .store(&png(2000, 1000), CoverSource::Tag)
            .expect("salvataggio");
        assert_eq!(salvata.width, MAX_LATO);
        assert_eq!(
            salvata.height,
            MAX_LATO / 2,
            "una copertina non va deformata"
        );
    }

    #[test]
    fn quel_che_non_e_un_immagine_da_un_errore_non_un_panico() {
        // Un tag con dentro spazzatura non deve fermare la scansione di
        // millequattrocento file.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let errore = store
            .store(b"non sono un'immagine", CoverSource::Tag)
            .expect_err("deve fallire");
        assert!(errore.to_string().contains("non decodificabile"));
    }

    #[test]
    fn i_file_si_distribuiscono_in_sottocartelle() {
        // Una cartella con decine di migliaia di voci rallenta ogni lettura.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let salvata = store
            .store(&png(50, 50), CoverSource::Tag)
            .expect("salvataggio");
        let percorso = store.path_for(&salvata.hash);
        let sottocartella = percorso
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert_eq!(sottocartella.len(), 2);
        assert!(salvata.hash.starts_with(&sottocartella));
    }

    #[test]
    fn una_scrittura_non_lascia_file_temporanei() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        store
            .store(&png(300, 300), CoverSource::Tag)
            .expect("salvataggio");
        // Si guarda il NOME del file, non il percorso: la cartella temporanea
        // del test si chiama già `.tmpXXXX`, e cercare «tmp» dentro il percorso
        // intero farebbe fallire questo test qualunque cosa faccia il codice.
        // È il primo modo in cui l'ha fatto, ed è il motivo per cui vale la pena
        // guardare PERCHÉ un test rosso è rosso.
        let temporanei = walk(dir.path())
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().contains("tmp"))
            })
            .count();
        assert_eq!(temporanei, 0);
    }

    fn walk(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out
    }

    /// Il CRC di un blocco PNG: quello di zlib, sul tipo e sui dati.
    fn crc_png(byte: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFF_u32;
        for &b in byte {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// Un PNG che **dichiara** `larghezza`×`altezza` e non porta i pixel.
    ///
    /// È la forma di una bomba di decompressione vista dalla parte che conta:
    /// l'intestazione. I pixel veri non servono — i limiti devono scattare
    /// prima di chiederli, ed è esattamente quel che si prova — e un blocco
    /// `IDAT` vuoto ma valido fa arrivare il lettore fino a dove i conti si
    /// fanno, invece di farlo inciampare su un file senza dati.
    fn png_finto(larghezza: u32, altezza: u32, profondita: u8, colore: u8) -> Vec<u8> {
        fn blocco(out: &mut Vec<u8>, tipo: &[u8; 4], dati: &[u8]) {
            out.extend_from_slice(&u32::try_from(dati.len()).expect("corto").to_be_bytes());
            let mut firmato = tipo.to_vec();
            firmato.extend_from_slice(dati);
            out.extend_from_slice(&firmato);
            out.extend_from_slice(&crc_png(&firmato).to_be_bytes());
        }
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&larghezza.to_be_bytes());
        ihdr.extend_from_slice(&altezza.to_be_bytes());
        ihdr.extend_from_slice(&[profondita, colore, 0, 0, 0]);
        blocco(&mut out, b"IHDR", &ihdr);
        // Un flusso zlib vuoto: intestazione, un blocco «stored» finale di
        // lunghezza zero, e l'adler32 del niente.
        blocco(
            &mut out,
            b"IDAT",
            &[
                0x78, 0x01, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01,
            ],
        );
        blocco(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn una_bomba_di_decompressione_non_arriva_a_chiedere_la_memoria() {
        // Tre intestazioni che i limiti di serie di `image` lascerebbero
        // passare fino all'allocazione, e che qui si fermano prima:
        //
        // - 8000×8000 RGBA a 16 bit: 512 milioni di byte, appena sotto i 512
        //   MiB di serie, molto sopra i nostri 192;
        // - 8200×8200 in scala di grigi: 67 megabyte soltanto, ma un lato oltre
        //   8192;
        // - 1×100 000: cento kilobyte e una striscia che il ridimensionamento
        //   tratterebbe come centomila righe.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        for (larghezza, altezza, profondita, colore) in
            [(8000, 8000, 16, 6), (8200, 8200, 8, 0), (1, 100_000, 8, 0)]
        {
            let bomba = png_finto(larghezza, altezza, profondita, colore);
            let errore = store
                .store(&bomba, CoverSource::Tag)
                .expect_err("oltre i limiti non si decodifica");
            assert!(
                errore.to_string().contains("oltre i limiti"),
                "{larghezza}×{altezza}: {errore}"
            );
            // E non resta niente nello store: nessuna copertina a metà.
            assert!(!store.path_for(&blake3::hash(&bomba).to_hex()).exists());
        }

        // La controprova, sull'unica delle tre che non chiede mezzo gigabyte
        // nemmeno per sbaglio: con i limiti di serie la stessa intestazione
        // passa il controllo e il guasto arriva **dopo**, dai dati che mancano
        // — cioè a memoria già chiesta. Se un giorno `image` stringesse i suoi
        // limiti di serie, questa riga lo direbbe, e i nostri si potrebbero
        // rileggere.
        let di_serie = ImageReader::new(std::io::Cursor::new(png_finto(8200, 8200, 8, 0)))
            .with_guessed_format()
            .expect("formato")
            .decode()
            .expect_err("i dati non ci sono");
        assert!(
            !matches!(di_serie, ImageError::Limits(_)),
            "coi limiti di serie non è un limite a fermarla: {di_serie}"
        );
    }

    #[test]
    fn una_copertina_grande_ma_vera_si_decodifica_ancora() {
        // I limiti non devono diventare un rifiuto delle copertine vere: una
        // 3000×3000, che è la «alta risoluzione» dei negozi, passa.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let salvata = store
            .store(&png(3000, 3000), CoverSource::Tag)
            .expect("una copertina vera");
        assert_eq!((salvata.width, salvata.height), (MAX_LATO, MAX_LATO));
    }

    /// Un database con dentro le impronte date in `cover_art`.
    fn con_le_copertine(hashes: &[&str]) -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        for hash in hashes {
            connection
                .execute(
                    "INSERT INTO cover_art (hash, mime_type, source, created_at)
                     VALUES (?1, 'image/jpeg', 'tag', 0)",
                    [hash],
                )
                .expect("riga della copertina");
        }
        connection
    }

    /// Salva `n` copertine diverse e ne restituisce le impronte.
    fn salva(store: &CoverStore, n: u32) -> Vec<String> {
        (0..n)
            .map(|i| {
                store
                    .store(&png(20 + i, 20), CoverSource::Tag)
                    .expect("salvataggio")
                    .hash
            })
            .collect()
    }

    #[test]
    fn la_pulizia_toglie_solo_quel_che_nessuno_nomina() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let h = salva(&store, 5);
        let riferite: Vec<&str> = h.iter().map(String::as_str).collect();
        let connection = con_le_copertine(&riferite);
        // Una per ognuna delle quattro colonne che nominano una copertina, e
        // l'ultima orfana. Più un brano **senza** copertina: il suo `NULL`
        // dentro un `NOT IN` scritto male renderebbe la pulizia muta.
        connection
            .execute_batch(&format!(
                "INSERT INTO tracks (path, track_key, title, artist, album, file_size,
                                     date_added, date_modified, cover_art_hash)
                 VALUES ('C:/m/a.mp3', 'a', 'A', 'B', 'C', 1, 0, 0, '{}'),
                        ('C:/m/b.mp3', 'b', 'A', 'B', 'C', 1, 0, 0, NULL);
                 INSERT INTO albums (album_key, title, artist, cover_art_hash)
                 VALUES ('k', 'C', 'B', '{}');
                 INSERT INTO artists (name, image_hash) VALUES ('B', '{}');
                 INSERT INTO playlists (playlist_key, name, created_at, updated_at,
                                        cover_art_hash)
                 VALUES ('p', 'P', 0, 0, '{}');",
                h[0], h[1], h[2], h[3]
            ))
            .expect("chi le nomina");

        assert_eq!(quante_orfane(&connection).expect("conto"), 1);
        let prima = store.occupazione();
        let esito = pulisci(&connection, &store, Duration::from_secs(3600)).expect("pulizia");

        assert_eq!(esito.righe, 1, "una sola riga orfana");
        assert_eq!(esito.liberato.file, 2, "la piena e la miniatura");
        assert_eq!(
            store.occupazione().byte,
            prima.byte - esito.liberato.byte,
            "i byte dichiarati sono quelli che il disco ha perso"
        );
        assert!(!store.path_for(&h[4]).exists());
        assert!(!store.thumbnail_path_for(&h[4]).exists());
        for viva in &h[..4] {
            assert!(store.path_for(viva).exists(), "{viva} è ancora in uso");
            assert!(store.thumbnail_path_for(viva).exists());
        }
        assert_eq!(quante_orfane(&connection).expect("conto"), 0);

        // E una seconda pulizia non ha niente da fare.
        let di_nuovo = pulisci(&connection, &store, Duration::from_secs(3600)).expect("ancora");
        assert_eq!(di_nuovo, Pulizia::default());
    }

    #[test]
    fn una_tabella_nuova_che_nomina_una_copertina_entra_nel_conto_da_sola() {
        // Il motivo per cui i riferimenti li dice lo schema: una migrazione di
        // domani che aggiunga una colonna `REFERENCES cover_art(hash)` non deve
        // ricordarsi di passare dalla pulizia, o la pulizia le toglierebbe le
        // copertine da sotto.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let h = salva(&store, 1);
        let connection = con_le_copertine(&[h[0].as_str()]);
        connection
            .execute_batch(&format!(
                "CREATE TABLE copertine_future (
                   id INTEGER PRIMARY KEY,
                   quale TEXT REFERENCES cover_art(hash) ON DELETE SET NULL
                 );
                 INSERT INTO copertine_future (quale) VALUES ('{}');",
                h[0]
            ))
            .expect("tabella nuova");

        let colonne = riferimenti(&connection).expect("riferimenti");
        assert!(
            colonne
                .iter()
                .any(|(t, c)| t == "copertine_future" && c == "quale"),
            "{colonne:?}"
        );
        let esito = pulisci(&connection, &store, Duration::from_secs(3600)).expect("pulizia");
        assert_eq!(
            esito.righe, 0,
            "la nomina qualcuno che l'elenco scritto a mano non conosceva"
        );
        assert!(store.path_for(&h[0]).exists());
    }

    #[test]
    fn senza_i_quattro_riferimenti_di_sempre_la_pulizia_si_rifiuta() {
        // Uno schema che non dice più chi nomina le copertine non è una buona
        // ragione per cancellarle tutte.
        let connection = Connection::open_in_memory().expect("database");
        connection
            .execute_batch(
                "CREATE TABLE cover_art (hash TEXT PRIMARY KEY);
                 CREATE TABLE tracks (id INTEGER PRIMARY KEY,
                   cover_art_hash TEXT REFERENCES cover_art(hash));
                 INSERT INTO cover_art VALUES ('x');",
            )
            .expect("schema monco");
        dimentica_orfane(&connection).expect_err("elenco monco");
        let rimaste: i64 = connection
            .query_row("SELECT COUNT(*) FROM cover_art", [], |r| r.get(0))
            .expect("conto");
        assert_eq!(rimaste, 1, "non si è cancellato niente");
    }

    #[test]
    fn gli_avanzi_se_ne_vanno_solo_da_vecchi_e_solo_se_sono_avanzi() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let h = salva(&store, 2);
        // La prima ha la sua riga, la seconda no: è quel che lascia una
        // scansione annullata dopo aver salvato l'immagine e prima di
        // registrarla.
        let registrate: HashSet<String> = std::iter::once(h[0].clone()).collect();
        let cartella = store
            .path_for(&h[0])
            .parent()
            .expect("sottocartella")
            .to_path_buf();
        // Un temporaneo di una scrittura interrotta, nella forma nuova e in
        // quella di prima, e un file che lo store non ha scritto.
        let temporaneo = cartella.join(format!("{}.jpg.1234-5.tmp", h[0]));
        let vecchio_stile = cartella.join(format!("{}.tmp1234", h[0]));
        let estraneo = cartella.join("leggimi.txt");
        for file in [&temporaneo, &vecchio_stile, &estraneo] {
            std::fs::write(file, b"x").expect("file");
        }

        // Giovani: non si tocca niente. Potrebbero essere la scrittura in
        // corso di un altro filo, o una copertina che sta per essere registrata.
        let niente = store.spazza_avanzi(&registrate, Duration::from_secs(3600));
        assert_eq!(niente, Liberato::default());

        // Vecchi: se ne vanno i due temporanei e i due file della seconda.
        let spazzati = store.spazza_avanzi(&registrate, Duration::ZERO);
        assert_eq!(spazzati.file, 4);
        assert!(!temporaneo.exists() && !vecchio_stile.exists());
        assert!(!store.path_for(&h[1]).exists());
        assert!(!store.thumbnail_path_for(&h[1]).exists());
        assert!(store.path_for(&h[0]).exists(), "quella registrata resta");
        assert!(estraneo.exists(), "quel che non si riconosce non si tocca");
    }

    #[test]
    fn il_provvisorio_di_un_estrazione_interrotta_si_spazza_da_vecchio() {
        // L'estrazione di un profilo scrive `<impronta>.parziale` (e
        // `<impronta>.t.parziale`) e lo rinomina alla fine: se il processo
        // muore a metà, resta. Prima nessuna pulizia lo riconosceva.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let hash = "ab".repeat(32);
        let cartella = dir.path().join("ab");
        std::fs::create_dir_all(&cartella).expect("cartella");
        let piena = cartella.join(format!("{hash}.parziale"));
        let miniatura = cartella.join(format!("{hash}.t.parziale"));
        let estraneo = cartella.join("non-un-impronta.parziale");
        for file in [&piena, &miniatura, &estraneo] {
            std::fs::write(file, b"a meta").expect("file");
        }

        let niente = store.spazza_avanzi(&HashSet::new(), Duration::from_secs(3600));
        assert_eq!(
            niente,
            Liberato::default(),
            "da giovane può essere in corso"
        );
        let spazzati = store.spazza_avanzi(&HashSet::new(), Duration::ZERO);
        assert_eq!(spazzati.file, 2);
        assert!(!piena.exists() && !miniatura.exists());
        assert!(estraneo.exists(), "quel che non si riconosce non si tocca");
    }

    #[test]
    fn l_occupazione_conta_piene_miniature_e_avanzi() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        assert_eq!(store.occupazione(), Occupazione::default());
        let h = salva(&store, 3);
        let attesi: u64 = h
            .iter()
            .flat_map(|hash| [store.path_for(hash), store.thumbnail_path_for(hash)])
            .map(|p| std::fs::metadata(p).expect("file").len())
            .sum();
        let occupazione = store.occupazione();
        assert_eq!(occupazione.file, 6);
        assert_eq!(occupazione.byte, attesi);
    }

    #[test]
    fn la_media_nasce_dalla_piena_alla_prima_richiesta() {
        // Una copertina salvata come le salva la scansione: piena e miniatura,
        // e la media no — come tutte quelle già nello store prima della 3.0.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let salvata = store
            .store(&png(2000, 1000), CoverSource::Tag)
            .expect("salvataggio");
        let media = store.percorso_media(&salvata.hash);
        assert!(
            !media.exists(),
            "la scansione non paga una terza ricodifica"
        );

        let byte = store.media(&salvata.hash).expect("media").expect("c'è");
        let immagine = image::load_from_memory(&byte).expect("un JPEG vero");
        assert_eq!(
            (immagine.width(), immagine.height()),
            (MAX_LATO_MEDIA, MAX_LATO_MEDIA / 2),
            "ridotta in proporzione dalla piena"
        );
        assert!(media.exists(), "e resta sul disco per la prossima volta");
        assert_eq!(
            media.file_name().and_then(|n| n.to_str()),
            Some(format!("{}.m.jpg", salvata.hash).as_str())
        );
        // La seconda volta si legge e basta: gli stessi byte.
        assert_eq!(store.media(&salvata.hash).expect("ancora"), Some(byte));
        // Le copertine di prima restano valide così come sono.
        assert!(store.path_for(&salvata.hash).exists());
        assert!(store.thumbnail_path_for(&salvata.hash).exists());
    }

    #[test]
    fn una_copertina_piccola_resta_piccola_anche_da_media() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let salvata = store
            .store(&png(200, 150), CoverSource::Tag)
            .expect("salvataggio");
        let byte = store.media(&salvata.hash).expect("media").expect("c'è");
        let immagine = image::load_from_memory(&byte).expect("un JPEG vero");
        assert_eq!((immagine.width(), immagine.height()), (200, 150));
    }

    #[test]
    fn senza_la_piena_la_media_non_c_e_e_non_e_un_guasto() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let assente = "ab".repeat(32);
        assert_eq!(store.media(&assente).expect("nessun guasto"), None);
        assert!(!store.percorso_media(&assente).exists());
    }

    #[test]
    fn una_miniatura_persa_si_rifa_alla_riscansione() {
        // Il ramo «già presente» guardava solo la piena: una miniatura persa
        // (un programma di pulizia, una cancellazione riuscita a metà) non si
        // rifaceva più, e la copertina restava un segnaposto negli elenchi.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let originale = png(300, 300);
        let prima = store.store(&originale, CoverSource::Tag).expect("prima");
        std::fs::remove_file(store.thumbnail_path_for(&prima.hash)).expect("via la miniatura");

        let seconda = store.store(&originale, CoverSource::Tag).expect("seconda");
        assert!(seconda.already_present);
        assert!(
            store.thumbnail_path_for(&seconda.hash).exists(),
            "la miniatura non è tornata"
        );
        assert!(store.tinta(&seconda.hash).is_some(), "e la tinta con lei");
    }

    #[cfg(windows)]
    #[test]
    fn una_piena_che_non_si_lascia_togliere_tiene_la_sua_miniatura() {
        // Aperta da un altro processo senza condivisione — un antivirus, un
        // indicizzatore — la piena resta: la miniatura deve restare con lei,
        // o «piena ⇒ miniatura» si rompe e la copertina che torna in uso non
        // ha più la sua miniatura.
        use std::os::windows::fs::OpenOptionsExt as _;
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let salvata = store
            .store(&png(300, 300), CoverSource::Tag)
            .expect("salvataggio");
        let tenuta = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(store.path_for(&salvata.hash))
            .expect("aperta senza condivisione");

        let tolti = store.togli_tutte(std::slice::from_ref(&salvata.hash));
        assert!(store.path_for(&salvata.hash).exists(), "la piena è rimasta");
        assert!(
            store.thumbnail_path_for(&salvata.hash).exists(),
            "la miniatura se n'è andata senza la piena"
        );
        assert_eq!(tolti.file, 0);
        drop(tenuta);
    }

    #[test]
    fn la_pulizia_conosce_anche_la_media() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let h = salva(&store, 3);
        for hash in &h {
            store.media(hash).expect("media").expect("c'è");
        }
        // Senza riga in `cover_art`, e vecchia: la media se ne va con le altre due.
        let registrate: HashSet<String> = [h[0].clone(), h[1].clone()].into_iter().collect();
        let spazzati = store.spazza_avanzi(&registrate, Duration::ZERO);
        assert_eq!(spazzati.file, 3, "piena, media e miniatura");
        assert!(!store.percorso_media(&h[2]).exists());
        // E chi la toglie per impronta toglie anche lei.
        let tolti = store.togli_tutte(&[h[0].clone()]);
        assert_eq!(tolti.file, 3);
        assert!(!store.percorso_media(&h[0]).exists());
        assert!(store.percorso_media(&h[1]).exists(), "quella in uso resta");
    }

    #[test]
    fn le_dimensioni_dichiarate_senza_decodificare_sono_quelle_vere() {
        // Il ramo «già presente» dichiara le dimensioni del file sul disco
        // potendo leggere solo l'originale: i suoi conti devono coincidere con
        // quelli di `fit`, o una copertina panoramica verrebbe registrata
        // quadrata — e `pick_album_cover` sceglie per area.
        for (w, h) in [(2000, 1000), (1000, 2000), (300, 300), (641, 640)] {
            let vera = fit(&image::DynamicImage::new_rgb8(w, h), MAX_LATO);
            assert_eq!(
                dimensioni_ridotte(w, h, MAX_LATO),
                (vera.width(), vera.height()),
                "su {w}×{h}"
            );
        }
    }

    // ── le impronte che non sono impronte ──

    /// Le forme che un'impronta scritta da fuori può avere per uscire dallo
    /// store: risalite, assoluti, prefissi d'unità, percorsi di rete, e il
    /// gemello maiuscolo di un'impronta vera.
    fn impronte_storte() -> Vec<String> {
        vec![
            "../../fuori".to_owned(),
            "..\\..\\fuori".to_owned(),
            "ab/../../fuori".to_owned(),
            "ab\\..\\..\\fuori".to_owned(),
            "C:\\Users\\qualcuno\\Pictures\\IMG_0001".to_owned(),
            "C:fuori".to_owned(),
            "/etc/fuori".to_owned(),
            "\\\\host\\condivisa\\fuori".to_owned(),
            "//host/condivisa/fuori".to_owned(),
            "AB".repeat(32),
            String::new(),
            "a".to_owned(),
        ]
    }

    #[test]
    fn un_impronta_che_non_e_esadecimale_non_esce_dallo_store() {
        // Solo calcoli sui percorsi: nessuna di queste forme tocca il disco, e
        // le due di rete non aprono niente verso nessun host.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path().join("copertine")).expect("store");
        for storta in impronte_storte() {
            for percorso in [
                store.path_for(&storta),
                store.thumbnail_path_for(&storta),
                store.percorso_media(&storta),
            ] {
                // `starts_with` da solo non basta: `radice/../..` comincia per
                // la radice, componente per componente.
                let dentro = percorso.starts_with(store.radice())
                    && !percorso
                        .components()
                        .any(|parte| matches!(parte, std::path::Component::ParentDir));
                assert!(
                    dentro,
                    "«{storta}» diventa {} — fuori dallo store",
                    percorso.display()
                );
            }
        }
    }

    #[test]
    fn togliere_un_impronta_storta_non_cancella_niente_fuori_dallo_store() {
        // Le foto dell'utente accanto alla cartella dei dati, e due righe
        // orfane che le nominano: una per assoluto, una risalendo. È quel che un
        // profilo costruito a mano lasciava in `cover_art` per la pulizia.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path().join("dati").join("copertine")).expect("store");
        let foto = dir.path().join("foto");
        std::fs::create_dir_all(&foto).expect("cartella delle foto");
        let mut vittime = Vec::new();
        for nome in ["IMG_0001", "IMG_0002"] {
            for coda in ["jpg", "m.jpg", "t.jpg"] {
                let file = foto.join(format!("{nome}.{coda}"));
                std::fs::write(&file, b"una foto").expect("foto");
                vittime.push(file);
            }
        }
        let assoluta = foto.join("IMG_0001").to_string_lossy().into_owned();
        // `..` come prefisso di due caratteri, poi un altro `..` nel nome: la
        // radice dello store sta due livelli sotto la cartella temporanea.
        let risalita = "../foto/IMG_0002".to_owned();

        let tolti = store.togli_tutte(&[assoluta, risalita]);

        for vittima in &vittime {
            assert!(vittima.exists(), "{} è stata cancellata", vittima.display());
        }
        assert_eq!(tolti, Liberato::default());
    }

    #[test]
    fn un_orfana_in_maiuscolo_non_porta_via_la_copertina_vera() {
        // Su NTFS `AB/ABCD….jpg` e `ab/abcd….jpg` sono lo stesso file, ma per
        // SQLite sono due chiavi: un gemello maiuscolo orfano in `cover_art`
        // toglieva la copertina di chi la usa ancora.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let vera = salva(&store, 1).remove(0);
        store.media(&vera).expect("media").expect("c'è");

        let tolti = store.togli_tutte(&[vera.to_uppercase()]);

        assert!(store.path_for(&vera).exists(), "la piena è sparita");
        assert!(store.thumbnail_path_for(&vera).exists());
        assert!(store.percorso_media(&vera).exists());
        assert_eq!(tolti, Liberato::default());
    }

    #[test]
    fn la_pulizia_di_una_riga_storta_toglie_la_riga_e_non_tocca_il_disco() {
        // Dal database in giù, come la fa il filo di manutenzione: la riga se
        // ne va — non la nomina nessuno — e il file fuori dallo store resta.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path().join("dati").join("copertine")).expect("store");
        let foto = dir.path().join("foto");
        std::fs::create_dir_all(&foto).expect("cartella delle foto");
        let vittima = foto.join("IMG_0003.jpg");
        std::fs::write(&vittima, b"una foto").expect("foto");
        let storta = foto.join("IMG_0003").to_string_lossy().into_owned();
        let connection = con_le_copertine(&[storta.as_str()]);

        let esito = pulisci(&connection, &store, Duration::ZERO).expect("pulizia");

        assert!(vittima.exists(), "la foto è stata cancellata");
        assert_eq!(esito.righe, 1, "la riga storta se ne va");
        assert_eq!(esito.liberato, Liberato::default());
    }

    #[test]
    fn un_impronta_storta_non_scrive_e_non_legge() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        // Senza le due forme di rete: questa prova chiede davvero al disco, e
        // prima della correzione le avrebbe chieste a un host.
        let locali = impronte_storte()
            .into_iter()
            .filter(|storta| !storta.starts_with("\\\\") && !storta.starts_with("//"));
        for storta in locali {
            assert_eq!(store.media(&storta).expect("nessun guasto"), None);
            assert_eq!(store.tinta(&storta), None);
        }
        assert_eq!(
            store.occupazione(),
            Occupazione::default(),
            "niente scritto"
        );
    }

    #[test]
    fn un_impronta_vera_resta_dove_stava() {
        // Il criterio nuovo non sposta niente: le copertine di prima stanno
        // dove le cerca lo store di adesso.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let hash = "ab".repeat(32);
        assert_eq!(
            store.path_for(&hash),
            dir.path().join("ab").join(format!("{hash}.jpg"))
        );
        assert_eq!(
            store.thumbnail_path_for(&hash),
            dir.path().join("ab").join(format!("{hash}.t.jpg"))
        );
        assert_eq!(
            store.percorso_media(&hash),
            dir.path().join("ab").join(format!("{hash}.m.jpg"))
        );
        assert!(e_un_impronta(&hash));
        assert!(e_un_impronta("abcdef0123"), "le impronte corte delle prove");
        for storta in impronte_storte() {
            assert!(!e_un_impronta(&storta), "«{storta}»");
        }
    }

    // ── le copertine arrivate da un altro PC ──

    /// Il codice di un rifiuto, e il rifiuto stesso per leggerne il testo.
    fn rifiuto(byte: &[u8]) -> AppError {
        let errore = ricodifica_estranea(byte).expect_err("va rifiutata");
        assert_eq!(
            errore.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::MetadataTagReadFailed,
            "lo stesso codice di ogni copertina illeggibile: {errore}"
        );
        errore
    }

    #[test]
    fn una_jpeg_estranea_torna_jpeg_nostra_e_ridotta() {
        let mut jpeg = Vec::new();
        // Sotto il lato massimo di una copertina estranea (1024): una più grande
        // non si decodifica nemmeno, ed è un'altra prova.
        image::load_from_memory(&png(1000, 500))
            .expect("png di prova")
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .expect("jpeg di prova");
        assert!(
            jpeg.len() <= MASSIMO_ESTRANEA,
            "la prova sta sotto il tetto"
        );

        let ricodificata = ricodifica_estranea(&jpeg).expect("ricodificata");
        assert_eq!(
            image::guess_format(&ricodificata).expect("formato"),
            image::ImageFormat::Jpeg
        );
        let letta = image::load_from_memory(&ricodificata).expect("si rilegge");
        assert_eq!(
            (letta.width(), letta.height()),
            (MAX_LATO, MAX_LATO / 2),
            "ridotta alla piena, senza deformarla"
        );

        // Anche una PNG piccola esce JPEG: il formato che arriva alla finestra
        // è sempre il nostro, non quello di chi l'ha mandata.
        let piccola = ricodifica_estranea(&png(100, 100)).expect("ricodificata");
        assert_eq!(
            image::guess_format(&piccola).expect("formato"),
            image::ImageFormat::Jpeg
        );
    }

    #[test]
    fn una_bomba_estranea_si_ferma_ai_limiti() {
        // Le stesse tre intestazioni della prova di `store`: i limiti sono gli
        // stessi, e devono scattare prima della memoria anche qui.
        for (larghezza, altezza, profondita, colore) in
            [(8000, 8000, 16, 6), (8200, 8200, 8, 0), (1, 100_000, 8, 0)]
        {
            let bomba = png_finto(larghezza, altezza, profondita, colore);
            let errore = rifiuto(&bomba);
            assert!(
                errore.to_string().contains("oltre i limiti"),
                "{larghezza}×{altezza}: {errore}"
            );
        }
    }

    #[test]
    fn byte_a_caso_non_sono_una_copertina() {
        let mut stato = 0x2545_F491_u32;
        let caso: Vec<u8> = (0..4096)
            .map(|_| {
                stato ^= stato << 13;
                stato ^= stato >> 17;
                stato ^= stato << 5;
                stato.to_le_bytes()[0]
            })
            .collect();
        rifiuto(&caso);
        rifiuto(&[]);
        rifiuto(b"GIF89a e poi niente");
    }

    #[test]
    fn oltre_un_mebibyte_si_rifiuta_prima_di_guardare() {
        // Un'immagine **vera** appena sopra il tetto, così il rifiuto è per il
        // peso e non per i byte: la stessa immagine passa dalla decodifica.
        let mut grande = png(64, 64);
        grande.resize(MASSIMO_ESTRANEA + 1, 0);
        assert!(
            decodifica(&grande, "prova").is_ok(),
            "i byte in coda a una PNG non ne impediscono la lettura"
        );
        let errore = rifiuto(&grande);
        assert!(
            errore.cause().is_some_and(|causa| causa.contains("tetto")),
            "{errore:?}"
        );
        // Al tetto esatto invece passa.
        let mut al_tetto = png(64, 64);
        al_tetto.resize(MASSIMO_ESTRANEA, 0);
        assert!(ricodifica_estranea(&al_tetto).is_ok());
    }

    // ── le copertine che arrivano da un altro dispositivo (libreria unica) ──

    /// Una JPEG come la manda un altro dispositivo: la «media» da 320 pixel.
    fn media_di_un_altro(lato: u32) -> Vec<u8> {
        let immagine =
            image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(lato, lato, |x, y| {
                image::Rgb([
                    u8::try_from(x % 251).unwrap_or(0),
                    u8::try_from(y % 251).unwrap_or(0),
                    77,
                ])
            }));
        encode_jpeg(&immagine, QUALITA_MEDIA, "prova").expect("jpeg")
    }

    #[test]
    fn una_copertina_estranea_sta_nello_store_sotto_l_impronta_data() {
        // `Domanda::Copertina` serve la media ricodificata, non l'originale:
        // l'impronta dei suoi byte non è quella dell'originale, che è quella
        // che la libreria di chi la manda e di chi la riceve usa come nome.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let impronta = "ab".repeat(32);
        let byte = media_di_un_altro(320);
        assert_ne!(blake3::hash(&byte).to_hex().to_string(), impronta);
        let salvata = store.salva_estranea(&impronta, &byte).expect("salvataggio");
        assert_eq!(salvata.hash, impronta);
        assert!(!salvata.already_present);
        assert!(
            store.path_for(&impronta).exists(),
            "la piena, dove la cerca il provider"
        );
        assert!(store.thumbnail_path_for(&impronta).exists());
        assert!(store.media(&impronta).expect("media").is_some());
        // Una seconda volta non rifà niente.
        assert!(
            store
                .salva_estranea(&impronta, &byte)
                .expect("di nuovo")
                .already_present
        );
    }

    #[test]
    fn una_copertina_estranea_non_prende_il_posto_di_una_che_c_e_gia() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let locale = store
            .store(&png(900, 900), CoverSource::Tag)
            .expect("locale");
        let prima = std::fs::read(store.path_for(&locale.hash)).expect("file");
        let salvata = store
            .salva_estranea(&locale.hash, &media_di_un_altro(320))
            .expect("estranea");
        assert!(salvata.already_present);
        assert_eq!(
            std::fs::read(store.path_for(&locale.hash)).expect("file"),
            prima
        );
    }

    #[test]
    fn una_copertina_estranea_non_esce_dallo_store_ne_e_un_estranea_qualunque() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        for storta in [
            "",
            "x",
            "../../fuori",
            "AB".repeat(32).as_str(),
            "a/b",
            r"ab\cd",
        ] {
            assert!(
                store
                    .salva_estranea(storta, &media_di_un_altro(64))
                    .is_err(),
                "{storta:?}"
            );
        }
        let impronta = "cd".repeat(32);
        assert!(
            store
                .salva_estranea(&impronta, b"non sono un'immagine")
                .is_err()
        );
        assert!(
            store
                .salva_estranea(&impronta, &vec![0_u8; MASSIMO_ESTRANEA + 1])
                .is_err(),
            "oltre il tetto non si guarda nemmeno"
        );
        assert!(
            !store.path_for(&impronta).exists(),
            "un errore non lascia niente"
        );
    }

    #[test]
    fn la_copertina_di_un_altro_dispositivo_cede_il_posto_all_originale_di_una_scansione() {
        // Guasto da evitare (advisor, punto 4): la media da 320 pixel preso da
        // fuori sta nello store come «piena»; una scansione dello stesso
        // originale, vedendo la piena, non farebbe niente, e la copertina
        // resterebbe a 320 pixel per sempre.
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let originale = png(900, 900);
        let impronta = blake3::hash(&originale).to_hex().to_string();
        store
            .salva_estranea(&impronta, &media_di_un_altro(320))
            .expect("da fuori");
        let (l, _) = imagesize::size(store.path_for(&impronta))
            .map(|s| (s.width, s.height))
            .expect("misure");
        assert_eq!(l, 320, "quella di prima");
        // La scansione dello stesso originale.
        let salvata = store
            .store(&originale, CoverSource::Tag)
            .expect("scansione");
        assert_eq!(salvata.hash, impronta);
        assert!(
            !salvata.already_present,
            "non si fida di quella presa da fuori"
        );
        assert_eq!((salvata.width, salvata.height), (MAX_LATO, MAX_LATO));
        let (l, _) = imagesize::size(store.path_for(&impronta))
            .map(|s| (s.width, s.height))
            .expect("misure");
        assert_eq!(l, 640, "la piena vera");
        // E la volta dopo è presente, e rapida.
        assert!(
            store
                .store(&originale, CoverSource::Tag)
                .expect("ancora")
                .already_present
        );
        // Nessun segno rimane: una copertina tolta se ne va tutta.
        let _ = store.togli_tutte(std::slice::from_ref(&impronta));
        assert!(!store.path_for(&impronta).exists());
        let vuota = std::fs::read_dir(dir.path().join(&impronta[..2]))
            .map(|mut d| d.next().is_none())
            .unwrap_or(true);
        assert!(vuota, "neanche il segno del file preso da fuori");
    }

    #[test]
    fn togliere_una_copertina_estranea_toglie_anche_il_suo_segno() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        let impronta = "ef".repeat(32);
        store
            .salva_estranea(&impronta, &media_di_un_altro(100))
            .expect("da fuori");
        let _ = store.togli_tutte(std::slice::from_ref(&impronta));
        let resta: Vec<_> = std::fs::read_dir(dir.path().join(&impronta[..2]))
            .map(|d| d.filter_map(Result::ok).collect())
            .unwrap_or_default();
        assert!(resta.is_empty(), "{resta:?}");
    }
}
