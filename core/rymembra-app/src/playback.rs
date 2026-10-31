//! Quel che la riproduzione lascia sul database.
//!
//! Il motore in `rymembra-play` non sa cos'è una libreria: riceve dei byte e li fa
//! uscire dalle casse. Questo modulo è il ponte fra le due cose — trova i byte
//! di un brano, e scrive quel che è successo dopo che è stato ascoltato.
//!
//! # Le tre scritture, e perché stanno in una transazione sola
//!
//! Un ascolto finito tocca `play_count`, `last_played_at` e `play_history`. Sono
//! tre fatti sullo stesso evento, e mezzo evento è peggio di nessun evento: una
//! riga di cronologia senza il conteggio corrispondente rende i due numeri
//! discordi per sempre, e non c'è nessun posto da cui riconciliarli — la
//! cronologia d'ascolto è, con voti e preferiti, l'unica cosa in tutta la
//! libreria che una scansione non può ricostruire.

use rusqlite::Connection;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::listen::Listen;
use rymembra_domain::queue::{QueueSnapshot, RepeatMode};

use crate::library::db_error;

mod impostazioni;
mod sorgente;

pub use impostazioni::{
    BERSAGLIO_ALTO_DB, BERSAGLIO_BASSO_DB, BERSAGLIO_PREDEFINITO_DB, CHIAVE_AUTOPLAY,
    CHIAVE_CROSSFADE, CHIAVE_LATENZA, CHIAVE_LATENZA_PER_USCITA, CHIAVE_USCITA,
    CROSSFADE_MASSIMO_S, Equalizzazione, FonteLatenza, LATENZA_MASSIMA_MS, LatenzaUscita,
    LatenzeLette, LatenzePerUscita, Normalizzazione, PresetEq, Qualita, Volume, cancella_preset,
    dichiara_latenza, latenza_effettiva, load_autoplay, load_crossfade, load_eq,
    load_formato_visibile, load_latenza, load_latenze_per_uscita, load_preset_eq, load_replaygain,
    load_spettro_bande, load_spettro_qualita, load_spettro_visibile, load_uscita, load_volume,
    normalizza, salva_preset, save_autoplay, save_crossfade, save_eq, save_formato_visibile,
    save_latenza, save_latenza_per_uscita, save_preset_eq, save_replaygain, save_spettro_bande,
    save_spettro_qualita, save_spettro_visibile, save_uscita, save_volume,
};
pub(crate) use sorgente::collocazione_su_disco;
pub use sorgente::{
    Collocazione, Intorno, ModoLivello, SchedaSorgente, scheda_da_suonare, scheda_sorgente,
    sorgente, sorgente_con, sorgente_da_scheda,
};

/// La chiave con cui la coda sta in `settings`.
pub const CHIAVE_CODA: &str = "player.queue";

/// La chiave con cui volume e silenziamento stanno in `settings`.
pub const CHIAVE_VOLUME: &str = "player.volume";

/// La chiave con cui la curva dell'equalizzatore sta in `settings`.
pub const CHIAVE_EQ: &str = "player.eq";

/// La chiave con cui stanno le curve salvate dall'utente.
///
/// Separata da [`CHIAVE_EQ`] perché sono due cose con due vite diverse: la curva
/// corrente cambia a ogni trascinamento di un cursore, l'elenco dei preset una
/// volta ogni tanto. Tenerle nello stesso valore vorrebbe dire riscrivere
/// l'elenco intero sedici volte al secondo.
pub const CHIAVE_EQ_PRESET: &str = "player.eq.presets";

/// La chiave con cui la normalizzazione ReplayGain sta in `settings`.
pub const CHIAVE_REPLAYGAIN: &str = "player.replaygain";

/// La chiave con cui sta quante barre disegna lo spettro.
///
/// In `settings` e non in `localStorage`, come tutto il resto delle preferenze
/// da quando il tema si è spostato qui: `localStorage` non sopravvive a una
/// reinstallazione, e una scelta che sparisce cambiando computer è una scelta
/// che va rifatta ogni volta.
///
/// Qui c'era scritto che le preferenze «viaggiano con il backup e con la
/// sincronia»: non era vero per questa chiave, e non lo è per quasi nessuna. Il
/// backup su Drive copia **due** righe di `settings` (le cartelle sorvegliate e
/// la skin attiva) e la sincronia **tre**. L'unico meccanismo che porta una
/// preferenza da un computer a un altro è il [profilo](crate::profilo), che è
/// un elenco di inclusioni — e fino a oggi questa chiave in quell'elenco non
/// c'era. Adesso c'è, insieme a [`CHIAVE_SPETTRO_VISIBILE`].
pub const CHIAVE_SPETTRO_BANDE: &str = "player.spectrum.bands";

/// La chiave con cui sta se la scena dello spettro parte accesa.
///
/// Di serie **spenta** — chi apre «In riproduzione» è venuto a guardare la
/// copertina — ma la scelta si ricorda, e viaggia nel profilo: «voglio vedere
/// lo spettro» è un gusto di chi ascolta, non un fatto di questa macchina, e
/// resta vero su qualunque computer.
pub const CHIAVE_SPETTRO_VISIBILE: &str = "player.spectrum.visible";

/// La chiave con cui sta il tetto di qualità della scena dello spettro.
///
/// **Non** viaggia nel profilo, ed è deliberato: è la terza omissione di
/// `crate::profilo`, dopo la coda e l'uscita audio, e per la stessa ragione —
/// «alta» descrive quel che *questa* scheda video regge. La prova che tiene
/// deliberata l'omissione sta in fondo a quel file.
pub const CHIAVE_SPETTRO_QUALITA: &str = "player.spectrum.quality";

/// La chiave con cui sta se i dati tecnici del file si mostrano.
///
/// Di serie **accesa**, al contrario dello spettro: quella riga non costa né
/// una GPU né un filo — è una query su una riga sola, fatta quando il brano
/// cambia — e chi apre «In riproduzione» ha il diritto di sapere cosa sta
/// sentendo senza prima scoprire che esiste un interruttore. Assente vuol dire
/// accesa, come per `aggiornamenti.attivo`: scrivere `true` al primo avvio
/// vorrebbe dire una riga in `settings` per non dire niente di nuovo.
///
/// Viaggia nel profilo, accanto a [`CHIAVE_SPETTRO_VISIBILE`] e per la stessa
/// ragione: «voglio leggere com'è fatto il file» è un gusto di chi ascolta, non
/// un fatto di questa macchina, e resta vero su qualunque computer.
pub const CHIAVE_FORMATO_VISIBILE: &str = "player.fileFormat.visible";

/// Registra un ascolto finito.
///
/// **Non scrive niente se l'ascolto non conta.** La decisione l'ha già presa
/// `rymembra_domain::listen::counts_as_play`, e questa funzione la rispetta invece
/// di riprenderla: nel vecchio albero il conteggio si incrementava all'avvio del
/// brano, e scorrere cinquanta canzoni saltandole dopo un secondo ne
/// incrementava cinquanta.
///
/// Restituisce `true` se ha scritto.
///
/// # A nome di chi
///
/// `dispositivo` è l'identificativo di **questa** macchina, e non è un dettaglio
/// contabile: il conteggio non è più un numero in una colonna ma una somma di
/// numeri, uno per dispositivo, e chi non dice il proprio nome non può
/// contribuire. Vedi [`crate::sincronia::conta_ascolto`] sul perché la somma
/// è l'unica forma che regge fra due macchine — con un numero solo, fondere può
/// solo raddoppiare la storia o perderne metà.
///
/// # Errori
///
/// `library.trackNotFound` per un identificativo ≤ 0, senza aprire nessuna
/// transazione: vedi [`riga_della_libreria`].
pub fn record_play(
    connection: &mut Connection,
    ascolto: &Listen,
    dispositivo: &str,
) -> Result<bool, AppError> {
    // Prima di tutto, anche prima di `counts`: l'invariante è «mai un id ≤ 0
    // sul database», e una guardia che dipende da un'altra condizione è una
    // guardia che un giorno qualcuno sposta senza accorgersene.
    riga_della_libreria(ascolto.track_id)?;
    if !ascolto.counts {
        return Ok(false);
    }
    let ms_played = i64::try_from(ascolto.listened_ms).unwrap_or(i64::MAX);
    let tx = crate::library::transazione_di_scrittura(connection)
        .map_err(|err| db_error("apertura della transazione d'ascolto", &err))?;

    // `stats_updated_at` insieme agli altri: è ciò su cui `merge_stats` decide
    // chi vince quando due dispositivi si allineano. Scrivere il conteggio senza
    // di lui vorrebbe dire che l'ascolto di stasera perde contro quello di ieri
    // fatto sul telefono.
    //
    // `play_count` non si incrementa più qui: lo ricalcola `conta_ascolto` come
    // somma dei contatori, subito sotto e nella stessa transazione. Farlo in due
    // posti darebbe due volte lo stesso ascolto.
    tx.execute(
        "UPDATE tracks
            SET last_played_at = ?2,
                stats_updated_at = ?2
          WHERE id = ?1",
        rusqlite::params![ascolto.track_id, ascolto.started_at],
    )
    .map_err(|err| db_error("conteggio d'ascolto", &err))?;

    crate::sincronia::conta_ascolto(&tx, ascolto.track_id, dispositivo)?;

    tx.execute(
        "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, ?2, ?3)",
        rusqlite::params![ascolto.track_id, ascolto.started_at, ms_played],
    )
    .map_err(|err| db_error("cronologia d'ascolto", &err))?;

    tx.commit()
        .map_err(|err| db_error("chiusura della transazione d'ascolto", &err))?;
    Ok(true)
}

/// La coda, nella forma che va sul disco.
///
/// Un tipo a parte invece di serializzare `QueueSnapshot` direttamente: il
/// dominio non ha serde fra le dipendenze, e non deve averlo. Il formato con cui
/// una cosa si scrive è una decisione di chi la scrive, non della cosa.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CodaSuDisco {
    tracks: Vec<i64>,
    order: Vec<usize>,
    order_pos: Option<usize>,
    shuffle: bool,
    repeat: String,
}

const fn nome_ripetizione(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "off",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    }
}

fn ripetizione_da_nome(nome: &str) -> RepeatMode {
    match nome {
        "one" => RepeatMode::One,
        "all" => RepeatMode::All,
        _ => RepeatMode::Off,
    }
}

/// Il brano è una riga della libreria di questo computer?
///
/// # La regola, e da dove viene
///
/// **id > 0 è una riga di `tracks`, id ≤ 0 non lo è mai.** Gli identificativi
/// che SQLite assegna partono da 1, e da «Ascolta insieme» in poi i numeri
/// negativi sono le voci di una sessione: maniglie che esistono solo in
/// memoria, e che su un altro computer — o su questo, a sessione finita — non
/// nominano niente. Lo zero non è di nessuno dei due, e si rifiuta con loro.
///
/// Le tre scritture della riproduzione — [`record_play`], [`save_posizione`] e
/// [`save_queue`] — passano di qui **prima** di toccare il database. È l'ultima
/// difesa e non la prima: chi sta in una sessione non dovrebbe nemmeno
/// chiamarle. Se un giorno un comando nuovo salta quella strada, il danno si
/// ferma qui invece di arrivare a una cronologia o a una coda salvata.
///
/// # Perché un errore e non un `Ok` muto
///
/// Perché un rifiuto qui vuol dire che qualcuno, più su, ha sbagliato strada, e
/// la finestra lo deve poter scrivere nel diario. È sicuro farlo: nessuno dei
/// chiamanti ripete la scrittura su un `Err` — la coda abbassa la sua bandiera e
/// annota una riga, l'ascolto annota una riga e non fa scrobble, la posizione
/// scarta l'esito — quindi un rifiuto costa al più una riga per gesto, non un
/// ciclo. La causa porta il numero e basta: un identificativo non dice niente
/// di chi ascolta.
///
/// # Chi altro passa di qui
///
/// `pub(crate)` e non copiata: l'audit delle guardie della 3.1.0 l'ha messa in
/// testa a ogni altra strada di questo crate in cui un id ≤ 0 farebbe nascere
/// una riga o entrerebbe in una coda — l'aggiunta a una playlist
/// (`playlists::accoda`), le scritture dei testi che ricevono un
/// `BranoDaTestare` già in mano, l'autoplay e la radio seminati da un brano —
/// e nelle due letture di questo modulo che darebbero a una voce qualcosa che
/// non è suo: il segno di posizione di prima ([`load_posizione`]) e una coda
/// sul disco che non dovrebbe esserci ([`load_queue`]). Una regola sola detta
/// in un posto solo: una seconda copia sarebbe la prima a dimenticarsi dello
/// zero.
///
/// # Errori
///
/// `library.trackNotFound` se `track_id` ≤ 0.
pub(crate) fn riga_della_libreria(track_id: i64) -> Result<(), AppError> {
    if track_id > 0 {
        return Ok(());
    }
    Err(AppError::new(ErrorCode::LibraryTrackNotFound {
        track_id: Some(track_id),
    })
    .with_cause(format!(
        "id {track_id}: non è una riga della libreria, non si scrive"
    )))
}

/// Scrive la durata di un brano che non ne aveva una, al primo ascolto.
///
/// # A chi serve
///
/// Alle righe provvisorie del cloud: entrano in libreria dall'elenco del
/// servizio, prima che si legga un byte (`library`, la prima delle due
/// passate), e la durata la sa solo chi l'ha già detta — OneDrive sì, gli
/// altri no. E il lettore dei tag a intervalli, su un file con le proprietà
/// troppo lontane, rilegge senza proprietà: anche lì la durata resta zero. Chi
/// suona il brano la conosce dal decodificatore, ed è un peccato buttarla.
///
/// # Solo sopra lo zero
///
/// Mai sopra una durata che c'è: quella viene dal file o da chi l'ha corretta,
/// e un decodificatore che si ferma prima della fine — un brano saltato, un
/// flusso caduto — direbbe un numero più piccolo del vero. Un brano a zero
/// invece non ha niente da perdere. Con zero (o meno) non si scrive niente:
/// non dice niente.
///
/// La rilettura vera dei tag, quando arriva, riscrive la durata del file come
/// per ogni altro campo. Gli aggregati non c'entrano: `albums` e `artists` non
/// tengono durate, si sommano quando si chiedono.
///
/// Restituisce se ha scritto.
///
/// # Errori
///
/// `library.trackNotFound` se `track_id` ≤ 0 (vedi [`riga_della_libreria`]);
/// `db.queryFailed` se il database non risponde.
pub fn ricorda_durata(
    connection: &Connection,
    track_id: i64,
    durata_ms: u64,
) -> Result<bool, AppError> {
    riga_della_libreria(track_id)?;
    let Ok(durata) = i64::try_from(durata_ms) else {
        return Ok(false);
    };
    if durata <= 0 {
        return Ok(false);
    }
    connection
        .execute(
            "UPDATE tracks SET duration_ms = ?2 WHERE id = ?1 AND duration_ms = 0",
            rusqlite::params![track_id, durata],
        )
        .map(|scritte| scritte > 0)
        .map_err(|err| db_error("durata di un brano al primo ascolto", &err))
}

/// Conserva la coda perché sopravviva alla chiusura.
///
/// # Una voce che non è della libreria, e la coda non si scrive affatto
///
/// Basta **un** identificativo ≤ 0, in qualunque punto, e la scrittura si
/// rifiuta tutta: sul database resta l'ultima coda fatta solo di righe della
/// libreria. Le due alternative avevano ciascuna un buco.
///
/// - **Rifiutare solo se il corrente è ≤ 0, e filtrare il resto.** Una coda di
///   sessione è fatta tutta di maniglie, e può non avere un corrente — finita,
///   o appena svuotata del brano che suonava. Lì il corrente è `None`, la
///   guardia non scatta, il filtro toglie tutto, e sul disco finisce una coda
///   **vuota** al posto di quella dell'utente: chiusa l'applicazione a metà
///   sessione, la sua coda di prima non c'è più.
/// - **Filtrare.** Togliere delle voci vuol dire rinumerare `order` e
///   spostare `order_pos`: codice nuovo che riscrive una permutazione, con
///   `Queue::restore` a scoprirne gli errori solo al prossimo avvio. E quel che
///   resta è una coda che l'utente non ha mai avuto davanti.
///
/// Rifiutare tutto non inventa niente: o si scrive la coda com'è, o resta
/// quella di prima. Una coda mista — righe e maniglie insieme — secondo il
/// piano non esiste; se esistesse, sarebbe un errore di chi l'ha costruita, e
/// il diario lo dice. Vedi [`riga_della_libreria`].
///
/// # Errori
///
/// `library.trackNotFound` se una voce ha id ≤ 0, senza scrivere niente;
/// `db.queryFailed` se il database non risponde.
pub fn save_queue(connection: &Connection, coda: &QueueSnapshot) -> Result<(), AppError> {
    for id in &coda.tracks {
        riga_della_libreria(*id)?;
    }
    let su_disco = CodaSuDisco {
        tracks: coda.tracks.clone(),
        order: coda.order.clone(),
        order_pos: coda.order_pos,
        shuffle: coda.shuffle,
        repeat: nome_ripetizione(coda.repeat).to_owned(),
    };
    let json = serde_json::to_string(&su_disco).unwrap_or_else(|_| "{}".to_owned());
    crate::settings::write(connection, CHIAVE_CODA, &json)
}

/// Rilegge la coda conservata.
///
/// Una coda illeggibile vale **coda vuota, non errore**: è la stessa scelta che
/// `comandi.rs` fa per le cartelle sorvegliate, e per la stessa ragione. Un
/// avvio che fallisce per un valore d'impostazione corrotto non lascia
/// all'utente nessun modo di correggerlo; una coda vuota sì, ne fa un'altra.
///
/// Gli invarianti — che l'ordine sia davvero una permutazione — li verifica
/// `Queue::restore`, che è dove stanno le regole della coda.
///
/// Una coda con un id ≤ 0 è illeggibile come una storta, ed è lo specchio di
/// [`save_queue`], che non la scrive mai: se c'è, l'ha messa qualcun altro, e
/// all'avvio quelle voci finirebbero nella coda del lettore come voci di
/// «Ascolta insieme» di una sessione che non c'è. Toglierle una per una non
/// si può — `order` e `order_pos` sono indici, e cambierebbero significato —
/// quindi vale la regola di tutta la funzione: coda vuota.
pub fn load_queue(connection: &Connection) -> Result<QueueSnapshot, AppError> {
    let Some(json) = crate::settings::read(connection, CHIAVE_CODA)? else {
        return Ok(QueueSnapshot::default());
    };
    let Ok(su_disco) = serde_json::from_str::<CodaSuDisco>(&json) else {
        return Ok(QueueSnapshot::default());
    };
    if su_disco
        .tracks
        .iter()
        .any(|id| riga_della_libreria(*id).is_err())
    {
        return Ok(QueueSnapshot::default());
    }
    Ok(QueueSnapshot {
        tracks: su_disco.tracks,
        order: su_disco.order,
        order_pos: su_disco.order_pos,
        shuffle: su_disco.shuffle,
        repeat: ripetizione_da_nome(&su_disco.repeat),
    })
}

// ── dov'eri rimasto ─────────────────────────────────────────────────────────

/// A che punto del brano corrente si era arrivati.
pub const CHIAVE_POSIZIONE: &str = "player.position";

/// Conserva la posizione dentro il brano.
///
/// # Perché una chiave sua e non un campo di `QueueSnapshot`
///
/// Perché `QueueSnapshot` è un tipo del **dominio**, e descrive la struttura
/// di una coda: quali brani, in che ordine, dove siamo dentro l'ordine.
/// Quanti millisecondi sono passati dentro una traccia non è una proprietà
/// della coda — è una proprietà del motore, che il dominio non conosce e non
/// deve conoscere. Infilarcelo dentro vorrebbe dire che
/// `rymembra_domain::queue`, che oggi non sa nemmeno cosa sia il tempo, si
/// ritrova un campo che solo `rymembra-play` sa produrre.
///
/// # Perché con il brano accanto
///
/// Perché il segno si scrive ogni cinque secondi e la coda a ogni cambio di
/// brano, e fra le due scritture i conti non tornano: per i primi cinque secondi
/// di un brano nuovo il numero sul database era ancora quello del brano di
/// prima. La Home diceva «Riprendi» su un brano di 0:22 «a 0:29», e il clic
/// saltava lì. Il numero da solo non sa di chi è; con l'identificativo accanto
/// chi lo legge può chiedere se parla del brano che ha in mano.
///
/// # Errori
///
/// `library.trackNotFound` se `track_id` ≤ 0, senza scrivere niente — il segno
/// che c'era resta quello di prima: vedi [`riga_della_libreria`].
/// `db.queryFailed` se il database non risponde.
pub fn save_posizione(connection: &Connection, track_id: i64, ms: u64) -> Result<(), AppError> {
    riga_della_libreria(track_id)?;
    crate::settings::write_json(
        connection,
        CHIAVE_POSIZIONE,
        &SegnoPosizione::DelBrano {
            brano: track_id,
            ms,
        },
    )
}

/// Legge dove si era arrivati **dentro `track_id`**. Assente, o di un altro
/// brano, vale l'inizio.
///
/// Il numero nudo di prima di questa versione vale per il brano che si chiede:
/// è l'unica lettura che quel formato ha sempre avuto, ed è giusta nel caso di
/// gran lunga più comune — chi aggiorna a metà di una canzone e riapre.
///
/// Un id ≤ 0 vale l'inizio senza nemmeno leggere. Il numero nudo vale per
/// «il brano che si chiede», e una voce di «Ascolta insieme» non è mai stata
/// quel brano: senza questa riga, la prima voce di una sessione partirebbe al
/// secondo in cui l'utente aveva lasciato una canzone sua, prima
/// dell'aggiornamento. Un segno `DelBrano` con un id ≤ 0 invece non può
/// esserci, perché [`save_posizione`] non lo scrive.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_posizione(connection: &Connection, track_id: i64) -> Result<u64, AppError> {
    if riga_della_libreria(track_id).is_err() {
        return Ok(0);
    }
    Ok(
        match crate::settings::read_json::<SegnoPosizione>(connection, CHIAVE_POSIZIONE)? {
            Some(SegnoPosizione::DelBrano { brano, ms }) if brano == track_id => ms,
            Some(SegnoPosizione::Vecchio(ms)) => ms,
            _ => 0,
        },
    )
}

/// Come sta sul database il segno di [`save_posizione`].
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum SegnoPosizione {
    /// Brano e millisecondo.
    DelBrano { brano: i64, ms: u64 },
    /// Il numero nudo che si scriveva prima.
    Vecchio(u64),
}

#[cfg(test)]
mod prove;
