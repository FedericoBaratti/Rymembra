//! Le costanti che Spotify cambia quando vuole, tenute dove si possono cambiare
//! anche noi.
//!
//! # Il problema che questo modulo risolve
//!
//! Tre cose di questo crate hanno una scadenza che non decidiamo noi: i cifrari
//! del TOTP, gli hash delle interrogazioni persistite di Pathfinder, e la
//! versione dichiarata del lettore web. Non sono *configurazione* nel senso
//! normale — l'utente non deve toccarle, e nessun valore è «giusto per lui» —
//! sono valori che oggi funzionano e domani no.
//!
//! Scritti come costanti Rust, ogni rotazione di Spotify diventerebbe una
//! release: compilare, firmare, pubblicare, e nel frattempo l'importazione è
//! rotta per tutti. Scritti in un file letto all'avvio, diventano una riga da
//! correggere — e nell'attesa il lettore scende comunque ai livelli che non
//! dipendono da questi valori.
//!
//! I valori compilati restano, e sono quelli buoni al momento in cui si scrive:
//! il file serve a **soprascriverli**, non a fornirli. Un'installazione senza
//! file funziona, ed è il caso normale.
//!
//! # Provenienza dei valori predefiniti
//!
//! - Cifrario TOTP **v61**: pubblicato da Spotify a gennaio 2026, tuttora quello
//!   che il lettore web seleziona (agosto 2026). Le versioni precedenti restano
//!   come ripiego: provarle non costa niente e ogni tanto una vecchia
//!   installazione del lettore ne accetta una.
//! - Hash delle interrogazioni: registro pubblico aggiornato a maggio 2026.
//! - Identificativo del lettore web: costante pubblica del lettore, non un
//!   segreto — è nel sorgente di ogni pagina di `open.spotify.com`.

use std::path::Path;

use serde::Deserialize;

use crate::totp::Cifrario;

/// Come Aether si presenta ai punti interni del lettore web.
///
/// Uno `User-Agent` da browser, e non il nostro: vedi la nota su
/// `Rete::nuova_con_agente` in `aether-net`. Non è per nascondersi — la
/// richiesta arriva da un'applicazione che non è un browser comunque — è che
/// quei punti a un agente sconosciuto rispondono in modo diverso, quando
/// rispondono.
pub const AGENTE: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0.0.0 Safari/537.36";

/// L'identificativo del lettore web, che serve allo scambio del `client-token`.
const CLIENT_ID_PREDEFINITO: &str = "d8a5ed958d274c2e8ee717e6a4b0971d";

/// La versione del lettore da dichiarare quando non si riesce a leggerla dalla
/// pagina. Un valore plausibile è meglio di nessun valore: il punto di scambio
/// rifiuta una richiesta senza versione, mentre una versione vecchia la accetta.
const VERSIONE_CLIENT_PREDEFINITA: &str = "harmony:4.42.0-2780565d";

/// Un'interrogazione persistita: come si chiama e l'impronta con cui il server
/// la ritrova.
///
/// Su Pathfinder non si può mandare il testo di una query GraphQL: si manda il
/// nome dell'operazione e l'impronta SHA-256 di un testo che il server ha già.
/// È la ragione per cui questi valori sono indispensabili e volatili insieme.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Interrogazione {
    /// Il nome dell'operazione, es. `getAlbum`.
    pub operazione: String,
    /// L'impronta SHA-256 del testo della query, in esadecimale.
    pub hash: String,
}

impl Interrogazione {
    fn nuova(operazione: &str, hash: &str) -> Self {
        Self {
            operazione: operazione.to_owned(),
            hash: hash.to_owned(),
        }
    }
}

/// Le quattro interrogazioni che servono a leggere un contenuto.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Interrogazioni {
    /// Un brano singolo.
    pub brano: Interrogazione,
    /// Un album e i suoi brani.
    pub album: Interrogazione,
    /// Una playlist e i suoi brani.
    pub playlist: Interrogazione,
    /// Un artista, di cui si prende quel che il lettore mostra in copertina.
    pub artista: Interrogazione,
}

impl Default for Interrogazioni {
    fn default() -> Self {
        Self {
            brano: Interrogazione::nuova(
                "getTrack",
                "612585ae06ba435ad26369870deaae23b5c8800a256cd8a57e08eddc25a37294",
            ),
            album: Interrogazione::nuova(
                "getAlbum",
                "b9bfabef66ed756e5e13f68a942deb60bd4125ec1f1be8cc42769dc0259b4b10",
            ),
            playlist: Interrogazione::nuova(
                "fetchPlaylist",
                "a65e12194ed5fc443a1cdebed5fabe33ca5b07b987185d63c72483867ad13cb4",
            ),
            artista: Interrogazione::nuova(
                "queryArtistOverview",
                "7f86ff63e38c24973a2842b672abe44c910c1973978dc8a4a0cb648edef34527",
            ),
        }
    }
}

/// Tutto quel che ha una scadenza decisa da Spotify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Configurazione {
    /// I cifrari del TOTP, dal più recente al più vecchio. Si provano in ordine.
    pub cifrari: Vec<Cifrario>,
    /// L'identificativo del lettore web.
    pub client_id: String,
    /// La versione del lettore da usare se non la si legge dalla pagina.
    pub versione_client: String,
    /// Le interrogazioni persistite.
    pub interrogazioni: Interrogazioni,
}

impl Default for Configurazione {
    fn default() -> Self {
        Self {
            cifrari: cifrari_predefiniti(),
            client_id: CLIENT_ID_PREDEFINITO.to_owned(),
            versione_client: VERSIONE_CLIENT_PREDEFINITA.to_owned(),
            interrogazioni: Interrogazioni::default(),
        }
    }
}

/// I cifrari noti, dal più recente. Provarli tutti costa una richiesta ciascuno
/// e solo quando il primo fallisce.
fn cifrari_predefiniti() -> Vec<Cifrario> {
    [
        (
            61_u32,
            &[
                44_u8, 55, 47, 42, 70, 40, 34, 114, 76, 74, 50, 111, 120, 97, 75, 76, 94, 102, 43,
                69, 49, 120, 118, 80, 64, 78,
            ][..],
        ),
        (
            14,
            &[
                62, 54, 109, 83, 107, 77, 41, 103, 45, 93, 114, 38, 41, 97, 64, 51, 95, 94, 95, 94,
            ][..],
        ),
        (
            13,
            &[
                59, 92, 64, 70, 99, 78, 117, 75, 99, 103, 116, 67, 103, 51, 87, 63, 93, 59, 70, 45,
                32,
            ][..],
        ),
        (
            12,
            &[
                12, 56, 76, 33, 88, 44, 88, 33, 78, 78, 11, 66, 22, 22, 55, 69, 54,
            ][..],
        ),
        (
            11,
            &[
                111, 45, 40, 73, 89, 53, 67, 47, 76, 105, 65, 116, 100, 45, 51, 78, 50,
            ][..],
        ),
    ]
    .into_iter()
    .map(|(versione, cifre)| Cifrario {
        versione,
        cifre: cifre.to_vec(),
    })
    .collect()
}

/// La forma del file di soprascrittura. Ogni campo è opzionale: si corregge quel
/// che è scaduto, non si riscrive tutto.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfigurazione {
    cifrari: Option<Vec<CifrarioSuFile>>,
    client_id: Option<String>,
    versione_client: Option<String>,
    interrogazioni: Option<Interrogazioni>,
}

/// Un cifrario come si scrive nel file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CifrarioSuFile {
    versione: u32,
    cifre: Vec<u8>,
}

/// Come si chiama il file, nella cartella dati dell'applicazione.
pub const NOME_FILE: &str = "spotify.json";

impl Configurazione {
    /// Carica la configurazione, soprascrivendo i valori compilati con quelli
    /// del file se il file c'è ed è leggibile.
    ///
    /// # Perché un file illeggibile non è un errore
    ///
    /// Perché il risultato di trattarlo come tale sarebbe peggio del problema:
    /// un JSON con una virgola di troppo impedirebbe di importare da Spotify
    /// anche quando i valori compilati vanno benissimo. Il file esiste per
    /// riparare, e una riparazione scritta male deve poter fallire senza
    /// rompere quel che riparava.
    ///
    /// Chi vuole sapere se è stato letto lo chiede a [`Self::carica_riportando`].
    #[must_use]
    pub fn carica(percorso: &Path) -> Self {
        Self::carica_riportando(percorso).0
    }

    /// Come [`Self::carica`], ma dice anche cosa è successo al file.
    ///
    /// Serve alla diagnostica: «il file c'è ma non si legge» è precisamente
    /// l'informazione che manca a chi ha appena provato a riparare una
    /// rotazione e non vede cambiare niente.
    #[must_use]
    pub fn carica_riportando(percorso: &Path) -> (Self, EsitoFile) {
        let mut configurazione = Self::default();
        let Ok(testo) = std::fs::read_to_string(percorso) else {
            return (configurazione, EsitoFile::Assente);
        };
        let file: FileConfigurazione = match serde_json::from_str(&testo) {
            Ok(letto) => letto,
            Err(err) => return (configurazione, EsitoFile::Illeggibile(err.to_string())),
        };

        if let Some(cifrari) = file.cifrari
            && !cifrari.is_empty()
        {
            configurazione.cifrari = cifrari
                .into_iter()
                .map(|c| Cifrario {
                    versione: c.versione,
                    cifre: c.cifre,
                })
                .collect();
        }
        if let Some(id) = file.client_id {
            configurazione.client_id = id;
        }
        if let Some(versione) = file.versione_client {
            configurazione.versione_client = versione;
        }
        if let Some(interrogazioni) = file.interrogazioni {
            configurazione.interrogazioni = interrogazioni;
        }
        (configurazione, EsitoFile::Letto)
    }
}

/// Cosa è successo al file di soprascrittura.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EsitoFile {
    /// Non c'è. È il caso normale.
    Assente,
    /// C'è, ed è stato applicato.
    Letto,
    /// C'è ma non si è capito, e i valori compilati sono rimasti.
    Illeggibile(String),
}

#[cfg(test)]
mod prove {
    use super::*;

    fn scrivi(contenuto: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let Ok(cartella) = tempfile::tempdir() else {
            panic!("serve una cartella temporanea");
        };
        let percorso = cartella.path().join(NOME_FILE);
        let Ok(()) = std::fs::write(&percorso, contenuto) else {
            panic!("il file si deve poter scrivere");
        };
        (cartella, percorso)
    }

    #[test]
    fn senza_file_restano_i_valori_compilati() {
        let (configurazione, esito) =
            Configurazione::carica_riportando(Path::new("non-esiste-davvero.json"));
        assert_eq!(esito, EsitoFile::Assente);
        assert_eq!(configurazione, Configurazione::default());
        assert!(
            !configurazione.cifrari.is_empty(),
            "senza cifrari non si può nemmeno provare"
        );
    }

    #[test]
    fn il_file_soprascrive_solo_quel_che_nomina() {
        let (_cartella, percorso) = scrivi(r#"{ "versione_client": "harmony:9.9.9" }"#);
        let (configurazione, esito) = Configurazione::carica_riportando(&percorso);
        assert_eq!(esito, EsitoFile::Letto);
        assert_eq!(configurazione.versione_client, "harmony:9.9.9");
        assert_eq!(
            configurazione.cifrari,
            cifrari_predefiniti(),
            "quel che il file non nomina resta com'era"
        );
    }

    #[test]
    fn un_cifrario_nuovo_prende_il_posto_dei_vecchi() {
        // È il gesto per cui questo file esiste: Spotify ruota, si incolla il
        // cifrario nuovo, l'importazione riparte senza una release.
        let (_cartella, percorso) =
            scrivi(r#"{ "cifrari": [ { "versione": 62, "cifre": [1, 2, 3] } ] }"#);
        let configurazione = Configurazione::carica(&percorso);
        assert_eq!(
            configurazione.cifrari,
            vec![Cifrario {
                versione: 62,
                cifre: vec![1, 2, 3]
            }]
        );
    }

    #[test]
    fn un_file_storto_non_rompe_quel_che_riparava() {
        let (_cartella, percorso) = scrivi("{ questo non è json");
        let (configurazione, esito) = Configurazione::carica_riportando(&percorso);
        assert!(matches!(esito, EsitoFile::Illeggibile(_)));
        assert_eq!(
            configurazione,
            Configurazione::default(),
            "i valori compilati sopravvivono a una riparazione scritta male"
        );
    }

    #[test]
    fn un_elenco_di_cifrari_vuoto_non_disarma_il_lettore() {
        // Scrivere `"cifrari": []` è un modo facile di rendersi inutilizzabili
        // senza accorgersene: si ignora.
        let (_cartella, percorso) = scrivi(r#"{ "cifrari": [] }"#);
        let configurazione = Configurazione::carica(&percorso);
        assert_eq!(configurazione.cifrari, cifrari_predefiniti());
    }

    #[test]
    fn gli_hash_predefiniti_sono_impronte_ben_formate() {
        let i = Interrogazioni::default();
        for interrogazione in [&i.brano, &i.album, &i.playlist, &i.artista] {
            assert_eq!(
                interrogazione.hash.len(),
                64,
                "un SHA-256 in esadecimale è lungo 64: {}",
                interrogazione.operazione
            );
            assert!(
                interrogazione
                    .hash
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "impronta non esadecimale minuscola: {}",
                interrogazione.operazione
            );
        }
    }
}
