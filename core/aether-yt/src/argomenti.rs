//! Gli argomenti di yt-dlp, e perché sono esattamente questi.
//!
//! Porto di `legacy/Aeter/electron/modules/download/ytArgs.ts`. Ogni costante
//! qui sotto è stata scritta in risposta a un guasto vero: **non si
//! semplificano**. Toglierne una non rompe la compilazione e non fa cadere
//! nessuna prova — fa tornare i `403 Forbidden` di YouTube, e mesi dopo.
//!
//! # L'unica differenza voluta rispetto al vecchio albero
//!
//! Là i tre tentativi allargavano progressivamente il formato
//! (`bestaudio[ext=m4a]/bestaudio/best` → `bestaudio/best` → `best`), perché con
//! ffmpeg a valle qualunque cosa arrivasse veniva poi convertita.
//!
//! Qui ffmpeg non c'è — si scarica m4a così com'è, senza ricomprimere audio già
//! compresso — quindi allargare così sarebbe una trappola: `bestaudio` su YouTube
//! è quasi sempre Opus in un contenitore WebM, e symphonia **non decodifica
//! Opus** (`core/aether-play/Cargo.toml:22-36` lo dice per esteso). Il file
//! arriverebbe, la scansione lo prenderebbe, e il guasto si scoprirebbe premendo
//! play — che è il modo peggiore di scoprirlo.
//!
//! I tre tentativi cambiano quindi **il client**, che è ciò che davvero risolve i
//! 403, e tengono il formato ancorato a m4a in tutti e tre.

/// Selezione del client e tolleranza al PO token. Il profilo principale.
///
/// YouTube rifiuta i flussi del client predefinito con `HTTP Error 403` a meno
/// che non si (a) scelgano client che restituiscono formati senza PO token del
/// GVS e (b) si accettino i formati che il token non ce l'hanno, invece di
/// lasciare che yt-dlp li filtri via tutti. Ricerca e scaricamento usano lo
/// stesso profilo di proposito: il client che abbina un video dev'essere quello
/// che poi lo scarica.
pub const ARG_ESTRATTORE: [&str; 2] = [
    "--extractor-args",
    "youtube:player_client=default,tv,web_safari;formats=missing_pot",
];

/// L'irrobustimento di rete comune a ogni invocazione.
///
/// `--force-ipv4` evita una classe di 403 e di strozzature legate a IPv6; i
/// conteggi di ritentativo assorbono i 403 passeggeri che YouTube spruzza sotto
/// SABR.
pub const ARG_RETE: [&str; 7] = [
    "--force-ipv4",
    "--retries",
    "5",
    "--fragment-retries",
    "10",
    "--extractor-retries",
    "3",
];

/// Gli argomenti di sola velocità, che alla ricerca non servono.
///
/// Non stanno in [`ARG_RETE`] perché ricerca e anteprima non trasferiscono
/// media. YouTube strozza per connessione, quindi il modo di riempire il tubo è
/// il parallelismo: sei frammenti insieme, e richieste a blocchi da 10M così
/// anche i formati a frammento unico scendono su più richieste di intervallo.
/// La qualità dell'audio non c'entra: cambia solo **come** si prendono i byte.
pub const ARG_VELOCITA: [&str; 4] = ["--concurrent-fragments", "6", "--http-chunk-size", "10M"];

/// Il formato: m4a, e solo m4a.
///
/// Tre alternative in cascata, tutte AAC in contenitore MP4, perché è ciò che
/// symphonia sa decodificare e `m4a` è in `SUPPORTED_EXTENSIONS`. Se nessuna
/// esiste per quel video, yt-dlp fallisce — ed è l'esito giusto: meglio un brano
/// segnato come non riuscito che un file muto in libreria.
pub const FORMATO_M4A: &str = "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/best[ext=m4a]";

/// Un tentativo di scaricamento: un profilo di client più il selettore di formato.
///
/// Gli scaricamenti camminano [`TENTATIVI`] in ordine, passando al successivo
/// **solo** quando il guasto somiglia a un 403 o a un problema di firma (vedi
/// [`ritentabile`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tentativo {
    /// Gli argomenti `--extractor-args` di questo profilo.
    pub estrattore: &'static [&'static str],
    /// Il selettore `--format`.
    pub formato: &'static str,
}

/// I tre profili, dal più compatibile al più ostinato.
pub const TENTATIVI: [Tentativo; 3] = [
    Tentativo {
        estrattore: &[
            "--extractor-args",
            "youtube:player_client=default,tv,web_safari;formats=missing_pot",
        ],
        formato: FORMATO_M4A,
    },
    Tentativo {
        estrattore: &[
            "--extractor-args",
            "youtube:player_client=tv;formats=missing_pot",
        ],
        formato: FORMATO_M4A,
    },
    Tentativo {
        estrattore: &["--extractor-args", "youtube:player_client=web_safari"],
        formato: FORMATO_M4A,
    },
];

/// Le tracce nello stderr che dicono «prova col profilo dopo».
const SPIE_RITENTABILI: [&str; 6] = [
    "http error 403",
    "403: forbidden",
    "nsig extraction failed",
    "signature extraction failed",
    "requested format is not available",
    "player_client",
];

/// Vero quando uno scaricamento fallito va ritentato col profilo successivo.
///
/// Un 403, o un guasto di firma o di formato che un client diverso può
/// risolvere. Distingue un «audio irraggiungibile» vero da un «non l'ho trovato»,
/// che sono la stessa schermata per l'utente e due cose opposte per la coda.
#[must_use]
pub fn ritentabile(stderr: &str) -> bool {
    let piegato = stderr.to_lowercase();
    SPIE_RITENTABILI.iter().any(|spia| piegato.contains(spia))
}

/// L'instradamento dei percorsi: i file finiti sotto `casa`, gli intermedi sotto
/// `temporanea`.
///
/// I `.part`, i frammenti e i file a metà scrittura devono stare **fuori** dalla
/// cartella sorvegliata, o la scansione ingoia un file troncato — che i lettori
/// di metadati spesso riescono a leggere lo stesso, e quindi entra in libreria
/// come un brano normale che non si sente (`aether_domain::paths::MIN_TRACK_BYTES`
/// racconta la stessa storia dall'altro lato).
///
/// # Attenzione
///
/// yt-dlp **ignora** `-P` quando `--output` è un percorso assoluto. Ogni modello
/// di uscita passato insieme a questi argomenti **deve** essere relativo.
#[must_use]
pub fn arg_percorsi(casa: &std::path::Path, temporanea: &std::path::Path) -> Vec<String> {
    vec![
        "-P".to_owned(),
        format!("home:{}", casa.display()),
        "-P".to_owned(),
        format!("temp:{}", temporanea.display()),
    ]
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_403_si_ritentano_e_il_resto_no() {
        assert!(ritentabile(
            "ERROR: unable to download video data: HTTP Error 403: Forbidden"
        ));
        assert!(ritentabile(
            "nsig extraction failed: Some players may not work"
        ));
        assert!(ritentabile("ERROR: Requested format is not available"));
        // Maiuscole e minuscole non devono contare: il testo di yt-dlp cambia.
        assert!(ritentabile("HTTP ERROR 403"));

        // Questi non sono problemi di client: cambiarlo non li risolve, e
        // ritentare tre volte vuol dire solo far aspettare l'utente il triplo.
        assert!(!ritentabile("ERROR: Video unavailable"));
        assert!(!ritentabile("ERROR: Private video"));
        assert!(!ritentabile(""));
    }

    #[test]
    fn ogni_tentativo_resta_ancorato_a_m4a() {
        // È la guardia contro la «semplificazione» che rimetterebbe `bestaudio`
        // nudo: quel formato dà Opus, e Opus non si sente (vedi il commento in
        // testa al modulo).
        for tentativo in TENTATIVI {
            assert!(
                !tentativo.formato.contains("bestaudio/best"),
                "un formato non vincolato rimette Opus in gioco"
            );
            assert!(tentativo.formato.contains("m4a") || tentativo.formato.contains("mp4a"));
        }
    }

    #[test]
    fn i_tentativi_cambiano_client_uno_per_uno() {
        // Se due profili fossero identici, il secondo giro sarebbe tempo
        // dell'utente speso a rifare esattamente la stessa richiesta.
        let mut visti: Vec<&[&str]> = Vec::new();
        for tentativo in TENTATIVI {
            assert!(
                !visti.contains(&tentativo.estrattore),
                "due tentativi con lo stesso client"
            );
            visti.push(tentativo.estrattore);
        }
    }

    #[test]
    fn i_percorsi_si_dichiarano_col_prefisso_giusto() {
        let args = arg_percorsi(
            std::path::Path::new("C:/Musica"),
            std::path::Path::new("C:/Temp/aether"),
        );
        assert_eq!(args.len(), 4);
        assert_eq!(args.first().map(String::as_str), Some("-P"));
        assert!(args.get(1).is_some_and(|a| a.starts_with("home:")));
        assert!(args.get(3).is_some_and(|a| a.starts_with("temp:")));
    }
}
