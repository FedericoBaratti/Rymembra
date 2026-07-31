//! Il pacchetto `.aeskin`: leggerlo e scriverlo.
//!
//! Un pacchetto arriva da fuori — da un file scelto dall'utente, o dalla rete
//! locale mandato da un telefono — quindi questo è il confine dove si assume che
//! chi ha costruito l'archivio possa averlo fatto in malafede. Le difese non
//! sono ipotetiche: un archivio è il vettore classico per tre attacchi concreti,
//! e ognuno ha qui una guardia dedicata.
//!
//! **Path traversal.** Una voce chiamata `../../.ssh/authorized_keys` scritta da
//! un estrattore ingenuo finisce fuori dalla cartella di destinazione. La difesa
//! non è normalizzare il percorso e sperare: è una lista chiusa di nomi
//! ammessi — `skin.json`, `preview.png`, `assets/<nome>.<estensione>` — quindi
//! non esiste un percorso da normalizzare.
//!
//! **Zip bomb.** Un archivio da 40 KB può espandersi in gigabyte e far cadere il
//! processo per esaurimento di memoria — che sul telefono significa app uccisa
//! da Android. Le guardie sono quattro e agiscono **prima** di decomprimere:
//! numero di voci, dimensione dichiarata per voce, rapporto di compressione, e
//! somma dichiarata. Le dimensioni si leggono dall'indice dell'archivio, che è
//! l'unico punto in cui si sanno senza espandere niente.
//!
//! **Tipo mentito.** Un file chiamato `.png` che contiene un eseguibile, o un
//! SVG — che è un documento, e può contenere script. Il tipo si determina dai
//! byte iniziali, non dall'estensione, e l'SVG non è nella lista.
//!
//! Nota su cosa **non** serve difendere: nessuna risorsa finisce in un `url()`,
//! perché il compilatore non emette `url()` in nessun caso. Le risorse diventano
//! sorgenti interne, quindi una skin non può fare una richiesta di rete nemmeno
//! con una risorsa costruita ad arte.

use std::io::{Cursor, Read as _, Write as _};

use aether_domain::errors::{AppError, ErrorCode};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::document::{SkinDocument, parse_skin_json};

/// Il nome del manifest. È l'unica voce obbligatoria.
pub const MANIFEST_NAME: &str = "skin.json";
/// Il nome della miniatura.
pub const PREVIEW_NAME: &str = "preview.png";
const PREFISSO_RISORSE: &str = "assets/";

/// I limiti.
///
/// Scelti sul contenuto reale: una skin è testo più qualche immagine e un paio
/// di caratteri woff2. Un pacchetto da 20 MB non è una skin, è qualcos'altro.
pub mod limits {
    /// Archivio compresso.
    pub const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024;
    /// Somma di tutto ciò che si decomprime.
    pub const MAX_TOTAL_BYTES: u64 = 60 * 1024 * 1024;
    /// Una singola voce.
    pub const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
    /// Il manifest: è testo, e un manifest da mezzo mega è già assurdo.
    pub const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
    /// Quante voci.
    pub const MAX_ENTRIES: usize = 64;
    /// Rapporto massimo fra decompresso e compresso, per voce.
    ///
    /// Il testo comprime bene — un JSON può arrivare a 15× — quindi la soglia
    /// non può essere bassa. 200× non ostacola nessun contenuto legittimo e
    /// taglia le bombe, che stanno negli ordini di 1.000× e oltre.
    pub const MAX_COMPRESSION_RATIO: f64 = 200.0;
}

/// Un'estensione ammessa per una risorsa, col suo tipo e la sua firma.
#[derive(Debug, PartialEq, Eq)]
struct TipoRisorsa {
    estensione: &'static str,
    mime: &'static str,
    firma: &'static [u8],
}

/// Le risorse ammesse.
///
/// L'SVG non c'è, e non è una dimenticanza: è un documento, non un'immagine, e
/// può contenere script. I caratteri di sistema restano fuori discussione — un
/// carattere in un pacchetto si carica con un nome riservato alla skin.
static TIPI: &[TipoRisorsa] = &[
    TipoRisorsa {
        estensione: "png",
        mime: "image/png",
        firma: &[0x89, 0x50, 0x4e, 0x47],
    },
    TipoRisorsa {
        estensione: "jpg",
        mime: "image/jpeg",
        firma: &[0xff, 0xd8, 0xff],
    },
    TipoRisorsa {
        estensione: "webp",
        mime: "image/webp",
        firma: &[0x52, 0x49, 0x46, 0x46],
    },
    TipoRisorsa {
        estensione: "woff2",
        mime: "font/woff2",
        firma: &[0x77, 0x4f, 0x46, 0x32],
    },
];

fn tipo(estensione: &str) -> Option<&'static TipoRisorsa> {
    TIPI.iter().find(|t| t.estensione == estensione)
}

/// Che cos'è una voce dell'archivio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Voce {
    Manifest,
    Miniatura,
    Risorsa(&'static TipoRisorsa),
}

/// Il nome è ammesso? Lista chiusa: non c'è nulla da normalizzare.
fn classifica(name: &str) -> Option<Voce> {
    if name == MANIFEST_NAME {
        return Some(Voce::Manifest);
    }
    if name == PREVIEW_NAME {
        return Some(Voce::Miniatura);
    }

    let foglia = name.strip_prefix(PREFISSO_RISORSE)?;
    // Un solo livello: nessuna sottocartella, quindi nessun percorso da
    // risolvere. E un nome di file vincolato, che esclude i punti doppi per
    // costruzione invece che per controllo.
    let (radice, estensione) = foglia.rsplit_once('.')?;
    let mut caratteri = radice.bytes();
    let ammesso = caratteri
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && caratteri
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        && estensione
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if !ammesso {
        return None;
    }

    tipo(estensione).map(Voce::Risorsa)
}

fn comincia_con(bytes: &[u8], firma: &[u8]) -> bool {
    bytes.len() >= firma.len() && bytes.iter().zip(firma).all(|(a, b)| a == b)
}

fn rifiutata(asset: &str, reason: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::SkinAssetRejected {
        asset: asset.to_owned(),
        reason: reason.into(),
    })
}

fn corrotto(detail: &str) -> AppError {
    AppError::new(ErrorCode::SkinPackageCorrupt {
        detail: Some(detail.to_owned()),
    })
}

/// Una risorsa del pacchetto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinAsset {
    /// Il nome dentro `assets/`, senza prefisso.
    pub name: String,
    /// Il tipo, determinato dai byte.
    pub mime: &'static str,
    /// Il contenuto.
    pub bytes: Vec<u8>,
}

/// Un pacchetto letto.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinPackage {
    /// Il documento validato, nella forma interna: colori scomposti in canali,
    /// lunghezze come numero più unità. È ciò che il compilatore consuma.
    pub document: SkinDocument,
    /// Il manifest come è stato scritto.
    ///
    /// Serve, e la ragione è strutturale: la forma interna **non è
    /// riserializzabile come sorgente**. Un colore validato è una quaterna di
    /// canali, e riscriverlo nel pacchetto produrrebbe un manifest che la
    /// lettura rifiuta — un pacchetto illeggibile dallo stesso codice che l'ha
    /// prodotto. Ed è anche la forma che un editor deve modificare, perché è
    /// quella che l'autore ha davanti.
    pub source: String,
    /// La miniatura per il selettore.
    pub preview: Option<Vec<u8>>,
    /// Le risorse, in ordine di nome.
    pub assets: Vec<SkinAsset>,
}

/// Cosa si è deciso di una voce, prima di decomprimerla.
struct Ammessa {
    indice: usize,
    nome: String,
    voce: Voce,
}

fn preflight(archivio: &mut ZipArchive<Cursor<&[u8]>>) -> Result<Vec<Ammessa>, AppError> {
    if archivio.len() > limits::MAX_ENTRIES {
        return Err(rifiutata(
            "(archivio)",
            format!(
                "il pacchetto ha {} voci, oltre il limite di {}",
                archivio.len(),
                limits::MAX_ENTRIES
            ),
        ));
    }

    let mut ammesse = Vec::new();
    let mut totale: u64 = 0;

    for indice in 0..archivio.len() {
        // `by_index_raw` legge la voce dall'indice dell'archivio senza preparare
        // alcun decompressore: è il punto in cui una bomba si ferma, perché dopo
        // la memoria è già stata chiesta.
        let voce_zip = archivio
            .by_index_raw(indice)
            .map_err(|err| corrotto(&format!("voce {indice} illeggibile: {err}")))?;
        let nome = voce_zip.name().to_owned();
        let dichiarata = voce_zip.size();
        let compressa = voce_zip.compressed_size();
        drop(voce_zip);

        if nome.ends_with('/') {
            // Le cartelle non servono: la struttura è fissa e a un solo livello.
            continue;
        }

        // Copre anche il path traversal: `../x` non è un nome ammesso, quindi
        // non arriva mai a essere un percorso.
        let Some(voce) = classifica(&nome) else {
            return Err(rifiutata(&nome, "nome non ammesso dal formato"));
        };

        let limite = match voce {
            Voce::Manifest => limits::MAX_MANIFEST_BYTES,
            Voce::Miniatura | Voce::Risorsa(_) => limits::MAX_ENTRY_BYTES,
        };
        if dichiarata > limite {
            return Err(rifiutata(
                &nome,
                format!("voce di {dichiarata} byte, oltre il limite di {limite}"),
            ));
        }

        if compressa > 0 {
            #[allow(clippy::cast_precision_loss)]
            let rapporto = dichiarata as f64 / compressa as f64;
            if rapporto > limits::MAX_COMPRESSION_RATIO {
                return Err(rifiutata(
                    &nome,
                    format!("rapporto di compressione {rapporto:.0}×, sospetto"),
                ));
            }
        }

        totale = totale.saturating_add(dichiarata);
        if totale > limits::MAX_TOTAL_BYTES {
            return Err(rifiutata(
                &nome,
                "il contenuto decompresso supera il limite totale",
            ));
        }

        ammesse.push(Ammessa { indice, nome, voce });
    }

    Ok(ammesse)
}

fn leggi_voce(
    archivio: &mut ZipArchive<Cursor<&[u8]>>,
    ammessa: &Ammessa,
) -> Result<Vec<u8>, AppError> {
    let mut voce = archivio
        .by_index(ammessa.indice)
        .map_err(|err| corrotto(&format!("«{}» illeggibile: {err}", ammessa.nome)))?;
    let limite = match ammessa.voce {
        Voce::Manifest => limits::MAX_MANIFEST_BYTES,
        Voce::Miniatura | Voce::Risorsa(_) => limits::MAX_ENTRY_BYTES,
    };
    // Il tetto vale anche qui, e non è una ripetizione inutile: la dimensione
    // nell'indice è dichiarata da chi ha costruito l'archivio, quindi può
    // mentire. Questa invece è la quantità di byte che si accetta davvero.
    let mut bytes = Vec::new();
    voce.by_ref()
        .take(limite.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| corrotto(&format!("«{}» illeggibile: {err}", ammessa.nome)))?;
    if bytes.len() as u64 > limite {
        return Err(rifiutata(
            &ammessa.nome,
            "la voce è più grande di quanto l'indice dichiarasse",
        ));
    }
    Ok(bytes)
}

/// Legge un pacchetto.
///
/// L'ordine dei controlli è deliberato: prima la dimensione dell'archivio, poi i
/// nomi e le dimensioni **dichiarate** delle voci, e solo dopo la
/// decompressione. Un controllo fatto dopo aver decompresso non protegge da
/// niente.
///
/// # Errori
///
/// `skin.tooLarge` se l'archivio sfora, `skin.assetRejected` se una voce non è
/// ammessa, `skin.packageCorrupt` se l'archivio non si legge, e i codici della
/// validazione se il manifest non è valido.
pub fn read_skin_package(archive: &[u8]) -> Result<SkinPackage, AppError> {
    let dimensione = archive.len() as u64;
    if dimensione > limits::MAX_ARCHIVE_BYTES {
        return Err(AppError::new(ErrorCode::SkinTooLarge {
            bytes: dimensione,
            limit_bytes: limits::MAX_ARCHIVE_BYTES,
        }));
    }

    // Un archivio illeggibile è corrotto, non malevolo: sono due messaggi
    // diversi e chi lo riceve deve poterli distinguere — uno si riscarica,
    // l'altro no.
    let mut archivio = ZipArchive::new(Cursor::new(archive))
        .map_err(|err| corrotto(&format!("archivio non leggibile: {err}")))?;

    let ammesse = preflight(&mut archivio)?;

    let mut source: Option<String> = None;
    let mut preview: Option<Vec<u8>> = None;
    let mut assets: Vec<SkinAsset> = Vec::new();

    for ammessa in &ammesse {
        let bytes = leggi_voce(&mut archivio, ammessa)?;
        match ammessa.voce {
            Voce::Manifest => {
                source = Some(String::from_utf8(bytes).map_err(|_| {
                    AppError::new(ErrorCode::SkinManifestInvalid {
                        detail: Some("il manifest non è testo UTF-8".to_owned()),
                    })
                })?);
            }
            Voce::Miniatura => {
                let png = tipo("png").map_or(&[][..], |t| t.firma);
                if !comincia_con(&bytes, png) {
                    return Err(rifiutata(PREVIEW_NAME, "la miniatura non è un PNG"));
                }
                preview = Some(bytes);
            }
            Voce::Risorsa(tipo) => {
                // Il tipo dai byte, non dall'estensione: un file chiamato .png
                // che contiene altro è precisamente il caso da fermare.
                if !comincia_con(&bytes, tipo.firma) {
                    return Err(rifiutata(
                        &ammessa.nome,
                        format!(
                            "il contenuto non corrisponde all'estensione .{}",
                            tipo.estensione
                        ),
                    ));
                }
                assets.push(SkinAsset {
                    name: ammessa
                        .nome
                        .strip_prefix(PREFISSO_RISORSE)
                        .unwrap_or(&ammessa.nome)
                        .to_owned(),
                    mime: tipo.mime,
                    bytes,
                });
            }
        }
    }

    let Some(source) = source else {
        return Err(corrotto(&format!("manca {MANIFEST_NAME}")));
    };

    assets.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(SkinPackage {
        document: parse_skin_json(&source)?,
        source,
        preview,
        assets,
    })
}

/// Cosa mettere in un pacchetto.
pub struct WritePackageInput<'a> {
    /// Il manifest nella forma **sorgente**, cioè come lo scrive un autore.
    ///
    /// Non il documento validato: la forma interna non si può riserializzare, e
    /// un manifest costruito da quella verrebbe rifiutato dalla lettura.
    pub source: &'a str,
    /// La miniatura, se c'è.
    pub preview: Option<&'a [u8]>,
    /// Le risorse.
    pub assets: &'a [SkinAsset],
}

/// Scrive un pacchetto.
///
/// Passa dalle **stesse** guardie della lettura, applicate al proprio output: un
/// pacchetto che questo codice produce e che la lettura rifiuterebbe è un bug, e
/// scoprirlo qui è meglio che scoprirlo sul telefono di qualcuno dopo il
/// trasferimento.
///
/// # Errori
///
/// I codici della validazione se il manifest non è valido, `skin.assetRejected`
/// se una risorsa non è ammessa, `skin.tooLarge` se ne supera il limite.
pub fn write_skin_package(input: &WritePackageInput<'_>) -> Result<Vec<u8>, AppError> {
    // Si valida prima di scrivere: un pacchetto non valido non deve poter
    // esistere, e scoprirlo all'esportazione è incomparabilmente meglio che
    // scoprirlo all'importazione sull'altro dispositivo.
    parse_skin_json(input.source)?;

    let mut buffer = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buffer));

        // Il manifest è testo e comprime bene. Le risorse no: png, jpg, webp e
        // woff2 sono già formati compressi, e ricomprimerli costa tempo per
        // guadagnare nulla. Nel vecchio albero il commento diceva esattamente
        // questo e il codice comprimeva tutto allo stesso livello.
        let compresso =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let intatto = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

        let mut scrivi = |nome: &str, bytes: &[u8], opzioni: SimpleFileOptions| {
            zip.start_file(nome, opzioni)
                .and_then(|()| zip.write_all(bytes).map_err(Into::into))
                .map_err(|err| corrotto(&format!("«{nome}» non si è potuto scrivere: {err}")))
        };

        scrivi(MANIFEST_NAME, input.source.as_bytes(), compresso)?;

        if let Some(preview) = input.preview {
            let png = tipo("png").map_or(&[][..], |t| t.firma);
            if !comincia_con(preview, png) {
                return Err(rifiutata(PREVIEW_NAME, "la miniatura deve essere un PNG"));
            }
            scrivi(PREVIEW_NAME, preview, intatto)?;
        }

        for asset in input.assets {
            let nome = format!("{PREFISSO_RISORSE}{}", asset.name);
            let Some(Voce::Risorsa(tipo)) = classifica(&nome) else {
                return Err(rifiutata(&asset.name, "nome non ammesso dal formato"));
            };
            if !comincia_con(&asset.bytes, tipo.firma) {
                return Err(rifiutata(
                    &asset.name,
                    "il contenuto non corrisponde all'estensione",
                ));
            }
            if asset.bytes.len() as u64 > limits::MAX_ENTRY_BYTES {
                return Err(AppError::new(ErrorCode::SkinTooLarge {
                    bytes: asset.bytes.len() as u64,
                    limit_bytes: limits::MAX_ENTRY_BYTES,
                }));
            }
            scrivi(&nome, &asset.bytes, intatto)?;
        }

        zip.finish()
            .map_err(|err| corrotto(&format!("l'archivio non si è chiuso: {err}")))?;
    }

    Ok(buffer)
}

/// Il nome del file per una skin.
///
/// L'id è già vincolato a minuscole, cifre e trattini, e la versione a tre
/// numeri: il nome è sicuro per costruzione, e non c'è niente da sanificare qui.
#[must_use]
pub fn package_file_name(document: &SkinDocument) -> String {
    format!("{}-{}.aeskin", document.id, document.meta.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r##"{
      "format": 1,
      "id": "prova",
      "meta": { "name": "Prova", "author": "Aether", "version": "1.2.3" },
      "tokens": { "color.accent": "#8b7cf6" }
    }"##;

    const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

    /// Un archivio costruito a mano, per poter mettere dentro quel che il nostro
    /// scrittore non produrrebbe mai.
    fn archivio(voci: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            let opzioni =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for (nome, bytes) in voci {
                zip.start_file(*nome, opzioni).expect("voce");
                zip.write_all(bytes).expect("byte");
            }
            zip.finish().expect("chiusura");
        }
        buffer
    }

    #[test]
    fn un_pacchetto_scritto_si_rilegge() {
        let scritto = write_skin_package(&WritePackageInput {
            source: MANIFEST,
            preview: Some(PNG),
            assets: &[SkinAsset {
                name: "sfondo.png".to_owned(),
                mime: "image/png",
                bytes: PNG.to_vec(),
            }],
        })
        .expect("scritto");

        let letto = read_skin_package(&scritto).expect("letto");
        assert_eq!(letto.document.id, "prova");
        // La sorgente torna indietro identica: è la forma che un editor modifica.
        assert_eq!(letto.source, MANIFEST);
        assert_eq!(letto.preview.as_deref(), Some(PNG));
        assert_eq!(letto.assets.len(), 1);
        assert_eq!(letto.assets.first().map(|a| a.mime), Some("image/png"));
        assert_eq!(package_file_name(&letto.document), "prova-1.2.3.aeskin");
    }

    #[test]
    fn il_path_traversal_non_e_un_percorso_da_normalizzare() {
        for nome in [
            "../../.ssh/authorized_keys",
            "assets/../../x.png",
            "assets/sotto/x.png",
            "/etc/passwd",
            "C:\\Windows\\x.png",
            "skin.json.bak",
            "assets/x.svg",
        ] {
            let dati = archivio(&[(nome, PNG), (MANIFEST_NAME, MANIFEST.as_bytes())]);
            let err = read_skin_package(&dati).expect_err(&format!("accettato: {nome}"));
            assert_eq!(
                err.code().kind(),
                aether_domain::errors::ErrorCodeKind::SkinAssetRejected,
                "{nome}"
            );
        }
    }

    #[test]
    fn un_png_che_non_e_un_png_si_ferma() {
        // Il caso da fermare: l'estensione dice una cosa, i byte un'altra.
        let dati = archivio(&[
            (MANIFEST_NAME, MANIFEST.as_bytes()),
            ("assets/finta.png", b"MZ\x90\x00 non sono un png"),
        ]);
        let err = read_skin_package(&dati).expect_err("accettato");
        assert!(
            err.code().kind().code().starts_with("skin.assetRejected"),
            "{:?}",
            err.code()
        );
    }

    #[test]
    fn una_bomba_si_ferma_prima_di_decomprimerla() {
        // Cinque megabyte di zeri comprimono a pochissimo: il rapporto è
        // l'unica guardia che li vede senza espanderli.
        let gonfio = vec![0_u8; 5 * 1024 * 1024];
        let dati = archivio(&[
            (MANIFEST_NAME, MANIFEST.as_bytes()),
            ("assets/bomba.png", &gonfio),
        ]);
        let err = read_skin_package(&dati).expect_err("accettato");
        let messaggio = format!("{:?}", err.code());
        assert!(messaggio.contains("compressione"), "{messaggio}");
    }

    #[test]
    fn un_archivio_senza_manifest_e_corrotto() {
        let dati = archivio(&[(PREVIEW_NAME, PNG)]);
        let err = read_skin_package(&dati).expect_err("accettato");
        assert_eq!(
            err.code().kind(),
            aether_domain::errors::ErrorCodeKind::SkinPackageCorrupt
        );
    }

    #[test]
    fn quel_che_non_e_un_archivio_e_corrotto_non_malevolo() {
        // Due messaggi diversi: uno si riscarica, l'altro no.
        let err = read_skin_package(b"non sono uno zip").expect_err("accettato");
        assert_eq!(
            err.code().kind(),
            aether_domain::errors::ErrorCodeKind::SkinPackageCorrupt
        );
    }

    #[test]
    fn un_manifest_non_valido_non_esce_da_qui() {
        // Scoprirlo all'esportazione è incomparabilmente meglio che scoprirlo
        // all'importazione sull'altro dispositivo.
        let rotto = MANIFEST.replace("#8b7cf6", "blu");
        let err = write_skin_package(&WritePackageInput {
            source: &rotto,
            preview: None,
            assets: &[],
        })
        .expect_err("scritto");
        assert_eq!(
            err.code().kind(),
            aether_domain::errors::ErrorCodeKind::SkinTokenInvalid
        );
    }

    #[test]
    fn una_risorsa_col_nome_sbagliato_non_si_scrive() {
        let err = write_skin_package(&WritePackageInput {
            source: MANIFEST,
            preview: None,
            assets: &[SkinAsset {
                name: "../fuori.png".to_owned(),
                mime: "image/png",
                bytes: PNG.to_vec(),
            }],
        })
        .expect_err("scritto");
        assert_eq!(
            err.code().kind(),
            aether_domain::errors::ErrorCodeKind::SkinAssetRejected
        );
    }
}
