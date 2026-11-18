//! I dispositivi collegati via internet, nella libreria: chi sono, e gli
//! ascolti dei loro brani da contare da loro. Vedi la migrazione 039.
//!
//! La rete non sta qui: questo crate non dipende da `rymembra-lontano`. Qui
//! ci sono solo le chiavi **pubbliche** degli altri; quella segreta di questo
//! dispositivo sta nel portachiavi.
//!
//! Come per «Ascolta insieme», un errore di questo modulo non porta un nome,
//! un indirizzo o una chiave: finisce nel diario.

use rusqlite::{Connection, OptionalExtension as _};
use rymembra_domain::errors::{AppError, ErrorCode};

use crate::library::db_error;

/// Quanti indirizzi di un dispositivo si ricordano.
pub const MASSIMO_INDIRIZZI: usize = 8;

/// Quanto si ricordano le chiavi degli ascolti già contati: un invio ripetuto
/// arriva entro pochi giorni, non dopo mesi.
pub const MEMORIA_RICEVUTI_MS: i64 = 30 * 24 * 3600 * 1000;

/// Un dispositivo collegato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispositivo {
    /// La riga.
    pub id: i64,
    /// La chiave pubblica, 64 cifre esadecimali.
    pub chiave: String,
    /// Il nome con cui lo si mostra.
    pub nome: String,
    /// `windows`, `android`.
    pub piattaforma: String,
    /// L'identificativo che ha nella sincronia, se l'ha detto.
    pub dispositivo: String,
    /// La sua chiave di «Ascolta insieme», se l'ha data.
    pub chiave_insieme: Option<String>,
    /// Gli ultimi indirizzi visti, `ip:porta`.
    pub indirizzi: Vec<String>,
    /// Quando ci si è associati.
    pub associato_ms: i64,
    /// L'ultima volta che ha risposto.
    pub visto_ms: Option<i64>,
    /// Non ci si fida più: dimenticato dall'altra parte.
    pub revocato: bool,
}

/// Quel che si sa di un dispositivo appena associato.
#[derive(Debug, Clone, Copy)]
pub struct Nuovo<'a> {
    /// La chiave pubblica.
    pub chiave: &'a str,
    /// Il nome che ha dichiarato.
    pub nome: &'a str,
    /// La piattaforma che ha dichiarato.
    pub piattaforma: &'a str,
    /// L'identificativo nella sincronia.
    pub dispositivo: &'a str,
    /// La chiave di «Ascolta insieme».
    pub chiave_insieme: Option<&'a str>,
    /// Gli indirizzi da cui si è presentato.
    pub indirizzi: &'a [String],
}

/// Un ascolto da contare dall'altra parte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AscoltoLontano {
    /// Il brano, con l'id della libreria dell'altro.
    pub brano: i64,
    /// Quando è cominciato.
    pub quando_ms: i64,
    /// Quanto si è ascoltato.
    pub ascoltato_ms: i64,
    /// La chiave che lo rende innocuo se arriva due volte.
    pub chiave: String,
}

// ── le impostazioni ─────────────────────────────────────────────────────────
//
// Come le `insieme.*`, sono fatti di **questa** macchina: il profilo e il
// backup portano solo le chiavi che elencano, e `lontano.*` non c'è. Un
// profilo portato su un altro computer non deve accendere da solo una porta
// verso internet.

/// L'interruttore «Raggiungibile da internet».
pub const CHIAVE_ATTIVO: &str = "lontano.attivo";

/// Il relay proprio, se ce n'è uno; assente, quelli pubblici di n0.
pub const CHIAVE_RELAY: &str = "lontano.relay";

/// I brani degli altri dispositivi sui dati mobili: vedi [`DatiMobili`].
pub const CHIAVE_DATI_MOBILI: &str = "lontano.dati_mobili";

/// L'interruttore «Resta attiva in sottofondo» del telefono: il servizio in
/// primo piano che tiene il nodo acceso anche con l'app chiusa.
///
/// Sta fuori dal profilo e dal backup, come `lontano.*`, e per la stessa
/// ragione ma più forte: è un fatto **di questo telefono** — la batteria che
/// vuole spendere, il produttore che lo lascia o no vivere — e portarlo su un
/// altro accenderebbe da sé un servizio sempre in primo piano.
pub const CHIAVE_SOTTOFONDO: &str = "daemon.attivo";

/// L'interruttore «Parti con l'auto» del telefono: quando l'auto si collega,
/// Rymembra riprende da sola la coda salvata, al posto dell'ultima app usata.
///
/// Fuori dal profilo e dal backup, e più forte di `daemon.attivo`: è un fatto
/// **di questo telefono e dell'auto in cui lo si collega**, e portarlo su un
/// altro farebbe partire la musica da sola in un'auto di un altro. Spenta di
/// serie, funziona solo col sottofondo acceso — è il suo servizio a guardare
/// quando l'auto si collega — e col sottofondo si spegne: vedi [`sottofondo`].
pub const CHIAVE_PARTI_CON_AUTO: &str = "daemon.auto";

/// Il relay più lungo che si accetta: un indirizzo, non un testo.
pub const MASSIMO_RELAY: usize = 300;

/// Che fare dei brani di un altro dispositivo quando si è sui dati mobili
/// («Solo Wi-Fi»). Vale per l'ascolto, gli scaricamenti e le sessioni via
/// internet; la presenza e gli elenchi passano sempre, perché pesano poco.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatiMobili {
    /// Lo si chiede, una volta: la risposta diventa [`DatiMobili::Si`] o
    /// [`DatiMobili::No`].
    #[default]
    Chiedi,
    /// Si usano.
    Si,
    /// Si aspetta il Wi-Fi.
    No,
}

/// Le impostazioni del collegamento via internet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Impostazioni {
    /// «Raggiungibile da internet». Spento di serie.
    pub attivo: bool,
    /// Il relay proprio (`https://…`), se ce n'è uno.
    pub relay: Option<String>,
    /// «Solo Wi-Fi».
    pub dati_mobili: DatiMobili,
    /// «Resta attiva in sottofondo» (solo sul telefono): il nodo non va più a
    /// dormire. Spento di serie, e **indipendente** da `attivo`: non
    /// accende la porta verso internet, che è un sì a parte.
    pub sottofondo: bool,
    /// «Parti con l'auto» (solo sul telefono): spenta di serie, vera solo
    /// insieme al sottofondo.
    pub parti_con_auto: bool,
}

/// Le impostazioni. Un valore illeggibile vale quello di serie: è
/// un'impostazione, non un dato da difendere.
///
/// # Errori
///
/// Quelli del database.
pub fn impostazioni(connection: &Connection) -> Result<Impostazioni, AppError> {
    let attivo = crate::settings::read_json::<bool>(connection, CHIAVE_ATTIVO)
        .ok()
        .flatten()
        .unwrap_or(false);
    let relay = crate::settings::read(connection, CHIAVE_RELAY)?
        .and_then(|grezzo| relay_valido(&grezzo).ok());
    let dati_mobili = crate::settings::read_json::<DatiMobili>(connection, CHIAVE_DATI_MOBILI)
        .ok()
        .flatten()
        .unwrap_or_default();
    let sottofondo = crate::settings::read_json::<bool>(connection, CHIAVE_SOTTOFONDO)
        .ok()
        .flatten()
        .unwrap_or(false);
    let parti_con_auto = crate::settings::read_json::<bool>(connection, CHIAVE_PARTI_CON_AUTO)
        .ok()
        .flatten()
        .unwrap_or(false);
    Ok(Impostazioni {
        attivo,
        relay,
        dati_mobili,
        sottofondo,
        parti_con_auto,
    })
}

/// Accende o spegne «Resta attiva in sottofondo».
///
/// Spegnerlo spegne anche «Parti con l'auto», e riaccenderlo non la
/// riaccende: era un sì per un servizio che c'era, e far partire la musica da
/// sola all'auto è una scelta che si rifà. Prima si scrive lei, poi lui: se la
/// seconda scrittura cade, il sottofondo resta acceso e la partenza spenta, che
/// è la direzione prudente.
///
/// # Errori
///
/// Quelli del database.
pub fn sottofondo(connection: &Connection, acceso: bool) -> Result<(), AppError> {
    if !acceso {
        crate::settings::write_json(connection, CHIAVE_PARTI_CON_AUTO, &false)?;
    }
    crate::settings::write_json(connection, CHIAVE_SOTTOFONDO, &acceso)
}

/// Accende o spegne «Parti con l'auto».
///
/// Accenderla vuole il sottofondo acceso: senza il suo servizio nessuno guarda
/// quando l'auto si collega, e un «sì» muto direbbe una cosa non vera. Spegnerla
/// si può sempre.
///
/// # Errori
///
/// `internal.aborted` se si accende col sottofondo spento — la finestra non lo
/// permette, e arrivarci vuol dire che il sottofondo si è spento un istante
/// prima —; quelli del database.
pub fn parti_con_auto(connection: &Connection, acceso: bool) -> Result<(), AppError> {
    if acceso && !impostazioni(connection)?.sottofondo {
        return Err(AppError::new(ErrorCode::InternalAborted {
            what: Some("«Parti con l'auto» senza il sottofondo".to_owned()),
        }));
    }
    crate::settings::write_json(connection, CHIAVE_PARTI_CON_AUTO, &acceso)
}

/// Accende o spegne «Raggiungibile da internet».
///
/// # Errori
///
/// Quelli del database.
pub fn attiva(connection: &Connection, attivo: bool) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_ATTIVO, &attivo)
}

/// Il relay proprio, o `None` per tornare a quelli di n0.
///
/// # Errori
///
/// `net.badSchema` per un indirizzo che non è `https://` o è troppo lungo;
/// quelli del database.
pub fn scegli_relay(connection: &Connection, relay: Option<&str>) -> Result<(), AppError> {
    match relay.map(str::trim).filter(|r| !r.is_empty()) {
        Some(grezzo) => {
            let buono = relay_valido(grezzo)?;
            crate::settings::write(connection, CHIAVE_RELAY, &buono)
        }
        None => crate::settings::forget(connection, CHIAVE_RELAY).map(|_| ()),
    }
}

/// La scelta sui dati mobili.
///
/// # Errori
///
/// Quelli del database.
pub fn scegli_dati_mobili(connection: &Connection, scelta: DatiMobili) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_DATI_MOBILI, &scelta)
}

/// Un relay è un indirizzo `https://` con un host, senza spazi. Il resto lo
/// controlla iroh quando lo usa.
fn relay_valido(grezzo: &str) -> Result<String, AppError> {
    let relay = grezzo.trim();
    let host = relay
        .strip_prefix("https://")
        .map(|resto| resto.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if host.is_empty()
        || relay.len() > MASSIMO_RELAY
        || relay.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AppError::new(ErrorCode::NetBadSchema {
            service: Some("lontano".to_owned()),
            detail: Some("il relay dev'essere un indirizzo https://".to_owned()),
        }));
    }
    Ok(relay.to_owned())
}

// ── i dispositivi ───────────────────────────────────────────────────────────

/// Tutti i dispositivi collegati, i più recenti prima.
///
/// # Errori
///
/// Quelli del database.
pub fn tutti(connection: &Connection) -> Result<Vec<Dispositivo>, AppError> {
    let guasto = |err: rusqlite::Error| db_error("elenco dei dispositivi", &err);
    let mut istruzione = connection
        .prepare(&format!(
            "SELECT {CAMPI} FROM dispositivi ORDER BY associato_ms DESC, id DESC"
        ))
        .map_err(guasto)?;
    let righe = istruzione.query_map([], riga).map_err(guasto)?;
    righe.collect::<Result<Vec<_>, _>>().map_err(guasto)
}

/// Un dispositivo, dalla sua chiave.
///
/// # Errori
///
/// Quelli del database.
pub fn uno(connection: &Connection, chiave: &str) -> Result<Option<Dispositivo>, AppError> {
    connection
        .query_row(
            &format!("SELECT {CAMPI} FROM dispositivi WHERE chiave = ?1"),
            [chiave.trim().to_ascii_lowercase()],
            riga,
        )
        .optional()
        .map_err(|err| db_error("un dispositivo", &err))
}

/// È un dispositivo collegato e di cui ci si fida?
///
/// # Errori
///
/// Quelli del database.
pub fn associato(connection: &Connection, chiave: &str) -> Result<bool, AppError> {
    Ok(uno(connection, chiave)?.is_some_and(|d| !d.revocato))
}

/// Ricorda un dispositivo appena associato. Se c'era già, lo aggiorna e torna
/// fidato: riassociarsi col QR è la prova che lo si vuole.
///
/// # Errori
///
/// `lontano.protocollo` per una chiave che non è di 64 cifre esadecimali.
/// Poi quelli del database.
pub fn associa(connection: &Connection, nuovo: Nuovo<'_>, adesso_ms: i64) -> Result<i64, AppError> {
    let chiave = chiave_valida(nuovo.chiave)?;
    let mut nome = crate::insieme::ripulisci_nome(nuovo.nome);
    if nome.is_empty() {
        nome = chiave.chars().take(8).collect();
    }
    let piattaforma: String = nuovo
        .piattaforma
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    let dispositivo: String = nuovo
        .dispositivo
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(64)
        .collect();
    let chiave_insieme = nuovo
        .chiave_insieme
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()));
    connection
        .query_row(
            "INSERT INTO dispositivi
               (chiave, nome, piattaforma, dispositivo, chiave_insieme, indirizzi,
                associato_ms, visto_ms, revocato)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 0)
             ON CONFLICT(chiave) DO UPDATE SET
               nome = excluded.nome,
               piattaforma = excluded.piattaforma,
               dispositivo = excluded.dispositivo,
               chiave_insieme = excluded.chiave_insieme,
               indirizzi = excluded.indirizzi,
               associato_ms = excluded.associato_ms,
               visto_ms = excluded.visto_ms,
               revocato = 0
             RETURNING id",
            rusqlite::params![
                chiave,
                nome,
                piattaforma,
                dispositivo,
                chiave_insieme,
                indirizzi_in_json(nuovo.indirizzi),
                adesso_ms
            ],
            |r| r.get(0),
        )
        .map_err(|err| db_error("associazione di un dispositivo", &err))
}

/// Rinomina un dispositivo.
///
/// # Errori
///
/// `ipc.payloadInvalid` per un nome vuoto; `lontano.nonAssociato` se il
/// dispositivo non c'è. Poi quelli del database.
pub fn rinomina(connection: &Connection, chiave: &str, nome: &str) -> Result<(), AppError> {
    let nome = crate::insieme::ripulisci_nome(nome);
    if nome.is_empty() {
        return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "lontano_rinomina".to_owned(),
            detail: Some("il nome è vuoto".to_owned()),
        }));
    }
    let toccate = connection
        .execute(
            "UPDATE dispositivi SET nome = ?2 WHERE chiave = ?1",
            rusqlite::params![chiave.trim().to_ascii_lowercase(), nome],
        )
        .map_err(|err| db_error("nome di un dispositivo", &err))?;
    c_era(toccate)
}

/// Non ci si fida più del dispositivo (ha chiesto di essere dimenticato), ma
/// la riga resta, perché lo si veda e lo si possa togliere.
///
/// La fiducia di «Ascolta insieme» resta com'è, a differenza di
/// [`dimentica`]: qui decide l'**altro**, e la chiave di «Ascolta insieme»
/// che aveva dato col QR l'aveva solo dichiarata. Un dispositivo collegato che
/// avesse dichiarato quella di un altro PC fidato potrebbe, con un
/// «dimenticami», togliergli la fiducia. La toglie la persona, con «Dimentica».
///
/// # Errori
///
/// Quelli del database.
pub fn revoca(connection: &Connection, chiave: &str) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE dispositivi SET revocato = 1 WHERE chiave = ?1",
            [chiave.trim().to_ascii_lowercase()],
        )
        .map(|_| ())
        .map_err(|err| db_error("revoca di un dispositivo", &err))
}

/// Dimentica un dispositivo, gli ascolti che aspettavano di arrivargli, e la
/// fiducia che il QR aveva dato in «Ascolta insieme» alla chiave che il
/// dispositivo aveva portato: un telefono perso e dimenticato non deve poter
/// entrare nelle sessioni di casa senza codice. Un PC che la persona aveva
/// **bloccato** resta com'è: tolto, alla prossima associazione — sua, non
/// nostra — tornerebbe fidato.
///
/// La chiave l'aveva dichiarata il dispositivo, e niente provava che fosse
/// sua: se era quella di un altro PC fidato, anche quello torna a chiedere il
/// codice. È la persona a dimenticare, e il prezzo è un codice a quattro
/// cifre; il contrario — un dispositivo dimenticato che resta fidato — no.
///
/// Dimenticare due volte è dimenticare una volta. `true` se la fiducia di
/// «Ascolta insieme» è cambiata: chi chiama la fa rileggere alla sua rete, che
/// chiude le connessioni di quella chiave.
///
/// # Errori
///
/// Quelli del database.
pub fn dimentica(connection: &Connection, chiave: &str) -> Result<bool, AppError> {
    let chiave = chiave.trim().to_ascii_lowercase();
    let guasto = |err: rusqlite::Error| db_error("dispositivo da dimenticare", &err);
    let tx = connection.unchecked_transaction().map_err(guasto)?;
    let insieme = chiave_insieme_di(&tx, &chiave).map_err(guasto)?;
    tx.execute("DELETE FROM dispositivi WHERE chiave = ?1", [&chiave])
        .map_err(guasto)?;
    tx.execute(
        "DELETE FROM dispositivi_ascolti WHERE dispositivo = ?1",
        [&chiave],
    )
    .map_err(guasto)?;
    // E la posta che aspettava di partire (un voto, un testo): a un dispositivo
    // dimenticato non si scrive più niente.
    tx.execute(
        "DELETE FROM dispositivi_posta WHERE dispositivo = ?1",
        [&chiave],
    )
    .map_err(guasto)?;
    let cambiata = senza_fiducia_insieme(&tx, insieme.as_deref()).map_err(guasto)?;
    tx.commit().map_err(guasto)?;
    Ok(cambiata)
}

/// La chiave di «Ascolta insieme» che il dispositivo aveva dato, se c'è.
fn chiave_insieme_di(connection: &Connection, chiave: &str) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT chiave_insieme FROM dispositivi WHERE chiave = ?1",
            [chiave],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .map(Option::flatten)
}

/// Toglie dai PC fidati di «Ascolta insieme» la chiave `insieme`, col suo
/// indirizzo ricordato, se c'è e non è bloccata. `true` se l'ha tolta.
fn senza_fiducia_insieme(connection: &Connection, insieme: Option<&str>) -> rusqlite::Result<bool> {
    let Some(insieme) = insieme.map(|k| k.trim().to_ascii_lowercase()) else {
        return Ok(false);
    };
    connection.execute(
        "DELETE FROM insieme_indirizzi
          WHERE chiave = ?1
            AND EXISTS (SELECT 1 FROM insieme_pari WHERE chiave = ?1 AND bloccato = 0)",
        [&insieme],
    )?;
    let tolti = connection.execute(
        "DELETE FROM insieme_pari WHERE chiave = ?1 AND bloccato = 0",
        [&insieme],
    )?;
    Ok(tolti > 0)
}

/// Il dispositivo ha risposto adesso, da questi indirizzi (vuoti: quelli di
/// prima restano).
///
/// # Errori
///
/// Quelli del database.
pub fn visto(
    connection: &Connection,
    chiave: &str,
    adesso_ms: i64,
    indirizzi: &[String],
) -> Result<(), AppError> {
    let chiave = chiave.trim().to_ascii_lowercase();
    let esito = if indirizzi.is_empty() {
        connection.execute(
            "UPDATE dispositivi SET visto_ms = ?2 WHERE chiave = ?1",
            rusqlite::params![chiave, adesso_ms],
        )
    } else {
        connection.execute(
            "UPDATE dispositivi SET visto_ms = ?2, indirizzi = ?3 WHERE chiave = ?1",
            rusqlite::params![chiave, adesso_ms, indirizzi_in_json(indirizzi)],
        )
    };
    esito
        .map(|_| ())
        .map_err(|err| db_error("dispositivo visto", &err))
}

/// Un ascolto fatto qui di un brano di `dispositivo`, da mandargli. Lo stesso
/// due volte (la stessa `chiave`) è uno solo.
///
/// # Errori
///
/// Quelli del database.
pub fn ricorda_ascolto(
    connection: &Connection,
    dispositivo: &str,
    ascolto: &AscoltoLontano,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT OR IGNORE INTO dispositivi_ascolti
               (dispositivo, brano, quando_ms, ascoltato_ms, chiave, inviato)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            rusqlite::params![
                dispositivo.trim().to_ascii_lowercase(),
                ascolto.brano,
                ascolto.quando_ms,
                ascolto.ascoltato_ms,
                ascolto.chiave
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("ascolto di un brano di un altro dispositivo", &err))
}

/// Gli ascolti che `dispositivo` non ha ancora ricevuto, dal più vecchio.
///
/// # Errori
///
/// Quelli del database.
pub fn ascolti_da_mandare(
    connection: &Connection,
    dispositivo: &str,
) -> Result<Vec<AscoltoLontano>, AppError> {
    let guasto = |err: rusqlite::Error| db_error("ascolti da mandare", &err);
    let mut istruzione = connection
        .prepare(
            "SELECT brano, quando_ms, ascoltato_ms, chiave FROM dispositivi_ascolti
              WHERE dispositivo = ?1 AND inviato = 0
              ORDER BY quando_ms, id",
        )
        .map_err(guasto)?;
    let righe = istruzione
        .query_map([dispositivo.trim().to_ascii_lowercase()], |r| {
            Ok(AscoltoLontano {
                brano: r.get(0)?,
                quando_ms: r.get(1)?,
                ascoltato_ms: r.get(2)?,
                chiave: r.get(3)?,
            })
        })
        .map_err(guasto)?;
    righe.collect::<Result<Vec<_>, _>>().map_err(guasto)
}

/// L'ascolto è arrivato: si toglie.
///
/// # Errori
///
/// Quelli del database.
pub fn ascolto_arrivato(connection: &Connection, chiave: &str) -> Result<(), AppError> {
    connection
        .execute(
            "DELETE FROM dispositivi_ascolti WHERE chiave = ?1",
            [chiave],
        )
        .map(|_| ())
        .map_err(|err| db_error("ascolto arrivato", &err))
}

/// Dall'altra parte: questo ascolto è nuovo? `true` la prima volta, e da lì
/// la chiave si ricorda per [`MEMORIA_RICEVUTI_MS`].
///
/// # Errori
///
/// Quelli del database.
pub fn ascolto_nuovo(
    connection: &Connection,
    chiave: &str,
    adesso_ms: i64,
) -> Result<bool, AppError> {
    let guasto = |err: rusqlite::Error| db_error("ascolto ricevuto", &err);
    connection
        .execute(
            "DELETE FROM dispositivi_ricevuti WHERE ricevuto_ms < ?1",
            [adesso_ms.saturating_sub(MEMORIA_RICEVUTI_MS)],
        )
        .map_err(guasto)?;
    let inseriti = connection
        .execute(
            "INSERT OR IGNORE INTO dispositivi_ricevuti (chiave, ricevuto_ms) VALUES (?1, ?2)",
            rusqlite::params![chiave, adesso_ms],
        )
        .map_err(guasto)?;
    Ok(inseriti > 0)
}

/// Quanti ascolti di `dispositivo` sono stati ricevuti da `dal_ms` in poi.
///
/// Le chiavi degli ascolti ricevuti cominciano con la chiave pubblica di chi
/// li ha mandati e `:` (le compone chi li riceve): il conto è un intervallo
/// della chiave primaria, `[<chiave>:, <chiave>;)`.
///
/// # Errori
///
/// Quelli del database.
pub fn ricevuti_da(
    connection: &Connection,
    dispositivo: &str,
    dal_ms: i64,
) -> Result<usize, AppError> {
    let chiave = dispositivo.trim().to_ascii_lowercase();
    connection
        .query_row(
            "SELECT COUNT(*) FROM dispositivi_ricevuti
              WHERE chiave >= ?1 AND chiave < ?2 AND ricevuto_ms >= ?3",
            rusqlite::params![format!("{chiave}:"), format!("{chiave};"), dal_ms],
            |r| r.get::<_, i64>(0),
        )
        .map(|quanti| usize::try_from(quanti).unwrap_or(usize::MAX))
        .map_err(|err| db_error("ascolti ricevuti da un dispositivo", &err))
}

/// Un ascolto ricevuto che non si è potuto contare: la sua chiave non lo
/// deve dire contato, o il prossimo invio lo scarterebbe.
///
/// # Errori
///
/// Quelli del database.
pub fn ascolto_da_ricontare(connection: &Connection, chiave: &str) -> Result<(), AppError> {
    connection
        .execute(
            "DELETE FROM dispositivi_ricevuti WHERE chiave = ?1",
            [chiave],
        )
        .map(|_| ())
        .map_err(|err| db_error("ascolto da ricontare", &err))
}

// ── la posta in uscita ──────────────────────────────────────────────────────
//
// Quel che si decide qui di un brano di un altro dispositivo e che lui deve
// sapere — un voto o un mi piace (`valuta`), un testo corretto (`testo`) — e
// che aspetta se lui non risponde. Una voce sola per (dispositivo, tipo,
// chiave): la scrittura più nuova prende il posto di quella che aspettava. Come
// gli ascolti, non viaggia nel profilo né nel backup: è un fatto di questa
// macchina. Vedi la migrazione 043.

/// Il tipo della posta per un voto o un mi piace.
pub const POSTA_VALUTA: &str = "valuta";

/// Quanto aspetta in posta una voce che il dispositivo rifiuta sempre (una
/// versione che non la capisce): oltre, si lascia.
pub const SCADENZA_POSTA_MS: i64 = 30 * 24 * 3600 * 1000;

/// Una voce della posta in uscita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocePosta {
    /// La riga.
    pub id: i64,
    /// Il tipo.
    pub tipo: String,
    /// Quel che la distingue dalle altre dello stesso tipo.
    pub chiave: String,
    /// Il contenuto, in JSON.
    pub corpo: String,
    /// Quando è stata decisa, ms.
    pub quando_ms: i64,
}

/// Mette in posta una voce per `dispositivo`, al posto di quella che c'era per
/// lo stesso (tipo, chiave).
///
/// # Errori
///
/// Quelli del database.
pub fn metti_in_posta(
    connection: &Connection,
    dispositivo: &str,
    tipo: &str,
    chiave: &str,
    corpo: &str,
    quando_ms: i64,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO dispositivi_posta (dispositivo, tipo, chiave, corpo, quando_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(dispositivo, tipo, chiave) DO UPDATE SET
               corpo = excluded.corpo, quando_ms = excluded.quando_ms",
            rusqlite::params![
                dispositivo.trim().to_ascii_lowercase(),
                tipo,
                chiave,
                corpo,
                quando_ms
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("posta in uscita", &err))
}

/// Una voce in posta, se c'è.
///
/// # Errori
///
/// Quelli del database.
pub fn in_posta(
    connection: &Connection,
    dispositivo: &str,
    tipo: &str,
    chiave: &str,
) -> Result<Option<VocePosta>, AppError> {
    connection
        .query_row(
            "SELECT id, tipo, chiave, corpo, quando_ms FROM dispositivi_posta
              WHERE dispositivo = ?1 AND tipo = ?2 AND chiave = ?3",
            rusqlite::params![dispositivo.trim().to_ascii_lowercase(), tipo, chiave],
            voce_di_posta,
        )
        .optional()
        .map_err(|err| db_error("posta in uscita", &err))
}

/// La posta che `dispositivo` non ha ancora ricevuto, dalla più vecchia.
///
/// # Errori
///
/// Quelli del database.
pub fn posta_da_mandare(
    connection: &Connection,
    dispositivo: &str,
) -> Result<Vec<VocePosta>, AppError> {
    let guasto = |err: rusqlite::Error| db_error("posta da mandare", &err);
    let mut istruzione = connection
        .prepare(
            "SELECT id, tipo, chiave, corpo, quando_ms FROM dispositivi_posta
              WHERE dispositivo = ?1 ORDER BY quando_ms, id",
        )
        .map_err(guasto)?;
    let righe = istruzione
        .query_map([dispositivo.trim().to_ascii_lowercase()], voce_di_posta)
        .map_err(guasto)?;
    righe.collect::<Result<Vec<_>, _>>().map_err(guasto)
}

/// La voce è arrivata: si toglie — se è ancora quella che si era mandata. Una
/// scrittura più nuova per lo stesso brano, arrivata mentre questa partiva,
/// resta in posta. `true` se l'ha tolta.
///
/// # Errori
///
/// Quelli del database.
pub fn posta_consegnata(
    connection: &Connection,
    id: i64,
    quando_ms: i64,
) -> Result<bool, AppError> {
    connection
        .execute(
            "DELETE FROM dispositivi_posta WHERE id = ?1 AND quando_ms = ?2",
            rusqlite::params![id, quando_ms],
        )
        .map(|toccate| toccate > 0)
        .map_err(|err| db_error("posta consegnata", &err))
}

/// I dispositivi che hanno posta in attesa.
///
/// # Errori
///
/// Quelli del database.
pub fn con_posta_in_attesa(connection: &Connection) -> Result<Vec<String>, AppError> {
    let guasto = |err: rusqlite::Error| db_error("dispositivi con posta", &err);
    let mut istruzione = connection
        .prepare("SELECT DISTINCT dispositivo FROM dispositivi_posta")
        .map_err(guasto)?;
    let righe = istruzione
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(guasto)?;
    righe.collect::<Result<Vec<_>, _>>().map_err(guasto)
}

fn voce_di_posta(r: &rusqlite::Row<'_>) -> rusqlite::Result<VocePosta> {
    Ok(VocePosta {
        id: r.get(0)?,
        tipo: r.get(1)?,
        chiave: r.get(2)?,
        corpo: r.get(3)?,
        quando_ms: r.get(4)?,
    })
}

const CAMPI: &str = "id, chiave, nome, piattaforma, dispositivo, chiave_insieme, indirizzi, \
                     associato_ms, visto_ms, revocato";

fn riga(r: &rusqlite::Row<'_>) -> rusqlite::Result<Dispositivo> {
    let indirizzi: String = r.get(6)?;
    Ok(Dispositivo {
        id: r.get(0)?,
        chiave: r.get(1)?,
        nome: r.get(2)?,
        piattaforma: r.get(3)?,
        dispositivo: r.get(4)?,
        chiave_insieme: r.get(5)?,
        indirizzi: serde_json::from_str(&indirizzi).unwrap_or_default(),
        associato_ms: r.get(7)?,
        visto_ms: r.get(8)?,
        revocato: r.get::<_, i64>(9)? != 0,
    })
}

fn indirizzi_in_json(indirizzi: &[String]) -> String {
    let tenuti: Vec<&String> = indirizzi
        .iter()
        .filter(|a| a.parse::<std::net::SocketAddr>().is_ok())
        .take(MASSIMO_INDIRIZZI)
        .collect();
    serde_json::to_string(&tenuti).unwrap_or_else(|_| "[]".to_owned())
}

fn chiave_valida(chiave: &str) -> Result<String, AppError> {
    let chiave = chiave.trim().to_ascii_lowercase();
    if chiave.len() == 64 && chiave.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Ok(chiave);
    }
    Err(
        AppError::new(ErrorCode::LontanoProtocollo).with_cause(format!(
            "chiave pubblica di {} caratteri, non 64 cifre esadecimali",
            chiave.chars().count()
        )),
    )
}

fn c_era(toccate: usize) -> Result<(), AppError> {
    if toccate > 0 {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::LontanoNonAssociato))
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::db::open_in_memory;

    const INSIEME: &str = "ABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABAB";

    fn nuovo<'a>(chiave: &'a str, indirizzi: &'a [String]) -> Nuovo<'a> {
        Nuovo {
            chiave,
            nome: " Nothing\u{202E} Phone 2a ",
            piattaforma: "android",
            dispositivo: "d-123",
            chiave_insieme: Some(INSIEME),
            indirizzi,
        }
    }

    #[test]
    fn le_impostazioni_hanno_un_valore_di_serie_e_il_relay_si_controlla() {
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        assert_eq!(impostazioni(c).unwrap(), Impostazioni::default());
        attiva(c, true).unwrap();
        scegli_dati_mobili(c, DatiMobili::No).unwrap();
        scegli_relay(c, Some(" https://relay.example.org/ ")).unwrap();
        let lette = impostazioni(c).unwrap();
        assert!(lette.attivo);
        assert_eq!(lette.dati_mobili, DatiMobili::No);
        assert_eq!(lette.relay.as_deref(), Some("https://relay.example.org/"));
        for cattivo in [
            "http://relay.example.org",
            "https://",
            "https:// spazio",
            "relay",
        ] {
            assert!(scegli_relay(c, Some(cattivo)).is_err(), "{cattivo}");
        }
        assert_eq!(
            impostazioni(c).unwrap().relay,
            lette.relay,
            "un no non tocca quello di prima"
        );
        scegli_relay(c, Some("  ")).unwrap();
        assert_eq!(impostazioni(c).unwrap().relay, None, "vuoto torna a n0");
        crate::settings::write(c, CHIAVE_DATI_MOBILI, "forse").unwrap();
        crate::settings::write(c, CHIAVE_ATTIVO, "{").unwrap();
        let rotte = impostazioni(c).unwrap();
        assert_eq!(rotte.dati_mobili, DatiMobili::Chiedi);
        assert!(!rotte.attivo);
    }

    #[test]
    fn il_sottofondo_e_spento_di_serie_e_un_valore_rotto_vale_spento() {
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        assert!(!impostazioni(c).unwrap().sottofondo, "spento di serie");
        sottofondo(c, true).unwrap();
        let acceso = impostazioni(c).unwrap();
        assert!(acceso.sottofondo);
        assert!(
            !acceso.attivo,
            "il sottofondo non accende «Raggiungibile da internet»: è un sì a parte"
        );
        attiva(c, true).unwrap();
        sottofondo(c, false).unwrap();
        let spento = impostazioni(c).unwrap();
        assert!(!spento.sottofondo);
        assert!(spento.attivo, "spegnere il sottofondo non tocca il resto");
        // Un valore illeggibile vale spento: è la scelta prudente, un servizio
        // in primo piano non si accende per un'impostazione che non si legge.
        crate::settings::write(c, CHIAVE_SOTTOFONDO, "{").unwrap();
        assert!(!impostazioni(c).unwrap().sottofondo);
    }

    #[test]
    fn parti_con_l_auto_e_spento_di_serie_serve_il_sottofondo_e_con_lui_si_spegne() {
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        assert!(!impostazioni(c).unwrap().parti_con_auto, "spenta di serie");

        // Senza il sottofondo non si accende, e lo si dice: nessuno guarderebbe
        // l'auto, e un «sì» muto direbbe una cosa che non è vera.
        assert!(parti_con_auto(c, true).is_err());
        assert!(!impostazioni(c).unwrap().parti_con_auto);

        sottofondo(c, true).unwrap();
        parti_con_auto(c, true).unwrap();
        let accesa = impostazioni(c).unwrap();
        assert!(accesa.parti_con_auto && accesa.sottofondo);

        // Spenta da sola, il sottofondo resta com'è.
        parti_con_auto(c, false).unwrap();
        let sola = impostazioni(c).unwrap();
        assert!(!sola.parti_con_auto && sola.sottofondo);

        // Col sottofondo si spegne anche lei, e riaccenderlo non la riaccende:
        // il sì era per un servizio che c'era, e ripartire con l'auto da soli
        // è una scelta che si rifà.
        parti_con_auto(c, true).unwrap();
        sottofondo(c, false).unwrap();
        assert!(!impostazioni(c).unwrap().parti_con_auto);
        sottofondo(c, true).unwrap();
        assert!(!impostazioni(c).unwrap().parti_con_auto);

        // Un valore illeggibile vale spento: la scelta prudente.
        crate::settings::write(c, CHIAVE_PARTI_CON_AUTO, "{").unwrap();
        assert!(!impostazioni(c).unwrap().parti_con_auto);
    }

    #[test]
    fn un_dispositivo_si_associa_si_rinomina_si_revoca_e_si_dimentica() {
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        let chiave = "0f".repeat(32);
        let indirizzi = vec!["192.168.1.5:4000".to_owned(), "non un indirizzo".to_owned()];
        let id = associa(c, nuovo(&chiave, &indirizzi), 1000).unwrap();
        let letto = uno(c, &chiave.to_ascii_uppercase()).unwrap().unwrap();
        assert_eq!(letto.id, id);
        assert_eq!(
            letto.nome, "Nothing Phone 2a",
            "ripulito come gli altri nomi"
        );
        assert_eq!(
            letto.chiave_insieme.as_deref(),
            Some("ab".repeat(32).as_str())
        );
        assert_eq!(letto.indirizzi, vec!["192.168.1.5:4000".to_owned()]);
        assert_eq!(letto.visto_ms, Some(1000));
        assert!(associato(c, &chiave).unwrap());

        rinomina(c, &chiave, "Telefono").unwrap();
        assert!(rinomina(c, &chiave, "   ").is_err());
        assert!(rinomina(c, &"aa".repeat(32), "x").is_err());

        visto(c, &chiave, 2000, &[]).unwrap();
        let letto = uno(c, &chiave).unwrap().unwrap();
        assert_eq!(
            (letto.nome.as_str(), letto.visto_ms),
            ("Telefono", Some(2000))
        );
        assert_eq!(letto.indirizzi.len(), 1, "visto senza indirizzi: restano");

        revoca(c, &chiave).unwrap();
        assert!(!associato(c, &chiave).unwrap());
        // Riassociarsi col QR lo rende di nuovo fidato, con la stessa riga.
        assert_eq!(associa(c, nuovo(&chiave, &[]), 3000).unwrap(), id);
        assert!(associato(c, &chiave).unwrap());

        dimentica(c, &chiave).unwrap();
        dimentica(c, &chiave).unwrap();
        assert!(tutti(c).unwrap().is_empty());
        assert!(associa(c, nuovo("corta", &[]), 1).is_err());
    }

    /// Le chiavi fidate di «Ascolta insieme», e se sono bloccate.
    fn fidati(c: &Connection) -> Vec<(String, bool)> {
        crate::insieme::pari(c)
            .unwrap()
            .into_iter()
            .map(|p| (p.chiave, p.bloccato))
            .collect()
    }

    #[test]
    fn dimenticare_un_dispositivo_toglie_la_fiducia_che_il_qr_gli_aveva_dato() {
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        let insieme = INSIEME.to_ascii_lowercase();
        // Il QR: il dispositivo, e la sua chiave di «Ascolta insieme» fidata.
        let telefono = "0f".repeat(32);
        associa(c, nuovo(&telefono, &[]), 1000).unwrap();
        crate::insieme::fidato_dal_qr(c, INSIEME, "Telefono", 1000).unwrap();
        crate::insieme::ricorda_indirizzo(c, &insieme, "192.168.1.5:47811", 1000).unwrap();
        // Un PC di «Ascolta insieme» che col dispositivo non c'entra.
        let estraneo = "cd".repeat(32);
        crate::insieme::associa(c, &estraneo, "Altro PC", 1000).unwrap();

        dimentica(c, &telefono).unwrap();
        assert_eq!(fidati(c), vec![(estraneo.clone(), false)]);
        assert!(
            crate::insieme::indirizzi_ricordati(c).unwrap().is_empty(),
            "nemmeno il suo indirizzo"
        );
        dimentica(c, &telefono).unwrap();

        // Un PC che la persona aveva bloccato resta bloccato: tolto, alla
        // prossima associazione tornerebbe fidato.
        let pc = "1f".repeat(32);
        let mut col_bloccato = nuovo(&pc, &[]);
        col_bloccato.chiave_insieme = Some(&estraneo);
        associa(c, col_bloccato, 2000).unwrap();
        let id = fidati_id(c, &estraneo);
        crate::insieme::blocca(c, id, true).unwrap();
        dimentica(c, &pc).unwrap();
        assert_eq!(fidati(c), vec![(estraneo, true)]);
    }

    fn fidati_id(c: &Connection, chiave: &str) -> i64 {
        crate::insieme::pari(c)
            .unwrap()
            .into_iter()
            .find(|p| p.chiave == chiave)
            .unwrap()
            .id
    }

    #[test]
    fn un_dimenticami_dall_altra_parte_non_tocca_ascolta_insieme() {
        // La chiave di «Ascolta insieme» l'aveva dichiarata lui: con un
        // «dimenticami» non deve poter togliere la fiducia a un altro PC.
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        let telefono = "0f".repeat(32);
        associa(c, nuovo(&telefono, &[]), 1000).unwrap();
        crate::insieme::fidato_dal_qr(c, INSIEME, "Telefono", 1000).unwrap();
        revoca(c, &telefono).unwrap();
        assert_eq!(fidati(c), vec![(INSIEME.to_ascii_lowercase(), false)]);
        assert!(
            uno(c, &telefono).unwrap().unwrap().revocato,
            "la riga resta"
        );
    }

    #[test]
    fn un_ascolto_si_manda_una_volta_e_si_conta_una_volta() {
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        let pc = "1a".repeat(32);
        let ascolto = AscoltoLontano {
            brano: 42,
            quando_ms: 10,
            ascoltato_ms: 200_000,
            chiave: "k1".to_owned(),
        };
        ricorda_ascolto(c, &pc, &ascolto).unwrap();
        ricorda_ascolto(c, &pc, &ascolto).unwrap();
        assert_eq!(ascolti_da_mandare(c, &pc).unwrap(), vec![ascolto.clone()]);
        ascolto_arrivato(c, "k1").unwrap();
        assert!(ascolti_da_mandare(c, &pc).unwrap().is_empty());

        // Dall'altra parte.
        assert!(ascolto_nuovo(c, "k1", 1_000).unwrap());
        assert!(!ascolto_nuovo(c, "k1", 2_000).unwrap(), "già contato");
        assert!(
            ascolto_nuovo(c, "k1", 1_000 + MEMORIA_RICEVUTI_MS + 1).unwrap(),
            "dopo un mese la chiave si dimentica"
        );
    }

    #[test]
    fn gli_ascolti_ricevuti_si_contano_per_dispositivo() {
        let db = open_in_memory().unwrap();
        let c = &db.connection;
        let uno = "1a".repeat(32);
        let altro = "1b".repeat(32);
        for (chiave, quando) in [
            (format!("{uno}:a"), 100),
            (format!("{uno}:b"), 200),
            (format!("{altro}:a"), 200),
        ] {
            assert!(ascolto_nuovo(c, &chiave, quando).unwrap());
        }
        assert_eq!(ricevuti_da(c, &uno.to_ascii_uppercase(), 0).unwrap(), 2);
        assert_eq!(ricevuti_da(c, &uno, 150).unwrap(), 1, "solo da allora");
        assert_eq!(ricevuti_da(c, &altro, 0).unwrap(), 1);
        assert_eq!(ricevuti_da(c, &"1c".repeat(32), 0).unwrap(), 0);
    }
}
