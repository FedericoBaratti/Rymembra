//! Il testo della canzone fra dispositivi collegati.
//!
//! Un brano di un altro dispositivo è una riga specchio (`lontano://…`, vedi
//! [`crate::specchio`]) e il suo testo lo possiede **lui**: il suo file, il
//! suo `.lrc`, la sua tabella `lyrics`. Qui sta quel che serve ai due lati.
//!
//! # Chi possiede il brano
//!
//! - [`brano_condiviso`] è il confine di fiducia: un brano si dà solo se è
//!   uno di quelli che la libreria condivide (`CONDIVISO`: né cloud, né
//!   catalogo, né una riga specchio di un terzo). Vale per la lettura e per
//!   la scrittura, e un brano che non lo è risponde come uno che non c'è.
//! - [`testo_da_dare`] compone il testo con **tutta** la catena di chi serve,
//!   senza scrivere: è una lettura, e sta fuori dal lucchetto.
//! - [`scrivi_dal_dispositivo`] applica una correzione con la logica dei
//!   comandi di sempre — [`imposta_scarto`], [`registra_a_mano`],
//!   [`ricorda_scelta`], [`rifiuta`] — dopo averla convalidata. **Solo la
//!   libreria**: un dispositivo collegato non scrive file nelle cartelle della
//!   musica di chi serve, neanche il `.lrc` che i salvataggi di qui scrivono
//!   quando l'interruttore è acceso.
//!
//! # Chi chiede
//!
//! - [`convalida_ricevuto`] non prende niente sulla parola: lunghezze, numero
//!   di righe, tempi, caratteri. Un testo che non sta nei tetti si scarta per
//!   intero, e per chi chiede vale «il proprietario non ha niente».
//! - [`metti_in_posta`] tiene le correzioni date qui a un brano dell'altro
//!   finché lui non risponde. Una voce sola per brano, in cui i gesti si
//!   fondono come si fonderebbero applicandoli in fila ([`Correzione`]).
//!
//! # I tetti
//!
//! Una domanda sul filo non supera [`rymembra_lontano::protocollo`]'s
//! `MASSIMA_DOMANDA` (64 KiB) **aperta**, escape compresi: un testo di [`MASSIMO_TESTO`]
//! byte, con un fine riga ogni venti caratteri, ci sta con margine. Un testo
//! più lungo non entra in posta: non verrebbe mai preso, e una voce che resta
//! lì ferma tutte le altre.

use rusqlite::Connection;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::testo::{self, Candidato, Testo};
use rymembra_meta::lrclib::Voce;
use serde::{Deserialize, Serialize};

use super::{
    BranoDaTestare, COLONNE, DalDisco, Fonte, Riga, TestoBrano, brano_da_riga, componi_con,
    imposta_scarto, registra_a_mano, ricorda_scelta, rifiuta,
};
use crate::dispositivi::{self, POSTA_TESTO};
use crate::library::condivisa::CONDIVISO;
use crate::library::db_error;

/// Quanti byte, al più, ha il testo di una correzione che parte da qui. Sta
/// nella cornice di una domanda con margine: vedi il `//!`.
pub const MASSIMO_TESTO: usize = 48_000;

/// Quanti byte, al più, ha un testo che arriva da un altro dispositivo. Una
/// risposta può essere più lunga di una domanda (il suo tetto è quello della
/// cornice), ma un testo di canzone non arriva a un quarto di megabyte.
pub const MASSIMO_TESTO_RICEVUTO: usize = 256 * 1024;

/// Quante righe con un tempo, al più, ha un testo.
pub const MASSIME_RIGHE: usize = 4_000;

/// Quante parole con un tempo, al più, ha un testo.
pub const MASSIME_PAROLE: usize = 40_000;

/// Il tempo più lontano che una riga o una parola può avere: dodici ore.
pub const MASSIMO_TEMPO_MS: u32 = 12 * 3_600 * 1_000;

/// Lo scarto più lontano che si accetta, in ms: un'ora.
pub const MASSIMO_SCARTO_MS: i32 = 3_600_000;

/// Il proprietario di un brano, come lo dice la riga specchio: il suo
/// dispositivo e l'id che il brano ha nella sua libreria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remoto {
    /// La chiave del dispositivo, in minuscolo.
    pub dispositivo: String,
    /// L'id del brano nella libreria del proprietario.
    pub id_remoto: i64,
}

/// Il testo di un brano com'è sul filo, dal proprietario a chi chiede.
///
/// Una forma a parte da [`Testo`] perché `rymembra-domain` non conosce serde, e
/// perché sul filo il testo viaggia come lo si scrive in un file: un LRC.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TestoCondiviso {
    /// L'LRC, se il testo scorre. Con i tempi delle parole e le traduzioni,
    /// quando le ha.
    pub sincronizzato: Option<String>,
    /// Il testo senza tempi, se non scorre.
    pub piatto: Option<String>,
    /// Il brano non ha parole.
    pub strumentale: bool,
    /// Da dove viene, per il proprietario: `sidecar`, `tag`, `lrclib`, `mano`.
    pub fonte: String,
    /// La correzione di chi ascolta dal proprietario, nel verso dello
    /// standard.
    pub scarto_ms: i32,
    /// Il proprietario ha già chiesto al catalogo per questo brano.
    pub cercato: bool,
}

/// Il testo del proprietario, convalidato e letto: quel che [`componi_con`]
/// mette in testa alla catena.
#[derive(Debug, Clone)]
pub struct DalProprietario {
    /// Il testo, nella forma del dominio.
    pub testo: Testo,
    /// Il testo com'è arrivato, per ricordarlo in tabella.
    pub grezzo: String,
    /// Da dove viene, per lui.
    pub fonte: Fonte,
    /// La sua correzione.
    pub scarto_ms: i32,
    /// Lui ha già chiesto al catalogo.
    pub cercato: bool,
}

/// Una scrittura che un altro dispositivo chiede a chi possiede il brano.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scrittura {
    /// Lo scarto di chi ascolta.
    Scarto(i32),
    /// Un nuovo testo.
    Testo(NuovoTesto),
}

/// Un gesto che cambia il testo di un brano.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "tipo",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NuovoTesto {
    /// Un testo sincronizzato a mano: l'LRC intero.
    Mano {
        /// L'LRC.
        lrc: String,
    },
    /// Una voce del catalogo scelta dall'elenco.
    Scelta {
        /// L'identificativo della voce.
        id_catalogo: i64,
        /// Il testo senza tempi, se la voce non ha l'LRC.
        piatto: Option<String>,
        /// L'LRC, quando la voce ce l'ha.
        sincronizzato: Option<String>,
        /// La voce dice che il brano non ha parole.
        strumentale: bool,
    },
    /// «Non è questo»: butta il testo del catalogo.
    Rifiuta,
}

/// Quel che aspetta in posta per un brano: l'ultimo scarto e l'ultimo testo,
/// fusi come lo sarebbero applicando i gesti in fila.
///
/// Il testo si applica **prima** dello scarto: un testo a mano azzera lo scarto
/// (le sue battute nascono nello spazio della posizione grezza), quindi uno
/// scarto dato dopo di lui deve vincere, e uno dato prima deve sparire. La
/// fusione lo sa: un testo a mano porta con sé uno scarto a zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Correzione {
    /// Lo scarto da mettere, se è cambiato.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scarto_ms: Option<i32>,
    /// Il testo da mettere, se è cambiato.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub testo: Option<NuovoTesto>,
}

impl Correzione {
    /// Il contenuto di una voce di posta, o `None` se non si legge.
    #[must_use]
    pub fn da_corpo(corpo: &str) -> Option<Self> {
        serde_json::from_str(corpo).ok()
    }

    /// Il contenuto, in JSON.
    #[must_use]
    pub fn in_corpo(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Questa correzione dopo un gesto in più.
    #[must_use]
    pub fn dopo(self, gesto: &Scrittura) -> Self {
        match gesto {
            Scrittura::Scarto(scarto) => Self {
                scarto_ms: Some(*scarto),
                ..self
            },
            Scrittura::Testo(testo @ NuovoTesto::Mano { .. }) => Self {
                scarto_ms: Some(0),
                testo: Some(testo.clone()),
            },
            Scrittura::Testo(testo) => Self {
                testo: Some(testo.clone()),
                ..self
            },
        }
    }

    /// Le scritture da mandare, nell'ordine in cui vanno applicate.
    #[must_use]
    pub fn scritture(&self) -> Vec<Scrittura> {
        self.testo
            .iter()
            .cloned()
            .map(Scrittura::Testo)
            .chain(self.scarto_ms.map(Scrittura::Scarto))
            .collect()
    }
}

fn fuori_dai_tetti(cosa: &str) -> AppError {
    AppError::new(ErrorCode::LontanoProtocollo).with_cause(cosa.to_owned())
}

// ── chi possiede il brano ───────────────────────────────────────────────────

/// Il brano `id`, solo se la libreria lo condivide: `None` per un brano del
/// cloud, di catalogo, per una riga specchio di un terzo dispositivo e per
/// uno che non c'è.
///
/// È la sola porta per i testi di un altro dispositivo, in lettura e in
/// scrittura: oggi `testi::brano` non conosce `CONDIVISO`, e un pari che
/// chiedesse un id qualunque leggerebbe — e scriverebbe — il testo di quel
/// che non gli si dà.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brano_condiviso(
    connection: &Connection,
    id: i64,
) -> Result<Option<BranoDaTestare>, AppError> {
    use rusqlite::OptionalExtension as _;
    let sql = format!("SELECT {COLONNE} FROM tracks t WHERE t.id = ?1 AND {CONDIVISO}");
    connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("lettura di un brano condiviso per il testo", &err))?
        .query_row([id], brano_da_riga)
        .optional()
        .map_err(|err| db_error("lettura di un brano condiviso per il testo", &err))
}

/// Il testo di un brano com'è da dare a un altro dispositivo: composto con la
/// catena intera di chi serve, **senza scrivere** la cache in tabella. `None`
/// se non c'è niente da dire (né un testo né uno strumentale).
///
/// Non tocca il database: la riga e il disco li ha letti chi chiama, nei tre
/// tempi dei comandi di sempre.
#[must_use]
pub fn testo_da_dare(
    brano: &BranoDaTestare,
    riga: Option<Riga>,
    disco: DalDisco,
) -> Option<TestoCondiviso> {
    let trovato: TestoBrano = componi_con(None, brano, riga, disco);
    if trovato.testo.vuoto() {
        return None;
    }
    let scorre = trovato.testo.sincronizzato();
    Some(TestoCondiviso {
        sincronizzato: scorre.then(|| testo::scrivi(&trovato.testo)),
        piatto: if scorre || trovato.testo.strumentale {
            None
        } else {
            trovato.testo.piatto.clone()
        },
        strumentale: trovato.testo.strumentale,
        fonte: trovato.fonte.nome().to_owned(),
        scarto_ms: trovato.scarto_ms,
        cercato: trovato.cercato,
    })
}

/// Applica una scrittura arrivata da un altro dispositivo a un brano di qui.
///
/// La convalida va prima di ogni scrittura, e il brano deve essere condiviso:
/// un brano che non lo è risponde `lontano.fileCambiato`, come per le altre
/// scritture. Le funzioni sono quelle dei comandi di sempre, quindi il testo
/// scritto a mano non si perde e una voce scartata non torna; nessun file
/// accanto al brano si tocca.
///
/// # Errori
///
/// `lontano.fileCambiato` per un brano che non è condiviso; `lontano.protocollo`
/// per una scrittura fuori dai tetti; `db.queryFailed` se il database non
/// risponde.
pub fn scrivi_dal_dispositivo(
    connection: &Connection,
    id: i64,
    scrittura: &Scrittura,
) -> Result<(), AppError> {
    // La convalida prima di tutto, anche prima di guardare il brano: una
    // scrittura storta si rifiuta con lo stesso codice per ogni brano.
    let voce = convalida_scrittura(scrittura)?;
    let Some(brano) = brano_condiviso(connection, id)? else {
        return Err(AppError::new(ErrorCode::LontanoFileCambiato)
            .with_cause("il brano non è più condiviso"));
    };
    match scrittura {
        Scrittura::Scarto(scarto) => imposta_scarto(connection, &brano.track_key, *scarto),
        Scrittura::Testo(NuovoTesto::Mano { lrc }) => registra_a_mano(connection, &brano, lrc),
        Scrittura::Testo(NuovoTesto::Scelta { .. }) => {
            let voce = voce.ok_or_else(|| fuori_dai_tetti("una voce scelta senza voce"))?;
            ricorda_scelta(connection, &brano, &voce)
        }
        Scrittura::Testo(NuovoTesto::Rifiuta) => rifiuta(connection, &brano.track_key),
    }
}

/// Controlla una scrittura, e per una voce scelta prepara la voce.
fn convalida_scrittura(scrittura: &Scrittura) -> Result<Option<Voce>, AppError> {
    match scrittura {
        Scrittura::Scarto(scarto) => {
            controlla_lo_scarto(*scarto)?;
            Ok(None)
        }
        Scrittura::Testo(NuovoTesto::Mano { lrc }) => {
            let letto = controlla_il_testo(lrc, MASSIMO_TESTO)?;
            if !letto.sincronizzato() {
                return Err(fuori_dai_tetti("un testo a mano senza i tempi"));
            }
            Ok(None)
        }
        Scrittura::Testo(NuovoTesto::Scelta {
            id_catalogo,
            piatto,
            sincronizzato,
            strumentale,
        }) => {
            if *id_catalogo <= 0 {
                return Err(fuori_dai_tetti("una voce senza identificativo"));
            }
            let mut nessuno = true;
            for grezzo in [piatto, sincronizzato].into_iter().flatten() {
                controlla_il_testo(grezzo, MASSIMO_TESTO)?;
                nessuno &= grezzo.trim().is_empty();
            }
            if nessuno && !strumentale {
                return Err(fuori_dai_tetti("una voce senza niente dentro"));
            }
            Ok(Some(Voce {
                candidato: Candidato {
                    id: *id_catalogo,
                    strumentale: *strumentale,
                    sincronizzato: sincronizzato.is_some(),
                    ..Candidato::default()
                },
                piatto: piatto.clone(),
                sincronizzato: sincronizzato.clone(),
            }))
        }
        Scrittura::Testo(NuovoTesto::Rifiuta) => Ok(None),
    }
}

fn controlla_lo_scarto(scarto: i32) -> Result<(), AppError> {
    if scarto.unsigned_abs() > MASSIMO_SCARTO_MS.unsigned_abs() {
        return Err(fuori_dai_tetti("uno scarto che non è uno scarto"));
    }
    Ok(())
}

/// Un testo che arriva da fuori: nei tetti di lunghezza, senza caratteri di
/// controllo, con un numero di righe e parole ragionevole e tempi che stanno in
/// una giornata. Lo restituisce già letto.
fn controlla_il_testo(grezzo: &str, massimo: usize) -> Result<Testo, AppError> {
    if grezzo.len() > massimo {
        return Err(fuori_dai_tetti("un testo più lungo del tetto"));
    }
    // Il fine riga e la tabulazione sì: sono quel che un testo ha. Gli altri
    // controlli, e i comandi di direzione, no.
    if grezzo.chars().any(|c| {
        (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
    }) {
        return Err(fuori_dai_tetti("un testo con dei caratteri di controllo"));
    }
    let letto = testo::leggi(grezzo);
    if letto.righe.len() > MASSIME_RIGHE {
        return Err(fuori_dai_tetti("un testo con troppe righe"));
    }
    let mut parole = 0_usize;
    for riga in &letto.righe {
        parole = parole.saturating_add(riga.parole.len());
        if riga.ms > MASSIMO_TEMPO_MS || riga.parole.iter().any(|p| p.ms > MASSIMO_TEMPO_MS) {
            return Err(fuori_dai_tetti("un testo con un tempo oltre la giornata"));
        }
    }
    if parole > MASSIME_PAROLE {
        return Err(fuori_dai_tetti("un testo con troppe parole"));
    }
    Ok(letto)
}

// ── chi chiede ──────────────────────────────────────────────────────────────

/// Convalida il testo che un altro dispositivo ha dato, e lo legge.
///
/// # Errori
///
/// `lontano.protocollo` per tutto quel che non sta nei tetti: chi chiede lo
/// tratta come «il proprietario non ha niente».
pub fn convalida_ricevuto(ricevuto: TestoCondiviso) -> Result<DalProprietario, AppError> {
    controlla_lo_scarto(ricevuto.scarto_ms)?;
    if ricevuto.sincronizzato.is_some() && ricevuto.piatto.is_some() {
        return Err(fuori_dai_tetti(
            "un testo che è insieme con i tempi e senza",
        ));
    }
    let fonte = Fonte::da_nome(&ricevuto.fonte);
    if ricevuto.strumentale {
        return Ok(DalProprietario {
            testo: Testo {
                strumentale: true,
                ..Testo::default()
            },
            grezzo: String::new(),
            fonte,
            scarto_ms: ricevuto.scarto_ms,
            cercato: ricevuto.cercato,
        });
    }
    let grezzo = ricevuto
        .sincronizzato
        .or(ricevuto.piatto)
        .unwrap_or_default();
    let letto = controlla_il_testo(&grezzo, MASSIMO_TESTO_RICEVUTO)?;
    Ok(DalProprietario {
        testo: letto,
        grezzo,
        fonte,
        scarto_ms: ricevuto.scarto_ms,
        cercato: ricevuto.cercato,
    })
}

/// Le correzioni di questo brano aspettano in posta: finché è così, il testo di
/// qui è quello giusto, e quel che dice il proprietario — che non le ha ancora
/// — non lo scavalca.
///
/// # Errori
///
/// Quelli del database.
pub fn in_attesa(connection: &Connection, remoto: &Remoto) -> Result<bool, AppError> {
    Ok(dispositivi::in_posta(
        connection,
        &remoto.dispositivo,
        POSTA_TESTO,
        &remoto.id_remoto.to_string(),
    )?
    .is_some())
}

/// Mette in posta un gesto dato qui al testo di un brano dell'altro,
/// fondendolo con quelli che già aspettano. `false` se non parte: un testo
/// troppo lungo per una domanda non verrebbe mai preso, e una voce che resta
/// lì ferma tutte le altre.
///
/// # Errori
///
/// Quelli del database.
pub fn metti_in_posta(
    connection: &Connection,
    remoto: &Remoto,
    gesto: &Scrittura,
    adesso_ms: i64,
) -> Result<bool, AppError> {
    if convalida_scrittura(gesto).is_err() {
        return Ok(false);
    }
    let chiave = remoto.id_remoto.to_string();
    let prima = dispositivi::in_posta(connection, &remoto.dispositivo, POSTA_TESTO, &chiave)?
        .and_then(|voce| Correzione::da_corpo(&voce.corpo))
        .unwrap_or_default();
    let corpo = prima.dopo(gesto).in_corpo();
    dispositivi::metti_in_posta(
        connection,
        &remoto.dispositivo,
        POSTA_TESTO,
        &chiave,
        &corpo,
        adesso_ms,
    )?;
    Ok(true)
}

#[cfg(test)]
mod prove {
    use rusqlite::{Connection, params};

    use crate::specchio::comuni::{ADESSO, chiave, giro, libreria, riga as riga_di_catalogo};
    use crate::testi::{
        BranoDaTestare, DalDisco, Fonte, Remoto, brano, componi, componi_senza_scrivere, copertura,
        da_cercare, imposta_scarto, leggi_dal_disco, leggi_riga, per_brano, quanti_da_cercare,
    };

    use super::*;

    const LRC: &str = "[00:01.00]prima riga\n[00:02.00]seconda riga\n";
    const PIATTO: &str = "una riga\nun'altra riga";

    /// Un brano di questa macchina, condiviso, con la chiave `k`.
    fn locale(c: &Connection, id: i64, k: &str, percorso: &str) {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, duration_ms,
                                 file_size, date_added, date_modified)
             VALUES (?1, ?2, ?3, 'Titolo', 'Artista', 'Album', 120000, 1, 0, 0)",
            params![id, percorso, k],
        )
        .expect("brano di qui");
    }

    /// L'id della riga specchio del brano `numero` del dispositivo `lettera`.
    fn specchio(c: &Connection, lettera: char, numero: i64) -> i64 {
        c.query_row(
            "SELECT track_id FROM lontano_brani WHERE dispositivo = ?1 AND id_remoto = ?2",
            params![chiave(lettera), numero],
            |r| r.get(0),
        )
        .expect("riga specchio")
    }

    fn il_testo(sincronizzato: Option<&str>, piatto: Option<&str>) -> TestoCondiviso {
        TestoCondiviso {
            sincronizzato: sincronizzato.map(str::to_owned),
            piatto: piatto.map(str::to_owned),
            strumentale: false,
            fonte: "mano".to_owned(),
            scarto_ms: 0,
            cercato: true,
        }
    }

    fn nelle_righe(c: &Connection, k: &str) -> i64 {
        c.query_row(
            "SELECT COUNT(*) FROM lyrics WHERE track_key = ?1",
            [k],
            |r| r.get(0),
        )
        .expect("conteggio")
    }

    fn codice<T: std::fmt::Debug>(esito: Result<T, rymembra_domain::errors::AppError>) -> String {
        esito
            .expect_err("un errore")
            .code()
            .kind()
            .code()
            .to_owned()
    }

    // ── chi serve: solo i brani condivisi ───────────────────────────────────

    #[test]
    fn un_pari_non_legge_il_testo_di_un_brano_che_non_e_condiviso() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/Musica/1.mp3");
        // Un brano del cloud, un brano di catalogo e una riga specchio: tutti e
        // tre hanno un testo in tabella, e nessuno va dato a un altro.
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                 date_added, date_modified)
             VALUES (2, 'nuvola://drive/Musica/2.mp3', 'k2', 'T', 'A', 'B', 1, 0, 0)",
            [],
        )
        .expect("brano del cloud");
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, source_service,
                                 fonte_url, licenza, disponibilita, date_added)
             VALUES (3, NULL, 'k3', 'T', 'A', 'B', 'jamendo', 'https://x/3', 'cc', 'ok', 0)",
            [],
        )
        .expect("brano di catalogo");
        giro(&c, &chiave('a'), &[riga_di_catalogo(4)], ADESSO);
        let specchio_id = specchio(&c, 'a', 4);
        for k in ["k1", "k2", "k3", "artista|titolo 4|album"] {
            c.execute(
                "INSERT INTO lyrics (track_key, synced, source, updated_at)
                 VALUES (?1, ?2, 'mano', 1)",
                params![k, LRC],
            )
            .expect("testo");
        }

        assert!(brano_condiviso(&c, 1).unwrap().is_some(), "il brano di qui");
        for (id, cosa) in [
            (2, "cloud"),
            (3, "catalogo"),
            (specchio_id, "riga specchio"),
            (999, "che non c'è"),
        ] {
            assert!(
                brano_condiviso(&c, id).unwrap().is_none(),
                "un brano {cosa} non si condivide"
            );
            // E nemmeno si scrive: niente si crea per un brano che non è dato.
            assert_eq!(
                codice(scrivi_dal_dispositivo(&c, id, &Scrittura::Scarto(100))),
                "lontano.fileCambiato",
                "{cosa}"
            );
        }
    }

    #[test]
    fn chi_serve_compone_il_testo_senza_scrivere_niente() {
        let dir = tempfile::tempdir().expect("cartella");
        let file = dir.path().join("1.mp3");
        std::fs::write(&file, b"finto").expect("file");
        std::fs::write(file.with_extension("lrc"), LRC).expect("sidecar");
        let c = libreria();
        locale(&c, 1, "k1", &file.display().to_string());

        let dato = brano_condiviso(&c, 1).unwrap().expect("condiviso");
        let riga = leggi_riga(&c, &dato.track_key);
        let disco = leggi_dal_disco(&dato, riga.as_ref());
        let risposta = testo_da_dare(&dato, riga, disco).expect("ha un testo");
        assert_eq!(risposta.fonte, "sidecar");
        assert_eq!(risposta.sincronizzato.as_deref(), Some(LRC));
        assert_eq!(risposta.piatto, None);
        assert!(!risposta.strumentale);
        // La lettura di chi serve non scrive la cache: è una lettura, e sta
        // fuori dal lucchetto.
        assert_eq!(nelle_righe(&c, "k1"), 0);
    }

    #[test]
    fn senza_niente_chi_serve_non_ha_un_testo_da_dare() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/non/esiste/1.mp3");
        let dato = brano_condiviso(&c, 1).unwrap().expect("condiviso");
        let disco = leggi_dal_disco(&dato, None);
        assert_eq!(testo_da_dare(&dato, None, disco), None);
    }

    #[test]
    fn uno_strumentale_si_da_come_una_risposta() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/non/esiste/1.mp3");
        c.execute(
            "INSERT INTO lyrics (track_key, source, instrumental, offset_ms, updated_at)
             VALUES ('k1', 'lrclib', 1, 0, 1)",
            [],
        )
        .expect("strumentale");
        let dato = brano_condiviso(&c, 1).unwrap().expect("condiviso");
        let riga = leggi_riga(&c, "k1");
        let disco = leggi_dal_disco(&dato, riga.as_ref());
        let risposta = testo_da_dare(&dato, riga, disco).expect("una risposta");
        assert!(risposta.strumentale);
        assert_eq!(risposta.sincronizzato, None);
        assert_eq!(risposta.piatto, None);
    }

    #[test]
    fn il_testo_piatto_e_lo_scarto_viaggiano_con_la_loro_fonte() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/non/esiste/1.mp3");
        c.execute(
            "INSERT INTO lyrics (track_key, plain, source, offset_ms, updated_at, checked_at)
             VALUES ('k1', ?1, 'lrclib', -150, 1, 5)",
            [PIATTO],
        )
        .expect("testo piatto");
        let dato = brano_condiviso(&c, 1).unwrap().expect("condiviso");
        let riga = leggi_riga(&c, "k1");
        let disco = leggi_dal_disco(&dato, riga.as_ref());
        let risposta = testo_da_dare(&dato, riga, disco).expect("una risposta");
        assert_eq!(risposta.piatto.as_deref(), Some(PIATTO));
        assert_eq!(risposta.sincronizzato, None);
        assert_eq!(risposta.fonte, "lrclib");
        assert_eq!(risposta.scarto_ms, -150);
        assert!(risposta.cercato);
    }

    // ── chi riceve: convalida tutto ─────────────────────────────────────────

    #[test]
    fn un_testo_ricevuto_dentro_i_tetti_passa_e_si_legge() {
        let dal = convalida_ricevuto(il_testo(Some(LRC), None)).expect("buono");
        assert!(dal.testo.sincronizzato());
        assert_eq!(dal.testo.righe.len(), 2);
        assert_eq!(dal.fonte, Fonte::Mano);
        let dal = convalida_ricevuto(il_testo(None, Some(PIATTO))).expect("piatto");
        assert!(!dal.testo.sincronizzato());
        assert_eq!(dal.testo.piatto.as_deref(), Some(PIATTO));
        // Uno strumentale non porta testo, e se lo porta si butta.
        let mut strumentale = il_testo(None, Some(PIATTO));
        strumentale.strumentale = true;
        let dal = convalida_ricevuto(strumentale).expect("strumentale");
        assert!(dal.testo.strumentale);
        assert!(dal.testo.righe.is_empty() && dal.testo.piatto.is_none());
    }

    #[test]
    fn un_testo_ricevuto_fuori_dai_tetti_si_scarta_per_intero() {
        let troppo = "a".repeat(MASSIMO_TESTO_RICEVUTO + 1);
        let molte_righe: String = (0..=MASSIME_RIGHE)
            .map(|n| format!("[{:02}:{:02}.00]x\n", n.div_euclid(60) % 99, n % 60))
            .collect();
        let tempo_enorme = "[9999:00.00]un tempo che non sta in una canzone\n".to_owned();
        let mut scarto = il_testo(Some(LRC), None);
        scarto.scarto_ms = MASSIMO_SCARTO_MS + 1;
        let con_un_comando = "[00:01.00]una riga\u{0}con un controllo\n";
        for (cosa, cattivo) in [
            ("troppo lungo", il_testo(None, Some(troppo.as_str()))),
            ("troppe righe", il_testo(Some(molte_righe.as_str()), None)),
            (
                "un tempo oltre la giornata",
                il_testo(Some(tempo_enorme.as_str()), None),
            ),
            ("uno scarto enorme", scarto),
            (
                "un carattere di controllo",
                il_testo(Some(con_un_comando), None),
            ),
            ("sia l'LRC sia il piatto", il_testo(Some(LRC), Some(PIATTO))),
        ] {
            assert_eq!(
                codice(convalida_ricevuto(cattivo)),
                "lontano.protocollo",
                "{cosa}"
            );
        }
    }

    // ── chi serve: le correzioni di un altro dispositivo ────────────────────

    #[test]
    fn una_correzione_ricevuta_passa_per_la_logica_dei_comandi() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/non/esiste/1.mp3");

        // Un testo a mano: la riga, azzera lo scarto.
        imposta_scarto(&c, "k1", 300).unwrap();
        scrivi_dal_dispositivo(
            &c,
            1,
            &Scrittura::Testo(NuovoTesto::Mano {
                lrc: LRC.to_owned(),
            }),
        )
        .unwrap();
        let r = leggi_riga(&c, "k1").expect("riga");
        assert_eq!(r.fonte, Fonte::Mano);
        assert_eq!(r.synced.as_deref(), Some(LRC));
        assert_eq!(r.scarto_ms, 0, "come quando lo si fa dove sta il brano");

        // Lo scarto.
        scrivi_dal_dispositivo(&c, 1, &Scrittura::Scarto(-250)).unwrap();
        assert_eq!(leggi_riga(&c, "k1").unwrap().scarto_ms, -250);
        // Due volte è lo stesso: dice come deve restare, non cosa aggiungere.
        scrivi_dal_dispositivo(&c, 1, &Scrittura::Scarto(-250)).unwrap();
        assert_eq!(leggi_riga(&c, "k1").unwrap().scarto_ms, -250);
    }

    #[test]
    fn una_voce_scelta_da_un_altro_dispositivo_si_scrive_come_scelta_e_si_rifiuta_come_in_locale() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/non/esiste/1.mp3");
        scrivi_dal_dispositivo(
            &c,
            1,
            &Scrittura::Testo(NuovoTesto::Scelta {
                id_catalogo: 4242,
                piatto: None,
                sincronizzato: Some(LRC.to_owned()),
                strumentale: false,
            }),
        )
        .unwrap();
        let r = leggi_riga(&c, "k1").expect("riga");
        assert_eq!(r.fonte, Fonte::Lrclib);
        assert_eq!(r.synced.as_deref(), Some(LRC));
        let scelta: i64 = c
            .query_row(
                "SELECT scelta FROM lyrics WHERE track_key = 'k1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(scelta, 1, "una scelta fatta a mano resta sua");

        // «Non è questo»: butta il testo del catalogo e non torna.
        scrivi_dal_dispositivo(&c, 1, &Scrittura::Testo(NuovoTesto::Rifiuta)).unwrap();
        let r = leggi_riga(&c, "k1").expect("riga");
        assert_eq!(r.synced, None);
        let scartati: String = c
            .query_row(
                "SELECT scartati FROM lyrics WHERE track_key = 'k1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(scartati.contains("4242"), "{scartati}");
    }

    #[test]
    fn una_correzione_fuori_dai_tetti_non_scrive_niente() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/non/esiste/1.mp3");
        let lunga = format!("[00:01.00]{}\n", "a".repeat(MASSIMO_TESTO));
        let piatta = Scrittura::Testo(NuovoTesto::Mano {
            lrc: PIATTO.to_owned(),
        });
        let vuota = Scrittura::Testo(NuovoTesto::Mano {
            lrc: "  ".to_owned(),
        });
        for (cosa, cattiva) in [
            ("uno scarto enorme", Scrittura::Scarto(i32::MAX)),
            (
                "un testo a mano troppo lungo",
                Scrittura::Testo(NuovoTesto::Mano { lrc: lunga }),
            ),
            ("un testo a mano senza tempi", piatta),
            ("un testo a mano vuoto", vuota),
            (
                "una voce senza identificativo",
                Scrittura::Testo(NuovoTesto::Scelta {
                    id_catalogo: 0,
                    piatto: Some(PIATTO.to_owned()),
                    sincronizzato: None,
                    strumentale: false,
                }),
            ),
            (
                "una voce senza niente dentro",
                Scrittura::Testo(NuovoTesto::Scelta {
                    id_catalogo: 5,
                    piatto: None,
                    sincronizzato: None,
                    strumentale: false,
                }),
            ),
        ] {
            assert_eq!(
                codice(scrivi_dal_dispositivo(&c, 1, &cattiva)),
                "lontano.protocollo",
                "{cosa}"
            );
        }
        assert_eq!(nelle_righe(&c, "k1"), 0, "niente è stato scritto");
    }

    // ── chi chiede: dove il testo del proprietario sta nella catena ──────────

    /// Una riga specchio e il suo brano, come lo legge il pannello.
    fn il_brano_specchio(c: &Connection) -> BranoDaTestare {
        giro(c, &chiave('a'), &[riga_di_catalogo(4)], ADESSO);
        brano(c, specchio(c, 'a', 4)).expect("brano")
    }

    #[test]
    fn una_riga_specchio_non_ha_un_file_e_dice_di_chi_e() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        // Niente percorso: non c'è un `.lrc` accanto né un tag da aprire, e
        // nessuno ci scrive un sidecar. Chi è il proprietario sta a parte.
        assert_eq!(b.path, None);
        assert_eq!(
            b.remoto,
            Some(Remoto {
                dispositivo: chiave('a'),
                id_remoto: 4
            })
        );
        let disco = leggi_dal_disco(&b, None);
        assert!(disco.sidecar.is_none() && disco.tag.is_none() && disco.proprietario.is_none());
    }

    fn dal_proprietario(testo: &TestoCondiviso) -> DalProprietario {
        convalida_ricevuto(testo.clone()).expect("buono")
    }

    #[test]
    fn il_testo_del_proprietario_viene_prima_della_catena_e_si_ricorda() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let mut t = il_testo(Some(LRC), None);
        t.fonte = "sidecar".to_owned();
        t.scarto_ms = 120;
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&t)),
            ..DalDisco::default()
        };
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        assert_eq!(esito.fonte, Fonte::Sidecar);
        assert_eq!(esito.scarto_ms, 120);
        assert!(esito.testo.sincronizzato());
        assert!(!esito.da_chiedere);
        // È rimasto qui come copia, anche per la copia di qui dello stesso brano
        // e per quando il PC è spento. `checked_at` no: quello dice che si è
        // chiesto al catalogo da qui, e non è successo.
        let r = leggi_riga(&c, &b.track_key).expect("copia");
        assert_eq!(r.synced.as_deref(), Some(LRC));
        assert_eq!(r.fonte, Fonte::Sidecar);
        assert_eq!(r.scarto_ms, 120);
        assert!(!r.cercato);
        // La cache non si riscrive se non è cambiato niente: ogni apertura del
        // pannello non deve far sembrare nuova una riga che viaggia nel profilo.
        c.execute("UPDATE lyrics SET updated_at = 7", []).unwrap();
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&t)),
            ..DalDisco::default()
        };
        let _ = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        assert_eq!(leggi_riga(&c, &b.track_key).unwrap().aggiornata_ms, 7);
    }

    #[test]
    fn senza_scrivere_non_si_ricorda_niente() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&il_testo(Some(LRC), None))),
            ..DalDisco::default()
        };
        let esito = componi_senza_scrivere(&b, None, disco);
        assert!(esito.testo.sincronizzato());
        assert!(leggi_riga(&c, &b.track_key).is_none());
    }

    #[test]
    fn il_testo_a_mano_di_qui_non_si_scavalca_nemmeno_dal_proprietario() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        c.execute(
            "INSERT INTO lyrics (track_key, synced, source, offset_ms, updated_at, checked_at)
             VALUES (?1, '[00:09.00]fatta qui\n', 'mano', 40, 1, 1)",
            [&b.track_key],
        )
        .unwrap();
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&il_testo(Some(LRC), None))),
            ..DalDisco::default()
        };
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        assert_eq!(esito.fonte, Fonte::Mano);
        assert_eq!(esito.testo.righe.first().map(|r| r.ms), Some(9_000));
        assert_eq!(esito.scarto_ms, 40);
        let r = leggi_riga(&c, &b.track_key).unwrap();
        assert_eq!(r.synced.as_deref(), Some("[00:09.00]fatta qui\n"));
    }

    #[test]
    fn un_testo_piatto_del_proprietario_non_scavalca_quello_che_scorre() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        // Qui il catalogo ha dato un testo con i tempi; il proprietario ha solo
        // le parole: come un sidecar senza tempi, non gli passa davanti.
        c.execute(
            "INSERT INTO lyrics (track_key, synced, source, updated_at, checked_at)
             VALUES (?1, ?2, 'lrclib', 1, 1)",
            params![&b.track_key, LRC],
        )
        .unwrap();
        let mut t = il_testo(None, Some(PIATTO));
        t.cercato = false;
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&t)),
            ..DalDisco::default()
        };
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        assert!(esito.testo.sincronizzato());
        assert_eq!(esito.fonte, Fonte::Lrclib);
        // Ma uno che scorre del proprietario sì, anche sopra uno di qui.
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&il_testo(
                Some("[00:03.00]del proprietario\n"),
                None,
            ))),
            ..DalDisco::default()
        };
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        assert_eq!(esito.testo.righe.first().map(|r| r.ms), Some(3_000));
    }

    #[test]
    fn un_testo_piatto_del_proprietario_resta_da_chiedere_se_non_si_e_cercato() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let mut t = il_testo(None, Some(PIATTO));
        t.cercato = false;
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&t)),
            ..DalDisco::default()
        };
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        // Nessuno ha cercato i tempi: il pannello li cerca da qui, con le regole
        // di sempre.
        assert!(esito.da_chiedere);
        // E quando si è cercato — dal proprietario o da qui — non si richiede.
        c.execute(
            "UPDATE lyrics SET checked_at = 5 WHERE track_key = ?1",
            [&b.track_key],
        )
        .unwrap();
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&t)),
            ..DalDisco::default()
        };
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        assert!(!esito.da_chiedere);
    }

    #[test]
    fn un_proprietario_senza_niente_lascia_la_catena_di_qui() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        c.execute(
            "INSERT INTO lyrics (track_key, synced, source, updated_at, checked_at)
             VALUES (?1, ?2, 'lrclib', 1, 1)",
            params![&b.track_key, LRC],
        )
        .unwrap();
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), DalDisco::default());
        assert_eq!(esito.fonte, Fonte::Lrclib);
        assert!(esito.testo.sincronizzato());
        // E la lettura per intero, come la fa il pannello.
        let intero = per_brano(&c, b.id).expect("testo");
        assert_eq!(intero.fonte, Fonte::Lrclib);
    }

    #[test]
    fn uno_strumentale_del_proprietario_vale_come_risposta() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let mut t = il_testo(None, None);
        t.strumentale = true;
        let disco = DalDisco {
            proprietario: Some(dal_proprietario(&t)),
            ..DalDisco::default()
        };
        let esito = componi(&c, &b, leggi_riga(&c, &b.track_key), disco);
        assert!(esito.testo.strumentale);
        assert!(!esito.da_chiedere);
        let r = leggi_riga(&c, &b.track_key).expect("copia");
        assert!(r.strumentale);
    }

    // ── la passata di sottofondo non prende i brani di un altro ─────────────

    #[test]
    fn la_passata_non_cerca_i_testi_dei_brani_di_un_altro_dispositivo() {
        let c = libreria();
        // Una riga specchio con un id più basso di una copia di qui dello
        // stesso brano: `MIN(id)` per chiave ha scelto lei, e il lotto avrebbe
        // mandato titolo e artista del PC a LRCLIB senza che nessuno avesse
        // aperto il pannello.
        giro(
            &c,
            &chiave('a'),
            &[riga_di_catalogo(1), riga_di_catalogo(2)],
            ADESSO,
        );
        let chiave_comune = riga_di_catalogo(1).track_key;
        locale(&c, 500, &chiave_comune, "C:/Musica/1.mp3");

        let coda = da_cercare(&c, ADESSO, 50).expect("coda");
        let ids: Vec<i64> = coda.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![500], "solo la copia di qui: {ids:?}");
        assert_eq!(quanti_da_cercare(&c, ADESSO).unwrap(), 1);
        // E la copertura conta la libreria di qui.
        let cop = copertura(&c).unwrap();
        assert_eq!(
            cop.mancanti + cop.sincronizzati + cop.piatti + cop.strumentali,
            1,
            "{cop:?}"
        );
    }

    // ── la posta: le correzioni che aspettano il proprietario ────────────────

    fn pc() -> Remoto {
        Remoto {
            dispositivo: chiave('a'),
            id_remoto: 4,
        }
    }

    fn in_posta(c: &Connection) -> Correzione {
        let voce =
            crate::dispositivi::in_posta(c, &chiave('a'), crate::dispositivi::POSTA_TESTO, "4")
                .unwrap()
                .expect("una voce");
        Correzione::da_corpo(&voce.corpo).expect("leggibile")
    }

    #[test]
    fn le_correzioni_in_posta_si_fondono_in_una_voce_sola_per_brano() {
        let c = libreria();
        assert!(!in_attesa(&c, &pc()).unwrap());
        assert!(metti_in_posta(&c, &pc(), &Scrittura::Scarto(200), ADESSO).unwrap());
        assert!(in_attesa(&c, &pc()).unwrap());
        assert_eq!(
            in_posta(&c),
            Correzione {
                scarto_ms: Some(200),
                testo: None
            }
        );

        // Un testo a mano dopo lo scarto: lo azzera, come in locale.
        let mano = NuovoTesto::Mano {
            lrc: LRC.to_owned(),
        };
        metti_in_posta(&c, &pc(), &Scrittura::Testo(mano.clone()), ADESSO + 1).unwrap();
        assert_eq!(
            in_posta(&c),
            Correzione {
                scarto_ms: Some(0),
                testo: Some(mano.clone())
            }
        );
        // Uno scarto dopo il testo: si applica dopo di lui.
        metti_in_posta(&c, &pc(), &Scrittura::Scarto(-50), ADESSO + 2).unwrap();
        assert_eq!(
            in_posta(&c),
            Correzione {
                scarto_ms: Some(-50),
                testo: Some(mano)
            }
        );
        // Una voce scelta sostituisce il testo, ma non tocca lo scarto.
        let scelta = NuovoTesto::Scelta {
            id_catalogo: 9,
            piatto: Some(PIATTO.to_owned()),
            sincronizzato: None,
            strumentale: false,
        };
        metti_in_posta(&c, &pc(), &Scrittura::Testo(scelta.clone()), ADESSO + 3).unwrap();
        assert_eq!(
            in_posta(&c),
            Correzione {
                scarto_ms: Some(-50),
                testo: Some(scelta)
            }
        );
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM dispositivi_posta", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn la_posta_manda_prima_il_testo_e_poi_lo_scarto() {
        let c = Correzione {
            scarto_ms: Some(30),
            testo: Some(NuovoTesto::Rifiuta),
        };
        assert_eq!(
            c.scritture(),
            vec![Scrittura::Testo(NuovoTesto::Rifiuta), Scrittura::Scarto(30)]
        );
        assert!(Correzione::default().scritture().is_empty());
    }

    #[test]
    fn un_testo_troppo_lungo_non_entra_in_posta() {
        let c = libreria();
        let lunga = format!("[00:01.00]{}\n", "a".repeat(MASSIMO_TESTO));
        let esito = metti_in_posta(
            &c,
            &pc(),
            &Scrittura::Testo(NuovoTesto::Mano { lrc: lunga }),
            ADESSO,
        )
        .unwrap();
        assert!(!esito, "non parte: il proprietario non la prenderebbe");
        assert!(!in_attesa(&c, &pc()).unwrap());
    }
}
