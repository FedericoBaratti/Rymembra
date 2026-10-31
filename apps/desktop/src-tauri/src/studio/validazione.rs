//! La validazione, la prova sull'app e la migrazione di un testo: i comandi che
//! dipendono soltanto dal testo e dalla bozza, coi ricordi delle loro risposte.

use std::hash::{Hash as _, Hasher as _};
use std::sync::{Mutex, PoisonError};

use rymembra_domain::errors::AppError;
use serde::Serialize;

use crate::errore::{Esito, errore};

use super::id_sicuro;

// ── La validazione ──────────────────────────────────────────────────────────

/// Un problema che blocca.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemaIpc {
    /// Il codice del catalogo.
    pub code: String,
    /// Dove, in forma di percorso nel documento.
    pub path: String,
    /// Cosa non va, e cosa scrivere al suo posto.
    pub message: String,
    /// Il nome che forse si voleva scrivere, quando ce n'è uno vicino.
    pub forse: Vec<String>,
    /// La riga in cui è scritto, da uno.
    ///
    /// `None` soltanto quando nemmeno la radice del documento esiste, cioè su un
    /// testo vuoto. Prima non c'era affatto, e la finestra la indovinava
    /// cercando l'ultimo pezzo del percorso col primo `indexOf` che
    /// corrispondeva: su `parts.x.background.0.stops.1.color` finiva a
    /// sottolineare la prima riga che nominasse un colore qualunque.
    pub riga: Option<u32>,
    /// La colonna, da uno, in unità UTF-16 come le conta la `<textarea>`.
    pub colonna: Option<u32>,
}

/// Un avviso, che non blocca.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvvisoIpc {
    /// Il genere, in camelCase: i nomi sono quelli di `nome_avviso`.
    ///
    /// L'elenco non si ricopia qui: `nome_avviso` è un `match` esaustivo, e un
    /// genere nuovo nel nucleo non compila finché non passa di là, mentre una
    /// copia in prosa restava indietro — questa ne aveva perso uno
    /// (`restingLoop`) proprio mentre diceva di essere stata riallineata. Chi
    /// aggiorna l'unione di `Avviso["kind"]` in `ipc.ts`, l'unico posto dove la
    /// stessa svista non la vede nessun compilatore, la confronti con quel
    /// `match`.
    pub kind: &'static str,
    /// Dove.
    pub path: String,
    /// Cosa.
    pub message: String,
    /// La riga in cui è scritto, da uno. Come per [`ProblemaIpc`]: un avviso ha
    /// un percorso, quindi ha un posto, e il bottone che ci porta è lo stesso.
    pub riga: Option<u32>,
    /// La colonna, da uno.
    pub colonna: Option<u32>,
}

/// Una coppia di colori misurata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContrastoIpc {
    /// Il token davanti.
    pub davanti: &'static str,
    /// Il token dietro.
    pub dietro: &'static str,
    /// Il rapporto nel tema di base.
    pub base: f64,
    /// Il rapporto nella variante, se c'è.
    pub variante: Option<f64>,
    /// Passa la soglia in tutti i temi misurati.
    pub passa: bool,
}

/// Quanto costa una parte, sui suoi due budget.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostoParteIpc {
    /// La parte.
    pub parte: &'static str,
    /// Motivi, livelli e filtri: il budget della superficie.
    pub superficie: u32,
    /// Le animazioni assegnate: il budget del movimento.
    pub movimento: u32,
}

/// L'esito di una validazione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidazioneIpc {
    /// Vuoto se il documento è valido.
    pub errori: Vec<ProblemaIpc>,
    /// Gli avvisi. Non bloccano: una skin con un avviso è una skin che funziona.
    pub avvisi: Vec<AvvisoIpc>,
    /// La tabella dei contrasti, tutta, non solo le righe che non passano.
    pub contrasti: Vec<ContrastoIpc>,
    /// Quante volte ogni colore della tavolozza è riferito.
    pub tavolozza: Vec<(String, usize)>,
    /// Il foglio compilato. Vuoto quando ci sono errori.
    pub css: String,
    /// L'impaginazione, con lo scafale completo.
    ///
    /// `None` quando ci sono errori: la vista Impagina resta all'ultimo albero
    /// valido, come l'anteprima resta all'ultimo foglio valido. Un editor che si
    /// svuota a metà di una parentesi costringe a scrivere in fretta.
    pub layout: Option<crate::skin::ImpaginazioneIpc>,
    /// Il costo totale di motivi e parti.
    pub costo: u32,
    /// Quanti pezzi dell'app lo scafale monta, sul suo budget separato.
    pub costo_scafale: u32,
    /// Quanto costa il movimento, sommato su tutte le parti animate.
    ///
    /// Terzo numero e terzo budget. Il compilatore lo calcolava da quando le
    /// animazioni nominate esistono e non lo leggeva nessuno: un numero
    /// prodotto e buttato via è una promessa che il crate faceva senza
    /// mantenerla. Il verdetto resta di `check_skin`, che il budget lo applica
    /// **per parte**; questa è la somma, cioè il numero che sta accanto agli
    /// altri due.
    pub costo_movimento: u32,
    /// Quante parti la skin ridisegna.
    pub parti: usize,
    /// Il costo di ogni parte ridisegnata: la superficie e il movimento.
    pub costi: Vec<CostoParteIpc>,
    /// Quanto movimento può essere in corso nello stesso momento, sulla
    /// finestra intera: il numero che il budget della finestra misura.
    pub costo_finestra: u32,
    /// Le risorse che il documento nomina.
    pub risorse: Vec<String>,
    /// I token che seguono la copertina.
    pub dinamici: Vec<String>,
    /// Quanti millisecondi ci ha messo a compilare.
    pub compilato_ms: u128,
    /// L'id sotto cui il foglio è compilato: quel che la tela scrive in
    /// `data-skin`. Vuoto quando ci sono errori, perché non c'è un foglio.
    ///
    /// È quello della bozza e non quello del documento — vedi
    /// [`radice_della_bozza`] — e viaggia **col** foglio perché i due devono
    /// dire la stessa cosa: fino alla 3.0 la tela scriveva l'id della bozza e
    /// il foglio usava quello del documento, e un documento che cambiava `id`
    /// lasciava la tela senza una regola che la trovasse finché la rinomina
    /// non arrivava.
    pub radice: String,
}

const fn nome_avviso(kind: rymembra_skin::WarningKind) -> &'static str {
    use rymembra_skin::WarningKind as W;
    match kind {
        W::MissingRequiredToken => "missingRequiredToken",
        W::UnkeptCapability => "unkeptCapability",
        W::UnusedPattern => "unusedPattern",
        W::UnusedPrefab => "unusedPrefab",
        W::UnusedAnimation => "unusedAnimation",
        W::CostBudget => "costBudget",
        W::Contrast => "contrast",
        W::RestingLoop => "restingLoop",
    }
}

// ── I ricordi: le ultime risposte, per testo ────────────────────────────────

/// Quante risposte si ricordano per comando.
///
/// Poche, e non è avarizia: il caso che le fa servire è l'annullo — Ctrl+Z,
/// Ctrl+Y, Ctrl+Z su tre testi — e il ritorno da «Prova sull'app», che
/// rimanda lo stesso testo di prima. Una risposta porta dentro un foglio da
/// qualche decina di kilobyte, e sei sono già più di quanti passi si rifanno
/// in fila.
const QUANTI_RICORDI: usize = 6;

/// Una risposta ricordata, col testo e la bozza a cui risponde.
struct Ricordo<V> {
    impronta: u64,
    bozza: Option<String>,
    testo: String,
    valore: V,
}

/// Le ultime risposte di un comando dello Studio che non dipende da nient'altro
/// che dal testo e dalla bozza.
///
/// # Perché si ricorda
///
/// `studio_valida` è `parse` + `check` + `compile` su tutto il documento, e
/// girava sul filo della finestra: a ogni passo dell'annullo, a ogni ritorno
/// dalla prova, la finestra aspettava il nucleo per un documento che aveva
/// già visto un secondo prima. Il nucleo è deterministico — stesso testo,
/// stessa bozza, stessa risposta — quindi la risposta si può tenere.
///
/// L'impronta serve a cercare in fretta; il confronto vero è sul testo intero,
/// così due testi con la stessa impronta non si scambiano mai la risposta.
struct Ricordi<V> {
    voci: Mutex<Vec<Ricordo<V>>>,
}

impl<V: Clone> Ricordi<V> {
    const fn nuovi() -> Self {
        Self {
            voci: Mutex::new(Vec::new()),
        }
    }

    /// La risposta ricordata, o quella calcolata adesso — e ricordata.
    ///
    /// Il calcolo sta **fuori** dal lucchetto: due validazioni diverse non si
    /// aspettano a vicenda, e due uguali al peggio calcolano due volte.
    fn o_calcola(&self, bozza: Option<&str>, testo: &str, calcola: impl FnOnce() -> V) -> V {
        let impronta = {
            let mut h = std::hash::DefaultHasher::new();
            bozza.hash(&mut h);
            testo.hash(&mut h);
            h.finish()
        };
        let stesso = |r: &Ricordo<V>| {
            r.impronta == impronta && r.bozza.as_deref() == bozza && r.testo == testo
        };
        {
            let voci = self.voci.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(trovato) = voci.iter().find(|r| stesso(r)) {
                return trovato.valore.clone();
            }
        }
        let valore = calcola();
        let mut voci = self.voci.lock().unwrap_or_else(PoisonError::into_inner);
        voci.retain(|r| !stesso(r));
        voci.insert(
            0,
            Ricordo {
                impronta,
                bozza: bozza.map(ToOwned::to_owned),
                testo: testo.to_owned(),
                valore: valore.clone(),
            },
        );
        voci.truncate(QUANTI_RICORDI);
        valore
    }
}

static VALIDAZIONI: Ricordi<ValidazioneIpc> = Ricordi::nuovi();
static PROVE: Ricordi<Result<crate::skin::SkinIpc, AppError>> = Ricordi::nuovi();
static MIGRAZIONI: Ricordi<MigratoIpc> = Ricordi::nuovi();

/// L'id sotto cui compilare la tela e la prova: quello della bozza, quando si
/// può usare, altrimenti quello del documento.
///
/// Quello della bozza perché è la cartella da cui vengono le risorse, e il
/// documento può dichiarare un `id` che la bozza non ha ancora — l'ha appena
/// cambiato chi scrive, o è di un'altra skin e la rinomina è stata rifiutata.
/// Compilare col documento voleva dire caratteri e immagini chiesti a una
/// cartella che non esiste. Si usa solo se è già nell'alfabeto del foglio
/// (lettere, cifre, trattini): il compilatore riduce l'id a quello, e un id
/// ridotto non nominerebbe più la cartella.
fn radice_della_bozza(bozza: Option<&str>, documento: &str) -> String {
    bozza
        .filter(|b| id_sicuro(b) && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(documento)
        .to_owned()
}

/// Valida e compila un documento skin scritto a mano.
///
/// **Non fallisce mai.** Un documento non valido è il caso normale mentre lo si
/// scrive — a metà di una parentesi il JSON non è JSON — e restituire un errore
/// IPC costringerebbe l'editor a distinguere «il comando è andato storto» da «il
/// documento non è ancora finito». Qui gli errori sono un campo del risultato.
///
/// `bozza` è la bozza aperta: il foglio si compila sotto il suo id, e le
/// risorse vengono dalla sua cartella. Vedi [`radice_della_bozza`].
///
/// Gira fuori dal filo della finestra, e ricorda le ultime risposte: vedi
/// [`Ricordi`].
#[tauri::command(async)]
#[must_use]
pub fn studio_valida(sorgente: String, bozza: Option<String>) -> ValidazioneIpc {
    VALIDAZIONI.o_calcola(bozza.as_deref(), &sorgente, || {
        valida(&sorgente, bozza.as_deref())
    })
}

/// Quel che fa [`studio_valida`], senza i ricordi.
fn valida(sorgente: &str, bozza: Option<&str>) -> ValidazioneIpc {
    let inizio = std::time::Instant::now();
    let vuoto = ValidazioneIpc {
        errori: Vec::new(),
        avvisi: Vec::new(),
        contrasti: Vec::new(),
        tavolozza: Vec::new(),
        css: String::new(),
        layout: None,
        costo: 0,
        costo_scafale: 0,
        costo_movimento: 0,
        parti: 0,
        costi: Vec::new(),
        costo_finestra: 0,
        risorse: Vec::new(),
        dinamici: Vec::new(),
        compilato_ms: 0,
        radice: String::new(),
    };

    // Le posizioni si calcolano una volta per validazione e non una per
    // problema: è una passata sola sul testo, e sono venti token sbagliati che
    // chiedono la stessa mappa.
    let dove = rymembra_skin::posizioni(sorgente);

    let documento = match rymembra_skin::leggi_skin(sorgente) {
        Ok(documento) => documento,
        Err(problemi) => {
            let mut errori: Vec<ProblemaIpc> =
                problemi.iter().map(|p| problema_da(p, &dove)).collect();
            // In ordine di riga, e non è cosmesi. I token e le parti si leggono
            // da una mappa di `serde_json`, che li ordina per nome: un elenco
            // così esce alfabetico, e chi lo scorre accanto al file salta su e
            // giù per il documento a ogni riga. Chi ha scritto tre token
            // sbagliati li corregge dall'alto in basso.
            //
            // Ordinamento stabile: a parità di posizione — due problemi sulla
            // stessa riga — resta l'ordine in cui il nucleo li ha trovati.
            errori.sort_by_key(|e| (e.riga.unwrap_or(u32::MAX), e.colonna.unwrap_or(u32::MAX)));
            return ValidazioneIpc { errori, ..vuoto };
        }
    };

    // Nell'ambito della tela: i selettori della finestra — la tela è un
    // `<iframe>`, un documento suo dove `:root` è la sua radice e la skin
    // attiva dell'app non c'è — con le risorse della bozza e gli stati che la
    // tela può forzare (`data-forza`). Sotto l'id della bozza: vedi
    // [`radice_della_bozza`].
    let ambito = rymembra_skin::Ambito::tela(&radice_della_bozza(bozza, &documento.id));
    let compilata = rymembra_skin::compile::compila(&documento, &ambito);
    ValidazioneIpc {
        errori: Vec::new(),
        avvisi: rymembra_skin::check_skin(&documento)
            .into_iter()
            .map(|a| {
                let punto = dove.di(&a.path);
                AvvisoIpc {
                    kind: nome_avviso(a.kind),
                    path: a.path,
                    message: a.message,
                    riga: punto.map(|p| p.riga),
                    colonna: punto.map(|p| p.colonna),
                }
            })
            .collect::<Vec<_>>(),
        contrasti: rymembra_skin::contrast_pairs(&documento)
            .into_iter()
            .map(|c| ContrastoIpc {
                passa: c.passa(),
                davanti: c.foreground,
                dietro: c.background,
                base: c.base,
                variante: c.variante,
            })
            .collect(),
        tavolozza: rymembra_skin::palette_usage(&documento),
        css: compilata.css,
        layout: Some(crate::skin::impaginazione(&documento)),
        costo: compilata.cost,
        costo_scafale: compilata.shell_cost,
        costo_movimento: compilata.motion_cost,
        parti: documento.parts.len(),
        costi: documento
            .parts
            .iter()
            .map(|stile| CostoParteIpc {
                parte: stile.def.name,
                superficie: stile.costo_superficie(),
                movimento: stile.motion.costo(),
            })
            .collect(),
        costo_finestra: documento
            .parts
            .iter()
            .map(|stile| stile.motion.costo_insieme())
            .sum(),
        risorse: documento.assets(),
        dinamici: compilata
            .dynamic_tokens
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        compilato_ms: inizio.elapsed().as_millis(),
        radice: ambito.id().to_owned(),
    }
}

/// I nomi suggeriti dal nucleo, presi dal messaggio come dati.
///
/// Il suggerimento di `vicini()` esiste già dentro la frase — «Forse intendevi
/// «color.accent»?» — ma una frase non è un bottone. Qui si rileggono le
/// virgolette basse e si tiene quel che è davvero un nome del registro, così
/// l'editor può offrirlo come correzione da premere.
///
/// # Perché tre registri e non solo le parti
///
/// Perché il filtro era `parts::part(nome).is_some()`, e i refusi sui **token**
/// sono i più frequenti di tutti: `color.accents` per `color.accent`. Il
/// suggerimento c'era, il bottone no, e la differenza la vedeva solo chi sapeva
/// già cosa scrivere. `rinominaChiave` — quel che il bottone chiama — lavora sul
/// percorso del problema, quindi non gli importa di che registro sia il nome.
fn suggeriti(messaggio: &str) -> Vec<String> {
    messaggio
        .split('«')
        .skip(1)
        .filter_map(|pezzo| pezzo.split_once('»').map(|(dentro, _)| dentro.to_owned()))
        .filter(|nome| {
            rymembra_skin::parts::part(nome).is_some()
                || rymembra_skin::tokens::token(nome).is_some()
                || rymembra_skin::effects::Effect::NAMES.contains(&nome.as_str())
        })
        .collect()
}

/// Traduce un problema di validazione in qualcosa che l'editor può mostrare.
///
/// # Cosa faceva prima, e perché non poteva funzionare
///
/// Prendeva un `AppError` — cioè i venti problemi già uniti da `in_errore` con
/// dei punti e virgola — e si riprendeva il percorso facendo `split_once(':')`
/// sulla prosa. Ne usciva **un** problema con dentro venti frasi, un percorso
/// che era quello del primo, e un pannello che cresceva finché non aveva mangiato
/// l'editor. Adesso il percorso, il messaggio e il codice arrivano dai campi di
/// [`rymembra_skin::SkinIssue`], che li ha sempre avuti.
fn problema_da(guasto: &rymembra_skin::SkinIssue, dove: &rymembra_skin::Posizioni) -> ProblemaIpc {
    // Il punto scritto nel problema vince su quello dedotto dal percorso: ce
    // l'ha soltanto l'errore di sintassi, che un percorso non ce l'ha affatto.
    let punto = guasto.punto.or_else(|| dove.di(&guasto.path));
    ProblemaIpc {
        code: guasto.code.kind().code().to_owned(),
        path: guasto.path.clone(),
        message: guasto.message.clone(),
        forse: suggeriti(&guasto.message),
        riga: punto.map(|p| p.riga),
        colonna: punto.map(|p| p.colonna),
    }
}

/// Un testo portato al formato di oggi.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigratoIpc {
    /// Il testo da aprire.
    pub sorgente: String,
    /// Di quale formato era.
    pub da: u64,
}

/// Porta un testo al formato di oggi: per chi incolla nello Studio una skin
/// della 2.x.
///
/// Un testo che non è JSON torna com'era, col formato di oggi: la migrazione
/// non è il posto in cui dire cosa non va in una parentesi — lo dice la
/// validazione, riga per riga.
///
/// Fuori dal filo della finestra, e coi suoi ricordi: vedi [`Ricordi`].
#[tauri::command(async)]
#[must_use]
pub fn studio_migra(sorgente: String) -> MigratoIpc {
    MIGRAZIONI.o_calcola(None, &sorgente, || {
        match rymembra_skin::migra_testo(&sorgente) {
            Ok(migrato) => MigratoIpc {
                sorgente: migrato.sorgente,
                da: migrato.da,
            },
            Err(_) => MigratoIpc {
                sorgente: sorgente.clone(),
                da: u64::from(rymembra_skin::SKIN_FORMAT_VERSION),
            },
        }
    })
}

/// Compila una bozza per la finestra vera: «Prova sull'app».
///
/// Selettori della finestra, risorse della bozza — e l'id della **bozza**,
/// come la tela ([`radice_della_bozza`]): le risorse stanno nella sua
/// cartella, e la skin che torna porta lo stesso id dei selettori, che è
/// quel che la finestra scrive in `data-skin`. Gli errori fermano: una bozza
/// che non si legge non si prova addosso.
///
/// Fuori dal filo della finestra, e coi suoi ricordi: tornare dalla prova e
/// riprovare lo stesso testo non ricompila niente.
#[tauri::command(async)]
pub fn studio_compila_app(sorgente: String, bozza: String) -> Esito<crate::skin::SkinIpc> {
    PROVE
        .o_calcola(Some(&bozza), &sorgente, || {
            compila_per_la_prova(&sorgente, &bozza)
        })
        .map_err(errore)
}

/// Quel che fa [`studio_compila_app`], senza i ricordi.
pub(super) fn compila_per_la_prova(
    sorgente: &str,
    bozza: &str,
) -> Result<crate::skin::SkinIpc, AppError> {
    let documento = rymembra_skin::parse_skin_json(sorgente)?;
    let ambito = rymembra_skin::Ambito::prova(&radice_della_bozza(Some(bozza), &documento.id));
    let mut skin = crate::skin::skin_ipc(&documento, &ambito);
    skin.id = ambito.id().to_owned();
    Ok(skin)
}
