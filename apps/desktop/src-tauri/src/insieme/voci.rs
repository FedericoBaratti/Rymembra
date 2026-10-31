//! Le voci della sessione sul filo, nei due versi.
//!
//! - **Chi ospita** trasforma un brano della sua libreria ([`BranoDiQuesto`])
//!   in una [`VoceSulFilo`], e gli dà i permessi per leggerla: un gettone per
//!   il file, uno per la copertina ([`Servite`]).
//! - **Chi segue** trasforma una [`VoceSulFilo`] in una [`Voce`] del suo
//!   lettore, con dentro chi ne apre i byte in rete ([`voce_da_filo`]).
//!
//! # Quel che parte deve passare `valida()` dall'altra parte
//!
//! Un membro scarta lo **stato intero** se una voce sola esce dai tetti di
//! [`rymembra_insieme::messaggi::Messaggio::valida`], e chiude la connessione:
//! un a capo nel titolo di un MP3 — succede, i tag li scrive chiunque — farebbe
//! cadere la sessione a tutti. Per questo ogni testo passa da [`testo`] prima
//! di partire, ogni numero sta nei suoi limiti, e la coda non supera mai
//! [`MASSIMO_VOCI`]: chi entra a ospitare con cinquemila brani in coda manda
//! una finestra di mille attorno a quello che suona ([`finestra`]).
//!
//! `valida()` però non conta i byte, e la cornice sì (1 MiB): mille voci con
//! titoli lunghi in caratteri di tre byte ci arrivano. [`nel_tetto`] misura lo
//! stato serializzato e, se serve, accorcia i testi e poi la coda, finché ci
//! sta.
//!
//! # Gettoni: uno per file, uno per copertina
//!
//! Non uno per voce. Le voci di un'immagine CD sono tratti dello **stesso**
//! file, e un gettone solo vuol dire che chi segue le apre come un file solo,
//! senza buchi fra un brano e l'altro; dodici brani di un album hanno la
//! stessa copertina, e un gettone solo vuol dire che chi segue la scarica una
//! volta. Ognuno si revoca quando l'ultima voce che lo usa esce dalla coda.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use rymembra_domain::errors::AppError;
use rymembra_domain::queue::RepeatMode;
use rymembra_insieme::messaggi::{
    DescrittoreCopia, MASSIMA_DURATA_MS, MASSIMO_RIFERIMENTO, MASSIMO_TESTO, MASSIMO_VOCI,
    Messaggio, Persona, Ripeti, SegmentoSulFilo, SorgenteSulFilo, StatoSessione, VoceSulFilo,
};
use rymembra_insieme::{FileServito, Gettone};

use crate::riproduzione::insieme::{
    Apertura, ApriInRete, BranoDiQuesto, Istantanea, SchedaVoce, Voce,
};

/// Il prefisso delle chiavi delle copertine arrivate dalla rete: `insieme-<uid>`.
pub const PREFISSO_COPERTINA: &str = "insieme-";

/// Il tetto di una copertina, in byte, nei due versi: chi ospita non ne serve
/// una più grande, e chi segue non ne decodifica una più grande. È quello di
/// `rymembra_app::covers::ricodifica_estranea`, che chi segue usa: lo stesso
/// numero da tutte e due le parti, così una copertina servita è sempre una
/// copertina accettata.
pub const BYTE_MASSIMI_COPERTINA: usize = rymembra_app::covers::MASSIMO_ESTRANEA;

/// Quanta memoria possono occupare le copertine arrivate dalla rete in una
/// sessione: da chi segue, e da chi ospita per le voci dei membri.
/// Sessantaquattro mebibyte: un migliaio di copertine da sessanta kilobyte, che
/// è la coda più lunga. Oltre, le voci restano senza; quelle uscite dalla coda
/// liberano il loro posto (vedi [`Copertine::dimentica`]).
pub const TETTO_COPERTINE: usize = 64 * 1024 * 1024;

/// Quanto può pesare lo stato serializzato. La cornice ammette 1 MiB per il
/// messaggio intero, involucro del trasporto compreso: i centoventiquattro
/// kilobyte che restano sono il margine per quello, e per non stare mai sul
/// filo del rasoio.
pub const BYTE_MASSIMI_STATO: usize = 900 * 1024;

/// Quante voci prima della corrente tiene la finestra di una coda troppo
/// lunga: quelle appena passate, che «precedente» può rimettere.
const PRIMA_DELLA_CORRENTE: usize = 50;

/// Un testo che passa `valida()`: niente caratteri di controllo (diventano uno
/// spazio, come fa la libreria coi nomi dei PC), spazi in testa e in coda
/// tolti, e al più `tetto` caratteri.
#[must_use]
pub fn testo(grezzo: &str, tetto: usize) -> String {
    let pulito: String = grezzo
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    pulito
        .trim()
        .chars()
        .take(tetto)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Il modo di ripetizione della coda, sul filo.
#[must_use]
pub const fn ripeti_sul_filo(modo: RepeatMode) -> Ripeti {
    match modo {
        RepeatMode::Off => Ripeti::No,
        RepeatMode::One => Ripeti::Una,
        RepeatMode::All => Ripeti::Tutta,
    }
}

/// Il modo di ripetizione arrivato dall'host, per lo specchio.
#[must_use]
pub const fn ripeti_da_filo(modo: Ripeti) -> RepeatMode {
    match modo {
        Ripeti::No => RepeatMode::Off,
        Ripeti::Una => RepeatMode::One,
        Ripeti::Tutta => RepeatMode::All,
        // Un modo che questa versione non conosce, da un host più nuovo:
        // nello specchio vale «niente», e la coda resta quella dell'host.
        Ripeti::Sconosciuto => RepeatMode::Off,
    }
}

/// La chiave `rymembra-cover` della copertina di una voce arrivata dalla rete.
#[must_use]
pub fn chiave_copertina(uid: u64) -> String {
    format!("{PREFISSO_COPERTINA}{uid}")
}

/// L'`uid` dentro una chiave di [`chiave_copertina`]; `None` per ogni altra
/// chiave, comprese le impronte della libreria.
#[must_use]
pub fn uid_della_chiave(chiave: &str) -> Option<u64> {
    let numero = chiave.strip_prefix(PREFISSO_COPERTINA)?;
    // Solo cifre: `+5` o ` 5` si leggerebbero come 5, e due chiavi diverse
    // per la stessa copertina sono una cache che si confonde.
    if numero.is_empty() || !numero.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    numero.parse().ok()
}

// ── chi ospita ──────────────────────────────────────────────────────────────

/// Che cosa fare col trasporto, dopo un aggiornamento di [`Servite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Servizio {
    /// Lascia leggere un file sotto un gettone: `Rete::servi`.
    File(Gettone, FileServito),
    /// Lascia leggere una copertina: i byte li legge chi esegue, dall'archivio
    /// delle copertine, per questa impronta. `Rete::servi_copertina`.
    Copertina(Gettone, String),
    /// Toglie un gettone: `Rete::revoca`.
    Revoca(Gettone),
    /// Fase 2, chi ospita: i byte di questo gettone li ha il membro con quel
    /// numero (`Rete::inoltra`, con la sua chiave).
    Inoltra(Gettone, u32),
    /// Fase 3: il descrittore di questo file va calcolato (dal disco, fuori
    /// dal filo della sessione), e poi messo con [`Servite::imposta_copia`].
    Descrivi {
        /// Il file.
        file: ChiaveFile,
        /// La sua riga, per la chiave di contenuto.
        track_id: i64,
    },
}

/// Un file come chiave: lo stesso percorso con un'altra dimensione o un'altra
/// data è un altro file, e merita un altro gettone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChiaveFile {
    /// Il percorso.
    pub percorso: PathBuf,
    /// La dimensione.
    pub dimensione: u64,
    /// L'ultima modifica, ms dall'epoca.
    pub modificato_ms: i64,
}

/// Un gettone condiviso da più voci.
#[derive(Debug, Clone, Copy)]
struct Condiviso {
    gettone: Gettone,
    quante: usize,
}

/// Una voce già descritta.
#[derive(Debug, Clone)]
struct Servita {
    voce: VoceSulFilo,
    file: Option<ChiaveFile>,
    copertina: Option<String>,
    /// La riga della libreria di questo PC, per le voci sue.
    track_id: Option<i64>,
    /// Di chi è: `0` le voci di questo PC, il numero del membro per quelle
    /// che un membro ha aggiunto (fase 2, chi ospita).
    di: u32,
}

/// Le voci descritte e i gettoni dati. Vedi il `//!`.
///
/// La usano tutti e due: chi ospita per le sue voci e per quelle dei membri
/// (dei loro gettoni tiene solo il conto, per smettere di inoltrarli), chi
/// segue per le voci che aggiunge lui (fase 2).
#[derive(Debug, Default)]
pub struct Servite {
    voci: HashMap<u64, Servita>,
    file: HashMap<ChiaveFile, Condiviso>,
    copertine: HashMap<String, Condiviso>,
    /// I gettoni dei membri inoltrati: di quale membro, e quante voci li usano.
    inoltri: HashMap<Gettone, (u32, usize)>,
    /// I descrittori arrivati, per file: valgono anche per le voci dello
    /// stesso file aggiunte dopo. `None`: chiesto e non venuto (un file che
    /// non si legge), che non si richiede più.
    copie: HashMap<ChiaveFile, Option<DescrittoreCopia>>,
}

impl Servite {
    /// La voce è già descritta.
    #[must_use]
    pub fn ha(&self, uid: u64) -> bool {
        self.voci.contains_key(&uid)
    }

    /// La voce sul filo, se è descritta.
    #[must_use]
    pub fn voce(&self, uid: u64) -> Option<&VoceSulFilo> {
        self.voci.get(&uid).map(|s| &s.voce)
    }

    /// Descrive una voce nuova e dice quali gettoni dare. Una voce che non si
    /// può mandare — niente file e niente riferimento, o un riferimento oltre
    /// il tetto — non si descrive: resta fuori dallo stato, e il diario lo
    /// conta.
    ///
    /// # Errori
    ///
    /// Quelli di `nuovo_gettone`: il generatore del sistema che non risponde.
    pub fn aggiungi(
        &mut self,
        uid: u64,
        brano: &BranoDiQuesto,
        nuovo_gettone: &mut dyn FnMut() -> Result<Gettone, AppError>,
    ) -> Result<Vec<Servizio>, AppError> {
        let mut servizi = Vec::new();
        if self.ha(uid) {
            return Ok(servizi);
        }
        let sorgente_grezza = match (&brano.file, &brano.riferimento) {
            (Some(file), _) => Some(Grezza::File(file)),
            (None, Some(riferimento)) => Some(Grezza::Catalogo(riferimento)),
            (None, None) => None,
        };
        let Some(sorgente_grezza) = sorgente_grezza else {
            return Ok(servizi);
        };
        let (sorgente, chiave_file) = match sorgente_grezza {
            Grezza::File(file) => {
                let chiave = ChiaveFile {
                    percorso: file.percorso.clone(),
                    dimensione: file.dimensione,
                    modificato_ms: file.modificato_ms,
                };
                let gettone = match self.file.get(&chiave) {
                    Some(c) => c.gettone,
                    None => {
                        let gettone = nuovo_gettone()?;
                        servizi.push(Servizio::File(
                            gettone,
                            FileServito {
                                percorso: chiave.percorso.clone(),
                                dimensione: chiave.dimensione,
                                modificato_ms: chiave.modificato_ms,
                            },
                        ));
                        // Un file nuovo: il suo descrittore, se non lo si ha
                        // già. La voce intanto parte senza (vedi il `//!`
                        // di `filo`, «Fase 3»).
                        if !self.copie.contains_key(&chiave) {
                            servizi.push(Servizio::Descrivi {
                                file: chiave.clone(),
                                track_id: brano.track_id,
                            });
                        }
                        gettone
                    }
                };
                (
                    SorgenteSulFilo::File {
                        gettone,
                        estensione: file.estensione.as_deref().and_then(estensione),
                        dimensione: file.dimensione,
                        segmento: file.segmento.and_then(segmento),
                    },
                    Some(chiave),
                )
            }
            Grezza::Catalogo(riferimento) => {
                // Un riferimento non si accorcia: tagliato non sarebbe più lui.
                if riferimento.chars().count() > MASSIMO_RIFERIMENTO
                    || riferimento.chars().any(char::is_control)
                {
                    return Ok(servizi);
                }
                (
                    SorgenteSulFilo::Catalogo {
                        riferimento: riferimento.clone(),
                    },
                    None,
                )
            }
        };
        let copertina = brano.copertina.clone().filter(|c| !c.is_empty());
        let gettone_copertina = match &copertina {
            Some(impronta) => Some(match self.copertine.get(impronta) {
                Some(c) => c.gettone,
                None => {
                    let gettone = nuovo_gettone()?;
                    servizi.push(Servizio::Copertina(gettone, impronta.clone()));
                    gettone
                }
            }),
            None => None,
        };
        // Solo adesso, con tutti i gettoni in mano, si contano: un errore del
        // generatore a metà non deve lasciare un conteggio senza la voce.
        if let (Some(chiave), SorgenteSulFilo::File { gettone, .. }) = (&chiave_file, &sorgente) {
            self.file
                .entry(chiave.clone())
                .or_insert(Condiviso {
                    gettone: *gettone,
                    quante: 0,
                })
                .quante += 1;
        }
        if let (Some(impronta), Some(gettone)) = (&copertina, gettone_copertina) {
            self.copertine
                .entry(impronta.clone())
                .or_insert(Condiviso { gettone, quante: 0 })
                .quante += 1;
        }
        let mut voce = descrivi(uid, &brano.scheda, sorgente, gettone_copertina);
        voce.copia = chiave_file
            .as_ref()
            .and_then(|chiave| self.copie.get(chiave))
            .cloned()
            .flatten();
        self.voci.insert(
            uid,
            Servita {
                voce,
                file: chiave_file,
                copertina,
                track_id: Some(brano.track_id),
                di: 0,
            },
        );
        Ok(servizi)
    }

    /// Chi ospita: una voce aggiunta da un membro, già sul filo, col suo `uid`
    /// della sessione e `di` già messo. I suoi gettoni si inoltrano al
    /// membro, uno per file e uno per copertina.
    ///
    /// # Errori
    ///
    /// [`GettoneAltrui`] se un gettone della voce è già di qualcun altro: un
    /// file o una copertina di questo PC, o un inoltro a un altro membro. Il
    /// gettone lo sceglie il membro, e accettarlo vorrebbe dire lasciargli
    /// leggere — o farsi chiedere — i byte di un brano che non è suo. Il
    /// trasporto fa lo stesso controllo (`Rete::inoltra` torna `false`):
    /// questo è il primo, e vede anche gli inoltri già contati qui.
    pub fn ospita_voce(&mut self, voce: VoceSulFilo) -> Result<Vec<Servizio>, GettoneAltrui> {
        let mut servizi = Vec::new();
        if self.ha(voce.uid) {
            return Ok(servizi);
        }
        let di = voce.di;
        let gettoni = gettoni_di(&voce);
        let altrui = |g: &Gettone| {
            self.inoltri.get(g).is_some_and(|(chi, _)| *chi != di)
                || self.file.values().any(|c| c.gettone == *g)
                || self.copertine.values().any(|c| c.gettone == *g)
        };
        if gettoni.iter().any(altrui) {
            return Err(GettoneAltrui);
        }
        for gettone in gettoni {
            let (_, quante) = self.inoltri.entry(gettone).or_insert((di, 0));
            if *quante == 0 {
                servizi.push(Servizio::Inoltra(gettone, di));
            }
            *quante = quante.saturating_add(1);
        }
        self.voci.insert(
            voce.uid,
            Servita {
                voce,
                file: None,
                copertina: None,
                track_id: None,
                di,
            },
        );
        Ok(servizi)
    }

    /// Chi ospita: gli `uid` delle voci di un membro.
    #[must_use]
    pub fn di_membro(&self, membro: u32) -> Vec<u64> {
        if membro == 0 {
            return Vec::new();
        }
        self.voci
            .iter()
            .filter(|(_, s)| s.di == membro)
            .map(|(uid, _)| *uid)
            .collect()
    }

    /// Le voci di altri, `uid` e numero del proprietario: per «di Marco».
    pub fn altrui(&self) -> impl Iterator<Item = (u64, u32)> + '_ {
        self.voci
            .iter()
            .filter(|(_, s)| s.di != 0)
            .map(|(uid, s)| (*uid, s.di))
    }

    /// Chi segue: la riga e la copertina di libreria di una voce sua, se è
    /// una di quelle che ha aggiunto lui. Si riconosce dal gettone del file
    /// (l'host rinumera gli `uid`, i gettoni no), o dal riferimento per un
    /// brano di catalogo.
    #[must_use]
    pub fn propria(&self, voce: &VoceSulFilo) -> Option<(i64, Option<String>)> {
        self.voci
            .values()
            .find(|s| match (&s.voce.sorgente, &voce.sorgente) {
                (
                    SorgenteSulFilo::File { gettone: a, .. },
                    SorgenteSulFilo::File { gettone: b, .. },
                ) => a == b,
                (
                    SorgenteSulFilo::Catalogo { riferimento: a },
                    SorgenteSulFilo::Catalogo { riferimento: b },
                ) => a == b,
                _ => false,
            })
            .and_then(|s| Some((s.track_id?, s.copertina.clone())))
    }

    /// Gli `uid` descritti.
    pub fn uids(&self) -> impl Iterator<Item = u64> + '_ {
        self.voci.keys().copied()
    }

    /// Fase 3: il descrittore di un file è arrivato, o è certo che non
    /// arriverà (`None`). Vale per tutte le voci di quel file e per quelle che
    /// arriveranno. Vero se ha cambiato una voce: allora lo stato va
    /// ripubblicato.
    pub fn imposta_copia(&mut self, chiave: &ChiaveFile, copia: Option<DescrittoreCopia>) -> bool {
        let mut cambiate = false;
        for servita in self.voci.values_mut() {
            if servita.file.as_ref() == Some(chiave) && servita.voce.copia != copia {
                servita.voce.copia.clone_from(&copia);
                cambiate = true;
            }
        }
        self.copie.insert(chiave.clone(), copia);
        cambiate
    }

    /// Fase 3: la voce ha già la risposta sul suo descrittore — trovato, o
    /// certo che non verrà — o non ne ha bisogno (un brano di catalogo).
    #[must_use]
    pub fn descritta(&self, uid: u64) -> bool {
        self.voci
            .get(&uid)
            .is_none_or(|s| s.file.as_ref().is_none_or(|f| self.copie.contains_key(f)))
    }

    /// Le voci descritte che non sono in `uids`: quelle che [`Servite::tieni`]
    /// toglierebbe.
    #[must_use]
    pub fn fuori(&self, uids: &HashSet<u64>) -> Vec<u64> {
        self.voci
            .keys()
            .filter(|uid| !uids.contains(uid))
            .copied()
            .collect()
    }

    /// Tiene solo le voci in `uids`, e revoca i gettoni che nessuna usa più.
    pub fn tieni(&mut self, uids: &HashSet<u64>) -> Vec<Servizio> {
        let via = self.fuori(uids);
        self.dimentica(&via)
    }

    /// Toglie queste voci, e revoca i gettoni che nessuna usa più: i propri
    /// non si servono più, quelli inoltrati non si inoltrano più.
    pub fn dimentica(&mut self, uids: &[u64]) -> Vec<Servizio> {
        let mut servizi = Vec::new();
        for uid in uids {
            let Some(servita) = self.voci.remove(uid) else {
                continue;
            };
            if servita.di != 0 {
                for gettone in gettoni_di(&servita.voce) {
                    let Some((_, quante)) = self.inoltri.get_mut(&gettone) else {
                        continue;
                    };
                    *quante = quante.saturating_sub(1);
                    if *quante == 0 {
                        self.inoltri.remove(&gettone);
                        servizi.push(Servizio::Revoca(gettone));
                    }
                }
            }
            if let Some(chiave) = servita.file {
                rilascia(&mut self.file, &chiave, &mut servizi);
            }
            if let Some(impronta) = servita.copertina {
                rilascia(&mut self.copertine, &impronta, &mut servizi);
            }
        }
        servizi
    }

    /// Una copertina che non si è potuta leggere dall'archivio: il suo gettone
    /// si revoca e le voci che la usavano restano senza, invece di promettere
    /// a chi segue un'immagine che risponderebbe `gettoneScaduto`.
    pub fn senza_copertina(&mut self, impronta: &str) -> Vec<Servizio> {
        let Some(condiviso) = self.copertine.remove(impronta) else {
            return Vec::new();
        };
        for servita in self.voci.values_mut() {
            if servita.copertina.as_deref() == Some(impronta) {
                servita.copertina = None;
                servita.voce.copertina = None;
            }
        }
        vec![Servizio::Revoca(condiviso.gettone)]
    }

    /// Dimentica tutto: la rete ha già revocato i gettoni (`revoca_tutto`).
    pub fn svuota(&mut self) {
        self.voci.clear();
        self.file.clear();
        self.copertine.clear();
        self.inoltri.clear();
        self.copie.clear();
    }
}

/// Un gettone che un membro vorrebbe usare è già di qualcun altro: vedi
/// [`Servite::ospita_voce`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GettoneAltrui;

/// I gettoni di una voce sul filo: il file e la copertina, se ci sono.
fn gettoni_di(voce: &VoceSulFilo) -> Vec<Gettone> {
    let mut gettoni = Vec::new();
    if let SorgenteSulFilo::File { gettone, .. } = &voce.sorgente {
        gettoni.push(*gettone);
    }
    if let Some(gettone) = voce.copertina {
        gettoni.push(gettone);
    }
    gettoni
}

/// Fase 3: il descrittore della libreria nella forma del filo, se passa i
/// tetti di `valida()` (un percorso UNC troppo lungo non si manda: la copia
/// si cercherà per impronta).
#[must_use]
pub fn descrittore_sul_filo(d: rymembra_app::insieme::Descrittore) -> Option<DescrittoreCopia> {
    if d.impronta_rapida.is_empty()
        || d.impronta_rapida.chars().count() > 64
        || d.impronta_rapida.chars().any(char::is_control)
    {
        return None;
    }
    let unc = d.unc.filter(|u| {
        u.starts_with(r"\\")
            && u.chars().count() <= MASSIMO_RIFERIMENTO
            && !u.chars().any(char::is_control)
    });
    let chiave_contenuto = d
        .chiave_contenuto
        .filter(|c| c.chars().count() <= 128 && !c.chars().any(char::is_control));
    Some(DescrittoreCopia {
        unc,
        dimensione: d.dimensione,
        modificato_ms: d.modificato_ms,
        impronta_rapida: d.impronta_rapida,
        chiave_contenuto,
    })
}

/// Da dove vengono i byte di un brano di questo PC, prima di descriverlo.
enum Grezza<'a> {
    File(&'a crate::riproduzione::insieme::FileDellaVoce),
    Catalogo(&'a String),
}

/// Toglie un uso da un gettone condiviso, e lo revoca se era l'ultimo.
fn rilascia<K: std::hash::Hash + Eq>(
    mappa: &mut HashMap<K, Condiviso>,
    chiave: &K,
    servizi: &mut Vec<Servizio>,
) {
    let Some(c) = mappa.get_mut(chiave) else {
        return;
    };
    c.quante = c.quante.saturating_sub(1);
    if c.quante == 0 {
        servizi.push(Servizio::Revoca(c.gettone));
        mappa.remove(chiave);
    }
}

/// L'estensione sul filo: minuscola, solo lettere e cifre, al più 16. Un'altra
/// cosa non serve a riconoscere un formato, e non si manda.
fn estensione(grezza: &str) -> Option<String> {
    let e = grezza.trim().trim_start_matches('.').to_ascii_lowercase();
    (!e.is_empty() && e.len() <= 16 && e.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(e)
}

/// Il tratto di un'immagine CD sul filo, se è un tratto possibile.
fn segmento((inizio_ms, fine_ms): (u64, Option<u64>)) -> Option<SegmentoSulFilo> {
    if inizio_ms > MASSIMA_DURATA_MS {
        return None;
    }
    Some(SegmentoSulFilo {
        inizio_ms,
        // Una fine prima dell'inizio non è un tratto: vale «fino alla fine».
        fine_ms: fine_ms.filter(|fine| *fine > inizio_ms),
    })
}

/// La voce sul filo, con ogni campo nei tetti di `valida()`.
#[must_use]
pub fn descrivi(
    uid: u64,
    scheda: &SchedaVoce,
    sorgente: SorgenteSulFilo,
    copertina: Option<Gettone>,
) -> VoceSulFilo {
    VoceSulFilo {
        uid,
        di: 0,
        titolo: testo(&scheda.titolo, MASSIMO_TESTO),
        artista: testo(&scheda.artista, MASSIMO_TESTO),
        album: testo(&scheda.album, MASSIMO_TESTO),
        durata_ms: scheda.durata_ms.min(MASSIMA_DURATA_MS),
        numero: scheda.numero,
        anno: scheda.anno,
        copertina,
        sorgente,
        // Fuori dai limiti non è una correzione, è un tag rotto: meglio
        // nessuna, che è quel che il lettore farebbe col file.
        replaygain_db: scheda
            .replaygain_db
            .filter(|db| db.is_finite() && db.abs() <= 60.0),
        picco: scheda
            .picco
            .filter(|p| p.is_finite() && (0.0..=16.0).contains(p)),
        // Fase 3, nell'ondata 2.
        copia: None,
    }
}

/// La parte della coda che va sul filo: tutta se sta in [`MASSIMO_VOCI`],
/// altrimenti mille voci a partire da un po' prima della corrente.
#[must_use]
pub fn finestra(coda: &[u64], corrente: Option<u64>) -> &[u64] {
    if coda.len() <= MASSIMO_VOCI {
        return coda;
    }
    let posto = corrente
        .and_then(|c| coda.iter().position(|uid| *uid == c))
        .unwrap_or(0);
    let inizio = posto
        .saturating_sub(PRIMA_DELLA_CORRENTE)
        .min(coda.len().saturating_sub(MASSIMO_VOCI));
    coda.get(inizio..inizio.saturating_add(MASSIMO_VOCI))
        .unwrap_or(coda)
}

/// Chi ospita: lo stato da mandare, con le voci descritte nell'ordine della
/// coda. Le voci non descritte (che non si potevano mandare) restano fuori.
#[must_use]
pub fn stato_sessione(
    versione: u64,
    persone: Vec<Persona>,
    istantanea: &Istantanea,
    servite: &Servite,
) -> StatoSessione {
    let coda: Vec<VoceSulFilo> = finestra(&istantanea.coda, istantanea.corrente)
        .iter()
        .filter_map(|uid| servite.voce(*uid).cloned())
        .collect();
    let mut stato = StatoSessione {
        versione,
        persone,
        coda,
        corrente: istantanea.corrente,
        in_pausa: istantanea.in_pausa,
        ripeti: ripeti_sul_filo(istantanea.ripeti),
    };
    nel_tetto(&mut stato);
    stato
}

/// Quanti byte pesa lo stato sul filo, come JSON.
fn peso(stato: &StatoSessione) -> usize {
    serde_json::to_vec(&Messaggio::Stato(stato.clone())).map_or(usize::MAX, |v| v.len())
}

/// Quanti byte pesa una voce dentro la coda dello stato.
fn peso_della_voce(voce: &VoceSulFilo) -> usize {
    serde_json::to_vec(voce).map_or(usize::MAX, |v| v.len())
}

/// Il peso dello stato fatto coi pesi delle sue voci, senza serializzarlo
/// tutto: lo stato con la coda vuota, più ogni voce, più le virgole fra una e
/// l'altra. È esatto — in un array JSON una voce si scrive come da sola — e
/// costa una serializzazione per voce invece di una per ogni voce tolta.
fn peso_a_pezzi(stato: &mut StatoSessione, pesi: &[usize]) -> usize {
    let coda = std::mem::take(&mut stato.coda);
    let senza_voci = peso(stato);
    stato.coda = coda;
    pesi.iter()
        .fold(senza_voci, |somma, p| somma.saturating_add(*p))
        .saturating_add(pesi.len().saturating_sub(1))
}

/// Fa stare lo stato in [`BYTE_MASSIMI_STATO`]: prima i testi più corti, poi
/// meno voci attorno alla corrente. Vedi il `//!`.
///
/// Lineare nel numero di voci: ogni voce si pesa una volta, e togliendone una
/// si sottrae il suo peso. Prima si riserializzava lo stato intero a ogni voce
/// tolta, e una coda di mille voci con titoli lunghi costava centinaia di
/// serializzazioni da quasi un megabyte nel filo della sessione.
pub fn nel_tetto(stato: &mut StatoSessione) {
    let mut pesi: Vec<usize> = stato.coda.iter().map(peso_della_voce).collect();
    if peso_a_pezzi(stato, &pesi) <= BYTE_MASSIMI_STATO {
        return;
    }
    for voce in &mut stato.coda {
        for campo in [&mut voce.titolo, &mut voce.artista, &mut voce.album] {
            *campo = testo(campo, 80);
        }
    }
    pesi = stato.coda.iter().map(peso_della_voce).collect();
    let mut totale = peso_a_pezzi(stato, &pesi);
    while totale > BYTE_MASSIMI_STATO && stato.coda.len() > 1 {
        let posto = stato
            .corrente
            .and_then(|c| stato.coda.iter().position(|v| v.uid == c))
            .unwrap_or(0);
        // Si toglie dal capo più lontano dalla corrente: il futuro vicino e
        // il passato appena suonato sono quel che si vede nel pannello.
        let dopo = stato.coda.len().saturating_sub(posto);
        let tolto = if dopo > posto {
            stato.coda.pop();
            pesi.pop()
        } else {
            stato.coda.remove(0);
            (!pesi.is_empty()).then(|| pesi.remove(0))
        };
        // La voce e la virgola che la separava dalle altre.
        totale = totale.saturating_sub(tolto.unwrap_or(0).saturating_add(1));
    }
}

// ── chi segue ───────────────────────────────────────────────────────────────

/// Il nome del proprietario di una voce, per chi segue: `None` se è di questo
/// PC (fase 2), altrimenti il nome che dichiara nello stato.
#[must_use]
pub fn proprietario(persone: &[Persona], di: u32, io: u32) -> Option<String> {
    if di == io {
        return None;
    }
    persone
        .iter()
        .find(|p| p.membro == di)
        .map(|p| p.nome.clone())
}

/// Chi segue: la voce del suo lettore per una voce arrivata dall'host.
///
/// `apri` costruisce chi legge i byte di un gettone; si chiama solo per le
/// voci che stanno in un file. `copertina` dice se i byte della copertina sono
/// già in memoria: finché non ci sono la voce resta senza, e la si rimette
/// quando arrivano — una chiave che risponde «non c'è» la finestra non la
/// richiede più.
pub fn voce_da_filo(
    v: &VoceSulFilo,
    proprietario: Option<String>,
    copertina: bool,
    apri: &dyn Fn(Gettone) -> ApriInRete,
) -> Voce {
    let apertura = match &v.sorgente {
        SorgenteSulFilo::File {
            gettone,
            estensione,
            segmento,
            ..
        } => Apertura::Rete {
            apri: apri(*gettone),
            estensione: estensione.clone(),
            segmento: segmento.map(|s| (s.inizio_ms, s.fine_ms)),
            immagine: segmento.map(|_| format!("insieme-{}", gettone.impronta())),
            copia: v.copia.clone(),
        },
        SorgenteSulFilo::Catalogo { riferimento } => Apertura::Catalogo {
            riferimento: riferimento.clone(),
        },
        // Una sorgente che questa versione non sa aprire, da un host più
        // nuovo: la voce resta in coda, col suo titolo, e aprirla dice
        // `playback.sourceUnavailable` come un riferimento che non porta a
        // niente. Non la si scarta: lo specchio deve avere le stesse posizioni
        // della coda dell'host.
        SorgenteSulFilo::Sconosciuta => Apertura::Catalogo {
            riferimento: String::new(),
        },
    };
    Voce {
        uid: v.uid,
        proprietario,
        scheda: SchedaVoce {
            titolo: v.titolo.clone(),
            artista: v.artista.clone(),
            album: v.album.clone(),
            durata_ms: v.durata_ms,
            numero: v.numero,
            anno: v.anno,
            copertina: (copertina && v.copertina.is_some()).then(|| chiave_copertina(v.uid)),
            replaygain_db: v.replaygain_db,
            picco: v.picco,
        },
        apertura,
        // Le voci altrui non contano e non fanno scrobble (fase 1 e 2); una
        // copia identica verificata (fase 3) lo deciderà il lettore.
        da_contare: None,
    }
}

/// Chi segue: una voce dello stato che è sua (l'ha aggiunta lui). Si suona
/// dal suo file e conta come suo ascolto: `Apertura::Libreria` con la sua
/// riga, e la copertina è quella della sua libreria.
#[must_use]
pub fn voce_propria(v: &VoceSulFilo, track_id: i64, copertina: Option<String>) -> Voce {
    Voce {
        uid: v.uid,
        proprietario: None,
        scheda: SchedaVoce {
            titolo: v.titolo.clone(),
            artista: v.artista.clone(),
            album: v.album.clone(),
            durata_ms: v.durata_ms,
            numero: v.numero,
            anno: v.anno,
            copertina,
            replaygain_db: v.replaygain_db,
            picco: v.picco,
        },
        apertura: Apertura::Libreria { track_id },
        da_contare: Some(track_id),
    }
}

/// Le copertine arrivate dalla rete, in memoria, per una sessione.
///
/// **Mai** nell'archivio delle copertine della libreria: la pulizia delle
/// orfane le cancellerebbe, e intanto sporcherebbero la libreria con immagini
/// di brani che non ha.
#[derive(Debug, Default)]
pub struct Copertine {
    per_gettone: HashMap<Gettone, Arc<[u8]>>,
    per_voce: HashMap<u64, Gettone>,
    occupati: usize,
}

impl Copertine {
    /// I byte per una chiave `insieme-<uid>`.
    #[must_use]
    pub fn byte(&self, chiave: &str) -> Option<Arc<[u8]>> {
        let uid = uid_della_chiave(chiave)?;
        let gettone = self.per_voce.get(&uid)?;
        self.per_gettone.get(gettone).cloned()
    }

    /// I byte di quel gettone ci sono già.
    #[must_use]
    pub fn ha(&self, gettone: &Gettone) -> bool {
        self.per_gettone.contains_key(gettone)
    }

    /// Ricorda di quale gettone è la copertina di una voce.
    pub fn collega(&mut self, uid: u64, gettone: Gettone) {
        self.per_voce.insert(uid, gettone);
    }

    /// Mette i byte di una copertina, se c'è posto. Vero se li ha tenuti.
    pub fn metti(&mut self, gettone: Gettone, byte: Arc<[u8]>) -> bool {
        if self.ha(&gettone) {
            return true;
        }
        let dopo = self.occupati.saturating_add(byte.len());
        if dopo > TETTO_COPERTINE {
            return false;
        }
        self.occupati = dopo;
        self.per_gettone.insert(gettone, byte);
        true
    }

    /// Dimentica le voci che non ci sono più, e i byte che nessuna usa. Chi
    /// segue, a ogni stato. Restituisce i gettoni che nessuna voce usa più:
    /// vedi [`Copertine::dimentica`].
    pub fn tieni(&mut self, uids: &HashSet<u64>) -> Vec<Gettone> {
        let via: Vec<u64> = self
            .per_voce
            .keys()
            .filter(|uid| !uids.contains(uid))
            .copied()
            .collect();
        self.dimentica(&via)
    }

    /// Dimentica queste voci, e i byte che nessuna voce usa più. Chi ospita,
    /// per le voci dei membri uscite dalla coda, di un membro uscito, di
    /// un'aggiunta rifiutata o rimasta fuori: prima nessuno lo faceva, e in
    /// una serata lunga, raggiunto [`TETTO_COPERTINE`], le voci aggiunte dopo
    /// restavano senza copertina.
    ///
    /// Restituisce i gettoni delle voci tolte che nessuna voce usa più: chi li
    /// aveva già chiesti li toglie dalle chieste, così una voce che torna con
    /// lo stesso gettone (un membro che riaggiunge lo stesso disco, una voce
    /// che rientra nella finestra di chi segue) li richiede invece di restare
    /// senza.
    pub fn dimentica(&mut self, uids: &[u64]) -> Vec<Gettone> {
        let tolti: Vec<Gettone> = uids
            .iter()
            .filter_map(|uid| self.per_voce.remove(uid))
            .collect();
        let usati: HashSet<Gettone> = self.per_voce.values().copied().collect();
        let mut liberati = 0_usize;
        self.per_gettone.retain(|g, byte| {
            let tieni = usati.contains(g);
            if !tieni {
                liberati = liberati.saturating_add(byte.len());
            }
            tieni
        });
        self.occupati = self.occupati.saturating_sub(liberati);
        let mut senza_voci: Vec<Gettone> = Vec::new();
        for gettone in tolti {
            if !usati.contains(&gettone) && !senza_voci.contains(&gettone) {
                senza_voci.push(gettone);
            }
        }
        senza_voci
    }

    /// Fine della sessione.
    pub fn svuota(&mut self) {
        *self = Self::default();
    }
}

/// Una copertina arrivata dalla rete, pronta per la finestra: decodificata
/// dentro i limiti di `rymembra_app::covers` e ricodificata in una JPEG di
/// Rymembra (al più 640 pixel). Alla finestra non arriva mai un byte scelto
/// dall'altro PC: un'immagine che non si decodifica entro quei limiti — una
/// bomba da diecimila pixel per lato, o spazzatura — resta fuori, e la voce
/// senza copertina. Si chiama fuori da ogni lucchetto: è lavoro di CPU.
#[must_use]
pub fn copertina_accettata(byte: &[u8]) -> Option<Arc<[u8]>> {
    rymembra_app::covers::ricodifica_estranea(byte)
        .ok()
        .map(Arc::from)
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::riproduzione::insieme::FileDellaVoce;
    use rymembra_insieme::messaggi::Diritti;

    fn scheda(titolo: &str) -> SchedaVoce {
        SchedaVoce {
            titolo: titolo.to_owned(),
            artista: "Artista".to_owned(),
            album: "Album".to_owned(),
            durata_ms: 200_000,
            numero: Some(3),
            anno: Some(1977),
            copertina: Some("abcd".to_owned()),
            replaygain_db: Some(-6.5),
            picco: Some(0.98),
        }
    }

    fn brano(id: i64, percorso: &str, segmento: Option<(u64, Option<u64>)>) -> BranoDiQuesto {
        BranoDiQuesto {
            track_id: id,
            scheda: scheda("Dogs"),
            file: Some(FileDellaVoce {
                percorso: PathBuf::from(percorso),
                dimensione: 1_000,
                modificato_ms: 5,
                estensione: Some("FLAC".to_owned()),
                segmento,
            }),
            riferimento: None,
            copertina: Some("abcd".to_owned()),
        }
    }

    fn generatore() -> impl FnMut() -> Result<Gettone, AppError> {
        let mut n = 0_u8;
        move || {
            n = n.wrapping_add(1);
            Ok(Gettone([n; 16]))
        }
    }

    fn valida(stato: &StatoSessione) {
        Messaggio::Stato(stato.clone())
            .valida()
            .expect("lo stato passa i tetti di chi lo riceve");
    }

    #[test]
    fn i_testi_ostili_diventano_testi_che_passano_valida() {
        assert_eq!(testo("  Dogs\n(live)\t ", 512), "Dogs (live)");
        assert_eq!(
            testo(&"é".repeat(600), MASSIMO_TESTO).chars().count(),
            MASSIMO_TESTO
        );
        let mut s = scheda(&format!("{}\u{7}\r\n", "x".repeat(700)));
        s.durata_ms = u64::MAX;
        s.replaygain_db = Some(f32::NAN);
        s.picco = Some(40.0);
        let voce = descrivi(
            1,
            &s,
            SorgenteSulFilo::Catalogo {
                riferimento: "jamendo:1".to_owned(),
            },
            None,
        );
        assert_eq!(voce.durata_ms, MASSIMA_DURATA_MS);
        assert_eq!(voce.replaygain_db, None);
        assert_eq!(voce.picco, None);
        valida(&StatoSessione {
            versione: 1,
            persone: vec![],
            coda: vec![voce],
            corrente: None,
            in_pausa: false,
            ripeti: Ripeti::No,
        });
    }

    #[test]
    fn le_voci_di_un_immagine_cd_condividono_un_gettone_solo() {
        let mut servite = Servite::default();
        let mut nuovo = generatore();
        let primi = servite
            .aggiungi(
                1,
                &brano(1, r"C:\m\a.flac", Some((0, Some(1_000)))),
                &mut nuovo,
            )
            .unwrap();
        let secondi = servite
            .aggiungi(
                2,
                &brano(2, r"C:\m\a.flac", Some((1_000, None))),
                &mut nuovo,
            )
            .unwrap();
        // Il primo dà il file, il suo descrittore da fare e la copertina; il
        // secondo niente di nuovo.
        assert_eq!(primi.len(), 3, "{primi:?}");
        assert!(
            primi
                .iter()
                .any(|s| matches!(s, Servizio::Descrivi { track_id: 1, .. }))
        );
        assert!(secondi.is_empty(), "{secondi:?}");
        let gettone = |uid| match &servite.voce(uid).unwrap().sorgente {
            SorgenteSulFilo::File { gettone, .. } => *gettone,
            SorgenteSulFilo::Catalogo { .. } | SorgenteSulFilo::Sconosciuta => panic!(),
        };
        assert_eq!(gettone(1), gettone(2));
        // Tolta una delle due, il gettone resta; tolte tutte e due, si revoca.
        assert!(servite.tieni(&HashSet::from([2])).is_empty());
        let revoche = servite.tieni(&HashSet::new());
        assert_eq!(revoche.len(), 2, "file e copertina: {revoche:?}");
        assert!(revoche.iter().all(|s| matches!(s, Servizio::Revoca(_))));
    }

    #[test]
    fn un_file_cambiato_ha_un_gettone_nuovo() {
        let mut servite = Servite::default();
        let mut nuovo = generatore();
        servite
            .aggiungi(1, &brano(1, r"C:\m\a.flac", None), &mut nuovo)
            .unwrap();
        let mut cambiato = brano(2, r"C:\m\a.flac", None);
        if let Some(file) = cambiato.file.as_mut() {
            file.modificato_ms = 99;
        }
        let servizi = servite.aggiungi(2, &cambiato, &mut nuovo).unwrap();
        assert!(
            matches!(
                servizi.as_slice(),
                [Servizio::File(..), Servizio::Descrivi { .. }]
            ),
            "{servizi:?}"
        );
    }

    #[test]
    fn un_brano_senza_byte_ne_riferimento_non_si_descrive() {
        let mut servite = Servite::default();
        let mut b = brano(1, "x", None);
        b.file = None;
        assert!(
            servite
                .aggiungi(1, &b, &mut generatore())
                .unwrap()
                .is_empty()
        );
        assert!(!servite.ha(1));
        b.riferimento = Some("a\nb".to_owned());
        assert!(
            servite
                .aggiungi(1, &b, &mut generatore())
                .unwrap()
                .is_empty()
        );
        assert!(
            !servite.ha(1),
            "un riferimento con un a capo non si accorcia: si salta"
        );
    }

    #[test]
    fn la_estensione_e_il_tratto_stanno_nei_tetti() {
        assert_eq!(estensione(".FLAC").as_deref(), Some("flac"));
        assert_eq!(estensione("m p3"), None);
        assert_eq!(estensione(&"a".repeat(17)), None);
        assert_eq!(
            segmento((5_000, Some(1_000))),
            Some(SegmentoSulFilo {
                inizio_ms: 5_000,
                fine_ms: None
            })
        );
        assert_eq!(segmento((MASSIMA_DURATA_MS + 1, None)), None);
    }

    #[test]
    fn una_coda_di_milleduecento_va_sul_filo_attorno_alla_corrente() {
        let coda: Vec<u64> = (1..=1_200).collect();
        let f = finestra(&coda, Some(900));
        assert_eq!(f.len(), MASSIMO_VOCI);
        assert_eq!(f.first(), Some(&201), "{:?}", f.first());
        assert!(f.contains(&900));
        let f = finestra(&coda, Some(10));
        assert_eq!(f.first(), Some(&1));
        assert_eq!(finestra(&coda[..5], Some(3)), &coda[..5]);
    }

    #[test]
    fn mille_voci_dai_titoli_lunghi_stanno_nella_cornice() {
        let mut servite = Servite::default();
        let mut nuovo = generatore();
        let mut b = brano(1, "x", None);
        // Caratteri di tre byte, al tetto: 1000 × 3 × 512 × 3 byte sono più di
        // quattro megabyte senza accorciare niente.
        b.scheda.titolo = "語".repeat(600);
        b.scheda.artista = "語".repeat(600);
        b.scheda.album = "語".repeat(600);
        for uid in 1..=1_200 {
            if let Some(file) = b.file.as_mut() {
                file.percorso = PathBuf::from(format!("f{uid}"));
            }
            servite.aggiungi(uid, &b, &mut nuovo).unwrap();
        }
        let istantanea = Istantanea {
            coda: (1..=1_200).collect(),
            corrente: Some(600),
            in_pausa: false,
            ripeti: RepeatMode::All,
        };
        let stato = stato_sessione(7, vec![], &istantanea, &servite);
        valida(&stato);
        assert!(peso(&stato) <= BYTE_MASSIMI_STATO);
        assert!(stato.coda.iter().any(|v| v.uid == 600), "la corrente resta");
        assert!(stato.coda.len() >= 500, "{}", stato.coda.len());
        assert_eq!(stato.ripeti, Ripeti::Tutta);
    }

    #[test]
    fn il_peso_fatto_a_pezzi_e_quello_vero_e_il_tetto_non_toglie_troppo() {
        let mut servite = Servite::default();
        let mut nuovo = generatore();
        let mut b = brano(1, "x", None);
        for uid in 1..=1_000 {
            // Testi di lunghezze e alfabeti diversi, e virgolette da
            // scappare: il conto deve tornare al byte.
            b.scheda.titolo = format!(
                "«{}» \"{uid}\"",
                "語".repeat(usize::try_from(uid % 300).unwrap())
            );
            b.scheda.artista = "a".repeat(usize::try_from(uid % 97).unwrap());
            if let Some(file) = b.file.as_mut() {
                file.percorso = PathBuf::from(format!("f{uid}"));
            }
            servite.aggiungi(uid, &b, &mut nuovo).unwrap();
        }
        let istantanea = Istantanea {
            coda: (1..=1_000).collect(),
            corrente: Some(900),
            in_pausa: false,
            ripeti: RepeatMode::All,
        };
        let mut intero = StatoSessione {
            versione: 1,
            persone: vec![],
            coda: (1..=1_000)
                .filter_map(|uid| servite.voce(uid).cloned())
                .collect(),
            corrente: Some(900),
            in_pausa: false,
            ripeti: Ripeti::Tutta,
        };
        let pesi: Vec<usize> = intero.coda.iter().map(peso_della_voce).collect();
        assert_eq!(peso_a_pezzi(&mut intero, &pesi), peso(&intero));
        assert_eq!(intero.coda.len(), 1_000, "pesare non toglie niente");

        let stato = stato_sessione(1, vec![], &istantanea, &servite);
        assert!(peso(&stato) <= BYTE_MASSIMI_STATO);
        assert!(stato.coda.iter().any(|v| v.uid == 900), "la corrente resta");
        // Non ne ha tolte più del necessario: con la prossima voce lontana
        // dalla corrente rimessa, il tetto sarebbe superato.
        if stato.coda.len() < 1_000 {
            let primo = stato.coda.first().unwrap().uid;
            let ultimo = stato.coda.last().unwrap().uid;
            let mut con_una_in_piu = stato.clone();
            if primo > 1 {
                con_una_in_piu
                    .coda
                    .insert(0, servite.voce(primo - 1).unwrap().clone());
            } else {
                con_una_in_piu
                    .coda
                    .push(servite.voce(ultimo + 1).unwrap().clone());
            }
            assert!(peso(&con_una_in_piu) > BYTE_MASSIMI_STATO);
        }
    }

    #[test]
    fn da_voce_sul_filo_a_voce_del_lettore_e_ritorno() {
        let mut servite = Servite::default();
        servite
            .aggiungi(
                7,
                &brano(7, r"\\nas\m\a.cue", Some((60_000, Some(120_000)))),
                &mut generatore(),
            )
            .unwrap();
        let sul_filo = servite.voce(7).unwrap().clone();
        let persone = vec![
            Persona {
                membro: 0,
                nome: "Sala".to_owned(),
                diritti: Diritti::Tutto,
            },
            Persona {
                membro: 2,
                nome: "Io".to_owned(),
                diritti: Diritti::Tutto,
            },
        ];
        let aperte = std::cell::Cell::new(0);
        let apri = |_g: Gettone| -> ApriInRete {
            aperte.set(aperte.get() + 1);
            Arc::new(|| {
                Err(AppError::new(
                    rymembra_domain::errors::ErrorCode::InsiemeGettoneScaduto,
                ))
            })
        };
        let voce = voce_da_filo(
            &sul_filo,
            proprietario(&persone, sul_filo.di, 2),
            true,
            &apri,
        );
        assert_eq!(aperte.get(), 1);
        assert_eq!(voce.uid, 7);
        assert_eq!(voce.proprietario.as_deref(), Some("Sala"));
        assert_eq!(voce.da_contare, None, "le voci altrui non contano");
        assert_eq!(voce.scheda.copertina.as_deref(), Some("insieme-7"));
        assert_eq!(voce.scheda.titolo, "Dogs");
        assert_eq!(voce.scheda.durata_ms, 200_000);
        match &voce.apertura {
            Apertura::Rete {
                estensione,
                segmento,
                ..
            } => {
                assert_eq!(estensione.as_deref(), Some("flac"));
                assert_eq!(*segmento, Some((60_000, Some(120_000))));
            }
            _ => panic!("un file dell'host si apre in rete"),
        }
        // E ridescritta da qui, è la stessa voce sul filo.
        let SorgenteSulFilo::File { gettone, .. } = &sul_filo.sorgente else {
            panic!()
        };
        let indietro = descrivi(
            7,
            &voce.scheda,
            sul_filo.sorgente.clone(),
            sul_filo.copertina,
        );
        assert_eq!(indietro, sul_filo);
        assert_ne!(Some(*gettone), sul_filo.copertina);
        // Senza i byte della copertina, niente chiave.
        let senza = voce_da_filo(&sul_filo, None, false, &apri);
        assert_eq!(senza.scheda.copertina, None);
    }

    #[test]
    fn un_brano_di_catalogo_lo_apre_ogni_pc_dal_suo_catalogo() {
        let v = descrivi(
            3,
            &scheda("x"),
            SorgenteSulFilo::Catalogo {
                riferimento: "archive:gd77/t01".to_owned(),
            },
            None,
        );
        let apri = |_g: Gettone| -> ApriInRete { panic!("niente rete per un catalogo") };
        let voce = voce_da_filo(&v, None, true, &apri);
        assert!(
            matches!(&voce.apertura, Apertura::Catalogo { riferimento } if riferimento == "archive:gd77/t01")
        );
        assert_eq!(
            voce.scheda.copertina, None,
            "nessun gettone, nessuna chiave"
        );
    }

    #[test]
    fn le_chiavi_delle_copertine_si_leggono_solo_intere() {
        assert_eq!(uid_della_chiave(&chiave_copertina(42)), Some(42));
        assert_eq!(uid_della_chiave("insieme-+5"), None);
        assert_eq!(uid_della_chiave("insieme-"), None);
        assert_eq!(uid_della_chiave("3fa2c0"), None);
    }

    #[test]
    fn la_cache_delle_copertine_ha_un_tetto_e_si_pulisce() {
        let mut c = Copertine::default();
        let g = Gettone([1; 16]);
        c.collega(5, g);
        assert!(c.metti(g, Arc::from(vec![0_u8; 10])));
        assert_eq!(c.byte("insieme-5").map(|b| b.len()), Some(10));
        assert!(!c.metti(Gettone([2; 16]), Arc::from(vec![0_u8; TETTO_COPERTINE])));
        c.tieni(&HashSet::new());
        assert_eq!(c.byte("insieme-5"), None);
        assert_eq!(c.occupati, 0);
    }

    #[test]
    fn l_host_libera_le_copertine_delle_voci_uscite() {
        let mut c = Copertine::default();
        let (g1, g2, g3) = (Gettone([1; 16]), Gettone([2; 16]), Gettone([3; 16]));
        // Il tetto raggiunto dalle voci dei membri: una nuova resta senza.
        c.collega(1, g1);
        assert!(c.metti(g1, Arc::from(vec![0_u8; TETTO_COPERTINE - 10])));
        c.collega(2, g2);
        assert!(!c.metti(g2, Arc::from(vec![0_u8; 100])));
        // La voce 1 esce dalla coda: il suo posto si libera, e il gettone
        // torna a chi lo vuole richiedere.
        assert_eq!(c.dimentica(&[1]), vec![g1]);
        assert_eq!(c.byte("insieme-1"), None);
        assert_eq!(c.occupati, 0);
        assert!(c.metti(g2, Arc::from(vec![0_u8; 100])));
        assert_eq!(c.byte("insieme-2").map(|b| b.len()), Some(100));
        // Un gettone che un'altra voce usa ancora resta, coi suoi byte.
        c.collega(5, g3);
        c.collega(6, g3);
        assert!(c.metti(g3, Arc::from(vec![0_u8; 10])));
        assert!(c.dimentica(&[5]).is_empty());
        assert_eq!(c.byte("insieme-6").map(|b| b.len()), Some(10));
        // `tieni` è la stessa cosa vista da chi segue.
        assert_eq!(c.tieni(&HashSet::from([2])), vec![g3]);
        assert_eq!(c.occupati, 100);
    }

    #[test]
    fn una_copertina_arrivata_dalla_rete_che_non_e_un_immagine_resta_fuori() {
        assert!(copertina_accettata(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(copertina_accettata(&[0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF]).is_none());
        assert!(copertina_accettata(&vec![0_u8; BYTE_MASSIMI_COPERTINA + 1]).is_none());
    }

    fn del_membro(uid: u64, file: u8, copertina: Option<u8>, di: u32) -> VoceSulFilo {
        let mut v = descrivi(
            uid,
            &scheda("Brano di Marco"),
            SorgenteSulFilo::File {
                gettone: Gettone([file; 16]),
                estensione: Some("mp3".to_owned()),
                dimensione: 10,
                segmento: None,
            },
            copertina.map(|c| Gettone([c; 16])),
        );
        v.di = di;
        v
    }

    #[test]
    fn i_gettoni_di_un_membro_si_inoltrano_una_volta_e_si_dimenticano_con_l_ultima_voce() {
        let mut servite = Servite::default();
        let primi = servite.ospita_voce(del_membro(1, 7, Some(8), 2)).unwrap();
        assert_eq!(
            primi,
            vec![
                Servizio::Inoltra(Gettone([7; 16]), 2),
                Servizio::Inoltra(Gettone([8; 16]), 2)
            ]
        );
        // Lo stesso file e la stessa copertina: niente di nuovo da inoltrare.
        assert!(
            servite
                .ospita_voce(del_membro(2, 7, Some(8), 2))
                .unwrap()
                .is_empty()
        );
        servite.ospita_voce(del_membro(3, 9, None, 3)).unwrap();
        let mut suoi = servite.di_membro(2);
        suoi.sort_unstable();
        assert_eq!(suoi, vec![1, 2]);
        assert_eq!(servite.di_membro(0), Vec::<u64>::new(), "lo 0 è chi ospita");
        assert!(servite.dimentica(&[1]).is_empty(), "la 2 li usa ancora");
        let revoche = servite.dimentica(&[2]);
        assert_eq!(revoche.len(), 2, "{revoche:?}");
        assert!(revoche.iter().all(|s| matches!(s, Servizio::Revoca(_))));
        let altrui: Vec<(u64, u32)> = servite.altrui().collect();
        assert_eq!(altrui, vec![(3, 3)]);
    }

    #[test]
    fn un_membro_non_puo_usare_il_gettone_di_un_altro() {
        let mut servite = Servite::default();
        // Un brano di chi ospita: gettone [1; 16] per il file, [2; 16] per la
        // copertina (vedi `generatore`).
        servite
            .aggiungi(1, &brano(1, r"C:\m\a.flac", None), &mut generatore())
            .unwrap();
        servite.ospita_voce(del_membro(10, 7, None, 2)).unwrap();
        assert_eq!(
            servite.ospita_voce(del_membro(11, 1, None, 3)),
            Err(GettoneAltrui),
            "il file di chi ospita"
        );
        assert_eq!(
            servite.ospita_voce(del_membro(12, 9, Some(2), 3)),
            Err(GettoneAltrui),
            "la copertina di chi ospita"
        );
        assert_eq!(
            servite.ospita_voce(del_membro(13, 7, None, 3)),
            Err(GettoneAltrui),
            "il gettone inoltrato a un altro membro"
        );
        assert!(!servite.ha(11) && !servite.ha(12) && !servite.ha(13));
        // Lo stesso membro con lo stesso gettone va bene.
        assert!(servite.ospita_voce(del_membro(14, 7, None, 2)).is_ok());
    }

    #[test]
    fn chi_segue_riconosce_le_sue_voci_dal_gettone_anche_rinumerate() {
        let mut proprie = Servite::default();
        proprie
            .aggiungi(1, &brano(42, r"C:\m\mia.flac", None), &mut generatore())
            .unwrap();
        let mia = proprie.voce(1).unwrap().clone();
        // L'host la rimanda con un altro `uid`, lo stesso gettone.
        let tornata = VoceSulFilo {
            uid: 1_099_511_627_777,
            di: 3,
            ..mia
        };
        assert_eq!(
            proprie.propria(&tornata),
            Some((42, Some("abcd".to_owned())))
        );
        assert_eq!(proprie.propria(&del_membro(5, 99, None, 3)), None);
        let voce = voce_propria(&tornata, 42, Some("abcd".to_owned()));
        assert!(matches!(voce.apertura, Apertura::Libreria { track_id: 42 }));
        assert_eq!(voce.da_contare, Some(42), "conta come suo ascolto");
        assert_eq!(voce.proprietario, None);
        assert_eq!(
            voce.scheda.copertina.as_deref(),
            Some("abcd"),
            "la sua copertina"
        );
    }

    fn descrittore() -> DescrittoreCopia {
        DescrittoreCopia {
            unc: None,
            dimensione: 1_000,
            modificato_ms: 5,
            impronta_rapida: "ab".repeat(32),
            chiave_contenuto: None,
        }
    }

    #[test]
    fn il_descrittore_arrivato_dopo_va_sulle_voci_del_suo_file_e_su_quelle_future() {
        let mut servite = Servite::default();
        let mut nuovo = generatore();
        servite
            .aggiungi(1, &brano(1, r"C:\m\a.flac", None), &mut nuovo)
            .unwrap();
        assert!(!servite.descritta(1));
        assert_eq!(servite.voce(1).unwrap().copia, None, "parte senza");
        let chiave = ChiaveFile {
            percorso: PathBuf::from(r"C:\m\a.flac"),
            dimensione: 1_000,
            modificato_ms: 5,
        };
        assert!(servite.imposta_copia(&chiave, Some(descrittore())));
        assert!(servite.descritta(1));
        assert_eq!(servite.voce(1).unwrap().copia, Some(descrittore()));
        assert!(
            !servite.imposta_copia(&chiave, Some(descrittore())),
            "niente di nuovo"
        );
        // Una voce dopo, dello stesso file: col descrittore, e senza chiederlo.
        let servizi = servite
            .aggiungi(2, &brano(2, r"C:\m\a.flac", None), &mut nuovo)
            .unwrap();
        assert!(
            !servizi
                .iter()
                .any(|s| matches!(s, Servizio::Descrivi { .. }))
        );
        assert_eq!(servite.voce(2).unwrap().copia, Some(descrittore()));
        // Lo stato che ne viene passa i tetti.
        let stato = stato_sessione(
            1,
            vec![],
            &Istantanea {
                coda: vec![1, 2],
                corrente: Some(1),
                in_pausa: false,
                ripeti: RepeatMode::Off,
            },
            &servite,
        );
        valida(&stato);
    }

    #[test]
    fn un_descrittore_che_non_viene_non_si_richiede_e_non_fa_aspettare() {
        let mut servite = Servite::default();
        let mut nuovo = generatore();
        servite
            .aggiungi(1, &brano(1, r"C:\m\a.flac", None), &mut nuovo)
            .unwrap();
        let chiave = ChiaveFile {
            percorso: PathBuf::from(r"C:\m\a.flac"),
            dimensione: 1_000,
            modificato_ms: 5,
        };
        assert!(!servite.imposta_copia(&chiave, None));
        assert!(servite.descritta(1));
        assert_eq!(servite.voce(1).unwrap().copia, None);
    }

    #[test]
    fn il_descrittore_della_libreria_passa_sul_filo_solo_nei_tetti() {
        let buono = rymembra_app::insieme::Descrittore {
            unc: Some(r"\\nas\m\a.flac".to_owned()),
            dimensione: 1,
            modificato_ms: 2,
            impronta_rapida: "f".repeat(64),
            chiave_contenuto: Some("ck".to_owned()),
        };
        let d = descrittore_sul_filo(buono.clone()).unwrap();
        assert_eq!(d.unc.as_deref(), Some(r"\\nas\m\a.flac"));
        assert_eq!(d.chiave_contenuto.as_deref(), Some("ck"));
        let locale = rymembra_app::insieme::Descrittore {
            unc: Some(r"C:\m\a.flac".to_owned()),
            ..buono.clone()
        };
        assert_eq!(
            descrittore_sul_filo(locale).unwrap().unc,
            None,
            "solo condivisioni"
        );
        let storto = rymembra_app::insieme::Descrittore {
            impronta_rapida: "f".repeat(65),
            ..buono
        };
        assert_eq!(descrittore_sul_filo(storto), None);
    }
}
