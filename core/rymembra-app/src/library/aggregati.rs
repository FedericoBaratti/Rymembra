//! Gli aggregati: `albums` e `artists` ricostruiti dai brani, per intero o soltanto per le
//! chiavi che una scrittura ha toccato ([`Toccati`]).

use super::*;

/// Quanti album e quanti artisti sono stati ricostruiti.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Aggregates {
    /// Righe in `albums`.
    pub albums: usize,
    /// Righe in `artists`.
    pub artists: usize,
}

fn member_from_row(row: &Row<'_>) -> rusqlite::Result<AlbumMember> {
    Ok(AlbumMember {
        album_key: row.get(0)?,
        album: row.get(1)?,
        album_artist: row.get(2)?,
        artist: row.get(3)?,
        year: row.get(4)?,
        genre: row.get(5)?,
        cover_art_hash: row.get(6)?,
        cover_source: row.get(7)?,
        cover_width: row.get(8)?,
        cover_height: row.get(9)?,
        mb_release_group_id: row.get(10)?,
        mb_release_id: row.get(11)?,
        spotify_album_id: row.get(12)?,
    })
}

/// L'artista sotto cui elencare un brano, in SQL.
///
/// L'artista dell'album quando c'è, altrimenti quello del brano: lo stesso
/// ripiego del dominio. Spezzare un «A feat. B» in due artisti è un mestiere
/// diverso — richiede sapere quali «feat.», «&» e virgole separano davvero due
/// nomi e quali fanno parte di uno solo — e appartiene all'arricchimento, non a
/// una scansione del disco.
pub(super) const ARTISTA_EFFETTIVO: &str = "TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist))";

/// Ricostruisce `albums` e `artists` dai brani, **tutti**.
///
/// È la ricostruzione intera, e da questa versione è il ripiego: chi ha appena
/// scritto pochi brani passa da [`ricostruisci_aggregati`], che rifà soltanto
/// quel che quei brani nominano e chiama questa quando serve davvero — la
/// prima volta, dopo una migrazione, dopo «Rileggi tutti i tag». Alla fine
/// scrive [`VERSIONE_AGGREGATI`] nel database: è la garanzia su cui la
/// ricostruzione per chiavi si appoggia.
///
/// # Perché `albums` si butta e `artists` no
///
/// In `albums` non c'è niente che non derivi dai brani: titolo, artista, anno,
/// genere, conteggio, copertina e identificativi li calcola tutti
/// [`build_album_groups`]. Cancellare e riscrivere è quindi la ricostruzione
/// **esatta**, e costa meno di una riconciliazione riga per riga.
///
/// In `artists` invece ci sono `bio`, `image_hash` e gli identificativi esterni:
/// arrivano dall'arricchimento in rete, non dai file, e una scansione che li
/// buttasse costringerebbe a riscaricarli tutti a ogni passata. Lì si aggiunge
/// chi manca e si toglie chi non è più nominato da nessun brano.
///
/// # Il rimappaggio
///
/// I brani nuovi arrivano con la chiave di base (titolo più cartella). Quando
/// due gruppi si fondono per un identificativo autorevole la chiave canonica è
/// un'altra, e va riscritta anche su `tracks`: altrimenti la giunzione
/// brani⋈album lascerebbe fuori la metà del disco che ha conservato la chiave
/// vecchia.
///
/// # La transazione
///
/// Legge tutti i brani e poi riscrive due tabelle: la transazione che la ospita
/// va aperta in scrittura fin dall'inizio, con `TransactionBehavior::Immediate`
/// — dentro questa cassa, `transazione_di_scrittura`. Aperta `DEFERRED`, una
/// scrittura dell'arricchimento arrivata fra la lettura e la prima `DELETE` la
/// farebbe fallire con `SQLITE_BUSY_SNAPSHOT`, senza nemmeno aspettare.
pub fn rebuild_aggregates(tx: &Transaction<'_>) -> Result<Aggregates, AppError> {
    #[cfg(test)]
    INTERE_IN_QUESTO_FILO.with(|n| n.set(n.get().saturating_add(1)));
    let members = {
        let mut statement = tx
            .prepare(
                "SELECT t.album_key, t.album, t.album_artist, t.artist, t.year, t.genre,
                        t.cover_art_hash, c.source, c.width, c.height,
                        t.mb_release_group_id, t.mb_release_id, t.spotify_album_id
                 FROM tracks t
                 LEFT JOIN cover_art c ON c.hash = t.cover_art_hash
                 WHERE t.album_key IS NOT NULL AND t.album_key <> ''
                 ORDER BY t.id",
            )
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        let rows = statement
            .query_map([], member_from_row)
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?
    };

    let groups = build_album_groups(&members);
    rimappa_le_chiavi(tx, &groups.remap)?;

    tx.execute("DELETE FROM albums", [])
        .map_err(|err| db_error("svuotamento degli album", &err))?;
    scrivi_gli_album(tx, &groups.albums)?;

    tx.execute_batch(&format!(
        "INSERT OR IGNORE INTO artists (name)
           SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks
           WHERE {ARTISTA_EFFETTIVO} <> '';
         DELETE FROM artists
           WHERE name NOT IN (SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks);"
    ))
    .map_err(|err| db_error("ricostruzione degli artisti", &err))?;

    let artists: i64 = tx
        .query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))
        .map_err(|err| db_error("conteggio degli artisti", &err))?;

    // Adesso le due tabelle sono esattamente quel che la regola di oggi produce
    // dai brani di oggi: da qui in poi la ricostruzione per chiavi toccate può
    // fidarsi del resto. Vedi [`VERSIONE_AGGREGATI`].
    tx.execute(
        "UPDATE libreria_stato SET aggregati = ?1 WHERE id = 1",
        [VERSIONE_AGGREGATI],
    )
    .map_err(|err| db_error("versione degli aggregati", &err))?;

    Ok(Aggregates {
        albums: groups.albums.len(),
        artists: usize::try_from(artists).unwrap_or(0),
    })
}

// Quante ricostruzioni intere ha fatto il filo corrente: serve alla prova di
// parità per sapere che la strada per chiavi l'ha davvero percorsa, e non è
// ripiegata ogni volta su quella intera. Per filo e non per processo, perché
// le prove girano in parallelo.
#[cfg(test)]
thread_local! {
    pub(super) static INTERE_IN_QUESTO_FILO: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// La versione della regola con cui si costruiscono `albums` e `artists`.
///
/// # A cosa serve un numero
///
/// La ricostruzione per chiavi toccata — [`ricostruisci_aggregati`] — rifà solo
/// gli album e gli artisti nominati dai brani che una scrittura ha cambiato, e
/// lascia stare il resto. È esatta **a una condizione**: che il resto sia già
/// quel che la regola di adesso produrrebbe. Dopo una ricostruzione intera lo
/// è per definizione, e da lì in poi ogni passata per chiavi lo mantiene; ma
/// una libreria scritta da una versione che raggruppava in un altro modo non lo
/// è, e nessuna passata per chiavi la rimetterebbe in riga — toccherebbe solo
/// gli album che qualcuno ha cambiato.
///
/// Il numero sta nel database, in `libreria_stato.aggregati`, e lo scrive
/// [`rebuild_aggregates`] alla fine di ogni ricostruzione intera. Finché non
/// coincide con questo, [`ricostruisci_aggregati`] ricostruisce per intero. Chi
/// cambia la regola — `rymembra_domain::album`, o il modo in cui qui si leggono
/// i membri — **alza questo numero**, e la prima scansione dopo l'aggiornamento
/// rimette tutto in riga da sé.
///
/// # Le versioni
///
/// - **1** — la regola con cui è arrivata la ricostruzione per chiavi.
/// - **2** — due super-gruppi con la stessa chiave canonica si fondono, e la
///   chiusura per chiavi segue anche quel legame: vedi [`build_album_groups`].
///   Con la 1 un disco riletto a metà faceva fallire ogni ricostruzione su
///   `UNIQUE constraint failed: albums.album_key`. Chi scrive i brani — la
///   scansione, l'arricchimento — li conferma prima, in una transazione sua, e
///   la ricostruzione fallita tornava indietro da sola: brani nuovi, album
///   vecchi, e le chiavi toccate perse con la transazione. Una libreria che a
///   quel punto diceva già 1 restava «in regola» senza esserlo, e nessuna
///   passata per chiavi l'avrebbe rimessa a posto.
pub const VERSIONE_AGGREGATI: i64 = 2;

/// Oltre quante chiavi d'album toccate conviene rifare tutto.
///
/// Duemila. La ricostruzione per chiavi paga ogni chiave con una lettura per
/// indice e un'ammissione in un elenco `IN`; quella intera paga una lettura di
/// tutta la tabella, una volta. Sopra qualche migliaio di album toccati — la
/// prima scansione, «Rileggi tutti i tag», una cartella enorme appena
/// riorganizzata — la seconda costa meno della prima, e non vale la pena
/// portarsi dietro in memoria un insieme che sta per diventare la libreria.
pub(super) const TETTO_PER_CHIAVI: usize = 2_000;

/// Quanti valori stanno in un elenco `IN (…)` di una query sola.
///
/// Cinquecento, come [`LOTTO`]: ben sotto i limiti di SQLite sul numero di
/// parametri, e abbastanza perché un elenco di qualche migliaio di chiavi
/// costi una manciata di query invece di una per chiave.
pub(super) const PEZZO: usize = 500;

/// Gli album e gli artisti che una scrittura ha toccato, **prima e dopo**.
///
/// # Perché prima e dopo
///
/// Perché un brano che cambia album ne tocca due: quello da cui esce, che ha un
/// brano in meno e forse non esiste più, e quello in cui entra. Lo stesso per
/// l'artista. Chi scrive segna i brani **prima** di cambiarli — le chiavi che
/// avevano — e **dopo** — quelle che hanno — con la stessa
/// [`Toccati::segna_brani`]: per una riga tolta la seconda lettura non trova
/// niente, per una inserita la prima, e in tutti e due i casi è giusto così.
///
/// # Perché le chiavi si leggono dal database
///
/// E non si calcolano dai tag appena letti: la chiave d'album di una riga è
/// quella che [`rebuild_aggregates`] ha rimappato, l'artista effettivo è
/// un'espressione SQL ([`ARTISTA_EFFETTIVO`]) con la sua idea di cosa sia uno
/// spazio da togliere, e una correzione a mano rimessa da `incerti` cambia
/// tutte e due dopo la scrittura dei tag. Ricalcolarle in Rust vorrebbe dire
/// una seconda definizione delle stesse due cose; leggerle costa una query per
/// lotto.
///
/// # Quando si smette di contare
///
/// Oltre [`TETTO_PER_CHIAVI`] chiavi d'album l'insieme si butta e resta la
/// bandiera «tutto»: la ricostruzione sarà intera comunque, e tenere in memoria
/// centomila chiavi per poi non usarle non serve a nessuno.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toccati {
    /// Le chiavi d'album, come stanno in `tracks.album_key`.
    album: std::collections::BTreeSet<String>,
    /// Gli artisti effettivi, come li calcola [`ARTISTA_EFFETTIVO`].
    artisti: std::collections::BTreeSet<String>,
    /// Si è rinunciato a contare: si rifà tutto.
    tutto: bool,
}

impl Toccati {
    /// Nessuna chiave toccata, ancora.
    #[must_use]
    pub fn nessuno() -> Self {
        Self::default()
    }

    /// Tutto toccato: la ricostruzione sarà intera.
    #[must_use]
    pub fn tutti() -> Self {
        Self {
            tutto: true,
            ..Self::default()
        }
    }

    /// Niente da ricostruire.
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        !self.tutto && self.album.is_empty() && self.artisti.is_empty()
    }

    /// Si rifà tutto, comunque.
    #[must_use]
    pub const fn e_tutto(&self) -> bool {
        self.tutto
    }

    /// Quante chiavi d'album sono segnate.
    #[must_use]
    pub fn quanti_album(&self) -> usize {
        self.album.len()
    }

    /// Aggiunge le chiavi che **adesso** hanno i brani indicati.
    ///
    /// Da chiamare prima di cambiarli e di nuovo dopo: vedi il tipo. Un
    /// identificativo che non c'è non aggiunge niente e non è un errore.
    ///
    /// # Errori
    ///
    /// `db.*` se la lettura fallisce.
    pub fn segna_brani(&mut self, connection: &Connection, ids: &[i64]) -> Result<(), AppError> {
        if self.tutto || ids.is_empty() {
            return Ok(());
        }
        for pezzo in ids.chunks(PEZZO) {
            let sql = format!(
                "SELECT album_key, {ARTISTA_EFFETTIVO} FROM tracks WHERE id IN ({})",
                segnaposti(pezzo.len())
            );
            let mut statement = connection
                .prepare_cached(&sql)
                .map_err(|err| db_error("chiavi dei brani toccati", &err))?;
            let righe = statement
                .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                    Ok((
                        riga.get::<_, Option<String>>(0)?,
                        riga.get::<_, Option<String>>(1)?,
                    ))
                })
                .map_err(|err| db_error("chiavi dei brani toccati", &err))?;
            for riga in righe {
                let (album, artista) =
                    riga.map_err(|err| db_error("chiavi dei brani toccati", &err))?;
                if let Some(album) = album.filter(|a| !a.is_empty()) {
                    self.album.insert(album);
                }
                if let Some(artista) = artista {
                    self.artisti.insert(artista);
                }
            }
            if self.album.len() > TETTO_PER_CHIAVI {
                *self = Self::tutti();
                return Ok(());
            }
        }
        Ok(())
    }

    /// Mette insieme quel che due scritture hanno toccato.
    pub fn unisci(&mut self, altro: Self) {
        if self.tutto || altro.tutto {
            *self = Self::tutti();
            return;
        }
        self.album.extend(altro.album);
        self.artisti.extend(altro.artisti);
        if self.album.len() > TETTO_PER_CHIAVI {
            *self = Self::tutti();
        }
    }
}

/// `?,?,?` per `quanti` parametri. I segnaposto si contano, non si compongono
/// con i valori.
pub(super) fn segnaposti(quanti: usize) -> String {
    std::iter::repeat_n("?", quanti)
        .collect::<Vec<_>>()
        .join(",")
}

/// `?1,?2,?3` per `quanti` parametri numerati: servono quando lo stesso elenco
/// compare più volte nella stessa query.
fn segnaposti_numerati(quanti: usize) -> String {
    (1..=quanti)
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Le righe di `albums` e `artists` si possono rifare per chiavi?
///
/// Sì quando l'ultima ricostruzione intera è stata fatta con la regola di
/// adesso: vedi [`VERSIONE_AGGREGATI`]. Una riga di stato che manca vale «no».
pub(super) fn aggregati_in_regola(connection: &Connection) -> Result<bool, AppError> {
    use rusqlite::OptionalExtension as _;
    connection
        .query_row(
            "SELECT aggregati FROM libreria_stato WHERE id = 1",
            [],
            |riga| riga.get::<_, i64>(0),
        )
        .optional()
        .map(|versione| versione == Some(VERSIONE_AGGREGATI))
        .map_err(|err| db_error("versione degli aggregati", &err))
}

/// Il segno dello stato di `tracks`: cambia a ogni scrittura, da chiunque venga.
///
/// Lo cambiano tre trigger della migrazione 030, su inserimento, aggiornamento
/// e cancellazione: nessun modulo che scrive deve ricordarsene, ed è il motivo
/// per cui una cache che lo usa come chiave non può restare indietro. Il valore
/// è a caso e non un contatore — la ragione sta nel file della migrazione — e
/// va solo confrontato per uguaglianza: uguale vuol dire «nessuno ha scritto un
/// brano da allora».
///
/// # Prima, non dopo
///
/// Chi lo usa come chiave di una cache lo legge **prima** di calcolare il
/// valore da ricordare. Letto dopo, una scrittura arrivata nel mezzo da
/// un'altra connessione farebbe ricordare il risultato vecchio sotto il segno
/// nuovo, cioè proprio il ritardo che il segno esiste per impedire; letto
/// prima, nello stesso caso si ricorda il risultato nuovo sotto un segno che
/// nessuno rivedrà più, e costa al massimo un calcolo in più.
///
/// # Errori
///
/// `db.*` se la lettura fallisce.
pub fn generazione(connection: &Connection) -> Result<i64, AppError> {
    connection
        .prepare_cached("SELECT generazione FROM libreria_stato WHERE id = 1")
        .and_then(|mut statement| statement.query_row([], |riga| riga.get(0)))
        .map_err(|err| db_error("generazione della libreria", &err))
}

/// Chiede che la prossima ricostruzione sia intera.
///
/// È il ripiego per chi ha cambiato la libreria in un modo che le chiavi
/// toccate non raccontano: «Rileggi tutti i tag», che fa rileggere ogni file.
///
/// # Errori
///
/// `db.*` se la scrittura fallisce.
pub fn chiedi_ricostruzione_intera(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute("UPDATE libreria_stato SET aggregati = 0 WHERE id = 1", [])
        .map(|_| ())
        .map_err(|err| db_error("versione degli aggregati", &err))
}

/// Quanti album e quanti artisti ci sono, senza ricostruire niente.
pub(super) fn conta_aggregati(connection: &Connection) -> Result<Aggregates, AppError> {
    connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM albums), (SELECT COUNT(*) FROM artists)",
            [],
            |riga| Ok((riga.get::<_, i64>(0)?, riga.get::<_, i64>(1)?)),
        )
        .map(|(albums, artists)| Aggregates {
            albums: usize::try_from(albums).unwrap_or(0),
            artists: usize::try_from(artists).unwrap_or(0),
        })
        .map_err(|err| db_error("conteggio degli aggregati", &err))
}

/// Rimette in pari `albums` e `artists` dopo una scrittura, rifacendo soltanto
/// quel che la scrittura ha toccato.
///
/// # Quando rifà tutto
///
/// Quando [`Toccati`] lo chiede, quando le chiavi toccate sono più di
/// [`TETTO_PER_CHIAVI`], e quando le tabelle non sono state costruite con la
/// regola di adesso — la prima scansione dopo questa versione, una migrazione,
/// «Rileggi tutti i tag»: vedi [`VERSIONE_AGGREGATI`]. Allora è
/// [`rebuild_aggregates`], identica a prima.
///
/// # Quando non fa niente
///
/// Quando nessuno ha toccato niente: la scansione periodica di una libreria
/// ferma, che fino alla 2.4 rileggeva cinquantamila brani per riscrivere gli
/// stessi album.
///
/// # Perché il risultato è lo stesso della ricostruzione intera
///
/// Gli album si fondono per identificativo — MusicBrainz, Spotify — e un brano
/// nuovo può fondere il suo album con uno che nessuno ha toccato. Non basta
/// quindi rifare le chiavi segnate: si rifà la loro **chiusura**, cioè le
/// chiavi segnate più ogni chiave che condivide con loro un identificativo,
/// finché l'insieme non cresce più. Su quell'insieme la partizione di
/// [`build_album_groups`] è la stessa che darebbe sulla libreria intera — un
/// gruppo fuori non si può fondere con uno dentro senza condividerne un
/// identificativo, e allora sarebbe dentro — e le righe che ne escono sono le
/// stesse, perché ogni scelta del dominio (titolo, artista, copertina, chiave
/// canonica) dipende solo dai membri del gruppo. Una prova in
/// `library/prove_per_chiavi.rs` lo verifica: dopo una serie di modifiche a
/// caso, per chiavi e per intero danno tabelle identiche.
///
/// I gruppi si fondono anche per **chiave canonica**: due a cui tocca la stessa
/// diventano uno, identificativi in comune o no — vedi «Le chiavi canoniche
/// sono uniche» in [`build_album_groups`]. La chiave di un gruppo è un suo
/// identificativo con il prefisso davanti, o una sua chiave di base; quindi due
/// gruppi con la stessa chiave o condividono un identificativo, o la chiave
/// dell'uno è scritta come chiave di base sui brani dell'altro — un `mbrg:X`
/// rimasto da una ricostruzione di prima. Per questo la chiusura percorre quel
/// legame nei due versi: dall'identificativo `X` ai brani sotto `mbrg:X`, e da
/// una chiave `mbrg:X` ai brani che portano `X`. Il secondo verso mancava, e
/// la prova `la_chiusura_va_dalla_chiave_canonica_all_identificativo` dice
/// cosa costava: un disco riletto a metà rifatto con dieci brani su tredici.
///
/// Per gli artisti è più semplice: un nome è nominato da qualche brano o non lo
/// è, e la risposta cambia solo per i nomi che i brani toccati avevano o hanno.
///
/// # La transazione
///
/// La stessa regola di [`rebuild_aggregates`]: legge e poi scrive, quindi va
/// aperta con [`transazione_di_scrittura`].
///
/// # Errori
///
/// `db.*` se una lettura o una scrittura falliscono.
pub fn ricostruisci_aggregati(
    tx: &Transaction<'_>,
    toccati: &Toccati,
) -> Result<Aggregates, AppError> {
    if toccati.tutto || toccati.album.len() > TETTO_PER_CHIAVI || !aggregati_in_regola(tx)? {
        return rebuild_aggregates(tx);
    }
    if toccati.e_vuoto() {
        return conta_aggregati(tx);
    }
    if !ricostruisci_gli_album(tx, &toccati.album)? {
        // La chiusura è diventata mezza libreria: fusioni a catena per
        // identificativi condivisi. Non si è ancora scritto niente, e da qui la
        // strada intera costa meno.
        return rebuild_aggregates(tx);
    }
    ricostruisci_gli_artisti(tx, &toccati.artisti)?;
    conta_aggregati(tx)
}

/// I membri degli album con queste chiavi, con l'identificativo del brano.
fn membri_delle_chiavi(
    connection: &Connection,
    chiavi: &[&str],
) -> Result<Vec<(i64, AlbumMember)>, AppError> {
    let mut fuori = Vec::new();
    for pezzo in chiavi.chunks(PEZZO) {
        // Le stesse colonne, nello stesso ordine, di `rebuild_aggregates`: è
        // `member_from_row` a leggerle. L'identificativo in coda serve solo a
        // rimettere i membri nell'ordine della lettura intera.
        let sql = format!(
            "SELECT t.album_key, t.album, t.album_artist, t.artist, t.year, t.genre,
                    t.cover_art_hash, c.source, c.width, c.height,
                    t.mb_release_group_id, t.mb_release_id, t.spotify_album_id, t.id
             FROM tracks t
             LEFT JOIN cover_art c ON c.hash = t.cover_art_hash
             WHERE t.album_key IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                Ok((riga.get::<_, i64>(13)?, member_from_row(riga)?))
            })
            .map_err(|err| db_error("lettura dei brani per gli album", &err))?;
        for riga in righe {
            fuori.push(riga.map_err(|err| db_error("lettura dei brani per gli album", &err))?);
        }
    }
    Ok(fuori)
}

/// Le chiavi d'album dei brani che portano uno di questi identificativi, o che
/// si chiamano come la chiave canonica che uno di loro produrrebbe.
///
/// Chi chiama ci mette anche gli identificativi scritti nelle chiavi del
/// fronte — `X` per una chiave `mbrg:X`, con [`identificativo_della_chiave`] —
/// e così la stessa lettura copre il verso opposto.
///
/// È il passo della chiusura in [`ricostruisci_aggregati`]. Si guardano **tutti**
/// gli identificativi dei membri, e non solo quello dominante di ogni gruppo
/// come fa [`build_album_groups`]: l'insieme che ne esce è più largo del
/// necessario, e più largo va bene — un gruppo in più rifatto dà la stessa riga
/// di prima — mentre uno in meno sarebbe una fusione persa.
///
/// `TRIM` sui due lati perché il dominio, prima di confrontare, toglie gli
/// spazi in testa e in coda; una lettura che non lo facesse non vedrebbe due
/// gruppi che invece si fondono.
fn chiavi_che_condividono(
    connection: &Connection,
    identificativi: &[&str],
) -> Result<Vec<String>, AppError> {
    let mut fuori = Vec::new();
    for pezzo in identificativi.chunks(PEZZO) {
        let elenco = segnaposti_numerati(pezzo.len());
        let sql = format!(
            "SELECT DISTINCT album_key FROM tracks
              WHERE album_key IS NOT NULL AND album_key <> ''
                AND (TRIM(mb_release_group_id) IN ({elenco})
                     OR TRIM(mb_release_id) IN ({elenco})
                     OR TRIM(spotify_album_id) IN ({elenco}))"
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(|err| db_error("album che si fondono", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                riga.get::<_, String>(0)
            })
            .map_err(|err| db_error("album che si fondono", &err))?;
        for riga in righe {
            fuori.push(riga.map_err(|err| db_error("album che si fondono", &err))?);
        }
        // Le chiavi canoniche che questi identificativi produrrebbero, se ci
        // sono già brani che le portano: una riga rimappata da una
        // ricostruzione di prima, il cui identificativo nel frattempo è
        // cambiato, si fonderebbe per nome e non per identificativo.
        let canoniche: Vec<String> = pezzo
            .iter()
            .flat_map(|id| {
                [
                    format!("mbrg:{id}"),
                    format!("mbr:{id}"),
                    format!("sp:{id}"),
                ]
            })
            .collect();
        for gruppo in canoniche.chunks(PEZZO) {
            let sql = format!(
                "SELECT DISTINCT album_key FROM tracks WHERE album_key IN ({})",
                segnaposti(gruppo.len())
            );
            let mut statement = connection
                .prepare_cached(&sql)
                .map_err(|err| db_error("album che si fondono", &err))?;
            let righe = statement
                .query_map(rusqlite::params_from_iter(gruppo), |riga| {
                    riga.get::<_, String>(0)
                })
                .map_err(|err| db_error("album che si fondono", &err))?;
            for riga in righe {
                fuori.push(riga.map_err(|err| db_error("album che si fondono", &err))?);
            }
        }
    }
    Ok(fuori)
}

/// L'identificativo da cui è nata una chiave canonica: `X` per `mbrg:X`,
/// `mbr:X` e `sp:X`; `None` per una chiave di base.
///
/// È la strada inversa dei tre `format!` di [`chiavi_che_condividono`], con gli
/// stessi prefissi di `build_album_groups`. Una chiave di base che per caso
/// cominciasse con uno dei tre — un album che si chiama «SP: Live» — dà un
/// identificativo che nessun brano porta: la chiusura cerca una volta in più e
/// non trova niente, che è il lato giusto da cui sbagliare.
fn identificativo_della_chiave(chiave: &str) -> Option<&str> {
    ["mbrg:", "mbr:", "sp:"]
        .into_iter()
        .find_map(|prefisso| chiave.strip_prefix(prefisso))
}

/// Rifà le righe di `albums` delle chiavi toccate e di quelle che con loro si
/// fondono. Vedi [`ricostruisci_aggregati`].
///
/// `Ok(false)` vuol dire «ho rinunciato, e non ho scritto niente»: la chiusura
/// ha superato [`TETTO_PER_CHIAVI`] o metà degli album, cioè gli identificativi
/// condivisi legano mezza libreria — un'importazione che ha dato lo stesso
/// gruppo di pubblicazione a cento dischi diversi — e a quel punto la
/// ricostruzione intera costa meno di inseguirli. Chi chiama la fa lui.
fn ricostruisci_gli_album(
    tx: &Transaction<'_>,
    toccate: &std::collections::BTreeSet<String>,
) -> Result<bool, AppError> {
    use std::collections::{BTreeMap, BTreeSet, HashSet};

    // Oltre quante chiavi la chiusura non conviene più: il tetto fisso, o metà
    // degli album che ci sono, se è meno. Una chiusura che abbraccia mezza
    // libreria — identificativi condivisi a catena — costa più della lettura
    // intera che vorrebbe evitare.
    let tetto = TETTO_PER_CHIAVI.min(conta_aggregati(tx)?.albums.div_euclid(2).max(1));

    // ── la chiusura: le chiavi toccate, e chi si fonde con loro ──
    let mut chiavi: BTreeSet<String> = toccate.iter().filter(|k| !k.is_empty()).cloned().collect();
    let mut membri: BTreeMap<i64, AlbumMember> = BTreeMap::new();
    let mut cercati: HashSet<String> = HashSet::new();
    let mut fronte: Vec<String> = chiavi.iter().cloned().collect();
    while !fronte.is_empty() {
        let fronte_ref: Vec<&str> = fronte.iter().map(String::as_str).collect();
        let nuovi = membri_delle_chiavi(tx, &fronte_ref)?;
        let mut identificativi: Vec<String> = Vec::new();
        for (_, membro) in &nuovi {
            for id in [
                membro.mb_release_group_id.as_deref(),
                membro.mb_release_id.as_deref(),
                membro.spotify_album_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let id = id.trim();
                if !id.is_empty() && cercati.insert(id.to_owned()) {
                    identificativi.push(id.to_owned());
                }
            }
        }
        // E l'identificativo scritto nella chiave stessa. Brani rimasti sotto
        // `mbrg:X` da una ricostruzione di prima, che di identificativi non ne
        // portano più, si fondono per chiave canonica con chi ha `X` come
        // dominante — vedi `build_album_groups` — e dalle loro righe a quelle
        // l'unica strada è la chiave.
        for id in fronte
            .iter()
            .filter_map(|chiave| identificativo_della_chiave(chiave))
        {
            let id = id.trim();
            if !id.is_empty() && cercati.insert(id.to_owned()) {
                identificativi.push(id.to_owned());
            }
        }
        membri.extend(nuovi);
        let identificativi_ref: Vec<&str> = identificativi.iter().map(String::as_str).collect();
        fronte = chiavi_che_condividono(tx, &identificativi_ref)?
            .into_iter()
            .filter(|chiave| chiavi.insert(chiave.clone()))
            .collect();
        if chiavi.len() > tetto {
            return Ok(false);
        }
    }

    // ── la regola del dominio, sugli stessi membri nello stesso ordine ──
    let membri: Vec<AlbumMember> = membri.into_values().collect();
    let groups = build_album_groups(&membri);
    rimappa_le_chiavi(tx, &groups.remap)?;

    // Via le righe di prima — con le chiavi della chiusura — e quelle che
    // portano una chiave canonica che sta per essere riscritta: una riga
    // rimasta indietro con quel nome farebbe fallire l'inserimento.
    let da_togliere: Vec<&str> = chiavi
        .iter()
        .map(String::as_str)
        .chain(groups.albums.iter().map(|album| album.album_key.as_str()))
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .collect();
    for pezzo in da_togliere.chunks(PEZZO) {
        let sql = format!(
            "DELETE FROM albums WHERE album_key IN ({})",
            segnaposti(pezzo.len())
        );
        tx.execute(&sql, rusqlite::params_from_iter(pezzo))
            .map_err(|err| db_error("svuotamento degli album toccati", &err))?;
    }
    scrivi_gli_album(tx, &groups.albums)?;
    Ok(true)
}

/// Riscrive `tracks.album_key` dalle chiavi di base alle canoniche, **tutte
/// insieme**.
///
/// # Il guasto che c'era
///
/// Fino alla 2.4 il rimappaggio era un `UPDATE … WHERE album_key = da` per
/// coppia, una dopo l'altra in ordine alfabetico. Le coppie però possono
/// incatenarsi: un disco che la volta prima era diventato `mbrg:A` e adesso è
/// `mbrg:B`, e nella stessa passata un altro gruppo la cui chiave canonica
/// diventa proprio `mbrg:A`. Applicata prima la seconda coppia, la prima
/// ripescava anche i brani appena arrivati in `mbrg:A` e li portava in
/// `mbrg:B`: brani sotto un album che li conta altrove, e una scheda d'album
/// con un brano in meno e uno fantasma in più. Raro — servono due gruppi che si
/// scambiano un identificativo nella stessa passata — e la prova di parità per
/// chiavi lo ha trovato in due passi.
///
/// # Come
///
/// Con un `UPDATE` solo, guidato da una tabella temporanea di coppie: ogni riga
/// prende la destinazione della **sua** chiave d'origine, letta una volta
/// sola, e nessuna può essere toccata due volte — la catena qui sopra non ha
/// modo di formarsi, perché nessuna istruzione viene dopo un'altra.
///
/// Fino alla 3.0.2 erano due `UPDATE` per coppia, con un nome di passaggio in
/// mezzo (`\u{1}` davanti alla destinazione) per spezzare la catena. Corretti,
/// ma ogni riga si riscriveva due volte, e su una ricostruzione intera dopo
/// «Rileggi tutti i tag» — seimila brani in cinquecento album da riportare
/// alla chiave canonica, misurati su una libreria vera — erano mille
/// istruzioni e più di metà del tempo: la ricostruzione passava da 60 ms a
/// 330, ancora 208 con l'indice di ricerca che non segue più `album_key`
/// (migrazione 033), e 114 così.
fn rimappa_le_chiavi(tx: &Transaction<'_>, remap: &[(String, String)]) -> Result<(), AppError> {
    let cambi: Vec<&(String, String)> = remap.iter().filter(|(da, a)| da != a).collect();
    if cambi.is_empty() {
        return Ok(());
    }
    let fallito = |err: &rusqlite::Error| db_error("rimappaggio delle chiavi d'album", err);
    // Temporanea: sta nella connessione, non nel file, e non la vede nessun
    // altro. Si svuota prima e dopo, così una ricostruzione caduta a metà non
    // lascia coppie a quella dopo.
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS rimappa_chiavi (da TEXT PRIMARY KEY, a TEXT NOT NULL);
         DELETE FROM temp.rimappa_chiavi;",
    )
    .map_err(|err| fallito(&err))?;
    {
        let mut coppia = tx
            .prepare_cached("INSERT OR REPLACE INTO temp.rimappa_chiavi (da, a) VALUES (?1, ?2)")
            .map_err(|err| fallito(&err))?;
        for (da, a) in &cambi {
            coppia
                .execute(rusqlite::params![da, a])
                .map_err(|err| fallito(&err))?;
        }
    }
    tx.execute(
        "UPDATE tracks
            SET album_key = (SELECT a FROM temp.rimappa_chiavi WHERE da = tracks.album_key)
          WHERE album_key IN (SELECT da FROM temp.rimappa_chiavi)",
        [],
    )
    .map_err(|err| fallito(&err))?;
    tx.execute("DELETE FROM temp.rimappa_chiavi", [])
        .map_err(|err| fallito(&err))?;
    Ok(())
}

/// Inserisce le righe di `albums` date. La stessa istruzione per la
/// ricostruzione intera e per quella per chiavi.
fn scrivi_gli_album(tx: &Transaction<'_>, albums: &[AlbumRow]) -> Result<(), AppError> {
    let mut statement = tx
        .prepare_cached(
            "INSERT INTO albums (album_key, title, artist, year, genre, total_tracks,
                                 cover_art_hash, mb_album_id, spotify_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .map_err(|err| db_error("scrittura di un album", &err))?;
    for album in albums {
        let AlbumRow {
            album_key,
            title,
            artist,
            year,
            genre,
            total_tracks,
            cover_art_hash,
            mb_album_id,
            spotify_id,
        } = album;
        statement
            .execute(rusqlite::params![
                album_key,
                title,
                artist,
                year,
                genre,
                i64::try_from(*total_tracks).unwrap_or(i64::MAX),
                cover_art_hash,
                mb_album_id,
                spotify_id,
            ])
            .map_err(|err| db_error("scrittura di un album", &err))?;
    }
    Ok(())
}

/// Rifà la presenza in `artists` dei nomi toccati: c'è chi è ancora nominato da
/// un brano, se ne va chi non lo è più. Vedi [`ricostruisci_aggregati`].
///
/// Le righe che restano non si riscrivono: `INSERT OR IGNORE`, come nella
/// ricostruzione intera, perché `bio`, `image_hash` e gli identificativi
/// esterni arrivano dall'arricchimento e non dai file.
pub(super) fn ricostruisci_gli_artisti(
    tx: &Transaction<'_>,
    toccati: &std::collections::BTreeSet<String>,
) -> Result<(), AppError> {
    let nomi: Vec<&str> = toccati.iter().map(String::as_str).collect();
    let mut presenti: std::collections::HashSet<String> = std::collections::HashSet::new();
    for pezzo in nomi.chunks(PEZZO) {
        // Scritta con [`ARTISTA_EFFETTIVO`] e non a mano: è l'espressione su
        // cui la migrazione 030 ha messo l'indice, e SQLite lo usa solo per una
        // domanda scritta identica.
        let sql = format!(
            "SELECT DISTINCT {ARTISTA_EFFETTIVO} FROM tracks
              WHERE {ARTISTA_EFFETTIVO} IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = tx
            .prepare_cached(&sql)
            .map_err(|err| db_error("artisti ancora nominati", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                riga.get::<_, String>(0)
            })
            .map_err(|err| db_error("artisti ancora nominati", &err))?;
        for riga in righe {
            presenti.insert(riga.map_err(|err| db_error("artisti ancora nominati", &err))?);
        }
    }
    let mut inserisci = tx
        .prepare_cached("INSERT OR IGNORE INTO artists (name) VALUES (?1)")
        .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
    let mut togli = tx
        .prepare_cached("DELETE FROM artists WHERE name = ?1")
        .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
    for nome in nomi {
        if presenti.contains(nome) {
            // Il nome vuoto non entra, come nella ricostruzione intera.
            if !nome.is_empty() {
                inserisci
                    .execute([nome])
                    .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
            }
        } else {
            togli
                .execute([nome])
                .map_err(|err| db_error("ricostruzione degli artisti", &err))?;
        }
    }
    Ok(())
}
