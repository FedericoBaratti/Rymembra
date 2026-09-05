//! ListenBrainz Labs: chi ascolta questo, ascolta anche quest'altro.
//!
//! # Che genere di metadato è
//!
//! Gli altri moduli di questo crate rispondono a domande sul **brano**: come si
//! chiama davvero, di che disco fa parte, che copertina ha. Questo risponde a
//! una domanda su chi lo ascolta, e la risposta non sta in nessun tag: due
//! canzoni sono vicine quando le stesse persone le mettono nella stessa
//! sessione d'ascolto. È lo strato culturale di un motore di suggerimenti —
//! quello che nessuna analisi del segnale e nessun catalogo possono ricavare,
//! perché non è una proprietà del suono né dell'edizione.
//!
//! I dati vengono da MetaBrainz e sono pubblici (CC0), calcolati aggregando gli
//! ascolti di tutta ListenBrainz. **La domanda non porta nessuna identità**:
//! parte un identificativo MusicBrainz, e nient'altro. Non c'è una chiave, non
//! c'è un account, non c'è un nome utente — nemmeno quando lo scrobbling verso
//! ListenBrainz è collegato, perché quello parla con un altro host, da un altro
//! crate, con un'altra credenziale.
//!
//! Sta in `aether-meta` per la stessa ragione di [`crate::lrclib`]: è un
//! servizio senza chiave, e da qui eredita gratis il deposito, la cadenza,
//! l'interruttore e la distinzione fra «non ce l'ha» e «non si sa».
//!
//! # I lotti, e come cambiano la forma di tutto
//!
//! I punti accettano **molti identificativi per richiesta**, e non è un
//! dettaglio di comodità: è la differenza fra millequattrocento richieste per
//! una libreria e cinquantasei. Ogni funzione qui dentro è scritta per il
//! lotto, e la versione a un identificativo solo è il lotto da uno.
//!
//! Ma la **chiave di cache resta il singolo identificativo**. Una chiave che
//! descrivesse il lotto dipenderebbe da come i brani sono stati raggruppati, e
//! due passate che li raggruppano anche solo in un ordine diverso non si
//! ritroverebbero mai — una cache che sbaglia sempre, che si riempie e non
//! risparmia niente. Da qui [`crate::Fornitori::json_senza_memoria`]: il ritmo
//! e l'interruttore restano di là, perché sono proprietà del servizio; la
//! memoria la gestisce questo modulo, che sa dividere la risposta.
//!
//! ## Quel che il lotto costa, e che va detto
//!
//! Il servizio applica un tetto **complessivo** di circa cento righe per
//! identificativo chiesto, non cento righe *a testa*: in un lotto di dieci, un
//! brano dai vicini forti se ne prende seicento e uno dai vicini deboli ne
//! riceve sette. È misurato, non dedotto — vedi [`MBID_PER_RICHIESTA`].
//!
//! La conseguenza pericolosa non è il brano che riceve poco: è il brano che
//! riceve **zero**, perché è indistinguibile da un brano che il servizio
//! davvero non conosce, e ricordarlo come «non c'è» per un mese sarebbe un
//! errore che si corregge da solo troppo tardi. Da qui [`Affini::satura`].
//!
//! # I punteggi non sono confrontabili, e vanno resi tali
//!
//! Il `score` grezzo è un **conteggio di co-occorrenze**: illimitato in alto e
//! con una coda lunghissima. Il primo vicino di un successo mondiale sta a
//! milleduecento, quello di un disco di nicchia a venti. Sono due numeri che non
//! si possono confrontare, e passarli così a chi ordina significherebbe
//! ordinare per fama.
//!
//! Perciò si normalizza **dentro la risposta, e per ciascun brano chiesto**:
//! il rapporto logaritmico contro il massimo del *suo* gruppo, che mette il
//! primo vicino esattamente a `1.0` e comprime la coda senza azzerarla. Con un
//! solo identificativo chiesto le due letture coincidono; è il lotto che le
//! separa, e prendere il massimo dell'intera risposta rifarebbe esattamente il
//! difetto che questa funzione esiste per evitare — il disco di nicchia
//! schiacciato a zero perché nel lotto c'era un successo mondiale.
//!
//! Una normalizzazione **globale**, contro un massimo tenuto fra una richiesta e
//! l'altra, sarebbe la stessa cosa in peggio: i brani famosi vincerebbero
//! sempre, dappertutto, per costruzione.
//!
//! # Quanti vicini si tengono
//!
//! Venticinque. Oltre il venticinquesimo i punteggi sono una frazione del
//! primo, e ognuno costa una riga **moltiplicata per l'intera libreria**: cento
//! vicini per millequattrocento brani sono centoquarantamila righe da scrivere,
//! indicizzare e risincronizzare, per una coda che nessun ordinamento
//! ragionevole andrà mai a pescare.
//!
//! # Come si sono verificati i nomi qui dentro
//!
//! Interrogando il servizio, non a memoria. Le stringhe degli algoritmi sono
//! nomi lunghi che ListenBrainz ha già cambiato più di una volta, e i nomi dei
//! campi non stanno in nessuna documentazione di riferimento: si leggono dalle
//! pagine di consultazione (`/similar-recordings`, `/similar-artists`) e dalle
//! risposte vere. La data della verifica è in [`VERIFICATO_IL`], ed è
//! l'informazione che rende ricontrollabile tutto il resto.

use std::collections::{BTreeMap, BTreeSet};

use aether_domain::errors::AppError;
use serde_json::{Value, json};

use crate::Fornitori;
use crate::deposito::{VIVE_AFFINITA_MS, VIVE_AFFINITA_NIENTE_MS, Voce};

/// Il punto delle API.
///
/// **Non** è `api.listenbrainz.org`, che è il servizio con cui `aether-scrobble`
/// manda gli ascolti. Questo è l'ospite dei dataset: stessa fondazione, altra
/// macchina, altro patto — vedi la nota in `PRIVACY.md`.
const BASE: &str = "https://labs.api.listenbrainz.org";

/// Quando i nomi di questo modulo sono stati verificati sul servizio vivo.
///
/// Serve a chi leggerà fra un anno: se una richiesta comincia a rispondere
/// `400`, la prima cosa da fare è riaprire `/similar-recordings` e confrontare
/// l'elenco degli algoritmi con quello scritto qui. Senza questa data non si
/// saprebbe nemmeno da quanto tempo la costante è vecchia.
pub const VERIFICATO_IL: &str = "4 settembre 2026";

/// L'algoritmo delle somiglianze fra brani.
///
/// Verificato il [`VERIFICATO_IL`] fra le opzioni della pagina
/// `/similar-recordings`. Di quelle offerte si sceglie la finestra più larga
/// (`days_7500`, cioè vent'anni di ascolti) **senza** il taglio
/// `top_n_listeners_1000`: quel taglio calcola le somiglianze guardando solo i
/// mille ascoltatori più assidui, ed è un'ottima idea per togliere rumore e una
/// pessima per una libreria personale, dove è proprio il disco che i mille non
/// ascoltano quello per cui si vorrebbe un vicino.
///
/// Non si scrive mai a memoria: è un nome lungo che ListenBrainz ha già
/// cambiato più di una volta, e sbagliarne un pezzo dà un `400` — non un
/// risultato peggiore, proprio nessun risultato.
const ALGORITMO_BRANI: &str =
    "session_based_days_7500_session_300_contribution_5_threshold_15_limit_50_skip_30";

/// L'algoritmo delle somiglianze fra artisti.
///
/// Verificato il [`VERIFICATO_IL`] fra le opzioni della pagina
/// `/similar-artists`, ed è anche quello che MetaBrainz indica nel proprio
/// annuncio pubblico di questi dataset. Stessa finestra di
/// [`ALGORITMO_BRANI`]; i parametri diversi (`threshold_10`, `limit_100`,
/// `filter_True`) sono quelli con cui quel dataset è stato calcolato, non una
/// nostra preferenza — qui si può solo scegliere fra i nomi che esistono.
const ALGORITMO_ARTISTI: &str =
    "session_based_days_7500_session_300_contribution_5_threshold_10_limit_100_filter_True_skip_30";

/// Quanti vicini si tengono per ogni brano o artista.
///
/// Venticinque: vedi la nota in testa al modulo su cosa costa il ventiseiesimo.
pub const VICINI_MASSIMI: usize = 25;

/// Quanti identificativi si mandano in una richiesta sola.
///
/// Venticinque, ed è un compromesso misurato, non un numero tondo.
///
/// Il servizio tronca la risposta a circa [`RIGHE_PER_MBID`] righe **per
/// identificativo chiesto**, ma su tutto il lotto insieme e non a testa: le
/// righe vanno a chi ha i punteggi più alti. Con venticinque il bilancio medio
/// è di cento righe a brano, cioè quattro volte le [`VICINI_MASSIMI`] che si
/// tengono — il lotto deve essere sbilanciato quattro a uno prima che il brano
/// medio ci perda qualcosa.
///
/// Più grande farebbe meno richieste e risposte più sbilanciate; più piccolo il
/// contrario. Venticinque riduce una libreria di millequattrocento brani a
/// cinquantasei richieste, che al ritmo di
/// [`crate::cadenza::RITMO_LISTENBRAINZ`] è meno di un minuto: accorciarlo
/// ancora non comprerebbe niente che si possa sentire.
pub const MBID_PER_RICHIESTA: usize = 25;

/// Quante righe il servizio concede per ogni identificativo chiesto.
///
/// Cento, misurato il [`VERIFICATO_IL`]: una richiesta con un identificativo
/// torna con cento righe, una con tre ne torna con trecento, una con dieci con
/// novecentonovantotto. Il tetto è **complessivo**, e la distribuzione fra i
/// brani chiesti la decide il punteggio.
///
/// Non è documentato da nessuna parte: è quel che il servizio fa. Sta qui
/// perché è l'unico modo di sapere se una risposta è stata tagliata — vedi
/// [`Affini::satura`].
const RIGHE_PER_MBID: usize = 100;

/// Quanto vicino alla saturazione si considera già saturo, su dieci.
///
/// Nove decimi, e il margine serve: la risposta a un lotto di dieci misurata il
/// [`VERIFICATO_IL`] era di novecentonovantotto righe su un tetto di mille — due
/// righe sotto, e già pesantemente tagliata. Un confronto secco contro il tetto
/// avrebbe risposto «non tagliata» e concluso che sette brani su dieci non
/// hanno vicini.
const MARGINE_SATURAZIONE: usize = 9;

// ── quel che torna ──────────────────────────────────────────────────────────

/// Un brano vicino a un altro brano.
#[derive(Debug, Clone, PartialEq)]
pub struct Vicino {
    /// L'identificativo MusicBrainz della registrazione, in minuscolo.
    pub mbid: String,
    /// Il titolo, come lo scrive ListenBrainz.
    pub titolo: String,
    /// L'interprete, già composto dal servizio in una riga sola.
    pub artista: String,
    /// Quanto è vicino, da `0.0` a `1.0`.
    ///
    /// Già normalizzato dentro la risposta e per il brano chiesto: il primo
    /// vicino sta esattamente a `1.0`. **Non** è il punteggio grezzo del
    /// servizio, e non lo si può confrontare con quello di un'altra risposta
    /// per dire quale dei due brani è più famoso — è precisamente la cosa che
    /// la normalizzazione toglie di mezzo.
    pub affinita: f64,
}

/// Un artista vicino a un altro artista.
#[derive(Debug, Clone, PartialEq)]
pub struct VicinoArtista {
    /// L'identificativo MusicBrainz dell'artista, in minuscolo.
    pub mbid: String,
    /// Il nome.
    pub nome: String,
    /// Quanto è vicino, da `0.0` a `1.0`. Vale la nota di [`Vicino::affinita`].
    pub affinita: f64,
}

/// Quel che una risposta dice, già divisa per identificativo chiesto.
#[derive(Debug, Clone, PartialEq)]
pub struct Affini<V> {
    /// I vicini di ogni identificativo che la risposta nomina, dal più vicino.
    ///
    /// Un identificativo chiesto e **assente** da questa mappa non vuol dire
    /// «non ha vicini»: vedi [`Self::satura`].
    pub per_mbid: BTreeMap<String, Vec<V>>,
    /// La risposta ha toccato il tetto del servizio, quindi è stata tagliata.
    ///
    /// È l'unica cosa che permette di leggere un'assenza. Con `satura` falso il
    /// servizio ha detto tutto quel che aveva, e un identificativo che non
    /// compare non ha vicini davvero — si può ricordare come «niente», che è la
    /// metà di cache che risparmia di più. Con `satura` vero l'assenza può
    /// essere solo il taglio, e ricordarla sarebbe scrivere per un mese una cosa
    /// che non si sa.
    ///
    /// Vero anche quando il corpo non si legge affatto: un corpo illeggibile è
    /// il caso in cui si sa ancora meno.
    pub satura: bool,
}

impl<V> Affini<V> {
    /// La risposta che non dice niente e non permette di concludere niente.
    fn illeggibile() -> Self {
        Self {
            per_mbid: BTreeMap::new(),
            satura: true,
        }
    }
}

// ── il genere di vicinato ───────────────────────────────────────────────────

/// Quel che i due punti hanno in comune, cioè quasi tutto.
///
/// Le due risposte differiscono per **tre nomi di campo** e nient'altro: la
/// forma — un elenco piatto di righe, ognuna con il suo `reference_mbid` e il
/// suo `score` — è la stessa. Scrivere due volte il raggruppamento, la
/// normalizzazione, il taglio e la memoria vorrebbe dire tenerli allineati a
/// mano per sempre, e la prima correzione che si dimentica su uno dei due è un
/// difetto che si vede solo nei suggerimenti.
trait Vicinato: Sized + Clone {
    /// Il cassetto del deposito.
    ///
    /// Porta in fondo il numero dell'algoritmo: le affinità ricordate valgono
    /// per l'algoritmo con cui sono state calcolate, e i punteggi di due
    /// algoritmi diversi non si mescolano. **Se un giorno si cambia
    /// [`ALGORITMO_BRANI`] o [`ALGORITMO_ARTISTI`], questo numero va cambiato
    /// insieme**, o per tre mesi si leggeranno risposte di un algoritmo che non
    /// esiste più — e non lo direbbe nessun errore.
    const SERVIZIO: &'static str;
    /// Il percorso del punto, dopo [`BASE`].
    const PERCORSO: &'static str;
    /// Come si chiama il parametro che porta gli identificativi.
    const PARAMETRO: &'static str;
    /// L'algoritmo da chiedere.
    const ALGORITMO: &'static str;

    /// Legge una riga della risposta, con l'affinità ancora da calcolare.
    fn dalla_riga(riga: &Value) -> Option<Self>;
    /// L'identificativo del vicino, per l'ordinamento a pari punteggio.
    fn mbid(&self) -> &str;
    /// Lo stesso vicino con l'affinità che gli spetta.
    fn con_affinita(self, affinita: f64) -> Self;
    /// La forma con cui si ricorda nel deposito.
    fn in_json(&self) -> Value;
    /// Il contrario di [`Self::in_json`].
    fn dal_json(voce: &Value) -> Option<Self>;
}

impl Vicinato for Vicino {
    const SERVIZIO: &'static str = "lb-brani-affini-1";
    const PERCORSO: &'static str = "/similar-recordings/json";
    const PARAMETRO: &'static str = "recording_mbids";
    const ALGORITMO: &'static str = ALGORITMO_BRANI;

    fn dalla_riga(riga: &Value) -> Option<Self> {
        Some(Self {
            mbid: riga
                .get("recording_mbid")
                .and_then(Value::as_str)?
                .to_ascii_lowercase(),
            titolo: testo(riga, "recording_name"),
            artista: testo(riga, "artist_credit_name"),
            affinita: 0.0,
        })
    }

    fn mbid(&self) -> &str {
        &self.mbid
    }

    fn con_affinita(self, affinita: f64) -> Self {
        Self { affinita, ..self }
    }

    // Titolo e interprete si ricordano insieme all'affinità, e non è una copia
    // di comodo: senza, mostrare un suggerimento di un brano che in libreria
    // **non c'è** — che è il caso interessante — costringerebbe a una lettura di
    // MusicBrainz per ogni riga, cioè a spendere una richiesta al secondo per
    // riavere due stringhe che avevamo già in mano.
    fn in_json(&self) -> Value {
        json!({"m": self.mbid, "t": self.titolo, "a": self.artista, "p": self.affinita})
    }

    fn dal_json(voce: &Value) -> Option<Self> {
        Some(Self {
            mbid: voce.get("m").and_then(Value::as_str)?.to_owned(),
            titolo: testo(voce, "t"),
            artista: testo(voce, "a"),
            affinita: voce.get("p").and_then(Value::as_f64)?,
        })
    }
}

impl Vicinato for VicinoArtista {
    const SERVIZIO: &'static str = "lb-artisti-affini-1";
    const PERCORSO: &'static str = "/similar-artists/json";
    const PARAMETRO: &'static str = "artist_mbids";
    const ALGORITMO: &'static str = ALGORITMO_ARTISTI;

    fn dalla_riga(riga: &Value) -> Option<Self> {
        Some(Self {
            mbid: riga
                .get("artist_mbid")
                .and_then(Value::as_str)?
                .to_ascii_lowercase(),
            nome: testo(riga, "name"),
            affinita: 0.0,
        })
    }

    fn mbid(&self) -> &str {
        &self.mbid
    }

    fn con_affinita(self, affinita: f64) -> Self {
        Self { affinita, ..self }
    }

    fn in_json(&self) -> Value {
        json!({"m": self.mbid, "n": self.nome, "p": self.affinita})
    }

    fn dal_json(voce: &Value) -> Option<Self> {
        Some(Self {
            mbid: voce.get("m").and_then(Value::as_str)?.to_owned(),
            nome: testo(voce, "n"),
            affinita: voce.get("p").and_then(Value::as_f64)?,
        })
    }
}

/// Una stringa da un campo che potrebbe non esserci.
///
/// Un campo mancante non fa cadere la riga: `artist_credit_name` è arrivato
/// vuoto più di una volta, e un vicino senza nome scritto è comunque un vicino
/// con un identificativo — che è la parte che serve a ordinare.
fn testo(voce: &Value, campo: &str) -> String {
    voce.get(campo)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

// ── la lettura, che è pura ──────────────────────────────────────────────────

/// Il punteggio come numero in virgola mobile, senza conversioni che tagliano.
///
/// Il passaggio da `i32` è quel che permette di usare `f64::from`, che non
/// perde niente e non chiede di spiegare un cast: i punteggi veri stanno nelle
/// migliaia, e un valore fuori scala verrebbe da una risposta malformata — dove
/// tenere il tetto è comunque la cosa giusta.
fn come_numero(punteggio: i64) -> f64 {
    let dentro = punteggio.clamp(0, i64::from(i32::MAX));
    f64::from(i32::try_from(dentro).unwrap_or(i32::MAX))
}

/// L'affinità di un punteggio rispetto al massimo del suo gruppo.
///
/// Rapporto logaritmico e non lineare: la coda dei conteggi di co-occorrenza è
/// lunghissima, e un rapporto lineare metterebbe il secondo vicino a `0.55` e
/// il ventesimo a `0.03` — cioè butterebbe via l'ordine che si voleva
/// conservare. Il logaritmo tiene il primo a `1.0` esatto e lascia agli altri
/// una distanza leggibile.
///
/// Funzione pura, e il caso limite conta: con un massimo a zero non c'è niente
/// da normalizzare e ogni affinità è zero — mai una divisione per zero, mai un
/// `NaN` che attraverserebbe in silenzio tutto lo strato di punteggio.
#[must_use]
pub fn affinita(punteggio: i64, massimo: i64) -> f64 {
    let alto = come_numero(massimo).ln_1p();
    if alto <= 0.0 {
        return 0.0;
    }
    (come_numero(punteggio).ln_1p() / alto).clamp(0.0, 1.0)
}

/// La risposta è stata tagliata dal tetto del servizio.
///
/// Vedi [`Affini::satura`] per cosa se ne fa, e [`MARGINE_SATURAZIONE`] per
/// perché non è un confronto secco. Con zero identificativi chiesti il tetto è
/// zero e la risposta si considera satura: è il verso prudente: dice «non
/// concludere niente».
fn e_satura(righe: usize, quanti_chiesti: usize) -> bool {
    let tetto = RIGHE_PER_MBID.saturating_mul(quanti_chiesti);
    righe.saturating_mul(10) >= tetto.saturating_mul(MARGINE_SATURAZIONE)
}

/// Divide una risposta per identificativo chiesto, ordina e normalizza.
fn interpreta<V: Vicinato>(corpo: &[u8], quanti_chiesti: usize) -> Affini<V> {
    let Ok(Value::Array(righe)) = serde_json::from_slice::<Value>(corpo) else {
        return Affini::illeggibile();
    };
    let satura = e_satura(righe.len(), quanti_chiesti);

    // La risposta a un lotto **non** è né raggruppata né ordinata: le righe dei
    // brani chiesti arrivano mescolate, e dentro lo stesso brano i punteggi non
    // sono decrescenti. Con un identificativo solo lo sono, ed è la trappola:
    // chi provasse a fidarsi dell'ordine lo vedrebbe funzionare in ogni prova a
    // una domanda e sbagliare in produzione, dove le domande sono venticinque.
    let mut per_riferimento: BTreeMap<String, Vec<(i64, V)>> = BTreeMap::new();
    for riga in &righe {
        let Some(riferimento) = riga.get("reference_mbid").and_then(Value::as_str) else {
            continue;
        };
        let Some(vicino) = V::dalla_riga(riga) else {
            continue;
        };
        let punteggio = riga.get("score").and_then(Value::as_i64).unwrap_or(0);
        per_riferimento
            .entry(riferimento.to_ascii_lowercase())
            .or_default()
            .push((punteggio, vicino));
    }

    let mut per_mbid = BTreeMap::new();
    for (riferimento, mut voci) in per_riferimento {
        // A pari punteggio decide l'identificativo: senza, l'ordine dei vicini
        // dipenderebbe da quello in cui il servizio li ha scritti, e due
        // passate identiche produrrebbero due elenchi diversi da riscrivere in
        // libreria per niente.
        voci.sort_by(|(a, va), (b, vb)| b.cmp(a).then_with(|| va.mbid().cmp(vb.mbid())));
        voci.truncate(VICINI_MASSIMI);
        let massimo = voci.first().map_or(0, |(punteggio, _)| *punteggio);
        per_mbid.insert(
            riferimento,
            voci.into_iter()
                .map(|(punteggio, vicino)| vicino.con_affinita(affinita(punteggio, massimo)))
                .collect(),
        );
    }

    Affini { per_mbid, satura }
}

/// Interpreta una risposta di `/similar-recordings/json`. Funzione pura.
///
/// `quanti_chiesti` è il numero di identificativi che la richiesta portava, e
/// serve solo a stabilire [`Affini::satura`]: senza di quello non si può dire
/// se un'assenza sia un fatto o un taglio.
#[must_use]
pub fn interpreta_brani(corpo: &[u8], quanti_chiesti: usize) -> Affini<Vicino> {
    interpreta::<Vicino>(corpo, quanti_chiesti)
}

/// Interpreta una risposta di `/similar-artists/json`. Funzione pura.
///
/// Vedi [`interpreta_brani`] per `quanti_chiesti`.
#[must_use]
pub fn interpreta_artisti(corpo: &[u8], quanti_chiesti: usize) -> Affini<VicinoArtista> {
    interpreta::<VicinoArtista>(corpo, quanti_chiesti)
}

// ── la memoria, per identificativo ──────────────────────────────────────────

/// I vicini ricordati, come byte da mettere nel deposito.
fn elenco_in_json<V: Vicinato>(vicini: &[V]) -> Vec<u8> {
    let elenco = Value::Array(vicini.iter().map(V::in_json).collect());
    serde_json::to_vec(&elenco).unwrap_or_default()
}

/// I vicini ricordati, riletti. `None` se il corpo non si legge per intero.
///
/// Tutto o niente di proposito: una voce di cache scritta da una versione con
/// un'altra forma non deve diventare un elenco a metà, deve diventare una
/// richiesta.
fn elenco_dal_json<V: Vicinato>(corpo: &[u8]) -> Option<Vec<V>> {
    let Ok(Value::Array(voci)) = serde_json::from_slice::<Value>(corpo) else {
        return None;
    };
    voci.iter().map(V::dal_json).collect()
}

// ── le richieste ────────────────────────────────────────────────────────────

/// La stringa ha la forma di un identificativo MusicBrainz.
///
/// Otto-quattro-quattro-quattro-dodici cifre esadecimali. Non è pignoleria: il
/// servizio valida gli identificativi **prima** di guardarli, e uno malformato
/// fa fallire con un `400` l'intera richiesta — cioè fa perdere i venticinque
/// brani buoni che gli stavano accanto. Un `mb_recording_id` storto in libreria
/// (un campo copiato a mano, un tag arrivato da un altro programma) diventerebbe
/// così un buco di venticinque brani che si sposta a ogni passata.
///
/// È anche la ragione per cui gli identificativi finiscono nell'indirizzo senza
/// codifica percentuale: dopo questo controllo contengono soltanto cifre
/// esadecimali e trattini.
fn e_un_mbid(grezzo: &str) -> bool {
    grezzo.len() == 36
        && grezzo.chars().enumerate().all(|(posizione, carattere)| {
            if matches!(posizione, 8 | 13 | 18 | 23) {
                carattere == '-'
            } else {
                carattere.is_ascii_hexdigit()
            }
        })
}

/// L'indirizzo per un lotto di identificativi.
///
/// Gli identificativi si ripetono come **parametro separato** — `?x=a&x=b` — e
/// non si uniscono con una virgola. Verificato il [`VERIFICATO_IL`]: la forma
/// con la virgola risponde `400 value is not a valid uuid`, perché il servizio
/// legge il parametro ripetuto come una lista e la stringa unita come un solo
/// identificativo lungo settantatré caratteri.
fn indirizzo<V: Vicinato>(lotto: &[String]) -> String {
    let mut url = format!("{BASE}{}?algorithm={}", V::PERCORSO, V::ALGORITMO);
    for chiave in lotto {
        url.push('&');
        url.push_str(V::PARAMETRO);
        url.push('=');
        url.push_str(chiave);
    }
    url
}

/// Il corpo dei vicini, dal deposito o dalla rete, per un gruppo di
/// identificativi.
fn affini<V: Vicinato>(
    fornitori: &Fornitori,
    mbid: &[&str],
) -> Result<BTreeMap<String, Vec<V>>, AppError> {
    let mut fuori: BTreeMap<String, Vec<V>> = BTreeMap::new();
    let mut da_chiedere: Vec<String> = Vec::new();
    let mut visti: BTreeSet<String> = BTreeSet::new();

    for grezzo in mbid {
        // Minuscolo perché la chiave di cache dev'essere una sola per
        // identificativo: MusicBrainz scrive i propri in minuscolo, ma un tag
        // riempito da un altro programma può portarli in maiuscolo, e due
        // grafie dello stesso brano sarebbero due voci di deposito e due
        // richieste.
        let chiave = grezzo.trim().to_ascii_lowercase();
        if !e_un_mbid(&chiave) || !visti.insert(chiave.clone()) {
            continue;
        }
        match fornitori.deposito.leggi(V::SERVIZIO, &chiave) {
            Some(Voce::Corpo(corpo)) => {
                if let Some(vicini) = elenco_dal_json::<V>(&corpo) {
                    fuori.insert(chiave, vicini);
                    continue;
                }
            }
            // «L'ho chiesto e non c'è», che è lo stato che risparmia di più:
            // la maggior parte di una libreria vera non sta in questa base
            // dati.
            Some(Voce::Niente) => {
                fuori.insert(chiave, Vec::new());
                continue;
            }
            None => {}
        }
        da_chiedere.push(chiave);
    }

    for lotto in da_chiedere.chunks(MBID_PER_RICHIESTA) {
        let indirizzo = indirizzo::<V>(lotto);
        let corpo = match fornitori.json_senza_memoria(&fornitori.listenbrainz, &indirizzo) {
            Ok(Some(corpo)) => corpo,
            // Un `404` qui **non** vuol dire «questi brani non hanno vicini»:
            // per un identificativo sconosciuto il servizio risponde `200` con
            // un elenco vuoto. Un `404` vuol dire che il punto non c'è più —
            // un ospite di dataset può ritirarne uno — e scriverlo nel deposito
            // marcherebbe per un mese come «senza vicini» ogni brano di ogni
            // lotto. Si smette e basta: il vicinato è una cosa in meno, non una
            // cosa sbagliata.
            Ok(None) => break,
            // Un guasto a metà non butta via i lotti già letti: si restituisce
            // quel che si ha e il resto lo trova la passata dopo. Quando invece
            // non c'è ancora niente, l'errore è tutto quel che si ha, e chi
            // chiama deve poterlo leggere come «non si sa».
            Err(err) if fuori.is_empty() => return Err(err),
            Err(_) => break,
        };

        let letto = interpreta::<V>(&corpo, lotto.len());
        for chiave in lotto {
            match letto.per_mbid.get(chiave) {
                Some(vicini) => {
                    fornitori.deposito.scrivi(
                        V::SERVIZIO,
                        chiave,
                        &Voce::Corpo(elenco_in_json(vicini)),
                        VIVE_AFFINITA_MS,
                    );
                    fuori.insert(chiave.clone(), vicini.clone());
                }
                None if !letto.satura => {
                    fornitori.deposito.scrivi(
                        V::SERVIZIO,
                        chiave,
                        &Voce::Niente,
                        VIVE_AFFINITA_NIENTE_MS,
                    );
                    fuori.insert(chiave.clone(), Vec::new());
                }
                // Assente da una risposta tagliata: non si sa, e non si scrive
                // niente. Il lotto della passata dopo sarà composto in un altro
                // modo e forse gli lascerà spazio.
                None => {}
            }
        }
    }

    Ok(fuori)
}

/// I brani che ascolta chi ascolta questi.
///
/// Prende **molti** identificativi e li divide da sé in richieste da
/// [`MBID_PER_RICHIESTA`], leggendo il deposito prima e scrivendolo dopo, un
/// identificativo per volta. Chiamarla con un elemento è legittimo e costa una
/// richiesta; chiamarla mille volte con un elemento costa mille richieste, e
/// non è così che va usata.
///
/// Nella mappa che torna, la chiave è l'identificativo **chiesto** (in
/// minuscolo) e il valore i suoi vicini dal più vicino, al massimo
/// [`VICINI_MASSIMI`]. Le tre risposte del deposito si leggono così:
///
/// - una chiave con dei vicini: si sa, ed eccoli;
/// - una chiave con un elenco **vuoto**: si sa, e non ne ha;
/// - una chiave **assente**: non si sa — l'identificativo era malformato,
///   oppure la risposta era tagliata, oppure quel lotto non è mai partito.
///
/// Gli identificativi malformati e i doppioni si scartano senza dire niente:
/// uno solo malformato farebbe fallire con un `400` la richiesta di tutti gli
/// altri.
///
/// # Errori
///
/// L'errore di rete così com'è — `net.offline`, `net.timeout`, `net.http`,
/// `net.circuitOpen` — e solo quando **non si è ottenuto niente**: chi chiama lo
/// tratta come «non si sa». Un guasto che arriva dopo che qualche lotto è già
/// tornato restituisce invece quel che si è raccolto, perché una mappa parziale
/// è un'informazione e un `Err` la butterebbe via.
pub fn brani_affini(
    fornitori: &Fornitori,
    mbid: &[&str],
) -> Result<BTreeMap<String, Vec<Vicino>>, AppError> {
    affini::<Vicino>(fornitori, mbid)
}

/// Gli artisti che ascolta chi ascolta questi.
///
/// Vale parola per parola quel che dice [`brani_affini`], con gli
/// identificativi degli artisti al posto di quelli delle registrazioni.
///
/// # Errori
///
/// Gli stessi di [`brani_affini`].
pub fn artisti_affini(
    fornitori: &Fornitori,
    mbid: &[&str],
) -> Result<BTreeMap<String, Vec<VicinoArtista>>, AppError> {
    affini::<VicinoArtista>(fornitori, mbid)
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::deposito::Deposito;
    use std::sync::Mutex;

    /// Un deposito che si può riempire prima e ispezionare dopo.
    ///
    /// È il modo in cui questo crate si prova senza rete: non c'è nessun
    /// finto server: c'è una memoria piena, e una funzione che con la memoria
    /// piena non ha nessun motivo di uscire di casa.
    #[derive(Debug, Default)]
    struct Memoria {
        voci: Mutex<Vec<(String, String, Voce, i64)>>,
    }

    impl Memoria {
        fn semina(&self, servizio: &str, chiave: &str, voce: Voce) {
            self.scrivi(servizio, chiave, &voce, 0);
        }
    }

    impl Deposito for Memoria {
        fn leggi(&self, servizio: &str, chiave: &str) -> Option<Voce> {
            let voci = self.voci.lock().ok()?;
            voci.iter()
                .rev()
                .find(|(s, c, _, _)| s == servizio && c == chiave)
                .map(|(_, _, voce, _)| voce.clone())
        }

        fn scrivi(&self, servizio: &str, chiave: &str, voce: &Voce, vive_ms: i64) {
            if let Ok(mut voci) = self.voci.lock() {
                voci.push((
                    servizio.to_owned(),
                    chiave.to_owned(),
                    voce.clone(),
                    vive_ms,
                ));
            }
        }
    }

    /// Una risposta finta con la forma vera: elenco piatto, righe mescolate.
    fn riga(riferimento: &str, mbid: &str, titolo: &str, punteggio: i64) -> Value {
        json!({
            "recording_mbid": mbid,
            "recording_name": titolo,
            "artist_credit_name": "Un Artista",
            "artist_credit_mbids": Value::Null,
            "release_name": "Un Disco",
            "score": punteggio,
            "reference_mbid": riferimento,
        })
    }

    const HIT: &str = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";
    const NICCHIA: &str = "40bd0203-bc3f-4b15-9ab3-ceeb2ef35d7a";

    fn vicino(numero: u8) -> String {
        format!("00000000-0000-0000-0000-0000000000{numero:02x}")
    }

    #[test]
    fn una_risposta_a_lotto_si_divide_per_riferimento() {
        // Le righe arrivano mescolate fra i due brani chiesti e fuori ordine
        // dentro ciascuno: è la forma vera della risposta a un lotto, ed è la
        // ragione per cui non ci si può fidare dell'ordine ricevuto.
        let corpo = serde_json::to_vec(&json!([
            riga(HIT, &vicino(1), "Uno", 400),
            riga(NICCHIA, &vicino(9), "Nove", 20),
            riga(HIT, &vicino(2), "Due", 1200),
            riga(NICCHIA, &vicino(8), "Otto", 7),
        ]))
        .expect("il corpo di prova si serializza");

        let letto = interpreta_brani(&corpo, 2);
        assert_eq!(letto.per_mbid.len(), 2);

        let del_hit = letto.per_mbid.get(HIT).expect("il primo brano chiesto");
        assert_eq!(
            del_hit
                .iter()
                .map(|v| v.titolo.as_str())
                .collect::<Vec<_>>(),
            vec!["Due", "Uno"],
            "si riordina per punteggio: la risposta non lo era"
        );
    }

    #[test]
    fn il_primo_vicino_sta_a_uno_in_ogni_gruppo() {
        // È il punto di tutta la normalizzazione: il vicino migliore di un
        // disco di nicchia vale quanto il vicino migliore di un successo
        // mondiale, anche se i due punteggi grezzi stanno a due ordini di
        // grandezza di distanza. Normalizzare sul massimo dell'intera risposta
        // — invece che su quello del gruppo — schiaccerebbe la nicchia a zero,
        // che è esattamente il difetto che questa funzione esiste per evitare.
        let corpo = serde_json::to_vec(&json!([
            riga(HIT, &vicino(1), "Successo", 1200),
            riga(HIT, &vicino(2), "Secondo", 600),
            riga(NICCHIA, &vicino(3), "Rarita", 20),
            riga(NICCHIA, &vicino(4), "Seconda", 10),
        ]))
        .expect("il corpo di prova si serializza");

        let letto = interpreta_brani(&corpo, 2);
        let primo = |chiave: &str| {
            letto
                .per_mbid
                .get(chiave)
                .and_then(|v| v.first())
                .map(|v| v.affinita)
        };
        assert_eq!(primo(HIT), Some(1.0));
        assert_eq!(primo(NICCHIA), Some(1.0));

        // E il secondo di ciascuno resta sotto il primo ma ben sopra lo zero:
        // un rapporto lineare avrebbe messo il secondo del successo a 0,5 e
        // quello della nicchia a 0,5, ma la coda dei conteggi non è lineare.
        for chiave in [HIT, NICCHIA] {
            let secondo = letto
                .per_mbid
                .get(chiave)
                .and_then(|v| v.get(1))
                .map(|v| v.affinita)
                .unwrap_or_default();
            assert!(
                (0.5..1.0).contains(&secondo),
                "il secondo vicino di {chiave} sta a {secondo}"
            );
        }
    }

    #[test]
    fn si_tengono_venticinque_vicini_e_non_di_piu() {
        let mut righe = Vec::new();
        for numero in 0..60_u8 {
            righe.push(riga(
                HIT,
                &vicino(numero),
                "X",
                i64::from(numero).saturating_add(1),
            ));
        }
        let corpo = serde_json::to_vec(&Value::Array(righe)).expect("si serializza");
        let letto = interpreta_brani(&corpo, 1);
        let vicini = letto.per_mbid.get(HIT).expect("il brano chiesto");
        assert_eq!(vicini.len(), VICINI_MASSIMI);
        // Si tengono i **primi**, non i primi venticinque arrivati.
        assert_eq!(vicini.first().map(|v| v.affinita), Some(1.0));
        assert!(
            vicini
                .iter()
                .all(|v| (0.0..=1.0).contains(&v.affinita) && v.affinita.is_finite())
        );
    }

    #[test]
    fn una_risposta_tagliata_non_permette_di_concludere_un_assenza() {
        // Il caso misurato: dieci identificativi chiesti, novecentonovantotto
        // righe tornate su un tetto di mille. Un confronto secco contro il
        // tetto direbbe «non tagliata», e sette brani su dieci finirebbero nel
        // deposito come «senza vicini» per un mese.
        assert!(e_satura(998, 10), "due righe sotto il tetto è già tagliata");
        assert!(e_satura(100, 1), "cento righe su una domanda sola: piena");
        assert!(
            !e_satura(100, 6),
            "cento righe su seicento possibili: il servizio ha detto tutto"
        );
        assert!(!e_satura(0, 1), "nessuna riga per una domanda: non ne ha");
        assert!(e_satura(0, 0), "senza domande non si conclude niente");
    }

    #[test]
    fn un_corpo_storto_non_fa_cadere_niente_e_non_conclude_niente() {
        for corpo in [&b"non json"[..], b"{}", b"{\"recordings\":[]}"] {
            let letto = interpreta_brani(corpo, 3);
            assert!(letto.per_mbid.is_empty());
            assert!(
                letto.satura,
                "da un corpo che non si legge non si deduce nessuna assenza"
            );
        }
        // Un elenco vero ma vuoto invece **è** una risposta: il servizio ha
        // detto che non ha niente per quei tre.
        let letto = interpreta_brani(b"[]", 3);
        assert!(letto.per_mbid.is_empty());
        assert!(!letto.satura);
    }

    #[test]
    fn gli_artisti_hanno_i_loro_nomi_di_campo() {
        // `artist_mbid` e `name`, non `recording_mbid` e `recording_name`:
        // sono i due punti in cui le due risposte differiscono, e sbagliarli
        // darebbe una mappa vuota senza nessun errore.
        let corpo = serde_json::to_vec(&json!([
            {"artist_mbid": &vicino(1), "name": "David Bowie", "comment": "",
             "type": "Person", "gender": "Male", "score": 11147, "reference_mbid": HIT},
            {"artist_mbid": &vicino(2), "name": "The Beatles", "comment": "",
             "type": "Group", "gender": Value::Null, "score": 10557, "reference_mbid": HIT},
        ]))
        .expect("si serializza");
        let letto = interpreta_artisti(&corpo, 1);
        let vicini = letto.per_mbid.get(HIT).expect("l'artista chiesto");
        assert_eq!(vicini.len(), 2);
        assert_eq!(vicini.first().map(|v| v.nome.as_str()), Some("David Bowie"));
        assert_eq!(vicini.first().map(|v| v.affinita), Some(1.0));
    }

    #[test]
    fn un_identificativo_storto_non_puo_far_fallire_il_lotto() {
        assert!(e_un_mbid(HIT));
        assert!(e_un_mbid("00000000-0000-0000-0000-000000000000"));
        assert!(!e_un_mbid(""));
        assert!(
            !e_un_mbid("b1a9c0e9d9874042ae9178d6a3267d69"),
            "senza trattini"
        );
        assert!(!e_un_mbid("b1a9c0e9-d987-4042-ae91-78d6a3267d6"), "corto");
        assert!(
            !e_un_mbid("g1a9c0e9-d987-4042-ae91-78d6a3267d69"),
            "non esadecimale"
        );
        assert!(
            !e_un_mbid("b1a9c0e9_d987_4042_ae91_78d6a3267d69"),
            "trattini bassi"
        );
    }

    #[test]
    fn l_indirizzo_ripete_il_parametro_invece_di_unire_con_la_virgola() {
        // La forma con la virgola risponde `400 value is not a valid uuid`:
        // verificata, non dedotta.
        let lotto = vec![HIT.to_owned(), NICCHIA.to_owned()];
        let url = indirizzo::<Vicino>(&lotto);
        assert!(url.starts_with(&format!("{BASE}/similar-recordings/json?algorithm=")));
        assert!(url.contains(ALGORITMO_BRANI));
        assert_eq!(url.matches("recording_mbids=").count(), 2);
        assert!(!url.contains(','));

        let url = indirizzo::<VicinoArtista>(&lotto);
        assert!(url.contains("/similar-artists/json"));
        assert_eq!(url.matches("artist_mbids=").count(), 2);
    }

    #[test]
    fn quel_che_si_ricorda_si_rilegge_uguale() {
        let vicini = vec![
            Vicino {
                mbid: vicino(1),
                titolo: "Hotel California".to_owned(),
                artista: "Eagles".to_owned(),
                affinita: 1.0,
            },
            Vicino {
                mbid: vicino(2),
                titolo: "Stairway to Heaven".to_owned(),
                artista: "Led Zeppelin".to_owned(),
                affinita: 0.75,
            },
        ];
        let corpo = elenco_in_json(&vicini);
        assert_eq!(elenco_dal_json::<Vicino>(&corpo).as_ref(), Some(&vicini));
        // Una voce scritta con un'altra forma diventa una richiesta, non un
        // elenco a metà.
        assert!(elenco_dal_json::<Vicino>(b"[{\"m\":1}]").is_none());
        assert!(elenco_dal_json::<Vicino>(b"non json").is_none());
    }

    #[test]
    fn l_affinita_non_produce_mai_un_valore_da_buttare() {
        assert_eq!(affinita(0, 0), 0.0, "niente divisioni per zero");
        assert_eq!(affinita(100, 0), 0.0);
        assert_eq!(affinita(100, 100), 1.0);
        assert_eq!(affinita(-5, 100), 0.0, "un punteggio negativo non esiste");
        // Un punteggio più alto del massimo del gruppo non può capitare — il
        // massimo è il primo dell'elenco — ma se capitasse resterebbe dentro.
        assert_eq!(affinita(1000, 100), 1.0);
        assert!(affinita(50, 1200) > 0.0);
        assert!(affinita(50, 1200) < affinita(600, 1200));
    }

    #[test]
    fn con_la_memoria_piena_non_si_esce_di_casa() {
        // Nessun finto server: se questa passasse toccando la rete, ci
        // metterebbe secondi e fallirebbe su una macchina staccata. Passa
        // perché ogni identificativo chiesto è già nel deposito.
        let memoria = Memoria::default();
        let vicini = vec![Vicino {
            mbid: vicino(7),
            titolo: "Un Vicino".to_owned(),
            artista: "Un Artista".to_owned(),
            affinita: 1.0,
        }];
        memoria.semina(
            <Vicino as Vicinato>::SERVIZIO,
            HIT,
            Voce::Corpo(elenco_in_json(&vicini)),
        );
        memoria.semina(<Vicino as Vicinato>::SERVIZIO, NICCHIA, Voce::Niente);

        let fornitori = Fornitori::nuovo(Box::new(memoria));
        let esito = brani_affini(&fornitori, &[HIT, NICCHIA, "storto", HIT])
            .expect("tutto era già ricordato");

        assert_eq!(esito.len(), 2, "il doppione e lo storto non sono chiavi");
        assert_eq!(esito.get(HIT), Some(&vicini));
        assert_eq!(
            esito.get(NICCHIA),
            Some(&Vec::new()),
            "un «niente» ricordato è un elenco vuoto, non una chiave assente"
        );
    }

    #[test]
    fn un_maiuscolo_nei_tag_non_diventa_una_seconda_richiesta() {
        let memoria = Memoria::default();
        memoria.semina(<Vicino as Vicinato>::SERVIZIO, HIT, Voce::Niente);
        let fornitori = Fornitori::nuovo(Box::new(memoria));
        let esito = brani_affini(&fornitori, &[&HIT.to_ascii_uppercase(), HIT])
            .expect("la chiave è la stessa, e sta nel deposito");
        assert_eq!(esito.len(), 1);
        assert!(esito.contains_key(HIT));
    }

    #[test]
    fn con_l_interruttore_aperto_si_dice_non_si_sa() {
        let fornitori = Fornitori::nuovo(Box::new(Memoria::default()));
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            fornitori.listenbrainz.guasto();
        }
        assert!(!fornitori.affinita_in_piedi());
        // MusicBrainz è in piedi: un servizio non deve poter spegnere l'altro.
        assert!(fornitori.in_piedi());

        let esito = brani_affini(&fornitori, &[HIT]);
        let Err(errore) = esito else {
            panic!("senza niente in mano l'interruttore aperto è un errore");
        };
        assert_eq!(errore.code().kind().code(), "net.circuitOpen");
    }

    #[test]
    fn un_lotto_caduto_non_butta_via_quelli_gia_letti() {
        // Un identificativo è ricordato, l'altro no, e l'interruttore è aperto:
        // quel che si ha in mano vale più di un errore, perché chi chiama
        // leggerebbe l'errore come «non si sa» e non scriverebbe niente.
        let memoria = Memoria::default();
        memoria.semina(<Vicino as Vicinato>::SERVIZIO, HIT, Voce::Niente);
        let fornitori = Fornitori::nuovo(Box::new(memoria));
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            fornitori.listenbrainz.guasto();
        }
        let esito = brani_affini(&fornitori, &[HIT, NICCHIA]).expect("il primo era in mano");
        assert_eq!(esito.len(), 1);
        assert!(
            !esito.contains_key(NICCHIA),
            "una chiave assente è «non si sa», e non va confusa con un elenco vuoto"
        );
    }

    #[test]
    fn i_tempi_di_vita_sono_quelli_della_scadenza_lunga() {
        // Una prova sulle costanti, perché sono l'unica parte di questo modulo
        // che si può sbagliare senza che nessuna richiesta fallisca — e in
        // blocco `const`, così il difetto non arriva nemmeno a compilare.
        const {
            assert!(
                VIVE_AFFINITA_MS > crate::deposito::VIVE_PUBBLICAZIONE_MS,
                "un'affinità è più stabile della pubblicazione più stabile"
            );
            assert!(
                VIVE_AFFINITA_NIENTE_MS > crate::deposito::VIVE_NIENTE_MS,
                "qui nessun collaboratore può riempire un buco stanotte"
            );
            assert!(
                VIVE_AFFINITA_NIENTE_MS < VIVE_AFFINITA_MS,
                "un no resta comunque meno affidabile di un sì"
            );
        }
    }
}
