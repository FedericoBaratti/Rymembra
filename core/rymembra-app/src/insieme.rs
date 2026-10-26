//! «Ascolta insieme» nella libreria: i PC associati, le impostazioni, e la
//! ricerca di una copia identica di un brano (fase 3).
//!
//! Le firme e i tipi pubblici sono un contratto: il desktop (`src-tauri/src/
//! insieme`) li usa così come sono. Si aggiunge, non si cambia.
//!
//! # Cosa non sta qui
//!
//! La chiave segreta di questo PC: sta nel portachiavi di Windows, mai nel
//! database. Qui ci sono solo le chiavi **pubbliche** degli altri PC.
//!
//! Nemmeno la rete: questo crate non dipende da `rymembra-insieme`, e le poche
//! costanti del trasporto che servono qui — la porta di serie, i tetti dei
//! campi che viaggiano — sono ripetute con il loro nome accanto. Dipendere dal
//! trasporto per tre numeri vorrebbe dire portare `snow` dentro ogni prova
//! della libreria.
//!
//! # Fuori da profili, backup e sincronia
//!
//! La tabella `insieme_pari` e le chiavi `insieme.*` sono fatti di **questa**
//! macchina: un profilo esportato o un database ripristinato su un altro PC
//! non deve portarsi dietro le sue associazioni. Nessuno dei tre le esclude con
//! una riga apposta, ed è il punto: il profilo porta solo le chiavi del suo
//! `CATALOGO` e solo le tabelle che elenca, il backup e la sincronia hanno una
//! forma fissa. Le prove che lo tengono vero stanno in ciascuno dei tre.
//!
//! # Il diario e gli errori
//!
//! Nessun errore di questo modulo porta un indirizzo, un nome o una chiave: un
//! errore finisce nel diario, e la regola di «Ascolta insieme» è che il diario
//! contiene codici, conteggi e `pari #id`. L'indirizzo sbagliato si nomina per
//! **posizione** («il 3»), che basta a chi lo sta correggendo.

use std::io::{Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::PathRules;

use crate::library::db_error;
use crate::settings;

/// L'interruttore di «Ascolta insieme». Spento di serie.
pub const CHIAVE_ATTIVO: &str = "insieme.attivo";
/// Il nome di questo PC per gli altri. Di serie il nome del computer.
pub const CHIAVE_NOME: &str = "insieme.nome";
/// Gli indirizzi scritti a mano, JSON `["ip:porta", …]`.
pub const CHIAVE_INDIRIZZI: &str = "insieme.indirizzi";
/// Il dialogo che spiega la richiesta del firewall è già stato visto.
pub const CHIAVE_FIREWALL: &str = "insieme.firewall_spiegato";

/// Quanti caratteri può avere un nome.
///
/// Lo stesso tetto di `rymembra_insieme::messaggi::MASSIMO_NOME`, contato allo
/// stesso modo: **caratteri** e non byte. Il trasporto rifiuta un messaggio con
/// un nome più lungo chiudendo la connessione (`insieme.protocollo`), quindi un
/// nome tagliato qui in byte e lì contato in caratteri andrebbe bene; tagliato
/// qui in caratteri e lì contato in byte, un «Città di Castello» un po' più
/// lungo farebbe cadere ogni sessione senza un perché visibile.
pub const MASSIMO_NOME: usize = 64;

/// Quanti indirizzi scritti a mano si tengono.
///
/// Sedici, come i vicini a cui la scoperta apre una connessione: un indirizzo
/// a mano è una domanda in più a ogni giro, e chi ne scrive più di sedici sta
/// scrivendo una rete intera, che è il mestiere del broadcast.
pub const MASSIMO_INDIRIZZI: usize = 16;

/// La porta quando un indirizzo non la dice.
///
/// È `rymembra_insieme::PORTA`, ripetuta perché questo crate non vede il
/// trasporto (vedi il `//!`). Se un giorno cambia di là, cambia qui: è la porta
/// che un host prende per prima, e un indirizzo senza porta vuol dire «l'host
/// che sta sulla porta di sempre».
pub const PORTA_DI_SERIE: u16 = 47811;

/// Il canale con cui un indirizzo storto torna alla finestra.
const CANALE_INDIRIZZI: &str = "insieme_indirizzi";

/// Il canale con cui un nome vuoto torna alla finestra.
const CANALE_RINOMINA: &str = "insieme_pari_rinomina";

/// Un PC associato: una riga di `insieme_pari`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pari {
    /// L'id della riga.
    pub id: i64,
    /// La chiave pubblica, 64 cifre esadecimali.
    pub chiave: String,
    /// Il nome: quello scelto qui, o quello che il PC dichiarava.
    pub nome: String,
    /// Bloccato: non ci si collega e non si accetta niente.
    pub bloccato: bool,
    /// Quando è stato associato, ms dall'epoca.
    pub associato_ms: i64,
    /// L'ultima volta che si è visto, ms dall'epoca.
    pub visto_ms: Option<i64>,
}

/// Le impostazioni di «Ascolta insieme».
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Impostazioni {
    /// L'interruttore.
    pub attivo: bool,
    /// Il nome scelto, se ce n'è uno.
    pub nome: Option<String>,
    /// Gli indirizzi scritti a mano, già controllati (`ip:porta`).
    pub indirizzi: Vec<String>,
    /// Il dialogo del firewall è già stato visto.
    pub firewall_spiegato: bool,
}

/// Fase 3: com'è fatto un file, per riconoscerne una copia identica. Gemello
/// di `rymembra_insieme::messaggi::DescrittoreCopia`; la conversione sta nel
/// desktop, perché questo crate non conosce il trasporto.
///
/// Quel che [`descrittore`] produce passa già la `valida()` del trasporto: un
/// percorso o una chiave che non ci starebbero escono `None`, invece di far
/// rifiutare l'intero stato della sessione all'altro capo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descrittore {
    /// Il percorso, **solo** se è una condivisione `\\server\…`.
    pub unc: Option<String>,
    /// Quanti byte.
    pub dimensione: u64,
    /// L'ultima modifica, ms dall'epoca Unix.
    pub modificato_ms: i64,
    /// blake3 di dimensione ‖ primi 64 KiB ‖ ultimi 64 KiB, in esadecimale.
    pub impronta_rapida: String,
    /// La chiave di contenuto della libreria, se il brano ne ha una.
    pub chiave_contenuto: Option<String>,
}

// ── le impostazioni ─────────────────────────────────────────────────────────

/// Le impostazioni. Valori mancanti o illeggibili valgono quelli di serie.
///
/// Anche gli indirizzi si ricontrollano uno per uno, e quello che non si legge
/// si lascia cadere da solo invece di far cadere l'elenco: un database toccato
/// a mano, o scritto da una versione con regole diverse, non deve spegnere la
/// scoperta verso gli altri quindici.
///
/// # Errori
///
/// Quelli del database.
pub fn impostazioni(connection: &Connection) -> Result<Impostazioni, AppError> {
    let attivo = settings::read_json::<bool>(connection, CHIAVE_ATTIVO)?.unwrap_or(false);
    let nome = settings::read(connection, CHIAVE_NOME)?
        .map(|grezzo| ripulisci_nome(&grezzo))
        .filter(|nome| !nome.is_empty());
    let mut indirizzi: Vec<String> = Vec::new();
    for grezzo in
        settings::read_json::<Vec<String>>(connection, CHIAVE_INDIRIZZI)?.unwrap_or_default()
    {
        if indirizzi.len() >= MASSIMO_INDIRIZZI {
            break;
        }
        if let Some(buono) = indirizzo(&grezzo)
            && !indirizzi.contains(&buono)
        {
            indirizzi.push(buono);
        }
    }
    let firewall_spiegato =
        settings::read_json::<bool>(connection, CHIAVE_FIREWALL)?.unwrap_or(false);
    Ok(Impostazioni {
        attivo,
        nome,
        indirizzi,
        firewall_spiegato,
    })
}

/// Accende o spegne.
///
/// # Errori
///
/// Quelli del database.
pub fn salva_attivo(connection: &Connection, attivo: bool) -> Result<(), AppError> {
    settings::write_json(connection, CHIAVE_ATTIVO, &attivo)
}

/// Il nome di questo PC: ripulito (spazi, caratteri di controllo) e tagliato a
/// 64 caratteri. Vuoto torna al nome di serie.
///
/// «Torna al nome di serie» vuol dire che la chiave si **toglie**, e non che ci
/// si scrive dentro una stringa vuota: il nome di serie è quello del computer,
/// lo decide il desktop, e una riga vuota lo coprirebbe. È la ragione di
/// [`settings::forget`].
///
/// # Errori
///
/// Quelli del database.
pub fn salva_nome(connection: &Connection, nome: &str) -> Result<(), AppError> {
    let pulito = ripulisci_nome(nome);
    if pulito.is_empty() {
        return settings::forget(connection, CHIAVE_NOME).map(|_| ());
    }
    settings::write(connection, CHIAVE_NOME, &pulito)
}

/// Gli indirizzi scritti a mano: ognuno `ip:porta` (IPv4, porta da 1 a
/// 65535; senza porta vale quella di serie), al più 16.
///
/// Le righe vuote si saltano — chi scrive in un riquadro di testo lascia un a
/// capo in fondo — e i doppioni si tolgono dopo averli scritti nella stessa
/// forma, così `10.0.0.5` e `10.0.0.5:47811` sono uno. Restituisce l'elenco
/// come è stato salvato, che è quello da mostrare.
///
/// Si rifiutano anche gli indirizzi che si leggono ma non possono essere un
/// host: `0.0.0.0`, un multicast, il broadcast limitato (che la scoperta manda
/// già da sé). Il **loopback** invece passa: due istanze sullo stesso PC si
/// trovano solo così, ed è la prova che si fa prima di ogni rilascio.
///
/// # Errori
///
/// `ipc.payloadInvalid` se uno non si legge, con la sua **posizione** e non il
/// suo testo (vedi il `//!`), o se sono più di [`MASSIMO_INDIRIZZI`]: un
/// elenco tagliato in silenzio sarebbe un indirizzo scritto e mai provato. Su
/// un errore non si scrive niente, e resta l'elenco di prima. Poi quelli del
/// database.
pub fn salva_indirizzi(
    connection: &Connection,
    indirizzi: &[String],
) -> Result<Vec<String>, AppError> {
    let mut buoni: Vec<String> = Vec::new();
    for (posto, grezzo) in indirizzi.iter().enumerate() {
        if grezzo.trim().is_empty() {
            continue;
        }
        let Some(buono) = indirizzo(grezzo) else {
            return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
                channel: CANALE_INDIRIZZI.to_owned(),
                detail: Some(format!(
                    "l'indirizzo {} non è un ip:porta IPv4 raggiungibile",
                    posto.saturating_add(1)
                )),
            }));
        };
        if !buoni.contains(&buono) {
            buoni.push(buono);
        }
        // Dentro il giro e non dopo: l'elenco arriva dalla finestra, e un
        // elenco di centomila righe farebbe di `contains` un lavoro quadratico
        // sul filo dei comandi prima di sentirsi dire di no.
        if buoni.len() > MASSIMO_INDIRIZZI {
            return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
                channel: CANALE_INDIRIZZI.to_owned(),
                detail: Some(format!("più di {MASSIMO_INDIRIZZI} indirizzi diversi")),
            }));
        }
    }
    if buoni.is_empty() {
        settings::forget(connection, CHIAVE_INDIRIZZI)?;
    } else {
        settings::write_json(connection, CHIAVE_INDIRIZZI, &buoni)?;
    }
    Ok(buoni)
}

/// Il dialogo del firewall è stato visto.
///
/// # Errori
///
/// Quelli del database.
pub fn segna_firewall_spiegato(connection: &Connection) -> Result<(), AppError> {
    settings::write_json(connection, CHIAVE_FIREWALL, &true)
}

/// Un indirizzo scritto a mano, nella forma in cui si salva; `None` se non può
/// essere quello di un host.
///
/// Solo IPv4, come dice la scoperta: la domanda unicast parte da un socket
/// IPv4, e un `::1` scritto qui sarebbe un indirizzo accettato e mai
/// interrogato. Niente nomi di macchina: risolverli vorrebbe dire una domanda
/// al DNS a ogni giro di scoperta, e un nome che non si risolve è un guasto
/// silenzioso in più da spiegare.
fn indirizzo(grezzo: &str) -> Option<String> {
    let testo = grezzo.trim();
    let presa = match testo.parse::<SocketAddrV4>() {
        Ok(presa) => presa,
        Err(_) => SocketAddrV4::new(testo.parse::<Ipv4Addr>().ok()?, PORTA_DI_SERIE),
    };
    let ip = presa.ip();
    if presa.port() == 0 || ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        return None;
    }
    Some(presa.to_string())
}

/// Un nome come lo vedranno gli altri: senza caratteri di controllo, con gli
/// spazi ridotti a uno solo e niente in testa e in coda, al più
/// [`MASSIMO_NOME`] caratteri.
///
/// Ogni carattere di controllo e ogni spazio — anche quelli larghi, anche le
/// tabulazioni — diventa **uno** spazio: un a capo incollato dentro il nome
/// spezzerebbe la riga dell'elenco dei vicini, e il trasporto lo rifiuterebbe
/// comunque.
///
/// Spariscono anche i comandi di direzione (U+202A–202E, U+2066–2069) e i due
/// caratteri invisibili (U+200B, U+FEFF). Non sono di controllo per Unicode, e
/// sono il trucco classico per far leggere un nome al contrario: «PC di
/// Marco» che a schermo diventa un altro nome. I nomi arrivano anche dagli
/// altri PC, che sono fidati per il codice ma non per quel che scrivono. Il
/// legante di larghezza zero (U+200D) e il non legante (U+200C) restano: il
/// primo tiene insieme le emoji composte, il secondo serve a scrivere in
/// persiano.
///
/// Il taglio è prima dello spazio che non ci starebbe: un nome tagliato non
/// finisce mai con uno spazio.
fn ripulisci_nome(grezzo: &str) -> String {
    let mut pulito = String::new();
    let mut quanti = 0_usize;
    let mut spazio_in_sospeso = false;
    for carattere in grezzo.chars() {
        if carattere.is_control() || carattere.is_whitespace() {
            spazio_in_sospeso = !pulito.is_empty();
            continue;
        }
        if matches!(
            carattere,
            '\u{200B}' | '\u{FEFF}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        ) {
            continue;
        }
        let servono = if spazio_in_sospeso { 2 } else { 1 };
        if quanti.saturating_add(servono) > MASSIMO_NOME {
            break;
        }
        if spazio_in_sospeso {
            pulito.push(' ');
            spazio_in_sospeso = false;
        }
        pulito.push(carattere);
        quanti = quanti.saturating_add(servono);
    }
    pulito
}

// ── i PC associati ──────────────────────────────────────────────────────────

/// Tutti i PC associati, i più recenti prima.
///
/// «Recenti» per data d'associazione, e a pari data per riga: è l'ordine in
/// cui la persona li ha aggiunti, l'unico che non cambia da solo mentre
/// guarda. L'ordine per «visto adesso» lo fa l'interfaccia, che sa chi è in
/// linea.
///
/// # Errori
///
/// Quelli del database.
pub fn pari(connection: &Connection) -> Result<Vec<Pari>, AppError> {
    let guasto = |err: rusqlite::Error| db_error("elenco dei PC associati", &err);
    let mut istruzione = connection
        .prepare(
            "SELECT id, chiave, nome, bloccato, associato_ms, visto_ms
               FROM insieme_pari
              ORDER BY associato_ms DESC, id DESC",
        )
        .map_err(guasto)?;
    let righe = istruzione
        .query_map([], |riga| {
            Ok(Pari {
                id: riga.get(0)?,
                chiave: riga.get(1)?,
                nome: riga.get(2)?,
                bloccato: riga.get::<_, i64>(3)? != 0,
                associato_ms: riga.get(4)?,
                visto_ms: riga.get(5)?,
            })
        })
        .map_err(guasto)?;
    righe.collect::<Result<Vec<_>, _>>().map_err(guasto)
}

/// Ricorda un PC appena associato. Se la chiave c'era già, aggiorna nome e
/// data e lo **sblocca**: riassociarsi a mano è la prova che lo si vuole.
///
/// Anche `visto_ms` diventa adesso: ci si è appena parlati.
///
/// La chiave si scrive in minuscolo, la forma del trasporto e l'unica che il
/// `CHECK` della 035 lascia entrare: una chiave in maiuscolo sarebbe un
/// secondo PC con la stessa identità, cioè un blocco che si aggira.
///
/// Il nome dichiarato passa da [`ripulisci_nome`] come quello scelto qui. Se
/// non ne resta niente si usano le prime otto cifre della chiave: un PC senza
/// nome deve restare distinguibile nell'elenco, e otto cifre non hanno
/// bisogno di traduzione.
///
/// # Errori
///
/// `insieme.protocollo` se la chiave non è di 64 cifre esadecimali — viene
/// dal trasporto, che la scrive sempre così, quindi una chiave storta è un
/// messaggio che non rispetta il protocollo. Poi quelli del database.
pub fn associa(
    connection: &Connection,
    chiave: &str,
    nome: &str,
    adesso_ms: i64,
) -> Result<i64, AppError> {
    let chiave = chiave_valida(chiave)?;
    let mut nome = ripulisci_nome(nome);
    if nome.is_empty() {
        nome = chiave.chars().take(8).collect();
    }
    connection
        .query_row(
            "INSERT INTO insieme_pari (chiave, nome, bloccato, associato_ms, visto_ms)
             VALUES (?1, ?2, 0, ?3, ?3)
             ON CONFLICT(chiave) DO UPDATE SET
               nome = excluded.nome,
               bloccato = 0,
               associato_ms = excluded.associato_ms,
               visto_ms = excluded.visto_ms
             RETURNING id",
            rusqlite::params![chiave, nome, adesso_ms],
            |riga| riga.get(0),
        )
        .map_err(|err| db_error("associazione di un PC", &err))
}

/// Rinomina un PC associato.
///
/// # Errori
///
/// `ipc.payloadInvalid` se il nome, ripulito, è vuoto: al contrario di quello
/// di questo PC non c'è un nome di serie a cui tornare. `insieme.nonAssociato`
/// se la riga non c'è (anche per un id ≤ 0, che non è mai una riga). Poi
/// quelli del database.
pub fn rinomina(connection: &Connection, id: i64, nome: &str) -> Result<(), AppError> {
    let nome = ripulisci_nome(nome);
    if nome.is_empty() {
        return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: CANALE_RINOMINA.to_owned(),
            detail: Some("il nome è vuoto".to_owned()),
        }));
    }
    let toccate = connection
        .execute(
            "UPDATE insieme_pari SET nome = ?2 WHERE id = ?1",
            rusqlite::params![id, nome],
        )
        .map_err(|err| db_error("nome di un PC associato", &err))?;
    c_era(id, toccate)
}

/// Blocca o sblocca un PC associato.
///
/// Bloccato resta in elenco e non si dimentica: dimenticarlo vorrebbe dire
/// che alla prossima associazione — sua, non nostra — tornerebbe fidato.
///
/// # Errori
///
/// `insieme.nonAssociato` se la riga non c'è. Poi quelli del database.
pub fn blocca(connection: &Connection, id: i64, bloccato: bool) -> Result<(), AppError> {
    let toccate = connection
        .execute(
            "UPDATE insieme_pari SET bloccato = ?2 WHERE id = ?1",
            rusqlite::params![id, i64::from(bloccato)],
        )
        .map_err(|err| db_error("blocco di un PC associato", &err))?;
    c_era(id, toccate)
}

/// Dimentica un PC associato.
///
/// Dimenticare due volte è dimenticare una volta: un PC che non c'è non è un
/// errore, come per [`settings::forget`]. Il doppio clic su «Dimentica» non
/// deve diventare un avviso.
///
/// # Errori
///
/// Quelli del database.
pub fn dimentica(connection: &Connection, id: i64) -> Result<(), AppError> {
    connection
        .execute("DELETE FROM insieme_pari WHERE id = ?1", [id])
        .map(|_| ())
        .map_err(|err| db_error("PC associato da dimenticare", &err))
}

/// Segna che un PC associato si è visto adesso.
///
/// Un PC dimenticato nel frattempo non è un errore: la connessione che lo ha
/// visto è partita prima che lo si dimenticasse, e se ne andrà da sola.
///
/// # Errori
///
/// Quelli del database.
pub fn visto(connection: &Connection, id: i64, adesso_ms: i64) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE insieme_pari SET visto_ms = ?2 WHERE id = ?1",
            rusqlite::params![id, adesso_ms],
        )
        .map(|_| ())
        .map_err(|err| db_error("PC associato visto", &err))
}

/// La chiave in minuscolo, o `insieme.protocollo`.
///
/// La causa dice quanto era lunga e basta: la chiave stessa non deve finire
/// nel diario, nemmeno quando è sbagliata.
fn chiave_valida(chiave: &str) -> Result<String, AppError> {
    let chiave = chiave.trim().to_ascii_lowercase();
    if chiave.len() == 64 && chiave.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Ok(chiave);
    }
    Err(
        AppError::new(ErrorCode::InsiemeProtocollo).with_cause(format!(
            "chiave pubblica di {} caratteri, non 64 cifre esadecimali",
            chiave.chars().count()
        )),
    )
}

/// `Ok` se l'aggiornamento ha toccato la riga, `insieme.nonAssociato` se no.
fn c_era(id: i64, toccate: usize) -> Result<(), AppError> {
    if toccate > 0 {
        return Ok(());
    }
    Err(AppError::new(ErrorCode::InsiemeNonAssociato).with_cause(format!("pari #{id}")))
}

// ── fase 3: la copia locale ─────────────────────────────────────────────────

/// Quanti byte dalla testa e quanti dalla coda entrano nell'impronta rapida.
///
/// 64 KiB per parte: abbastanza da coprire i tag in testa (ID3v2, i blocchi
/// FLAC) e quelli in coda (ID3v1, APE), cioè le parti che cambiano quando
/// qualcuno ritagga un file senza toccare l'audio — ed è proprio quello che
/// rende «stesso contenuto» diverso da «stesso file». Su un NAS sono due
/// letture brevi invece di un brano intero.
const PARTE_IMPRONTA: u64 = 64 * 1024;

/// Di quanto può differire la durata di una copia, in millisecondi.
///
/// Mezzo secondo, come dice il piano: due letture della stessa durata con due
/// lettori diversi — o con lo stesso lettore prima e dopo un aggiornamento —
/// possono stimare un MP3 senza intestazione Xing in modi diversi, e una
/// tolleranza più stretta lascerebbe fuori copie vere. L'impronta, dopo, è
/// quella che decide davvero.
const TOLLERANZA_DURATA_MS: i64 = 500;

/// Quanti candidati «da verificare» si restituiscono al più.
///
/// Ognuno costa a chi chiama due letture da 64 KiB, magari su una share:
/// quattro bastano per i doppioni veri (un brano in due cartelle, una
/// raccolta che ripete un singolo), e un tetto evita che una chiave di
/// contenuto troppo generica — «Traccia 1» di cento dischi senza tag — diventi
/// cento aperture di file mentre il brano prima sta finendo.
const MASSIMO_DA_VERIFICARE: usize = 4;

/// Quanti caratteri può avere il percorso UNC che viaggia.
///
/// `rymembra_insieme::messaggi::MASSIMO_RIFERIMENTO`, ripetuto (vedi il `//!`).
const MASSIMO_UNC: usize = 2048;

/// Quanti caratteri può avere la chiave di contenuto che viaggia.
///
/// Il tetto che la `valida()` del trasporto mette a `chiave_contenuto`.
const MASSIMO_CHIAVE: usize = 128;

/// Fase 3, chi possiede il brano: il descrittore del suo file, letto dal
/// disco (128 KiB). Niente database: il percorso e la chiave di contenuto li
/// legge prima chi chiama, così la libreria non resta occupata mentre il disco
/// — magari quello di un NAS — risponde.
///
/// `percorso` è il file che si apre davvero: per un brano d'immagine CD è
/// l'immagine, non il percorso virtuale `…\Album.cue|NN`.
///
/// Dimensione e data vengono dal file **aperto**, non da un'altra lettura del
/// percorso: fra le due il file potrebbe cambiare, e l'impronta direbbe un
/// file e la dimensione un altro. La data è nella forma della scansione
/// (`files::modified_ms`), perché è con `tracks.date_modified` che l'altro PC
/// la confronta.
///
/// # Errori
///
/// Quelli del disco.
pub fn descrittore(
    percorso: &std::path::Path,
    chiave_contenuto: Option<&str>,
) -> std::io::Result<Descrittore> {
    let mut file = std::fs::File::open(crate::files::percorso_lungo(percorso))?;
    let metadati = file.metadata()?;
    let dimensione = metadati.len();
    let impronta_rapida = impronta(&mut file, dimensione)?;
    Ok(Descrittore {
        unc: percorso.to_str().and_then(unc_di),
        dimensione,
        modificato_ms: crate::files::modified_ms(&metadati),
        impronta_rapida,
        chiave_contenuto: chiave_contenuto
            .filter(|chiave| {
                !chiave.is_empty()
                    && chiave.chars().count() <= MASSIMO_CHIAVE
                    && !chiave.chars().any(char::is_control)
            })
            .map(str::to_owned),
    })
}

/// Il percorso come può viaggiare, se è una condivisione di rete.
///
/// Un percorso locale non esce mai da questo PC: direbbe a tutti come sono
/// fatte le cartelle di chi ospita, e su un altro PC `D:\Musica` è un altro
/// disco. Una condivisione invece è lo stesso posto per tutti.
///
/// La forma è quella che la `valida()` del trasporto pretende — comincia con
/// `\\` — quindi la forma verbatim (`\\?\UNC\nas\…`) perde il prefisso e gli
/// slash diventano barre rovesciate. Un percorso troppo lungo o con un
/// carattere di controllo resta a casa: farlo viaggiare farebbe rifiutare
/// all'altro capo l'intero stato della sessione.
fn unc_di(testo: &str) -> Option<String> {
    if !crate::files::percorso_di_rete(Path::new(testo)) {
        return None;
    }
    // Prima le barre e poi il prefisso: `//?/UNC/nas/…` è la stessa forma
    // verbatim scritta con gli slash, e al contrario resterebbe verbatim.
    let unc = crate::volumi::senza_verbatim(&testo.replace('/', "\\"));
    (unc.starts_with(r"\\")
        && unc.chars().count() <= MASSIMO_UNC
        && !unc.chars().any(char::is_control))
    .then_some(unc)
}

/// L'impronta rapida di un file aperto, lungo `dimensione` byte.
///
/// La formula è un **contratto fra due PC**, e va detta esatta: blake3 di
///
/// 1. `dimensione` come `u64` little-endian (8 byte);
/// 2. i primi `min(dimensione, 64 KiB)` byte;
/// 3. gli ultimi `min(dimensione, 64 KiB)` byte.
///
/// Su un file più corto di 128 KiB le due parti si sovrappongono, e va bene:
/// conta solo che i due capi facciano lo stesso conto. Cambiarla vuol dire
/// alzare la versione del protocollo, altrimenti due versioni diverse non si
/// riconoscerebbero mai una copia.
///
/// Un file che si accorcia mentre lo si legge è un errore e non un'impronta
/// più corta: un'impronta di un file a metà sarebbe un'impronta sbagliata.
///
/// Quel che l'impronta **non** vede è il mezzo del file: due file della stessa
/// dimensione, con testa e coda uguali e un pezzo d'audio diverso in mezzo,
/// hanno la stessa impronta. È il prezzo di leggere 128 KiB invece di cento
/// megabyte, e la dimensione identica al byte lo rende un caso da laboratorio.
fn impronta(file: &mut std::fs::File, dimensione: u64) -> std::io::Result<String> {
    let parte = dimensione.min(PARTE_IMPRONTA);
    let mut hasher = blake3::Hasher::new();
    hasher.update(&dimensione.to_le_bytes());

    file.seek(SeekFrom::Start(0))?;
    leggi_parte(file, parte, &mut hasher)?;
    file.seek(SeekFrom::Start(dimensione.saturating_sub(parte)))?;
    leggi_parte(file, parte, &mut hasher)?;

    Ok(hasher.finalize().to_hex().to_string())
}

/// Legge esattamente `quanti` byte dalla posizione corrente dentro l'impronta.
fn leggi_parte(
    file: &mut std::fs::File,
    quanti: u64,
    hasher: &mut blake3::Hasher,
) -> std::io::Result<()> {
    let mut letti = Vec::new();
    file.by_ref().take(quanti).read_to_end(&mut letti)?;
    if u64::try_from(letti.len()).ok() != Some(quanti) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "il file si è accorciato mentre se ne leggeva l'impronta",
        ));
    }
    hasher.update(&letti);
    Ok(())
}

/// Una riga che potrebbe essere una copia del file descritto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidato {
    /// La riga della libreria.
    pub track_id: i64,
    /// Il file da aprire (l'immagine, per un brano d'immagine CD).
    pub percorso: std::path::PathBuf,
    /// Stesso percorso UNC, stessa dimensione, stessa data: è lo stesso file,
    /// e l'impronta non serve.
    pub certa: bool,
}

/// Fase 3, chi ascolta: le righe di questa libreria che possono essere una
/// copia identica, migliori prima. Solo database:
///
/// 1. stesso percorso UNC, stessa dimensione e stessa data → `certa`;
/// 2. stessa chiave di contenuto, stessa dimensione e durata entro ±500 ms →
///    da verificare con [`e_la_stessa`].
///
/// # I brani d'immagine CD
///
/// Solo al primo passo, e con due regole loro:
///
/// - **la data è «non più vecchia»** invece che uguale. Per una riga
///   d'immagine `date_modified` è la più recente fra quella del `.cue` e
///   quella dell'immagine (vedi la 034), quindi non coincide con la data
///   dell'immagine quando il foglio è più nuovo. Un `>=` resta una prova:
///   l'immagine cambiata dopo la nostra scansione avrebbe una data più nuova
///   di entrambe, e non passerebbe;
/// - **la durata decide quale brano**. Un'immagine è un file per molte righe,
///   e il descrittore parla del file: fra le righe di quell'immagine passano
///   quelle entro ±500 ms dalla durata della voce, la più vicina per prima. È
///   la riga che conterà l'ascolto, e contarlo al brano accanto sarebbe un
///   ascolto inventato.
///
/// Al secondo passo un brano d'immagine non entra mai: la sua chiave di
/// contenuto è quella del brano, la dimensione quella del disco intero, e
/// un'immagine «uguale» trovata per chiave sarebbe un'altra copertina dello
/// stesso disco — il piano dice «CUE solo con copia certa». Non entrano
/// nemmeno i brani di catalogo, che non hanno un file da aprire.
///
/// Il confronto del percorso segue le regole di questo filesystem: su Windows
/// `\\NAS\Musica` e `\\nas\musica` sono lo stesso posto. Prima si cerca la
/// forma esatta, che ha l'indice di `UNIQUE(path)`; quella piegata solo se la
/// prima non trova niente, come fa `library::ids_by_path`.
///
/// Una dimensione o una durata che non stanno in un `i64` non sono un errore:
/// nessuna riga della libreria le ha, quindi nessun candidato.
///
/// # Errori
///
/// Quelli del database.
pub fn candidati_copia(
    connection: &Connection,
    descrittore: &Descrittore,
    durata_ms: u64,
) -> Result<Vec<Candidato>, AppError> {
    let (Ok(dimensione), Ok(durata)) = (
        i64::try_from(descrittore.dimensione),
        i64::try_from(durata_ms),
    ) else {
        return Ok(Vec::new());
    };
    let regole = PathRules::for_current_platform();
    let mut trovati: Vec<Candidato> = Vec::new();

    if let Some(unc) = descrittore.unc.as_deref() {
        if let Some(certo) = file_certo(
            connection,
            unc,
            dimensione,
            descrittore.modificato_ms,
            regole,
        )? {
            trovati.push(certo);
        }
        trovati.extend(brani_d_immagine_certi(
            connection,
            unc,
            dimensione,
            descrittore.modificato_ms,
            durata,
            regole,
        )?);
    }

    if let Some(chiave) = descrittore
        .chiave_contenuto
        .as_deref()
        .filter(|chiave| !chiave.is_empty())
    {
        let guasto = |err: rusqlite::Error| db_error("copie per chiave di contenuto", &err);
        let mut istruzione = connection
            .prepare_cached(
                "SELECT id, path FROM tracks
                  WHERE content_key = ?1 AND file_size = ?2
                    AND abs(duration_ms - ?3) <= ?4
                    AND immagine IS NULL AND path IS NOT NULL
                  ORDER BY abs(duration_ms - ?3), id",
            )
            .map_err(guasto)?;
        let righe = istruzione
            .query_map(
                rusqlite::params![chiave, dimensione, durata, TOLLERANZA_DURATA_MS],
                |riga| Ok((riga.get::<_, i64>(0)?, riga.get::<_, String>(1)?)),
            )
            .map_err(guasto)?;
        let mut da_verificare = 0_usize;
        for riga in righe {
            let (track_id, percorso) = riga.map_err(guasto)?;
            // Una riga già certa non si ripropone da verificare: chi chiama
            // aprirebbe due volte lo stesso file.
            if trovati.iter().any(|gia| gia.track_id == track_id) {
                continue;
            }
            if da_verificare >= MASSIMO_DA_VERIFICARE {
                break;
            }
            da_verificare = da_verificare.saturating_add(1);
            trovati.push(Candidato {
                track_id,
                percorso: PathBuf::from(percorso),
                certa: false,
            });
        }
    }
    Ok(trovati)
}

/// Il primo passo per un file normale: stesso percorso, stessa dimensione,
/// stessa data.
fn file_certo(
    connection: &Connection,
    unc: &str,
    dimensione: i64,
    modificato_ms: i64,
    regole: PathRules,
) -> Result<Option<Candidato>, AppError> {
    let guasto = |err: rusqlite::Error| db_error("copia certa per percorso", &err);
    let mut esatto = connection
        .prepare_cached(
            "SELECT id, path FROM tracks
              WHERE path = ?1 AND immagine IS NULL
                AND file_size = ?2 AND date_modified = ?3",
        )
        .map_err(guasto)?;
    let mut piegato = connection
        .prepare_cached(
            "SELECT id, path FROM tracks
              WHERE path = ?1 COLLATE NOCASE AND immagine IS NULL
                AND file_size = ?2 AND date_modified = ?3
              ORDER BY id LIMIT 1",
        )
        .map_err(guasto)?;
    let parametri = rusqlite::params![unc, dimensione, modificato_ms];
    let leggi = |riga: &rusqlite::Row<'_>| Ok((riga.get::<_, i64>(0)?, riga.get::<_, String>(1)?));
    let mut trovato = esatto
        .query_row(parametri, leggi)
        .optional()
        .map_err(guasto)?;
    if trovato.is_none() && regole.case_insensitive {
        trovato = piegato
            .query_row(parametri, leggi)
            .optional()
            .map_err(guasto)?;
    }
    Ok(trovato.map(|(track_id, percorso)| Candidato {
        track_id,
        percorso: PathBuf::from(percorso),
        certa: true,
    }))
}

/// Il primo passo per un'immagine CD: le sue righe con la durata giusta, la
/// più vicina prima. Vedi [`candidati_copia`] per le due regole.
///
/// La forma piegata qui legge solo le righe d'immagine (l'indice parziale
/// della 034 le tiene a parte), che sono poche anche su una libreria grande.
fn brani_d_immagine_certi(
    connection: &Connection,
    unc: &str,
    dimensione: i64,
    modificato_ms: i64,
    durata: i64,
    regole: PathRules,
) -> Result<Vec<Candidato>, AppError> {
    let guasto = |err: rusqlite::Error| db_error("copia certa di un'immagine", &err);
    let confronto = if regole.case_insensitive {
        "immagine = ?1 COLLATE NOCASE"
    } else {
        "immagine = ?1"
    };
    let sql = format!(
        "SELECT id, immagine FROM tracks
          WHERE immagine IS NOT NULL AND {confronto}
            AND file_size = ?2 AND date_modified >= ?3
            AND abs(duration_ms - ?4) <= ?5
          ORDER BY abs(duration_ms - ?4), id"
    );
    let mut istruzione = connection.prepare_cached(&sql).map_err(guasto)?;
    let righe = istruzione
        .query_map(
            rusqlite::params![unc, dimensione, modificato_ms, durata, TOLLERANZA_DURATA_MS],
            |riga| Ok((riga.get::<_, i64>(0)?, riga.get::<_, String>(1)?)),
        )
        .map_err(guasto)?;
    righe
        .map(|riga| {
            riga.map(|(track_id, percorso)| Candidato {
                track_id,
                percorso: PathBuf::from(percorso),
                certa: true,
            })
            .map_err(guasto)
        })
        .collect()
}

/// Fase 3: il file di qui ha la stessa impronta rapida? Solo disco; un file
/// che non si apre non è una copia.
///
/// La dimensione si guarda prima: un file di un'altra lunghezza non è una
/// copia, e lo si sa senza leggere un byte. La data no — una copia fatta con
/// Esplora risorse ha la data della copia, ed è la stessa musica.
#[must_use]
pub fn e_la_stessa(percorso: &std::path::Path, descrittore: &Descrittore) -> bool {
    let Ok(mut file) = std::fs::File::open(crate::files::percorso_lungo(percorso)) else {
        return false;
    };
    let Ok(metadati) = file.metadata() else {
        return false;
    };
    if metadati.len() != descrittore.dimensione {
        return false;
    }
    impronta(&mut file, descrittore.dimensione)
        .is_ok_and(|qui| qui.eq_ignore_ascii_case(&descrittore.impronta_rapida))
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_domain::errors::ErrorCodeKind;

    /// Una libreria vera, migrazioni comprese: così le prove passano dal
    /// `CHECK` della 035 e non da una tabella scritta a mano.
    fn libreria() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    const CHIAVE: &str = "3fa291c0aabbccddeeff00112233445566778899aabbccddeeff001122334455";
    const ALTRA: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    /// Nessuna di queste stringhe deve comparire in un errore: finirebbe nel
    /// diario.
    fn non_dice(errore: &AppError, segreti: &[&str]) {
        let tutto = format!("{errore:?} {errore}");
        for segreto in segreti {
            assert!(
                !tutto.contains(segreto),
                "«{segreto}» non deve stare in un errore: {tutto}"
            );
        }
    }

    // ── le impostazioni ──

    #[test]
    fn di_serie_e_spento_senza_nome_e_senza_indirizzi() {
        let c = libreria();
        assert_eq!(impostazioni(&c).expect("lette"), Impostazioni::default());
    }

    #[test]
    fn accendere_spegnere_e_il_firewall_restano_scritti() {
        let c = libreria();
        salva_attivo(&c, true).expect("acceso");
        segna_firewall_spiegato(&c).expect("visto");
        let lette = impostazioni(&c).expect("lette");
        assert!(lette.attivo);
        assert!(lette.firewall_spiegato);
        salva_attivo(&c, false).expect("spento");
        assert!(!impostazioni(&c).expect("rilette").attivo);
    }

    #[test]
    fn un_valore_illeggibile_vale_quello_di_serie() {
        let c = libreria();
        settings::write(&c, CHIAVE_ATTIVO, "forse").expect("scritto");
        settings::write(&c, CHIAVE_INDIRIZZI, "{non è json").expect("scritto");
        let lette = impostazioni(&c).expect("un valore storto non è un guasto");
        assert!(!lette.attivo);
        assert!(lette.indirizzi.is_empty());
    }

    #[test]
    fn il_nome_perde_i_controlli_e_gli_spazi_doppi() {
        assert_eq!(
            ripulisci_nome("  PC\tdi\n\n Marco\u{0007}  "),
            "PC di Marco",
            "ogni controllo e ogni corsa di spazi diventa uno spazio solo"
        );
        assert_eq!(
            ripulisci_nome("Sala\u{00A0}\u{3000}riunioni"),
            "Sala riunioni"
        );
    }

    #[test]
    fn il_nome_perde_i_comandi_di_direzione_e_tiene_le_emoji_composte() {
        // «ocraM» con U+202E davanti si leggerebbe al contrario: l'override
        // sparisce, e il nome resta quello che i caratteri dicono davvero.
        assert_eq!(ripulisci_nome("PC di \u{202E}ocraM"), "PC di ocraM");
        assert_eq!(ripulisci_nome("\u{FEFF}Cucina\u{200B}"), "Cucina");
        let famiglia = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(ripulisci_nome(famiglia), famiglia);
    }

    #[test]
    fn il_nome_si_taglia_a_64_caratteri_e_non_a_64_byte() {
        // Cento «à» sono duecento byte: tagliate a 64 **caratteri** passano la
        // `valida()` del trasporto, che conta allo stesso modo.
        let lungo = "à".repeat(100);
        let tagliato = ripulisci_nome(&lungo);
        assert_eq!(tagliato.chars().count(), MASSIMO_NOME);
        assert!(tagliato.len() > MASSIMO_NOME);

        // Il taglio non lascia uno spazio in coda.
        let con_spazio = format!("{} b", "a".repeat(63));
        assert_eq!(ripulisci_nome(&con_spazio), "a".repeat(63));
    }

    #[test]
    fn un_nome_vuoto_torna_a_quello_di_serie() {
        let c = libreria();
        salva_nome(&c, " Studio ").expect("salvato");
        assert_eq!(
            impostazioni(&c).expect("lette").nome.as_deref(),
            Some("Studio")
        );
        salva_nome(&c, " \t\u{202E} ").expect("vuoto");
        assert_eq!(impostazioni(&c).expect("rilette").nome, None);
        assert_eq!(
            settings::read(&c, CHIAVE_NOME).expect("letta"),
            None,
            "la chiave si toglie, non si svuota: il nome di serie lo decide il desktop"
        );
    }

    #[test]
    fn gli_indirizzi_si_leggono_e_si_scrivono_in_una_forma_sola() {
        let c = libreria();
        let salvati = salva_indirizzi(
            &c,
            &[
                "127.0.0.1:47812".to_owned(),
                " 192.168.1.20 ".to_owned(),
                String::new(),
                "192.168.1.20:47811".to_owned(),
            ],
        )
        .expect("salvati");
        assert_eq!(
            salvati,
            ["127.0.0.1:47812", "192.168.1.20:47811"],
            "la porta di serie si scrive, il doppione sparisce, la riga vuota si salta"
        );
        assert_eq!(impostazioni(&c).expect("lette").indirizzi, salvati);

        salva_indirizzi(&c, &[]).expect("svuotati");
        assert_eq!(settings::read(&c, CHIAVE_INDIRIZZI).expect("letta"), None);
    }

    #[test]
    fn un_indirizzo_storto_si_rifiuta_per_posizione_senza_dirlo() {
        let c = libreria();
        salva_indirizzi(&c, &["10.0.0.5".to_owned()]).expect("il primo elenco");
        for storto in [
            "nas.local",
            "10.0.0.7:0",
            "10.0.0.7:70000",
            "::1",
            "[::1]:47811",
            "0.0.0.0:47811",
            "224.0.0.251:47811",
            "255.255.255.255",
            "10.0.0.7:",
        ] {
            let errore =
                salva_indirizzi(&c, &["10.0.0.9".to_owned(), storto.to_owned()]).expect_err(storto);
            assert_eq!(
                errore.code().kind(),
                ErrorCodeKind::IpcPayloadInvalid,
                "{storto}"
            );
            assert!(
                format!("{:?}", errore.code()).contains("l'indirizzo 2"),
                "la posizione basta a chi corregge: {:?}",
                errore.code()
            );
            non_dice(&errore, &[storto, "10.0.0.9"]);
        }
        assert_eq!(
            impostazioni(&c).expect("lette").indirizzi,
            ["10.0.0.5:47811"],
            "su un errore resta l'elenco di prima"
        );
    }

    #[test]
    fn piu_di_sedici_indirizzi_si_rifiutano_invece_di_tagliarli() {
        let c = libreria();
        let troppi: Vec<String> = (1..=17).map(|n| format!("10.0.0.{n}")).collect();
        let errore = salva_indirizzi(&c, &troppi).expect_err("diciassette");
        assert_eq!(errore.code().kind(), ErrorCodeKind::IpcPayloadInvalid);
        let sedici = troppi.get(..16).expect("sedici").to_vec();
        assert_eq!(salva_indirizzi(&c, &sedici).expect("sedici").len(), 16);
    }

    #[test]
    fn un_indirizzo_storto_scritto_a_mano_si_scarta_da_solo() {
        let c = libreria();
        settings::write_json(
            &c,
            CHIAVE_INDIRIZZI,
            &["10.0.0.5:47811", "rotto", "10.0.0.5", "10.0.0.6:1"],
        )
        .expect("scritto a mano");
        assert_eq!(
            impostazioni(&c).expect("lette").indirizzi,
            ["10.0.0.5:47811", "10.0.0.6:1"]
        );
    }

    // ── i PC associati ──

    #[test]
    fn associare_ricorda_il_pari_e_lo_dice_visto() {
        let c = libreria();
        let id = associa(&c, CHIAVE, "PC di Marco", 1_000).expect("associato");
        assert_eq!(
            pari(&c).expect("elenco"),
            [Pari {
                id,
                chiave: CHIAVE.to_owned(),
                nome: "PC di Marco".to_owned(),
                bloccato: false,
                associato_ms: 1_000,
                visto_ms: Some(1_000),
            }]
        );
    }

    #[test]
    fn riassociarsi_aggiorna_la_stessa_riga_e_la_sblocca() {
        let c = libreria();
        let id = associa(&c, CHIAVE, "Vecchio", 1_000).expect("associato");
        blocca(&c, id, true).expect("bloccato");
        // In maiuscolo: è la stessa chiave, e deve essere la stessa riga.
        let di_nuovo =
            associa(&c, &CHIAVE.to_ascii_uppercase(), "Nuovo", 2_000).expect("riassociato");
        assert_eq!(di_nuovo, id);
        let elenco = pari(&c).expect("elenco");
        assert_eq!(elenco.len(), 1);
        let unico = elenco.first().expect("uno");
        assert_eq!(unico.nome, "Nuovo");
        assert!(
            !unico.bloccato,
            "riassociarsi a mano è la prova che lo si vuole"
        );
        assert_eq!(unico.associato_ms, 2_000);
        assert_eq!(
            unico.chiave, CHIAVE,
            "in minuscolo, come la scrive il trasporto"
        );
    }

    #[test]
    fn una_chiave_storta_non_entra_e_non_finisce_nell_errore() {
        let c = libreria();
        let corta = "abc123";
        let non_esadecimale = "z".repeat(64);
        for storta in [corta, non_esadecimale.as_str()] {
            let errore = associa(&c, storta, "X", 1).expect_err(storta);
            assert_eq!(errore.code().kind(), ErrorCodeKind::InsiemeProtocollo);
            non_dice(&errore, &[storta]);
        }
        assert!(pari(&c).expect("elenco").is_empty());
    }

    #[test]
    fn il_check_della_035_tiene_fuori_le_chiavi_in_maiuscolo() {
        // La guardia di sotto, quella che regge anche se un giorno qualcuno
        // scrive nella tabella senza passare da `associa`.
        let c = libreria();
        let esito = c.execute(
            "INSERT INTO insieme_pari (chiave, nome, associato_ms) VALUES (?1, 'X', 1)",
            [CHIAVE.to_ascii_uppercase()],
        );
        assert!(
            esito.is_err(),
            "una chiave in maiuscolo sarebbe un secondo PC"
        );
    }

    #[test]
    fn un_pari_senza_nome_prende_le_prime_cifre_della_chiave() {
        let c = libreria();
        associa(&c, CHIAVE, " \u{202E}\t", 1).expect("associato");
        assert_eq!(
            pari(&c).expect("elenco").first().map(|p| p.nome.clone()),
            Some("3fa291c0".to_owned())
        );
    }

    #[test]
    fn i_piu_recenti_vengono_prima() {
        let c = libreria();
        associa(&c, ALTRA, "Primo", 1_000).expect("primo");
        associa(&c, CHIAVE, "Secondo", 2_000).expect("secondo");
        let nomi: Vec<String> = pari(&c)
            .expect("elenco")
            .into_iter()
            .map(|p| p.nome)
            .collect();
        assert_eq!(nomi, ["Secondo", "Primo"]);
    }

    #[test]
    fn rinominare_bloccare_e_vedere_toccano_solo_quella_riga() {
        let c = libreria();
        let id = associa(&c, CHIAVE, "A", 1_000).expect("associato");
        let altro = associa(&c, ALTRA, "B", 1_000).expect("l'altro");
        rinomina(&c, id, "  Salotto\n").expect("rinominato");
        blocca(&c, id, true).expect("bloccato");
        visto(&c, id, 5_000).expect("visto");
        let elenco = pari(&c).expect("elenco");
        let questo = elenco.iter().find(|p| p.id == id).expect("c'è");
        assert_eq!(questo.nome, "Salotto");
        assert!(questo.bloccato);
        assert_eq!(questo.visto_ms, Some(5_000));
        let l_altro = elenco.iter().find(|p| p.id == altro).expect("c'è");
        assert_eq!((l_altro.nome.as_str(), l_altro.bloccato), ("B", false));
    }

    #[test]
    fn un_pari_che_non_c_e_non_si_rinomina_e_non_si_blocca() {
        let c = libreria();
        for id in [0, -3, 42] {
            for errore in [
                rinomina(&c, id, "X").expect_err("rinomina"),
                blocca(&c, id, true).expect_err("blocca"),
            ] {
                assert_eq!(errore.code().kind(), ErrorCodeKind::InsiemeNonAssociato);
            }
            // Dimenticare e vedere un pari che non c'è non sono guasti.
            dimentica(&c, id).expect("dimentica a vuoto");
            visto(&c, id, 1).expect("visto a vuoto");
        }
    }

    #[test]
    fn un_nome_vuoto_non_rinomina() {
        let c = libreria();
        let id = associa(&c, CHIAVE, "Cucina", 1).expect("associato");
        let errore = rinomina(&c, id, " \u{2066} ").expect_err("vuoto");
        assert_eq!(errore.code().kind(), ErrorCodeKind::IpcPayloadInvalid);
        assert_eq!(
            pari(&c).expect("elenco").first().map(|p| p.nome.clone()),
            Some("Cucina".to_owned())
        );
    }

    #[test]
    fn dimenticare_toglie_la_riga_e_due_volte_e_come_una() {
        let c = libreria();
        let id = associa(&c, CHIAVE, "A", 1).expect("associato");
        dimentica(&c, id).expect("dimenticato");
        dimentica(&c, id).expect("di nuovo");
        assert!(pari(&c).expect("elenco").is_empty());
    }

    // ── fase 3: il descrittore e l'impronta ──

    /// Un file di `quanti` byte che non si ripetono, così una parte spostata
    /// cambia l'impronta.
    fn file(cartella: &Path, nome: &str, quanti: usize) -> PathBuf {
        let percorso = cartella.join(nome);
        let byte: Vec<u8> = (0..quanti)
            .map(|i| u8::try_from((i * 7 + (i >> 8)) % 256).expect("un byte"))
            .collect();
        std::fs::write(&percorso, byte).expect("scritto");
        percorso
    }

    /// Cambia un byte al posto `dove`.
    fn tocca(percorso: &Path, dove: usize) {
        let mut byte = std::fs::read(percorso).expect("letto");
        let b = byte.get_mut(dove).expect("il posto c'è");
        *b = b.wrapping_add(1);
        std::fs::write(percorso, byte).expect("riscritto");
    }

    #[test]
    fn l_impronta_segue_la_formula_scritta() {
        // La formula è un contratto fra due PC: la si rifà a mano qui, così
        // cambiarla senza accorgersene fa cadere questa prova.
        let cartella = tempfile::tempdir().expect("cartella");
        for quanti in [0, 1_000, 64 * 1024, 100_000, 300_000] {
            let percorso = file(cartella.path(), &format!("f{quanti}.flac"), quanti);
            let tutto = std::fs::read(&percorso).expect("letto");
            let parte = quanti.min(64 * 1024);
            let mut atteso = blake3::Hasher::new();
            atteso.update(&u64::try_from(quanti).expect("u64").to_le_bytes());
            atteso.update(tutto.get(..parte).expect("testa"));
            atteso.update(tutto.get(quanti - parte..).expect("coda"));

            let d = descrittore(&percorso, None).expect("descrittore");
            assert_eq!(d.dimensione, u64::try_from(quanti).expect("u64"));
            assert_eq!(
                d.impronta_rapida,
                atteso.finalize().to_hex().to_string(),
                "{quanti} byte"
            );
            assert_eq!(d.impronta_rapida.len(), 64);
            assert_eq!(d.unc, None, "un percorso locale non viaggia");
        }
    }

    #[test]
    fn la_data_e_quella_della_scansione() {
        let cartella = tempfile::tempdir().expect("cartella");
        let percorso = file(cartella.path(), "a.mp3", 10);
        filetime::set_file_mtime(
            &percorso,
            filetime::FileTime::from_unix_time(1_700_000_000, 123_456_789),
        )
        .expect("data");
        let d = descrittore(&percorso, None).expect("descrittore");
        assert_eq!(
            d.modificato_ms, 1_700_000_000_123,
            "millisecondi troncati, come `tracks.date_modified`"
        );
    }

    #[test]
    fn la_chiave_di_contenuto_viaggia_solo_se_ci_sta() {
        let cartella = tempfile::tempdir().expect("cartella");
        let percorso = file(cartella.path(), "a.mp3", 10);
        let lunga = "k".repeat(MASSIMO_CHIAVE + 1);
        for (chiave, attesa) in [
            (Some("artista|titolo|album"), Some("artista|titolo|album")),
            (Some(""), None),
            (Some(lunga.as_str()), None),
            (Some("a\nb"), None),
            (None, None),
        ] {
            assert_eq!(
                descrittore(&percorso, chiave)
                    .expect("descrittore")
                    .chiave_contenuto
                    .as_deref(),
                attesa
            );
        }
    }

    #[test]
    fn solo_una_condivisione_diventa_un_percorso_che_viaggia() {
        assert_eq!(
            unc_di(r"\\nas\musica\a.flac").as_deref(),
            Some(r"\\nas\musica\a.flac")
        );
        assert_eq!(
            unc_di("//nas/musica/a.flac").as_deref(),
            Some(r"\\nas\musica\a.flac"),
            "il trasporto pretende le barre rovesciate in testa"
        );
        for verbatim in [r"\\?\UNC\nas\musica\a.flac", "//?/UNC/nas/musica/a.flac"] {
            assert_eq!(
                unc_di(verbatim).as_deref(),
                Some(r"\\nas\musica\a.flac"),
                "{verbatim}"
            );
        }
        for locale in [
            r"C:\Musica\a.flac",
            r"\\?\C:\Musica\a.flac",
            r"\\.\PhysicalDrive0",
        ] {
            assert_eq!(unc_di(locale), None, "{locale}");
        }
        let troppo = format!(r"\\nas\{}", "a".repeat(MASSIMO_UNC));
        assert_eq!(unc_di(&troppo), None);
    }

    #[test]
    fn una_copia_identica_si_riconosce_e_una_toccata_no() {
        let cartella = tempfile::tempdir().expect("cartella");
        for quanti in [5_000, 300_000] {
            let originale = file(cartella.path(), &format!("o{quanti}.flac"), quanti);
            let d = descrittore(&originale, None).expect("descrittore");

            let copia = cartella.path().join(format!("copia{quanti}.flac"));
            std::fs::copy(&originale, &copia).expect("copia");
            // Un'altra data non conta: è la data della copia, non della musica.
            filetime::set_file_mtime(&copia, filetime::FileTime::from_unix_time(1, 0))
                .expect("data");
            assert!(e_la_stessa(&copia, &d), "{quanti}: la copia è la stessa");

            // In testa, in coda, e una lunghezza diversa: tre file diversi.
            for dove in [0, quanti - 1] {
                let toccata = cartella.path().join(format!("t{quanti}-{dove}.flac"));
                std::fs::copy(&originale, &toccata).expect("copia");
                tocca(&toccata, dove);
                assert!(!e_la_stessa(&toccata, &d), "{quanti}: byte {dove} cambiato");
            }
            let corta = file(cartella.path(), &format!("c{quanti}.flac"), quanti - 1);
            assert!(!e_la_stessa(&corta, &d));
        }
        let sparita = cartella.path().join("non-c-e.flac");
        let d = Descrittore {
            unc: None,
            dimensione: 0,
            modificato_ms: 0,
            impronta_rapida: String::new(),
            chiave_contenuto: None,
        };
        assert!(
            !e_la_stessa(&sparita, &d),
            "un file che non si apre non è una copia"
        );
    }

    // ── fase 3: i candidati ──

    /// Una riga di `tracks` con quel che serve ai candidati.
    #[expect(
        clippy::too_many_arguments,
        reason = "una riga di prova, colonna per colonna"
    )]
    fn riga(
        c: &Connection,
        id: i64,
        path: Option<&str>,
        immagine: Option<&str>,
        dimensione: i64,
        data: i64,
        durata: i64,
        chiave: Option<&str>,
    ) {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                 file_size, date_added, date_modified, immagine, content_key)
             VALUES (?1, ?2, 'k' || ?1, 'T', 'A', 'B', ?3, ?4, 0, ?5, ?6, ?7)",
            rusqlite::params![id, path, durata, dimensione, data, immagine, chiave],
        )
        .expect("brano");
    }

    fn descritto(unc: Option<&str>, chiave: Option<&str>) -> Descrittore {
        Descrittore {
            unc: unc.map(str::to_owned),
            dimensione: 5_000_000,
            modificato_ms: 1_000,
            impronta_rapida: "0".repeat(64),
            chiave_contenuto: chiave.map(str::to_owned),
        }
    }

    fn quali(candidati: &[Candidato]) -> Vec<(i64, bool)> {
        candidati.iter().map(|c| (c.track_id, c.certa)).collect()
    }

    #[test]
    fn lo_stesso_file_sulla_condivisione_e_una_copia_certa() {
        let c = libreria();
        riga(
            &c,
            1,
            Some(r"\\nas\m\a.flac"),
            None,
            5_000_000,
            1_000,
            200_000,
            None,
        );
        // Stesso percorso ma un'altra data: il file è cambiato dopo la nostra
        // scansione, e non è certo niente.
        riga(
            &c,
            2,
            Some(r"\\nas\m\b.flac"),
            None,
            5_000_000,
            999,
            200_000,
            None,
        );

        let certi = candidati_copia(&c, &descritto(Some(r"\\nas\m\a.flac"), None), 200_000)
            .expect("candidati");
        assert_eq!(quali(&certi), [(1, true)]);
        assert_eq!(
            certi.first().map(|c| c.percorso.clone()),
            Some(PathBuf::from(r"\\nas\m\a.flac"))
        );
        assert!(
            candidati_copia(&c, &descritto(Some(r"\\nas\m\b.flac"), None), 200_000)
                .expect("candidati")
                .is_empty()
        );
    }

    #[test]
    fn sulla_condivisione_le_maiuscole_non_contano_dove_non_contano() {
        let c = libreria();
        riga(
            &c,
            1,
            Some(r"\\NAS\Musica\a.flac"),
            None,
            5_000_000,
            1_000,
            1,
            None,
        );
        let trovati = candidati_copia(&c, &descritto(Some(r"\\nas\musica\A.FLAC"), None), 1)
            .expect("candidati");
        if PathRules::for_current_platform().case_insensitive {
            assert_eq!(quali(&trovati), [(1, true)]);
        } else {
            assert!(trovati.is_empty());
        }
    }

    #[test]
    fn per_chiave_di_contenuto_si_trova_una_copia_da_verificare() {
        let c = libreria();
        // Quella buona, una più lontana nella durata ma dentro il mezzo
        // secondo, e due che non passano: durata e dimensione. Un brano di
        // catalogo non si può nemmeno scrivere qui: la 026 gli vuole
        // `file_size` a `NULL`, quindi non ha una dimensione da far coincidere.
        riga(
            &c,
            1,
            Some(r"D:\M\a.flac"),
            None,
            5_000_000,
            7,
            200_100,
            Some("ck"),
        );
        riga(
            &c,
            2,
            Some(r"E:\Copie\a.flac"),
            None,
            5_000_000,
            7,
            200_400,
            Some("ck"),
        );
        riga(
            &c,
            3,
            Some(r"D:\M\lungo.flac"),
            None,
            5_000_000,
            7,
            200_600,
            Some("ck"),
        );
        riga(
            &c,
            4,
            Some(r"D:\M\altro.flac"),
            None,
            4_999_999,
            7,
            200_000,
            Some("ck"),
        );

        let trovati =
            candidati_copia(&c, &descritto(None, Some("ck")), 200_000).expect("candidati");
        assert_eq!(
            quali(&trovati),
            [(1, false), (2, false)],
            "la più vicina nella durata prima, e nessuna è certa"
        );
    }

    #[test]
    fn un_brano_d_immagine_e_solo_certo_e_la_durata_sceglie_quale() {
        let c = libreria();
        let immagine = r"\\nas\m\Disco.flac";
        // Due brani della stessa immagine: la data della riga è quella del
        // foglio, più nuova di quella dell'immagine.
        riga(
            &c,
            1,
            Some(r"\\nas\m\Disco.cue|01"),
            Some(immagine),
            5_000_000,
            2_000,
            180_000,
            Some("ck1"),
        );
        riga(
            &c,
            2,
            Some(r"\\nas\m\Disco.cue|02"),
            Some(immagine),
            5_000_000,
            2_000,
            240_000,
            Some("ck2"),
        );

        let trovati = candidati_copia(&c, &descritto(Some(immagine), Some("ck2")), 240_200)
            .expect("candidati");
        assert_eq!(
            quali(&trovati),
            [(2, true)],
            "il brano con la durata giusta, e nessun passo per chiave"
        );
        assert_eq!(
            trovati.first().map(|c| c.percorso.clone()),
            Some(PathBuf::from(immagine)),
            "si apre l'immagine, non il percorso virtuale"
        );

        // Un'immagine cambiata dopo la nostra scansione ha una data più nuova
        // di quella della riga: niente di certo.
        let mut cambiata = descritto(Some(immagine), Some("ck2"));
        cambiata.modificato_ms = 3_000;
        assert!(
            candidati_copia(&c, &cambiata, 240_000)
                .expect("candidati")
                .is_empty()
        );
    }

    #[test]
    fn una_riga_certa_non_torna_da_verificare_e_le_altre_hanno_un_tetto() {
        let c = libreria();
        riga(
            &c,
            1,
            Some(r"\\nas\m\a.flac"),
            None,
            5_000_000,
            1_000,
            200_000,
            Some("ck"),
        );
        for id in 2..=9 {
            riga(
                &c,
                id,
                Some(&format!(r"D:\M\{id}.flac")),
                None,
                5_000_000,
                7,
                200_000,
                Some("ck"),
            );
        }
        let trovati = candidati_copia(&c, &descritto(Some(r"\\nas\m\a.flac"), Some("ck")), 200_000)
            .expect("candidati");
        assert_eq!(
            trovati.first().map(|c| (c.track_id, c.certa)),
            Some((1, true))
        );
        assert_eq!(
            trovati.iter().filter(|c| !c.certa).count(),
            MASSIMO_DA_VERIFICARE
        );
        assert!(trovati.iter().filter(|c| c.track_id == 1).count() == 1);
    }

    #[test]
    fn numeri_fuori_scala_non_sono_un_guasto() {
        let c = libreria();
        let mut d = descritto(Some(r"\\nas\m\a.flac"), Some("ck"));
        d.dimensione = u64::MAX;
        assert!(candidati_copia(&c, &d, 1).expect("candidati").is_empty());
        assert!(
            candidati_copia(&c, &descritto(None, Some("ck")), u64::MAX)
                .expect("candidati")
                .is_empty()
        );
    }
}
