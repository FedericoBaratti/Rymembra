//! La portabilità: la stessa musica, un'altra lettera di unità.
//!
//! # Il caso normale, non quello raro
//!
//! Un profilo si esporta su un computer e si importa su un altro. Le
//! preferenze attraversano senza problemi — un tema è un tema — ma i
//! **percorsi** no: `D:\Musica` sull'altra macchina è la lettera di un disco
//! che non c'è, o c'è e contiene altro. Fino alla 2.3.0 il profilo lo diceva e
//! basta: scriveva le radici così com'erano, avvisava che non esistevano, e
//! lasciava a chi importava il compito di riscriverle a mano nelle
//! impostazioni.
//!
//! Bastava finché il profilo portava solo le preferenze. Adesso porta anche la
//! libreria, e la libreria è fatta di `tracks.path`: diciottomila righe che
//! nominano `D:\Musica\…`. Riscrivere le due radici a mano e lasciare
//! diciottomila percorsi che puntano a un disco che non c'è vorrebbe dire una
//! libreria intera che non suona, con la scansione che la ricostruisce da capo
//! perdendo l'abbinamento con tutto quello che il profilo ha appena portato.
//!
//! # Cosa fa una rimappatura, e cosa non fa
//!
//! Dice «quel che stava sotto *questo* prefisso adesso sta sotto *quello*», e
//! si applica in tre posti: `library.roots`, `download.folder` e, brano per
//! brano, `tracks.path`.
//!
//! **Solo quando il file nuovo esiste davvero.** Un percorso riscritto verso un
//! file che non c'è è peggio di uno vecchio che non c'è: il vecchio dice la
//! verità su dov'era, il nuovo mente su dov'è. I brani che non si ritrovano si
//! **contano** e si dicono, e la loro riga resta esattamente com'era.
//!
//! **Nessuna riga si cancella.** Vale qui la stessa regola di tutto il resto
//! dell'importazione: un profilo aggiunge, non toglie.
//!
//! # Perché il confronto passa da `path_key` e non dalle stringhe
//!
//! Perché `D:\Musica`, `d:/musica` e `D:\Musica\` sono lo stesso posto, e un
//! confronto a stringhe li vedrebbe come tre. La chiave è la stessa che usa la
//! scansione per decidere se un file sta dentro una cartella sorvegliata, e
//! usarne una seconda qui vorrebbe dire due idee di «stesso posto» nello stesso
//! programma.
//!
//! Il confine di separatore è tutto il punto, e viene gratis da
//! [`aether_domain::paths::is_under`]: senza, `D:\Musica` conterrebbe
//! `D:\Musical`, e una rimappatura sposterebbe i percorsi di una cartella che
//! non c'entra niente.
//!
//! # «Il file c'è» ha tre risposte, e si chiede fuori dal lucchetto
//!
//! La regola «solo quando il file nuovo esiste davvero» costa una domanda al
//! disco per brano, e su un NAS diciottomila domande sono minuti. Fino alla 3.0
//! si facevano **dentro** la transazione, cioè con il lucchetto della libreria
//! in mano: la finestra si fermava, il lettore non passava al brano dopo, e
//! dopo quattro secondi ogni altro comando rispondeva «database occupato». Con
//! la share spenta ogni domanda aspettava i quaranta secondi di SMB e poi
//! rispondeva «no», cioè «il file non c'è» — che non era vero.
//!
//! Adesso le domande si fanno **prima**, e in parallelo: [`candidati`] legge i
//! percorsi nuovi con il lucchetto preso per un attimo, chi chiama li fa
//! verificare fuori ([`crate::volumi::esistenze`]) e porta le risposte dentro
//! la transazione già pronte. Le risposte sono tre ([`Esistenza`]), e la terza
//! — «non si sa» — non riscrive e non si conta fra gli irrintracciabili: si
//! conta a parte, in [`Riscritti::non_verificati`].
//!
//! # Le cartelle sorvegliate, e le due grafie dello stesso posto
//!
//! La stessa idea — la stessa musica sotto un altro nome — vale anche senza
//! profilo. `Z:\` e `\\nas\musica` possono essere la stessa cartella, e
//! aggiungere la seconda accanto alla prima voleva dire una seconda scansione
//! della stessa musica: ogni brano rientrava come nuovo, con la storia vuota,
//! accanto a quello vecchio. [`ordina_cartelle`] se ne accorge e propone il
//! trasloco — che è una rimappatura, e passa da [`applica_verificati`] — invece
//! di aggiungere il doppione.

use std::collections::HashMap;

use aether_domain::errors::AppError;
use aether_domain::paths::{PathRules, is_under, path_key};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::library::db_error;
use crate::volumi::Esistenza;

/// Una radice del profilo che qui non c'è, e dove metterla.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rimappatura {
    /// Il prefisso come sta nel profilo.
    pub da: String,
    /// Il prefisso su questo computer. Vuoto finché nessuno l'ha scelto.
    pub a: String,
    /// Quanti brani di **questa** libreria stanno sotto `da`.
    ///
    /// È il numero che dà senso alla proposta: zero vuol dire che qui non c'è
    /// niente da spostare e la rimappatura riguarda solo le due preferenze;
    /// diciottomila vuol dire che è tutta la libreria.
    pub brani: usize,
}

/// Cosa una rimappatura ha cambiato.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Riscritti {
    /// Percorsi di brani riscritti, perché il file nuovo c'è.
    pub percorsi: usize,
    /// Brani il cui file, sotto il prefisso nuovo, non si trova.
    ///
    /// La loro riga resta com'era: dire dove **non** sono è meno utile che
    /// continuare a dire dove erano.
    pub irrintracciabili: usize,
    /// Brani il cui percorso nuovo è già di un'altra riga.
    ///
    /// Capita quando la stessa cartella è già stata scansionata dalla parte
    /// nuova: il brano c'è già, con la sua storia, e sovrascriverlo vorrebbe
    /// dire perderne una delle due. `tracks.path` è `UNIQUE`, quindi il
    /// database lo impedirebbe comunque — qui lo si conta invece di lasciare
    /// che faccia fallire tutta l'importazione.
    pub gia_presenti: usize,
    /// Brani il cui file nuovo non si è potuto verificare: la share non
    /// rispondeva, o il tempo è scaduto.
    ///
    /// Non sono irrintracciabili, e contarli insieme sarebbe dire «non c'è» di
    /// un file che forse c'è. La loro riga resta com'era, come per quelli: la
    /// differenza è nella frase, che per loro dice «riprova quando la rete
    /// torna».
    pub non_verificati: usize,
}

/// Le regole di confronto di questa piattaforma.
fn regole() -> PathRules {
    PathRules::for_current_platform()
}

/// Toglie i separatori in coda, senza toccare il resto della grafia.
fn senza_coda(percorso: &str) -> &str {
    let potato = percorso.trim_end_matches(['/', '\\']);
    // Una radice che è **solo** separatori resterebbe vuota, e un prefisso
    // vuoto contiene mezzo filesystem. Meglio lasciarla com'era e lasciare che
    // `is_under` la rifiuti.
    if potato.is_empty() { percorso } else { potato }
}

/// Il percorso nuovo, se questo sta sotto `da`.
///
/// Restituisce `None` quando non ci sta sotto: il chiamante lo lascia com'era.
#[must_use]
pub fn riscrivi(percorso: &str, da: &str, a: &str) -> Option<String> {
    // Lo spazio si toglie prima dei separatori, e non è pedanteria: una
    // destinazione fatta di soli spazi è una proposta che nessuno ha
    // compilato, e senza questa riga passerebbe per un prefisso valido —
    // riscrivendo diciottomila percorsi verso «   \qualcosa».
    let da = senza_coda(da.trim());
    let a = senza_coda(a.trim());
    if a.is_empty() || !is_under(percorso, da, regole()) {
        return None;
    }
    // Il resto si prende dall'**originale** e non dalla chiave: la chiave ha
    // già unificato le barre e piegato le maiuscole, e ricostruire un percorso
    // da lì darebbe un `d:/musica/...` minuscolo con le barre al contrario da
    // aprire su Windows. Si taglia per lunghezza, dopo aver verificato che il
    // taglio cade su un confine di carattere e che il pezzo tagliato è davvero
    // `da`.
    let testa = percorso.get(..da.len())?;
    if path_key(testa, regole()) != path_key(da, regole()) {
        return None;
    }
    let resto = percorso.get(da.len()..)?;
    Some(format!("{a}{resto}"))
}

/// Una riga di proposta per ogni radice del profilo che qui non esiste.
///
/// `esiste` dice se un percorso c'è su questo computer; gliela si passa invece
/// di guardare il disco da qui, così questa funzione si prova senza avere le
/// cartelle di nessuno — è la stessa scelta che [`crate::profilo`] fa da
/// sempre.
///
/// # Errori
///
/// `db.queryFailed` se il conteggio dei brani fallisce.
pub fn proposte(
    connection: &Connection,
    radici: &[String],
    esiste: &dyn Fn(&str) -> bool,
) -> Result<Vec<Rimappatura>, AppError> {
    let mut proposte = Vec::new();
    for radice in radici {
        let radice = radice.trim();
        if radice.is_empty() || esiste(radice) {
            continue;
        }
        proposte.push(Rimappatura {
            da: radice.to_owned(),
            a: String::new(),
            brani: quanti_sotto(connection, radice)?,
        });
    }
    Ok(proposte)
}

/// Quanti brani di questa libreria stanno sotto un prefisso.
///
/// Si legge tutto e si conta in Rust invece di scrivere un `LIKE`: il confronto
/// deve passare da `path_key`, e un `LIKE 'D:\Musica%'` sarebbe insensibile al
/// confine di separatore e alle due grafie della barra — cioè darebbe un numero
/// diverso da quello che la rimappatura poi userà. Una scansione della colonna
/// `path` su diciottomila righe è una decina di millisecondi, una volta, mentre
/// si guarda un piano.
fn quanti_sotto(connection: &Connection, radice: &str) -> Result<usize, AppError> {
    let mut istruzione = connection
        .prepare("SELECT path FROM tracks WHERE path IS NOT NULL")
        .map_err(|err| db_error("conteggio dei brani sotto una radice", &err))?;
    let righe = istruzione
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error("conteggio dei brani sotto una radice", &err))?;
    let mut quanti = 0;
    for riga in righe {
        let percorso = riga.map_err(|err| db_error("lettura di un percorso", &err))?;
        if is_under(&percorso, senza_coda(radice), regole()) {
            quanti += 1;
        }
    }
    Ok(quanti)
}

/// Riscrive un percorso solo, con la prima rimappatura che lo riguarda.
#[must_use]
pub fn rimappa_percorso(percorso: &str, rimappature: &[Rimappatura]) -> String {
    rimappature
        .iter()
        .find_map(|r| riscrivi(percorso, &r.da, &r.a))
        .unwrap_or_else(|| percorso.to_owned())
}

/// Riscrive un valore di preferenza che contiene percorsi.
///
/// Due forme, perché nel database ce ne sono due: `library.roots` è un elenco
/// JSON, `download.folder` è un percorso e basta. La distinzione la fa il
/// valore, non la chiave — vedi `percorsi_di` in [`crate::profilo`], che ha la
/// stessa scelta e la stessa ragione.
#[must_use]
pub fn rimappa_valore(valore: &str, rimappature: &[Rimappatura]) -> String {
    let rifai = |percorso: &str| -> String { rimappa_percorso(percorso, rimappature) };
    match serde_json::from_str::<Vec<String>>(valore) {
        Ok(elenco) => {
            let rifatti: Vec<String> = elenco.iter().map(|p| rifai(p)).collect();
            serde_json::to_string(&rifatti).unwrap_or_else(|_| valore.to_owned())
        }
        Err(_) => rifai(valore),
    }
}

/// Riscrive i percorsi dei brani, **solo quando il file nuovo esiste**.
///
/// `esiste` è la stessa funzione di [`proposte`], e serve qui per la ragione
/// più importante di tutte: senza, una rimappatura sbagliata riscriverebbe
/// diciottomila righe verso un posto vuoto e la libreria diventerebbe un elenco
/// di file che non si aprono. Con, una rimappatura sbagliata non fa niente e lo
/// dice.
///
/// Con due risposte sole: chi sa distinguere «non c'è» da «non si sa» passa da
/// [`applica_verificati`].
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub fn applica_in(
    tx: &Transaction<'_>,
    rimappature: &[Rimappatura],
    esiste: &dyn Fn(&str) -> bool,
) -> Result<Riscritti, AppError> {
    applica_verificati(tx, rimappature, &|percorso| {
        if esiste(percorso) {
            Esistenza::Presente
        } else {
            Esistenza::Assente
        }
    })
}

/// Le rimappature che hanno tutti e due i lati: le altre sono proposte che
/// nessuno ha compilato.
fn vive(rimappature: &[Rimappatura]) -> Vec<&Rimappatura> {
    rimappature
        .iter()
        .filter(|r| !r.da.trim().is_empty() && !r.a.trim().is_empty())
        .collect()
}

/// Le righe da riscrivere: identificativo, percorso nuovo, e la posizione
/// della rimappatura che l'ha prodotto.
fn da_riscrivere(
    connection: &Connection,
    vive: &[&Rimappatura],
) -> Result<Vec<(i64, String, usize)>, AppError> {
    let mut istruzione = connection
        .prepare("SELECT id, path FROM tracks WHERE path IS NOT NULL")
        .map_err(|err| db_error("percorsi da rimappare", &err))?;
    let righe = istruzione
        .query_map([], |riga| {
            Ok((riga.get::<_, i64>(0)?, riga.get::<_, String>(1)?))
        })
        .map_err(|err| db_error("percorsi da rimappare", &err))?;
    let mut trovati = Vec::new();
    for riga in righe {
        let (id, percorso) = riga.map_err(|err| db_error("lettura di un percorso", &err))?;
        if let Some((quale, nuovo)) = vive
            .iter()
            .enumerate()
            .find_map(|(quale, r)| riscrivi(&percorso, &r.da, &r.a).map(|n| (quale, n)))
        {
            trovati.push((id, nuovo, quale));
        }
    }
    Ok(trovati)
}

/// Un percorso nuovo da verificare, e la rimappatura da cui viene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidato {
    /// Il percorso che il brano avrebbe dopo la rimappatura.
    pub percorso: String,
    /// La posizione della rimappatura, fra quelle compilate.
    pub rimappatura: usize,
}

/// I percorsi nuovi che una rimappatura scriverebbe: quelli da verificare.
///
/// Si legge con il lucchetto preso **per un attimo**, e la verifica si fa
/// fuori: vedi il `//!`. È lo stesso elenco che [`applica_verificati`]
/// ricalcola dentro la transazione, quindi una risposta preparata qui trova il
/// suo percorso di là.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn candidati(
    connection: &Connection,
    rimappature: &[Rimappatura],
) -> Result<Vec<Candidato>, AppError> {
    let vive = vive(rimappature);
    if vive.is_empty() {
        return Ok(Vec::new());
    }
    Ok(da_riscrivere(connection, &vive)?
        .into_iter()
        .map(|(_, percorso, rimappatura)| Candidato {
            percorso,
            rimappatura,
        })
        .collect())
}

/// Le risposte del disco sui percorsi nuovi, raccolte prima del lucchetto.
///
/// # Il campione
///
/// Nel **piano** non si verifica tutto: si guarda un campione per ogni
/// rimappatura, e ai percorsi non guardati si dà il verdetto del loro
/// campione. È una stima, e lo dice ([`Self::stima`]). La regola di questo
/// modulo è che il piano sia l'esecuzione annullata e non una previsione, e qui
/// la si piega apposta: l'alternativa è che **guardare** il piano di una
/// libreria su un NAS costi quanto importarla. L'importazione vera verifica
/// ogni file.
///
/// Il verdetto di un campione non è una media: è la risposta che vale per una
/// rimappatura intera. Una destinazione giusta ritrova quasi tutto, una
/// sbagliata quasi niente, una share spenta non risponde su niente — e quel che
/// il piano deve dire è quale dei tre casi è.
#[derive(Debug, Clone, Default)]
pub struct Verifiche {
    /// Le risposte vere, per i percorsi guardati.
    esiti: HashMap<String, Esistenza>,
    /// I percorsi non guardati, con la rimappatura da cui vengono.
    stimati: HashMap<String, usize>,
    /// Il verdetto del campione, per rimappatura.
    verdetti: Vec<Esistenza>,
}

impl Verifiche {
    /// La risposta per un percorso, se è fra i candidati.
    ///
    /// `None` per un percorso che non è un candidato — una radice, una
    /// preferenza — che chi chiama verifica da sé.
    #[must_use]
    pub fn esito(&self, percorso: &str) -> Option<Esistenza> {
        if let Some(esito) = self.esiti.get(percorso) {
            return Some(*esito);
        }
        self.stimati.get(percorso).map(|quale| {
            self.verdetti
                .get(*quale)
                .copied()
                .unwrap_or(Esistenza::Ignota)
        })
    }

    /// Qualche risposta è una stima, non una verifica.
    #[must_use]
    pub fn stima(&self) -> bool {
        !self.stimati.is_empty()
    }
}

/// Verifica i candidati — tutti, o un campione per rimappatura — con la
/// funzione data.
///
/// `controlla` riceve i percorsi da guardare e restituisce le risposte nello
/// stesso ordine: in produzione è [`crate::volumi::esistenze`], nelle prove un
/// doppio che non tocca il disco. `campione` è quanti percorsi guardare per
/// rimappatura; `None` vuol dire tutti.
///
/// Il campione è **distribuito**, non i primi: i percorsi arrivano
/// nell'ordine delle righe, cioè più o meno per data di aggiunta, e i primi
/// cento di una libreria sono spesso la stessa cartella. Uno ogni tanti guarda
/// album diversi.
#[must_use]
pub fn verifica(
    candidati: Vec<Candidato>,
    campione: Option<usize>,
    controlla: &dyn Fn(Vec<String>) -> Vec<Esistenza>,
) -> Verifiche {
    let quante = candidati
        .iter()
        .map(|c| c.rimappatura.saturating_add(1))
        .max()
        .unwrap_or(0);
    let mut per_rimappatura: Vec<Vec<String>> = vec![Vec::new(); quante];
    for candidato in candidati {
        if let Some(gruppo) = per_rimappatura.get_mut(candidato.rimappatura) {
            gruppo.push(candidato.percorso);
        }
    }

    let tetto = campione.unwrap_or(usize::MAX).max(1);
    let mut da_guardare: Vec<String> = Vec::new();
    let mut stimati: HashMap<String, usize> = HashMap::new();
    let mut guardati_di: Vec<std::ops::Range<usize>> = Vec::with_capacity(quante);
    for (quale, gruppo) in per_rimappatura.into_iter().enumerate() {
        let inizio = da_guardare.len();
        if gruppo.len() <= tetto {
            da_guardare.extend(gruppo);
        } else {
            // Uno ogni `passo`, a partire dal primo: al più `tetto` percorsi
            // sparsi lungo tutto il gruppo.
            let passo = gruppo.len().div_ceil(tetto);
            for (posto, percorso) in gruppo.into_iter().enumerate() {
                if posto % passo == 0 {
                    da_guardare.push(percorso);
                } else {
                    stimati.insert(percorso, quale);
                }
            }
        }
        guardati_di.push(inizio..da_guardare.len());
    }

    let risposte = controlla(da_guardare.clone());
    // Una risposta mancante — un controllo che ne restituisse meno di quante
    // gliene si sono chieste — vale «non si sa», non «c'è».
    let esiti: HashMap<String, Esistenza> = da_guardare
        .into_iter()
        .zip(
            risposte
                .iter()
                .copied()
                .chain(std::iter::repeat(Esistenza::Ignota)),
        )
        .collect();
    let verdetti = guardati_di
        .into_iter()
        .map(|intervallo| verdetto(risposte.get(intervallo).unwrap_or(&[])))
        .collect();
    Verifiche {
        esiti,
        stimati,
        verdetti,
    }
}

/// La risposta che un campione dà per la sua rimappatura intera.
///
/// «C'è» solo se c'è la **maggioranza**: è la risposta che riscrive, e si dà
/// quando la destinazione è davvero quella giusta. Altrimenti vince fra «non
/// c'è» e «non si sa» la più frequente, e a parità «non si sa»: dire «non c'è»
/// di diciottomila file sulla base di un campione mezzo muto sarebbe la stessa
/// bugia che questo modulo esiste per non dire.
fn verdetto(campione: &[Esistenza]) -> Esistenza {
    let conta = |cosa: Esistenza| campione.iter().filter(|e| **e == cosa).count();
    let (presenti, assenti, ignoti) = (
        conta(Esistenza::Presente),
        conta(Esistenza::Assente),
        conta(Esistenza::Ignota),
    );
    if presenti > assenti.saturating_add(ignoti) {
        Esistenza::Presente
    } else if assenti > ignoti {
        Esistenza::Assente
    } else {
        Esistenza::Ignota
    }
}

/// Riscrive i percorsi dei brani, **solo quando il file nuovo c'è**, con le
/// tre risposte.
///
/// `verifica` dice [`Esistenza::Presente`], [`Esistenza::Assente`] o
/// [`Esistenza::Ignota`] per un percorso nuovo. Chi chiama la prepara **fuori**
/// dal lucchetto (vedi il `//!`) e qui la si consulta soltanto: nessuna
/// domanda al disco deve partire da dentro la transazione.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub fn applica_verificati(
    tx: &Transaction<'_>,
    rimappature: &[Rimappatura],
    verifica: &dyn Fn(&str) -> Esistenza,
) -> Result<Riscritti, AppError> {
    let vive = vive(rimappature);
    if vive.is_empty() {
        return Ok(Riscritti::default());
    }
    let da_riscrivere = da_riscrivere(tx, &vive)?;

    let mut esito = Riscritti::default();
    // «Occupato» con le regole del filesystem, non con quelle del confronto fra
    // stringhe. Dove le maiuscole non contano, `E:\Musica\Pink floyd\x.flac`
    // rimappato e `E:\Musica\Pink Floyd\x.flac` già scansionato sono **lo
    // stesso file**: il confronto esatto li lasciava entrare tutti e due —
    // l'indice `UNIQUE` di `path` le maiuscole le guarda — e la scansione dopo
    // toglieva la riga in più, che era quasi sempre quella appena rimappata,
    // con ascolti, voti e playlist dietro. L'indice
    // `idx_tracks_percorso_piegato` della 030 tiene veloce anche questa forma.
    let occupato_sql = if regole().case_insensitive {
        "SELECT 1 FROM tracks WHERE path = ?1 COLLATE NOCASE AND id <> ?2"
    } else {
        "SELECT 1 FROM tracks WHERE path = ?1 AND id <> ?2"
    };
    let mut occupato = tx
        .prepare(occupato_sql)
        .map_err(|err| db_error("percorsi da rimappare", &err))?;
    let mut sposta = tx
        .prepare("UPDATE tracks SET path = ?2 WHERE id = ?1")
        .map_err(|err| db_error("percorsi da rimappare", &err))?;

    for (id, nuovo, _) in da_riscrivere {
        match verifica(&nuovo) {
            Esistenza::Presente => {}
            Esistenza::Assente => {
                esito.irrintracciabili += 1;
                continue;
            }
            Esistenza::Ignota => {
                esito.non_verificati += 1;
                continue;
            }
        }
        let gia = occupato
            .exists(rusqlite::params![nuovo, id])
            .map_err(|err| db_error("percorsi da rimappare", &err))?;
        if gia {
            esito.gia_presenti += 1;
            continue;
        }
        esito.percorsi += sposta
            .execute(rusqlite::params![id, nuovo])
            .map_err(|err| db_error("riscrittura di un percorso", &err))?;
    }
    Ok(esito)
}

// ── le cartelle sorvegliate ─────────────────────────────────────────────────

/// Una cartella che non entra nell'elenco perché ci sta già dentro un'altra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assorbita {
    /// La cartella tolta.
    pub cartella: String,
    /// Quella dell'elenco che la contiene.
    pub dentro: String,
}

/// Una cartella nuova che è una già sorvegliata scritta in un altro modo — o
/// che la contiene, scritta in un altro modo.
///
/// Non entra nell'elenco: si propone il trasloco da [`Self::esistente`] a
/// [`Self::a`], che è una rimappatura ([`applica_verificati`]) e porta con sé
/// ascolti, voti e playlist. Aggiungerla e basta avrebbe fatto rientrare ogni
/// brano una seconda volta, col nome nuovo e la storia vuota.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gemella {
    /// La cartella come l'ha scelta chi la aggiunge.
    pub nuova: String,
    /// Quella già sorvegliata, che sta nello stesso posto.
    pub esistente: String,
    /// Come si chiamerebbe `esistente` nella grafia di `nuova`: la
    /// destinazione del trasloco. È `nuova` stessa quando le due sono la stessa
    /// cartella; è una sua sottocartella quando `nuova` contiene `esistente`.
    pub a: String,
}

/// L'elenco delle cartelle sorvegliate, messo in ordine, e cosa si è deciso.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartelleOrdinate {
    /// L'elenco da scrivere.
    pub cartelle: Vec<String>,
    /// Le cartelle tolte perché già dentro un'altra.
    pub assorbite: Vec<Assorbita>,
    /// Le cartelle nuove che sono una già sorvegliata con un altro nome.
    pub gemelle: Vec<Gemella>,
}

/// Mette in ordine le cartelle sorvegliate.
///
/// `prima` è l'elenco com'è adesso nel database, `chieste` quello che la
/// finestra vuole scrivere (di solito `prima` più una cartella, o meno una).
/// `canonica` dà il nome vero di un percorso — la sua forma risolta dal
/// sistema, senza prefisso verbatim — quando si è saputo: chi chiama lo calcola
/// con [`crate::volumi::risolvi_tutti`] **fuori** dal lucchetto, e qui lo si
/// consulta soltanto. Pura, quindi, e provabile senza nessuna share.
///
/// # Le quattro regole, in quest'ordine
///
/// 1. **Doppioni**: `D:\Musica`, `d:\musica` e `D:\Musica\` sono una cartella
///    sola, e resta la grafia che compare per prima — quella già in elenco.
/// 2. **Gemelle**: una cartella nuova il cui nome vero coincide con quello di
///    una già sorvegliata (o la contiene) non si aggiunge, e si propone il
///    trasloco. Solo fra una nuova e una di `prima`: due nuove gemelle fra loro
///    non hanno storia da portare, e la seconda si assorbe nella prima.
/// 3. **Dentro un'altra, con un altro nome**: una nuova il cui nome vero sta
///    sotto quello di una già in elenco è già sorvegliata — `\\nas\m\Rock`
///    quando c'è `Z:\` e `Z:` è `\\nas\m`. Si assorbe.
/// 4. **Annidate con lo stesso nome**: `D:\M` e `D:\M\Rock` — resta la madre.
///    È sicuro in tutti e due i versi perché i percorsi dei brani di
///    `D:\M\Rock` stanno sotto `D:\M` **per stringa**, quindi la scansione li
///    ritrova com'erano. Per questo la regola non si estende alle grafie
///    diverse fra cartelle già sorvegliate: lì togliere la figlia lascerebbe i
///    suoi brani fuori da ogni radice, e la scansione li rimetterebbe dentro col
///    nome della madre e la storia vuota.
///
/// La madre prende il posto della prima figlia che assorbe: l'ordine conta,
/// perché la prima cartella dell'elenco è quella dove finiscono i download se
/// nessuno ne ha scelta un'altra.
#[must_use]
pub fn ordina_cartelle(
    prima: &[String],
    chieste: &[String],
    canonica: &dyn Fn(&str) -> Option<String>,
) -> CartelleOrdinate {
    let regole = regole();
    let chiave = |p: &str| path_key(p, regole);
    let stava_prima = |p: &str| prima.iter().any(|q| chiave(q) == chiave(p));

    // 1. I doppioni, e le righe vuote.
    let mut viste = std::collections::HashSet::new();
    let elenco: Vec<String> = chieste
        .iter()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .filter(|c| viste.insert(chiave(c)))
        .map(ToOwned::to_owned)
        .collect();

    let mut esito = CartelleOrdinate::default();
    let mut tenute: Vec<String> = Vec::with_capacity(elenco.len());

    // 2 e 3. Le nuove, confrontate per nome vero.
    for cartella in &elenco {
        if stava_prima(cartella) {
            tenute.push(cartella.clone());
            continue;
        }
        let Some(vera) = canonica(cartella) else {
            // Il nome vero non si è saputo — la share non risponde, la cartella
            // non c'è ancora — e allora si decide sulla sola grafia, come
            // prima: la regola 4 fa quel che può.
            tenute.push(cartella.clone());
            continue;
        };
        let mut decisa = false;
        for altra in &tenute {
            let Some(vera_altra) = canonica(altra) else {
                continue;
            };
            // Stessa grafia: è la regola 4, che sa farlo senza perdere niente.
            if is_under(cartella, altra, regole) || is_under(altra, cartella, regole) {
                continue;
            }
            let dentro = is_under(&vera, &vera_altra, regole);
            let contiene = is_under(&vera_altra, &vera, regole);
            if !dentro && !contiene {
                continue;
            }
            if dentro && chiave(&vera) != chiave(&vera_altra) {
                // Regola 3: già sorvegliata sotto un altro nome.
                esito.assorbite.push(Assorbita {
                    cartella: cartella.clone(),
                    dentro: altra.clone(),
                });
            } else if stava_prima(altra) {
                // Regola 2: la stessa cartella, o una che la contiene, con un
                // altro nome. Il trasloco porta `altra` nella grafia nuova.
                let a = riscrivi(&vera_altra, &vera, cartella).unwrap_or_else(|| cartella.clone());
                esito.gemelle.push(Gemella {
                    nuova: cartella.clone(),
                    esistente: altra.clone(),
                    a,
                });
            } else if dentro {
                // Due nuove gemelle fra loro: nessuna storia da portare, si
                // tiene la prima.
                esito.assorbite.push(Assorbita {
                    cartella: cartella.clone(),
                    dentro: altra.clone(),
                });
            } else {
                // Una nuova che contiene un'altra nuova sotto un altro nome:
                // togliere la madre lascerebbe fuori le sorelle della figlia,
                // togliere la figlia non si può senza sapere chi arriva dopo.
                // Si tengono tutte e due: è il caso raro, e il costo è una
                // cartella letta due volte, non una storia persa.
                continue;
            }
            decisa = true;
            break;
        }
        if !decisa {
            tenute.push(cartella.clone());
        }
    }

    // 4. Le annidate con lo stesso nome: resta la madre più esterna.
    let madre_di = |figlia: &str| -> Option<&String> {
        tenute
            .iter()
            .filter(|m| chiave(m) != chiave(figlia) && is_under(figlia, m, regole))
            .min_by_key(|m| chiave(m).len())
    };
    let mut messe: Vec<String> = Vec::with_capacity(tenute.len());
    for cartella in &tenute {
        let da_mettere = match madre_di(cartella) {
            Some(madre) => {
                esito.assorbite.push(Assorbita {
                    cartella: cartella.clone(),
                    dentro: madre.clone(),
                });
                madre
            }
            None => cartella,
        };
        if !messe.iter().any(|m| chiave(m) == chiave(da_mettere)) {
            messe.push(da_mettere.clone());
        }
    }
    esito.cartelle = messe;
    esito
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_prefisso_si_sostituisce_conservando_la_grafia() {
        assert_eq!(
            riscrivi(
                r"D:\Musica\Pink Floyd\a.flac",
                r"D:\Musica",
                r"E:\Media\Musica"
            ),
            Some(r"E:\Media\Musica\Pink Floyd\a.flac".to_owned())
        );
        // La coda di separatori nella radice non cambia il risultato.
        assert_eq!(
            riscrivi(r"D:\Musica\a.flac", "D:\\Musica\\", r"E:\M"),
            Some(r"E:\M\a.flac".to_owned())
        );
    }

    #[test]
    fn il_confine_di_separatore_non_sposta_la_cartella_sbagliata() {
        // Senza il confine `D:\Musical` finirebbe sotto `D:\Musica`, e i suoi
        // percorsi verrebbero riscritti da una rimappatura che non lo riguarda.
        assert_eq!(riscrivi(r"D:\Musical\a.flac", r"D:\Musica", r"E:\M"), None);
    }

    #[test]
    fn una_destinazione_vuota_non_riscrive_niente() {
        // Una proposta che nessuno ha compilato deve restare una proposta.
        assert_eq!(riscrivi(r"D:\Musica\a.flac", r"D:\Musica", ""), None);
        assert_eq!(riscrivi(r"D:\Musica\a.flac", r"D:\Musica", "   "), None);
    }

    #[test]
    fn le_due_forme_di_valore_si_rimappano_tutte_e_due() {
        let rimappature = vec![Rimappatura {
            da: r"D:\Musica".to_owned(),
            a: r"E:\M".to_owned(),
            brani: 0,
        }];
        assert_eq!(
            rimappa_valore(r#"["D:\\Musica","F:\\Altro"]"#, &rimappature),
            r#"["E:\\M","F:\\Altro"]"#
        );
        assert_eq!(
            rimappa_valore(r"D:\Musica\Scarichi", &rimappature),
            r"E:\M\Scarichi"
        );
    }

    /// Una libreria in memoria con tre brani sotto `D:\Musica`.
    fn libreria() -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        connection
            .execute_batch(
                r"INSERT INTO tracks (id, path, track_key, title, artist, album,
                                      file_size, date_added, date_modified)
                  VALUES (1, 'D:\Musica\a.flac', 'k1', 'A', 'X', 'Y', 1, 1, 1),
                         (2, 'D:\Musica\b.flac', 'k2', 'B', 'X', 'Y', 1, 1, 1),
                         (3, 'D:\Musica\c.flac', 'k3', 'C', 'X', 'Y', 1, 1, 1);",
            )
            .expect("brani");
        connection
    }

    fn percorso_di(connection: &Connection, id: i64) -> String {
        connection
            .query_row("SELECT path FROM tracks WHERE id = ?1", [id], |r| r.get(0))
            .expect("percorso")
    }

    #[test]
    fn non_sapere_non_e_non_trovare() {
        // Il NAS spento: prima la risposta era «no», cioè «il file non c'è», e
        // i brani finivano fra gli irrintracciabili. Adesso «non si sa» ha un
        // conto suo, e la riga resta com'era come per gli altri due casi.
        let mut connection = libreria();
        let rimappature = vec![Rimappatura {
            da: r"D:\Musica".to_owned(),
            a: r"E:\M".to_owned(),
            brani: 3,
        }];
        let tx = connection.transaction().expect("transazione");
        let fatto = applica_verificati(&tx, &rimappature, &|percorso| {
            if percorso.ends_with("a.flac") {
                Esistenza::Presente
            } else if percorso.ends_with("b.flac") {
                Esistenza::Assente
            } else {
                Esistenza::Ignota
            }
        })
        .expect("rimappatura");
        tx.commit().expect("chiusura");
        assert_eq!(
            fatto,
            Riscritti {
                percorsi: 1,
                irrintracciabili: 1,
                gia_presenti: 0,
                non_verificati: 1,
            }
        );
        assert_eq!(percorso_di(&connection, 1), r"E:\M\a.flac");
        assert_eq!(percorso_di(&connection, 2), r"D:\Musica\b.flac");
        assert_eq!(percorso_di(&connection, 3), r"D:\Musica\c.flac");
    }

    #[test]
    fn lo_stesso_file_con_altre_maiuscole_e_gia_presente() {
        // Dove il filesystem ignora le maiuscole, `E:\m\a.flac` già in libreria
        // e `E:\M\a.flac` rimappato sono lo stesso file. Prima entravano tutti
        // e due, e la scansione dopo toglieva la riga rimappata — quella con
        // la storia.
        if !regole().case_insensitive {
            return;
        }
        let mut connection = libreria();
        connection
            .execute_batch(
                r"INSERT INTO tracks (id, path, track_key, title, artist, album,
                                      file_size, date_added, date_modified)
                  VALUES (4, 'E:\m\a.flac', 'k4', 'A', 'X', 'Y', 1, 1, 1);",
            )
            .expect("brano già scansionato");
        let rimappature = vec![Rimappatura {
            da: r"D:\Musica".to_owned(),
            a: r"E:\M".to_owned(),
            brani: 3,
        }];
        let tx = connection.transaction().expect("transazione");
        let fatto = applica_verificati(&tx, &rimappature, &|percorso| {
            if percorso.ends_with("a.flac") {
                Esistenza::Presente
            } else {
                Esistenza::Ignota
            }
        })
        .expect("rimappatura");
        tx.commit().expect("chiusura");
        assert_eq!(fatto.gia_presenti, 1);
        assert_eq!(fatto.percorsi, 0);
        assert_eq!(percorso_di(&connection, 1), r"D:\Musica\a.flac");
    }

    #[test]
    fn i_candidati_sono_quel_che_la_rimappatura_scriverebbe() {
        // Letti con il lucchetto preso per un attimo, verificati fuori: devono
        // essere esattamente i percorsi che `applica_verificati` poi consulta.
        let connection = libreria();
        let rimappature = vec![
            // Una proposta non compilata non produce candidati.
            Rimappatura {
                da: r"F:\Altro".to_owned(),
                a: String::new(),
                brani: 0,
            },
            Rimappatura {
                da: r"D:\Musica".to_owned(),
                a: r"\\nas\musica".to_owned(),
                brani: 3,
            },
        ];
        let trovati = candidati(&connection, &rimappature).expect("candidati");
        let percorsi: Vec<&str> = trovati.iter().map(|c| c.percorso.as_str()).collect();
        assert_eq!(
            percorsi,
            [
                r"\\nas\musica\a.flac",
                r"\\nas\musica\b.flac",
                r"\\nas\musica\c.flac"
            ]
        );
        assert!(
            trovati.iter().all(|c| c.rimappatura == 0),
            "fra le compilate"
        );
    }

    #[test]
    fn il_piano_guarda_un_campione_e_lo_dice() {
        // Mille brani, un campione da dieci: il disco riceve dieci domande e
        // non mille, e ai novecentonovanta non guardati tocca il verdetto del
        // campione.
        let candidati: Vec<Candidato> = (0..1000)
            .map(|n| Candidato {
                percorso: format!(r"E:\M\{n:04}.flac"),
                rimappatura: 0,
            })
            .collect();
        let chieste = std::cell::Cell::new(0_usize);
        let verifiche = verifica(candidati.clone(), Some(10), &|percorsi| {
            chieste.set(percorsi.len());
            vec![Esistenza::Presente; percorsi.len()]
        });
        assert!(chieste.get() <= 10, "domande al disco: {}", chieste.get());
        assert!(verifiche.stima(), "il piano dice che è una stima");
        assert_eq!(
            verifiche.esito(r"E:\M\0999.flac"),
            Some(Esistenza::Presente)
        );
        // Una radice o una preferenza non è un candidato: la verifica chi chiama.
        assert_eq!(verifiche.esito(r"E:\M"), None);

        // La share spenta: il campione tace, e tutti sono «non si sa».
        let muta = verifica(candidati.clone(), Some(10), &|percorsi| {
            vec![Esistenza::Ignota; percorsi.len()]
        });
        assert_eq!(muta.esito(r"E:\M\0500.flac"), Some(Esistenza::Ignota));

        // L'importazione vera guarda tutto, e non stima niente.
        let tutto = verifica(candidati, None, &|percorsi| {
            vec![Esistenza::Assente; percorsi.len()]
        });
        assert!(!tutto.stima());
        assert_eq!(tutto.esito(r"E:\M\0500.flac"), Some(Esistenza::Assente));
    }

    #[test]
    fn il_verdetto_di_un_campione_non_dice_c_e_senza_maggioranza() {
        use Esistenza::{Assente, Ignota, Presente};
        assert_eq!(verdetto(&[Presente, Presente, Assente]), Presente);
        // Metà e metà non è «c'è»: è la risposta che riscrive, e si dà solo
        // quando la destinazione è chiaramente quella giusta.
        assert_eq!(verdetto(&[Presente, Assente]), Assente);
        assert_eq!(verdetto(&[Presente, Ignota]), Ignota);
        assert_eq!(verdetto(&[Assente, Assente, Ignota]), Assente);
        // A parità fra «non c'è» e «non si sa», «non si sa».
        assert_eq!(verdetto(&[Assente, Ignota]), Ignota);
        assert_eq!(verdetto(&[]), Ignota);
    }

    /// Nessun nome vero: le prove che non parlano di grafie diverse.
    fn senza_nomi(_: &str) -> Option<String> {
        None
    }

    fn elenco(voci: &[&str]) -> Vec<String> {
        voci.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn la_stessa_cartella_scritta_in_tre_modi_e_una() {
        let date = elenco(&[r"D:\Musica", r"D:\Musica\", r"D:\Live", "   "]);
        let fatto = ordina_cartelle(&[], &date, &senza_nomi);
        assert_eq!(fatto.cartelle, elenco(&[r"D:\Musica", r"D:\Live"]));
        // Le maiuscole contano solo dove il filesystem le distingue.
        if cfg!(any(target_os = "windows", target_os = "macos")) {
            let mut con_minuscole = date;
            con_minuscole.insert(1, r"d:\musica".to_owned());
            let fatto = ordina_cartelle(&[], &con_minuscole, &senza_nomi);
            assert_eq!(fatto.cartelle, elenco(&[r"D:\Musica", r"D:\Live"]));
        }
    }

    #[test]
    fn le_cartelle_annidate_si_raccolgono_nella_madre() {
        // La madre nuova sopra una figlia già sorvegliata: resta la madre, al
        // posto della figlia — la prima dell'elenco è quella dei download.
        let prima = elenco(&[r"D:\M\Rock", r"E:\Live"]);
        let chieste = elenco(&[r"D:\M\Rock", r"E:\Live", r"D:\M"]);
        let fatto = ordina_cartelle(&prima, &chieste, &senza_nomi);
        assert_eq!(fatto.cartelle, elenco(&[r"D:\M", r"E:\Live"]));
        assert_eq!(
            fatto.assorbite,
            vec![Assorbita {
                cartella: r"D:\M\Rock".to_owned(),
                dentro: r"D:\M".to_owned(),
            }]
        );

        // E la figlia nuova sotto una madre già sorvegliata non entra.
        let fatto = ordina_cartelle(
            &elenco(&[r"D:\M"]),
            &elenco(&[r"D:\M", r"D:\M\Jazz\Blue Note"]),
            &senza_nomi,
        );
        assert_eq!(fatto.cartelle, elenco(&[r"D:\M"]));
        assert_eq!(fatto.assorbite.len(), 1);
        assert!(fatto.gemelle.is_empty());
    }

    #[test]
    fn la_stessa_cartella_con_un_altro_nome_propone_il_trasloco() {
        // `Z:` è mappata su `\\nas\musica`: aggiungere la seconda accanto alla
        // prima voleva dire ogni brano due volte, e il secondo senza storia.
        let nomi = |p: &str| -> Option<String> {
            match p {
                r"Z:\" | r"\\nas\musica" => Some(r"\\nas\musica".to_owned()),
                r"Z:\Rock" => Some(r"\\nas\musica\Rock".to_owned()),
                r"\\nas\musica\Jazz" => Some(r"\\nas\musica\Jazz".to_owned()),
                _ => None,
            }
        };

        let fatto = ordina_cartelle(
            &elenco(&[r"Z:\"]),
            &elenco(&[r"Z:\", r"\\nas\musica"]),
            &nomi,
        );
        assert_eq!(fatto.cartelle, elenco(&[r"Z:\"]), "il doppione non entra");
        assert_eq!(
            fatto.gemelle,
            vec![Gemella {
                nuova: r"\\nas\musica".to_owned(),
                esistente: r"Z:\".to_owned(),
                a: r"\\nas\musica".to_owned(),
            }]
        );

        // La nuova **contiene** quella già sorvegliata con un altro nome: il
        // trasloco porta `Z:\Rock` nella grafia nuova, e dopo la madre la
        // raccoglie senza perdere niente.
        let fatto = ordina_cartelle(
            &elenco(&[r"Z:\Rock"]),
            &elenco(&[r"Z:\Rock", r"\\nas\musica"]),
            &nomi,
        );
        assert_eq!(fatto.cartelle, elenco(&[r"Z:\Rock"]));
        assert_eq!(
            fatto.gemelle.first().map(|g| g.a.as_str()),
            Some(r"\\nas\musica\Rock")
        );

        // Una nuova che sta **dentro** una sorvegliata con un altro nome è già
        // sorvegliata: si assorbe, e non c'è niente da traslocare.
        let fatto = ordina_cartelle(
            &elenco(&[r"Z:\"]),
            &elenco(&[r"Z:\", r"\\nas\musica\Jazz"]),
            &nomi,
        );
        assert_eq!(fatto.cartelle, elenco(&[r"Z:\"]));
        assert!(fatto.gemelle.is_empty());
        assert_eq!(
            fatto.assorbite,
            vec![Assorbita {
                cartella: r"\\nas\musica\Jazz".to_owned(),
                dentro: r"Z:\".to_owned(),
            }]
        );
    }

    #[test]
    fn senza_il_nome_vero_si_decide_sulla_grafia() {
        // La share spenta al momento dell'aggiunta: il nome vero non si sa, e
        // la cartella entra — come prima, e come l'utente ha chiesto.
        let fatto = ordina_cartelle(
            &elenco(&[r"Z:\"]),
            &elenco(&[r"Z:\", r"\\nas\musica"]),
            &senza_nomi,
        );
        assert_eq!(fatto.cartelle, elenco(&[r"Z:\", r"\\nas\musica"]));
        assert!(fatto.gemelle.is_empty() && fatto.assorbite.is_empty());
    }
}
