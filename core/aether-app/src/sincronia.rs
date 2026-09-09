//! Il ponte fra il database e la fusione.
//!
//! `aether-sync` decide cosa sopravvive quando due dispositivi si incontrano, e
//! per farlo non ha bisogno di sapere che esiste SQLite: riceve documenti e ne
//! restituisce uno stato. Questo modulo è l'unico posto in tutto l'albero che
//! sa tradurre fra le due lingue — righe da una parte, mappe dall'altra.
//!
//! Tre funzioni, in quest'ordine e per un motivo:
//!
//! 1. [`allinea`] porta le tabelle della sincronia in pari con la libreria. È
//!    l'unica scrittura che avviene *prima* di guardare fuori, e serve perché il
//!    resto dell'applicazione continua a scrivere dove ha sempre scritto: chi
//!    aggiunge un brano a una playlist tocca `playlist_tracks`, non
//!    `sync_sequenza`, e ha ragione a farlo.
//! 2. [`contenuto`] costruisce il documento da pubblicare. Sola lettura.
//! 3. [`applica`] scrive quel che la fusione ha deciso.
//!
//! Fra la seconda e la terza c'è la rete, e il lucchetto della libreria **non**
//! ci sta attorno. È la stessa disciplina che `nuvola.rs` documenta da tempo, e
//! la ragione per cui queste sono tre funzioni invece di una.
//!
//! # Perché l'allineamento è pigro e non un obbligo per chi scrive
//!
//! L'alternativa sarebbe far scrivere `sync_ascolti` e `sync_sequenza` a ogni
//! percorso che tocca la libreria: l'importazione dal vecchio database, quella
//! da un account, la scansione, i comandi delle playlist. Sono una dozzina di
//! posti, e ognuno di loro sarebbe un posto in cui *dimenticarsene* — con il
//! sintomo che compare settimane dopo, su un dispositivo diverso, come un
//! conteggio che non torna.
//!
//! Qui invece la verità resta una sola: `tracks.play_count` e `playlist_tracks`
//! restano ciò che sono sempre stati, e la sincronia si allinea a loro. Chi
//! importa non cambia di una riga. Il prezzo è una passata in più sui dati
//! all'inizio di ogni sincronizzazione, ed è un prezzo in millisecondi.
//!
//! # Cosa questo modulo NON fa, e non è una dimenticanza
//!
//! Non apre file, non parla con la rete e non decide niente. Il deposito è un
//! [`aether_sync::Magazzino`], la decisione è in [`aether_sync::fondi`], e chi
//! mette insieme le tre cose è la finestra. Qui c'è solo la traduzione.

use std::collections::{BTreeMap, BTreeSet};

use aether_domain::errors::AppError;
use aether_sync::contatore::IMPORTAZIONE;
use aether_sync::{
    Contenuto, Elemento, Fuso, Identita, Interruttore, Lapidi, Momento, PlaylistSincronizzata,
    Scelta, Sequenza, Voto,
};
use rusqlite::{Connection, Transaction};

use crate::library::db_error;
use crate::settings::{
    self, CHIAVE_CARTELLE, CHIAVE_CARTELLE_SINCRONIA, CHIAVE_SKIN, CHIAVE_SKIN_QUANDO,
};

/// Il dispositivo fittizio con cui si datano le sequenze già esistenti.
///
/// Una playlist nata prima che ci fosse una sincronia non ha identità per i suoi
/// elementi, e bisogna dargliele. La tentazione è usare il proprio nome di
/// dispositivo; sarebbe sbagliato, e in un modo che si vede solo al primo
/// incontro fra due macchine.
///
/// Due dispositivi che hanno la **stessa** playlist — perché l'hanno importata
/// dallo stesso vecchio database — la battezzerebbero ciascuno a modo proprio,
/// `portatile:0…9` e `telefono:0…9`. Sono venti elementi distinti per dieci
/// brani, e la fusione li terrebbe tutti: la playlist raddoppierebbe alla prima
/// passata, con ogni canzone due volte.
///
/// Con un nome uguale per tutti le identità coincidono, l'unione è un'operazione
/// a vuoto, e la playlist resta di dieci. Dove i due elenchi differiscono davvero
/// resta la parte comune e si aggiunge il resto, che è il comportamento voluto.
const ORIGINE: &str = "origine";

// ── il conteggio d'ascolto ──────────────────────────────────────────────────

/// Segna un ascolto a nome di questo dispositivo.
///
/// Due scritture e non una: sale il contatore di chi ha ascoltato, e
/// `tracks.play_count` si rifà come somma. La seconda esiste perché tutte le
/// interrogazioni della libreria continuano a leggere quella colonna e non
/// devono accorgersi di niente — sono più di cento, e riscriverle sarebbe stato
/// il modo di introdurre cento occasioni di sbagliare per un cambiamento che qui
/// costa una riga.
///
/// La somma è su `track_key` e non su `id`: due file dello stesso brano sono lo
/// stesso brano, ed è già così che `backup.rs` li tratta quando salva.
///
/// # Errori
///
/// `db.queryFailed` se una delle due scritture fallisce.
pub fn conta_ascolto(
    tx: &Transaction<'_>,
    track_id: i64,
    dispositivo: &str,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO sync_ascolti (track_key, dispositivo, quanti)
         SELECT track_key, ?2, 1 FROM tracks WHERE id = ?1
         ON CONFLICT(track_key, dispositivo) DO UPDATE SET quanti = quanti + 1",
        rusqlite::params![track_id, dispositivo],
    )
    .map_err(|err| db_error("conteggio per dispositivo", &err))?;

    tx.execute(
        "UPDATE tracks
            SET play_count = COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                        WHERE a.track_key = tracks.track_key), 0)
          WHERE track_key = (SELECT track_key FROM tracks WHERE id = ?1)",
        rusqlite::params![track_id],
    )
    .map_err(|err| db_error("somma degli ascolti", &err))?;
    Ok(())
}

/// Ricorda dove si era arrivati in un brano lasciato a metà.
///
/// Solo il più recente vince, e non il più avanti: chi ha riascoltato ieri sa
/// meglio dove si è fermato di chi il mese scorso era andato più in là.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn segna_posizione(
    connection: &Connection,
    track_key: &str,
    ms: i64,
    adesso_ms: i64,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO sync_posizioni (track_key, ms, at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_key) DO UPDATE SET ms = excluded.ms, at_ms = excluded.at_ms
             WHERE excluded.at_ms >= sync_posizioni.at_ms",
            rusqlite::params![track_key, ms.max(0), adesso_ms],
        )
        .map_err(|err| db_error("posizione di riascolto", &err))?;
    Ok(())
}

// ── allineamento ────────────────────────────────────────────────────────────

/// Quante cose l'allineamento ha dovuto mettere in pari.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Allineato {
    /// Brani il cui storico ereditato è cambiato.
    pub ascolti: usize,
    /// Playlist la cui sequenza è stata rifatta.
    pub sequenze: usize,
    /// Cartelle accese o spente da qui.
    pub cartelle: usize,
}

impl Allineato {
    /// Non c'era niente da mettere in pari.
    #[must_use]
    pub const fn e_vuoto(&self) -> bool {
        self.ascolti == 0 && self.sequenze == 0 && self.cartelle == 0
    }
}

/// Porta le tabelle della sincronia in pari con la libreria.
///
/// Da chiamare all'inizio di una passata, prima di [`contenuto`]. Su una
/// libreria ferma non scrive niente e costa tre interrogazioni.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub fn allinea(
    connection: &mut Connection,
    dispositivo: &str,
    adesso_ms: i64,
) -> Result<Allineato, AppError> {
    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura dell'allineamento", &err))?;

    let mut fatto = Allineato {
        ascolti: allinea_ascolti(&tx)?,
        sequenze: allinea_sequenze(&tx, adesso_ms)?,
        cartelle: 0,
    };
    fatto.cartelle = allinea_cartelle(&tx, adesso_ms)?;

    tx.commit()
        .map_err(|err| db_error("chiusura dell'allineamento", &err))?;
    let _ = dispositivo;
    Ok(fatto)
}

/// Riporta sotto `importazione` tutto lo storico che nessun dispositivo rivendica.
///
/// `tracks.play_count` può crescere per vie che la sincronia non vede:
/// un'importazione dal vecchio database, o l'archivio che un servizio spedisce.
/// Quella crescita è storia vera e non va persa, ma non appartiene a questo
/// dispositivo — appartiene a `importazione`, che è lo stesso pseudo-dispositivo
/// ovunque e si fonde col massimo.
///
/// Il calcolo è una sottrazione: quel che la colonna dice, meno quel che i
/// dispositivi rivendicano, è quel che resta da attribuire. Non scende mai —
/// `max` nell'upsert — perché una diminuzione qui significherebbe buttare via
/// ascolti sulla base di un numero che potrebbe essere solo arrivato tardi.
fn allinea_ascolti(tx: &Transaction<'_>) -> Result<usize, AppError> {
    let toccate = tx
        .execute(
            "INSERT INTO sync_ascolti (track_key, dispositivo, quanti)
             SELECT chiave, ?1, eccedenza FROM (
               SELECT t.track_key AS chiave,
                      MAX(t.play_count) - COALESCE((
                        SELECT SUM(a.quanti) FROM sync_ascolti a
                         WHERE a.track_key = t.track_key AND a.dispositivo <> ?1
                      ), 0) AS eccedenza
                 FROM tracks t
                GROUP BY t.track_key
             )
             WHERE eccedenza > 0
             ON CONFLICT(track_key, dispositivo) DO UPDATE SET
               quanti = max(sync_ascolti.quanti, excluded.quanti)",
            [IMPORTAZIONE],
        )
        .map_err(|err| db_error("storico ereditato", &err))?;

    // Il `WHERE` non è un'ottimizzazione da manuale: `tracks` ha un trigger che
    // rifà l'indice di ricerca a **ogni** UPDATE, anche quando il valore scritto
    // è identico a quello che c'era. Senza la guardia, un allineamento a vuoto su
    // diecimila brani ricostruirebbe diecimila righe di FTS ogni volta.
    tx.execute(
        "UPDATE tracks
            SET play_count = COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                        WHERE a.track_key = tracks.track_key), 0)
          WHERE play_count <> COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                         WHERE a.track_key = tracks.track_key), 0)",
        [],
    )
    .map_err(|err| db_error("somma degli ascolti", &err))?;
    Ok(toccate)
}

/// Le sequenze delle playlist, in pari con `playlist_tracks`.
///
/// Chi aggiunge o sposta un brano scrive `playlist_tracks` e non sa che esista
/// una sequenza; qui si guarda la differenza fra l'ordine che la sequenza
/// produce e quello che la tabella dice, e la si riversa con
/// [`Sequenza::riordina`] — che tocca solo ciò che si è mosso davvero invece di
/// rifare tutto, e quindi non lascia una lapide per brano a ogni trascinamento.
fn allinea_sequenze(tx: &Transaction<'_>, adesso_ms: i64) -> Result<usize, AppError> {
    let membri = membri_per_playlist(tx)?;
    let mut sequenze = sequenze_salvate(tx)?;

    let mut rifatte = 0;
    for (chiave, brani) in &membri {
        let sequenza = sequenze.remove(chiave).unwrap_or_else(Sequenza::vuota);
        let mut aggiornata = sequenza.clone();
        if aggiornata.e_vuota() {
            // Il primo battesimo. Vedi [`ORIGINE`] sul perché non porta il nome
            // di questo dispositivo.
            aggiornata = Sequenza::dalla_lista(ORIGINE, brani);
        } else {
            let atteso: Vec<&str> = brani.iter().map(String::as_str).collect();
            if aggiornata.ordine() == atteso {
                continue;
            }
            aggiornata.riordina(ORIGINE, brani, adesso_ms);
        }
        if aggiornata == sequenza {
            continue;
        }
        scrivi_sequenza(tx, chiave, &aggiornata)?;
        rifatte += 1;
    }

    // Le sequenze rimaste sono di playlist che non hanno più membri: o sono state
    // svuotate, o la playlist non c'è più. Nel primo caso vanno sepolti tutti gli
    // elementi, o al prossimo giro i brani tornerebbero da soli.
    for (chiave, sequenza) in sequenze {
        if sequenza.ordine().is_empty() {
            continue;
        }
        let mut vuotata = sequenza.clone();
        vuotata.riordina(ORIGINE, &[], adesso_ms);
        scrivi_sequenza(tx, &chiave, &vuotata)?;
        rifatte += 1;
    }
    Ok(rifatte)
}

/// Le cartelle sorvegliate come mappa datata, in pari con l'elenco.
///
/// L'elenco è la verità su questo computer; la mappa è ciò che di quella verità
/// si può raccontare a un altro dispositivo. Una cartella mai vista prima entra
/// con data **zero** e non con l'ora attuale, ed è la differenza fra una
/// sincronia che funziona e una che rimette le cartelle appena tolte: al primo
/// avvio nessuna delle cartelle già configurate è una *decisione* presa adesso, e
/// datarle adesso le farebbe vincere su una rimozione fatta ieri sul portatile.
fn allinea_cartelle(tx: &Transaction<'_>, adesso_ms: i64) -> Result<usize, AppError> {
    let elenco: Vec<String> = settings::read_json(tx, CHIAVE_CARTELLE)?.unwrap_or_default();
    let mut mappa: BTreeMap<String, Interruttore> =
        settings::read_json(tx, CHIAVE_CARTELLE_SINCRONIA)?.unwrap_or_default();
    let prima_volta = mappa.is_empty();
    let presenti: BTreeSet<&str> = elenco.iter().map(String::as_str).collect();

    let mut cambiate = 0;
    for percorso in &elenco {
        match mappa.get(percorso) {
            Some(stato) if stato.on => {}
            Some(_) => {
                mappa.insert(
                    percorso.clone(),
                    Interruttore {
                        on: true,
                        at: adesso_ms,
                    },
                );
                cambiate += 1;
            }
            None => {
                mappa.insert(
                    percorso.clone(),
                    Interruttore {
                        on: true,
                        at: if prima_volta { 0 } else { adesso_ms },
                    },
                );
                cambiate += 1;
            }
        }
    }
    for (percorso, stato) in &mut mappa {
        if stato.on && !presenti.contains(percorso.as_str()) {
            *stato = Interruttore {
                on: false,
                at: adesso_ms,
            };
            cambiate += 1;
        }
    }

    if cambiate > 0 {
        settings::write_json(tx, CHIAVE_CARTELLE_SINCRONIA, &mappa)?;
    }
    Ok(cambiate)
}

// ── dal database al documento ───────────────────────────────────────────────

/// Costruisce il documento di questo dispositivo.
///
/// `skin` e `bozze` arrivano da fuori, come già fa `backup::snapshot`: i
/// pacchetti stanno su disco e questo crate non li apre.
///
/// # Cosa non entra nel documento
///
/// I brani **senza niente da dire**. Un brano mai ascoltato, senza voto e senza
/// preferito non porta informazione che una fusione possa usare, e su una
/// libreria appena scansionata sono la quasi totalità: è la differenza fra un
/// documento da settanta chilobyte e uno da mezzo megabyte, riscritto a ogni
/// passata.
///
/// # Errori
///
/// `db.queryFailed` se una lettura fallisce.
pub fn contenuto(
    connection: &Connection,
    dispositivo: &str,
    skin: BTreeMap<String, String>,
    bozze: BTreeMap<String, String>,
) -> Result<Contenuto, AppError> {
    Ok(Contenuto {
        ascolti: ascolti_di(connection, dispositivo)?,
        ereditati: ascolti_di(connection, IMPORTAZIONE)?,
        ultimo: ultimi_ascolti(connection)?,
        voti: voti(connection)?,
        preferiti: preferiti(connection)?,
        posizioni: posizioni(connection)?,
        playlist: playlist(connection)?,
        lapidi: lapidi(connection)?,
        cartelle: settings::read_json(connection, CHIAVE_CARTELLE_SINCRONIA)?.unwrap_or_default(),
        skin,
        bozze,
        skin_attiva: settings::read(connection, CHIAVE_SKIN)?.map(|id| Scelta {
            id,
            at: settings::read(connection, CHIAVE_SKIN_QUANDO)
                .ok()
                .flatten()
                .and_then(|quando| quando.parse().ok())
                .unwrap_or(0),
        }),
    })
}

/// Gli ascolti attribuiti a un dispositivo.
fn ascolti_di(
    connection: &Connection,
    dispositivo: &str,
) -> Result<BTreeMap<String, i64>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key, quanti FROM sync_ascolti WHERE dispositivo = ?1 AND quanti > 0")
        .map_err(|err| db_error("ascolti da pubblicare", &err))?;
    let righe = statement
        .query_map([dispositivo], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| db_error("ascolti da pubblicare", &err))?;
    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, quanti) = riga.map_err(|err| db_error("ascolti da pubblicare", &err))?;
        mappa.insert(chiave, quanti);
    }
    Ok(mappa)
}

/// L'ultimo ascolto di ciascun brano.
fn ultimi_ascolti(connection: &Connection) -> Result<BTreeMap<String, i64>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT track_key, MAX(last_played_at) FROM tracks
              WHERE last_played_at IS NOT NULL GROUP BY track_key",
        )
        .map_err(|err| db_error("ultimi ascolti", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| db_error("ultimi ascolti", &err))?;
    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, quando) = riga.map_err(|err| db_error("ultimi ascolti", &err))?;
        mappa.insert(chiave, quando);
    }
    Ok(mappa)
}

/// I voti, con la data della decisione.
///
/// Solo le righe con `rating_at`: una riga senza è un brano su cui **nessuno ha
/// mai deciso niente**, ed è diverso da uno votato zero. Spedire uno zero non
/// datato come se fosse una decisione toglierebbe le stelle sull'altro
/// dispositivo, che è esattamente il guasto che `merge_rating` evita trattando
/// lo zero come assenza.
fn voti(connection: &Connection) -> Result<BTreeMap<String, Voto>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key, rating, rating_at FROM tracks WHERE rating_at IS NOT NULL")
        .map_err(|err| db_error("voti da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| db_error("voti da pubblicare", &err))?;
    let mut mappa: BTreeMap<String, Voto> = BTreeMap::new();
    for riga in righe {
        let (chiave, stelle, quando) = riga.map_err(|err| db_error("voti da pubblicare", &err))?;
        let voto = Voto {
            v: u8::try_from(stelle.clamp(0, 5)).unwrap_or(0),
            at: quando,
        };
        // Due file dello stesso brano: vince la decisione più recente, che è la
        // stessa regola con cui i due si fonderebbero fra dispositivi.
        mappa
            .entry(chiave)
            .and_modify(|gia| {
                if voto.at > gia.at {
                    *gia = voto;
                }
            })
            .or_insert(voto);
    }
    Ok(mappa)
}

/// I preferiti, messi e tolti.
///
/// `liked_at` c'è dalla migrazione 1 e data la **decisione**: è la ragione per
/// cui un «non mi piace più» viaggia già oggi invece di essere riassorbito dal
/// cuoricino più vecchio dell'altro dispositivo.
fn preferiti(connection: &Connection) -> Result<BTreeMap<String, Interruttore>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key, liked, liked_at FROM tracks WHERE liked_at IS NOT NULL")
        .map_err(|err| db_error("preferiti da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| db_error("preferiti da pubblicare", &err))?;
    let mut mappa: BTreeMap<String, Interruttore> = BTreeMap::new();
    for riga in righe {
        let (chiave, acceso, quando) =
            riga.map_err(|err| db_error("preferiti da pubblicare", &err))?;
        let stato = Interruttore {
            on: acceso != 0,
            at: quando,
        };
        mappa
            .entry(chiave)
            .and_modify(|gia| {
                if stato.at > gia.at {
                    *gia = stato;
                }
            })
            .or_insert(stato);
    }
    Ok(mappa)
}

/// Dove si era arrivati nei brani lasciati a metà.
fn posizioni(connection: &Connection) -> Result<BTreeMap<String, Momento>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key, ms, at_ms FROM sync_posizioni")
        .map_err(|err| db_error("posizioni da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Momento {
                    ms: row.get(1)?,
                    at: row.get(2)?,
                },
            ))
        })
        .map_err(|err| db_error("posizioni da pubblicare", &err))?;
    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, momento) = riga.map_err(|err| db_error("posizioni da pubblicare", &err))?;
        mappa.insert(chiave, momento);
    }
    Ok(mappa)
}

/// Le playlist con la loro sequenza.
fn playlist(connection: &Connection) -> Result<BTreeMap<String, PlaylistSincronizzata>, AppError> {
    let mut sequenze = sequenze_salvate(connection)?;
    let mut statement = connection
        .prepare(
            "SELECT playlist_key, name, description, created_at, updated_at, is_smart, rules
               FROM playlists",
        )
        .map_err(|err| db_error("playlist da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            let smart: i64 = row.get(5)?;
            Ok((
                row.get::<_, String>(0)?,
                PlaylistSincronizzata {
                    nome: row.get(1)?,
                    descrizione: row.get(2)?,
                    creata_at: row.get(3)?,
                    at: row.get(4)?,
                    smart: smart != 0,
                    regole: row.get(6)?,
                    sequenza: Sequenza::vuota(),
                },
            ))
        })
        .map_err(|err| db_error("playlist da pubblicare", &err))?;

    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, mut playlist) =
            riga.map_err(|err| db_error("playlist da pubblicare", &err))?;
        // Le automatiche non portano appartenenza: ogni dispositivo la ricalcola
        // dalle regole, così resta vera anche sui brani che l'altro non ha.
        if !playlist.smart {
            playlist.sequenza = sequenze.remove(&chiave).unwrap_or_else(Sequenza::vuota);
        }
        mappa.insert(chiave, playlist);
    }
    Ok(mappa)
}

/// Le cancellazioni registrate qui.
fn lapidi(connection: &Connection) -> Result<Lapidi, AppError> {
    let mut statement = connection
        .prepare("SELECT kind, key, deleted_at FROM sync_tombstones")
        .map_err(|err| db_error("lapidi da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| db_error("lapidi da pubblicare", &err))?;
    let mut lapidi = Lapidi::default();
    for riga in righe {
        let (tipo, chiave, quando) = riga.map_err(|err| db_error("lapidi da pubblicare", &err))?;
        match tipo.as_str() {
            "track" => {
                lapidi.brani.insert(chiave, quando);
            }
            "playlist" => {
                lapidi.playlist.insert(chiave, quando);
            }
            _ => {}
        }
    }
    Ok(lapidi)
}

// ── le sequenze su disco ────────────────────────────────────────────────────

/// I membri di ogni playlist non automatica, nell'ordine.
///
/// Una interrogazione per tutte e non una per ciascuna: su quaranta playlist la
/// differenza fra le due forme è quaranta andate e ritorno, e questo codice gira
/// con il lucchetto della libreria in mano.
fn membri_per_playlist(connection: &Connection) -> Result<BTreeMap<String, Vec<String>>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT p.playlist_key, t.track_key
               FROM playlist_tracks pt
               JOIN playlists p ON p.id = pt.playlist_id
               JOIN tracks t    ON t.id = pt.track_id
              WHERE p.is_smart = 0
              ORDER BY p.playlist_key, pt.position",
        )
        .map_err(|err| db_error("membri delle playlist", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| db_error("membri delle playlist", &err))?;
    let mut mappa: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for riga in righe {
        let (playlist, brano) = riga.map_err(|err| db_error("membri delle playlist", &err))?;
        mappa.entry(playlist).or_default().push(brano);
    }

    // Anche le playlist vuote: senza, una playlist svuotata non verrebbe mai
    // confrontata e i suoi elementi resterebbero vivi nella sequenza.
    let mut vuote = connection
        .prepare("SELECT playlist_key FROM playlists WHERE is_smart = 0")
        .map_err(|err| db_error("playlist vuote", &err))?;
    let chiavi = vuote
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| db_error("playlist vuote", &err))?;
    for chiave in chiavi {
        mappa
            .entry(chiave.map_err(|err| db_error("playlist vuote", &err))?)
            .or_default();
    }
    Ok(mappa)
}

/// Le sequenze come stanno scritte.
fn sequenze_salvate(connection: &Connection) -> Result<BTreeMap<String, Sequenza>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT playlist_key, elemento, dopo, track_key, tolto_ms
               FROM sync_sequenza ORDER BY playlist_key",
        )
        .map_err(|err| db_error("sequenze salvate", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })
        .map_err(|err| db_error("sequenze salvate", &err))?;

    let mut per_playlist: BTreeMap<String, Vec<(Identita, Elemento)>> = BTreeMap::new();
    for riga in righe {
        let (playlist, elemento, dopo, brano, tolto) =
            riga.map_err(|err| db_error("sequenze salvate", &err))?;
        // Un'identità illeggibile fa sparire **quell'elemento**, non la playlist:
        // è la stessa indulgenza con cui `interpreta` legge un documento remoto,
        // e per la stessa ragione — perdere un brano è meglio che perdere l'ordine
        // di tutti gli altri.
        let Some(identita) = Identita::da_testo(&elemento) else {
            continue;
        };
        per_playlist.entry(playlist).or_default().push((
            identita,
            Elemento {
                dopo: dopo.as_deref().and_then(Identita::da_testo),
                brano,
                tolto,
            },
        ));
    }
    Ok(per_playlist
        .into_iter()
        .map(|(chiave, elementi)| (chiave, Sequenza::dagli_elementi(elementi)))
        .collect())
}

/// Riscrive la sequenza di una playlist.
fn scrivi_sequenza(
    tx: &Transaction<'_>,
    playlist_key: &str,
    sequenza: &Sequenza,
) -> Result<(), AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_sequenza (playlist_key, elemento, dopo, track_key, tolto_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(playlist_key, elemento) DO UPDATE SET
               dopo      = excluded.dopo,
               track_key = excluded.track_key,
               -- La lapide più vecchia vince: il momento in cui una cosa è stata
               -- tolta la prima volta è un fatto, non un'opinione. È la stessa
               -- regola che `Sequenza::fondi` applica in memoria.
               tolto_ms  = CASE
                             WHEN sync_sequenza.tolto_ms IS NULL THEN excluded.tolto_ms
                             WHEN excluded.tolto_ms IS NULL THEN sync_sequenza.tolto_ms
                             ELSE min(sync_sequenza.tolto_ms, excluded.tolto_ms)
                           END",
        )
        .map_err(|err| db_error("sequenza", &err))?;
    for (identita, elemento) in sequenza.elementi() {
        scrivi
            .execute(rusqlite::params![
                playlist_key,
                identita.testo(),
                elemento.dopo.as_ref().map(Identita::testo),
                elemento.brano,
                elemento.tolto,
            ])
            .map_err(|err| db_error("sequenza", &err))?;
    }
    Ok(())
}

// ── dal fuso al database ────────────────────────────────────────────────────

/// Cosa la sincronia ha cambiato, da raccontare a chi guarda.
///
/// Non è telemetria: è la sola cosa che permette di fidarsi di un automatismo che
/// scrive nella libreria da solo. «142 ascolti da *portatile*, 3 playlist
/// riordinate» è una frase che si può leggere; «sincronizzato» non è.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cambiamenti {
    /// Brani il cui conteggio d'ascolto è cambiato.
    pub ascolti: usize,
    /// Voti arrivati da fuori.
    pub voti: usize,
    /// Cuoricini messi o tolti da fuori.
    pub preferiti: usize,
    /// Posizioni di riascolto aggiornate.
    pub posizioni: usize,
    /// Playlist create o rifatte.
    pub playlist: usize,
    /// Playlist cancellate altrove.
    pub playlist_tolte: usize,
    /// Cartelle sorvegliate aggiunte o tolte.
    pub cartelle: usize,
    /// Dispositivi visti in questa passata.
    pub dispositivi: usize,
}

impl Cambiamenti {
    /// Nessuna scrittura: la libreria era già allineata.
    #[must_use]
    pub const fn e_vuoto(&self) -> bool {
        self.ascolti == 0
            && self.voti == 0
            && self.preferiti == 0
            && self.posizioni == 0
            && self.playlist == 0
            && self.playlist_tolte == 0
            && self.cartelle == 0
    }
}

/// Scrive nella libreria quel che la fusione ha deciso.
///
/// Una transazione sola: o la libreria è quella di prima, o è quella dopo. Una
/// sincronia interrotta a metà che lasciasse gli ascolti nuovi e le playlist
/// vecchie sarebbe indistinguibile, dall'esterno, da una che ha perso dei dati.
///
/// # Perché scrive da sé
///
/// È la decisione che questo ramo ha preso, e va detta dove si vede: la
/// sincronia applica senza chiedere. Regge perché un CRDT non cancella per
/// costruzione — ogni fusione qui dentro è commutativa, associativa e
/// idempotente, e i test di convergenza lo dimostrano invece di affermarlo.
/// L'unica cosa che sparisce è ciò che qualcuno ha cancellato *apposta*, e viaggia
/// come lapide.
///
/// # Errori
///
/// `db.queryFailed` se una scrittura fallisce.
pub fn applica(
    connection: &mut Connection,
    fuso: &Fuso,
    adesso_ms: i64,
) -> Result<Cambiamenti, AppError> {
    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della sincronia", &err))?;

    let mut cambiamenti = Cambiamenti {
        ascolti: applica_ascolti(&tx, fuso)?,
        voti: applica_voti(&tx, fuso)?,
        preferiti: applica_preferiti(&tx, fuso)?,
        posizioni: applica_posizioni(&tx, fuso)?,
        ..Cambiamenti::default()
    };
    applica_ultimi(&tx, fuso)?;
    applica_lapidi(&tx, fuso)?;
    cambiamenti.playlist_tolte = togli_playlist(&tx, fuso)?;
    cambiamenti.playlist = applica_playlist(&tx, fuso)?;
    cambiamenti.cartelle = applica_cartelle(&tx, fuso)?;
    applica_skin(&tx, fuso)?;
    cambiamenti.dispositivi = applica_dispositivi(&tx, fuso, adesso_ms)?;

    tx.commit()
        .map_err(|err| db_error("chiusura della sincronia", &err))?;
    Ok(cambiamenti)
}

/// Gli ascolti di tutti i dispositivi, e la somma in `tracks`.
fn applica_ascolti(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    if fuso.ascolti.is_empty() {
        return Ok(0);
    }
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_ascolti (track_key, dispositivo, quanti) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_key, dispositivo) DO UPDATE SET
               -- Il massimo, mai il valore che arriva: il numero di un dispositivo
               -- sale solo per mano sua, e un documento vecchio che passa di qui
               -- non deve poter far scendere quello che avevamo già letto.
               quanti = max(sync_ascolti.quanti, excluded.quanti)
             WHERE excluded.quanti > sync_ascolti.quanti",
        )
        .map_err(|err| db_error("ascolti ricevuti", &err))?;

    let mut toccati = 0;
    for (brano, contatore) in &fuso.ascolti {
        let mut cambiato = false;
        for (dispositivo, quanti) in contatore.dispositivi() {
            if quanti <= 0 {
                continue;
            }
            let scritte = scrivi
                .execute(rusqlite::params![brano, dispositivo, quanti])
                .map_err(|err| db_error("ascolti ricevuti", &err))?;
            cambiato |= scritte > 0;
        }
        if cambiato {
            toccati += 1;
        }
    }

    if toccati > 0 {
        tx.execute(
            "UPDATE tracks
                SET play_count = COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                            WHERE a.track_key = tracks.track_key), 0)
              WHERE play_count <> COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                             WHERE a.track_key = tracks.track_key), 0)",
            [],
        )
        .map_err(|err| db_error("somma degli ascolti", &err))?;
    }
    Ok(toccati)
}

/// L'ultimo ascolto: il più recente fra i due.
fn applica_ultimi(tx: &Transaction<'_>, fuso: &Fuso) -> Result<(), AppError> {
    let mut scrivi = tx
        .prepare(
            "UPDATE tracks SET last_played_at = ?2
              WHERE track_key = ?1 AND COALESCE(last_played_at, 0) < ?2",
        )
        .map_err(|err| db_error("ultimi ascolti ricevuti", &err))?;
    for (brano, quando) in &fuso.ultimo {
        scrivi
            .execute(rusqlite::params![brano, quando])
            .map_err(|err| db_error("ultimi ascolti ricevuti", &err))?;
    }
    Ok(())
}

/// I voti, con la loro data.
fn applica_voti(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "UPDATE tracks
                SET rating = ?2, rating_at = ?3, stats_updated_at = max(stats_updated_at, ?3)
              WHERE track_key = ?1 AND COALESCE(rating_at, -1) < ?3",
        )
        .map_err(|err| db_error("voti ricevuti", &err))?;
    let mut cambiati = 0;
    for (brano, voto) in &fuso.voti {
        let scritte = scrivi
            .execute(rusqlite::params![brano, i64::from(voto.v), voto.at])
            .map_err(|err| db_error("voti ricevuti", &err))?;
        if scritte > 0 {
            cambiati += 1;
        }
    }
    Ok(cambiati)
}

/// I preferiti, messi e tolti.
fn applica_preferiti(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "UPDATE tracks
                SET liked = ?2, liked_at = ?3, stats_updated_at = max(stats_updated_at, ?3)
              WHERE track_key = ?1 AND COALESCE(liked_at, -1) < ?3",
        )
        .map_err(|err| db_error("preferiti ricevuti", &err))?;
    let mut cambiati = 0;
    for (brano, stato) in &fuso.preferiti {
        let scritte = scrivi
            .execute(rusqlite::params![brano, i64::from(stato.on), stato.at])
            .map_err(|err| db_error("preferiti ricevuti", &err))?;
        if scritte > 0 {
            cambiati += 1;
        }
    }
    Ok(cambiati)
}

/// Dove si era arrivati.
fn applica_posizioni(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_posizioni (track_key, ms, at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_key) DO UPDATE SET ms = excluded.ms, at_ms = excluded.at_ms
             WHERE excluded.at_ms > sync_posizioni.at_ms",
        )
        .map_err(|err| db_error("posizioni ricevute", &err))?;
    let mut cambiate = 0;
    for (brano, momento) in &fuso.posizioni {
        let scritte = scrivi
            .execute(rusqlite::params![brano, momento.ms.max(0), momento.at])
            .map_err(|err| db_error("posizioni ricevute", &err))?;
        if scritte > 0 {
            cambiate += 1;
        }
    }
    Ok(cambiate)
}

/// Le lapidi: entrano tutte, e restano.
///
/// Anche quelle di brani che qui non ci sono mai stati. Una lapide che smette di
/// viaggiare è un brano cancellato che torna: basta un dispositivo rimasto
/// indietro a rimandarlo indietro alla prima passata.
fn applica_lapidi(tx: &Transaction<'_>, fuso: &Fuso) -> Result<(), AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(kind, key) DO UPDATE SET
               deleted_at = min(sync_tombstones.deleted_at, excluded.deleted_at)",
        )
        .map_err(|err| db_error("lapidi ricevute", &err))?;
    for (chiave, quando) in &fuso.lapidi.brani {
        scrivi
            .execute(rusqlite::params!["track", chiave, quando])
            .map_err(|err| db_error("lapidi ricevute", &err))?;
    }
    for (chiave, quando) in &fuso.lapidi.playlist {
        scrivi
            .execute(rusqlite::params!["playlist", chiave, quando])
            .map_err(|err| db_error("lapidi ricevute", &err))?;
    }
    Ok(())
}

/// Toglie le playlist cancellate altrove.
///
/// Solo le playlist, mai i brani — ed è la stessa asimmetria di `backup.rs`, non
/// una distrazione. Una riga di brano non la crea mai una sincronia: la crea la
/// scansione, leggendo un file che c'è. Cancellarla qui vorrebbe dire buttare via
/// una storia d'ascolto senza togliere niente in cambio, e alla scansione
/// successiva il brano tornerebbe comunque, senza la sua storia.
fn togli_playlist(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    if fuso.lapidi.playlist.is_empty() {
        return Ok(0);
    }
    let mut cancella = tx
        .prepare(
            "DELETE FROM playlists WHERE playlist_key = ?1 AND updated_at <= ?2 AND is_smart = 0",
        )
        .map_err(|err| db_error("playlist cancellate", &err))?;
    let mut sgombera = tx
        .prepare("DELETE FROM sync_sequenza WHERE playlist_key = ?1")
        .map_err(|err| db_error("playlist cancellate", &err))?;
    let mut tolte = 0;
    for (chiave, quando) in &fuso.lapidi.playlist {
        // Una playlist rifatta dopo essere stata cancellata sopravvive: la
        // fusione l'ha già tenuta, e qui la si riconosce dal fatto che è più
        // recente della lapide.
        if fuso.playlist.contains_key(chiave) {
            continue;
        }
        let via = cancella
            .execute(rusqlite::params![chiave, quando])
            .map_err(|err| db_error("playlist cancellate", &err))?;
        if via > 0 {
            sgombera
                .execute([chiave])
                .map_err(|err| db_error("playlist cancellate", &err))?;
            tolte += 1;
        }
    }
    Ok(tolte)
}

/// Le playlist: la riga, la sequenza, e l'appartenenza compattata.
fn applica_playlist(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    if fuso.playlist.is_empty() {
        return Ok(0);
    }
    let mut inserisci = tx
        .prepare(
            "INSERT INTO playlists
                 (playlist_key, name, description, created_at, updated_at, is_smart, rules)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(playlist_key) DO UPDATE SET
                 name        = excluded.name,
                 description = excluded.description,
                 -- La data di creazione più antica delle due: è quella vera, e una
                 -- sincronia non deve far sembrare appena nata una playlist che
                 -- esiste da tre anni.
                 created_at  = min(playlists.created_at, excluded.created_at),
                 updated_at  = max(playlists.updated_at, excluded.updated_at),
                 is_smart    = excluded.is_smart,
                 rules       = excluded.rules",
        )
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut identifica = tx
        .prepare("SELECT id FROM playlists WHERE playlist_key = ?1")
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut cerca_brano = tx
        .prepare("SELECT id FROM tracks WHERE track_key = ?1 ORDER BY id LIMIT 1")
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut svuota = tx
        .prepare("DELETE FROM playlist_tracks WHERE playlist_id = ?1")
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut accoda = tx
        .prepare(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        )
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut dissotterra = tx
        .prepare("DELETE FROM sync_tombstones WHERE kind = 'playlist' AND key = ?1")
        .map_err(|err| db_error("playlist ricevute", &err))?;

    let mut scritte = 0;
    for (chiave, playlist) in &fuso.playlist {
        inserisci
            .execute(rusqlite::params![
                chiave,
                playlist.nome,
                playlist.descrizione,
                playlist.creata_at,
                playlist.at,
                i64::from(playlist.smart),
                playlist.regole,
            ])
            .map_err(|err| db_error("playlist ricevute", &err))?;
        dissotterra
            .execute([chiave])
            .map_err(|err| db_error("playlist ricevute", &err))?;
        scritte += 1;

        if playlist.smart {
            continue;
        }
        scrivi_sequenza(tx, chiave, &playlist.sequenza)?;

        let id: i64 = identifica
            .query_row([chiave], |row| row.get(0))
            .map_err(|err| db_error("playlist ricevute", &err))?;

        // I brani che qui non ci sono si saltano nell'appartenenza ma **restano**
        // nella sequenza: il giorno che il file arriva, il brano ricompare al suo
        // posto invece che in fondo. È la differenza fra una playlist che
        // sopravvive a una libreria incompleta e una che si accorcia ogni volta.
        let mut brani = Vec::new();
        for chiave_brano in playlist.sequenza.ordine() {
            let trovato: Option<i64> = cerca_brano
                .query_row([chiave_brano], |row| row.get(0))
                .or_else(|err| match err {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    altro => Err(altro),
                })
                .map_err(|err| db_error("playlist ricevute", &err))?;
            if let Some(track_id) = trovato {
                brani.push(track_id);
            }
        }

        // Si svuota e si riscrive: `playlist_tracks` ha `PRIMARY KEY (playlist_id,
        // position)` e il vincolo si verifica a ogni istruzione, non alla COMMIT.
        // Le posizioni escono `0..n` senza buchi, che è la forma che la finestra
        // usa come indici.
        svuota
            .execute([id])
            .map_err(|err| db_error("playlist ricevute", &err))?;
        for (posizione, track_id) in brani.iter().enumerate() {
            let posizione = i64::try_from(posizione).unwrap_or(i64::MAX);
            accoda
                .execute(rusqlite::params![id, track_id, posizione])
                .map_err(|err| db_error("playlist ricevute", &err))?;
        }
    }
    Ok(scritte)
}

/// Le cartelle sorvegliate: la mappa datata, e l'elenco che ne discende.
///
/// L'elenco si rifà dalla mappa invece di essere unito: è l'unico modo perché una
/// cartella **tolta** resti tolta. Le cartelle sono percorsi di un computer
/// preciso, e quelle di un altro qui non esistono — ma entrano ugualmente
/// nell'elenco, come già fa un ripristino da backup: una cartella che non c'è
/// costa una scansione a vuoto, mentre una cartella che manca costa una libreria
/// che non si popola e nessuno sa perché.
fn applica_cartelle(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    if fuso.cartelle.is_empty() {
        return Ok(0);
    }
    let mut mappa: BTreeMap<String, Interruttore> =
        settings::read_json(tx, CHIAVE_CARTELLE_SINCRONIA)?.unwrap_or_default();
    let mut cambiate = 0;
    for (percorso, stato) in &fuso.cartelle {
        match mappa.get(percorso) {
            Some(gia) if gia.at >= stato.at => {}
            Some(gia) => {
                if gia.on != stato.on {
                    cambiate += 1;
                }
                mappa.insert(percorso.clone(), *stato);
            }
            None => {
                mappa.insert(percorso.clone(), *stato);
                cambiate += 1;
            }
        }
    }
    settings::write_json(tx, CHIAVE_CARTELLE_SINCRONIA, &mappa)?;

    let elenco: Vec<&str> = mappa
        .iter()
        .filter(|(_, stato)| stato.on)
        .map(|(percorso, _)| percorso.as_str())
        .collect();
    settings::write_json(tx, CHIAVE_CARTELLE, &elenco)?;
    Ok(cambiate)
}

/// La skin scelta per ultima.
fn applica_skin(tx: &Transaction<'_>, fuso: &Fuso) -> Result<(), AppError> {
    let Some(scelta) = &fuso.skin_attiva else {
        return Ok(());
    };
    let mia: i64 = settings::read(tx, CHIAVE_SKIN_QUANDO)?
        .and_then(|quando| quando.parse().ok())
        .unwrap_or(0);
    if scelta.at <= mia {
        return Ok(());
    }
    // La skin si scrive anche se il pacchetto qui non c'è: chi la legge ricade
    // sulla skin di serie quando non la trova, e il giorno che il `.aeskin`
    // arriva dalla nuvola la scelta è già quella giusta. Il contrario — rifiutare
    // la scelta finché il file non c'è — produrrebbe due dispositivi con due skin
    // diverse e nessun modo di accorgersi del perché.
    settings::write(tx, CHIAVE_SKIN, &scelta.id)?;
    settings::write(tx, CHIAVE_SKIN_QUANDO, &scelta.at.to_string())?;
    Ok(())
}

/// I dispositivi visti in questa passata.
fn applica_dispositivi(
    tx: &Transaction<'_>,
    fuso: &Fuso,
    adesso_ms: i64,
) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_dispositivi (id, visto_ms) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET visto_ms = excluded.visto_ms",
        )
        .map_err(|err| db_error("dispositivi visti", &err))?;
    for dispositivo in &fuso.dispositivi {
        scrivi
            .execute(rusqlite::params![dispositivo, adesso_ms])
            .map_err(|err| db_error("dispositivi visti", &err))?;
    }
    Ok(fuso.dispositivi.len())
}

// ── i dispositivi, per chi li deve mostrare ─────────────────────────────────

/// Un dispositivo conosciuto.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Dispositivo {
    /// L'identificativo con cui firma i suoi documenti.
    pub id: String,
    /// Come si chiama, se qualcuno gliel'ha detto.
    pub nome: Option<String>,
    /// La sua chiave pubblica, base64url.
    pub chiave: Option<String>,
    /// Se ci si fida di quel che scrive.
    pub fidato: bool,
    /// L'ultima volta che il suo documento è stato letto.
    pub visto_ms: Option<i64>,
}

/// I dispositivi conosciuti, i più visti di recente per primi.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn dispositivi(connection: &Connection) -> Result<Vec<Dispositivo>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, nome, chiave, fidato, visto_ms FROM sync_dispositivi
              ORDER BY visto_ms DESC NULLS LAST, id",
        )
        .map_err(|err| db_error("dispositivi", &err))?;
    let righe = statement
        .query_map([], |row| {
            let fidato: i64 = row.get(3)?;
            Ok(Dispositivo {
                id: row.get(0)?,
                nome: row.get(1)?,
                chiave: row.get(2)?,
                fidato: fidato != 0,
                visto_ms: row.get(4)?,
            })
        })
        .map_err(|err| db_error("dispositivi", &err))?;
    let mut elenco = Vec::new();
    for riga in righe {
        elenco.push(riga.map_err(|err| db_error("dispositivi", &err))?);
    }
    Ok(elenco)
}

/// Registra i dispositivi visti ma non ancora accettati.
///
/// Senza `fidato`, e senza toccarlo se la riga c'è già: un dispositivo accettato
/// non deve tornare ignoto perché lo si è rivisto. `visto_ms` invece si aggiorna
/// sempre — è l'unica cosa che distingue un dispositivo ancora in uso da uno che
/// non scrive più da mesi.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn intravisti(
    connection: &Connection,
    elenco: &[String],
    adesso_ms: i64,
) -> Result<usize, AppError> {
    if elenco.is_empty() {
        return Ok(0);
    }
    let mut scrivi = connection
        .prepare(
            "INSERT INTO sync_dispositivi (id, visto_ms) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET visto_ms = excluded.visto_ms",
        )
        .map_err(|err| db_error("dispositivi intravisti", &err))?;
    for id in elenco {
        scrivi
            .execute(rusqlite::params![id, adesso_ms])
            .map_err(|err| db_error("dispositivi intravisti", &err))?;
    }
    Ok(elenco.len())
}

/// Accetta un dispositivo: da qui in poi quel che scrive vale.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn fidati(
    connection: &Connection,
    id: &str,
    nome: Option<&str>,
    chiave: Option<&str>,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO sync_dispositivi (id, nome, chiave, fidato) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(id) DO UPDATE SET
               nome   = COALESCE(excluded.nome, sync_dispositivi.nome),
               chiave = COALESCE(excluded.chiave, sync_dispositivi.chiave),
               fidato = 1",
            rusqlite::params![id, nome, chiave],
        )
        .map_err(|err| db_error("fiducia in un dispositivo", &err))?;
    Ok(())
}

/// Dimentica un dispositivo.
///
/// Non tocca gli ascolti che ha già portato: erano ascolti veri, e cancellarli
/// perché non si vuole più sentire quel telefono significherebbe che dimenticare
/// un dispositivo riscrive la storia della libreria. Il suo documento smette
/// semplicemente di essere letto.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn dimentica(connection: &Connection, id: &str) -> Result<bool, AppError> {
    let via = connection
        .execute("DELETE FROM sync_dispositivi WHERE id = ?1", [id])
        .map_err(|err| db_error("dispositivo dimenticato", &err))?;
    Ok(via > 0)
}

#[cfg(test)]
mod prove {
    use aether_sync::{Documento, fondi};

    use super::*;
    use crate::db;

    /// Una libreria con qualche brano e una playlist.
    fn libreria() -> Connection {
        let connection = db::open_in_memory()
            .expect("database in memoria")
            .connection;
        for (id, chiave) in [(1, "a"), (2, "b"), (3, "c")] {
            connection
                .execute(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                         date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?3, 'chi', 'dove', 0, 0, 0)",
                    rusqlite::params![id, format!("/musica/{chiave}.flac"), chiave],
                )
                .expect("brano");
        }
        connection
    }

    fn playlist_con(connection: &Connection, chiave: &str, brani: &[i64]) {
        connection
            .execute(
                "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                 VALUES (?1, ?1, 100, 100)",
                [chiave],
            )
            .expect("playlist");
        let id: i64 = connection
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = ?1",
                [chiave],
                |row| row.get(0),
            )
            .expect("identificativo");
        for (posizione, brano) in brani.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, brano, i64::try_from(posizione).expect("posizione")],
                )
                .expect("membro");
        }
    }

    fn ordine(connection: &Connection, chiave: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(
                "SELECT t.track_key FROM playlist_tracks pt
                   JOIN playlists p ON p.id = pt.playlist_id
                   JOIN tracks t    ON t.id = pt.track_id
                  WHERE p.playlist_key = ?1 ORDER BY pt.position",
            )
            .expect("ordine");
        let righe = statement
            .query_map([chiave], |row| row.get::<_, String>(0))
            .expect("ordine");
        righe.map(|riga| riga.expect("riga")).collect()
    }

    fn conteggio(connection: &Connection, chiave: &str) -> i64 {
        connection
            .query_row(
                "SELECT play_count FROM tracks WHERE track_key = ?1",
                [chiave],
                |row| row.get(0),
            )
            .expect("conteggio")
    }

    #[test]
    fn lo_storico_che_c_era_diventa_dello_pseudo_dispositivo_importazione() {
        let mut connection = libreria();
        connection
            .execute("UPDATE tracks SET play_count = 7 WHERE track_key = 'a'", [])
            .expect("storico");

        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let quanti: i64 = connection
            .query_row(
                "SELECT quanti FROM sync_ascolti WHERE track_key = 'a' AND dispositivo = ?1",
                [IMPORTAZIONE],
                |row| row.get(0),
            )
            .expect("importazione");
        assert_eq!(quanti, 7);
        assert_eq!(
            conteggio(&connection, "a"),
            7,
            "la somma non cambia il totale"
        );
    }

    #[test]
    fn allineare_due_volte_non_raddoppia_niente() {
        let mut connection = libreria();
        connection
            .execute("UPDATE tracks SET play_count = 4 WHERE track_key = 'b'", [])
            .expect("storico");

        for _ in 0..5 {
            allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        }
        assert_eq!(conteggio(&connection, "b"), 4);
    }

    #[test]
    fn un_ascolto_si_somma_allo_storico_invece_di_sostituirlo() {
        let mut connection = libreria();
        connection
            .execute("UPDATE tracks SET play_count = 4 WHERE track_key = 'b'", [])
            .expect("storico");
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let tx = connection.transaction().expect("transazione");
        conta_ascolto(&tx, 2, "portatile").expect("ascolto");
        tx.commit().expect("commit");

        assert_eq!(conteggio(&connection, "b"), 5);
    }

    #[test]
    fn gli_ascolti_di_due_dispositivi_si_sommano_nella_libreria() {
        let mut connection = libreria();
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let tx = connection.transaction().expect("transazione");
        for _ in 0..3 {
            conta_ascolto(&tx, 1, "portatile").expect("ascolto");
        }
        tx.commit().expect("commit");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = Contenuto::default();
        suo.ascolti.insert("a".to_owned(), 2);

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(
            conteggio(&connection, "a"),
            5,
            "tre più due, non tre né due"
        );
    }

    #[test]
    fn una_passata_a_vuoto_non_cambia_la_libreria() {
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1, 2]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[Documento::nuovo("portatile", mio.clone(), 2_000)]);
        let cambiamenti = applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(cambiamenti.playlist_tolte, 0);
        assert_eq!(ordine(&connection, "sera"), vec!["a", "b"]);
        let dopo = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        assert_eq!(mio, dopo, "il documento è lo stesso di prima");
    }

    #[test]
    fn due_dispositivi_che_aggiungono_un_brano_ciascuno_li_tengono_entrambi() {
        // Il caso che oggi si perde: due aggiunte in contemporanea alla stessa
        // playlist, e l'ultimo che scrive cancella l'altro.
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let comune = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");

        let mut suo = comune.clone();
        if let Some(playlist) = suo.playlist.get_mut("sera") {
            playlist.sequenza.accoda("telefono", "c");
            playlist.at = 1_500;
        }

        // Intanto, di qua, si aggiunge un altro brano per la via normale.
        let id: i64 = connection
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = 'sera'",
                [],
                |row| row.get(0),
            )
            .expect("playlist");
        connection
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, 2, 1)",
                [id],
            )
            .expect("aggiunta locale");
        connection
            .execute("UPDATE playlists SET updated_at = 1600 WHERE id = ?1", [id])
            .expect("data");
        allinea(&mut connection, "portatile", 1_600).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        let finale = ordine(&connection, "sera");
        assert!(
            finale.contains(&"b".to_owned()),
            "l'aggiunta locale resta: {finale:?}"
        );
        assert!(
            finale.contains(&"c".to_owned()),
            "quella remota arriva: {finale:?}"
        );
        assert_eq!(finale.len(), 3);
    }

    #[test]
    fn una_playlist_cancellata_altrove_se_ne_va() {
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1, 2]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = Contenuto::default();
        suo.lapidi.playlist.insert("sera".to_owned(), 1_500);

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        let cambiamenti = applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(cambiamenti.playlist_tolte, 1);
        let quante: i64 = connection
            .query_row("SELECT COUNT(*) FROM playlists", [], |row| row.get(0))
            .expect("conteggio");
        assert_eq!(quante, 0);
    }

    #[test]
    fn togliere_un_voto_viaggia_come_metterlo() {
        let mut connection = libreria();
        connection
            .execute(
                "UPDATE tracks SET rating = 5, rating_at = 1000, stats_updated_at = 1000
                  WHERE track_key = 'a'",
                [],
            )
            .expect("voto");
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = Contenuto::default();
        suo.voti.insert("a".to_owned(), Voto { v: 0, at: 2_000 });

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 3_000),
            Documento::nuovo("telefono", suo, 3_000),
        ]);
        applica(&mut connection, &fuso, 3_000).expect("applicazione");

        let stelle: i64 = connection
            .query_row(
                "SELECT rating FROM tracks WHERE track_key = 'a'",
                [],
                |row| row.get(0),
            )
            .expect("voto");
        assert_eq!(stelle, 0, "lo zero datato è una decisione, non un'assenza");
    }

    #[test]
    fn un_brano_che_qui_non_c_e_resta_nella_playlist_e_torna_quando_arriva() {
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = mio.clone();
        if let Some(playlist) = suo.playlist.get_mut("sera") {
            playlist.sequenza.accoda("telefono", "sconosciuto");
            playlist.sequenza.accoda("telefono", "b");
            playlist.at = 1_500;
        }

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(
            ordine(&connection, "sera"),
            vec!["a", "b"],
            "il brano assente si salta"
        );

        // Arriva il file.
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified)
                 VALUES (9, '/musica/x.flac', 'sconosciuto', 'x', 'chi', 'dove', 0, 0, 0)",
                [],
            )
            .expect("brano arrivato");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[Documento::nuovo("portatile", mio, 3_000)]);
        applica(&mut connection, &fuso, 3_000).expect("applicazione");

        assert_eq!(
            ordine(&connection, "sera"),
            vec!["a", "sconosciuto", "b"],
            "torna al suo posto, non in fondo"
        );
    }

    #[test]
    fn una_cartella_tolta_resta_tolta() {
        let mut connection = libreria();
        settings::write_json(
            &connection,
            CHIAVE_CARTELLE,
            &vec!["C:\\Musica", "D:\\Altro"],
        )
        .expect("cartelle");
        allinea(&mut connection, "portatile", 1_000).expect("primo allineamento");

        // Di là qualcuno la toglie.
        let mut suo = Contenuto::default();
        suo.cartelle.insert(
            "D:\\Altro".to_owned(),
            Interruttore {
                on: false,
                at: 1_500,
            },
        );
        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        let cartelle: Vec<String> = settings::read_json(&connection, CHIAVE_CARTELLE)
            .expect("cartelle")
            .unwrap_or_default();
        assert_eq!(cartelle, vec!["C:\\Musica".to_owned()]);

        // E non torna al giro dopo, che è il vero difetto di un elenco unito.
        allinea(&mut connection, "portatile", 2_500).expect("secondo allineamento");
        let cartelle: Vec<String> = settings::read_json(&connection, CHIAVE_CARTELLE)
            .expect("cartelle")
            .unwrap_or_default();
        assert_eq!(cartelle, vec!["C:\\Musica".to_owned()]);
    }

    #[test]
    fn i_dispositivi_visti_finiscono_nell_elenco() {
        let mut connection = libreria();
        let fuso = fondi(&[
            Documento::nuovo("portatile", Contenuto::default(), 1_000),
            Documento::nuovo("telefono", Contenuto::default(), 1_000),
        ]);
        applica(&mut connection, &fuso, 1_000).expect("applicazione");

        let elenco = dispositivi(&connection).expect("dispositivi");
        assert_eq!(elenco.len(), 2);
        assert!(
            elenco.iter().all(|d| !d.fidato),
            "nessuno è fidato per il solo fatto di esistere"
        );

        fidati(&connection, "telefono", Some("Il telefono"), None).expect("fiducia");
        let elenco = dispositivi(&connection).expect("dispositivi");
        let telefono = elenco
            .iter()
            .find(|d| d.id == "telefono")
            .expect("telefono");
        assert!(telefono.fidato);
        assert_eq!(telefono.nome.as_deref(), Some("Il telefono"));

        assert!(dimentica(&connection, "telefono").expect("dimenticato"));
        assert_eq!(dispositivi(&connection).expect("dispositivi").len(), 1);
    }
}
