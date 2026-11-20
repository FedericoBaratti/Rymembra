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
    BranoDaTestare, COLONNE, DalDisco, Fonte, Riga, TestoBrano, brano_da_riga, cerca_riga,
    componi_con, copia, imposta_scarto_alle, registra_a_mano_alle, ricorda_scelta, rifiuta,
};
use crate::dispositivi::{self, POSTA_TESTO};
use crate::library::condivisa::CONDIVISO;
use crate::library::db_error;

/// Quanti byte, al più, ha il testo di una correzione che parte da qui. Sta
/// nella cornice di una domanda con margine: vedi il `//!`.
pub const MASSIMO_TESTO: usize = 48_000;

/// Quanti byte, al più, ha un testo che arriva da un altro dispositivo. Una
/// risposta può essere più lunga di una domanda (il suo tetto è quello della
/// cornice), ma un testo di canzone non arriva a un quarto di megabyte: è lo
/// stesso tetto di ogni testo che si legge da fuori
/// ([`rymembra_domain::testo::MASSIMO_TESTO_BYTE`]).
pub const MASSIMO_TESTO_RICEVUTO: usize = rymembra_domain::testo::MASSIMO_TESTO_BYTE;

/// Quante righe con un tempo, al più, ha un testo.
pub const MASSIME_RIGHE: usize = 4_000;

/// Quante parole con un tempo, al più, ha un testo.
pub const MASSIME_PAROLE: usize = 40_000;

/// Il tempo più lontano che una riga o una parola può avere: dodici ore.
pub const MASSIMO_TEMPO_MS: u32 = 12 * 3_600 * 1_000;

/// Lo scarto più lontano che si accetta, in ms: un'ora. Lo stesso tetto del
/// `[offset:]` che un LRC dichiara
/// ([`rymembra_domain::testo::MASSIMO_OFFSET_MS`]): quello che il lettore trova
/// dentro il testo oltre un'ora vale zero, e non serve ripeterlo qui.
pub const MASSIMO_SCARTO_MS: i32 = rymembra_domain::testo::MASSIMO_OFFSET_MS;

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

/// Quanti gesti sul testo, al più, aspettano in posta per un brano.
pub const MASSIMI_GESTI_IN_POSTA: usize = 6;

/// Quel che aspetta in posta per un brano: l'ultimo scarto e i gesti sul testo,
/// fusi in modo che il proprietario finisca com'è finita la copia di qui.
///
/// # Lo scarto
///
/// Si applica **dopo** i testi: un testo a mano azzera lo scarto (le sue battute
/// nascono nello spazio della posizione grezza), quindi uno scarto dato dopo di
/// lui deve vincere, e uno dato prima deve sparire. La fusione lo sa: un testo a
/// mano porta con sé uno scarto a zero.
///
/// # I gesti sul testo, in ordine
///
/// Non vale «l'ultimo vince»: scegliere una voce e poi dire «non è questo»
/// butta **quella voce** — il proprietario la mette fra gli scartati — mentre il
/// solo «non è questo» butterebbe il testo che il proprietario ha di suo. Quindi
/// i gesti restano in fila, e si semplifica solo dove il risultato è lo stesso:
///
/// - un testo a mano rifà tutto, e prende il posto dei gesti di prima;
/// - due scelte, o due rifiuti, uno dopo l'altro valgono l'ultimo.
///
/// Oltre [`MASSIMI_GESTI_IN_POSTA`] si lasciano i più vecchi.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Correzione {
    /// Lo scarto da mettere, se è cambiato.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scarto_ms: Option<i32>,
    /// I gesti sul testo, nell'ordine in cui sono stati dati.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub testi: Vec<NuovoTesto>,
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
                testi: vec![testo.clone()],
            },
            Scrittura::Testo(testo) => {
                let mut testi = self.testi;
                let stesso_gesto = testi.last().is_some_and(|ultimo| {
                    std::mem::discriminant(ultimo) == std::mem::discriminant(testo)
                });
                if stesso_gesto {
                    testi.pop();
                }
                testi.push(testo.clone());
                if testi.len() > MASSIMI_GESTI_IN_POSTA {
                    testi.remove(0);
                }
                Self { testi, ..self }
            }
        }
    }

    /// Le scritture da mandare, nell'ordine in cui vanno applicate: i gesti sul
    /// testo, poi lo scarto.
    #[must_use]
    pub fn scritture(&self) -> Vec<Scrittura> {
        self.testi
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

/// Chi scrive una correzione a un brano di qui, e quando l'ha decisa.
#[derive(Debug, Clone, Copy)]
pub struct Scrivente<'a> {
    /// La chiave del dispositivo che manda: per il tetto totale.
    pub dispositivo: &'a str,
    /// L'orologio di chi manda, quando ha deciso la correzione, in millisecondi.
    /// Zero se non lo dice (una build di prima): vale come «adesso».
    pub quando_ms: i64,
    /// Adesso, sull'orologio di chi risponde.
    pub adesso_ms: i64,
}

impl Scrivente<'_> {
    /// L'istante con cui la correzione si confronta con il testo che c'è.
    ///
    /// Quando l'ha deciso chi la manda, e mai più avanti di adesso: un orologio
    /// avanti non fissa un testo che nessuna correzione vera scavalcherebbe,
    /// come per `Valuta`. Senza orologio, adesso: vince l'ultima arrivata, che è
    /// com'è sempre stato — per quelle di una build di prima non si può sapere di
    /// più, e **non** si trattano come «le più vecchie»: una correzione dal
    /// telefono con la build di oggi non deve sparire in silenzio.
    #[must_use]
    pub const fn quando(&self) -> i64 {
        if self.quando_ms <= 0 || self.quando_ms > self.adesso_ms {
            self.adesso_ms
        } else {
            self.quando_ms
        }
    }
}

/// Applica una scrittura arrivata da un altro dispositivo a un brano di qui.
///
/// La convalida va prima di ogni scrittura, e il brano deve essere condiviso:
/// un brano che non lo è risponde `lontano.fileCambiato`, come per le altre
/// scritture. Le funzioni sono quelle dei comandi di sempre; nessun file accanto
/// al brano si tocca.
///
/// # La più recente vince, non l'ultima arrivata
///
/// Un testo sincronizzato a mano (`source = 'mano'`) ha come orologio
/// `updated_at`, e una correzione arrivata da un altro dispositivo lo scavalca
/// solo se è **più recente**: una correzione decisa prima e consegnata dopo —
/// per giorni, dalla posta di un telefono senza rete — non cancella il lavoro
/// fatto qui nel frattempo. Una scrittura vecchia non è un errore: si risponde
/// `Fatto` e non si scrive, o resterebbe in posta per trenta giorni. La riga
/// ricorda l'orologio **di chi ha deciso** e non l'ora di arrivo, altrimenti due
/// correzioni consegnate in ordine inverso si scavalcherebbero a vicenda.
///
/// Vale per il testo a mano e per lo scarto, e solo contro un testo a mano: quel
/// che viene dal catalogo, da un file o dal tag non ha un orologio di chi lo
/// decide, e la voce scelta e il rifiuto non toccano un testo a mano.
///
/// # Quanto un dispositivo può scrivere, e cosa resta di quel che sostituisce
///
/// Un testo — a mano o una voce scelta — si conta contro
/// [`dispositivi::TESTI_AL_GIORNO`] per dispositivo; oltre, `lontano.irraggiungibile`
/// e chi manda lo tiene in posta. Quando un testo a mano di qui viene
/// sostituito, quello di prima resta come copia di un livello
/// ([`super::copia`]) e si ripristina con un gesto.
///
/// # Errori
///
/// `lontano.fileCambiato` per un brano che non è condiviso; `lontano.protocollo`
/// per una scrittura fuori dai tetti; `lontano.irraggiungibile` oltre il tetto
/// dei testi di un dispositivo; `db.queryFailed` se il database non risponde.
pub fn scrivi_dal_dispositivo(
    connection: &Connection,
    id: i64,
    scrittura: &Scrittura,
    scrivente: &Scrivente<'_>,
) -> Result<(), AppError> {
    // La convalida prima di tutto, anche prima di guardare il brano: una
    // scrittura storta si rifiuta con lo stesso codice per ogni brano.
    let voce = convalida_scrittura(scrittura)?;
    let Some(brano) = brano_condiviso(connection, id)? else {
        return Err(AppError::new(ErrorCode::LontanoFileCambiato)
            .with_cause("il brano non è più condiviso"));
    };
    let quando = scrivente.quando();
    // Il confronto degli orologi, il tetto, la copia e la scrittura sono un
    // gesto solo: o tutti o niente. Uscire prima senza `commit` li annulla.
    let gesto = connection
        .unchecked_transaction()
        .map_err(|err| db_error("scrittura di un testo da un dispositivo", &err))?;
    let attuale = cerca_riga(&gesto, &brano.track_key)?;
    let a_mano = attuale
        .as_ref()
        .filter(|riga| riga.fonte.e_di_chi_ascolta());
    match scrittura {
        Scrittura::Scarto(scarto) => {
            // Uno scarto deciso prima dell'ultimo tocco al testo a mano non lo
            // sposta: lo ha già scavalcato chi l'ha toccato dopo.
            if a_mano.is_some_and(|riga| riga.aggiornata_ms > quando) {
                return Ok(());
            }
            imposta_scarto_alle(&gesto, &brano.track_key, *scarto, quando)?;
        }
        Scrittura::Testo(NuovoTesto::Mano { lrc }) => {
            // Strettamente più recente: la stessa correzione consegnata due
            // volte è la stessa, e la seconda non riscrive niente.
            if a_mano.is_some_and(|riga| riga.aggiornata_ms >= quando) {
                return Ok(());
            }
            ammetti(&gesto, scrivente, brano.id, quando)?;
            if let Some(prima) = a_mano
                .filter(|riga| riga.ha_i_tempi() && riga.synced.as_deref() != Some(lrc.as_str()))
            {
                copia::tieni(&gesto, &brano.track_key, prima, scrivente.adesso_ms)?;
            }
            registra_a_mano_alle(&gesto, &brano, lrc, quando)?;
        }
        Scrittura::Testo(NuovoTesto::Scelta { .. }) => {
            let voce = voce.ok_or_else(|| fuori_dai_tetti("una voce scelta senza voce"))?;
            ammetti(&gesto, scrivente, brano.id, quando)?;
            ricorda_scelta(&gesto, &brano, &voce)?;
        }
        Scrittura::Testo(NuovoTesto::Rifiuta) => rifiuta(&gesto, &brano.track_key)?,
    }
    gesto
        .commit()
        .map_err(|err| db_error("scrittura di un testo da un dispositivo", &err))
}

/// Il dispositivo può scrivere un testo in più? Altrimenti
/// `lontano.irraggiungibile`: chi manda lo tiene e riprova.
fn ammetti(
    connection: &Connection,
    scrivente: &Scrivente<'_>,
    brano: i64,
    quando_ms: i64,
) -> Result<(), AppError> {
    if dispositivi::testo_ammesso(
        connection,
        scrivente.dispositivo,
        brano,
        quando_ms,
        scrivente.adesso_ms,
    )? {
        return Ok(());
    }
    Err(AppError::new(ErrorCode::LontanoIrraggiungibile)
        .with_cause("troppi testi da questo dispositivo nelle ultime ventiquattr'ore"))
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
    // «Sincronizzato a mano» dal proprietario non vale come sincronizzato a mano
    // **qui**: per chi lo riceve è la copia di un altro, che lui può aggiornare.
    // Il `mano` di qui è lavoro di chi ascolta, e non lo scavalca nessuno: se lo
    // dichiarasse un pari, un testo qualunque bloccherebbe la riga.
    let fonte = match Fonte::da_nome(&ricevuto.fonte) {
        Fonte::Mano => Fonte::Proprietario,
        altra => altra,
    };
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

/// Cosa fare della posta dopo un gesto dato qui al testo di un brano.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Posta {
    /// Il gesto è in posta per questo dispositivo: va svegliato chi la manda.
    Mandare(String),
    /// Non c'è niente da mandare: il brano è di questa macchina, o il gesto
    /// non ha cambiato niente qui.
    Niente,
    /// Il gesto non parte: è troppo lungo per una domanda.
    TroppoLungo,
}

/// Mette in posta, se serve, un gesto che si è **appena** scritto nella copia di
/// qui del testo di un brano.
///
/// Un brano di questa macchina non ha un proprietario a cui scrivere. E una
/// voce scelta o un «non è questo» che sulla copia di qui non hanno cambiato
/// niente — il testo è a mano, e le due scritture non lo toccano mai — non
/// partono: dall'altra parte cambierebbero un testo che chi ha dato il gesto non
/// ha visto cambiare, e in posta prenderebbero il posto di un testo a mano che
/// aspetta il suo turno. Per saperlo guarda l'ultima scrittura della connessione
/// (`changes`): va chiamata subito dopo di lei.
///
/// # Errori
///
/// Quelli del database.
pub fn dopo_un_gesto(
    connection: &Connection,
    brano: &BranoDaTestare,
    gesto: &Scrittura,
    adesso_ms: i64,
) -> Result<Posta, AppError> {
    let Some(remoto) = &brano.remoto else {
        return Ok(Posta::Niente);
    };
    let la_copia_dice_di_si = !matches!(
        gesto,
        Scrittura::Testo(NuovoTesto::Scelta { .. } | NuovoTesto::Rifiuta)
    ) || connection.changes() > 0;
    if !la_copia_dice_di_si {
        return Ok(Posta::Niente);
    }
    if metti_in_posta(connection, remoto, gesto, adesso_ms)? {
        Ok(Posta::Mandare(remoto.dispositivo.clone()))
    } else {
        Ok(Posta::TroppoLungo)
    }
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
        ricorda_scelta, rifiuta,
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

    /// Il dispositivo che scrive, nelle prove: il `b` di sempre.
    fn mittente() -> String {
        chiave('b')
    }

    /// Una scrittura senza orologio (come la mandava una build di prima), da
    /// `mittente`, adesso. Ha il nome della funzione che prova, e la nasconde:
    /// le prove che hanno bisogno di un orologio o di un altro dispositivo
    /// chiamano `super::scrivi_dal_dispositivo` con il loro [`Scrivente`].
    fn scrivi_dal_dispositivo(
        c: &Connection,
        id: i64,
        scrittura: &Scrittura,
    ) -> Result<(), rymembra_domain::errors::AppError> {
        super::scrivi_dal_dispositivo(
            c,
            id,
            scrittura,
            &Scrivente {
                dispositivo: &mittente(),
                quando_ms: 0,
                adesso_ms: ADESSO,
            },
        )
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
        // «Mano» dichiarato dal proprietario è, per chi lo riceve, la sua copia.
        assert_eq!(dal.fonte, Fonte::Proprietario);
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
                testi: vec![]
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
                testi: vec![mano.clone()]
            }
        );
        // Uno scarto dopo il testo: si applica dopo di lui.
        metti_in_posta(&c, &pc(), &Scrittura::Scarto(-50), ADESSO + 2).unwrap();
        assert_eq!(
            in_posta(&c),
            Correzione {
                scarto_ms: Some(-50),
                testi: vec![mano.clone()]
            }
        );
        // Una voce scelta dopo il testo a mano sta in fila dopo di lui, e non
        // tocca lo scarto.
        let scelta = voce_scelta(9);
        metti_in_posta(&c, &pc(), &Scrittura::Testo(scelta.clone()), ADESSO + 3).unwrap();
        assert_eq!(
            in_posta(&c),
            Correzione {
                scarto_ms: Some(-50),
                testi: vec![mano, scelta]
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
            testi: vec![NuovoTesto::Rifiuta],
        };
        assert_eq!(
            c.scritture(),
            vec![Scrittura::Testo(NuovoTesto::Rifiuta), Scrittura::Scarto(30)]
        );
        assert!(Correzione::default().scritture().is_empty());
    }

    fn voce_scelta(id_catalogo: i64) -> NuovoTesto {
        NuovoTesto::Scelta {
            id_catalogo,
            piatto: Some(PIATTO.to_owned()),
            sincronizzato: None,
            strumentale: false,
        }
    }

    #[test]
    fn scegliere_e_poi_dire_non_e_questo_lascia_al_proprietario_lo_stesso_stato_di_qui() {
        // La voce scelta e poi rifiutata finisce fra gli scartati: se in posta
        // restasse solo «non è questo», il proprietario butterebbe il testo che
        // ha di suo, e la voce scelta non sarebbe bandita. Si prova sulle due
        // librerie: una con i gesti dati in fila come qui, una con la posta
        // consegnata dopo.
        let qui = libreria();
        locale(&qui, 1, "k1", "C:/non/esiste/1.mp3");
        let proprietario = libreria();
        locale(&proprietario, 1, "k1", "C:/non/esiste/1.mp3");
        for libreria in [&qui, &proprietario] {
            // Il testo di partenza, che il proprietario aveva dal catalogo.
            libreria
                .execute(
                    "INSERT INTO lyrics (track_key, plain, source, lrclib_id, updated_at)
                     VALUES ('k1', 'il suo testo', 'lrclib', 111, 1)",
                    [],
                )
                .unwrap();
        }
        let gesti = [
            Scrittura::Testo(voce_scelta(222)),
            Scrittura::Testo(NuovoTesto::Rifiuta),
        ];
        // Qui, in fila.
        for gesto in &gesti {
            scrivi_dal_dispositivo(&qui, 1, gesto).unwrap();
        }
        // Dalla posta: fusi, e poi consegnati nell'ordine.
        let mut posta = Correzione::default();
        for gesto in &gesti {
            posta = posta.dopo(gesto);
        }
        assert_eq!(posta.testi.len(), 2, "restano in fila: {posta:?}");
        for scrittura in posta.scritture() {
            scrivi_dal_dispositivo(&proprietario, 1, &scrittura).unwrap();
        }
        let stato = |c: &Connection| -> (Option<String>, Option<String>, Option<i64>) {
            c.query_row(
                "SELECT plain, scartati, lrclib_id FROM lyrics WHERE track_key = 'k1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!(stato(&proprietario), stato(&qui));
        let (testo, scartati, _) = stato(&proprietario);
        assert_eq!(testo, None);
        assert!(scartati.unwrap_or_default().contains("222"));
    }

    #[test]
    fn un_testo_a_mano_prende_il_posto_dei_gesti_di_prima_e_gesti_uguali_valgono_l_ultimo() {
        let mano = NuovoTesto::Mano {
            lrc: LRC.to_owned(),
        };
        let c = Correzione::default()
            .dopo(&Scrittura::Testo(voce_scelta(1)))
            .dopo(&Scrittura::Testo(voce_scelta(2)))
            .dopo(&Scrittura::Testo(NuovoTesto::Rifiuta))
            .dopo(&Scrittura::Testo(NuovoTesto::Rifiuta));
        assert_eq!(c.testi, vec![voce_scelta(2), NuovoTesto::Rifiuta]);
        let c = c.dopo(&Scrittura::Testo(mano.clone()));
        assert_eq!(c.testi, vec![mano]);
        assert_eq!(c.scarto_ms, Some(0));
        // E non crescono senza fine.
        let mut c = Correzione::default();
        for n in 0..20 {
            c = c
                .dopo(&Scrittura::Testo(voce_scelta(n)))
                .dopo(&Scrittura::Testo(NuovoTesto::Rifiuta));
        }
        assert_eq!(c.testi.len(), MASSIMI_GESTI_IN_POSTA);
    }

    /// Una voce del catalogo, come la darebbe l'elenco dei candidati.
    fn voce_del_catalogo(id: i64) -> rymembra_meta::lrclib::Voce {
        rymembra_meta::lrclib::Voce {
            candidato: rymembra_domain::testo::Candidato {
                id,
                sincronizzato: true,
                ..Default::default()
            },
            piatto: None,
            sincronizzato: Some(LRC.to_owned()),
        }
    }

    #[test]
    fn un_gesto_che_qui_non_ha_cambiato_niente_non_va_al_proprietario() {
        // Il testo di qui è a mano: la voce scelta e il «non è questo» non lo
        // toccano mai, e al proprietario non devono arrivare — né, in posta,
        // prendere il posto del testo a mano che aspetta il suo turno.
        let c = libreria();
        let b = il_brano_specchio(&c);
        let remoto = b.remoto.clone().expect("una riga specchio");
        c.execute(
            "INSERT INTO lyrics (track_key, synced, source, updated_at, checked_at)
             VALUES (?1, ?2, 'mano', 1, 1)",
            params![&b.track_key, LRC],
        )
        .unwrap();
        let mano = Scrittura::Testo(NuovoTesto::Mano {
            lrc: LRC.to_owned(),
        });
        assert_eq!(
            dopo_un_gesto(&c, &b, &mano, ADESSO).unwrap(),
            Posta::Mandare(remoto.dispositivo.clone()),
            "un testo a mano parte sempre"
        );

        ricorda_scelta(&c, &b, &voce_del_catalogo(5)).unwrap();
        let scelta = Scrittura::Testo(voce_scelta(5));
        assert_eq!(
            dopo_un_gesto(&c, &b, &scelta, ADESSO + 1).unwrap(),
            Posta::Niente
        );
        rifiuta(&c, &b.track_key).unwrap();
        assert_eq!(
            dopo_un_gesto(&c, &b, &Scrittura::Testo(NuovoTesto::Rifiuta), ADESSO + 2).unwrap(),
            Posta::Niente
        );
        let voce = crate::dispositivi::in_posta(
            &c,
            &remoto.dispositivo,
            crate::dispositivi::POSTA_TESTO,
            &remoto.id_remoto.to_string(),
        )
        .unwrap()
        .expect("la voce c'è ancora");
        assert_eq!(
            Correzione::da_corpo(&voce.corpo).unwrap().testi,
            vec![NuovoTesto::Mano {
                lrc: LRC.to_owned()
            }],
            "il testo a mano aspetta ancora il suo turno"
        );
    }

    #[test]
    fn un_gesto_che_qui_ha_cambiato_qualcosa_va_al_proprietario() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let remoto = b.remoto.clone().expect("una riga specchio");
        ricorda_scelta(&c, &b, &voce_del_catalogo(5)).unwrap();
        assert_eq!(
            dopo_un_gesto(&c, &b, &Scrittura::Testo(voce_scelta(5)), ADESSO).unwrap(),
            Posta::Mandare(remoto.dispositivo.clone())
        );
        rifiuta(&c, &b.track_key).unwrap();
        assert_eq!(
            dopo_un_gesto(&c, &b, &Scrittura::Testo(NuovoTesto::Rifiuta), ADESSO + 1).unwrap(),
            Posta::Mandare(remoto.dispositivo.clone())
        );
        assert_eq!(in_posta(&c).testi.len(), 2, "scelta e rifiuto, in fila");
        // Un brano di questa macchina non ha un proprietario.
        locale(&c, 600, "k600", "C:/Musica/600.mp3");
        let mio = brano(&c, 600).unwrap();
        assert_eq!(
            dopo_un_gesto(&c, &mio, &Scrittura::Scarto(5), ADESSO).unwrap(),
            Posta::Niente
        );
        // E un testo che non sta in una domanda non parte.
        let lunga = format!("[00:01.00]{}\n", "a".repeat(MASSIMO_TESTO));
        assert_eq!(
            dopo_un_gesto(
                &c,
                &b,
                &Scrittura::Testo(NuovoTesto::Mano { lrc: lunga }),
                ADESSO + 2
            )
            .unwrap(),
            Posta::TroppoLungo
        );
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

    // ── il «mano» del proprietario è una copia, non un testo di qui ─────────

    /// Il testo che il proprietario dice di aver sincronizzato a mano: il suo
    /// `fonte` è `mano`, e per chi lo riceve non lo è.
    fn dal_proprietario_a_mano(lrc: &str) -> DalDisco {
        DalDisco {
            proprietario: Some(dal_proprietario(&il_testo(Some(lrc), None))),
            ..DalDisco::default()
        }
    }

    #[test]
    fn il_testo_a_mano_del_proprietario_si_aggiorna_quando_lui_lo_rifa() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let primo = componi(
            &c,
            &b,
            leggi_riga(&c, &b.track_key),
            dal_proprietario_a_mano(LRC),
        );
        assert_eq!(primo.testo.righe.first().map(|r| r.ms), Some(1_000));

        // Lui lo rifà, e chi ascolta qui lo vede cambiare: la copia non è
        // congelata.
        let nuovo = "[00:09.00]rifatta\n";
        let dopo = componi(
            &c,
            &b,
            leggi_riga(&c, &b.track_key),
            dal_proprietario_a_mano(nuovo),
        );
        assert_eq!(dopo.testo.righe.first().map(|r| r.ms), Some(9_000));
        let r = leggi_riga(&c, &b.track_key).expect("copia");
        assert_eq!(r.synced.as_deref(), Some(nuovo));
        assert_eq!(r.fonte.nome(), "proprietario");
    }

    #[test]
    fn un_mano_dichiarato_dal_proprietario_non_e_un_mano_di_qui() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let _ = componi(
            &c,
            &b,
            leggi_riga(&c, &b.track_key),
            dal_proprietario_a_mano(LRC),
        );
        let r = leggi_riga(&c, &b.track_key).expect("copia");
        assert!(
            !r.fonte.e_di_chi_ascolta(),
            "il testo del proprietario è diventato un testo di chi ascolta"
        );
        // Non si pubblica come lavoro di chi ascolta: non l'ha fatto lui.
        assert!(crate::testi::da_restituire(&c, b.id).is_err());
    }

    #[test]
    fn non_e_questo_scarta_la_copia_del_proprietario() {
        let c = libreria();
        let b = il_brano_specchio(&c);
        let _ = componi(
            &c,
            &b,
            leggi_riga(&c, &b.track_key),
            dal_proprietario_a_mano(LRC),
        );
        rifiuta(&c, &b.track_key).unwrap();
        let r = leggi_riga(&c, &b.track_key).expect("riga");
        assert!(
            r.synced.is_none() && r.plain.is_none(),
            "la copia è rimasta"
        );
    }

    // ── gli scartati hanno un tetto ─────────────────────────────────────────

    #[test]
    fn gli_scartati_di_un_brano_hanno_un_tetto() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/Musica/1.mp3");
        let b = brano(&c, 1).expect("brano");
        // Un dispositivo che alterna «scelgo la voce n» e «non è questo» con un
        // n sempre nuovo: gli scartati di quel brano non crescono senza fine.
        for n in 1..=500 {
            let voce = Voce {
                candidato: Candidato {
                    id: n,
                    sincronizzato: true,
                    ..Candidato::default()
                },
                piatto: None,
                sincronizzato: Some(LRC.to_owned()),
            };
            ricorda_scelta(&c, &b, &voce).unwrap();
            rifiuta(&c, &b.track_key).unwrap();
        }
        let quanti: i64 = c
            .query_row(
                "SELECT json_array_length(COALESCE(scartati, '[]')) FROM lyrics WHERE track_key = 'k1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(quanti <= 64, "{quanti} scartati");
        // E si tengono i più recenti: l'ultima voce rifiutata c'è ancora.
        let c_e: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM lyrics, json_each(lyrics.scartati)
                  WHERE track_key = 'k1' AND value = 500",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(c_e, 1);
    }

    // ── la più recente vince, e un dispositivo non scrive all'infinito ──────

    const DOPO: &str = "[00:09.00]nuova riga\n";
    const ALTRA: &str = "[00:07.00]altra riga\n";
    const UN_SECONDO: i64 = 1_000;

    /// Una correzione da `da`, decisa a `quando_ms` (zero: senza orologio), arrivata
    /// a `adesso_ms`.
    fn scrive(
        c: &Connection,
        id: i64,
        scrittura: &Scrittura,
        da: &str,
        quando_ms: i64,
        adesso_ms: i64,
    ) -> Result<(), rymembra_domain::errors::AppError> {
        super::scrivi_dal_dispositivo(
            c,
            id,
            scrittura,
            &Scrivente {
                dispositivo: da,
                quando_ms,
                adesso_ms,
            },
        )
    }

    fn mano(lrc: &str) -> Scrittura {
        Scrittura::Testo(NuovoTesto::Mano {
            lrc: lrc.to_owned(),
        })
    }

    /// Un brano condiviso `1` con `k1` e il suo testo a mano, fatto qui a `quando_ms`.
    fn con_un_testo_a_mano(quando_ms: i64) -> Connection {
        let c = libreria();
        locale(&c, 1, "k1", "C:/Musica/1.mp3");
        c.execute(
            "INSERT INTO lyrics (track_key, synced, source, offset_ms, updated_at, checked_at)
             VALUES ('k1', ?1, 'mano', 40, ?2, ?2)",
            params![LRC, quando_ms],
        )
        .unwrap();
        c
    }

    #[test]
    fn una_correzione_decisa_prima_non_scavalca_un_testo_a_mano_piu_nuovo() {
        // Il testo di qui è stato rifatto a ADESSO − 10 s. Una correzione decisa
        // un'ora fa dal telefono, che l'ha tenuta in posta, arriva adesso.
        let c = con_un_testo_a_mano(ADESSO - 10 * UN_SECONDO);
        let decisa = ADESSO - 3_600 * UN_SECONDO;
        scrive(&c, 1, &mano(DOPO), &mittente(), decisa, ADESSO).expect("risponde Fatto");
        let r = leggi_riga(&c, "k1").unwrap();
        assert_eq!(
            r.synced.as_deref(),
            Some(LRC),
            "il testo di qui è stato scavalcato"
        );
        assert_eq!(r.scarto_ms, 40);
        // Lo scarto che accompagna quel testo (un testo a mano porta con sé uno
        // scarto a zero) non sposta il testo che è rimasto.
        scrive(&c, 1, &Scrittura::Scarto(0), &mittente(), decisa, ADESSO).expect("Fatto");
        assert_eq!(leggi_riga(&c, "k1").unwrap().scarto_ms, 40);
        // E non ha lasciato una copia: non è stato sostituito niente.
        assert_eq!(nelle_righe(&c, "k1\u{1f}prima"), 0);
    }

    #[test]
    fn una_correzione_piu_recente_scavalca_e_la_riga_ricorda_chi_l_ha_decisa() {
        let c = con_un_testo_a_mano(ADESSO - 3_600 * UN_SECONDO);
        let decisa = ADESSO - 60 * UN_SECONDO;
        scrive(&c, 1, &mano(DOPO), &mittente(), decisa, ADESSO).unwrap();
        let r = leggi_riga(&c, "k1").unwrap();
        assert_eq!(r.synced.as_deref(), Some(DOPO));
        // L'orologio della riga è quello di chi ha deciso, non l'ora di arrivo.
        assert_eq!(r.aggiornata_ms, decisa);
        assert_eq!(r.scarto_ms, 0, "un testo a mano azzera lo scarto");
        // Lo scarto che segue, con lo stesso orologio, si applica: è parte della
        // stessa correzione.
        scrive(&c, 1, &Scrittura::Scarto(250), &mittente(), decisa, ADESSO).unwrap();
        assert_eq!(leggi_riga(&c, "k1").unwrap().scarto_ms, 250);
    }

    #[test]
    fn due_correzioni_consegnate_in_ordine_inverso_vincono_per_orologio() {
        // A decide a T1, B a T2 > T1, e arrivano nell'ordine sbagliato: A prima.
        let (t1, t2) = (ADESSO - 600 * UN_SECONDO, ADESSO - 300 * UN_SECONDO);
        let c = con_un_testo_a_mano(ADESSO - 3_600 * UN_SECONDO);
        scrive(&c, 1, &mano(DOPO), &mittente(), t1, ADESSO).unwrap();
        scrive(&c, 1, &mano(ALTRA), &mittente(), t2, ADESSO + UN_SECONDO).unwrap();
        // Vince B, la più recente, anche se è arrivata dopo di A — e se la riga
        // avesse ricordato l'ora di arrivo di A, B sarebbe stata scartata.
        assert_eq!(leggi_riga(&c, "k1").unwrap().synced.as_deref(), Some(ALTRA));
        // Nell'altro ordine vince la stessa.
        let c = con_un_testo_a_mano(ADESSO - 3_600 * UN_SECONDO);
        scrive(&c, 1, &mano(ALTRA), &mittente(), t2, ADESSO).unwrap();
        scrive(&c, 1, &mano(DOPO), &mittente(), t1, ADESSO + UN_SECONDO).unwrap();
        assert_eq!(leggi_riga(&c, "k1").unwrap().synced.as_deref(), Some(ALTRA));
    }

    #[test]
    fn senza_orologio_vale_adesso_e_un_orologio_avanti_vale_adesso() {
        // Una build di prima non manda l'orologio: vince l'ultima arrivata, com'è
        // sempre stato — e non sparisce in silenzio.
        let c = con_un_testo_a_mano(ADESSO - 10 * UN_SECONDO);
        scrive(&c, 1, &mano(DOPO), &mittente(), 0, ADESSO).unwrap();
        let r = leggi_riga(&c, "k1").unwrap();
        assert_eq!(r.synced.as_deref(), Some(DOPO));
        assert_eq!(r.aggiornata_ms, ADESSO);
        // Un orologio nel futuro non fissa un testo che nessuna correzione vera
        // scavalcherebbe: si riporta ad adesso.
        scrive(
            &c,
            1,
            &mano(ALTRA),
            &mittente(),
            ADESSO + 86_400_000,
            ADESSO + UN_SECONDO,
        )
        .unwrap();
        let r = leggi_riga(&c, "k1").unwrap();
        assert_eq!(r.synced.as_deref(), Some(ALTRA));
        assert_eq!(r.aggiornata_ms, ADESSO + UN_SECONDO);
    }

    #[test]
    fn la_stessa_correzione_due_volte_e_innocua() {
        let c = con_un_testo_a_mano(ADESSO - 3_600 * UN_SECONDO);
        let decisa = ADESSO - 60 * UN_SECONDO;
        scrive(&c, 1, &mano(DOPO), &mittente(), decisa, ADESSO).unwrap();
        // Una consegna interrotta si rifà per intero: la seconda non scrive.
        c.execute(
            "UPDATE lyrics SET checked_at = 123 WHERE track_key = 'k1'",
            [],
        )
        .unwrap();
        scrive(&c, 1, &mano(DOPO), &mittente(), decisa, ADESSO + UN_SECONDO).unwrap();
        let r = leggi_riga(&c, "k1").unwrap();
        assert_eq!(r.synced.as_deref(), Some(DOPO));
        // La copia è ancora il testo di prima, non il testo appena scritto.
        let copia: String = c
            .query_row(
                "SELECT synced FROM lyrics WHERE track_key = ?1",
                ["k1\u{1f}prima"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(copia, LRC);
    }

    #[test]
    fn un_dispositivo_non_scrive_oltre_il_tetto_al_giorno() {
        let c = libreria();
        let tetto = dispositivi::TESTI_AL_GIORNO;
        for n in 1..=i64::try_from(tetto + 5).unwrap() {
            locale(&c, n, &format!("k{n}"), &format!("C:/Musica/{n}.mp3"));
        }
        let b = mittente();
        let un_altro = chiave('c');
        let decisa = ADESSO - 60 * UN_SECONDO;
        for n in 1..=i64::try_from(tetto).unwrap() {
            scrive(&c, n, &mano(LRC), &b, decisa, ADESSO).unwrap_or_else(|e| {
                panic!(
                    "il testo {n} di {tetto} non passa: {}",
                    e.code().kind().code()
                )
            });
        }
        // Il successivo sente `lontano.irraggiungibile` e non scrive niente: chi
        // manda lo tiene in posta e riprova.
        let n = i64::try_from(tetto + 1).unwrap();
        assert_eq!(
            codice(scrive(&c, n, &mano(LRC), &b, decisa, ADESSO)),
            "lontano.irraggiungibile"
        );
        assert_eq!(nelle_righe(&c, &format!("k{n}")), 0);
        // Anche una voce scelta dal catalogo si conta: scrive un testo.
        let scelta = Scrittura::Testo(NuovoTesto::Scelta {
            id_catalogo: 9,
            piatto: None,
            sincronizzato: Some(LRC.to_owned()),
            strumentale: false,
        });
        assert_eq!(
            codice(scrive(&c, n, &scelta, &b, decisa, ADESSO)),
            "lontano.irraggiungibile"
        );
        // La consegna di una correzione già applicata che si ripete, a tetto
        // raggiunto, non si rifiuta: è la stessa, e non c'è niente da scrivere.
        scrive(&c, 1, &mano(LRC), &b, decisa, ADESSO).expect("la stessa, già applicata");
        // Un altro dispositivo ha il suo tetto.
        scrive(&c, n, &mano(LRC), &un_altro, decisa, ADESSO).expect("un altro dispositivo");
        // Uno scarto e un «non è questo» non scrivono un testo: non si contano.
        scrive(&c, n + 1, &Scrittura::Scarto(5), &b, decisa, ADESSO).expect("uno scarto");
        scrive(
            &c,
            n + 1,
            &Scrittura::Testo(NuovoTesto::Rifiuta),
            &b,
            decisa,
            ADESSO,
        )
        .expect("un rifiuto");
        // E la finestra scorre: dopo ventiquattr'ore si può di nuovo.
        let dopo = ADESSO + dispositivi::FINESTRA_TESTI_MS + UN_SECONDO;
        scrive(&c, n, &mano(LRC), &b, decisa + dopo - ADESSO, dopo).expect("il giorno dopo");
    }

    #[test]
    fn il_tetto_dei_testi_non_tocca_quello_degli_ascolti() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/Musica/1.mp3");
        let b = mittente();
        scrive(&c, 1, &mano(LRC), &b, ADESSO - UN_SECONDO, ADESSO).unwrap();
        assert_eq!(
            dispositivi::ricevuti_da(&c, &b, ADESSO - 3_600 * UN_SECONDO).unwrap(),
            0,
            "un testo scritto si è contato fra gli ascolti"
        );
    }

    // ── la copia di un livello ──────────────────────────────────────────────

    fn copia_di(c: &Connection, k: &str) -> Option<String> {
        c.query_row(
            "SELECT synced FROM lyrics WHERE track_key = ?1",
            [format!("{k}\u{1f}prima")],
            |r| r.get(0),
        )
        .ok()
    }

    #[test]
    fn il_testo_a_mano_sostituito_resta_come_copia_di_un_livello() {
        let c = con_un_testo_a_mano(ADESSO - 3_600 * UN_SECONDO);
        scrive(
            &c,
            1,
            &mano(DOPO),
            &mittente(),
            ADESSO - 60 * UN_SECONDO,
            ADESSO,
        )
        .unwrap();
        assert_eq!(copia_di(&c, "k1").as_deref(), Some(LRC));
        let copia = crate::testi::copia::leggi(&c, "k1")
            .unwrap()
            .expect("copia");
        assert_eq!(copia.sostituita_ms, ADESSO);
        assert_eq!(copia.byte, LRC.len());
        // Un livello solo: una seconda sostituzione prende il posto della copia.
        scrive(
            &c,
            1,
            &mano(ALTRA),
            &mittente(),
            ADESSO - 30 * UN_SECONDO,
            ADESSO + 5,
        )
        .unwrap();
        assert_eq!(copia_di(&c, "k1").as_deref(), Some(DOPO));
        // La copia non è il testo di nessun brano: nessuna chiave di brano la
        // incontra, e la riga vera è sempre una sola.
        assert_eq!(nelle_righe(&c, "k1"), 1);
        assert_eq!(leggi_riga(&c, "k1").unwrap().synced.as_deref(), Some(ALTRA));
    }

    #[test]
    fn quel_che_non_e_a_mano_non_lascia_copia() {
        let c = libreria();
        locale(&c, 1, "k1", "C:/Musica/1.mp3");
        c.execute(
            "INSERT INTO lyrics (track_key, synced, source, updated_at) VALUES ('k1', ?1, 'lrclib', 5)",
            [LRC],
        )
        .unwrap();
        scrive(&c, 1, &mano(DOPO), &mittente(), ADESSO - UN_SECONDO, ADESSO).unwrap();
        assert_eq!(leggi_riga(&c, "k1").unwrap().synced.as_deref(), Some(DOPO));
        assert_eq!(copia_di(&c, "k1"), None, "un testo scaricato si riscarica");
        // E scrivere due volte lo stesso testo non lo copia su sé stesso.
        let c = con_un_testo_a_mano(ADESSO - 3_600 * UN_SECONDO);
        scrive(&c, 1, &mano(LRC), &mittente(), ADESSO - UN_SECONDO, ADESSO).unwrap();
        assert_eq!(copia_di(&c, "k1"), None);
    }

    #[test]
    fn ripristinare_scambia_il_testo_con_la_copia_e_si_disfa() {
        let c = con_un_testo_a_mano(ADESSO - 3_600 * UN_SECONDO);
        // Senza copia non c'è niente da ripristinare.
        assert!(!crate::testi::copia::ripristina(&c, "k1", ADESSO).unwrap());
        scrive(
            &c,
            1,
            &mano(DOPO),
            &mittente(),
            ADESSO - 60 * UN_SECONDO,
            ADESSO,
        )
        .unwrap();

        assert!(crate::testi::copia::ripristina(&c, "k1", ADESSO + 10).unwrap());
        let r = leggi_riga(&c, "k1").unwrap();
        assert_eq!(r.synced.as_deref(), Some(LRC));
        assert!(r.fonte.e_di_chi_ascolta());
        assert_eq!(
            r.aggiornata_ms,
            ADESSO + 10,
            "un gesto di chi ascolta vale adesso"
        );
        assert_eq!(copia_di(&c, "k1").as_deref(), Some(DOPO));
        // Una correzione decisa prima del ripristino non lo scavalca.
        scrive(
            &c,
            1,
            &mano(ALTRA),
            &mittente(),
            ADESSO - UN_SECONDO,
            ADESSO + 20,
        )
        .unwrap();
        assert_eq!(leggi_riga(&c, "k1").unwrap().synced.as_deref(), Some(LRC));

        // Ripristinare ancora è disfare: torna il testo della correzione.
        assert!(crate::testi::copia::ripristina(&c, "k1", ADESSO + 30).unwrap());
        assert_eq!(leggi_riga(&c, "k1").unwrap().synced.as_deref(), Some(DOPO));
        assert_eq!(copia_di(&c, "k1").as_deref(), Some(LRC));
    }
}
