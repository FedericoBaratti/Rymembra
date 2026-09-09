//! Le playlist intelligenti: dalle regole ai brani.
//!
//! # La regola che regge tutto il file
//!
//! **Nessun valore raggiunge mai la query.** Il pezzo di SQL che varia — il
//! nome della colonna e la forma del confronto — esce sempre da un `match` su
//! un `enum` chiuso di [`aether_domain::regole`]; quel che l'utente ha scritto
//! diventa un `?` e viaggia come parametro. Non è prudenza generica: qui
//! l'utente compone letteralmente dei pezzi di condizione da un menù, ed è il
//! posto di tutta l'applicazione in cui una concatenazione sembrerebbe più
//! naturale e sarebbe più pericolosa.
//!
//! Il modo di sbagliare è uno solo, e si riconosce a vista: un `format!` che
//! interpola qualcosa che non sia una costante di questo file.
//!
//! # Perché i brani non si materializzano in `playlist_tracks`
//!
//! Perché una playlist intelligente **è** la sua interrogazione. Scrivere le
//! righe vorrebbe dire tenerle aggiornate: a ogni scansione, a ogni ascolto, a
//! ogni cuore messo. Una regola come «ascoltati negli ultimi 30 giorni» cambia
//! risultato mentre nessuno tocca niente — basta che passi la mezzanotte — e
//! una copia materializzata sarebbe sbagliata dal momento in cui viene scritta.
//!
//! Il costo è una query a ogni apertura, sugli indici che la libreria ha già.
//! Il guadagno è che non esiste nessun cammino in cui il contenuto mostrato e
//! le regole scritte dicano due cose diverse.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::regole::{
    Campo, Combinazione, GenereCampo, Insieme, Operatore, Ordinamento, Regola, Valore,
};
use rusqlite::Connection;
use rusqlite::types::Value as SqlValue;

use crate::library::{COLONNE_BRANO, TrackSummary, db_error, track_from_row};

/// Il tetto oltre cui una playlist intelligente non va, comunque.
///
/// Non è una preferenza: è la differenza fra una schermata che si apre e una
/// finestra che si pianta. Una regola vuota su una libreria da centomila brani
/// produrrebbe centomila righe da mandare attraverso l'IPC, e chi l'ha scritta
/// voleva quasi certamente altro. Il limite dell'utente, se c'è, è più stretto.
pub const TETTO: u32 = 5_000;

// ── come stanno su disco ────────────────────────────────────────────────────
//
// La forma serializzata sta qui e non nel dominio, che di serde non ne vuole
// sapere: là una `derive` costringerebbe le decisioni pure ad avere
// un'opinione su come vengono trasmesse. Il prezzo è questo strato di
// traduzione; il guadagno è che il giorno in cui il formato su disco dovesse
// cambiare — una versione, un campo nuovo — le regole non se ne accorgono.

/// Una regola come sta scritta in `playlists.rules`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RegolaSuDisco {
    campo: String,
    operatore: String,
    /// Il valore, di qualunque dei tre tipi. Assente per `vuoto`/`nonVuoto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    valore: Option<serde_json::Value>,
}

/// Un insieme come sta scritto in `playlists.rules`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct InsiemeSuDisco {
    combinazione: String,
    regole: Vec<RegolaSuDisco>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limite: Option<u32>,
    ordinamento: String,
}

/// Serializza le regole per la colonna `playlists.rules`.
///
/// # Errori
///
/// `internal.unexpected` se la serializzazione fallisce — cosa che per questi
/// tipi non può succedere, e che non si nasconde dietro una stringa vuota:
/// scrivere `""` al posto delle regole trasformerebbe una playlist intelligente
/// in una che prende tutta la libreria, senza dirlo a nessuno.
pub fn a_json(insieme: &Insieme) -> Result<String, AppError> {
    let su_disco = InsiemeSuDisco {
        combinazione: insieme.combinazione.come_testo().to_owned(),
        regole: insieme
            .regole
            .iter()
            .map(|r| RegolaSuDisco {
                campo: r.campo.come_testo().to_owned(),
                operatore: r.operatore.come_testo().to_owned(),
                valore: match &r.valore {
                    Valore::Testo(t) => Some(serde_json::Value::String(t.clone())),
                    Valore::Numero(n) => Some(serde_json::Value::Number((*n).into())),
                    Valore::Nessuno => None,
                },
            })
            .collect(),
        limite: insieme.limite,
        ordinamento: insieme.ordinamento.come_testo().to_owned(),
    };
    serde_json::to_string(&su_disco).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("regole di una playlist intelligente".to_owned()),
        })
        .with_cause(err.to_string())
    })
}

/// Rilegge le regole dalla colonna `playlists.rules`.
///
/// # Cosa succede a quel che non si riconosce
///
/// Un campo o un operatore che questa versione non conosce **salta quella
/// regola**, non l'intero insieme: è il caso di una playlist scritta da una
/// versione successiva e riaperta da una precedente, e perdere una condizione
/// su cinque è meglio che perdere la playlist. Le regole saltate si contano
/// come «scartate», perché `Regola::valida` le avrebbe scartate comunque — e
/// quella è la voce che la finestra mostra.
///
/// Un JSON del tutto illeggibile vale un insieme vuoto, cioè «tutta la
/// libreria», che è visibile subito e correggibile dall'interfaccia. Vedi la
/// regola di `settings::read_json`, e la sua ragione.
#[must_use]
pub fn da_json(raw: &str) -> Insieme {
    let Ok(su_disco) = serde_json::from_str::<InsiemeSuDisco>(raw) else {
        return Insieme::default();
    };
    let regole = su_disco
        .regole
        .into_iter()
        .filter_map(|r| {
            let campo = Campo::da_testo(&r.campo)?;
            let operatore = Operatore::da_testo(&r.operatore)?;
            let valore = match r.valore {
                Some(serde_json::Value::String(t)) => Valore::Testo(t),
                Some(serde_json::Value::Number(n)) => Valore::Numero(n.as_i64()?),
                Some(serde_json::Value::Bool(b)) => Valore::Numero(i64::from(b)),
                _ => Valore::Nessuno,
            };
            Some(Regola {
                campo,
                operatore,
                valore,
            })
        })
        .collect();
    Insieme {
        combinazione: Combinazione::da_testo(&su_disco.combinazione).unwrap_or_default(),
        regole,
        limite: su_disco.limite,
        ordinamento: Ordinamento::da_testo(&su_disco.ordinamento).unwrap_or_default(),
    }
}

/// La colonna di un campo. **L'unico posto** in cui un nome di colonna diventa
/// testo, e tutti i rami sono costanti scritte qui.
const fn colonna(campo: Campo) -> &'static str {
    match campo {
        Campo::Titolo => "t.title",
        Campo::Artista => "t.artist",
        Campo::Album => "t.album",
        Campo::Genere => "t.genre",
        Campo::Anno => "t.year",
        Campo::Valutazione => "t.rating",
        Campo::Preferito => "t.liked",
        Campo::Riproduzioni => "t.play_count",
        Campo::Aggiunto => "t.date_added",
        Campo::Durata => "t.duration_ms",
        Campo::UltimoAscolto => "t.last_played_at",
    }
}

/// Una condizione tradotta: il pezzo di `WHERE` e i suoi parametri.
struct Pezzo {
    sql: String,
    parametri: Vec<SqlValue>,
}

/// Traduce una regola.
///
/// `adesso_ms` arriva da fuori perché il dominio non ha un orologio e questo
/// modulo non deve averne uno **suo**: la stessa istantanea vale per tutte le
/// regole di un insieme, altrimenti «negli ultimi 7 giorni» e «non negli ultimi
/// 7 giorni» valutate a cavallo di un millisecondo potrebbero essere vere
/// insieme, o false insieme.
fn traduci(regola: &Regola, adesso_ms: i64) -> Option<Pezzo> {
    if !regola.valida() {
        return None;
    }
    let col = colonna(regola.campo);
    let genere = regola.campo.genere();

    // I due operatori che non guardano il valore. `Vuoto` copre sia `NULL` sia
    // la stringa vuota, e non è pignoleria: la scansione scrive l'uno o l'altra
    // a seconda di com'era il tag, e chi scrive una regola non deve saperlo.
    // Per i numeri copre anche lo zero, che per `year` e `rating` è il modo in
    // cui questa libreria scrive «non lo so».
    match regola.operatore {
        Operatore::Vuoto => {
            let sql = match genere {
                GenereCampo::Testo => format!("({col} IS NULL OR TRIM({col}) = '')"),
                _ => format!("({col} IS NULL OR {col} = 0)"),
            };
            return Some(Pezzo {
                sql,
                parametri: Vec::new(),
            });
        }
        Operatore::NonVuoto => {
            let sql = match genere {
                GenereCampo::Testo => format!("({col} IS NOT NULL AND TRIM({col}) <> '')"),
                _ => format!("({col} IS NOT NULL AND {col} <> 0)"),
            };
            return Some(Pezzo {
                sql,
                parametri: Vec::new(),
            });
        }
        _ => {}
    }

    match (&regola.valore, regola.operatore) {
        // ── testo ──────────────────────────────────────────────────────────
        //
        // `LIKE` con `ESCAPE`: senza, un titolo che contiene `%` o `_` —
        // «Hit 'Em Up (100%)» — diventerebbe un carattere jolly, e la regola
        // selezionerebbe mezza libreria senza che niente lo dica. I due
        // caratteri si proteggono nel **parametro**, che è dove stanno.
        (Valore::Testo(t), op) => {
            let ago = protetto(t.trim());
            let (forma, parametro) = match op {
                Operatore::Contiene => ("LIKE", format!("%{ago}%")),
                Operatore::NonContiene => ("NOT LIKE", format!("%{ago}%")),
                Operatore::Inizia => ("LIKE", format!("{ago}%")),
                Operatore::Finisce => ("LIKE", format!("%{ago}")),
                Operatore::Uguale => ("LIKE", ago),
                Operatore::Diverso => ("NOT LIKE", ago),
                _ => return None,
            };
            // `NOT LIKE` su una colonna `NULL` è `NULL`, cioè non vero: senza
            // il `COALESCE` «artista non contiene X» escluderebbe anche i brani
            // senza artista, che è l'opposto di quel che chiede chi la scrive.
            let sinistra = if matches!(op, Operatore::NonContiene | Operatore::Diverso) {
                format!("COALESCE({col}, '')")
            } else {
                col.to_owned()
            };
            Some(Pezzo {
                sql: format!("{sinistra} {forma} ? ESCAPE '\\'"),
                parametri: vec![SqlValue::Text(parametro)],
            })
        }

        // ── date, espresse in giorni indietro ──────────────────────────────
        (Valore::Numero(giorni), Operatore::NegliUltimi | Operatore::NonNegliUltimi) => {
            let soglia = adesso_ms.saturating_sub(giorni.saturating_mul(86_400_000));
            let sql = if regola.operatore == Operatore::NegliUltimi {
                format!("({col} IS NOT NULL AND {col} >= ?)")
            } else {
                // «non negli ultimi N giorni» **include** chi non l'ha mai
                // sentito: è esattamente la playlist che chi scrive questa
                // regola sta cercando — le canzoni dimenticate.
                format!("({col} IS NULL OR {col} = 0 OR {col} < ?)")
            };
            Some(Pezzo {
                sql,
                parametri: vec![SqlValue::Integer(soglia)],
            })
        }

        // ── numeri ─────────────────────────────────────────────────────────
        (Valore::Numero(n), op) => {
            let forma = match op {
                Operatore::Uguale => "=",
                Operatore::Diverso => "<>",
                Operatore::Maggiore => ">",
                Operatore::Minore => "<",
                _ => return None,
            };
            // `COALESCE` per la stessa ragione del testo, e in più perché
            // `play_count` e `rating` sono `NOT NULL` mentre `year` no: una
            // regola non deve comportarsi diversamente su due colonne che a
            // chi la scrive sembrano la stessa cosa.
            Some(Pezzo {
                sql: format!("COALESCE({col}, 0) {forma} ?"),
                parametri: vec![SqlValue::Integer(*n)],
            })
        }

        (Valore::Nessuno, _) => None,
    }
}

/// Protegge i jolly di `LIKE` dentro il valore cercato.
fn protetto(testo: &str) -> String {
    let mut fuori = String::with_capacity(testo.len());
    for c in testo.chars() {
        if matches!(c, '%' | '_' | '\\') {
            fuori.push('\\');
        }
        fuori.push(c);
    }
    fuori
}

/// La clausola `ORDER BY`. Chiusa in un `match`, come `TrackOrder::sql`.
const fn ordine(ordinamento: Ordinamento) -> &'static str {
    match ordinamento {
        Ordinamento::Scaffale => {
            "t.artist COLLATE NOCASE, t.album COLLATE NOCASE,
             t.disc_number, t.track_number, t.title COLLATE NOCASE"
        }
        Ordinamento::PiuAscoltati => "t.play_count DESC, t.last_played_at DESC",
        Ordinamento::MenoAscoltati => "t.play_count ASC, t.last_played_at IS NOT NULL, t.title",
        Ordinamento::Recenti => "t.date_added DESC, t.id DESC",
        // `RANDOM()` di SQLite e non un mescolamento nostro: mescolare qui
        // vorrebbe dire tirare su tutti i brani che passano il filtro per
        // tenerne cinquanta. Il seme non si può fissare, e va bene — una
        // playlist casuale che dà sempre lo stesso ordine non è casuale.
        Ordinamento::Casuale => "RANDOM()",
    }
}

/// La query di un insieme di regole, con i suoi parametri.
///
/// Pubblica dentro il crate perché la prova che conta è su di lei: guardare la
/// stringa prodotta è l'unico modo di dimostrare che nessun valore ci è finito
/// dentro.
pub(crate) fn query(insieme: &Insieme, adesso_ms: i64) -> (String, Vec<SqlValue>) {
    let mut condizioni: Vec<String> = Vec::new();
    let mut parametri: Vec<SqlValue> = Vec::new();
    for regola in &insieme.regole {
        if let Some(pezzo) = traduci(regola, adesso_ms) {
            condizioni.push(pezzo.sql);
            parametri.extend(pezzo.parametri);
        }
    }

    let dove = if condizioni.is_empty() {
        // Un `OR` vuoto è falso, un `AND` vuoto è vero: sono le due risposte
        // giuste, e `Insieme::prende_tutto`/`prende_niente` le dichiarano a chi
        // deve avvisare l'utente prima.
        match insieme.combinazione {
            Combinazione::Tutte => "1 = 1".to_owned(),
            Combinazione::Qualsiasi => "0 = 1".to_owned(),
        }
    } else {
        let giunzione = match insieme.combinazione {
            Combinazione::Tutte => " AND ",
            Combinazione::Qualsiasi => " OR ",
        };
        condizioni.join(giunzione)
    };

    let limite = insieme.limite.unwrap_or(TETTO).min(TETTO);
    let sql = format!(
        "SELECT {COLONNE_BRANO} FROM tracks t WHERE {dove} ORDER BY {} LIMIT {limite}",
        ordine(insieme.ordinamento)
    );
    (sql, parametri)
}

/// I brani che soddisfano un insieme di regole.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brani(
    connection: &Connection,
    insieme: &Insieme,
    adesso_ms: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let (sql, parametri) = query(insieme, adesso_ms);
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("playlist intelligente", &err))?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parametri.iter()), track_from_row)
        .map_err(|err| db_error("playlist intelligente", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("playlist intelligente", &err))
}

/// Quanti brani, e quanto durano in tutto.
///
/// Una query sola per due numeri che vanno insieme, e **con** il limite: sono
/// il «12 brani · 48 minuti» sotto il nome nell'elenco, cioè la descrizione di
/// quel che si vedrà aprendo. Un conteggio senza limite accanto a una lista
/// limitata sarebbe due verità diverse a due centimetri di distanza.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn conteggi(
    connection: &Connection,
    insieme: &Insieme,
    adesso_ms: i64,
) -> Result<(i64, i64), AppError> {
    let (sql, parametri) = query(insieme, adesso_ms);
    let riassunto = format!("SELECT COUNT(*), COALESCE(SUM(duration_ms), 0) FROM ({sql})");
    connection
        .prepare(&riassunto)
        .and_then(|mut s| {
            s.query_row(rusqlite::params_from_iter(parametri.iter()), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
        })
        .map_err(|err| db_error("riassunto di una playlist intelligente", &err))
}

/// Quanti brani soddisfano le regole, senza il limite.
///
/// Serve a dire «120 brani, ne mostro 50» invece di lasciar credere che siano
/// cinquanta. Il tetto non si applica: è un conteggio, non un trasferimento.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti(connection: &Connection, insieme: &Insieme, adesso_ms: i64) -> Result<i64, AppError> {
    let senza_limite = Insieme {
        limite: None,
        ..insieme.clone()
    };
    let (sql, parametri) = query(&senza_limite, adesso_ms);
    // Si conta la query com'è, sottoquery compresa: rifare le condizioni a mano
    // vorrebbe dire due traduzioni da tenere d'accordo, e la seconda
    // sbaglierebbe per prima.
    let conteggio = format!("SELECT COUNT(*) FROM ({sql})");
    connection
        .prepare(&conteggio)
        .and_then(|mut s| {
            s.query_row(rusqlite::params_from_iter(parametri.iter()), |row| {
                row.get(0)
            })
        })
        .map_err(|err| db_error("conteggio di una playlist intelligente", &err))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn regola(campo: Campo, operatore: Operatore, valore: Valore) -> Regola {
        Regola {
            campo,
            operatore,
            valore,
        }
    }

    fn insieme(regole: Vec<Regola>) -> Insieme {
        Insieme {
            combinazione: Combinazione::Tutte,
            regole,
            limite: None,
            ordinamento: Ordinamento::Scaffale,
        }
    }

    /// Una libreria minima: la sola tabella che serve a queste regole.
    fn db() -> Connection {
        let c = Connection::open_in_memory().expect("database in memoria");
        c.execute_batch(
            "CREATE TABLE tracks (
                id INTEGER PRIMARY KEY, path TEXT NOT NULL, title TEXT NOT NULL,
                artist TEXT NOT NULL, album TEXT NOT NULL, album_key TEXT,
                track_number INTEGER, disc_number INTEGER, duration_ms INTEGER NOT NULL,
                year INTEGER, cover_art_hash TEXT, play_count INTEGER NOT NULL DEFAULT 0,
                liked INTEGER NOT NULL DEFAULT 0, rating INTEGER NOT NULL DEFAULT 0,
                genre TEXT, date_added INTEGER NOT NULL DEFAULT 0, last_played_at INTEGER)",
        )
        .expect("schema");
        c
    }

    fn brano(c: &Connection, id: i64, titolo: &str, artista: &str, altro: &str) {
        c.execute(
            &format!(
                "INSERT INTO tracks (id, path, title, artist, album, duration_ms, {altro})
                 VALUES (?1, ?2, ?3, ?4, 'Al', 200000, {})",
                altro
                    .split(',')
                    .map(|_| "?5")
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            rusqlite::params![id, format!("/m/{id}.mp3"), titolo, artista, 1],
        )
        .expect("brano");
    }

    #[test]
    fn nessun_valore_finisce_nella_query() {
        // La prova che vale per tutto il file. Il valore contiene una
        // terminazione di stringa e un commento SQL: se comparisse nel testo
        // della query, questo `assert` sarebbe l'unico posto in cui ce ne
        // accorgeremmo prima di un utente.
        let cattivo = "'; DROP TABLE tracks; --";
        let (sql, parametri) = query(
            &insieme(vec![regola(
                Campo::Artista,
                Operatore::Contiene,
                Valore::Testo(cattivo.to_owned()),
            )]),
            0,
        );
        assert!(
            !sql.contains("DROP"),
            "il valore è finito nella query: {sql}"
        );
        assert!(!sql.contains('\''.to_string().as_str()) || !sql.contains("DROP"));
        assert_eq!(parametri.len(), 1);
        assert!(matches!(
            parametri.first(),
            Some(SqlValue::Text(t)) if t.contains("DROP")
        ));
    }

    #[test]
    fn i_jolly_di_like_non_sono_jolly() {
        // «100%» è un titolo, non «qualunque cosa». Senza `ESCAPE` questa
        // regola selezionerebbe mezza libreria, in silenzio.
        let c = db();
        brano(&c, 1, "Hit 'Em Up (100%)", "2Pac", "genre");
        brano(&c, 2, "Altro", "Tale", "genre");
        let trovati = brani(
            &c,
            &insieme(vec![regola(
                Campo::Titolo,
                Operatore::Contiene,
                Valore::Testo("100%".to_owned()),
            )]),
            0,
        )
        .expect("query");
        assert_eq!(trovati.len(), 1);
        assert_eq!(trovati.first().map(|b| b.id), Some(1));
    }

    #[test]
    fn non_contiene_tiene_i_brani_senza_quel_campo() {
        // `NOT LIKE` su `NULL` è `NULL`, cioè non vero: senza `COALESCE`,
        // «genere non contiene rock» butterebbe via anche i brani senza genere
        // — l'opposto di quel che chiede chi la scrive.
        let c = db();
        c.execute(
            "INSERT INTO tracks (id, path, title, artist, album, duration_ms, genre)
             VALUES (1, '/a', 'A', 'X', 'Al', 1000, 'Rock'),
                    (2, '/b', 'B', 'Y', 'Al', 1000, NULL)",
            [],
        )
        .expect("brani");
        let trovati = brani(
            &c,
            &insieme(vec![regola(
                Campo::Genere,
                Operatore::NonContiene,
                Valore::Testo("rock".to_owned()),
            )]),
            0,
        )
        .expect("query");
        assert_eq!(trovati.len(), 1);
        assert_eq!(trovati.first().map(|b| b.id), Some(2));
    }

    #[test]
    fn le_canzoni_dimenticate_comprendono_quelle_mai_sentite() {
        // «non ascoltate negli ultimi 90 giorni» deve includere chi non è mai
        // stato ascoltato: è esattamente la playlist che si sta cercando.
        let c = db();
        let adesso = 1_000_000_000_000_i64;
        c.execute(
            "INSERT INTO tracks (id, path, title, artist, album, duration_ms, last_played_at)
             VALUES (1, '/a', 'Ieri', 'X', 'Al', 1000, ?1),
                    (2, '/b', 'Mai', 'Y', 'Al', 1000, NULL),
                    (3, '/c', 'Anni fa', 'Z', 'Al', 1000, ?2)",
            rusqlite::params![adesso - 86_400_000, adesso - 400 * 86_400_000_i64],
        )
        .expect("brani");
        let trovati = brani(
            &c,
            &insieme(vec![regola(
                Campo::UltimoAscolto,
                Operatore::NonNegliUltimi,
                Valore::Numero(90),
            )]),
            adesso,
        )
        .expect("query");
        let ids: Vec<i64> = trovati.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![2, 3], "manca chi non l'ha mai sentito");
    }

    #[test]
    fn qualsiasi_e_tutte_non_danno_lo_stesso_risultato() {
        let c = db();
        c.execute(
            "INSERT INTO tracks (id, path, title, artist, album, duration_ms, liked, play_count)
             VALUES (1, '/a', 'A', 'X', 'Al', 1000, 1, 0),
                    (2, '/b', 'B', 'Y', 'Al', 1000, 0, 9)",
            [],
        )
        .expect("brani");
        let regole = vec![
            regola(Campo::Preferito, Operatore::Uguale, Valore::Numero(1)),
            regola(Campo::Riproduzioni, Operatore::Maggiore, Valore::Numero(5)),
        ];
        let tutte = brani(&c, &insieme(regole.clone()), 0).expect("query");
        assert!(
            tutte.is_empty(),
            "nessuno è insieme preferito e riascoltato"
        );
        let qualsiasi = brani(
            &c,
            &Insieme {
                combinazione: Combinazione::Qualsiasi,
                regole,
                limite: None,
                ordinamento: Ordinamento::Scaffale,
            },
            0,
        )
        .expect("query");
        assert_eq!(qualsiasi.len(), 2);
    }

    #[test]
    fn una_regola_storta_si_salta_e_le_altre_valgono() {
        let c = db();
        brano(&c, 1, "Uno", "Björk", "genre");
        brano(&c, 2, "Due", "Tale", "genre");
        let insieme = insieme(vec![
            regola(
                Campo::Artista,
                Operatore::Contiene,
                Valore::Testo("Björk".to_owned()),
            ),
            // Storta: operatore di testo su un campo numerico.
            regola(Campo::Anno, Operatore::Contiene, Valore::Numero(1997)),
        ]);
        let trovati = brani(&c, &insieme, 0).expect("query");
        assert_eq!(trovati.len(), 1, "la regola valida deve valere lo stesso");
        assert_eq!(insieme.scartate(), 1);
    }

    #[test]
    fn un_or_vuoto_non_prende_niente_e_un_and_vuoto_prende_tutto() {
        let c = db();
        brano(&c, 1, "Uno", "X", "genre");
        let tutto = brani(&c, &insieme(vec![]), 0).expect("query");
        assert_eq!(tutto.len(), 1);
        let niente = brani(
            &c,
            &Insieme {
                combinazione: Combinazione::Qualsiasi,
                ..Insieme::default()
            },
            0,
        )
        .expect("query");
        assert!(niente.is_empty());
    }

    #[test]
    fn il_tetto_vale_anche_se_il_limite_e_piu_alto() {
        let (sql, _) = query(
            &Insieme {
                limite: Some(1_000_000),
                ..Insieme::default()
            },
            0,
        );
        assert!(
            sql.contains(&format!("LIMIT {TETTO}")),
            "il tetto deve vincere: {sql}"
        );
    }

    #[test]
    fn le_regole_fanno_andata_e_ritorno_su_disco() {
        let originale = Insieme {
            combinazione: Combinazione::Qualsiasi,
            regole: vec![
                regola(
                    Campo::Artista,
                    Operatore::Contiene,
                    Valore::Testo("Björk".to_owned()),
                ),
                regola(Campo::Anno, Operatore::Maggiore, Valore::Numero(1990)),
                regola(Campo::Genere, Operatore::Vuoto, Valore::Nessuno),
            ],
            limite: Some(42),
            ordinamento: Ordinamento::MenoAscoltati,
        };
        let json = a_json(&originale).expect("serializzata");
        assert_eq!(da_json(&json), originale);
    }

    #[test]
    fn una_regola_di_una_versione_futura_non_porta_via_le_altre() {
        // Il caso vero: una playlist scritta da una versione con un campo in
        // più, riaperta da questa. Perdere una condizione su tre è meglio che
        // perdere la playlist.
        let json = r#"{"combinazione":"tutte","ordinamento":"scaffale","regole":[
            {"campo":"artista","operatore":"contiene","valore":"X"},
            {"campo":"tempoInBpm","operatore":"maggiore","valore":120}
        ]}"#;
        let letto = da_json(json);
        assert_eq!(letto.regole.len(), 1);
        assert_eq!(letto.regole.first().map(|r| r.campo), Some(Campo::Artista));
    }

    #[test]
    fn un_json_illeggibile_vale_insieme_vuoto() {
        assert_eq!(da_json("{non json"), Insieme::default());
    }

    #[test]
    fn il_conteggio_ignora_il_limite() {
        let c = db();
        for id in 1..=5 {
            brano(&c, id, "T", "X", "genre");
        }
        let insieme = Insieme {
            limite: Some(2),
            ..Insieme::default()
        };
        assert_eq!(brani(&c, &insieme, 0).expect("query").len(), 2);
        assert_eq!(quanti(&c, &insieme, 0), Ok(5));
    }
}
