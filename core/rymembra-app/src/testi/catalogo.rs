//! Il catalogo dei testi, cioè LRCLIB (non `crate::catalogo`): le domande, gli esiti, la coda.
//!
//! Un pezzo di `crate::testi`: dove un commento dice «questo modulo», intende `testi` intero.

use rusqlite::Connection;
use rymembra_domain::enrich::titolo_da_cercare;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::indizi::e_segnaposto;
use rymembra_domain::testo::Cercato;
use rymembra_meta::Fornitori;
use rymembra_meta::lrclib::{self, Voce};

use super::{BranoDaTestare, COLONNE, brano_da_riga};
use crate::library::{db_error, now_ms};

// ── il catalogo ─────────────────────────────────────────────────────────────

/// Dopo quanto si torna a chiedere un brano che il catalogo non aveva.
///
/// Quattordici giorni. Deve essere **più lungo** della memoria del «non ce
/// l'ho» di `rymembra_meta::deposito` — tre giorni — altrimenti la passata
/// rimetterebbe in coda ogni due giorni brani per cui la risposta arriverebbe
/// comunque dalla cache HTTP: traffico zero, ma una passata che scorre sempre
/// gli stessi mille brani senza mai arrivare a quelli nuovi.
///
/// Più lungo ancora sarebbe sbagliato nell'altro verso: un testo che oggi manca
/// può essere caricato domani da qualcuno, ed è tutto il punto di un catalogo
/// alimentato dalla comunità.
pub const RIPROVA_MS: i64 = 14 * 24 * 60 * 60 * 1000;

/// Quanti brani prende una passata.
///
/// Cinquanta, contro i dodici gruppi dell'arricchimento, e la differenza è nel
/// costo di una richiesta: là ogni gruppo sono più letture a una al secondo con
/// dei file da riscrivere in fondo, qui è una richiesta ogni quarto di secondo
/// e una riga di database. Cinquanta brani sono una dozzina di secondi.
pub const LOTTO: usize = 50;

/// Chiede il testo di un brano al catalogo. **Non tocca il database.**
///
/// La firma è quella che è per una ragione sola: chi la chiama non ha modo di
/// tenere il lucchetto della libreria mentre aspetta la rete, perché qui dentro
/// una `Connection` non entra. È la stessa disciplina dell'arricchimento, e non
/// è affidata all'attenzione di chi legge.
///
/// # Errori
///
/// L'errore di rete così com'è. `Ok(None)` è «il catalogo non ce l'ha», che è
/// un fatto sul brano e si ricorda; l'errore è «non si sa», che è un fatto
/// sulla rete di adesso e non si ricorda.
pub fn cerca_in_rete(
    fornitori: &Fornitori,
    brano: &BranoDaTestare,
) -> Result<Option<Voce>, AppError> {
    let domanda = da_chiedere_al_catalogo(brano);
    con_ritentativi(|| lrclib::cerca(fornitori, &domanda.cercato()))
}

/// Tutte le voci che il catalogo ha per questo brano, per sceglierne una a mano.
/// **Non tocca il database.**
///
/// Stessa domanda di [`cerca_in_rete`] — gli stessi segnaposto tolti, lo stesso
/// titolo ripulito — e senza nessuna scelta: la fa chi guarda. Vedi
/// [`rymembra_meta::lrclib::candidati`].
///
/// # Errori
///
/// Gli stessi di [`cerca_in_rete`].
pub fn candidati_in_rete(
    fornitori: &Fornitori,
    brano: &BranoDaTestare,
) -> Result<Vec<Voce>, AppError> {
    let domanda = da_chiedere_al_catalogo(brano);
    lrclib::candidati(fornitori, &domanda.cercato())
}

/// Come [`cerca_in_rete`], ma senza fidarsi di quel che il deposito ricorda.
///
/// La chiama il ritentativo esplicito, cioè il pulsante «Cerca di nuovo». Il
/// perché per esteso sta su [`rymembra_meta::lrclib::cerca_di_nuovo`]; qui basta
/// dire che chi preme quel pulsante ha davanti un pannello vuoto, e rileggere
/// la risposta di ieri gli darebbe lo stesso pannello vuoto.
///
/// # Errori
///
/// Gli stessi di [`cerca_in_rete`].
pub fn cerca_di_nuovo_in_rete(
    fornitori: &Fornitori,
    brano: &BranoDaTestare,
) -> Result<Option<Voce>, AppError> {
    let domanda = da_chiedere_al_catalogo(brano);
    con_ritentativi(|| lrclib::cerca_di_nuovo(fornitori, &domanda.cercato()))
}

/// La domanda al catalogo per un brano, con i campi già decisi.
///
/// Possiede le sue stringhe perché due di loro possono non venire dai tag: vedi
/// [`da_chiedere_al_catalogo`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Domanda {
    pub(super) titolo: String,
    /// Vuoto quando non si sa: il catalogo allora si interroga per titolo.
    pub(super) artista: String,
    pub(super) album: Option<String>,
    pub(super) durata_ms: Option<u64>,
}

impl Domanda {
    fn cercato(&self) -> Cercato<'_> {
        Cercato {
            titolo: &self.titolo,
            artista: &self.artista,
            album: self.album.as_deref(),
            durata_ms: self.durata_ms,
        }
    }
}

/// Con che cosa si interroga il catalogo per questo brano.
///
/// # Il titolo si ripulisce prima di chiedere
///
/// [`titolo_da_cercare`] esisteva da quando esiste l'arricchimento e qui non
/// era mai stata usata: si mandava `title` grezzo, quindi «Poetica (Official
/// Video)» — cioè come si chiama il file di chi la sua libreria l'ha costruita
/// scaricando — arrivava così com'era a un catalogo che quella voce non ce
/// l'ha e non ce l'avrà mai.
///
/// Toglie solo il rumore di **pubblicazione**: i gruppi fra parentesi fatti
/// interamente di parole come «official», «video», «lyrics», e la coda `-
/// Topic` dei canali che YouTube genera da sé. `(Live)`, `- Remastered` e
/// `feat.` **restano**, ed è giusto: cercare la versione dal vivo di un file
/// dal vivo è quel che si vuole, e toglierlo dalla domanda farebbe trovare lo
/// studio. La regola sta scritta per esteso sulla costante `RUMORE_DI_CARICAMENTO`.
///
/// # Cosa non cambia, e perché il rischio è piccolo
///
/// Né la chiave con cui la risposta si ricorda né i veti di
/// [`rymembra_domain::testo::scegli`]: tutt'e due passano da
/// `normalize_for_match`, che i gruppi fra parentesi li toglie **tutti** già
/// da prima. Il peggio che può succedere è quindi una domanda esatta che
/// fallisce su un titolo che il catalogo conosceva per intero, cioè una
/// richiesta in più — e la domanda generosa, che è quella che poi decide, ci
/// guadagna sempre.
///
/// Ripiego sul grezzo se la ripulitura non lascia niente: un brano che si
/// chiama davvero «Video» esiste, e cercarne la stringa vuota non è cercare.
///
/// # I segnaposto non si mandano
///
/// Un file senza tag arriva qui con «Artista sconosciuto» e «Album sconosciuto»,
/// che sono quel che la scansione scrive per dire «non lo so» — e si mandavano
/// al catalogo così com'erano: una domanda con una risposta vuota garantita, e
/// il brano segnato come cercato per due settimane. Adesso un segnaposto non
/// entra nella domanda: l'album si omette, e l'artista si prende dal **nome del
/// file** quando lo porta — «Artista - Titolo.mp3» — altrimenti si lascia
/// vuoto e il catalogo si interroga per titolo. Dal nome del file e non dalle
/// cartelle: senza sapere quali sono le cartelle sorvegliate, «Musica» o
/// «Desktop» diventerebbero l'artista.
pub(super) fn da_chiedere_al_catalogo(brano: &BranoDaTestare) -> Domanda {
    let dal_nome = || {
        // Un brano di catalogo non ha un nome di file da cui indovinare
        // l'artista, e non gli serve: il titolo e l'autore glieli ha dati il
        // catalogo, che li sa.
        rymembra_domain::indizi::dal_percorso(
            brano
                .path
                .as_deref()
                .map_or("", rymembra_domain::paths::base_name),
            &[],
            rymembra_domain::paths::PathRules::for_current_platform(),
        )
    };
    let (grezzo, artista) = if e_segnaposto(Some(&brano.artist)) {
        let indizi = dal_nome();
        (
            // Il titolo dei tag, se c'è; altrimenti quello del nome del file,
            // senza il numero di traccia e l'artista davanti.
            if e_segnaposto(Some(&brano.title)) || indizi.artista.is_some() {
                indizi.titolo.unwrap_or_else(|| brano.title.clone())
            } else {
                brano.title.clone()
            },
            indizi.artista.unwrap_or_default(),
        )
    } else {
        (brano.title.clone(), brano.artist.clone())
    };
    let titolo = if e_segnaposto(Some(&grezzo)) {
        String::new()
    } else {
        let pulito = titolo_da_cercare(&grezzo);
        if pulito.trim().is_empty() {
            grezzo
        } else {
            pulito
        }
    };
    let album = brano.album.trim();
    Domanda {
        titolo,
        artista,
        album: (!e_segnaposto(Some(album))).then(|| album.to_owned()),
        durata_ms: u64::try_from(brano.duration_ms).ok().filter(|d| *d > 0),
    }
}

/// Quanto si aspetta prima del secondo e del terzo tentativo.
///
/// Mezzo secondo e un secondo e mezzo. Il primo prende il caso che capita più
/// spesso di tutti — il wifi che si riaggancia, la connessione che cade sul
/// primo pacchetto — e il secondo dà tempo a un servizio che sta ripartendo.
/// Un quarto tentativo non c'è: oltre i due secondi non è più un inciampo, è
/// «adesso non si può», e insistere vorrebbe dire tenere fermo il pannello del
/// testo — o la passata — su un guasto che non passerà da solo.
const RITENTATIVI_MS: [u64; 2] = [500, 1500];

/// Ripete una domanda al catalogo quando il guasto è di quelli che passano.
///
/// # Cosa si ritenta, e cosa no
///
/// Solo `is_retryable`: un `400` è una domanda scritta male da noi e rifarla
/// tre volte darebbe tre volte lo stesso `400`.
///
/// E **mai** l'interruttore aperto, che pure si dichiara ritentabile. Quel
/// codice non descrive un guasto: descrive la [`rymembra_meta::Cadenza`] che ha
/// **già** deciso di non parlare con quel servizio per qualche minuto, e
/// riprovare significa ricevere lo stesso rifiuto in un microsecondo, tre
/// volte, dopo due secondi di attesa buttati. Il ritentativo che serve lì è
/// quello dell'interruttore, non il nostro.
///
/// `Ok(None)` — «il catalogo non ce l'ha» — non è un guasto e non si ripete:
/// è una risposta, ed è arrivata.
fn con_ritentativi<F>(mut chiedi: F) -> Result<Option<Voce>, AppError>
where
    F: FnMut() -> Result<Option<Voce>, AppError>,
{
    let mut fatti = 0_usize;
    loop {
        let err = match chiedi() {
            Ok(voce) => return Ok(voce),
            Err(err) => err,
        };
        let ancora = err.is_retryable()
            && !matches!(err.code(), ErrorCode::NetCircuitOpen { .. })
            && fatti < RITENTATIVI_MS.len();
        let Some(attesa) = RITENTATIVI_MS.get(fatti).copied().filter(|_| ancora) else {
            return Err(err);
        };
        std::thread::sleep(std::time::Duration::from_millis(attesa));
        fatti = fatti.saturating_add(1);
    }
}

/// Scrive quel che il catalogo ha risposto, sì o no che sia.
///
/// Il «no» si scrive esattamente come il «sì», e vale quanto: una riga con i
/// testi a `NULL` e `checked_at` pieno è la memoria di un brano che nessuno
/// conosce, ed è quel che impedisce alla passata di ricominciare ogni volta dai
/// duecento file che non troverà mai.
///
/// Non sovrascrive mai una sincronizzazione fatta a mano: la condizione sta
/// nella `WHERE` dell'`UPSERT`, cioè nel database, e non in un `if` che il
/// prossimo punto di chiamata potrebbe dimenticare.
///
/// # Il «no» non cancella quel che si aveva
///
/// Un `None` scriveva i testi a `NULL` sopra la riga che c'era, e ci arrivava
/// «Cerca di nuovo»: chi aveva un testo scaricato ieri e premeva il pulsante per
/// vedere se ce n'era uno con i tempi, se il catalogo nel frattempo non
/// rispondeva più per quel brano, perdeva anche quello di ieri. Adesso il «no»
/// su una riga che esiste scrive soltanto che si è chiesto; su una riga che non
/// esiste la crea vuota, come prima, perché è quella la memoria del brano che
/// nessuno conosce. Buttare un testo sbagliato è un gesto a sé: [`rifiuta`].
///
/// # Quel che chi ascolta ha deciso
///
/// Non passa sopra a una voce scelta a mano — vedi [`ricorda_scelta`] — e non
/// scrive una voce che chi ascolta ha scartato: vedi [`rifiuta`]. In tutti e due
/// i casi la risposta vale come un «no», e resta solo da segnare che si è
/// chiesto. Le due condizioni stanno nella `WHERE` dell'`UPSERT`, come quella di
/// `mano`, e per la stessa ragione.
///
/// # Errori
///
/// `db.queryFailed`. Qui l'errore si propaga: una passata che crede di aver
/// registrato un esito senza averlo fatto rifà lo stesso lavoro per sempre.
/// `library.trackNotFound` se `brano.id` ≤ 0, senza scrivere niente: è una
/// voce di «Ascolta insieme», non una riga di questa libreria (vedi
/// `playback::riga_della_libreria`). Vale per tutte le scritture di questo
/// modulo che ricevono un [`BranoDaTestare`] già in mano.
pub fn ricorda_esito(
    connection: &Connection,
    brano: &BranoDaTestare,
    voce: Option<&Voce>,
) -> Result<(), AppError> {
    crate::playback::riga_della_libreria(brano.id)?;
    let Some(voce) = voce else {
        return connection
            .execute(
                "INSERT INTO lyrics
                   (track_key, source, offset_ms, instrumental, duration_ms, updated_at, checked_at)
                 VALUES (?1, 'lrclib', 0, 0, ?2, ?3, ?3)
                 ON CONFLICT(track_key) DO UPDATE SET checked_at = excluded.checked_at",
                rusqlite::params![brano.track_key, brano.duration_ms, now_ms()],
            )
            .map(|_| ())
            .map_err(|err| db_error("scrittura dell'esito di un testo", &err));
    };
    let (piatto, sincronizzato, strumentale, id_catalogo) = (
        voce.piatto.as_deref(),
        voce.sincronizzato.as_deref(),
        voce.candidato.strumentale,
        Some(voce.candidato.id),
    );
    connection
        .execute(
            "INSERT INTO lyrics
               (track_key, plain, synced, source, lrclib_id, offset_ms, instrumental,
                duration_ms, updated_at, checked_at)
             VALUES (?1, ?2, ?3, 'lrclib', ?4, 0, ?5, ?6, ?7, ?7)
             ON CONFLICT(track_key) DO UPDATE SET
               plain = excluded.plain,
               synced = excluded.synced,
               source = excluded.source,
               lrclib_id = excluded.lrclib_id,
               instrumental = excluded.instrumental,
               duration_ms = excluded.duration_ms,
               updated_at = excluded.updated_at,
               checked_at = excluded.checked_at
             WHERE lyrics.source <> 'mano'
               AND lyrics.scelta = 0
               AND NOT EXISTS (
                 SELECT 1 FROM json_each(COALESCE(lyrics.scartati, '[]'))
                 WHERE value = excluded.lrclib_id
               )",
            rusqlite::params![
                brano.track_key,
                piatto,
                sincronizzato,
                id_catalogo,
                i64::from(strumentale),
                brano.duration_ms,
                now_ms(),
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("scrittura dell'esito di un testo", &err))?;

    // Se l'`UPSERT` non ha toccato niente — il testo è di chi ascolta, la voce
    // l'ha scelta lui, o l'ha scartata — resta comunque da segnare che si è
    // guardato: senza, la passata rimetterebbe in coda per sempre un brano a
    // cui ha già chiesto.
    let _ = connection.execute(
        "UPDATE lyrics SET checked_at = ?2 WHERE track_key = ?1",
        rusqlite::params![brano.track_key, now_ms()],
    );
    Ok(())
}

/// Scrive la voce del catalogo che chi ascolta ha scelto dall'elenco.
///
/// Come [`ricorda_esito`], con due differenze. La riga si segna `scelta`, e da
/// quel momento le risposte del catalogo non le passano sopra e la passata non
/// la rimette in coda: prima una voce senza tempi scelta a mano tornava in coda
/// dopo quattordici giorni, e la scelta automatica la sostituiva con quella che
/// chi ascolta aveva appena scartato. E la voce esce dagli scartati, se c'era:
/// sceglierla adesso è un ripensamento.
///
/// Il testo sincronizzato a mano resta, come in ogni `UPSERT` di questo modulo.
///
/// # Errori
///
/// `db.queryFailed`; `library.trackNotFound` per un id ≤ 0, come
/// [`ricorda_esito`].
pub fn ricorda_scelta(
    connection: &Connection,
    brano: &BranoDaTestare,
    voce: &Voce,
) -> Result<(), AppError> {
    crate::playback::riga_della_libreria(brano.id)?;
    connection
        .execute(
            "INSERT INTO lyrics
               (track_key, plain, synced, source, lrclib_id, offset_ms, instrumental,
                duration_ms, updated_at, checked_at, scelta)
             VALUES (?1, ?2, ?3, 'lrclib', ?4, 0, ?5, ?6, ?7, ?7, 1)
             ON CONFLICT(track_key) DO UPDATE SET
               plain = excluded.plain,
               synced = excluded.synced,
               source = excluded.source,
               lrclib_id = excluded.lrclib_id,
               instrumental = excluded.instrumental,
               duration_ms = excluded.duration_ms,
               updated_at = excluded.updated_at,
               checked_at = excluded.checked_at,
               scelta = 1,
               scartati = (
                 SELECT json_group_array(value)
                 FROM json_each(COALESCE(lyrics.scartati, '[]'))
                 WHERE value <> excluded.lrclib_id
               )
             WHERE lyrics.source <> 'mano'",
            rusqlite::params![
                brano.track_key,
                voce.piatto.as_deref(),
                voce.sincronizzato.as_deref(),
                voce.candidato.id,
                i64::from(voce.candidato.strumentale),
                brano.duration_ms,
                now_ms(),
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("scrittura di un testo scelto a mano", &err))
}

/// Dimentica di aver già chiesto al catalogo per questo brano.
///
/// Azzera `checked_at`, e **solo** quello: il testo che c'è resta, la
/// correzione di chi ascolta resta, la provenienza resta. Quel che se ne va è
/// la frase «a questo brano si è già chiesto», che è ciò che tiene un brano
/// fuori dalla coda della passata per [`RIPROVA_MS`] — quattordici giorni.
///
/// Serve al ritentativo esplicito, ed è la metà locale del gesto: l'altra metà
/// è saltare il deposito, che sta in [`cerca_di_nuovo_in_rete`]. Servono tutt'e
/// due, perché ricordano due cose diverse in due posti diversi — il deposito
/// ricorda la *risposta*, questa colonna ricorda la *domanda* — e sanarne una
/// sola lascerebbe il pulsante senza effetto.
///
/// Tocca anche la riga di `mano`, al contrario degli `UPSERT` di questo modulo:
/// lì la protezione serve a non riscrivere il testo di chi ascolta, e qui il
/// testo non si tocca. Non ha comunque conseguenze — una riga con i tempi in
/// coda non ci torna — ma la regola va detta invece che dedotta.
///
/// # Errori
///
/// `db.queryFailed`. Si propaga: un ritentativo che crede di aver dimenticato
/// senza averlo fatto rifà la stessa domanda alla stessa cache.
pub fn dimentica_esito(connection: &Connection, track_key: &str) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE lyrics SET checked_at = NULL WHERE track_key = ?1",
            [track_key],
        )
        .map(|_| ())
        .map_err(|err| db_error("azzeramento della memoria di una ricerca", &err))
}

/// Dopo quanto si riprova un brano su cui il catalogo ha risposto con un guasto.
///
/// Un giorno, contro i quattordici di [`RIPROVA_MS`]: un guasto non è una
/// risposta sul brano, e fra un giorno quel titolo potrebbe andare benissimo.
/// Ma nemmeno zero, che era il valore di prima.
pub const RIPROVA_DOPO_UN_GUASTO_MS: i64 = 24 * 60 * 60 * 1000;

/// Il catalogo ha risposto a questo brano con un guasto che non passa riprovando.
///
/// # Il difetto che chiude
///
/// La passata non registrava niente per un brano che faceva inciampare il
/// catalogo — «non si sa» non è «non c'è» — e la coda si rilegge da capo a ogni
/// lotto, ordinata per ascolti. Cinque brani così in cima alla coda erano cinque
/// guasti di fila, e cinque guasti di fila fermano la passata: a ogni avvio la
/// stessa passata ripartiva, inciampava sugli stessi cinque, e si fermava. Il
/// resto della libreria non veniva mai guardato.
///
/// Adesso il brano si segna come cercato **un giorno fa meno quattordici**:
/// esce dalla coda per [`RIPROVA_DOPO_UN_GUASTO_MS`] invece che per due
/// settimane, e non tocca nessun testo che la riga avesse già. Un
/// `checked_at` più recente non si abbassa.
///
/// # Errori
///
/// `db.queryFailed`; `library.trackNotFound` per un id ≤ 0, come
/// [`ricorda_esito`].
pub fn rimanda(connection: &Connection, brano: &BranoDaTestare) -> Result<(), AppError> {
    crate::playback::riga_della_libreria(brano.id)?;
    let adesso = now_ms();
    let finto = adesso
        .saturating_sub(RIPROVA_MS)
        .saturating_add(RIPROVA_DOPO_UN_GUASTO_MS);
    connection
        .execute(
            "INSERT INTO lyrics
               (track_key, source, offset_ms, instrumental, duration_ms, updated_at, checked_at)
             VALUES (?1, 'lrclib', 0, 0, ?2, ?3, ?4)
             ON CONFLICT(track_key) DO UPDATE SET checked_at = excluded.checked_at
             WHERE lyrics.checked_at IS NULL OR lyrics.checked_at < excluded.checked_at",
            rusqlite::params![brano.track_key, brano.duration_ms, adesso, finto],
        )
        .map(|_| ())
        .map_err(|err| db_error("rinvio di un testo che il catalogo non ha dato", &err))
}

/// Butta il testo arrivato dal catalogo per questo brano: «non è questo».
///
/// # Perché un gesto a sé
///
/// Perché fino a qui un testo scelto male dal catalogo non si toglieva più: la
/// scelta automatica aveva passato i veti, «Cerca di nuovo» ritrovava la stessa
/// voce, e l'unico modo di non vederlo era sincronizzarne uno a mano. Adesso chi
/// ascolta lo riconosce e lo scarta, e il brano resta segnato come cercato —
/// quindi né il pannello né la passata vanno a riprenderlo da soli. La voce
/// giusta, se c'è, si sceglie dall'elenco: vedi [`candidati_in_rete`].
///
/// Tocca **solo** quel che è venuto dal catalogo. Il testo di chi ascolta non si
/// butta da qui, e quello del sidecar o del tag sta in un file che non è nostro:
/// scartarne la copia in tabella non servirebbe a niente, perché alla prossima
/// apertura il file lo ridarebbe.
///
/// # E non torna
///
/// Segnarlo come cercato non bastava: la passata lo richiede dopo quattordici
/// giorni, «Cerca di nuovo» subito, e tutt'e due ritrovavano la stessa voce e la
/// riscrivevano. L'identificativo scartato si aggiunge a `scartati`, e
/// [`ricorda_esito`] non scrive più quella voce per questo brano. Una voce
/// **diversa** invece sì: un catalogo si corregge, e il testo giusto può
/// arrivare domani.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn rifiuta(connection: &Connection, track_key: &str) -> Result<(), AppError> {
    let adesso = now_ms();
    connection
        .execute(
            "UPDATE lyrics
               SET scartati = CASE
                     WHEN lrclib_id IS NULL THEN scartati
                     ELSE json_insert(COALESCE(scartati, '[]'), '$[#]', lrclib_id)
                   END,
                   plain = NULL, synced = NULL, lrclib_id = NULL, instrumental = 0,
                   scelta = 0, updated_at = ?2, checked_at = ?2
             WHERE track_key = ?1 AND source = 'lrclib'",
            rusqlite::params![track_key, adesso],
        )
        .map(|_| ())
        .map_err(|err| db_error("scarto di un testo del catalogo", &err))
}

/// Chi è in coda per il catalogo, come condizione `WHERE` su `tracks t`.
///
/// Una stringa sola perché a questa domanda rispondono in due — [`da_cercare`],
/// che prende il lotto, e [`quanti_da_cercare`], che dice a chi guarda quanto
/// manca — e due condizioni scritte a mano due volte diventano due condizioni
/// diverse al primo ripensamento. Il sintomo sarebbe una barra che arriva a zero
/// mentre la passata continua a lavorare.
///
/// `?1` è la soglia del ritentativo: `adesso − `[`RIPROVA_MS`].
///
/// Una voce scelta a mano non è una domanda aperta, anche senza tempi: vedi
/// [`ricorda_scelta`].
const IN_CODA: &str = "t.id IN (SELECT MIN(id) FROM tracks GROUP BY track_key)
       AND NOT EXISTS (
         SELECT 1 FROM lyrics l
         WHERE l.track_key = t.track_key
           AND (l.synced IS NOT NULL
                OR l.instrumental = 1
                OR l.scelta = 1
                OR l.checked_at >= ?1)
       )";

/// Quanti brani aspettano il catalogo, adesso.
///
/// Non è [`Copertura::mancanti`]: quello conta i brani di cui non si ha
/// **niente**, questo conta le domande ancora aperte — e un brano di cui si ha
/// il testo piatto è una domanda aperta, perché i tempi possono esserci.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn quanti_da_cercare(connection: &Connection, adesso_ms: i64) -> Result<i64, AppError> {
    let sql = format!("SELECT COUNT(*) FROM tracks t WHERE {IN_CODA}");
    connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("quanti testi restano da cercare", &err))?
        .query_row([adesso_ms.saturating_sub(RIPROVA_MS)], |row| row.get(0))
        .map_err(|err| db_error("quanti testi restano da cercare", &err))
}

/// I prossimi brani da chiedere al catalogo, i più ascoltati per primi.
///
/// # Uno per brano, non uno per file
///
/// `IN (SELECT MIN(id) … GROUP BY track_key)` tiene un file solo per ogni
/// brano: chi ha la stessa canzone in FLAC e in mp3 la fa cercare una volta, e
/// la riga che ne esce vale per tutt'e due perché la tabella è indicizzata su
/// `track_key`.
///
/// # Chi si ascolta viene prima
///
/// L'ordine è per numero di ascolti. Su una libreria di duemila brani la prima
/// passata copre le canzoni che si sentono davvero mentre le altre aspettano il
/// loro turno — che è la differenza fra «fra due minuti serve» e «fra venti
/// minuti è tutto pronto».
///
/// # Un testo senza tempi non è un brano già fatto
///
/// Quel che esclude un brano dalla coda sono i **tempi**, lo strumentale e un
/// `checked_at` recente — non l'esistenza di un testo qualunque. Il testo
/// piatto arriva quasi sempre dal tag del file, e [`ricorda`](super::ricorda) lo scrive senza
/// toccare `checked_at` proprio perché al catalogo non gli si è ancora chiesto
/// niente: escluderlo qui vorrebbe dire che chi ha i testi dentro i propri mp3
/// non ottiene i tempi mai, per nessun brano, e non capisce perché.
///
/// Il costo è che un brano che il catalogo conosce solo in piatto torna in coda
/// ogni [`RIPROVA_MS`]. È la stessa spesa che si accetta per i brani che il
/// catalogo non conosce affatto, e per la stessa ragione: quel che manca oggi
/// può caricarlo domani qualcuno.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn da_cercare(
    connection: &Connection,
    adesso_ms: i64,
    quanti: usize,
) -> Result<Vec<BranoDaTestare>, AppError> {
    let sql = format!(
        "SELECT {COLONNE} FROM tracks t
         WHERE {IN_CODA}
         ORDER BY t.play_count DESC, t.id
         LIMIT ?2"
    );
    let soglia = adesso_ms.saturating_sub(RIPROVA_MS);
    let quanti = i64::try_from(quanti).unwrap_or(i64::MAX);
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("coda dei testi da cercare", &err))?;
    let righe = statement
        .query_map(rusqlite::params![soglia, quanti], brano_da_riga)
        .map_err(|err| db_error("coda dei testi da cercare", &err))?;
    righe
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| db_error("coda dei testi da cercare", &err))
}

/// Quanti brani stanno in ciascuno dei quattro stati.
///
/// Contati dal database a ogni richiesta e non tenuti in un contatore, per la
/// ragione già scritta a proposito dell'arricchimento: un contatore
/// divergerebbe al primo brano cancellato, e il sintomo sarebbe un pannello che
/// promette più di quel che c'è.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copertura {
    /// Brani con i tempi: scorrono.
    pub sincronizzati: i64,
    /// Brani col solo testo: si leggono.
    pub piatti: i64,
    /// Brani che non hanno parole.
    pub strumentali: i64,
    /// Brani per cui non si è ancora trovato niente.
    pub mancanti: i64,
}

/// I quattro numeri, su tutta la libreria.
///
/// Si contano i **brani** e non i file: `DISTINCT track_key`, come la coda. Una
/// libreria con ogni album in due formati direbbe altrimenti il doppio di
/// tutto, e la percentuale resterebbe giusta mentre i numeri mentirebbero.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn copertura(connection: &Connection) -> Result<Copertura, AppError> {
    connection
        .prepare_cached(
            "SELECT
               COALESCE(SUM(CASE WHEN l.instrumental = 1 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN l.instrumental = 0 AND l.synced IS NOT NULL
                                 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN l.instrumental = 0 AND l.synced IS NULL
                                      AND l.plain IS NOT NULL
                                 THEN 1 ELSE 0 END), 0),
               COUNT(*)
             FROM (SELECT DISTINCT track_key FROM tracks) t
             LEFT JOIN lyrics l ON l.track_key = t.track_key",
        )
        .map_err(|err| db_error("copertura dei testi", &err))?
        .query_row([], |row| {
            let strumentali: i64 = row.get(0)?;
            let sincronizzati: i64 = row.get(1)?;
            let piatti: i64 = row.get(2)?;
            let totale: i64 = row.get(3)?;
            Ok(Copertura {
                sincronizzati,
                piatti,
                strumentali,
                mancanti: totale
                    .saturating_sub(strumentali)
                    .saturating_sub(sincronizzati)
                    .saturating_sub(piatti)
                    .max(0),
            })
        })
        .map_err(|err| db_error("copertura dei testi", &err))
}
