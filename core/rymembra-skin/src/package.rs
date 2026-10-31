//! Il pacchetto `.ryskin`: leggerlo e scriverlo.
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

use rymembra_domain::errors::{AppError, ErrorCode};
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

/// Il tipo di una risorsa dal suo nome dentro `assets/`, se il nome è ammesso.
///
/// È la stessa regola che decide quali voci un pacchetto può contenere, messa
/// a disposizione di chi **nomina** una risorsa — il documento, quando un font
/// o un'immagine la richiamano. Due regole per lo stesso nome sarebbero due
/// alfabeti, e un nome che uno accetta e l'altro no sarebbe un riferimento che
/// si scrive e non si risolve mai.
#[must_use]
pub fn tipo_risorsa(nome: &str) -> Option<&'static str> {
    match classifica(&format!("{PREFISSO_RISORSE}{nome}"))? {
        Voce::Risorsa(tipo) => Some(tipo.mime),
        Voce::Manifest | Voce::Miniatura => None,
    }
}

/// Il tipo di una risorsa dal suo nome **e** dai suoi byte.
///
/// `None` se il nome non è ammesso, o se i byte dicono un'altra cosa rispetto
/// all'estensione. È il controllo che [`read_skin_package`] fa voce per voce,
/// per chi legge una risorsa che non sta in un archivio: la bozza dello Studio
/// tiene le sue in una cartella, e un file rinominato lì dentro non è più
/// fidato di uno arrivato zippato.
#[must_use]
pub fn riconosci_risorsa(nome: &str, bytes: &[u8]) -> Option<&'static str> {
    match classifica(&format!("{PREFISSO_RISORSE}{nome}"))? {
        Voce::Risorsa(tipo) if comincia_con(bytes, tipo.firma) => Some(tipo.mime),
        Voce::Risorsa(_) | Voce::Manifest | Voce::Miniatura => None,
    }
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
            #[expect(
                clippy::cast_precision_loss,
                reason = "il rapporto serve a confrontarlo con una soglia e a stamparlo senza \
                          decimali: i bit che un u64 enorme perderebbe in fondo non spostano né \
                          l'una né l'altro"
            )]
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
    letti: &mut u64,
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

    // E la somma, sui byte **letti** e non su quelli dichiarati.
    //
    // Il conto di `preflight` è sull'indice, cioè su numeri che chi ha
    // costruito l'archivio ha scritto lui: un indice che dichiara cento byte
    // per voce passa il totale di sessanta megabyte qualunque cosa contenga.
    // Con `deflate` un flusso rende fino a circa mille volte quel che occupa,
    // quindi mezzo megabyte di archivio bastava a farne decomprimere cinquecento
    // — sessantadue voci da otto megabyte ciascuna, ognuna sotto il proprio
    // tetto, e la somma mai contata. Su Android è l'applicazione uccisa dal
    // sistema, che è il caso che il tetto totale esiste per impedire.
    *letti = letti.saturating_add(bytes.len() as u64);
    if *letti > limits::MAX_TOTAL_BYTES {
        return Err(rifiutata(
            &ammessa.nome,
            "il contenuto decompresso supera il limite totale",
        ));
    }
    Ok(bytes)
}

/// Un pacchetto passato dalle guardie dell'archivio, col manifest **non**
/// ancora validato.
///
/// # Perché esiste una lettura a metà
///
/// Per lo Studio. Una skin installata può smettere di passare la validazione
/// senza che nessuno la tocchi — una regola nuova del formato, un budget che
/// adesso è un errore — e fino alla 3.0 lo Studio la leggeva con
/// [`read_skin_package`], che fallisce proprio lì: la skin che più aveva
/// bisogno dell'editor era l'unica che l'editor non apriva. Qui le guardie
/// contro un archivio ostile ci sono tutte — i nomi, le dimensioni, il
/// rapporto di compressione, le firme dei tipi — e manca soltanto la domanda
/// «il documento è valido?», che nello Studio fa la validazione riga per riga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacchettoGrezzo {
    /// Il manifest come è scritto, valido o no.
    pub source: String,
    /// La miniatura, se c'è.
    pub preview: Option<Vec<u8>>,
    /// Le risorse, in ordine di nome.
    pub assets: Vec<SkinAsset>,
}

/// Un archivio aperto, e le voci che i controlli sull'indice hanno ammesso.
type Aperto<'a> = (ZipArchive<Cursor<&'a [u8]>>, Vec<Ammessa>);

/// Un archivio aperto, dopo i controlli che non decomprimono niente.
fn apri_archivio(archive: &[u8]) -> Result<Aperto<'_>, AppError> {
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
    Ok((archivio, ammesse))
}

fn manifest_da(bytes: Vec<u8>) -> Result<String, AppError> {
    String::from_utf8(bytes).map_err(|_| {
        AppError::new(ErrorCode::SkinManifestInvalid {
            detail: Some("il manifest non è testo UTF-8".to_owned()),
        })
    })
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
    let grezzo = leggi_pacchetto_grezzo(archive)?;
    let document = parse_skin_json(&grezzo.source)?;
    nominate_presenti(&document, grezzo.assets.iter().map(|a| a.name.as_str()))?;
    Ok(SkinPackage {
        document,
        source: grezzo.source,
        preview: grezzo.preview,
        assets: grezzo.assets,
    })
}

/// Legge un pacchetto con tutte le guardie dell'archivio, senza validare il
/// manifest. Vedi [`PacchettoGrezzo`].
///
/// # Errori
///
/// Quelli di [`read_skin_package`] meno quelli della validazione: un manifest
/// che non è testo UTF-8 resta un errore, perché non c'è niente da aprire.
pub fn leggi_pacchetto_grezzo(archive: &[u8]) -> Result<PacchettoGrezzo, AppError> {
    let (mut archivio, ammesse) = apri_archivio(archive)?;

    let mut source: Option<String> = None;
    let mut preview: Option<Vec<u8>> = None;
    let mut assets: Vec<SkinAsset> = Vec::new();

    let mut letti: u64 = 0;
    for ammessa in &ammesse {
        let bytes = leggi_voce(&mut archivio, ammessa, &mut letti)?;
        match ammessa.voce {
            Voce::Manifest => {
                source = Some(manifest_da(bytes)?);
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
    Ok(PacchettoGrezzo {
        source,
        preview,
        assets,
    })
}

/// Il manifest di un pacchetto, e i nomi delle risorse che l'archivio
/// dichiara.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifesto {
    /// Il manifest come è scritto, non validato.
    pub source: String,
    /// I nomi delle risorse, dentro `assets/` e senza prefisso, in ordine.
    pub risorse: Vec<String>,
}

/// Legge il solo manifest di un pacchetto, senza decomprimere le risorse.
///
/// È la lettura dell'elenco delle skin: per scrivere un nome e tre colori su
/// una scheda non serve espandere due caratteri e un'immagine di sfondo, e
/// fino alla 3.0 l'elenco lo faceva per ogni skin installata a ogni apertura
/// delle Impostazioni. Le guardie sono le stesse — l'indice passa da
/// [`preflight`] voce per voce, e il manifest si legge col suo tetto — e i nomi
/// delle risorse arrivano dall'indice, così [`risorse_presenti`] può dire se
/// il documento nomina qualcosa che manca senza aprire niente.
///
/// # Errori
///
/// Quelli di [`leggi_pacchetto_grezzo`] che riguardano l'indice e il manifest.
pub fn leggi_manifesto(archive: &[u8]) -> Result<Manifesto, AppError> {
    let (mut archivio, ammesse) = apri_archivio(archive)?;
    let mut source: Option<String> = None;
    let mut risorse = Vec::new();
    let mut letti: u64 = 0;
    for ammessa in &ammesse {
        match ammessa.voce {
            Voce::Manifest => {
                source = Some(manifest_da(leggi_voce(
                    &mut archivio,
                    ammessa,
                    &mut letti,
                )?)?);
            }
            Voce::Risorsa(_) => risorse.push(
                ammessa
                    .nome
                    .strip_prefix(PREFISSO_RISORSE)
                    .unwrap_or(&ammessa.nome)
                    .to_owned(),
            ),
            Voce::Miniatura => {}
        }
    }
    let Some(source) = source else {
        return Err(corrotto(&format!("manca {MANIFEST_NAME}")));
    };
    risorse.sort();
    Ok(Manifesto { source, risorse })
}

/// Ogni risorsa che il documento nomina sta fra quelle date.
///
/// È il controllo che la lettura e la scrittura di un pacchetto fanno da sé,
/// per chi ha i nomi e non le risorse: l'elenco delle skin, che legge solo il
/// manifest ([`leggi_manifesto`]).
///
/// # Errori
///
/// `skin.assetRejected` col nome della prima risorsa nominata e assente.
pub fn risorse_presenti(document: &SkinDocument, nomi: &[String]) -> Result<(), AppError> {
    nominate_presenti(document, nomi.iter().map(String::as_str))
}

/// Quanto occupa nell'archivio una voce oltre al suo contenuto, quando il nome
/// non si conosce.
///
/// L'intestazione locale (30 byte), quella dell'indice (46), e il nome scritto
/// in tutte e due, contato a ottanta caratteri. **Non** è un tetto del
/// formato: la lettura non limita la lunghezza di un nome, e lo Studio lo
/// ricava dal nome del file senza tagliarlo. Fino alla 3.2.0 di prova questo commento
/// diceva il contrario, e una bozza con nomi di 250 caratteri vicina ai 20 MB
/// passava la domanda e si fermava all'esportazione. Chi ha i nomi chiede a
/// [`entra_in_un_pacchetto_con_nomi`], che conta [`cornice`].
const CORNICE_VOCE: u64 = 30 + 46 + 2 * 80;

/// Quanto occupa nell'archivio una voce oltre al suo contenuto: le due
/// intestazioni, locale (30 byte) e dell'indice (46), e il nome intero scritto
/// in tutte e due.
fn cornice(nome: &str) -> u64 {
    let lunghezza = u64::try_from(nome.len()).unwrap_or(u64::MAX);
    lunghezza.saturating_mul(2).saturating_add(30 + 46)
}

/// La coda dell'archivio: il record di fine dell'indice.
const CODA_ARCHIVIO: u64 = 22;

/// Un pacchetto con queste voci starebbe nei limiti della lettura?
///
/// La domanda che lo scrittore si fa prima di scrivere, e che lo Studio si fa
/// prima di aggiungere una risorsa alla bozza: fino alla 3.0 una bozza poteva
/// raccogliere risorse finché l'esportazione — o peggio l'installazione su
/// un altro computer — non la rifiutava tutta insieme. Le risorse si scrivono
/// senza comprimerle, quindi il tetto che morde davvero è quello
/// dell'archivio, non quello del decompresso; il manifest si conta intero, che
/// è il caso peggiore.
///
/// Senza i nomi la cornice di ogni voce è una stima ([`CORNICE_VOCE`]), che
/// sbaglia per difetto con nomi oltre gli ottanta caratteri: chi li ha usi
/// [`entra_in_un_pacchetto_con_nomi`].
///
/// # Errori
///
/// `skin.tooLarge` per una voce, il totale o l'archivio oltre il loro tetto,
/// `skin.assetRejected` per troppe voci.
pub fn entra_in_un_pacchetto(
    manifest: u64,
    preview: Option<u64>,
    risorse: impl IntoIterator<Item = u64>,
) -> Result<(), AppError> {
    misura_del_pacchetto(
        (manifest, CORNICE_VOCE),
        preview.map(|byte| (byte, CORNICE_VOCE)),
        risorse.into_iter().map(|byte| (byte, CORNICE_VOCE)),
    )
}

/// Come [`entra_in_un_pacchetto`], con i nomi delle risorse: la misura esatta.
///
/// `risorse` sono coppie (nome dentro `assets/`, senza prefisso; byte), come
/// [`SkinAsset::name`] e la lunghezza di [`SkinAsset::bytes`].
///
/// # Errori
///
/// Gli stessi di [`entra_in_un_pacchetto`].
pub fn entra_in_un_pacchetto_con_nomi<'a>(
    manifest: u64,
    preview: Option<u64>,
    risorse: impl IntoIterator<Item = (&'a str, u64)>,
) -> Result<(), AppError> {
    misura_del_pacchetto(
        (manifest, cornice(MANIFEST_NAME)),
        preview.map(|byte| (byte, cornice(PREVIEW_NAME))),
        risorse
            .into_iter()
            .map(|(nome, byte)| (byte, cornice(&format!("{PREFISSO_RISORSE}{nome}")))),
    )
}

/// Il conto comune: ogni voce con i suoi byte e la sua cornice.
fn misura_del_pacchetto(
    (manifest, cornice_manifest): (u64, u64),
    preview: Option<(u64, u64)>,
    risorse: impl Iterator<Item = (u64, u64)>,
) -> Result<(), AppError> {
    let troppo = |bytes: u64, limit_bytes: u64| {
        Err(AppError::new(ErrorCode::SkinTooLarge {
            bytes,
            limit_bytes,
        }))
    };
    if manifest > limits::MAX_MANIFEST_BYTES {
        return troppo(manifest, limits::MAX_MANIFEST_BYTES);
    }
    let mut voci: usize = 1;
    let mut totale = manifest;
    let mut archivio = CODA_ARCHIVIO
        .saturating_add(manifest)
        .saturating_add(cornice_manifest);
    for (voce, cornice) in preview.into_iter().chain(risorse) {
        if voce > limits::MAX_ENTRY_BYTES {
            return troppo(voce, limits::MAX_ENTRY_BYTES);
        }
        voci = voci.saturating_add(1);
        totale = totale.saturating_add(voce);
        archivio = archivio.saturating_add(voce).saturating_add(cornice);
    }
    if voci > limits::MAX_ENTRIES {
        return Err(rifiutata(
            "(archivio)",
            format!(
                "un pacchetto tiene al più {} voci, e questo ne avrebbe {voci}",
                limits::MAX_ENTRIES
            ),
        ));
    }
    if totale > limits::MAX_TOTAL_BYTES {
        return troppo(totale, limits::MAX_TOTAL_BYTES);
    }
    if archivio > limits::MAX_ARCHIVE_BYTES {
        return troppo(archivio, limits::MAX_ARCHIVE_BYTES);
    }
    Ok(())
}

/// Ogni risorsa che il documento nomina sta nel pacchetto.
///
/// Un carattere nominato e assente non è un errore del motore: il foglio
/// chiede un indirizzo che risponde 404, e la skin si disegna col carattere di
/// ripiego senza che niente lo dica. Si ferma qui, alla porta, con il nome di
/// quel che manca.
fn nominate_presenti<'a>(
    document: &SkinDocument,
    presenti: impl Iterator<Item = &'a str> + Clone,
) -> Result<(), AppError> {
    for nome in document.assets() {
        if !presenti.clone().any(|c| c == nome) {
            return Err(rifiutata(
                &nome,
                "il documento la nomina, e il pacchetto non la contiene",
            ));
        }
    }
    Ok(())
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
/// Fino alla 3.0 questa riga era una promessa scritta e non mantenuta: si
/// controllavano i nomi e le firme, e basta. Una bozza con settanta risorse, o
/// con venticinque megabyte di immagini, usciva come un `.ryskin` che nessuna
/// installazione avrebbe accettato. Adesso i tetti si misurano prima
/// ([`entra_in_un_pacchetto`]), e l'archivio scritto si **rilegge** con
/// [`leggi_pacchetto_grezzo`] prima di restituirlo: le guardie sono quelle, non
/// una loro copia.
///
/// # Errori
///
/// I codici della validazione se il manifest non è valido, `skin.assetRejected`
/// se una risorsa non è ammessa o se ce ne sono troppe, `skin.tooLarge` se una
/// voce, il totale o l'archivio superano il loro limite.
pub fn write_skin_package(input: &WritePackageInput<'_>) -> Result<Vec<u8>, AppError> {
    // Si valida prima di scrivere: un pacchetto non valido non deve poter
    // esistere, e scoprirlo all'esportazione è incomparabilmente meglio che
    // scoprirlo all'importazione sull'altro dispositivo.
    let document = parse_skin_json(input.source)?;
    nominate_presenti(&document, input.assets.iter().map(|a| a.name.as_str()))?;
    entra_in_un_pacchetto_con_nomi(
        input.source.len() as u64,
        input.preview.map(|p| p.len() as u64),
        input
            .assets
            .iter()
            .map(|a| (a.name.as_str(), a.bytes.len() as u64)),
    )?;
    // Due risorse con lo stesso nome sono due voci con lo stesso nome: la
    // lettura ne terrebbe una a caso, e lo scrittore dello zip si ferma a metà
    // con un messaggio che non nomina la risorsa.
    let mut nomi: Vec<&str> = input.assets.iter().map(|a| a.name.as_str()).collect();
    nomi.sort_unstable();
    if let Some(doppia) = nomi.windows(2).find_map(|due| match due {
        [a, b] if a == b => Some(*a),
        _ => None,
    }) {
        return Err(rifiutata(doppia, "due risorse con lo stesso nome"));
    }

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

    // La rilettura: le guardie della lettura sul pacchetto appena scritto.
    // Il documento è già stato validato sopra, quindi basta la metà che
    // riguarda l'archivio.
    leggi_pacchetto_grezzo(&buffer)?;
    Ok(buffer)
}

/// Il nome del file per una skin.
///
/// L'id è già vincolato a minuscole, cifre e trattini, e la versione a tre
/// numeri: il nome è sicuro per costruzione, e non c'è niente da sanificare qui.
#[must_use]
pub fn package_file_name(document: &SkinDocument) -> String {
    format!("{}-{}.ryskin", document.id, document.meta.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r##"{
      "format": 1,
      "id": "prova",
      "meta": { "name": "Prova", "author": "Rymembra", "version": "1.2.3" },
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
        assert_eq!(package_file_name(&letto.document), "prova-1.2.3.ryskin");
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
                rymembra_domain::errors::ErrorCodeKind::SkinAssetRejected,
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

    /// Riscrive nell'indice la dimensione decompressa dichiarata di ogni voce.
    ///
    /// Serve a costruire quel che il nostro scrittore non produce mai: un
    /// archivio che dichiara poco e contiene molto. I quattro byte stanno a
    /// scarto 24 dentro un record dell'indice centrale, subito dopo il CRC e la
    /// dimensione compressa.
    fn indice_bugiardo(archivio: &mut [u8], dichiarata: u32) {
        const FIRMA: &[u8] = &[0x50, 0x4b, 0x01, 0x02];
        const SCARTO: usize = 24;
        for inizio in 0..archivio.len() {
            if archivio.get(inizio..inizio.saturating_add(4)) != Some(FIRMA) {
                continue;
            }
            let da = inizio.saturating_add(SCARTO);
            if let Some(campo) = archivio.get_mut(da..da.saturating_add(4)) {
                campo.copy_from_slice(&dichiarata.to_le_bytes());
            }
        }
    }

    #[test]
    fn un_indice_che_mente_non_fa_saltare_il_totale() {
        // Otto voci da otto megabyte: ognuna sta sotto il **proprio** tetto, e
        // insieme sfondano quello totale. L'indice ne dichiara cento byte
        // l'una, quindi il conto di `preflight` — che legge l'indice — non le
        // vede passare, e nemmeno il rapporto di compressione, che è anche lui
        // un rapporto fra due numeri dichiarati.
        //
        // Con `deflate` un flusso rende fino a circa mille volte quel che
        // occupa: questo archivio sta in poche decine di kilobyte.
        let quanto = usize::try_from(limits::MAX_ENTRY_BYTES).unwrap_or(usize::MAX);
        let mut grossa = vec![0_u8; quanto];
        if let Some(testa) = grossa.get_mut(..PNG.len()) {
            testa.copy_from_slice(PNG);
        }

        let nomi: Vec<String> = (0..8).map(|i| format!("assets/a{i}.png")).collect();
        let mut voci: Vec<(&str, &[u8])> = vec![(MANIFEST_NAME, MANIFEST.as_bytes())];
        voci.extend(nomi.iter().map(|nome| (nome.as_str(), grossa.as_slice())));
        let mut dati = archivio(&voci);
        assert!(
            dati.len() as u64 <= limits::MAX_ARCHIVE_BYTES,
            "l'archivio da solo sfora, e il test proverebbe un'altra guardia"
        );
        indice_bugiardo(&mut dati, 100);

        let err = read_skin_package(&dati).expect_err("accettato");
        let messaggio = format!("{:?}", err.code());
        assert!(messaggio.contains("limite totale"), "{messaggio}");
    }

    #[test]
    fn un_archivio_senza_manifest_e_corrotto() {
        let dati = archivio(&[(PREVIEW_NAME, PNG)]);
        let err = read_skin_package(&dati).expect_err("accettato");
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::SkinPackageCorrupt
        );
    }

    #[test]
    fn quel_che_non_e_un_archivio_e_corrotto_non_malevolo() {
        // Due messaggi diversi: uno si riscarica, l'altro no.
        let err = read_skin_package(b"non sono uno zip").expect_err("accettato");
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::SkinPackageCorrupt
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
            rymembra_domain::errors::ErrorCodeKind::SkinTokenInvalid
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
            rymembra_domain::errors::ErrorCodeKind::SkinAssetRejected
        );
    }

    #[test]
    fn una_risorsa_fuori_dal_pacchetto_si_riconosce_dai_byte() {
        assert_eq!(
            riconosci_risorsa("titoli.woff2", b"wOF2..."),
            Some("font/woff2")
        );
        assert_eq!(
            riconosci_risorsa("trama.png", b"\x89PNG..."),
            Some("image/png")
        );
        // Il nome dice una cosa, i byte un'altra.
        assert_eq!(riconosci_risorsa("trama.png", b"GIF89a"), None);
        // Il nome non è ammesso, qualunque cosa ci sia dentro.
        assert_eq!(riconosci_risorsa("../trama.png", b"\x89PNG"), None);
        assert_eq!(riconosci_risorsa("trama.svg", b"<svg"), None);
    }

    #[test]
    fn una_risorsa_nominata_deve_stare_nel_pacchetto() {
        let sorgente = crate::PLAIN_SOURCE.replacen(
            "\"id\": \"plain\",",
            "\"id\": \"plain\", \"fonts\": { \"Prova\": { \"src\": \"prova.woff2\" } },",
            1,
        );
        let senza = write_skin_package(&WritePackageInput {
            source: &sorgente,
            preview: None,
            assets: &[],
        })
        .expect_err("manca il carattere");
        assert_eq!(
            senza.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::SkinAssetRejected
        );

        let con = write_skin_package(&WritePackageInput {
            source: &sorgente,
            preview: None,
            assets: &[SkinAsset {
                name: "prova.woff2".to_owned(),
                mime: "font/woff2",
                bytes: b"wOF2 finto".to_vec(),
            }],
        })
        .expect("col carattere si scrive");
        let letto = read_skin_package(&con).expect("e si rilegge");
        assert_eq!(letto.document.assets(), vec!["prova.woff2".to_owned()]);
    }

    #[test]
    fn un_manifest_rotto_si_legge_grezzo_e_non_si_installa() {
        // La skin che non passa più la validazione: lo Studio la deve poter
        // aprire per aggiustarla, l'installazione no.
        let rotto = MANIFEST.replace("#8b7cf6", "blu");
        let dati = archivio(&[
            (MANIFEST_NAME, rotto.as_bytes()),
            (PREVIEW_NAME, PNG),
            ("assets/trama.png", PNG),
        ]);
        let grezzo = leggi_pacchetto_grezzo(&dati).expect("le guardie passano");
        assert_eq!(grezzo.source, rotto);
        assert_eq!(grezzo.preview.as_deref(), Some(PNG));
        assert_eq!(grezzo.assets.len(), 1);
        assert!(read_skin_package(&dati).is_err());

        // E le guardie restano: un nome storto ferma anche la lettura grezza.
        let ostile = archivio(&[(MANIFEST_NAME, rotto.as_bytes()), ("../x.png", PNG)]);
        assert!(leggi_pacchetto_grezzo(&ostile).is_err());
    }

    #[test]
    fn il_solo_manifest_si_legge_senza_le_risorse() {
        let dati = archivio(&[
            ("assets/zeta.png", PNG),
            (MANIFEST_NAME, MANIFEST.as_bytes()),
            ("assets/alfa.woff2", b"wOF2 finto"),
        ]);
        let letto = leggi_manifesto(&dati).expect("letto");
        assert_eq!(letto.source, MANIFEST);
        assert_eq!(letto.risorse, ["alfa.woff2", "zeta.png"]);

        let sorgente = crate::PLAIN_SOURCE.replacen(
            "\"id\": \"plain\",",
            "\"id\": \"plain\", \"fonts\": { \"Prova\": { \"src\": \"prova.woff2\" } },",
            1,
        );
        let documento = parse_skin_json(&sorgente).expect("valido");
        assert!(risorse_presenti(&documento, &letto.risorse).is_err());
        assert!(risorse_presenti(&documento, &["prova.woff2".to_owned()]).is_ok());
    }

    #[test]
    fn lo_scrittore_rispetta_i_tetti_della_lettura() {
        // Troppe voci: sessantacinque risorse minuscole.
        let tante: Vec<SkinAsset> = (0..limits::MAX_ENTRIES)
            .map(|i| SkinAsset {
                name: format!("r{i}.png"),
                mime: "image/png",
                bytes: PNG.to_vec(),
            })
            .collect();
        let err = write_skin_package(&WritePackageInput {
            source: MANIFEST,
            preview: None,
            assets: &tante,
        })
        .expect_err("troppe voci");
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::SkinAssetRejected
        );

        // Tre risorse da sette megabyte: ognuna sta sotto il suo tetto, e
        // insieme non stanno in un archivio.
        let quanto = 7 * 1024 * 1024;
        let grosse: Vec<SkinAsset> = (0..3)
            .map(|i| {
                let mut bytes = vec![0_u8; quanto];
                if let Some(testa) = bytes.get_mut(..PNG.len()) {
                    testa.copy_from_slice(PNG);
                }
                SkinAsset {
                    name: format!("g{i}.png"),
                    mime: "image/png",
                    bytes,
                }
            })
            .collect();
        let err = write_skin_package(&WritePackageInput {
            source: MANIFEST,
            preview: None,
            assets: &grosse,
        })
        .expect_err("troppo grande");
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::SkinTooLarge
        );

        // Due risorse con lo stesso nome.
        let doppie = [
            SkinAsset {
                name: "x.png".to_owned(),
                mime: "image/png",
                bytes: PNG.to_vec(),
            },
            SkinAsset {
                name: "x.png".to_owned(),
                mime: "image/png",
                bytes: PNG.to_vec(),
            },
        ];
        let err = write_skin_package(&WritePackageInput {
            source: MANIFEST,
            preview: None,
            assets: &doppie,
        })
        .expect_err("doppie");
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::SkinAssetRejected
        );
    }

    #[test]
    fn la_misura_di_un_pacchetto_si_chiede_prima_di_scriverlo() {
        assert!(entra_in_un_pacchetto(10_000, Some(4_000), [1_000, 2_000]).is_ok());
        assert!(entra_in_un_pacchetto(limits::MAX_MANIFEST_BYTES + 1, None, []).is_err());
        assert!(entra_in_un_pacchetto(10, None, [limits::MAX_ENTRY_BYTES + 1]).is_err());
        let venti = [
            limits::MAX_ENTRY_BYTES,
            limits::MAX_ENTRY_BYTES,
            4 * 1024 * 1024,
        ];
        assert!(entra_in_un_pacchetto(10, None, venti).is_err());
    }

    #[test]
    fn un_nome_di_risorsa_lungo_non_sfugge_alla_stima() {
        // Il formato non pone un tetto alla lunghezza di un nome, e lo Studio
        // lo ricava dal nome del file senza tagliarlo. Sessantatré risorse con
        // un nome di 254 caratteri: la stima con nomi sotto gli ottanta le
        // lascia passare, l'archivio vero sfora di una ventina di KB.
        let lungo = format!("{}.png", "a".repeat(250));
        let quante = limits::MAX_ENTRIES - 1;
        let ciascuna: u64 = 332_500;
        let lunghi: Vec<(&str, u64)> = (0..quante).map(|_| (lungo.as_str(), ciascuna)).collect();
        assert!(
            entra_in_un_pacchetto(1_000, None, lunghi.iter().map(|(_, b)| *b)).is_ok(),
            "la stima di prima"
        );
        let err = entra_in_un_pacchetto_con_nomi(1_000, None, lunghi.iter().copied())
            .expect_err("con i nomi veri sfora");
        assert_eq!(
            err.code().kind(),
            rymembra_domain::errors::ErrorCodeKind::SkinTooLarge
        );
        // Gli stessi byte con nomi corti stanno nell'archivio.
        let corti: Vec<(&str, u64)> = (0..quante).map(|_| ("a.png", ciascuna)).collect();
        assert!(entra_in_un_pacchetto_con_nomi(1_000, None, corti).is_ok());
    }
}
