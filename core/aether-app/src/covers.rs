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

use std::path::{Path, PathBuf};

use aether_domain::errors::{AppError, ErrorCode};
use image::ImageReader;
use image::imageops::FilterType;

use crate::files::io_error;

/// Il lato massimo della copertina salvata.
///
/// 640 copre uno schermo ad alta densità che ne mostri una a tutta larghezza in
/// una colonna; oltre, si pagherebbero byte per pixel che nessuno guarda.
pub const MAX_LATO: u32 = 640;

/// Il lato massimo della miniatura, quella delle liste e delle griglie.
pub const MAX_LATO_MINIATURA: u32 = 160;

/// Qualità JPEG della copertina piena.
const QUALITA: u8 = 85;
/// Qualità della miniatura: più bassa, perché a 160 pixel non si distingue.
const QUALITA_MINIATURA: u8 = 78;

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
}

impl CoverStore {
    /// Apre (o crea) lo store nella cartella data.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, AppError> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|err| io_error(&root.display().to_string(), &err))?;
        Ok(Self { root })
    }

    /// Il percorso della copertina piena.
    ///
    /// I file sono divisi in sottocartelle per i primi due caratteri
    /// dell'impronta. Non è eleganza: una cartella con decine di migliaia di
    /// voci rallenta ogni `readdir`, e su qualche filesystem molto più che
    /// linearmente.
    #[must_use]
    pub fn path_for(&self, hash: &str) -> PathBuf {
        let prefix = hash.get(..2).unwrap_or("__");
        self.root.join(prefix).join(format!("{hash}.jpg"))
    }

    /// Il percorso della miniatura.
    #[must_use]
    pub fn thumbnail_path_for(&self, hash: &str) -> PathBuf {
        let prefix = hash.get(..2).unwrap_or("__");
        self.root.join(prefix).join(format!("{hash}.t.jpg"))
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
        let bytes = std::fs::read(self.thumbnail_path_for(hash)).ok()?;
        let immagine = ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?;
        crate::tinta::dominante(&immagine.to_rgb8())
    }

    /// Salva una copertina, ricodificandola. Se c'è già, non fa niente.
    pub fn store(&self, original: &[u8], source: CoverSource) -> Result<StoredCover, AppError> {
        let _ = source; // la provenienza la registra il database, non lo store
        let hash = blake3::hash(original).to_hex().to_string();
        let full = self.path_for(&hash);

        if let Ok(meta) = std::fs::metadata(&full) {
            // Già presente: si legge la dimensione senza decodificare. È il
            // caso normale di una riscansione, e deve costare quanto uno `stat`.
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

        let decoded = ImageReader::new(std::io::Cursor::new(original))
            .with_guessed_format()
            .map_err(|err| decode_error(&hash, &err.to_string()))?
            .decode()
            .map_err(|err| decode_error(&hash, &err.to_string()))?;

        let piena = fit(&decoded, MAX_LATO);
        let miniatura = fit(&decoded, MAX_LATO_MINIATURA);
        let width = piena.width();
        let height = piena.height();

        let bytes_piena = encode_jpeg(&piena, QUALITA, &hash)?;
        let bytes_miniatura = encode_jpeg(&miniatura, QUALITA_MINIATURA, &hash)?;

        let byte_size = bytes_piena.len() as u64;
        write_atomic(&full, &bytes_piena)?;
        write_atomic(&self.thumbnail_path_for(&hash), &bytes_miniatura)?;

        Ok(StoredCover {
            hash,
            width,
            height,
            byte_size,
            mime_type: "image/jpeg",
            already_present: false,
        })
    }
}

/// Un'immagine illeggibile non è un guasto della scansione: è un file con
/// dentro qualcosa che non è un'immagine, e va saltato riportando il perché.
fn decode_error(hash: &str, detail: &str) -> AppError {
    AppError::new(ErrorCode::MetadataTagReadFailed { path: None })
        .with_message(format!("copertina {hash} non decodificabile"))
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

/// Scrive prima accanto e poi rinomina.
///
/// Una scrittura interrotta a metà lascerebbe nello store un file parziale col
/// nome giusto — e siccome lo store si fida del nome, quel file verrebbe servito
/// per sempre come se fosse valido. Il rinomina è atomico: o c'è tutto o non
/// c'è niente.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| io_error(&parent.display().to_string(), &err))?;
    }
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temp, bytes).map_err(|err| io_error(&temp.display().to_string(), &err))?;
    std::fs::rename(&temp, path).map_err(|err| {
        let _ = std::fs::remove_file(&temp);
        io_error(&path.display().to_string(), &err)
    })
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
}
