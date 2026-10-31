//! Leggere i tag di un file audio.
//!
//! Una libreria di lettura tag per tutti i formati, la stessa sul desktop e su
//! Android. Il vecchio albero ne usava due — `music-metadata` per leggere,
//! `node-taglib-sharp` per scrivere — e non concordavano su tutto: la copertina
//! letta da una non era sempre quella che l'altra riscriveva.

use std::io::{Read, Seek, SeekFrom};

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::picture::{Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::extension_of;
use unicode_normalization::UnicodeNormalization as _;

use crate::files::MusicFiles;

/// La versione del lettore dei tag, cioè di quel che [`read_tags`] sa leggere.
///
/// Entra nella memoria dei rifiuti della scansione (`file_illeggibili`, vedi la
/// migrazione 032): un file che questo lettore ha rifiutato non si riapre finché
/// non cambia il file **o questo numero**. Chi insegna al lettore a leggere
/// qualcosa che prima rifiutava — un formato in più, un ripiego in più — lo
/// alza, e la prima scansione dopo riprova da sola tutti i rifiutati. Non
/// alzarlo vorrebbe dire un lettore migliore che non rivede mai i file su cui
/// quello di prima aveva detto di no.
///
/// - **1** — il ripiego sull'estensione, la seconda prova col tipo
///   dell'estensione, le sole proprietà quando i tag sono rotti, il rifiuto
///   degli APE prima della 3.99.
pub const VERSIONE_LETTORE: i64 = 1;

/// La prima versione di Monkey's Audio che il motore sa suonare: la 3.99.
///
/// Sotto, `ape-decoder` non apre il file (vedi `rymembra-play`), e il brano
/// entrerebbe in libreria per poi rispondere «formato non supportato» al primo
/// clic: un brano muto è peggio di un brano che non c'è.
const APE_MINIMA: u16 = 3990;

/// La copertina trovata dentro il file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedCover {
    /// I byte dell'immagine, così come stanno nel tag.
    pub data: Vec<u8>,
    /// Il tipo dichiarato dal tag, se c'è.
    pub mime_type: Option<String>,
}

/// Quel che si è riusciti a leggere da un file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackTags {
    /// Titolo.
    pub title: Option<String>,
    /// Interprete.
    pub artist: Option<String>,
    /// Album.
    pub album: Option<String>,
    /// Artista dell'album.
    pub album_artist: Option<String>,
    /// Anno.
    pub year: Option<i32>,
    /// Numero di traccia.
    pub track_number: Option<u32>,
    /// Numero di disco.
    pub disc_number: Option<u32>,
    /// Genere.
    pub genre: Option<String>,
    /// Commento.
    pub comment: Option<String>,
    /// Testo non sincronizzato.
    pub lyrics: Option<String>,
    /// Battiti al minuto.
    pub bpm: Option<f64>,
    /// Tonalità.
    pub musical_key: Option<String>,
    /// Durata in millisecondi.
    pub duration_ms: u64,
    /// Bitrate dichiarato, in kbps.
    pub bitrate: Option<u32>,
    /// Frequenza di campionamento.
    pub sample_rate: Option<u32>,
    /// Canali.
    pub channels: Option<u8>,
    /// Il formato, come lo riporta il lettore.
    pub codec: Option<String>,
    /// Guadagno ReplayGain della traccia, in dB.
    pub replaygain_track_db: Option<f32>,
    /// Guadagno ReplayGain dell'album, in dB.
    pub replaygain_album_db: Option<f32>,
    /// Identificativo MusicBrainz della registrazione.
    pub mb_recording_id: Option<String>,
    /// Identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
    /// Identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
    /// La copertina incorporata, se c'è.
    pub cover: Option<EmbeddedCover>,
}

/// Un valore di tag ripulito: niente spazi ai bordi, vuoto vale come assente, e
/// in forma NFC.
///
/// Un titolo di soli spazi non è un titolo. Lasciarlo passare produce brani che
/// in libreria appaiono senza nome e non si possono cercare.
///
/// # Perché NFC
///
/// Perché «ö» si può scrivere in due modi: un carattere solo, U+00F6, o una «o»
/// seguita dalla dieresi combinante U+0308. Sono la stessa lettera per chi
/// legge e due stringhe diverse per chi confronta, e i taggatori del Mac — e i
/// file passati da un filesystem Apple — scrivono volentieri la seconda. Il
/// danno si vedeva nella ricerca: il tokenizzatore di FTS5 (`unicode61`) non
/// considera la dieresi staccata parte della parola, quindi «Björk» scomposto
/// finiva nell'indice come «bjo» e «rk», e cercare *björk* non lo trovava.
///
/// NFC ricompone, e da qui in poi ogni testo letto da un file ha una forma sola.
/// Le chiavi di identità non cambiano: `fold_text` passa comunque da NFD, e le
/// due forme della stessa lettera ci arrivano identiche.
pub(crate) fn clean(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else if unicode_normalization::is_nfc(trimmed) {
        // Il caso di quasi tutti i tag: nessuna ricomposizione da fare, e una
        // copia sola invece di un giro carattere per carattere.
        Some(trimmed.to_owned())
    } else {
        Some(trimmed.nfc().collect())
    }
}

/// I valori ReplayGain viaggiano come testo: `-3.21 dB`, `+1.5`, `-7,2 dB`.
///
/// Si prende il primo numero e si ignora il resto. Una virgola decimale si
/// accetta: qualche codificatore la scrive secondo la localizzazione di sistema,
/// e rifiutarla vorrebbe dire spegnere la normalizzazione del volume proprio su
/// quei file, cioè far suonare qualche brano più forte degli altri.
fn parse_replaygain(value: Option<&str>) -> Option<f32> {
    let raw = value?.trim().replace(',', ".");
    let numeric: String = raw
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    numeric.parse::<f32>().ok()
}

/// La copertina da tenere, fra quelle che il file contiene.
///
/// **Si preferisce quella dichiarata come copertina frontale.** Un file può
/// incorporarne diverse — retro, libretto, foto dell'artista — e prendere la
/// prima significa prendere quella che il taggatore ha scritto per prima, che
/// spesso è il retro. In libreria si vedrebbe una griglia di retrocopertine.
fn pick_cover(file: &TaggedFile) -> Option<EmbeddedCover> {
    let to_cover = |picture: &Picture| EmbeddedCover {
        data: picture.data().to_vec(),
        mime_type: picture.mime_type().map(ToString::to_string),
    };
    let mut fallback: Option<&Picture> = None;
    for tag in file.tags() {
        for picture in tag.pictures() {
            if picture.pic_type() == PictureType::CoverFront {
                return Some(to_cover(picture));
            }
            if fallback.is_none() {
                fallback = Some(picture);
            }
        }
    }
    fallback.map(to_cover)
}

/// Sotto questo numero non c'è musica, c'è un campo sbagliato.
///
/// Trentadue kbit/s: sotto, un file musicale non ci sta. La codifica più magra
/// che si incontri in una libreria vera — un Opus di parlato, un MP3 mono di
/// venticinque anni fa — parte da lì, e la musica stereo normale sta fra i
/// centoventotto e i trecentoventi.
///
/// **La soglia può essere generosa senza far danni**, ed è il motivo per cui
/// non è otto. Il ripiego è `overall_bitrate`, che è lo stesso numero più
/// l'involucro: su un file davvero magro i due valori distano qualche punto
/// percentuale, quindi ripiegare per sbaglio costa un'inezia. Sbagliare nel
/// verso opposto costa invece la riga dei dati tecnici che dichiara «4 kbps»
/// su un brano da centotrenta, ed è quel che succedeva con gli M4A, che
/// dichiaravano da zero a quindici.
const BITRATE_MINIMO_KBPS: u32 = 32;

/// Quanto pesa il suono, in kbit/s.
///
/// `audio_bitrate` è il numero che si vuole — quanto pesa il **suono**, non
/// quanto pesa il file con dentro la copertina e i tag — e per MP3, FLAC e Ogg
/// è quello giusto. Su MP4 no: `lofty` lo legge dal descrittore `esds`, e in
/// quel campo i taggatori scrivono quel che capita. In una libreria vera i
/// quarantaquattro M4A dichiaravano tutti da 0 a 15 kbit/s dove il conto su
/// byte e durata ne dà centotrenta, e la riga dei dati tecnici sotto i comandi
/// scriveva «M4A · 44.1 kHz · Stereo · 4 kbps» a chi stava ascoltando un file
/// normalissimo.
///
/// Sotto [`BITRATE_MINIMO_KBPS`] quindi non si crede al campo e si ripiega su
/// `overall_bitrate`, che `lofty` ricava dalla dimensione del file: comprende
/// l'involucro, quindi su un brano corto con una copertina grossa è un po'
/// generoso, ed è comunque il numero di cui chi guarda riconosce l'ordine di
/// grandezza. Meglio centotrenta con dentro i tag che quattro.
fn bitrate_del_suono(properties: &lofty::properties::FileProperties) -> Option<u32> {
    let plausibile = |kbps: &u32| *kbps >= BITRATE_MINIMO_KBPS;
    properties
        .audio_bitrate()
        .filter(plausibile)
        .or_else(|| properties.overall_bitrate().filter(plausibile))
}

/// Il racconto intero di un errore di `lofty`, cause comprese.
///
/// Dalla 0.25 gli errori di `lofty` dicono in `Display` solo **dove** si sono
/// fermati — «failed to parse Mpeg file», «failed to write Mp4 file» — e il
/// **perché** lo tengono nella catena di `source()`: l'accesso negato dal
/// sistema, il frame più lungo del file, il formato che non si riconosce. Un
/// `to_string()` da solo consegnerebbe al dettaglio dell'errore proprio la metà
/// che non aiuta nessuno, e un «non riesco a scrivere i tag» senza il motivo è
/// la segnalazione a cui non si sa rispondere.
///
/// Qui si cuciono tutti gli anelli, dal primo all'ultimo, separati da `: `.
/// Vale anche per [`crate::tag_scrittura`], che scrive con la stessa libreria
/// e fallisce allo stesso modo.
pub(crate) fn racconta_errore(errore: &(dyn std::error::Error + 'static)) -> String {
    let mut racconto = errore.to_string();
    let mut causa = errore.source();
    while let Some(anello) = causa {
        racconto.push_str(": ");
        racconto.push_str(&anello.to_string());
        causa = anello.source();
    }
    racconto
}

/// Il dettaglio di un rifiuto del lettore: `metadata.tagReadFailed` con il
/// perché.
fn rifiuto(path: &str, detail: &str) -> AppError {
    AppError::new(ErrorCode::MetadataTagReadFailed {
        path: Some(path.to_owned()),
    })
    .with_cause(detail.to_owned())
}

/// L'errore di I/O sepolto nella catena di un errore di `lofty`, se ce n'è uno
/// che parla del **disco** e non del file.
///
/// `lofty` incarta tutto in un `FileParseError`, anche la rete che cade a metà
/// lettura: detto `metadata.tagReadFailed`, quel guasto finirebbe nella memoria
/// dei rifiuti della scansione come se il file fosse rotto, e un brano buono
/// sparirebbe per sempre perché quel giorno il NAS ha singhiozzato. Qui lo si
/// ripesca e lo si restituisce per quel che è.
///
/// Qualche errore di I/O invece resta del file, perché è il modo in cui `lofty`
/// dice che la **struttura** non torna: la fine inattesa (`UnexpectedEof`, un
/// blocco che dichiara più byte di quanti ce ne siano), i dati che non sono quel
/// che dichiarano (`InvalidData`), e il salto prima dell'inizio — un'intestazione
/// che punta fuori dal file, o la ricerca dell'ID3v1 negli ultimi 128 byte di un
/// file che ne ha meno. Il salto Windows lo dice con un suo numero (vedi
/// [`SALTO_PRIMA_DELL_INIZIO`]), gli altri sistemi con `InvalidInput`. Un file
/// fatto così resta tale finché qualcuno non lo cambia — e allora cambiano anche
/// dimensione e data, e la memoria lo lascia riprovare.
///
/// Mai del file, invece, un errore di I/O che porta dentro il suo `AppError`:
/// è il lettore dei tag del cloud che dice cos'è successo davvero — un `429`,
/// la rete caduta a metà — passando dall'unica strada che `Read` gli lascia.
/// Qualunque `ErrorKind` abbia, lo traduce [`crate::files::io_error`], che lo
/// tira fuori così com'è.
fn guasto_del_disco<'a>(
    errore: &'a (dyn std::error::Error + 'static),
) -> Option<&'a std::io::Error> {
    use std::io::ErrorKind as K;
    let mut anello: Option<&(dyn std::error::Error + 'static)> = Some(errore);
    while let Some(adesso) = anello {
        if let Some(io) = adesso.downcast_ref::<std::io::Error>() {
            let della_struttura = matches!(
                io.kind(),
                K::UnexpectedEof | K::InvalidInput | K::InvalidData
            ) || io.raw_os_error() == SALTO_PRIMA_DELL_INIZIO;
            let con_il_suo_perche = io
                .get_ref()
                .is_some_and(|dentro| dentro.downcast_ref::<AppError>().is_some());
            let del_file = della_struttura
                && !rymembra_domain::errors::rete::e_di_rete(io)
                && !con_il_suo_perche;
            return (!del_file).then_some(io);
        }
        anello = adesso.source();
    }
    None
}

/// `ERROR_NEGATIVE_SEEK`: Windows rifiuta di spostarsi prima dell'inizio del
/// file con questo numero, che Rust non sa classificare e lascia
/// `Uncategorized`. Sugli altri sistemi lo stesso caso è un `InvalidInput`.
#[cfg(windows)]
const SALTO_PRIMA_DELL_INIZIO: Option<i32> = Some(131);
/// Vedi la gemella per Windows: qui il caso è già un `InvalidInput`.
#[cfg(not(windows))]
const SALTO_PRIMA_DELL_INIZIO: Option<i32> = None;

/// Perché una lettura non è riuscita: un errore da dire, o il tetto della
/// sorgente.
enum Inciampo {
    /// Un errore come tutti: del file, del disco, della rete.
    Errore(AppError),
    /// Il lettore ha superato i byte che la sorgente concede a una lettura dei
    /// tag. Vedi [`oltre_il_tetto`].
    Tetto,
}

impl From<AppError> for Inciampo {
    fn from(errore: AppError) -> Self {
        Self::Errore(errore)
    }
}

/// Il segnale «tetto superato» della sorgente: un errore di I/O di tipo
/// `FileTooLarge`.
///
/// # Chi lo manda, e perché non è un guasto
///
/// Il lettore dei tag a intervalli del cloud (`rymembra-cloud`) legge a
/// finestre piccole e conta i byte; oltre il suo tetto — otto mebibyte, cioè
/// un file le cui proprietà stanno lontane dalla testa — risponde così invece
/// di scaricare il brano intero. Non dice niente del file né della rete: dice
/// «così costa troppo», e la risposta giusta è rileggere **senza le
/// proprietà**, che sono la parte che costringe `lofty` a camminare il file. La
/// durata allora viene dal servizio, o resta zero e la scrive il primo ascolto
/// (`playback::ricorda_durata`).
///
/// Un disco non lo manda mai: per un file locale questo ramo non esiste.
fn oltre_il_tetto(io: &std::io::Error) -> bool {
    io.kind() == std::io::ErrorKind::FileTooLarge
}

/// L'errore di I/O di una lettura fallita, detto per quel che è: il tetto
/// della sorgente, o un errore del disco.
fn inciampo_di_io(path: &str, io: &std::io::Error) -> Inciampo {
    if oltre_il_tetto(io) {
        Inciampo::Tetto
    } else {
        Inciampo::Errore(crate::files::io_error(path, io))
    }
}

/// Com'è andata la lettura che ha funzionato.
struct Letto {
    /// Il file, con quel che `lofty` ne ha capito.
    file: TaggedFile,
    /// I tag erano illeggibili, e si sono lette le sole proprietà.
    solo_proprieta: bool,
}

/// Apre il file con `lofty`, ripiegando dove il primo tentativo sbaglia.
///
/// # I tre ripieghi, e i file veri che li chiedono
///
/// Misurati sui 186 file che una libreria vera rifiutava a ogni scansione:
///
/// 1. **L'estensione, quando il contenuto non basta.** 104 MP3 avevano dopo il
///    tag ID3 della spazzatura più lunga di quella che il riconoscimento dal
///    contenuto è disposto a scorrere (`max_junk_bytes`), e `lofty` concludeva
///    «nessun formato». Il tipo dell'estensione si dà **prima** di indovinare:
///    `guess_file_type` lo sostituisce solo se dal contenuto capisce qualcosa,
///    e il lettore MP3 la sua sincronia la cerca da sé, senza quel limite.
/// 2. **L'estensione, quando il contenuto inganna.** Due file si facevano
///    scambiare per un altro formato — un `.mp3` che sembrava AAC, un `.flac`
///    che sembrava MPEG — e la lettura col tipo indovinato falliva. Se il tipo
///    dell'estensione è un altro, si riprova con quello.
/// 3. **Le sole proprietà, quando sono i tag a essere rotti.** 26 MP3 avevano
///    frame ID3 (`TXXX`, `TRCK`) in UTF-8 o UTF-16 malformato. Il parsing
///    tollerante di `lofty` 0.25 (`ParsingMode::Relaxed`) **non** li salta:
///    nel sorgente salta i frame con l'intestazione rotta, ma un contenuto che
///    non si decodifica risale con un `?` da `read_all_frames_into_list` in
///    qualunque modo, e con lui l'intero file. Si rilegge allora con
///    `read_tags(false)` — durata, frequenza, canali dal flusso — e il resto
///    viene dai ripieghi dei file senza tag, più il vecchio ID3v1 in coda al
///    file se c'è (vedi [`id3v1`]). Un brano col titolo del nome del file è
///    meglio di un brano che non c'è.
///
/// Ogni ripiego si tenta solo dopo un errore **del file**: un guasto del disco
/// (vedi [`guasto_del_disco`]) esce subito per quel che è, perché riprovare su
/// una rete caduta costa un'altra attesa per lo stesso esito. Se nessun
/// tentativo riesce, l'errore è quello del primo, che è quello che racconta
/// meglio il file.
fn leggi_il_file<R: Read + Seek>(reader: &mut R, path: &str) -> Result<Letto, Inciampo> {
    // La durata va calcolata: senza, i file a bitrate variabile la riportano
    // stimata dall'intestazione, che su un VBR lungo sbaglia di parecchi
    // secondi — e la durata finisce nell'interfaccia e nei confronti di
    // somiglianza.
    let con_i_tag = ParseOptions::new().read_properties(true);
    let senza_tag = ParseOptions::new().read_properties(true).read_tags(false);
    let dall_estensione = FileType::from_ext(extension_of(path));

    let mut probe = Probe::new(&mut *reader).options(con_i_tag);
    if let Some(tipo) = dall_estensione {
        probe = probe.set_file_type(tipo);
    }
    let probe = probe
        .guess_file_type()
        .map_err(|err| inciampo_di_io(path, &err))?;
    let Some(indovinato) = probe.file_type() else {
        // Né il contenuto né l'estensione: è un formato che `lofty` non
        // conosce — il WMA, che sta fra le estensioni della libreria ma non fra
        // quelle che si sanno leggere. Un rifiuto che non cambierà finché non
        // cambia il lettore.
        return Err(AppError::new(ErrorCode::PlaybackFormatUnsupported {
            format: Some(extension_of(path)),
        })
        .with_cause("nessun formato riconosciuto, né dal contenuto né dall'estensione".to_owned())
        .into());
    };
    let primo = match probe.read() {
        Ok(file) => {
            return Ok(Letto {
                file,
                solo_proprieta: false,
            });
        }
        Err(err) => {
            if let Some(io) = guasto_del_disco(&err) {
                return Err(inciampo_di_io(path, io));
            }
            rifiuto(path, &racconta_errore(&err))
        }
    };

    // I ripieghi, in ordine: il tipo dell'estensione se è un altro, poi le sole
    // proprietà con il tipo indovinato e con quello dell'estensione.
    let altro = dall_estensione.filter(|tipo| *tipo != indovinato);
    let mut tentativi: Vec<(FileType, ParseOptions, bool)> = Vec::with_capacity(3);
    if let Some(tipo) = altro {
        tentativi.push((tipo, con_i_tag, false));
    }
    tentativi.push((indovinato, senza_tag, true));
    if let Some(tipo) = altro {
        tentativi.push((tipo, senza_tag, true));
    }
    for (tipo, opzioni, solo_proprieta) in tentativi {
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|err| inciampo_di_io(path, &err))?;
        match Probe::new(&mut *reader)
            .options(opzioni)
            .set_file_type(tipo)
            .read()
        {
            Ok(file) => {
                return Ok(Letto {
                    file,
                    solo_proprieta,
                });
            }
            Err(err) => {
                if let Some(io) = guasto_del_disco(&err) {
                    return Err(inciampo_di_io(path, io));
                }
            }
        }
    }
    Err(primo.into())
}

/// La seconda lettura, dopo il tetto: i tag sì, le proprietà no.
///
/// Con un lettore **nuovo**: il conto dei byte sta nel lettore, e quello di
/// prima l'ha già esaurito. Una volta sola: se anche questa supera il tetto —
/// tag più grandi di otto mebibyte, una copertina enorme in testa — è un
/// guasto del momento (`fs.readFailed`), non un rifiuto del file, e non si
/// ricorda: la passata dopo ci riprova.
fn leggi_senza_proprieta<R: Read + Seek>(reader: &mut R, path: &str) -> Result<Letto, AppError> {
    let opzioni = ParseOptions::new().read_properties(false);
    let mut probe = Probe::new(&mut *reader).options(opzioni);
    if let Some(tipo) = FileType::from_ext(extension_of(path)) {
        probe = probe.set_file_type(tipo);
    }
    let probe = probe
        .guess_file_type()
        .map_err(|err| crate::files::io_error(path, &err))?;
    if probe.file_type().is_none() {
        return Err(AppError::new(ErrorCode::PlaybackFormatUnsupported {
            format: Some(extension_of(path)),
        })
        .with_cause(
            "nessun formato riconosciuto, né dal contenuto né dall'estensione".to_owned(),
        ));
    }
    match probe.read() {
        Ok(file) => Ok(Letto {
            file,
            solo_proprieta: false,
        }),
        Err(err) => match guasto_del_disco(&err) {
            Some(io) => Err(crate::files::io_error(path, io)),
            None => Err(rifiuto(path, &racconta_errore(&err))),
        },
    }
}

/// La versione di Monkey's Audio scritta in testa al file: `MAC ` e poi un
/// `u16` little-endian (3970 per la 3.97). Prima può esserci un tag ID3v2, che
/// si salta.
///
/// `None` se l'intestazione non è quella attesa: allora decide `lofty`, che il
/// file l'ha già letto.
fn versione_ape<R: Read + Seek>(reader: &mut R) -> std::io::Result<Option<u16>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut testa = [0_u8; 10];
    reader.read_exact(&mut testa)?;
    if testa.starts_with(b"ID3") {
        // La dimensione è sincronizzata: sette bit per byte, e i dieci byte
        // dell'intestazione non sono contati.
        let dimensione = testa
            .get(6..10)
            .unwrap_or_default()
            .iter()
            .fold(0_u64, |somma, byte| (somma << 7) | u64::from(byte & 0x7F));
        reader.seek(SeekFrom::Start(dimensione.saturating_add(10)))?;
    } else {
        reader.seek(SeekFrom::Start(0))?;
    }
    let mut mac = [0_u8; 6];
    reader.read_exact(&mut mac)?;
    let [b'M', b'A', b'C', b' ', basso, alto] = mac else {
        return Ok(None);
    };
    Ok(Some(u16::from_le_bytes([basso, alto])))
}

/// [`versione_ape`], con il tetto della sorgente che vale «non si sa».
///
/// Se anche la lettura dell'intestazione tocca il tetto (vedi
/// [`oltre_il_tetto`]) la versione non si sa, e decide `lofty`, che il file
/// l'ha già letto: è la stessa risposta di un'intestazione che non si
/// riconosce.
fn versione_ape_nota<R: Read + Seek>(reader: &mut R, path: &str) -> Result<Option<u16>, AppError> {
    match versione_ape(reader) {
        Ok(versione) => Ok(versione),
        Err(err) if oltre_il_tetto(&err) => Ok(None),
        Err(err) => Err(crate::files::io_error(path, &err)),
    }
}

/// I campi del vecchio ID3v1 in coda al file: gli ultimi 128 byte, se cominciano
/// con `TAG`.
///
/// Serve solo al ripiego delle sole proprietà (vedi [`leggi_il_file`]), dove
/// l'ID3v2 è rotto e `lofty` non lo legge più: lì i trenta caratteri del
/// vecchio tag sono tutto quel che resta di titolo, interprete e album, e un
/// titolo troncato è meglio del nome del file. Nella lettura normale non si usa
/// mai: il tag ricco, quando si legge, vince sempre.
///
/// Il testo è Latin-1, come vuole lo standard: un byte, un carattere.
fn id3v1<R: Read + Seek>(reader: &mut R) -> Option<TrackTags> {
    reader.seek(SeekFrom::End(-128)).ok()?;
    let mut coda = [0_u8; 128];
    reader.read_exact(&mut coda).ok()?;
    if !coda.starts_with(b"TAG") {
        return None;
    }
    let campo = |da: usize, a: usize| -> Option<String> {
        let testo: String = coda
            .get(da..a)?
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| char::from(*byte))
            .collect();
        clean(Some(&testo))
    };
    // ID3v1.1: lo zero al penultimo byte del commento dice che l'ultimo è il
    // numero di traccia.
    let traccia = match (coda.get(125), coda.get(126)) {
        (Some(0), Some(&numero)) if numero > 0 => Some(u32::from(numero)),
        _ => None,
    };
    Some(TrackTags {
        title: campo(3, 33),
        artist: campo(33, 63),
        album: campo(63, 93),
        year: campo(93, 97).and_then(|anno| anno.parse::<i32>().ok()),
        track_number: traccia,
        ..TrackTags::default()
    })
}

/// Legge i tag di un file attraverso il fornitore di file dato.
///
/// Con i ripieghi di [`leggi_il_file`]. Un `.ape` più vecchio della 3.99 è un
/// rifiuto anche se `lofty` lo legge: vedi [`APE_MINIMA`].
///
/// # Errori
///
/// - `metadata.tagReadFailed` se il file non si legge in nessun modo;
/// - `playback.formatUnsupported` per un formato che non si sa leggere o non si
///   sa suonare (il WMA, un APE vecchio);
/// - un errore del disco (`fs.*`) se a non rispondere è il disco e non il file.
///
/// Solo i primi due dicono qualcosa del file, e solo quelli la scansione li
/// ricorda: vedi [`VERSIONE_LETTORE`].
pub fn read_tags(files: &dyn MusicFiles, path: &str) -> Result<TrackTags, AppError> {
    let mut reader = files.open(path)?;
    let Letto {
        file: tagged,
        solo_proprieta,
    } = match leggi_il_file(&mut reader, path) {
        Ok(letto) => letto,
        Err(Inciampo::Errore(errore)) => return Err(errore),
        // Il tetto della sorgente: si rilegge da capo senza le proprietà, con
        // un lettore nuovo. Vedi [`oltre_il_tetto`].
        Err(Inciampo::Tetto) => {
            reader = files.open(path)?;
            leggi_senza_proprieta(&mut reader, path)?
        }
    };

    if tagged.file_type() == FileType::Ape
        && let Some(versione) = versione_ape_nota(&mut reader, path)?
        && versione < APE_MINIMA
    {
        return Err(AppError::new(ErrorCode::PlaybackFormatUnsupported {
            format: Some(format!(
                "Monkey's Audio {}.{:02}",
                versione.div_euclid(1000),
                versione.rem_euclid(1000).div_euclid(10)
            )),
        })
        .with_cause(format!(
            "Monkey's Audio {versione}: il motore suona dalla {APE_MINIMA} in su"
        )));
    }

    let properties = tagged.properties();
    let file_type = format!("{:?}", tagged.file_type());

    let mut tags = TrackTags {
        duration_ms: u64::try_from(properties.duration().as_millis()).unwrap_or(0),
        bitrate: bitrate_del_suono(properties),
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        codec: Some(file_type),
        cover: pick_cover(&tagged),
        ..TrackTags::default()
    };

    if solo_proprieta {
        if let Some(vecchio) = id3v1(&mut reader) {
            tags.title = vecchio.title;
            tags.artist = vecchio.artist;
            tags.album = vecchio.album;
            tags.year = vecchio.year;
            tags.track_number = vecchio.track_number;
        }
        return Ok(tags);
    }

    // Il tag principale quando c'è; altrimenti il primo. Un mp3 può avere sia
    // ID3v2 sia ID3v1: il primario è quello ricco, e ripiegare sul secondo
    // darebbe titoli troncati a 30 caratteri.
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(tags);
    };

    tags.title = clean(tag.title().as_deref());
    tags.artist = clean(tag.artist().as_deref());
    tags.album = clean(tag.album().as_deref());
    tags.genre = clean(tag.genre().as_deref());
    tags.comment = clean(tag.comment().as_deref());
    tags.album_artist = clean(tag.get_string(ItemKey::AlbumArtist));
    // `Lyrics` è il testo non sincronizzato; quello con i tempi (LRC) è un altro
    // mestiere e verrà letto a parte, perché va mostrato scorrendo.
    tags.lyrics = clean(tag.get_string(ItemKey::Lyrics));
    tags.musical_key = clean(tag.get_string(ItemKey::InitialKey));
    // `date()` legge la data di registrazione e, se manca, ripiega da sola sul
    // vecchio campo `Year`: i due convivono nei file taggati in epoche diverse.
    tags.year = tag.date().map(|date| i32::from(date.year));
    tags.track_number = tag.track();
    tags.disc_number = tag.disk();
    tags.bpm = tag
        .get_string(ItemKey::Bpm)
        .and_then(|v| v.trim().parse::<f64>().ok());
    tags.replaygain_track_db = parse_replaygain(tag.get_string(ItemKey::ReplayGainTrackGain));
    tags.replaygain_album_db = parse_replaygain(tag.get_string(ItemKey::ReplayGainAlbumGain));
    tags.mb_recording_id = clean(tag.get_string(ItemKey::MusicBrainzRecordingId));
    tags.mb_release_group_id = clean(tag.get_string(ItemKey::MusicBrainzReleaseGroupId));
    tags.mb_release_id = clean(tag.get_string(ItemKey::MusicBrainzReleaseId));

    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaygain_dalle_sue_scritture() {
        assert_eq!(parse_replaygain(Some("-3.21 dB")), Some(-3.21));
        assert_eq!(parse_replaygain(Some("+1.5")), Some(1.5));
        // Virgola decimale: la scrivono i codificatori che seguono la
        // localizzazione di sistema. Rifiutarla spegnerebbe la normalizzazione
        // del volume proprio su quei file.
        assert_eq!(parse_replaygain(Some("-7,2 dB")), Some(-7.2));
        assert_eq!(parse_replaygain(Some("dB")), None);
        assert_eq!(parse_replaygain(None), None);
    }

    #[test]
    fn un_valore_di_soli_spazi_non_e_un_valore() {
        // Passasse, in libreria comparirebbe un brano senza nome, non cercabile.
        assert_eq!(clean(Some("   ")), None);
        assert_eq!(clean(Some("  Titolo  ")), Some("Titolo".to_owned()));
        assert_eq!(clean(None), None);
    }

    #[test]
    fn un_tag_scomposto_si_ricompone() {
        // «Björk» come lo scrive un taggatore del Mac: una «o» e la dieresi
        // combinante. Esce con la «ö» di un carattere solo, che è quella che
        // l'indice di ricerca sa tenere intera.
        let scomposto = "Bjo\u{0308}rk";
        assert_eq!(clean(Some(scomposto)), Some("Bj\u{00F6}rk".to_owned()));
        // Un testo già composto esce com'è entrato.
        assert_eq!(clean(Some("Sigur Rós")), Some("Sigur Rós".to_owned()));
    }

    #[test]
    fn un_file_che_non_e_audio_da_un_errore_di_dominio() {
        use crate::files::LocalFiles;
        use rymembra_domain::errors::ErrorCodeKind;

        let dir = tempfile::tempdir().expect("cartella");
        let path = dir.path().join("finto.mp3");
        std::fs::write(&path, b"non sono un mp3").expect("scrittura");

        let errore = read_tags(&LocalFiles, &path.to_string_lossy()).expect_err("deve fallire");
        // Non un panico e non una stringa opaca: un codice che la scansione sa
        // trattare come «salta questo file e prosegui».
        assert_eq!(errore.code().kind(), ErrorCodeKind::MetadataTagReadFailed);
        assert!(!errore.is_retryable() || errore.is_retryable());
    }

    /// Quaranta fotogrammi MPEG-1 Layer III, 128 kbit/s a 44,1 kHz: quanto basta
    /// perché `lofty` trovi la sincronia, la confermi sul fotogramma dopo e
    /// calcoli una durata.
    fn fotogrammi_mp3() -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..40 {
            out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
            out.resize(out.len() + 413, 0);
        }
        out
    }

    /// Un tag ID3v2.3 con i frame dati, ognuno `(identificativo, contenuto)`.
    fn id3v23(frame: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut corpo = Vec::new();
        for (id, contenuto) in frame {
            corpo.extend_from_slice(*id);
            let lunghezza = u32::try_from(contenuto.len()).expect("un frame piccolo");
            corpo.extend_from_slice(&lunghezza.to_be_bytes());
            corpo.extend_from_slice(&[0, 0]);
            corpo.extend_from_slice(contenuto);
        }
        let n = u32::try_from(corpo.len()).expect("un tag piccolo");
        let mut out = b"ID3\x03\x00\x00".to_vec();
        for spostamento in [21, 14, 7, 0] {
            out.push(u8::try_from((n >> spostamento) & 0x7F).expect("sette bit"));
        }
        out.extend_from_slice(&corpo);
        out
    }

    /// Il vecchio ID3v1, centoventotto byte in coda.
    fn id3v1_di(titolo: &str, artista: &str) -> Vec<u8> {
        let mut out = b"TAG".to_vec();
        for (testo, lunghezza) in [(titolo, 30), (artista, 30), ("Disco", 30), ("1999", 4)] {
            let mut campo = testo.as_bytes().to_vec();
            campo.resize(lunghezza, 0);
            out.extend_from_slice(&campo);
        }
        // Commento di 28 byte, lo zero dell'ID3v1.1, la traccia, il genere.
        out.resize(125, 0);
        out.extend_from_slice(&[0, 7, 255]);
        out
    }

    #[test]
    fn la_spazzatura_dopo_il_tag_non_nasconde_un_mp3() {
        use crate::files::LocalFiles;

        // Più spazzatura di quella che il riconoscimento dal contenuto scorre
        // (`max_junk_bytes`, 1024), come nei 104 MP3 che il NAS rifiutava.
        let mut byte = id3v23(&[(b"TIT2", b"\x00Titolo vero")]);
        byte.extend(std::iter::repeat_n(0x55_u8, 4096));
        byte.extend(fotogrammi_mp3());
        let dir = tempfile::tempdir().expect("cartella");

        // Senza l'estensione giusta il contenuto da solo non basta: è la prova
        // che il file qui sotto è davvero uno di quelli che si perdevano.
        let senza = dir.path().join("brano.bin");
        std::fs::write(&senza, &byte).expect("scrittura");
        assert!(read_tags(&LocalFiles, &senza.to_string_lossy()).is_err());

        let mp3 = dir.path().join("brano.mp3");
        std::fs::write(&mp3, &byte).expect("scrittura");
        let letti = read_tags(&LocalFiles, &mp3.to_string_lossy()).expect("si legge");
        assert_eq!(letti.title.as_deref(), Some("Titolo vero"));
        assert_eq!(letti.codec.as_deref(), Some("Mpeg"));
        assert!(letti.duration_ms > 0);
    }

    #[test]
    fn un_frame_rotto_lascia_le_proprieta_e_il_vecchio_tag() {
        use crate::files::LocalFiles;

        // `TRCK` dichiarato UTF-16 e senza il segno d'ordine dei byte: è il
        // guasto di 12 MP3 veri, e `lofty` rifiuta l'intero file.
        let mut byte = id3v23(&[
            (b"TIT2", b"\x00Titolo ricco"),
            (b"TRCK", b"\x01\x41\x00\x42\x00"),
        ]);
        byte.extend(fotogrammi_mp3());
        byte.extend(id3v1_di("Titolo povero", "Artista"));
        let dir = tempfile::tempdir().expect("cartella");
        let mp3 = dir.path().join("brano.mp3");
        std::fs::write(&mp3, &byte).expect("scrittura");

        let letti = read_tags(&LocalFiles, &mp3.to_string_lossy()).expect("si legge lo stesso");
        assert!(letti.duration_ms > 0, "le proprietà vengono dal flusso");
        // Il tag ricco è perso con il suo frame rotto: restano i trenta
        // caratteri in coda, che sono meglio del nome del file.
        assert_eq!(letti.title.as_deref(), Some("Titolo povero"));
        assert_eq!(letti.artist.as_deref(), Some("Artista"));
        assert_eq!(letti.album.as_deref(), Some("Disco"));
        assert_eq!(letti.year, Some(1999));
        assert_eq!(letti.track_number, Some(7));
    }

    #[test]
    fn la_versione_di_un_ape_si_legge_in_testa_anche_dopo_un_id3() {
        use std::io::Cursor;

        let mut vecchio = b"MAC ".to_vec();
        vecchio.extend_from_slice(&3970_u16.to_le_bytes());
        vecchio.resize(64, 0);
        assert_eq!(
            versione_ape(&mut Cursor::new(vecchio.clone())).expect("lettura"),
            Some(3970)
        );

        // Lo stesso dietro un tag ID3v2 di dieci byte di corpo.
        let mut con_id3 = b"ID3\x03\x00\x00\x00\x00\x00\x0A".to_vec();
        con_id3.resize(20, 0);
        con_id3.extend_from_slice(&vecchio);
        assert_eq!(
            versione_ape(&mut Cursor::new(con_id3)).expect("lettura"),
            Some(3970)
        );

        // Un'intestazione che non è quella attesa non decide niente.
        assert_eq!(
            versione_ape(&mut Cursor::new(vec![0_u8; 64])).expect("lettura"),
            None
        );
    }

    #[test]
    fn un_formato_che_nessuno_sa_leggere_e_non_supportato() {
        use crate::files::LocalFiles;
        use rymembra_domain::errors::ErrorCodeKind;

        // Un WMA: sta fra le estensioni della libreria, ma `lofty` non lo
        // conosce né dal contenuto né dall'estensione.
        let dir = tempfile::tempdir().expect("cartella");
        let wma = dir.path().join("brano.wma");
        let mut byte = vec![
            0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62,
            0xCE, 0x6C,
        ];
        byte.resize(4096, 0);
        std::fs::write(&wma, &byte).expect("scrittura");
        let errore = read_tags(&LocalFiles, &wma.to_string_lossy()).expect_err("non si legge");
        assert_eq!(
            errore.code().kind(),
            ErrorCodeKind::PlaybackFormatUnsupported
        );
    }

    #[test]
    fn un_errore_del_disco_dentro_lofty_resta_del_disco() {
        // Un `FileParseError` che incarta un errore di I/O vero: il codice deve
        // dire il disco, non il file. Con `metadata.tagReadFailed` la scansione
        // lo ricorderebbe come rifiuto, e il brano sparirebbe per un singhiozzo.
        let rete = std::io::Error::other("la rete se n'è andata");
        let incartato = lofty::error::FileParseError::from(rete);
        assert!(guasto_del_disco(&incartato).is_some());

        // Un file troncato invece è del file.
        let troncato = std::io::Error::from(std::io::ErrorKind::UnexpectedEof);
        let incartato = lofty::error::FileParseError::from(troncato);
        assert!(guasto_del_disco(&incartato).is_none());
    }

    /// Un bitrate impossibile non si crede: si ripiega sul conto del file.
    ///
    /// È il caso MP4 di tutti i giorni. `esds` dichiara cinque kbit/s per un
    /// brano che ne pesa centotrenta, e cinque finiva sotto i comandi come se
    /// qualcuno l'avesse misurato.
    #[test]
    fn un_bitrate_impossibile_lascia_il_posto_a_quello_del_file() {
        let mp4 = lofty::properties::FileProperties::new(
            std::time::Duration::from_secs(249),
            Some(130),
            Some(5),
            Some(44_100),
            None,
            Some(2),
            None,
        );
        assert_eq!(bitrate_del_suono(&mp4), Some(130));
    }

    /// Un bitrate plausibile resta quello del suono, non quello del file.
    ///
    /// La differenza si vede su un MP3 con una copertina grossa: il suono pesa
    /// centonovantadue, il file di più, e il numero da mostrare è il primo.
    #[test]
    fn un_bitrate_plausibile_non_viene_sostituito() {
        let mp3 = lofty::properties::FileProperties::new(
            std::time::Duration::from_secs(255),
            Some(240),
            Some(192),
            Some(44_100),
            None,
            Some(2),
            None,
        );
        assert_eq!(bitrate_del_suono(&mp3), Some(192));
    }

    /// Senza nessuno dei due non si inventa niente.
    ///
    /// `None` vuol dire «non lo so», e la riga dei dati tecnici sa non
    /// scrivere quel pezzo. Uno zero, invece, sarebbe una misura.
    #[test]
    fn senza_numeri_credibili_non_si_dichiara_un_bitrate() {
        let ignoto = lofty::properties::FileProperties::new(
            std::time::Duration::from_secs(10),
            Some(12),
            Some(0),
            None,
            None,
            None,
            None,
        );
        assert_eq!(bitrate_del_suono(&ignoto), None);
    }
}
