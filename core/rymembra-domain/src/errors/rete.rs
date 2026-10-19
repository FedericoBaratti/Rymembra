//! Riconoscere, da un errore di sistema, che il guasto viene dalla rete.
//!
//! # Perché sta nel dominio e non accanto al filesystem
//!
//! Perché la stessa domanda se la fanno due crate che non si conoscono, e non
//! possono conoscersi: `rymembra-app`, quando **apre** un file per leggerne i tag
//! o per suonarlo, e `rymembra-play`, quando lo sta **già leggendo** e la
//! condivisione muore a metà brano. Il secondo non può dipendere dal primo — è
//! scritto nel suo `Cargo.toml`, ed è la stessa riga che tiene aperta la strada
//! per Android, dove i byte arrivano dallo Storage Access Framework.
//!
//! Due elenchi di numeri di sistema, uno per crate, sarebbero due elenchi che un
//! giorno diranno cose diverse: la share che cade all'apertura racconterebbe
//! «riprova» e la stessa share che cade tre secondi dopo racconterebbe «il file
//! è danneggiato». È esattamente il difetto che questo modulo esiste per
//! rendere impossibile, e `rymembra-domain` è l'unica dipendenza che i due hanno
//! in comune.
//!
//! Qui non si fa I/O: si guarda un errore già successo e si dice di che specie
//! è. È dominio puro, come tutto il resto di questo crate.

/// Il guasto viene dalla rete, non dal file?
///
/// Windows non ha un `ErrorKind` per «la share non risponde»: quei guasti
/// arrivano come `Uncategorized` con dentro il numero del sistema, e il numero è
/// l'unica cosa che li distingue da un file rotto. L'elenco è quello che si vede
/// davvero su SMB quando il server si spegne, la VPN cade o si stacca il cavo:
///
/// - 51 `ERROR_REM_NOT_LIST` — la rete non risponde;
/// - 53 `ERROR_BAD_NETPATH` — il percorso di rete non è stato trovato;
/// - 55 `ERROR_DEV_NOT_EXIST` — la risorsa condivisa non c'è più;
/// - 59 `ERROR_UNEXP_NET_ERR` — errore inatteso di rete;
/// - 64 `ERROR_NETNAME_DELETED` — il nome di rete non è più disponibile: è il
///   più comune, ed è quello che arriva **a metà** di una lettura;
/// - 67 `ERROR_BAD_NET_NAME` — il nome di rete non esiste;
/// - 121 `ERROR_SEM_TIMEOUT` — il periodo di timeout del semaforo è scaduto,
///   cioè la share ha smesso di rispondere mentre si leggeva;
/// - 1203 `ERROR_NO_NET_OR_BAD_PATH` — nessun provider di rete ha accettato il
///   percorso;
/// - 1231 `ERROR_NETWORK_UNREACHABLE` e 1232 `ERROR_HOST_UNREACHABLE`;
/// - 1222 `ERROR_NO_NETWORK` — la rete non c'è proprio, il portatile appena
///   svegliato prima che il Wi-Fi si riagganci;
/// - 2250 `ERROR_NOT_CONNECTED` — la lettera mappata esiste ma non è connessa:
///   è quel che risponde `Z:\` all'avvio di Windows, prima che la
///   riconnessione automatica delle unità abbia finito;
/// - 1201 `ERROR_CONNECTION_UNAVAIL` — «il dispositivo non è connesso, ma è una
///   connessione ricordata»: la stessa lettera mappata di 2250 detta con
///   l'altro numero, quello che Windows dà mentre la riconnessione è ancora in
///   corso;
/// - 58 `ERROR_BAD_NET_RESP`, 70 `ERROR_SHARING_PAUSED`, 240
///   `ERROR_VC_DISCONNECTED` (la sessione SMB è stata chiusa), 1225
///   `ERROR_CONNECTION_REFUSED`, 1236 `ERROR_CONNECTION_ABORTED`, 1256
///   `ERROR_HOST_DOWN` e 1262 `ERROR_CSCSHARE_OFFLINE` — un server che c'è ma
///   non serve, una sessione caduta, un NAS che si riavvia;
/// - 65 `ERROR_NETWORK_ACCESS_DENIED`, 1219 `ERROR_SESSION_CREDENTIAL_CONFLICT`,
///   1244 `ERROR_NOT_AUTHENTICATED` e 1326 `ERROR_LOGON_FAILURE` — la share c'è
///   ma non riconosce chi bussa. Sono di rete anche loro, e il «Riprova» qui ha
///   senso: basta aprire la share una volta in Esplora risorse, con la
///   password, perché la seconda lettura passi. Chiamarli guasti del file
///   vorrebbe dire «il file è danneggiato» su un NAS che chiedeva soltanto di
///   accedere.
///
/// Il 3 `ERROR_PATH_NOT_FOUND` **non** c'è, anche se una lettera mappata che
/// non esiste in questa sessione lo restituisce: lo restituisce anche una
/// cartella locale cancellata, e lì «Riprova» sarebbe una promessa falsa.
///
/// Fuori da Windows è sempre `false`, e non per pigrizia: il bersaglio di questa
/// applicazione desktop è Windows, gli errori di rete POSIX (`ENETDOWN`,
/// `EHOSTUNREACH`, `ESTALE`…) non hanno un chiamante che li produca qui, e un
/// elenco scritto a occhio per un sistema che nessuno prova sarebbe un elenco
/// che sbaglia in silenzio. Il ramo resta, dichiarato, per il giorno in cui
/// servisse.
#[cfg(windows)]
#[must_use]
pub fn e_di_rete(err: &std::io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(
            51 | 53
                | 55
                | 58
                | 59
                | 64
                | 65
                | 67
                | 70
                | 121
                | 240
                | 1201
                | 1203
                | 1219
                | 1222
                | 1225
                | 1231
                | 1232
                | 1236
                | 1244
                | 1256
                | 1262
                | 1326
                | 2250
        )
    )
}

/// Vedi la versione Windows: qui è sempre `false`.
#[cfg(not(windows))]
#[must_use]
pub fn e_di_rete(err: &std::io::Error) -> bool {
    let _ = err;
    false
}

#[cfg(test)]
mod prove {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn i_numeri_della_share_si_riconoscono_e_gli_altri_no() {
        for numero in [
            51, 53, 55, 58, 59, 64, 65, 67, 70, 121, 240, 1201, 1203, 1219, 1222, 1225, 1231, 1232,
            1236, 1244, 1256, 1262, 1326, 2250,
        ] {
            assert!(
                e_di_rete(&std::io::Error::from_raw_os_error(numero)),
                "il codice {numero} viene da una share che non risponde"
            );
        }
        // 2 è «file non trovato» e 5 è «accesso negato»: sono guasti del file,
        // e chiamarli rete vorrebbe dire offrire «Riprova» per un file che non
        // tornerà mai. Il 3 per la stessa ragione: è anche la cartella locale
        // cancellata.
        for numero in [2, 3, 5, 32, 112] {
            assert!(
                !e_di_rete(&std::io::Error::from_raw_os_error(numero)),
                "il codice {numero} non c'entra con la rete"
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn fuori_da_windows_non_si_riconosce_niente() {
        assert!(!e_di_rete(&std::io::Error::from_raw_os_error(64)));
    }
}
