//! Cosa suonare quando la coda è finita.
//!
//! # Il problema che risolve
//!
//! A coda esaurita Aether si fermava. Non è un guasto — ha fatto quel che gli
//! era stato chiesto — ma è l'unico momento in cui l'applicazione smette di
//! funzionare da sola, e capita a ogni ascolto: si mette un album, l'album
//! finisce, e il silenzio è la risposta.
//!
//! # Perché non è un raccomandatore
//!
//! Perché non serve, e perché non si potrebbe. Non c'è nessun servizio da
//! interrogare — è precisamente il punto del progetto — e non c'è nessun
//! grafo dei gusti da costruire su una libreria di qualche migliaio di brani.
//! Quel che c'è è quel che serve: un artista, un album, un genere, un anno,
//! quante volte una cosa è stata suonata e quando è stata suonata l'ultima
//! volta. Sono le stesse colonne su cui lavorano le playlist intelligenti, e
//! infatti è il loro motore che risponde anche qui.
//!
//! # La cascata
//!
//! Le regole si provano in ordine e la prima che dà un risultato vince:
//!
//! 1. **Il resto dell'album** — se si stava ascoltando un disco, il disco
//!    continua. È la risposta giusta più spesso di qualunque affinità.
//! 2. **Lo stesso artista, un altro album** — chi ha finito un disco di
//!    qualcuno di solito ne vuole ancora.
//! 3. **Lo stesso genere, non di recente** — qui comincia la scoperta, e la
//!    condizione «non negli ultimi trenta giorni» è ciò che le impedisce di
//!    girare sempre sugli stessi dieci brani.
//! 4. **Un preferito mai ascoltato** — la contraddizione che ogni libreria
//!    grande contiene, e l'occasione di risolverla.
//! 5. **Uno qualunque** — perché fermarsi è la cosa che questo modulo esiste
//!    per non fare.
//!
//! # Perché non si escludono i brani già in coda con una regola
//!
//! Perché [`aether_domain::regole::Campo`] non ha una variante «identificativo»
//! e non deve averla: è il vocabolario che si vede nell'editor delle playlist
//! intelligenti, e «id non in (4, 17, 233)» non è una cosa che qualcuno
//! scriverebbe a mano. Si chiedono quindi più candidati del necessario e si
//! scartano qui quelli già visti.

use rusqlite::Connection;

use aether_domain::errors::AppError;
use aether_domain::regole::{Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore};

use crate::library::{self, TrackSummary};
use crate::smart;

/// Da quanti giorni un brano dev'essere fermo per contare come «non di recente».
///
/// Un mese: abbastanza perché una scaletta di un pomeriggio non si riproponga,
/// poco abbastanza perché una libreria di qualche centinaio di brani abbia
/// sempre qualcosa da offrire.
const GIORNI_RECENTI: i64 = 30;

/// Quanti candidati chiedere a ogni passo della cascata.
///
/// Più di uno perché i brani già in coda si scartano qui e non nella query: se
/// se ne chiedesse uno solo e fosse già stato ascoltato in questa sessione, il
/// passo fallirebbe e si scenderebbe a un criterio peggiore avendo ancora
/// ottime risposte a disposizione.
const CANDIDATI: u32 = 24;

/// Il brano da suonare dopo quello corrente, se ce n'è uno sensato.
///
/// `esclusi` sono gli identificativi che **non** vanno riproposti: quel che è
/// già in coda, cioè quel che si è appena sentito. `None` solo su una libreria
/// vuota, o quando ogni candidato è già stato scartato.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn prossimo(
    connection: &Connection,
    corrente: i64,
    esclusi: &[i64],
    adesso_ms: i64,
) -> Result<Option<i64>, AppError> {
    let brano = library::read_summary(connection, corrente)?;

    if let Some(brano) = brano.as_ref() {
        if let Some(id) = resto_dell_album(connection, brano, esclusi)? {
            return Ok(Some(id));
        }
        if let Some(id) = stesso_artista(connection, brano, esclusi, adesso_ms)? {
            return Ok(Some(id));
        }
        if let Some(id) = stesso_genere(connection, corrente, esclusi, adesso_ms)? {
            return Ok(Some(id));
        }
    }

    if let Some(id) = un_preferito_mai_sentito(connection, esclusi, adesso_ms)? {
        return Ok(Some(id));
    }
    uno_qualunque(connection, esclusi, adesso_ms)
}

/// Il brano dopo, nello stesso album.
///
/// Non passa dalle regole: l'ordine di un disco è quello dei numeri di traccia,
/// e `album_tracks` lo restituisce già così. Una regola `Album Uguale X` con
/// ordinamento «scaffale» darebbe la stessa cosa passando per il motore, ma
/// perderebbe il punto — qui non si sta scegliendo, si sta continuando.
fn resto_dell_album(
    connection: &Connection,
    brano: &TrackSummary,
    esclusi: &[i64],
) -> Result<Option<i64>, AppError> {
    let Some(chiave) = brano.album_key.as_deref() else {
        return Ok(None);
    };
    let tracce = library::album_tracks(connection, chiave)?;
    // Solo quel che viene **dopo** il brano corrente: ripescare l'inizio del
    // disco che si è appena finito è la cosa che fa sembrare rotto un lettore.
    let dopo_il_corrente = tracce
        .iter()
        .skip_while(|t| t.id != brano.id)
        .skip(1)
        .map(|t| t.id);
    Ok(scegli(dopo_il_corrente, esclusi))
}

/// Un altro disco dello stesso artista, preferendo quelli meno consumati.
fn stesso_artista(
    connection: &Connection,
    brano: &TrackSummary,
    esclusi: &[i64],
    adesso_ms: i64,
) -> Result<Option<i64>, AppError> {
    if brano.artist.is_empty() {
        return Ok(None);
    }
    let insieme = Insieme {
        combinazione: Combinazione::Tutte,
        regole: vec![
            regola(
                Campo::Artista,
                Operatore::Uguale,
                Valore::Testo(brano.artist.clone()),
            ),
            regola(
                Campo::Album,
                Operatore::Diverso,
                Valore::Testo(brano.album.clone()),
            ),
        ],
        limite: Some(CANDIDATI),
        ordinamento: Ordinamento::MenoAscoltati,
    };
    candidati(connection, &insieme, esclusi, adesso_ms)
}

/// Lo stesso genere, ma non quel che si è già sentito questo mese.
///
/// Il genere non sta in [`TrackSummary`] — non lo disegna nessun elenco — e si
/// legge qui con una domanda sola invece di allargare una struttura che
/// attraversa l'IPC a ogni cambio di brano.
fn stesso_genere(
    connection: &Connection,
    corrente: i64,
    esclusi: &[i64],
    adesso_ms: i64,
) -> Result<Option<i64>, AppError> {
    let genere: Option<String> = connection
        .query_row(
            "SELECT genre FROM tracks WHERE id = ?1",
            [corrente],
            |riga| riga.get(0),
        )
        .map_err(|err| library::db_error("genere del brano corrente", &err))?;
    let Some(genere) = genere.filter(|g| !g.is_empty()) else {
        return Ok(None);
    };
    let insieme = Insieme {
        combinazione: Combinazione::Tutte,
        regole: vec![
            regola(Campo::Genere, Operatore::Uguale, Valore::Testo(genere)),
            regola(
                Campo::UltimoAscolto,
                Operatore::NonNegliUltimi,
                Valore::Numero(GIORNI_RECENTI),
            ),
        ],
        limite: Some(CANDIDATI),
        ordinamento: Ordinamento::Casuale,
    };
    candidati(connection, &insieme, esclusi, adesso_ms)
}

/// Un brano col cuore che non è mai stato suonato.
fn un_preferito_mai_sentito(
    connection: &Connection,
    esclusi: &[i64],
    adesso_ms: i64,
) -> Result<Option<i64>, AppError> {
    let insieme = Insieme {
        combinazione: Combinazione::Tutte,
        regole: vec![
            regola(Campo::Preferito, Operatore::Uguale, Valore::Numero(1)),
            regola(Campo::UltimoAscolto, Operatore::Vuoto, Valore::Nessuno),
        ],
        limite: Some(CANDIDATI),
        ordinamento: Ordinamento::Casuale,
    };
    candidati(connection, &insieme, esclusi, adesso_ms)
}

/// Uno a caso, purché non sia di quelli appena sentiti.
///
/// L'ultimo gradino, e l'unico senza condizioni sul contenuto: a questo punto
/// la scelta è fra un brano qualsiasi e il silenzio.
fn uno_qualunque(
    connection: &Connection,
    esclusi: &[i64],
    adesso_ms: i64,
) -> Result<Option<i64>, AppError> {
    let insieme = Insieme {
        combinazione: Combinazione::Tutte,
        regole: Vec::new(),
        limite: Some(CANDIDATI),
        ordinamento: Ordinamento::Casuale,
    };
    candidati(connection, &insieme, esclusi, adesso_ms)
}

/// Una regola, senza la cerimonia.
const fn regola(campo: Campo, operatore: Operatore, valore: Valore) -> Regola {
    Regola {
        campo,
        operatore,
        valore,
    }
}

/// Chiede i candidati al motore delle regole e ne sceglie uno buono.
fn candidati(
    connection: &Connection,
    insieme: &Insieme,
    esclusi: &[i64],
    adesso_ms: i64,
) -> Result<Option<i64>, AppError> {
    let trovati = smart::brani(connection, insieme, adesso_ms)?;
    Ok(scegli(trovati.iter().map(|t| t.id), esclusi))
}

/// Il primo che non sia già stato messo in coda.
fn scegli(fra: impl Iterator<Item = i64>, esclusi: &[i64]) -> Option<i64> {
    fra.into_iter().find(|id| !esclusi.contains(id))
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Lo schema vero, come in `backup.rs`: la cascata legge `album_key`,
    /// `genre` e `last_played_at`, e una tabella semplificata non li avrebbe
    /// con gli stessi vincoli.
    ///
    /// Due dischi dello stesso artista più uno di un altro, tutti mai
    /// ascoltati salvo dove le singole prove dicono il contrario.
    fn libreria() -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, album_key, genre,
                      track_number, duration_ms, file_size, date_added, date_modified)
                 VALUES
                   (1, 'a1.mp3', 'k1', 'Uno',     'Art', 'Primo',  'art|primo',  'Rock', 1, 1000, 1, 1, 1),
                   (2, 'a2.mp3', 'k2', 'Due',     'Art', 'Primo',  'art|primo',  'Rock', 2, 1000, 1, 1, 1),
                   (3, 'a3.mp3', 'k3', 'Tre',     'Art', 'Primo',  'art|primo',  'Rock', 3, 1000, 1, 1, 1),
                   (4, 'b1.mp3', 'k4', 'Quattro', 'Art', 'Secondo','art|secondo','Rock', 1, 1000, 1, 1, 1),
                   (5, 'c1.mp3', 'k5', 'Cinque',  'Altro', 'Terzo','altro|terzo','Jazz', 1, 1000, 1, 1, 1);",
            )
            .expect("brani");
        connection
    }

    /// Il primo gradino: si finisce il disco che si stava ascoltando.
    #[test]
    fn dopo_un_brano_viene_il_seguente_dello_stesso_album() {
        let connection = libreria();
        let scelto = prossimo(&connection, 1, &[1], 0).expect("scelta");
        assert_eq!(scelto, Some(2), "non ha continuato il disco");
    }

    /// Non si ricomincia da capo il disco appena finito.
    ///
    /// È il difetto che fa sembrare rotto un lettore: l'album finisce e
    /// riparte dalla prima traccia come se nessuno avesse ascoltato.
    #[test]
    fn finito_il_disco_non_si_torna_alla_prima_traccia() {
        let connection = libreria();
        // Tutto il disco è già passato in coda: il terzo brano è l'ultimo.
        let scelto = prossimo(&connection, 3, &[1, 2, 3], 0).expect("scelta");
        assert!(
            scelto != Some(1) && scelto != Some(2),
            "ha ripescato una traccia del disco appena finito: {scelto:?}"
        );
        // Il gradino successivo è un altro album dello stesso artista.
        assert_eq!(
            scelto,
            Some(4),
            "non è passato all'altro disco dell'artista"
        );
    }

    /// Quel che è già in coda non si ripropone.
    #[test]
    fn non_ripropone_quel_che_e_gia_in_coda() {
        let connection = libreria();
        let scelto = prossimo(&connection, 1, &[1, 2], 0).expect("scelta");
        assert_eq!(scelto, Some(3), "ha riproposto un brano già accodato");
    }

    /// Esaurito l'artista, si cambia: qui resta solo l'altro genere.
    #[test]
    fn esaurito_l_artista_si_scende_di_gradino() {
        let connection = libreria();
        let scelto = prossimo(&connection, 4, &[1, 2, 3, 4], 0).expect("scelta");
        assert_eq!(scelto, Some(5), "non ha trovato l'unico brano rimasto");
    }

    /// Con tutto già in coda non c'è niente da proporre, e non è un errore.
    #[test]
    fn quando_non_resta_niente_lo_dice_invece_di_fallire() {
        let connection = libreria();
        let scelto = prossimo(&connection, 1, &[1, 2, 3, 4, 5], 0).expect("scelta");
        assert_eq!(scelto, None, "ha proposto qualcosa che era già in coda");
    }

    /// Una libreria vuota non fa cadere niente.
    #[test]
    fn una_libreria_vuota_non_e_un_guasto() {
        let connection = crate::db::open_in_memory().expect("database").connection;
        let scelto = prossimo(&connection, 1, &[], 0).expect("scelta");
        assert_eq!(scelto, None);
    }
}
