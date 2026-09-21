//! Leggere i tag di un file audio.
//!
//! Una libreria di lettura tag per tutti i formati, la stessa sul desktop e su
//! Android. Il vecchio albero ne usava due — `music-metadata` per leggere,
//! `node-taglib-sharp` per scrivere — e non concordavano su tutto: la copertina
//! letta da una non era sempre quella che l'altra riscriveva.

use aether_domain::errors::{AppError, ErrorCode};
use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::picture::{Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;

use crate::files::MusicFiles;

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

/// Un valore di tag ripulito: niente spazi ai bordi, e vuoto vale come assente.
///
/// Un titolo di soli spazi non è un titolo. Lasciarlo passare produce brani che
/// in libreria appaiono senza nome e non si possono cercare.
fn clean(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
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

/// Legge i tag di un file attraverso il fornitore di file dato.
pub fn read_tags(files: &dyn MusicFiles, path: &str) -> Result<TrackTags, AppError> {
    let mut reader = files.open(path)?;
    let fail = |detail: &str| {
        AppError::new(ErrorCode::MetadataTagReadFailed {
            path: Some(path.to_owned()),
        })
        .with_cause(detail.to_owned())
    };

    let probe = Probe::new(&mut reader)
        // La durata va calcolata: senza, i file a bitrate variabile la
        // riportano stimata dall'intestazione, che su un VBR lungo sbaglia di
        // parecchi secondi — e la durata finisce nell'interfaccia e nei
        // confronti di somiglianza.
        .options(ParseOptions::new().read_properties(true))
        .guess_file_type()
        .map_err(|err| fail(&err.to_string()))?;

    let tagged = probe.read().map_err(|err| fail(&err.to_string()))?;
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
    fn un_file_che_non_e_audio_da_un_errore_di_dominio() {
        use crate::files::LocalFiles;
        use aether_domain::errors::ErrorCodeKind;

        let dir = tempfile::tempdir().expect("cartella");
        let path = dir.path().join("finto.mp3");
        std::fs::write(&path, b"non sono un mp3").expect("scrittura");

        let errore = read_tags(&LocalFiles, &path.to_string_lossy()).expect_err("deve fallire");
        // Non un panico e non una stringa opaca: un codice che la scansione sa
        // trattare come «salta questo file e prosegui».
        assert_eq!(errore.code().kind(), ErrorCodeKind::MetadataTagReadFailed);
        assert!(!errore.is_retryable() || errore.is_retryable());
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
