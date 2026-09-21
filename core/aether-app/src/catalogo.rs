//! I brani di catalogo che stanno in libreria senza essere file.
//!
//! È il posto che mancava. Il lettore sa suonare byte che arrivano dalla rete
//! dalla 2.2.0 — `aether_play::Sorgente` prende un `Flusso`, non un percorso —
//! ma la libreria non aveva dove annotare un brano che un file non ce l'ha, e
//! quel ramo restava raggiungibile nel codice e irraggiungibile da chi usa il
//! programma. La migrazione 26 ha aperto il posto; questo modulo ci scrive
//! dentro.
//!
//! # Il cancello, e dove sta
//!
//! Qui **non** si decide cosa si può fare di un brano: lo ha già deciso il
//! catalogo che lo ha dato, e viaggia dentro il [`Candidato`] come
//! [`Disponibilita`]. Questo modulo ne guarda una cosa sola — che dei byte da
//! ascoltare ci siano — e rifiuta [`Disponibilita::SoloAcquisto`], che è la
//! risposta «nessuna fonte lecita lo dà». Aggiungere in libreria un brano che
//! nessuno consegna vorrebbe dire una riga che non suonerà mai.
//!
//! Il permesso di **tenere una copia sul disco** è un'altra domanda, e la
//! risponde `aether_catalogo::prelievo` prima di ogni richiesta. Qui non si
//! scrive nessun byte da nessuna parte: si scrive un indirizzo.
//!
//! # Perché non passa da `desiderati`
//!
//! Perché `desiderati` è la coda di quel che si vorrebbe **avere**: ogni riga
//! aspetta di diventare un file, e chi la lavora scarica. Un brano di solo
//! ascolto non diventerà mai un file, e metterlo in quella coda vorrebbe dire
//! un tentativo di prelievo respinto a ogni passata, per sempre.

use aether_domain::album::album_group_key;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::Disponibilita;
use aether_domain::keys::{TrackKey, TrackKeyInput};
use aether_domain::scelta::Candidato;
use rusqlite::Connection;

use crate::library::{db_error, now_ms, rebuild_aggregates};

/// Quel che l'aggiunta di un pugno di brani ha prodotto.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Aggiunti {
    /// Quante righe nuove.
    pub aggiunti: usize,
    /// Quanti c'erano già.
    ///
    /// Non è un errore e non è un dettaglio: è la differenza fra «fatto» e «ce
    /// l'avevi già», e sono due cose da dire in due modi diversi a chi ha
    /// appena premuto un tasto.
    pub gia_presenti: usize,
    /// Quanti sono stati rifiutati perché nessuna fonte lecita li consegna.
    ///
    /// È un **no del catalogo**, e si dice a chi ascolta con la frase che
    /// nomina il negozio ([`nessuno_lo_consegna`]).
    pub senza_byte: usize,
    /// Quanti sono stati scartati perché la riga del catalogo era inservibile:
    /// titolo vuoto, o nessun indirizzo.
    ///
    /// Sta accanto a [`Self::senza_byte`] e non dentro, e la distinzione non è
    /// accademica: per un pezzo di tempo erano lo stesso contatore, e a chi
    /// aggiungeva un brano con il titolo vuoto Aether rispondeva «nessuna
    /// fonte lecita consegna questo brano: resta il negozio» — una frase
    /// perfettamente formata su una causa che non c'entrava niente.
    pub scartati: usize,
}

/// Il titolo con cui si elenca un brano che non dichiara l'album.
///
/// Su Audius quasi tutto è un singolo e l'album non c'è. Lasciare vuota la
/// colonna vorrebbe dire una scheda d'album senza nome nella griglia; scriverci
/// il nome del catalogo sarebbe una bugia in una colonna che si sincronizza fra
/// dispositivi. Si usa il titolo del brano, che è quel che i lettori fanno da
/// sempre con i singoli.
fn album_di(candidato: &Candidato) -> String {
    candidato
        .album
        .clone()
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| candidato.titolo.clone())
}

/// L'interprete, o quel che si scrive quando il catalogo non lo dice.
fn artista_di(candidato: &Candidato) -> String {
    candidato
        .autore
        .clone()
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| aether_domain::album::UNKNOWN_ARTIST.to_owned())
}

/// Mette in libreria dei brani di catalogo, come riferimenti e non come file.
///
/// Salta in silenzio quelli che ci sono già: l'indice unico su
/// `(source_service, fonte_url)` lo rende una proprietà del database e non una
/// query di controllo fatta prima, che fra il controllo e la scrittura
/// lascerebbe una finestra aperta.
///
/// # Perché gli aggregati si rifanno qui
///
/// Perché `albums` e `artists` non si ricostruiscono da soli: li rifà la
/// scansione, e una scansione questi brani non li incontrerà mai. Senza questa
/// riga un brano aggiunto si troverebbe cercandolo — l'indice di ricerca lo
/// segue con un trigger — ma non comparirebbe in nessuna griglia, che è il modo
/// più confondente di essere presenti.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce. Un candidato che nessuno consegna
/// non è un errore: si conta in [`Aggiunti::senza_byte`], perché rifiutare
/// l'intero gesto per una riga su dieci sarebbe una risposta peggiore.
pub fn aggiungi(
    connection: &mut Connection,
    candidati: &[Candidato],
) -> Result<Aggiunti, AppError> {
    if candidati.is_empty() {
        return Ok(Aggiunti::default());
    }
    let adesso = now_ms();
    let mut esito = Aggiunti::default();

    let tx = connection
        .transaction()
        .map_err(|err| db_error("aggiunta di brani di catalogo", &err))?;
    {
        let mut statement = tx
            .prepare(
                "INSERT OR IGNORE INTO tracks (
                     path, track_key, title, artist, album, album_artist, album_key,
                     duration_ms, date_added, source,
                     source_service, fonte_url, fonte_pagina, licenza, disponibilita)
                 VALUES (NULL, ?1, ?2, ?3, ?4, ?3, ?5, ?6, ?7, 'catalogo',
                         ?8, ?9, ?10, ?11, ?12)",
            )
            .map_err(|err| db_error("aggiunta di brani di catalogo", &err))?;

        for candidato in candidati {
            if candidato.disponibilita == Disponibilita::SoloAcquisto {
                esito.senza_byte = esito.senza_byte.saturating_add(1);
                continue;
            }
            let titolo = candidato.titolo.trim();
            if titolo.is_empty() || candidato.url.trim().is_empty() {
                esito.scartati = esito.scartati.saturating_add(1);
                continue;
            }

            let artista = artista_di(candidato);
            let album = album_di(candidato);
            let track_key = TrackKey::compute(TrackKeyInput {
                artist: Some(&artista),
                title: Some(titolo),
                album: Some(&album),
            })
            .into_string();
            // La «cartella» con cui si raggruppano le edizioni è l'indirizzo:
            // per l'Internet Archive il pezzo che sta prima del file è l'item,
            // cioè il concerto, che è esattamente il raggruppamento giusto. Per
            // Audius è l'identificativo del brano, e un singolo per conto suo è
            // la verità.
            let album_key = album_group_key(&album, &candidato.url);
            let durata_ms = i64::from(candidato.durata_sec.unwrap_or(0)).saturating_mul(1000);

            let scritte = statement
                .execute(rusqlite::params![
                    track_key,
                    titolo,
                    artista,
                    album,
                    album_key,
                    durata_ms,
                    adesso,
                    candidato.fonte.nome(),
                    candidato.url,
                    candidato.pagina,
                    candidato.licenza.nome(),
                    candidato.disponibilita.nome(),
                ])
                .map_err(|err| db_error("aggiunta di un brano di catalogo", &err))?;

            if scritte == 0 {
                esito.gia_presenti = esito.gia_presenti.saturating_add(1);
            } else {
                esito.aggiunti = esito.aggiunti.saturating_add(1);
            }
        }
    }

    if esito.aggiunti > 0 {
        rebuild_aggregates(&tx)?;
    }
    tx.commit()
        .map_err(|err| db_error("aggiunta di brani di catalogo", &err))?;
    Ok(esito)
}

/// Toglie dalla libreria un brano di catalogo.
///
/// Esiste accanto a [`crate::library::rimuovi_dalla_libreria`] e non al suo
/// posto: quella è il gesto generale e tratta già i riferimenti insieme ai file.
/// Questa serve al caso in cui si abbia in mano l'indirizzo e non
/// l'identificativo — il tasto «togli» accanto a un risultato di ricerca, che
/// mostra quel che sta nel catalogo e non quel che sta in tabella.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn togli(connection: &mut Connection, fonte: &str, fonte_url: &str) -> Result<bool, AppError> {
    let tx = connection
        .transaction()
        .map_err(|err| db_error("rimozione di un brano di catalogo", &err))?;
    let tolte = tx
        .execute(
            "DELETE FROM tracks WHERE source_service = ?1 AND fonte_url = ?2",
            rusqlite::params![fonte, fonte_url],
        )
        .map_err(|err| db_error("rimozione di un brano di catalogo", &err))?;
    if tolte > 0 {
        rebuild_aggregates(&tx)?;
    }
    tx.commit()
        .map_err(|err| db_error("rimozione di un brano di catalogo", &err))?;
    Ok(tolte > 0)
}

/// Quali di questi riferimenti stanno già in libreria.
///
/// Serve alla schermata di ricerca, che deve mostrare «aggiungi» o «c'è già»
/// accanto a ogni risultato senza fare una query per riga. Prende coppie
/// `(fonte, indirizzo)` e restituisce gli indirizzi trovati.
///
/// # Perché la coppia e non il solo indirizzo
///
/// Perché la coppia è l'identità: è su `(source_service, fonte_url)` che sta
/// l'indice unico che la migrazione 26 ha creato, ed è la coppia che guardano
/// [`togli`] e [`id_di`]. Con il solo indirizzo questa funzione rispondeva a
/// una domanda leggermente diversa dalle altre tre — e tre funzioni che
/// chiedono «lo stesso brano?» in due modi diversi sono il modo normale di
/// scoprire un giorno che una dice sì e l'altra no.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn gia_in_libreria(
    connection: &Connection,
    riferimenti: &[(String, String)],
) -> Result<std::collections::HashSet<String>, AppError> {
    if riferimenti.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    // `(a, b) IN (VALUES (?,?), (?,?))`: i valori di riga sono SQL standard e
    // SQLite li conosce dalla 3.15. L'alternativa — un `OR` per coppia — è la
    // stessa query scritta più lunga.
    let segnaposto = std::iter::repeat_n("(?,?)", riferimenti.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT fonte_url FROM tracks
         WHERE (source_service, fonte_url) IN (VALUES {segnaposto})"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| db_error("brani di catalogo già in libreria", &err))?;
    let piatti: Vec<&String> = riferimenti
        .iter()
        .flat_map(|(fonte, url)| [fonte, url])
        .collect();
    let righe = statement
        .query_map(rusqlite::params_from_iter(piatti), |riga| riga.get(0))
        .map_err(|err| db_error("brani di catalogo già in libreria", &err))?;
    righe
        .collect::<Result<std::collections::HashSet<String>, _>>()
        .map_err(|err| db_error("brani di catalogo già in libreria", &err))
}

/// Il brano di catalogo con questo indirizzo, se sta in libreria.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn id_di(
    connection: &Connection,
    fonte: &str,
    fonte_url: &str,
) -> Result<Option<i64>, AppError> {
    connection
        .query_row(
            "SELECT id FROM tracks WHERE source_service = ?1 AND fonte_url = ?2",
            rusqlite::params![fonte, fonte_url],
            |riga| riga.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("brano di catalogo per indirizzo", &altro)),
        })
}

/// Il codice d'errore per un candidato che nessuno consegna.
///
/// Non si usa da [`aggiungi`], che li conta invece di fallire; serve a chi
/// aggiunge **un** brano solo e deve dire perché non si è potuto.
#[must_use]
pub fn nessuno_lo_consegna() -> AppError {
    AppError::new(ErrorCode::DownloadNotPermitted { licenza: None })
        .with_cause("nessuna fonte lecita consegna questo brano: resta il negozio".to_owned())
}

/// Il no della licenza a «tieni una copia».
///
/// Il terzo della famiglia, e come gli altri due dice una cosa sola e precisa.
/// [`nessuno_lo_consegna`] è «nessuno te lo dà, resta il negozio»; questo è
/// «te lo do da ascoltare e non da tenere», che è il caso normale del Live
/// Music Archive — il patto dei tapers copre lo scambio, non la copia che
/// chiunque si porta via.
#[must_use]
pub fn non_si_tiene() -> AppError {
    AppError::new(ErrorCode::DownloadNotPermitted { licenza: None })
        .with_cause("la licenza di questo brano permette di ascoltarlo, non di tenerlo".to_owned())
}

/// Il codice d'errore per una riga di catalogo inservibile.
///
/// Il gemello di [`nessuno_lo_consegna`], e la ragione per cui sono due: un
/// titolo vuoto o un indirizzo mancante non sono un no del catalogo, sono un
/// dato rotto, e rispondere «resta il negozio» a chi ha incontrato il secondo
/// manda a cercare in un posto dove non c'è niente da trovare.
#[must_use]
pub fn riga_illeggibile() -> AppError {
    AppError::new(ErrorCode::CatalogoResolveFailed)
        .with_cause("questo risultato non ha un titolo o un indirizzo utilizzabile".to_owned())
}

/// Mette nella lista della spesa un brano che nessun catalogo libero consegna.
///
/// # Perché esiste, dopo che il modulo dichiara di non passare da `desiderati`
///
/// Perché la carta in testa a questo file parla dei brani di **solo ascolto**,
/// che in quella coda non devono finire: diventerebbero un tentativo di
/// prelievo respinto a ogni passata. Un brano di solo acquisto è l'altro caso,
/// e la riga giusta per lui in `desiderati` esiste da sempre —
/// [`crate::desiderati::Stato::Introvabile`], quella che nessuno riprova e che
/// `da_comprare` raccoglie. Senza questa funzione «quel che nessun catalogo
/// libero consegna finisce nella lista della spesa» valeva per un brano
/// incontrato importando una playlist e non per lo stesso brano incontrato
/// cercandolo, il che è una differenza che nessuno saprebbe spiegare.
///
/// # La provenienza
///
/// `source_kind` e `source_id` sono `esplora`, e `source_title` è **la frase
/// cercata**: nella lista della spesa la colonna «da dove viene» dirà «cercato:
/// grateful dead 1977» invece del nome di una playlist, che è l'informazione
/// vera e quella utile.
///
/// Restituisce quante righe sono state scritte davvero: un brano già in lista
/// non si duplica, ed è l'indice unico `(track_key, source_id)` a dirlo.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn nella_lista_della_spesa(
    connection: &mut Connection,
    candidati: &[Candidato],
    frase: &str,
) -> Result<usize, AppError> {
    fra_i_desiderati(
        connection,
        candidati,
        frase,
        crate::desiderati::Stato::Introvabile,
        Some(MOTIVO_SOLO_NEGOZIO),
        |c| c.disponibilita == Disponibilita::SoloAcquisto,
    )
}

/// Mette in coda di prelievo un brano che la licenza permette di tenere.
///
/// # La metà che mancava a «Si può tenere»
///
/// La pastiglia lo diceva e nessun tasto lo faceva. [`aggiungi`] scrive un
/// **riferimento**: il brano entra in libreria e suona arrivando dalla rete,
/// che è la risposta giusta per quel che si può solo ascoltare e la risposta
/// sbagliata per quel che si potrebbe tenere. Una schermata che promette
/// «ascoltala, e tienila quando si può» offriva un gesto solo su due.
///
/// # Perché qui `desiderati` va bene, e nella carta in testa al file no
///
/// La carta esclude da quella coda i brani di **solo ascolto**: là ogni riga
/// aspetta di diventare un file, e una che non può diventarlo sarebbe un
/// prelievo respinto a ogni passata, per sempre. Un brano scaricabile è
/// esattamente il contrario — è quel che quella coda è fatta per prendere — e
/// il prelievo sa già cosa farne senza una riga di codice nuova: `procura::trova`
/// vede un `fonte_url` con `disponibilita` scaricabile e **salta la ricerca**,
/// portando giù quel file lì. Non un file simile scelto da un algoritmo: quello
/// che chi cercava aveva davanti.
///
/// # Quel che questa funzione **non** fa
///
/// Non avvia niente. La passata di prelievo si accende da Importazioni, come
/// per ogni altra riga desiderata, e la ragione è che quella passata prende
/// **tutte** le righe in attesa: un brano tenuto da qui che facesse partire
/// trecento scaricamenti rimasti in sospeso da un'importazione di un mese fa
/// sarebbe una sorpresa, e le sorprese in uscita sulla rete non si fanno.
///
/// Restituisce quante righe sono state scritte davvero: un brano già in coda
/// non si duplica.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn da_tenere(
    connection: &mut Connection,
    candidati: &[Candidato],
    frase: &str,
) -> Result<usize, AppError> {
    fra_i_desiderati(
        connection,
        candidati,
        frase,
        crate::desiderati::Stato::Attesa,
        None,
        |c| c.disponibilita == Disponibilita::Scaricabile,
    )
}

/// Scrive delle righe in `desiderati` a partire da dei candidati di catalogo.
///
/// Il corpo comune di [`nella_lista_della_spesa`] e [`da_tenere`], che
/// differiscono in tre cose: quali candidati accettano, con che stato li
/// scrivono e con che motivo. Tutto il resto — la chiave, la provenienza, il
/// non duplicare — è lo stesso, e lo era anche quando era scritto due volte.
///
/// `ammesso` non è una comodità: è il cancello. Senza, «tieni una copia» su un
/// brano di solo ascolto scriverebbe una riga che il prelievo riproverebbe a
/// ogni passata senza poterla mai chiudere.
fn fra_i_desiderati(
    connection: &mut Connection,
    candidati: &[Candidato],
    frase: &str,
    stato: crate::desiderati::Stato,
    motivo: Option<&str>,
    ammesso: impl Fn(&Candidato) -> bool,
) -> Result<usize, AppError> {
    if candidati.is_empty() {
        return Ok(0);
    }
    let adesso = now_ms();
    let provenienza = format!("cercato: {}", frase.trim());
    let mut scritte = 0_usize;

    let tx = connection
        .transaction()
        .map_err(|err| db_error("lista della spesa", &err))?;
    {
        let mut statement = tx
            .prepare(
                "INSERT OR IGNORE INTO desiderati (
                     track_key, title, artist, album, duration_ms,
                     source_kind, source_id, source_title, source_service,
                     fonte_url, licenza, disponibilita,
                     download_state, download_error, added_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5,
                         'esplora', 'esplora', ?6, ?7,
                         ?8, ?9, ?10,
                         ?11, ?12, ?13, ?13)",
            )
            .map_err(|err| db_error("lista della spesa", &err))?;

        for candidato in candidati {
            let titolo = candidato.titolo.trim();
            if titolo.is_empty() || !ammesso(candidato) {
                continue;
            }
            let artista = artista_di(candidato);
            let album = album_di(candidato);
            let track_key = TrackKey::compute(TrackKeyInput {
                artist: Some(&artista),
                title: Some(titolo),
                album: Some(&album),
            })
            .into_string();
            let durata_ms = i64::from(candidato.durata_sec.unwrap_or(0)).saturating_mul(1000);

            scritte = scritte.saturating_add(
                statement
                    .execute(rusqlite::params![
                        track_key,
                        titolo,
                        artista,
                        album,
                        durata_ms,
                        provenienza,
                        candidato.fonte.nome(),
                        candidato.url,
                        candidato.licenza.nome(),
                        candidato.disponibilita.nome(),
                        stato.come_testo(),
                        motivo,
                        adesso,
                    ])
                    .map_err(|err| db_error("lista della spesa", &err))?,
            );
        }
    }
    tx.commit()
        .map_err(|err| db_error("lista della spesa", &err))?;
    Ok(scritte)
}

/// Perché quel brano sta nella lista della spesa e non in libreria.
///
/// Finisce in `download_error`, che è la colonna che `da_comprare` mostra come
/// «motivo»: non è un guasto, è la risposta.
const MOTIVO_SOLO_NEGOZIO: &str = "nessun catalogo libero lo distribuisce";

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::esterno::{Fonte, Licenza};

    fn candidato(url: &str, titolo: &str, disponibilita: Disponibilita) -> Candidato {
        Candidato {
            url: url.to_owned(),
            titolo: titolo.to_owned(),
            autore: Some("Grateful Dead".to_owned()),
            album: Some("Barton Hall 1977".to_owned()),
            durata_sec: Some(754),
            fonte: Fonte::InternetArchive,
            licenza: Licenza::LiberaNonCommerciale,
            disponibilita,
            pagina: Some("https://archive.org/details/gd1977-05-08".to_owned()),
            ..Candidato::default()
        }
    }

    fn libreria() -> Connection {
        crate::db::open_in_memory()
            .expect("apertura in memoria")
            .connection
    }

    #[test]
    fn un_brano_di_solo_ascolto_entra_in_libreria_senza_percorso() {
        let mut c = libreria();
        let esito = aggiungi(
            &mut c,
            &[candidato(
                "https://archive.org/download/gd1977-05-08/t01.mp3",
                "Scarlet Begonias",
                Disponibilita::SoloAscolto,
            )],
        )
        .expect("aggiunta");
        assert_eq!(esito.aggiunti, 1);

        let (path, servizio, url, licenza, disponibilita): (
            Option<String>,
            String,
            String,
            String,
            String,
        ) = c
            .query_row(
                "SELECT path, source_service, fonte_url, licenza, disponibilita FROM tracks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("riga");
        assert_eq!(path, None, "un flusso non ha un percorso");
        assert_eq!(servizio, "internet-archive");
        assert_eq!(url, "https://archive.org/download/gd1977-05-08/t01.mp3");
        assert_eq!(licenza, "liberaNonCommerciale");
        assert_eq!(disponibilita, "soloAscolto");
    }

    #[test]
    fn si_trova_cercandolo_e_compare_fra_gli_album() {
        let mut c = libreria();
        aggiungi(
            &mut c,
            &[candidato(
                "https://archive.org/download/gd1977-05-08/t01.mp3",
                "Scarlet Begonias",
                Disponibilita::SoloAscolto,
            )],
        )
        .expect("aggiunta");

        let trovati: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'Scarlet'",
                [],
                |r| r.get(0),
            )
            .expect("ricerca");
        assert_eq!(trovati, 1, "un brano che esiste e non si trova non c'è");

        // La griglia degli album legge `albums`, che nessuna scansione
        // ricostruirà mai per questi brani.
        let album: i64 = c
            .query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))
            .expect("album");
        assert_eq!(album, 1, "senza aggregati il brano non comparirebbe");
    }

    #[test]
    fn lo_stesso_brano_due_volte_non_si_duplica() {
        let mut c = libreria();
        let uno = candidato(
            "https://archive.org/download/gd1977-05-08/t01.mp3",
            "Scarlet Begonias",
            Disponibilita::SoloAscolto,
        );
        assert_eq!(
            aggiungi(&mut c, std::slice::from_ref(&uno))
                .expect("prima")
                .aggiunti,
            1
        );
        let seconda = aggiungi(&mut c, &[uno]).expect("seconda");
        assert_eq!(seconda.aggiunti, 0);
        assert_eq!(
            seconda.gia_presenti, 1,
            "«ce l'avevi già» non è «fatto» e non è un errore"
        );
    }

    #[test]
    fn quel_che_nessuno_consegna_non_entra() {
        let mut c = libreria();
        let esito = aggiungi(
            &mut c,
            &[candidato(
                "https://archive.org/download/x/y.mp3",
                "Solo in negozio",
                Disponibilita::SoloAcquisto,
            )],
        )
        .expect("aggiunta");
        assert_eq!(esito.aggiunti, 0);
        assert_eq!(esito.senza_byte, 1);
        let righe: i64 = c
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("conteggio");
        assert_eq!(righe, 0, "una riga che non suonerà mai non si scrive");
    }

    #[test]
    fn un_brano_senza_album_si_elenca_sotto_il_proprio_titolo() {
        let mut c = libreria();
        let mut solo = candidato("/v1/tracks/aB3/stream", "Notte", Disponibilita::SoloAscolto);
        solo.album = None;
        solo.fonte = Fonte::Audius;
        solo.licenza = Licenza::OpenMusicLicense;
        aggiungi(&mut c, &[solo]).expect("aggiunta");
        let album: String = c
            .query_row("SELECT album FROM tracks", [], |r| r.get(0))
            .expect("album");
        assert_eq!(album, "Notte");
    }

    #[test]
    fn si_toglie_per_indirizzo_e_si_ritrova_per_identificativo() {
        let mut c = libreria();
        let uno = candidato(
            "https://archive.org/download/gd1977-05-08/t01.mp3",
            "Scarlet Begonias",
            Disponibilita::SoloAscolto,
        );
        aggiungi(&mut c, std::slice::from_ref(&uno)).expect("aggiunta");

        assert!(
            id_di(&c, "internet-archive", &uno.url)
                .expect("lettura")
                .is_some()
        );
        let riferimento = ("internet-archive".to_owned(), uno.url.clone());
        assert_eq!(
            gia_in_libreria(&c, std::slice::from_ref(&riferimento))
                .expect("elenco")
                .len(),
            1
        );
        // La chiave è la coppia: lo stesso indirizzo sotto un'altra fonte è un
        // altro brano, ed è quel che dice l'indice unico della migrazione 26.
        let altra_fonte = ("audius".to_owned(), uno.url.clone());
        assert!(
            gia_in_libreria(&c, std::slice::from_ref(&altra_fonte))
                .expect("elenco")
                .is_empty()
        );
        assert!(togli(&mut c, "internet-archive", &uno.url).expect("rimozione"));
        assert!(
            id_di(&c, "internet-archive", &uno.url)
                .expect("lettura")
                .is_none()
        );
        let album: i64 = c
            .query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))
            .expect("album");
        assert_eq!(album, 0, "l'album vuoto se ne va con l'ultimo brano");
    }

    #[test]
    fn una_riga_senza_titolo_si_scarta_e_non_si_chiama_negozio() {
        // I due contatori erano uno solo, e a chi incontrava una riga rotta
        // Aether rispondeva «resta il negozio»: una causa che non c'entrava.
        let mut c = libreria();
        let rotto = candidato(
            "https://archive.org/download/x/y.mp3",
            "   ",
            Disponibilita::SoloAscolto,
        );
        let solo_negozio = candidato(
            "https://archive.org/download/x/z.mp3",
            "Un disco che si compra",
            Disponibilita::SoloAcquisto,
        );

        let esito = aggiungi(&mut c, &[rotto, solo_negozio]).expect("aggiunta");
        assert_eq!(esito.aggiunti, 0);
        assert_eq!(esito.scartati, 1, "il titolo vuoto");
        assert_eq!(esito.senza_byte, 1, "il solo acquisto");
    }

    #[test]
    fn quel_che_nessuno_consegna_finisce_nella_lista_della_spesa() {
        let mut c = libreria();
        let solo_negozio = candidato(
            "https://archive.org/download/x/z.mp3",
            "Un disco che si compra",
            Disponibilita::SoloAcquisto,
        );

        let scritte = nella_lista_della_spesa(
            &mut c,
            std::slice::from_ref(&solo_negozio),
            "  un disco che si compra  ",
        )
        .expect("lista della spesa");
        assert_eq!(scritte, 1);

        // In libreria non è entrato: la lista della spesa è un'altra tabella,
        // ed è tutto il punto.
        let brani: i64 = c
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("brani");
        assert_eq!(brani, 0);

        // E nella lista c'è con la sua provenienza e il suo motivo.
        let (titolo, provenienza, stato, motivo): (String, String, String, String) = c
            .query_row(
                "SELECT title, source_title, download_state, download_error FROM desiderati",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("la riga");
        assert_eq!(titolo, "Un disco che si compra");
        assert_eq!(provenienza, "cercato: un disco che si compra");
        assert_eq!(stato, crate::desiderati::Stato::Introvabile.come_testo());
        assert_eq!(motivo, MOTIVO_SOLO_NEGOZIO);

        // Due volte non fanno due righe: `da_comprare` mostrerebbe lo stesso
        // brano due volte, e una lista della spesa che si ripete fa sembrare
        // il problema più grande di quel che è.
        let ancora = nella_lista_della_spesa(&mut c, std::slice::from_ref(&solo_negozio), "altro")
            .expect("lista della spesa");
        assert_eq!(ancora, 0);
    }

    #[test]
    fn dalla_lista_della_spesa_si_arriva_a_da_comprare() {
        // La prova che le due metà si toccano: quel che scrive Esplora è quel
        // che la lista della spesa mostra, senza nessun passaggio in mezzo.
        let mut c = libreria();
        let solo_negozio = candidato(
            "https://archive.org/download/x/z.mp3",
            "Un disco che si compra",
            Disponibilita::SoloAcquisto,
        );
        nella_lista_della_spesa(&mut c, std::slice::from_ref(&solo_negozio), "prova")
            .expect("lista della spesa");

        let da_comprare = crate::desiderati::da_comprare(&c, 10).expect("da comprare");
        assert_eq!(da_comprare.len(), 1);
        let primo = da_comprare.first().expect("la riga");
        assert_eq!(primo.titolo, "Un disco che si compra");
        assert_eq!(primo.provenienza, "cercato: prova");
        assert_eq!(primo.motivo, MOTIVO_SOLO_NEGOZIO);
    }
    /// «Tieni una copia» mette il brano in coda di prelievo, non in libreria.
    ///
    /// La riga deve essere in `attesa` e portare l'indirizzo esatto che chi
    /// cercava aveva davanti: è quello che fa saltare la ricerca a
    /// `procura::trova`, e senza il prelievo sceglierebbe da sé un file simile
    /// invece di quello.
    #[test]
    fn quel_che_si_puo_tenere_va_in_coda_col_suo_indirizzo() {
        let mut c = libreria();
        let tenibile = candidato(
            "https://archive.org/download/x/y.flac",
            "Un brano che si tiene",
            Disponibilita::Scaricabile,
        );

        let scritte = da_tenere(&mut c, std::slice::from_ref(&tenibile), "  netlabel  ")
            .expect("coda di prelievo");
        assert_eq!(scritte, 1);

        let (stato, motivo, url, provenienza): (String, Option<String>, String, String) = c
            .query_row(
                "SELECT download_state, download_error, fonte_url, source_title FROM desiderati",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("la riga");
        assert_eq!(stato, crate::desiderati::Stato::Attesa.come_testo());
        assert_eq!(motivo, None, "non è un guasto: è una cosa da fare");
        assert_eq!(url, "https://archive.org/download/x/y.flac");
        assert_eq!(provenienza, "cercato: netlabel");

        // E non in libreria: quella è `aggiungi`, ed è l'altro gesto.
        let brani: i64 = c
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .expect("brani");
        assert_eq!(brani, 0);
    }

    /// I due gesti non si scambiano i brani.
    ///
    /// Un brano di solo ascolto messo in coda di prelievo sarebbe un tentativo
    /// respinto a ogni passata, per sempre — la ragione per cui la carta in
    /// testa a questo file dice che da `desiderati` si sta alla larga. Un brano
    /// scaricabile nella lista della spesa sarebbe la bugia opposta: «vallo a
    /// comprare» per qualcosa che si prende gratis e legalmente.
    #[test]
    fn i_due_cancelli_non_si_aprono_a_vicenda() {
        let mut c = libreria();
        let solo_ascolto = candidato(
            "https://archive.org/download/x/a.mp3",
            "Un brano che si ascolta e basta",
            Disponibilita::SoloAscolto,
        );
        let tenibile = candidato(
            "https://archive.org/download/x/b.flac",
            "Un brano che si tiene",
            Disponibilita::Scaricabile,
        );

        assert_eq!(
            da_tenere(&mut c, std::slice::from_ref(&solo_ascolto), "prova").expect("prelievo"),
            0,
            "il solo ascolto non entra in coda di prelievo"
        );
        assert_eq!(
            nella_lista_della_spesa(&mut c, std::slice::from_ref(&tenibile), "prova")
                .expect("spesa"),
            0,
            "quel che si tiene non si manda a comprare"
        );

        let righe: i64 = c
            .query_row("SELECT COUNT(*) FROM desiderati", [], |r| r.get(0))
            .expect("desiderati");
        assert_eq!(righe, 0);
    }
}
