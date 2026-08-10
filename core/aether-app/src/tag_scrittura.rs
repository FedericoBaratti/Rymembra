//! Scrivere i tag su un file appena scaricato.
//!
//! [`crate::metadata`] legge; questo scrive. Stessa libreria — `lofty` — e non è
//! un dettaglio: il vecchio albero ne usava due, `music-metadata` per leggere e
//! `node-taglib-sharp` per scrivere, e non concordavano su tutto. La copertina
//! letta dall'una non era sempre quella che l'altra riscriveva, e il disaccordo
//! si vedeva come brani che dopo un'operazione di scrittura cambiavano immagine
//! da soli.
//!
//! # A cosa serve davvero
//!
//! yt-dlp può ricavare dei tag dal titolo del video, ed è quel che farebbe con
//! `--embed-metadata`. Ma «Cesare Cremonini - Poetica (Official Video)» non è un
//! titolo, e «Radiohead - Topic» non è un interprete: sono la trascrizione di
//! come qualcuno ha chiamato un caricamento su YouTube.
//!
//! Spotify quei campi li ha giusti. Questo modulo li scrive **sopra**, ed è il
//! passo che trasforma un file scaricato in un brano di libreria.
//!
//! # `album_artist` è il campo che conta più di tutti
//!
//! È la chiave con cui `rebuild_aggregates` raggruppa un album in **una** uscita.
//! Sbagliarlo — o lasciarlo vuoto — non produce un tag sbagliato: produce un
//! album che in libreria appare spezzato in tante uscite quante sono le tracce
//! con un ospite diverso. Per questo, quando Spotify non lo dà, si ripiega
//! sull'interprete del brano invece di lasciarlo assente.

use aether_domain::SpotifyTrack;
use aether_domain::enrich::Fields;
use aether_domain::errors::{AppError, ErrorCode};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::id3::v2::Id3v2Tag;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{ItemValue, Tag, TagItem, TagType};

/// Quanto può essere grande una copertina da incorporare.
///
/// Otto megabyte: sopra questa soglia non è una copertina, è un'immagine finita
/// lì per sbaglio, e incorporarla vorrebbe dire pagarla su ogni lettura del file
/// per sempre.
const COPERTINA_MASSIMA: usize = 8 * 1024 * 1024;

/// Scrive sul file i tag autorevoli di Spotify.
///
/// `posizione` è il numero di traccia di ripiego, da usare quando Spotify non ne
/// dà uno — si conta da 1.
///
/// La copertina si incorpora solo se `copertina` contiene byte plausibili: un
/// corpo HTML di errore travestito da JPEG non deve finire dentro il file, dove
/// resterebbe per sempre e comparirebbe in griglia come un rettangolo rotto.
///
/// # Errori
///
/// `metadata.tagWriteFailed` se il file non si apre, non si interpreta o non si
/// riscrive.
pub fn scrivi_tag(
    percorso: &std::path::Path,
    brano: &SpotifyTrack,
    posizione: u32,
    copertina: Option<&[u8]>,
) -> Result<(), AppError> {
    let fallito = |dettaglio: String| {
        AppError::new(ErrorCode::MetadataTagWriteFailed {
            path: Some(percorso.display().to_string()),
            detail: Some(dettaglio),
        })
    };

    let mut file = Probe::open(percorso)
        .map_err(|err| fallito(err.to_string()))?
        .read()
        .map_err(|err| fallito(err.to_string()))?;

    // Il tag primario se c'è; altrimenti se ne crea uno del tipo che quel
    // formato usa di suo. Senza il secondo ramo, un m4a appena scaricato — che
    // di tag non ne ha nessuno — non riceverebbe niente e la funzione
    // riporterebbe successo avendo scritto zero campi.
    let tipo = file
        .primary_tag()
        .map_or_else(|| file.file_type().primary_tag_type(), |tag| tag.tag_type());
    let mut tag = file.remove(tipo).unwrap_or_else(|| Tag::new(tipo));

    tag.set_title(brano.title.clone());
    if let Some(artista) = pulito(brano.artist.as_deref()) {
        tag.set_artist(artista);
    }
    if let Some(album) = pulito(brano.album.as_deref()) {
        tag.set_album(album);
    }
    // Mai lasciato vuoto: vedi la nota in testa al modulo.
    if let Some(interprete_album) =
        pulito(brano.album_artist.as_deref()).or_else(|| pulito(brano.artist.as_deref()))
    {
        tag.insert_text(ItemKey::AlbumArtist, interprete_album);
    }
    // L'anno passa da `set_date` e non da un campo `Year`: è la stessa
    // rappresentazione che [`crate::metadata`] rilegge con `tag.date()`, e usare
    // due campi diversi per scrivere e leggere è il difetto che il vecchio
    // albero aveva per aver usato due librerie.
    if let Some(anno) = brano.year.filter(|a| *a > 0)
        && let Ok(anno) = u16::try_from(anno)
    {
        tag.set_date(Timestamp {
            year: anno,
            ..Timestamp::default()
        });
    }
    if let Some(disco) = brano.disc_number.filter(|d| *d > 0) {
        tag.set_disk(disco);
    }
    // Il numero vero di Spotify, e solo in mancanza la posizione nella coda.
    tag.set_track(brano.track_number.filter(|n| *n > 0).unwrap_or(posizione));

    if let Some(byte) = copertina.filter(|b| plausibile(b))
        && let Some(immagine) = immagine(byte)
    {
        tag.push_picture(immagine);
    }

    tag.save_to_path(percorso, WriteOptions::default())
        .map_err(|err| fallito(err.to_string()))
}

/// Scrive **solo** i campi presenti, conservando tutto il resto del tag.
///
/// # La differenza con [`scrivi_tag`], e perché sono due funzioni
///
/// [`scrivi_tag`] scrive su un file appena scaricato, dove i tag di partenza
/// sono quelli che yt-dlp ha ricavato dal titolo del video: sostituirli in
/// blocco è esattamente quel che serve. Qui il file è **dell'utente**, e può
/// contenere anni di lavoro — un testo trascritto a mano, un commento, un
/// `ReplayGain` calcolato da un altro programma. Un `Fields` con nove campi su
/// undici a `None` significa «tocca due cose e lascia stare le altre nove», e
/// questa funzione è il punto in cui quella promessa si mantiene o si rompe.
///
/// Il tag esistente si riprende e si modifica, non si ricostruisce: `remove`
/// restituisce quello che c'era, e si riscrive quello. Costruire un `Tag::new`
/// e riempirlo coi soli campi trovati cancellerebbe tutto il resto — ed è il
/// modo in cui questa funzione andrebbe storta senza che nessun test dei campi
/// scritti se ne accorga.
///
/// # La copertina sostituisce, non si aggiunge
///
/// Un `push_picture` su un tag che ha già una copertina frontale lascia il file
/// con due, e [`crate::metadata::pick_cover`] ne sceglierebbe una delle due
/// senza un criterio stabile. Si toglie prima quella che c'è.
///
/// # Errori
///
/// `metadata.tagWriteFailed` se il file non si apre, non si interpreta o non si
/// riscrive.
pub fn scrivi_campi(
    percorso: &std::path::Path,
    campi: &Fields,
    copertina: Option<&[u8]>,
) -> Result<(), AppError> {
    let fallito = |dettaglio: String| {
        AppError::new(ErrorCode::MetadataTagWriteFailed {
            path: Some(percorso.display().to_string()),
            detail: Some(dettaglio),
        })
    };

    let mut file = Probe::open(percorso)
        .map_err(|err| fallito(err.to_string()))?
        .read()
        .map_err(|err| fallito(err.to_string()))?;

    let tipo = file
        .primary_tag()
        .map_or_else(|| file.file_type().primary_tag_type(), |tag| tag.tag_type());
    let mut tag = file.remove(tipo).unwrap_or_else(|| Tag::new(tipo));

    if let Some(titolo) = pulito(campi.title.as_deref()) {
        tag.set_title(titolo);
    }
    if let Some(artista) = pulito(campi.artist.as_deref()) {
        tag.set_artist(artista);
    }
    if let Some(album) = pulito(campi.album.as_deref()) {
        tag.set_album(album);
    }
    if let Some(interprete) = pulito(campi.album_artist.as_deref()) {
        tag.insert_text(ItemKey::AlbumArtist, interprete);
    }
    if let Some(genere) = pulito(campi.genre.as_deref()) {
        tag.set_genre(genere);
    }
    // Come in `scrivi_tag`: l'anno passa da `set_date`, che è la stessa
    // rappresentazione che `crate::metadata` rilegge con `tag.date()`.
    if let Some(anno) = campi.year.filter(|a| *a > 0)
        && let Ok(anno) = u16::try_from(anno)
    {
        tag.set_date(Timestamp {
            year: anno,
            ..Timestamp::default()
        });
    }
    if let Some(numero) = campi.track_number.filter(|n| *n > 0) {
        tag.set_track(numero);
    }
    if let Some(disco) = campi.disc_number.filter(|d| *d > 0) {
        tag.set_disk(disco);
    }
    for (chiave, valore) in [
        (ItemKey::MusicBrainzRecordingId, &campi.mb_recording_id),
        (ItemKey::MusicBrainzReleaseId, &campi.mb_release_id),
        (
            ItemKey::MusicBrainzReleaseGroupId,
            &campi.mb_release_group_id,
        ),
    ] {
        if let Some(identificativo) = pulito(valore.as_deref()) {
            // `insert_unchecked` e non `insert_text`: la variante controllata
            // chiede alla tabella delle chiavi mappate il permesso di scrivere,
            // e per l'identificativo di registrazione in ID3v2 quel permesso non
            // c'è — restituisce `false` e butta via il valore senza dirlo. La
            // ragione sta in [`salva`], insieme al resto di questa storia.
            tag.insert_unchecked(TagItem::new(chiave, ItemValue::Text(identificativo)));
        }
    }

    if let Some(byte) = copertina.filter(|b| plausibile(b))
        && let Some(immagine) = immagine(byte)
    {
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(immagine);
    }

    salva(percorso, tipo, tag, campi).map_err(|err| fallito(err.to_string()))
}

/// Scrive il tag sul file, aggiungendo a mano ciò che la strada generica perde.
///
/// # Perché non basta `tag.save_to_path`
///
/// I tre identificativi MusicBrainz sono l'unica cosa che l'arricchimento scrive
/// e che non si può riscoprire: sono ciò che dice «questo brano l'abbiamo già
/// riconosciuto, ed è questo». Se non tornano indietro dalla rilettura, ogni
/// passata ritrova gli stessi file da capo, per sempre.
///
/// In ID3v2 nessuno dei tre passa dalla conversione generica di `lofty`, e per
/// due motivi diversi:
///
/// * quello di **registrazione** non è un campo di testo ma un frame `UFID`
///   intestato a MusicBrainz. `lofty` lo sa costruire e lo sa rileggere, ma non
///   lo elenca fra le chiavi mappate — così `Tag::insert_text` lo rifiuta prima
///   ancora di arrivare alla conversione (di qui l'`insert_unchecked` sopra);
/// * quelli di **pubblicazione** e di **gruppo di pubblicazione** sarebbero due
///   `TXXX`, ma non compaiono nell'elenco delle chiavi che la conversione manda
///   in `TXXX` — vi compaiono `MusicBrainzArtistId` e `MusicBrainzWorkId`, non
///   loro. Finiscono nel ramo generico, che pretende un identificativo di frame
///   di quattro caratteri, e «MusicBrainz Album Id» non lo è: vengono scartati.
///
/// In entrambi i casi il valore sparisce **in silenzio**, e sparisce solo negli
/// mp3: sui flac e sugli m4a le stesse chiavi sono mappate e la strada generica
/// funziona. È il genere di guasto che si nota mesi dopo, come «l'arricchimento
/// rifà sempre lo stesso lavoro».
///
/// La rilettura invece li ricompone entrambi, saltando gli stessi controlli. È
/// l'asimmetria fra le due direzioni ad averlo nascosto: il codice scriveva un
/// campo che nessuno rileggeva, e nessuna delle due metà sembrava sbagliata.
fn salva(
    percorso: &std::path::Path,
    tipo: TagType,
    tag: Tag,
    campi: &Fields,
) -> Result<(), lofty::error::LoftyError> {
    if tipo != TagType::Id3v2 {
        return tag.save_to_path(percorso, WriteOptions::default());
    }

    // La conversione porta con sé i frame che `lofty` non sa tradurre — stanno
    // nel «companion tag» del tag generico — quindi non si perde niente di
    // quello che c'era nel file.
    let mut id3 = Id3v2Tag::from(tag);
    // Le descrizioni sono quelle di MusicBrainz Picard, e non è una scelta
    // estetica: un `TXXX` si ritrova per descrizione, e sceglierne una diversa
    // vorrebbe dire depositare l'identificativo dove nessun altro programma — e
    // nemmeno la rilettura di `lofty` — andrebbe a cercarlo.
    for (descrizione, valore) in [
        ("MusicBrainz Album Id", &campi.mb_release_id),
        ("MusicBrainz Release Group Id", &campi.mb_release_group_id),
    ] {
        if let Some(identificativo) = pulito(valore.as_deref()) {
            id3.insert_user_text(descrizione.to_owned(), identificativo);
        }
    }
    id3.save_to_path(percorso, WriteOptions::default())
}

/// Riporta un file ai tag che aveva prima dell'arricchimento.
///
/// # La differenza con [`scrivi_campi`], ed è l'unica che conta
///
/// Qui un `None` **svuota**. È il contrario esatto dell'altra funzione, e va
/// così perché fanno due lavori opposti: [`scrivi_campi`] aggiunge quel che ha
/// scoperto senza toccare il resto, questa deve riportare il file a uno stato
/// preciso — e se prima dell'arricchimento il genere non c'era, riportarcelo
/// vuol dire toglierlo.
///
/// Senza questa asimmetria un annullamento lascerebbe indietro proprio i campi
/// che l'arricchimento aveva **aggiunto**, cioè quelli su cui è più facile che
/// abbia sbagliato: un genere inventato e tre identificativi MusicBrainz di un
/// disco che non è quello.
///
/// Gli identificativi si tolgono sempre. Non stanno in una fotografia perché
/// non possono esserci stati: un brano che ne portava uno nei tag non è mai
/// entrato fra i candidati.
///
/// # Errori
///
/// `metadata.tagWriteFailed` se il file non si apre, non si interpreta o non si
/// riscrive.
pub fn ripristina_campi(percorso: &std::path::Path, originali: &Fields) -> Result<(), AppError> {
    let fallito = |dettaglio: String| {
        AppError::new(ErrorCode::MetadataTagWriteFailed {
            path: Some(percorso.display().to_string()),
            detail: Some(dettaglio),
        })
    };

    let mut file = Probe::open(percorso)
        .map_err(|err| fallito(err.to_string()))?
        .read()
        .map_err(|err| fallito(err.to_string()))?;

    let tipo = file
        .primary_tag()
        .map_or_else(|| file.file_type().primary_tag_type(), |tag| tag.tag_type());
    let mut tag = file.remove(tipo).unwrap_or_else(|| Tag::new(tipo));

    match pulito(originali.title.as_deref()) {
        Some(titolo) => tag.set_title(titolo),
        None => tag.remove_title(),
    }
    match pulito(originali.artist.as_deref()) {
        Some(artista) => tag.set_artist(artista),
        None => tag.remove_artist(),
    }
    match pulito(originali.album.as_deref()) {
        Some(album) => tag.set_album(album),
        None => tag.remove_album(),
    }
    match pulito(originali.album_artist.as_deref()) {
        Some(interprete) => {
            tag.insert_text(ItemKey::AlbumArtist, interprete);
        }
        None => tag.remove_key(ItemKey::AlbumArtist),
    }
    match pulito(originali.genre.as_deref()) {
        Some(genere) => tag.set_genre(genere),
        None => tag.remove_genre(),
    }
    match originali
        .year
        .filter(|a| *a > 0)
        .and_then(|a| u16::try_from(a).ok())
    {
        Some(anno) => tag.set_date(Timestamp {
            year: anno,
            ..Timestamp::default()
        }),
        None => {
            tag.remove_key(ItemKey::RecordingDate);
            tag.remove_key(ItemKey::Year);
        }
    }
    match originali.track_number.filter(|n| *n > 0) {
        Some(numero) => tag.set_track(numero),
        None => tag.remove_track(),
    }
    match originali.disc_number.filter(|d| *d > 0) {
        Some(disco) => tag.set_disk(disco),
        None => tag.remove_disk(),
    }
    for chiave in [
        ItemKey::MusicBrainzRecordingId,
        ItemKey::MusicBrainzReleaseId,
        ItemKey::MusicBrainzReleaseGroupId,
    ] {
        tag.remove_key(chiave);
    }

    tag.save_to_path(percorso, WriteOptions::default())
        .map_err(|err| fallito(err.to_string()))
}

/// Rilegge il file e nomina i campi che non corrispondono a quel che si è scritto.
///
/// # Perché si rilegge, e perché non si fallisce
///
/// Si rilegge perché una scrittura riuscita non è una scrittura avvenuta: un
/// formato che `lofty` sa aprire ma non riscrivere per intero restituisce `Ok`
/// e lascia il file com'era. Senza la rilettura, l'arricchimento direbbe di aver
/// corretto dei brani che alla scansione successiva risultano ancora sbagliati —
/// e il difetto si presenterebbe come «l'arricchimento non funziona», senza un
/// punto a cui ricondurlo.
///
/// Non si fallisce perché su Windows un file in riproduzione è bloccato, e
/// questo è codice che gira in sottofondo mentre l'utente ascolta. Un elenco di
/// campi discordi è un avviso da registrare, non una ragione per fermare una
/// passata.
///
/// Elenco vuoto significa «tutto corrisponde», ed è anche quel che si ottiene da
/// un file illeggibile: non si sa niente, e non si accusa nessuno.
#[must_use]
pub fn rileggi_e_confronta(percorso: &std::path::Path, campi: &Fields) -> Vec<&'static str> {
    let Ok(riletti) =
        crate::metadata::read_tags(&crate::files::LocalFiles, &percorso.display().to_string())
    else {
        return Vec::new();
    };
    let mut discordi = Vec::new();
    let confronta = |atteso: Option<&str>, trovato: Option<&str>| -> bool {
        match pulito(atteso) {
            None => true,
            Some(atteso) => trovato.map(str::trim) == Some(atteso.as_str()),
        }
    };
    if !confronta(campi.title.as_deref(), riletti.title.as_deref()) {
        discordi.push("titolo");
    }
    if !confronta(campi.artist.as_deref(), riletti.artist.as_deref()) {
        discordi.push("interprete");
    }
    if !confronta(campi.album.as_deref(), riletti.album.as_deref()) {
        discordi.push("album");
    }
    if !confronta(campi.genre.as_deref(), riletti.genre.as_deref()) {
        discordi.push("genere");
    }
    if campi.year.is_some() && campi.year != riletti.year {
        discordi.push("anno");
    }
    if campi.track_number.is_some() && campi.track_number != riletti.track_number {
        discordi.push("numero di traccia");
    }
    discordi
}

/// Un valore di tag ripulito: vuoto vale come assente.
fn pulito(valore: Option<&str>) -> Option<String> {
    let ripulito = valore?.trim();
    (!ripulito.is_empty()).then(|| ripulito.to_owned())
}

/// I byte somigliano davvero a un'immagine.
///
/// Stesso cancello di ogni altra sorgente di copertine, e per lo stesso motivo:
/// un CDN che risponde con una pagina d'errore restituisce byte, e senza questo
/// controllo quei byte finiscono incorporati nel file.
fn plausibile(byte: &[u8]) -> bool {
    if byte.len() < 16 || byte.len() > COPERTINA_MASSIMA {
        return false;
    }
    firma(byte).is_some()
}

/// Il tipo dell'immagine, dai suoi primi byte.
fn firma(byte: &[u8]) -> Option<MimeType> {
    // I numeri magici e non l'estensione o l'intestazione HTTP: sono l'unica
    // cosa che descrive i byte che abbiamo davvero in mano.
    if byte.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(MimeType::Jpeg);
    }
    if byte.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(MimeType::Png);
    }
    if byte.starts_with(b"RIFF") && byte.get(8..12) == Some(b"WEBP") {
        // `lofty` non ha una variante per WebP: si dichiara per esteso.
        return Some(MimeType::Unknown("image/webp".to_owned()));
    }
    None
}

/// Costruisce la copertina da incorporare.
///
/// `unchecked` perché il tipo l'abbiamo già riconosciuto noi da [`firma`], che
/// guarda i numeri magici: far ricontrollare i byte a lofty rifiuterebbe i
/// formati che lui non conosce e che il nostro elenco invece accetta.
fn immagine(byte: &[u8]) -> Option<Picture> {
    let mut costruttore = Picture::unchecked(byte.to_vec()).pic_type(PictureType::CoverFront);
    if let Some(tipo) = firma(byte) {
        costruttore = costruttore.mime_type(tipo);
    }
    Some(costruttore.build())
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::files::LocalFiles;
    use crate::metadata::read_tags;

    /// Un brano di Spotify completo di tutto.
    fn brano() -> SpotifyTrack {
        SpotifyTrack {
            title: "Poetica".to_owned(),
            artist: Some("Cesare Cremonini".to_owned()),
            album: Some("Possibili scenari".to_owned()),
            album_artist: Some("Cesare Cremonini".to_owned()),
            track_number: Some(3),
            disc_number: Some(1),
            year: Some(2017),
            duration_ms: Some(297_000),
            ..SpotifyTrack::default()
        }
    }

    /// Un mp3 minimo ma vero, che lofty sa aprire e riscrivere.
    ///
    /// Si costruisce a mano invece di tenerlo come file di prova nel repo: un
    /// binario opaco in `tests/` è una cosa che nessuno può leggere in una
    /// revisione, e qui basta un fotogramma silenzioso.
    fn file_di_prova() -> (tempfile::TempDir, std::path::PathBuf) {
        let cartella = tempfile::tempdir().expect("cartella temporanea");
        let percorso = cartella.path().join("brano.mp3");
        // Tre fotogrammi MPEG-1 Layer III, 128 kbps, 44.1 kHz, stereo: ognuno è
        // l'intestazione di quattro byte più silenzio. Non si sente niente, ed è
        // esattamente quel che serve — qui si provano i tag, non l'audio.
        //
        // **Tre e non uno**: chi legge un mp3 conferma il primo fotogramma
        // trovando il sincronismo di quello dopo, perché `0xFF 0xFB` compare per
        // caso anche in mezzo ai dati. Con un fotogramma solo il file viene
        // rifiutato come «invalid frame».
        //
        // 417 byte: `144 * 128000 / 44100`, troncato, che è la formula del
        // formato.
        const FOTOGRAMMA: usize = 417;
        let mut dati = Vec::with_capacity(FOTOGRAMMA * 3);
        for _ in 0..3 {
            dati.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            dati.resize(dati.len() + FOTOGRAMMA - 4, 0);
        }
        std::fs::write(&percorso, &dati).expect("scrittura del file di prova");
        (cartella, percorso)
    }

    #[test]
    fn i_tag_di_spotify_finiscono_sul_file() {
        let (_cartella, percorso) = file_di_prova();
        scrivi_tag(&percorso, &brano(), 1, None).expect("scrittura dei tag");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert_eq!(letti.title.as_deref(), Some("Poetica"));
        assert_eq!(letti.artist.as_deref(), Some("Cesare Cremonini"));
        assert_eq!(letti.album.as_deref(), Some("Possibili scenari"));
        assert_eq!(letti.album_artist.as_deref(), Some("Cesare Cremonini"));
        assert_eq!(letti.track_number, Some(3));
        assert_eq!(letti.disc_number, Some(1));
        assert_eq!(letti.year, Some(2017));
    }

    #[test]
    fn linterprete_dellalbum_non_resta_mai_vuoto() {
        // È la chiave con cui un album si raggruppa in una sola uscita: vuota,
        // l'album si spezzerebbe in libreria.
        let (_cartella, percorso) = file_di_prova();
        let senza = SpotifyTrack {
            album_artist: None,
            ..brano()
        };
        scrivi_tag(&percorso, &senza, 1, None).expect("scrittura dei tag");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert_eq!(letti.album_artist.as_deref(), Some("Cesare Cremonini"));
    }

    #[test]
    fn senza_numero_di_traccia_vale_la_posizione() {
        let (_cartella, percorso) = file_di_prova();
        let senza = SpotifyTrack {
            track_number: None,
            ..brano()
        };
        scrivi_tag(&percorso, &senza, 7, None).expect("scrittura dei tag");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert_eq!(letti.track_number, Some(7));
    }

    #[test]
    fn una_copertina_vera_si_incorpora() {
        let (_cartella, percorso) = file_di_prova();
        let mut jpeg = vec![0xFF_u8, 0xD8, 0xFF, 0xE0];
        jpeg.resize(64, 0);
        scrivi_tag(&percorso, &brano(), 1, Some(&jpeg)).expect("scrittura dei tag");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        let copertina = letti.cover.expect("la copertina c'è");
        assert_eq!(copertina.mime_type.as_deref(), Some("image/jpeg"));
    }

    #[test]
    fn una_pagina_derrore_non_e_una_copertina() {
        // Il caso vero: il CDN risponde 200 con dell'HTML. Sono byte, e senza il
        // cancello finirebbero incorporati nel file per sempre.
        let (_cartella, percorso) = file_di_prova();
        let html = b"<!DOCTYPE html><html><body>404 Not Found</body></html>";
        scrivi_tag(&percorso, &brano(), 1, Some(html)).expect("scrittura dei tag");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert!(letti.cover.is_none(), "l'HTML non deve entrare nei tag");
        // …e il resto dei tag si è scritto lo stesso: una copertina rifiutata
        // non è una ragione per lasciare il file senza titolo.
        assert_eq!(letti.title.as_deref(), Some("Poetica"));
    }

    #[test]
    fn una_copertina_troncata_non_e_una_copertina() {
        let (_cartella, percorso) = file_di_prova();
        scrivi_tag(&percorso, &brano(), 1, Some(&[0xFF, 0xD8])).expect("scrittura dei tag");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert!(letti.cover.is_none());
    }

    #[test]
    fn un_file_che_non_esiste_da_un_errore_di_dominio() {
        let errore = scrivi_tag(
            std::path::Path::new("C:/non/esiste/proprio.m4a"),
            &brano(),
            1,
            None,
        )
        .expect_err("un file inesistente non si tagga");
        assert!(matches!(
            errore.code(),
            ErrorCode::MetadataTagWriteFailed { .. }
        ));
    }

    // ── scrittura selettiva ──

    #[test]
    fn si_scrive_solo_quel_che_e_presente_e_il_resto_resta() {
        // Il difetto che questa prova impedisce: costruire un tag nuovo invece
        // di modificare quello che c'è. I campi scritti sarebbero giusti lo
        // stesso, e l'utente perderebbe il testo trascritto a mano e il
        // commento senza che nessuna asserzione sui campi scritti se ne accorga.
        let (_cartella, percorso) = file_di_prova();
        scrivi_tag(&percorso, &brano(), 1, None).expect("tag di partenza");

        let campi = Fields {
            album: Some("Possibili scenari (Deluxe)".to_owned()),
            year: Some(2018),
            ..Fields::default()
        };
        scrivi_campi(&percorso, &campi, None).expect("scrittura selettiva");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert_eq!(letti.album.as_deref(), Some("Possibili scenari (Deluxe)"));
        assert_eq!(letti.year, Some(2018));
        // …e tutto il resto è ancora quello di prima.
        assert_eq!(letti.title.as_deref(), Some("Poetica"));
        assert_eq!(letti.artist.as_deref(), Some("Cesare Cremonini"));
        assert_eq!(letti.album_artist.as_deref(), Some("Cesare Cremonini"));
        assert_eq!(letti.track_number, Some(3));
        assert_eq!(letti.disc_number, Some(1));
    }

    #[test]
    fn gli_identificativi_musicbrainz_fanno_andata_e_ritorno() {
        // Sono il motivo per cui l'arricchimento mette in ordine la libreria
        // invece di limitarsi a correggere dei testi: `build_album_groups` li
        // usa per fondere due cartelle che sono lo stesso disco.
        let (_cartella, percorso) = file_di_prova();
        let campi = Fields {
            mb_recording_id: Some("rec-1".to_owned()),
            mb_release_id: Some("rel-1".to_owned()),
            mb_release_group_id: Some("grp-1".to_owned()),
            ..Fields::default()
        };
        scrivi_campi(&percorso, &campi, None).expect("scrittura");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert_eq!(letti.mb_recording_id.as_deref(), Some("rec-1"));
        assert_eq!(letti.mb_release_id.as_deref(), Some("rel-1"));
        assert_eq!(letti.mb_release_group_id.as_deref(), Some("grp-1"));
    }

    #[test]
    fn una_copertina_nuova_sostituisce_quella_che_ce_ra() {
        // Con `push_picture` e basta il file resterebbe con due copertine
        // frontali, e quale delle due si vede non sarebbe più deciso da niente.
        let (_cartella, percorso) = file_di_prova();
        let mut prima = vec![0xFF_u8, 0xD8, 0xFF, 0xE0];
        prima.resize(64, 1);
        scrivi_tag(&percorso, &brano(), 1, Some(&prima)).expect("prima copertina");

        let mut dopo = vec![0xFF_u8, 0xD8, 0xFF, 0xE0];
        dopo.resize(96, 2);
        scrivi_campi(&percorso, &Fields::default(), Some(&dopo)).expect("seconda copertina");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        let copertina = letti.cover.expect("la copertina c'è");
        assert_eq!(copertina.data.len(), 96, "deve essere la seconda");
    }

    #[test]
    fn un_campo_nullo_non_svuota_niente() {
        let (_cartella, percorso) = file_di_prova();
        scrivi_tag(&percorso, &brano(), 1, None).expect("tag di partenza");
        scrivi_campi(&percorso, &Fields::default(), None).expect("scrittura a vuoto");

        let letti = read_tags(&LocalFiles, &percorso.display().to_string()).expect("rilettura");
        assert_eq!(letti.title.as_deref(), Some("Poetica"));
        assert_eq!(letti.album.as_deref(), Some("Possibili scenari"));
    }

    #[test]
    fn la_rilettura_nomina_i_campi_discordi() {
        let (_cartella, percorso) = file_di_prova();
        let campi = Fields {
            title: Some("Poetica".to_owned()),
            year: Some(2017),
            ..Fields::default()
        };
        scrivi_campi(&percorso, &campi, None).expect("scrittura");
        assert!(
            rileggi_e_confronta(&percorso, &campi).is_empty(),
            "quel che si è scritto si deve rileggere"
        );

        // Un campo che nessuno ha scritto: la rilettura lo trova diverso.
        let mai_scritti = Fields {
            title: Some("Un altro titolo".to_owned()),
            ..Fields::default()
        };
        assert_eq!(rileggi_e_confronta(&percorso, &mai_scritti), vec!["titolo"]);
    }

    #[test]
    fn un_file_illeggibile_non_accusa_nessuno() {
        // Su Windows un file in riproduzione è bloccato, e questo codice gira
        // mentre l'utente ascolta: «non lo so» non deve diventare «è sbagliato».
        let campi = Fields {
            title: Some("Poetica".to_owned()),
            ..Fields::default()
        };
        assert!(
            rileggi_e_confronta(std::path::Path::new("C:/non/esiste/proprio.mp3"), &campi)
                .is_empty()
        );
    }

    #[test]
    fn le_firme_riconoscono_i_formati_veri() {
        assert!(plausibile(
            &[&[0xFF, 0xD8, 0xFF][..], &[0_u8; 32][..]].concat()
        ));
        assert!(plausibile(
            &[&b"\x89PNG\r\n\x1a\n"[..], &[0_u8; 32][..]].concat()
        ));
        assert!(!plausibile(b"GIF89a e poi chissa'"));
        assert!(!plausibile(&[]));
    }
}
