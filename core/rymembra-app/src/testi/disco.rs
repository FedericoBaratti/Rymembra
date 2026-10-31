//! Le fonti sul disco: il sidecar accanto al brano e i tag sincronizzati dentro il file.
//!
//! Un pezzo di `crate::testi`: dove un commento dice «questo modulo», intende `testi` intero.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::testo::{self, Testo};

// ── il sidecar ──────────────────────────────────────────────────────────────

/// La chiave in `settings` dell'interruttore dei `.lrc` accanto alla musica.
pub const CHIAVE_ACCANTO: &str = "testi.accanto";

/// Un testo sincronizzato a mano si scrive **anche** in un `.lrc` accanto al brano.
///
/// # Spento di serie, e perché
///
/// Fino alla 2.4 il salvataggio scriveva sempre, accanto al file musicale, il
/// `.lrc`, a volte l'`.a2.lrc`, e al primo giro un `.lrc.bak` con quel che
/// c'era prima: tre file nella cartella della musica dell'utente che nessuno
/// aveva chiesto. Chi ha la libreria su un NAS condiviso con altri lettori li
/// vuole, e chi l'ha sincronizzata con un telefono se li ritrova ovunque. La
/// regola della 3.0 è che i dati di Rymembra stanno nel database di Rymembra: la
/// riga di `lyrics` è la fonte, e un file accanto al brano si scrive solo se lo
/// si chiede qui.
///
/// Leggerli non cambia: un `.lrc` che c'è — scritto da un altro programma, o da
/// una versione passata — resta la prima fonte della catena.
///
/// Un valore illeggibile vale spento: è il valore di serie, e un interruttore
/// che non si legge non deve mettersi a scrivere file.
#[must_use]
pub fn accanto_ai_file(connection: &Connection) -> bool {
    crate::settings::read(connection, CHIAVE_ACCANTO)
        .ok()
        .flatten()
        .is_some_and(|valore| valore == "1")
}

/// I nomi che un sidecar può avere, dal più ricco al più comune.
///
/// `.a2.lrc` porta i tempi delle singole parole; `.lrc` quelli delle righe.
/// Guardare prima il primo costa una `stat` in più su ogni brano che non ce
/// l'ha, cioè quasi tutti — ma la spesa è una chiamata al filesystem, e
/// l'alternativa è non accorgersi mai del formato migliore.
const CODE: [&str; 2] = ["a2.lrc", "lrc"];

/// Il percorso di un sidecar per questo brano.
#[must_use]
pub fn percorso_sidecar(brano: &Path, coda: &str) -> PathBuf {
    brano.with_extension(coda)
}

/// Il contenuto del primo sidecar che esiste, se ne esiste uno.
///
/// # Le codifiche che non sono UTF-8
///
/// Un `.lrc` scaricato dieci anni fa può essere in windows-1252 o in Shift-JIS,
/// e non c'è niente nel file che lo dica. Si leggeva in modo tollerante, cioè
/// con un segno di sostituzione al posto di ogni «è» — e un testo giapponese
/// intero fatto di segni di sostituzione. La scusa scritta qui era che
/// riconoscere la codifica fosse una dipendenza in più: non lo è, `encoding_rs`
/// sta già nel dominio per riparare i tag. La scelta la fa
/// [`rymembra_domain::codifica::leggi_byte`], che dice anche come.
#[must_use]
pub fn leggi_sidecar(brano: &Path) -> Option<String> {
    leggi_sidecar_con_data(brano).map(|(grezzo, _)| grezzo)
}

/// Come [`leggi_sidecar`], con l'istante dell'ultima modifica del file in
/// millisecondi, quando il filesystem lo dice.
pub(super) fn leggi_sidecar_con_data(brano: &Path) -> Option<(String, Option<i64>)> {
    for coda in CODE {
        let percorso = percorso_sidecar(brano, coda);
        let Ok(byte) = std::fs::read(&percorso) else {
            continue;
        };
        let (grezzo, _codifica) = rymembra_domain::codifica::leggi_byte(&byte);
        if !grezzo.trim().is_empty() {
            let modificato = std::fs::metadata(&percorso)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|quando| quando.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|durata| i64::try_from(durata.as_millis()).ok());
            return Some((grezzo, modificato));
        }
    }
    None
}

/// Scrive — o toglie, con `None` — un sidecar accanto al brano, senza perdere
/// quel che c'era.
///
/// # Il `.bak`
///
/// Un `.lrc` accanto al brano può non essere nostro: l'ha scaricato qualcuno,
/// l'ha scritto un altro lettore, l'ha sincronizzato l'utente anni fa con un
/// programma che non c'è più. Prima lo si sovrascriveva, e l'`.a2.lrc` si
/// cancellava, senza chiedere e senza copia. Adesso, la prima volta che Rymembra
/// sostituisce o toglie un sidecar che ha un contenuto diverso, lo sposta in
/// `<nome>.<coda>.bak`. Solo la prima: un `.bak` che c'è già è l'originale, e
/// le scritture dopo sono le nostre.
///
/// # Errori
///
/// `fs.writeFailed` col percorso, per la scrittura, lo spostamento o la
/// rimozione — e per la **lettura** del sidecar di prima, quando c'è e non si
/// lascia leggere: senza il suo contenuto non si sa se meriti un `.bak`, e
/// allora non si tocca. Un sidecar che non c'era, da togliere, non è un guasto.
pub fn scrivi_sidecar(brano: &Path, coda: &str, contenuto: Option<&str>) -> Result<(), AppError> {
    // Accanto a un brano del cloud non si scrive: Rymembra ha solo il
    // permesso di leggere quel servizio, e il testo resta comunque nella
    // libreria (la tabella `lyrics` è legata al brano, non al file). Un
    // salto e non un errore, per la stessa ragione per cui un brano di
    // catalogo non ha sidecar: non c'è un «accanto» dove scrivere.
    if rymembra_domain::nuvola::e_nuvola(&brano.to_string_lossy()) {
        return Ok(());
    }
    let percorso = percorso_sidecar(brano, coda);
    let copia = percorso_sidecar(brano, &format!("{coda}.bak"));
    let guaio = |dove: &Path, err: std::io::Error| {
        AppError::new(ErrorCode::FsWriteFailed {
            path: dove.display().to_string(),
            detail: None,
        })
        .with_cause(err.to_string())
    };
    // «Non c'è» è solo `NotFound`. Ogni altro errore di lettura — un altro
    // programma che lo tiene aperto senza condividerne la lettura, una share
    // che risponde a metà — vuol dire che il file c'è e non si sa cosa
    // contenga: valeva «niente da mettere da parte», e la scrittura o la
    // rimozione che seguono lo portavano via senza `.bak`. Meglio non toccarlo
    // e dirlo.
    let di_prima = match std::fs::read(&percorso) {
        Ok(byte) => Some(byte),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(guaio(&percorso, err)),
    };
    let da_mettere_da_parte = di_prima
        .as_deref()
        .is_some_and(|c| contenuto.is_none_or(|nuovo| c != nuovo.as_bytes()))
        && !copia.exists();
    if da_mettere_da_parte {
        std::fs::rename(&percorso, &copia).map_err(|err| guaio(&copia, err))?;
    }
    match contenuto {
        Some(nuovo) => std::fs::write(&percorso, nuovo).map_err(|err| guaio(&percorso, err)),
        None => match std::fs::remove_file(&percorso) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(guaio(&percorso, err)),
        },
    }
}

// ── i tag sincronizzati ─────────────────────────────────────────────────────

/// Il nome sotto cui Vorbis e APE tengono un LRC.
///
/// Non è uno standard: è la convenzione che hanno preso i taggatori — Mp3tag,
/// MusicBee, foobar2000 con il suo componente — e che i lettori che i tempi li
/// mostrano vanno a cercare. `UNSYNCEDLYRICS` è l'altro, ed è il testo piatto:
/// quello lo legge già la scansione, mappato su `lofty::prelude::ItemKey`.
pub(super) const CHIAVE_SINCRONIZZATA: &str = "SYNCEDLYRICS";

/// L'LRC che sta dentro il file musicale, se ce n'è uno.
///
/// # Perché non lo legge la scansione
///
/// Perché la scansione tocca decine di migliaia di file, e leggere questi
/// frammenti costa un'apertura in più per file — su una libreria vera sono
/// minuti, spesi su un tag che quasi nessuno scrive. Qui invece si apre **un**
/// file, quello che si sta guardando, e quel che si trova finisce in tabella:
/// l'apertura si paga una volta per brano, non una per scansione.
///
/// # Le due forme, e perché servono due strade
///
/// * **`SYLT`** è un frame ID3v2 binario, e in `lofty` — la 0.25 come la
///   0.24 — **non entra nel `Tag` unificato**: arriva come `Frame::Binary` e va
///   interpretato a mano con `SynchronizedTextFrame::parse`. Vive solo dove
///   vive ID3v2, cioè mp3, aiff e wav (più, fuori standard, flac e mpc).
/// * **`SYNCEDLYRICS`** è una chiave di testo qualunque dentro Vorbis o APE, e
///   nemmeno lei attraversa il `Tag` unificato: `ItemKey` è un'enumerazione
///   chiusa e una chiave che non le corrisponde non ci finisce. Si legge dal
///   tag concreto, che è l'unico che le chiavi le tiene tutte.
///
/// Da qui la forma di questa funzione: si apre il file **come quel che è**
/// invece di passare da `Probe::read`, perché quel che serve sta esattamente
/// nei pezzi che la lettura generica butta via.
///
/// Non fallisce mai: un file che non si apre, un frame illeggibile, un formato
/// che questi tag non li ha, sono tutti «niente da qui», che è una fonte in
/// meno e non un errore. È la stessa disciplina di [`leggi_sidecar`].
///
/// # Dove non va chiamata
///
/// Apre un file che può stare su una condivisione di rete, quindi vale la
/// regola di `crate::testi`: questa catena non sta sul filo che disegna la
/// finestra. Non allarga però la superficie del problema, e vale la pena
/// dirlo: chi arriva fin qui su una share morta si è già fermato una riga più
/// su, dentro [`leggi_sidecar`], che apre un file nella stessa cartella un
/// istante prima.
#[must_use]
pub fn tag_sincronizzato(percorso: &Path) -> Option<String> {
    use lofty::config::ParseOptions;
    use lofty::file::{AudioFile as _, FileType};

    let file = std::fs::File::open(percorso).ok()?;
    let mut lettore = std::io::BufReader::new(file);
    // Il tipo dal contenuto e non dall'estensione: un `.mp3` che dentro è un
    // flac esiste, e sbagliare qui vorrebbe dire non trovare niente su un file
    // che il testo ce l'ha.
    //
    // La sonda sta in un blocco suo perché tiene il prestito del lettore, e il
    // lettore serve subito dopo per la lettura vera.
    let tipo = {
        let sonda = lofty::probe::Probe::new(&mut lettore)
            .guess_file_type()
            .ok()?;
        sonda.file_type()?
    };
    std::io::Seek::rewind(&mut lettore).ok()?;

    // Le proprietà audio non si leggono: sono la parte cara — su un VBR lungo
    // vuol dire percorrere il file intero — e qui non servono a niente.
    let opzioni = ParseOptions::new().read_properties(false);
    match tipo {
        FileType::Mpeg => {
            let letto = lofty::mpeg::MpegFile::read_from(&mut lettore, opzioni).ok()?;
            da_id3v2(letto.id3v2()).or_else(|| da_ape(letto.ape()))
        }
        FileType::Aiff => {
            let letto = lofty::iff::aiff::AiffFile::read_from(&mut lettore, opzioni).ok()?;
            da_id3v2(letto.id3v2())
        }
        FileType::Wav => {
            let letto = lofty::iff::wav::WavFile::read_from(&mut lettore, opzioni).ok()?;
            da_id3v2(letto.id3v2())
        }
        FileType::Flac => {
            let letto = lofty::flac::FlacFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(letto.vorbis_comments()).or_else(|| da_id3v2(letto.id3v2()))
        }
        FileType::Vorbis => {
            let letto = lofty::ogg::VorbisFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(Some(letto.vorbis_comments()))
        }
        FileType::Opus => {
            let letto = lofty::ogg::OpusFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(Some(letto.vorbis_comments()))
        }
        FileType::Speex => {
            let letto = lofty::ogg::SpeexFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(Some(letto.vorbis_comments()))
        }
        FileType::Ape => {
            let letto = lofty::ape::ApeFile::read_from(&mut lettore, opzioni).ok()?;
            da_ape(letto.ape())
        }
        FileType::WavPack => {
            let letto = lofty::wavpack::WavPackFile::read_from(&mut lettore, opzioni).ok()?;
            da_ape(letto.ape())
        }
        FileType::Mpc => {
            let letto = lofty::musepack::MpcFile::read_from(&mut lettore, opzioni).ok()?;
            da_ape(letto.ape()).or_else(|| da_id3v2(letto.id3v2()))
        }
        // Mp4 e aac non hanno un posto concordato dove mettere un LRC: `©lyr`
        // è il testo piatto, e la scansione lo legge già. Inventarne uno qui
        // vorrebbe dire cercare una chiave che nessun taggatore scrive.
        _ => None,
    }
}

/// L'LRC dal frame `SYLT` di un tag ID3v2.
fn da_id3v2(tag: Option<&lofty::id3::v2::Id3v2Tag>) -> Option<String> {
    use lofty::id3::v2::{Frame, FrameFlags, FrameId, SynchronizedTextFrame};

    let frame = tag?.get(&FrameId::Valid(std::borrow::Cow::Borrowed("SYLT")))?;
    let Frame::Binary(binario) = frame else {
        return None;
    };
    let letto = SynchronizedTextFrame::parse(&binario.data, FrameFlags::default()).ok()?;
    lrc_da_sylt(&letto)
}

/// Il testo di `SYNCEDLYRICS` in un tag Vorbis.
pub(super) fn da_vorbis(tag: Option<&lofty::ogg::tag::VorbisComments>) -> Option<String> {
    tag?.get(CHIAVE_SINCRONIZZATA)
        .filter(|t| !t.trim().is_empty())
        .map(ToOwned::to_owned)
}

/// Il testo di `SYNCEDLYRICS` in un tag APE.
fn da_ape(tag: Option<&lofty::ape::ApeTag>) -> Option<String> {
    match tag?.get(CHIAVE_SINCRONIZZATA)?.value() {
        lofty::tag::ItemValue::Text(testo) if !testo.trim().is_empty() => Some(testo.clone()),
        _ => None,
    }
}

/// Un `SYLT` riscritto come LRC.
///
/// # I due modi in cui un `SYLT` è scritto, e come si distinguono
///
/// Lo standard ID3v2 dice che il testo è **sillabato**: ogni voce è un
/// frammento, e una nuova riga comincia dove il frammento porta un a capo in
/// testa. Quasi nessuno lo scrive così: i taggatori veri mettono una riga
/// intera per voce, e l'a capo non lo scrivono affatto.
///
/// Trattarli allo stesso modo rompe uno dei due — o si ottiene una canzone su
/// una riga sola, o duecento righe di due sillabe. Quindi si guarda: se
/// **almeno una** voce comincia con un a capo, il file è sillabato e si
/// ricompongono le righe; se nessuna lo fa, ogni voce **è** una riga. Non è
/// un'euristica su cosa il testo sembri: è la sola cosa che il formato lascia
/// dire a chi scrive.
///
/// I tempi in fotogrammi MPEG si scartano: convertirli vorrebbe sapere la
/// durata del fotogramma, cioè aver già decodificato il file, e sbagliarla
/// darebbe un testo che scorre a una velocità inventata. Meglio nessun testo
/// che un testo storto, che è la regola di tutto questo modulo.
pub(super) fn lrc_da_sylt(frame: &lofty::id3::v2::SynchronizedTextFrame<'_>) -> Option<String> {
    use lofty::id3::v2::{SyncTextContentType, TimestampFormat};

    if frame.timestamp_format != TimestampFormat::MS {
        return None;
    }
    // Un `SYLT` può portare accordi, titoli di movimento o didascalie di scena:
    // sono tutte cose sincronizzate, e nessuna è il testo della canzone.
    if !matches!(
        frame.content_type,
        SyncTextContentType::Lyrics | SyncTextContentType::TextTranscription
    ) {
        return None;
    }

    let sillabato = frame
        .content
        .iter()
        .any(|(_, pezzo)| pezzo.starts_with('\n') || pezzo.starts_with('\r'));

    let mut righe: Vec<testo::Riga> = Vec::new();
    for (ms, pezzo) in &frame.content {
        let pulito = pezzo.trim_matches(['\n', '\r']);
        let apre = !sillabato || pezzo.starts_with('\n') || pezzo.starts_with('\r');
        match righe.last_mut() {
            Some(ultima) if !apre => ultima.testo.push_str(pulito),
            _ => righe.push(testo::Riga {
                ms: *ms,
                testo: pulito.to_owned(),
                parole: Vec::new(),
                secondaria: None,
            }),
        }
    }
    // Le righe vuote in coda e in testa non dicono niente, e una raccolta di
    // sole righe vuote non è un testo.
    if righe.iter().all(|r| r.testo.trim().is_empty()) {
        return None;
    }

    // Si ricompone passando da `scrivi`, che è l'inverso provato di `leggi`:
    // così questo tag entra nel resto del programma sotto la stessa forma di un
    // `.lrc` sul disco, e non c'è un secondo formato interno da mantenere.
    Some(testo::scrivi(&Testo {
        righe,
        piatto: None,
        strumentale: false,
        offset_ms: 0,
    }))
}
