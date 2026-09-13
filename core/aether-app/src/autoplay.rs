//! Cosa suonare quando la coda è finita.
//!
//! # Il problema che risolve
//!
//! A coda esaurita Aether si fermava. Non è un guasto — ha fatto quel che gli
//! era stato chiesto — ma è l'unico momento in cui l'applicazione smette di
//! funzionare da sola, e capita a ogni ascolto: si mette un album, l'album
//! finisce, e il silenzio è la risposta.
//!
//! # Cosa c'era scritto qui, e cosa è cambiato
//!
//! Qui si leggeva «perché non è un raccomandatore», e l'argomento era che non
//! c'è nessun servizio da interrogare — che è il punto del progetto — e nessun
//! grafo dei gusti da costruire su una libreria di qualche migliaio di brani.
//!
//! La prima metà è ancora vera, ed è la ragione per cui vale la pena riscrivere
//! il paragrafo invece di cancellarlo: **al momento di scegliere non si chiede
//! niente a nessuno**. Questa funzione gira sul filo che prepara il brano
//! successivo mentre quello corrente sta ancora suonando, e una richiesta di
//! rete lì dentro sarebbe un buco udibile ogni volta che la connessione fa i
//! capricci.
//!
//! La seconda metà è cambiata: un grafo adesso c'è. Lo costruisce
//! [`crate::sonora`] leggendo i file una volta, e — se l'utente lo lascia
//! acceso — lo completa con gli ascolti aggregati di ListenBrainz. Qui dentro
//! arriva già fatto, in tre letture del database.
//!
//! Quel che **non** è cambiato è chi decide quando non si sa niente: la cascata.
//!
//! # La cascata, che è rimasta il pavimento
//!
//! Le regole si provano in ordine e la prima che dà un risultato vince. Il
//! primo gradino sceglie da solo; dal secondo in giù l'affinità **riordina** i
//! candidati che quel gradino ha già trovato, senza cambiarli e senza saltarne
//! nessuno. Senza una scala con cui misurarla — prima della prima analisi, su
//! una libreria troppo piccola, su un brano corrente mai analizzato — si prende
//! il primo della lista, che è quel che questo modulo ha sempre fatto.
//!
//! 1. **Il resto dell'album** — se si stava ascoltando un disco, il disco
//!    continua. È la risposta giusta più spesso di qualunque affinità, e infatti
//!    è l'unico gradino che l'affinità non tocca: un disco è un disco, e nessuna
//!    somiglianza ha titolo per spezzarlo a metà.
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

use std::collections::HashMap;

use rusqlite::Connection;

use aether_domain::affinita::{self, Ascolto, Pesi, Scala};
use aether_domain::errors::AppError;
use aether_domain::regole::{Campo, Combinazione, Insieme, Operatore, Ordinamento, Regola, Valore};
use aether_play::impronta::RAGGRUPPAMENTO;

use crate::library::{self, TrackSummary};
use crate::smart;
use crate::sonora;

/// Da quanti giorni un brano dev'essere fermo per contare come «non di recente».
///
/// Un mese: abbastanza perché una scaletta di un pomeriggio non si riproponga,
/// poco abbastanza perché una libreria di qualche centinaio di brani abbia
/// sempre qualcosa da offrire.
const GIORNI_RECENTI: i64 = 30;

/// Quanti candidati chiedere a ogni passo della cascata.
///
/// Erano ventiquattro, e ventiquattro bastavano a quel che serviva allora: i
/// brani già in coda si scartano qui e non nella query, e chiederne uno solo
/// avrebbe fatto scendere a un criterio peggiore ogni volta che quell'uno era
/// già stato sentito.
///
/// Duecento perché adesso il passo non prende il primo che passa: lo
/// **riordina** per affinità quando c'è una scala con cui misurarla (vedi
/// [`Contesto`]). Un bacino di ventiquattro darebbe a quell'ordinamento troppo
/// poco da ordinare — e duecento è il punto in cui il costo resta quello di
/// duecento letture per chiave, cioè meno di un millisecondo, su un filo che
/// intanto ha un brano intero di tempo davanti.
const CANDIDATI: u32 = 200;

/// Perché questo brano e non un altro.
///
/// # Perché la coda si spiega
///
/// Perché quando la musica continua da sola, la domanda che uno si fa è «e
/// questo da dove salta fuori». Un servizio in streaming non può rispondere:
/// dovrebbe ammettere quando sta spingendo qualcosa. Qui non c'è niente da
/// spingere — la scelta esce da tre numeri e da una cascata, e dirlo costa una
/// riga.
///
/// # Perché un `enum` e non una frase
///
/// Perché la frase va tradotta, e una stringa italiana che attraversa l'IPC è
/// una stringa italiana anche per chi ha l'interfaccia in inglese. Il nucleo
/// manda il **motivo**, `lingue/` lo scrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motivo {
    /// Il resto del disco. Non è una scelta: è continuare.
    Album,
    /// Un altro disco dello stesso artista.
    Artista,
    /// Lo stesso genere.
    Genere,
    /// Un preferito mai sentito.
    Preferito,
    /// Uno qualunque, che è l'ultimo gradino della cascata.
    Caso,
    /// Suona come quello di prima.
    Suono,
    /// Chi ascolta questo ascolta anche quello.
    Ascolti,
    /// Ti è piaciuto, e si vede da quanto ne hai sentito.
    Gusto,
    /// Non lo senti da mesi.
    DaTanto,
}

impl Motivo {
    /// Il codice che attraversa l'IPC, e che `lingue/` traduce.
    #[must_use]
    pub const fn codice(self) -> &'static str {
        match self {
            Self::Album => "album",
            Self::Artista => "artista",
            Self::Genere => "genere",
            Self::Preferito => "preferito",
            Self::Caso => "caso",
            Self::Suono => "suono",
            Self::Ascolti => "ascolti",
            Self::Gusto => "gusto",
            Self::DaTanto => "datanto",
        }
    }
}

/// Un brano scelto, col perché.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scelta {
    /// Quale brano.
    pub id: i64,
    /// Perché lui.
    pub motivo: Motivo,
}

/// Sopra quanto la somiglianza sonora è una spiegazione e non un caso.
///
/// Due brani estranei stanno a 0,5 esatto — è la conseguenza della
/// normalizzazione, scritta in [`aether_domain::affinita::RIFERIMENTO`] — quindi
/// la soglia sta sopra: sotto questo valore «suona come quello di prima» sarebbe
/// una frase che descrive due brani qualunque.
pub const SOGLIA_SUONO: f32 = 0.68;

/// Sopra quanto un vicino di ListenBrainz è un vicino da nominare.
///
/// I punteggi sono normalizzati **dentro la risposta a cui appartengono**: 0,5 è
/// metà del vicino più forte di quel brano, che è già una co-occorrenza netta.
pub const SOGLIA_ASCOLTI: f32 = 0.5;

/// Sopra quanto «ti piace» è una spiegazione.
pub const SOGLIA_GUSTO: f32 = 0.6;

/// Da quanto un brano dev'essere fermo perché il tempo diventi il motivo.
///
/// Sei mesi, come il ripiano «Trascurati» della Home: due idee diverse di «da
/// tanto» nella stessa applicazione sarebbero due, e questa è la seconda.
pub const DA_TANTO_MS: i64 = 180 * 24 * 60 * 60 * 1000;

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
) -> Result<Option<Scelta>, AppError> {
    let brano = library::read_summary(connection, corrente)?;

    // Il primo gradino non lo tocca nessuno: un disco è un disco, e nessuna
    // affinità ha titolo per spezzarlo a metà. Il contesto si legge **dopo**,
    // così chi sta ascoltando un album non paga nemmeno le tre letture.
    if let Some(brano) = brano.as_ref()
        && let Some(id) = resto_dell_album(connection, brano, esclusi)?
    {
        return Ok(Some(Scelta {
            id,
            motivo: Motivo::Album,
        }));
    }

    // Una lettura sola per tutti e quattro i passi che riordinano. `None`
    // finché la passata di analisi non ha prodotto una scala: da lì in giù il
    // modulo si comporta esattamente come prima.
    let contesto = Contesto::leggi(connection, corrente)?;
    let contesto = contesto.as_ref();

    if let Some(brano) = brano.as_ref() {
        if let Some(scelta) = stesso_artista(connection, brano, esclusi, adesso_ms, contesto)? {
            return Ok(Some(scelta));
        }
        if let Some(scelta) = stesso_genere(connection, corrente, esclusi, adesso_ms, contesto)? {
            return Ok(Some(scelta));
        }
    }

    if let Some(scelta) = un_preferito_mai_sentito(connection, esclusi, adesso_ms, contesto)? {
        return Ok(Some(scelta));
    }
    uno_qualunque(connection, esclusi, adesso_ms, contesto)
}

/// Quanti brani semina una radio.
///
/// Trenta, cioè quasi due ore. Non è la lunghezza della sessione — quando
/// finiscono, [`prossimo`] continua come sempre — è quanto se ne vede nel
/// pannello della coda: abbastanza da capire dove sta andando, non tanto da
/// dover scorrere per trovare il brano corrente.
pub const RADIO: usize = 30;

/// Quanti brani di fila può avere lo stesso artista in una radio.
///
/// Tre. Senza un tetto, una radio seminata da un disco poco comune diventa quel
/// disco: i brani sonoramente più vicini a un brano sono quasi sempre gli altri
/// dello stesso album, ed è vero e inutile. La radio esiste per portare
/// **altrove**, e il tetto è il modo più semplice di dirlo — più semplice, e
/// meno arbitrario, di un peso che penalizzi l'artista dentro il punteggio.
pub const TETTO_ARTISTA: usize = 3;

/// Una coda che parte da un brano e ci somiglia.
///
/// # In che cosa è diversa da [`prossimo`]
///
/// [`prossimo`] continua una **sessione**: il primo gradino è il resto del
/// disco, perché chi ha messo su un album vuole sentire l'album. Una radio non
/// continua niente — è un gesto esplicito, «portami dove porta questo» — quindi
/// il disco non ha nessuna precedenza, e il bacino è la libreria intera.
///
/// # Il pavimento
///
/// Senza una scala — prima che l'analisi abbia coperto la libreria, o su una
/// libreria troppo piccola — non c'è niente su cui riordinare, e quel che resta
/// è il bacino nell'ordine in cui il motore delle regole l'ha dato: un
/// mescolamento. È una radio peggiore e non è una radio rotta, ed è la stessa
/// disciplina di [`prossimo`].
///
/// Il seme non è nel risultato: chi chiama lo mette davanti, perché è il brano
/// che sta già suonando.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn radio(
    connection: &Connection,
    seme: i64,
    quanti: usize,
    adesso_ms: i64,
) -> Result<Vec<i64>, AppError> {
    if quanti == 0 {
        return Ok(Vec::new());
    }
    let insieme = Insieme {
        combinazione: Combinazione::Tutte,
        regole: Vec::new(),
        limite: Some(CANDIDATI),
        ordinamento: Ordinamento::Casuale,
    };
    let trovati = smart::brani(connection, &insieme, adesso_ms)?;
    let ammessi: Vec<&TrackSummary> = trovati.iter().filter(|b| b.id != seme).collect();
    if ammessi.is_empty() {
        return Ok(Vec::new());
    }

    let mut ordine: Vec<usize> = (0..ammessi.len()).collect();
    if let Some(contesto) = Contesto::leggi(connection, seme)? {
        let voti = punteggi(connection, &ammessi, &contesto, adesso_ms)?;
        // Stabile di proposito: a pari punteggio — e senza punteggio, che è il
        // caso di chi non è ancora stato analizzato — resta l'ordine casuale che
        // il motore delle regole ha già dato. Cioè il pavimento riemerge dove
        // l'affinità non sa dire niente, brano per brano invece che in blocco.
        let quanto = |i: &usize| {
            voti.get(*i)
                .and_then(|voto| voto.as_ref())
                .map_or(f32::MIN, |voto| voto.totale)
        };
        ordine.sort_by(|a, b| quanto(b).total_cmp(&quanto(a)));
    }

    // Prima passata col tetto per artista, seconda senza. La seconda non è un
    // ripiego teorico: su una libreria di tre dischi il tetto da solo darebbe
    // nove brani, e una radio di nove brani su una libreria che ne ha ottanta
    // sarebbe un difetto che si vede.
    let mut per_artista: HashMap<&str, usize> = HashMap::new();
    let mut fuori: Vec<i64> = Vec::with_capacity(quanti);
    for indice in &ordine {
        if fuori.len() >= quanti {
            break;
        }
        let Some(brano) = ammessi.get(*indice) else {
            continue;
        };
        let quante = per_artista.entry(brano.artist.as_str()).or_insert(0);
        if *quante >= TETTO_ARTISTA {
            continue;
        }
        *quante += 1;
        fuori.push(brano.id);
    }
    for indice in &ordine {
        if fuori.len() >= quanti {
            break;
        }
        if let Some(brano) = ammessi.get(*indice)
            && !fuori.contains(&brano.id)
        {
            fuori.push(brano.id);
        }
    }
    Ok(fuori)
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
    contesto: Option<&Contesto>,
) -> Result<Option<Scelta>, AppError> {
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
    candidati(
        connection,
        &insieme,
        esclusi,
        adesso_ms,
        contesto,
        Motivo::Artista,
    )
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
    contesto: Option<&Contesto>,
) -> Result<Option<Scelta>, AppError> {
    // Nessuna riga **non** è un guasto, ed è l'unico punto della catena in cui
    // lo si sbagliava. Il brano corrente può sparire da `tracks` mentre suona:
    // basta una riscansione che trovi il file rimosso o rinominato. Con un
    // `map_err` secco quel caso diventava `db.queryFailed`, cioè la scelta
    // automatica del brano dopo si fermava con un errore invece di provare il
    // criterio successivo — e la musica finiva lì, a metà ascolto, senza che
    // niente fosse rotto davvero. È la stessa forma che usano già
    // `sonora::scala` e `settimana::della_settimana`.
    let genere: Option<String> = connection
        .query_row(
            "SELECT genre FROM tracks WHERE id = ?1",
            [corrente],
            |riga| riga.get(0),
        )
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(library::db_error("genere del brano corrente", &altro)),
        })?;
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
    candidati(
        connection,
        &insieme,
        esclusi,
        adesso_ms,
        contesto,
        Motivo::Genere,
    )
}

/// Un brano col cuore che non è mai stato suonato.
fn un_preferito_mai_sentito(
    connection: &Connection,
    esclusi: &[i64],
    adesso_ms: i64,
    contesto: Option<&Contesto>,
) -> Result<Option<Scelta>, AppError> {
    let insieme = Insieme {
        combinazione: Combinazione::Tutte,
        regole: vec![
            regola(Campo::Preferito, Operatore::Uguale, Valore::Numero(1)),
            regola(Campo::UltimoAscolto, Operatore::Vuoto, Valore::Nessuno),
        ],
        limite: Some(CANDIDATI),
        ordinamento: Ordinamento::Casuale,
    };
    candidati(
        connection,
        &insieme,
        esclusi,
        adesso_ms,
        contesto,
        Motivo::Preferito,
    )
}

/// Uno a caso, purché non sia di quelli appena sentiti.
///
/// L'ultimo gradino, e l'unico senza condizioni sul contenuto: a questo punto
/// la scelta è fra un brano qualsiasi e il silenzio.
fn uno_qualunque(
    connection: &Connection,
    esclusi: &[i64],
    adesso_ms: i64,
    contesto: Option<&Contesto>,
) -> Result<Option<Scelta>, AppError> {
    let insieme = Insieme {
        combinazione: Combinazione::Tutte,
        regole: Vec::new(),
        limite: Some(CANDIDATI),
        ordinamento: Ordinamento::Casuale,
    };
    candidati(
        connection,
        &insieme,
        esclusi,
        adesso_ms,
        contesto,
        Motivo::Caso,
    )
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
///
/// Senza [`Contesto`] prende il primo che non sia già in coda, che è quel che
/// questo modulo ha sempre fatto: è il pavimento, e ci si torna ogni volta che
/// l'affinità non ha niente da dire — prima della prima analisi, su una libreria
/// troppo piccola per una scala, su un brano corrente mai analizzato.
fn candidati(
    connection: &Connection,
    insieme: &Insieme,
    esclusi: &[i64],
    adesso_ms: i64,
    contesto: Option<&Contesto>,
    ripiego: Motivo,
) -> Result<Option<Scelta>, AppError> {
    let trovati = smart::brani(connection, insieme, adesso_ms)?;
    match contesto {
        None => Ok(
            scegli(trovati.iter().map(|t| t.id), esclusi).map(|id| Scelta {
                id,
                motivo: ripiego,
            }),
        ),
        Some(contesto) => il_migliore(connection, &trovati, esclusi, contesto, adesso_ms, ripiego),
    }
}

/// Il primo che non sia già stato messo in coda.
fn scegli(fra: impl Iterator<Item = i64>, esclusi: &[i64]) -> Option<i64> {
    fra.into_iter().find(|id| !esclusi.contains(id))
}

/// Quel che serve per dire quanto un candidato c'entra col brano che suona.
///
/// Si legge una volta sola in cima a [`prossimo`] e vale per tutti e quattro i
/// passi che riordinano: leggerlo dentro ogni passo vorrebbe dire rileggere la
/// scala e l'impronta di partenza fino a quattro volte per scegliere un brano.
///
/// # Niente rete, mai
///
/// I vicini si leggono dalla tabella che una passata di sottofondo ha già
/// riempito, non si chiedono a nessuno. Questa funzione gira sul filo che
/// prepara il brano successivo **mentre quello corrente suona**, e una richiesta
/// HTTP lì dentro sarebbe un buco udibile ogni volta che la rete fa i capricci —
/// cioè il difetto che la 2.1.0 ha passato un mese a chiudere.
#[derive(Debug, Clone)]
struct Contesto {
    /// L'impronta del brano corrente, già normalizzata. `None` se non è ancora
    /// stato analizzato: allora lo strato sonoro tace e gli altri due reggono.
    partenza: Option<Vec<f32>>,
    /// La scala su cui si misura la distanza.
    scala: Scala,
    /// Quanto ogni brano della libreria è vicino al corrente, secondo gli
    /// ascolti aggregati altrui. Vuota finché la passata non ha chiesto niente.
    vicini: HashMap<i64, f32>,
    /// Quanto conta ogni strato.
    pesi: Pesi,
}

impl Contesto {
    /// Legge il contesto, se c'è di che costruirlo.
    ///
    /// `None` quando manca la scala — cioè prima che la passata di analisi abbia
    /// finito, o su una libreria sotto
    /// [`aether_domain::affinita::MINIMO_BRANI`]. È la condizione che fa
    /// ricadere tutto sul comportamento di prima, riga per riga.
    fn leggi(connection: &Connection, corrente: i64) -> Result<Option<Self>, AppError> {
        let Some(scala) = sonora::scala(connection)? else {
            return Ok(None);
        };
        let partenza = sonora::impronte(connection, &[corrente])?
            .remove(&corrente)
            .and_then(|grezza| scala.normalizza(&grezza));
        Ok(Some(Self {
            partenza,
            scala,
            vicini: vicini_di(connection, corrente)?,
            pesi: Pesi::default(),
        }))
    }
}

/// I vicini del brano corrente secondo ListenBrainz, dalla tabella.
fn vicini_di(connection: &Connection, corrente: i64) -> Result<HashMap<i64, f32>, AppError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT vicino_id, MAX(punteggio) FROM brano_vicino
             WHERE track_id = ?1 GROUP BY vicino_id",
        )
        .map_err(|err| library::db_error("vicini del brano corrente", &err))?;
    let righe = statement
        .query_map([corrente], |riga| {
            Ok((riga.get::<_, i64>(0)?, riga.get::<_, f64>(1)?))
        })
        .map_err(|err| library::db_error("vicini del brano corrente", &err))?;

    let mut fuori = HashMap::new();
    for riga in righe {
        let (id, punteggio) =
            riga.map_err(|err| library::db_error("vicini del brano corrente", &err))?;
        // Il database tiene i decimali in doppia precisione perché SQLite non ha
        // i float a 32 bit; qui torna singola, che per un punteggio fra zero e
        // uno è largamente abbastanza.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "un punteggio in 0..1: la precisione di un f32 è un milionesimo"
        )]
        fuori.insert(id, punteggio as f32);
    }
    Ok(fuori)
}

/// Fra i candidati ammessi, quello che c'entra di più.
///
/// A parità di punteggio vince il primo che il motore delle regole ha
/// restituito, cioè l'ordinamento che quel passo aveva già scelto: su una
/// libreria non ancora analizzata i punteggi sono tutti uguali e il
/// comportamento torna a essere quello di prima, senza un ramo apposta.
fn il_migliore(
    connection: &Connection,
    trovati: &[TrackSummary],
    esclusi: &[i64],
    contesto: &Contesto,
    adesso_ms: i64,
    ripiego: Motivo,
) -> Result<Option<Scelta>, AppError> {
    let ammessi: Vec<&TrackSummary> = trovati
        .iter()
        .filter(|brano| !esclusi.contains(&brano.id))
        .collect();
    let Some(primo) = ammessi.first() else {
        return Ok(None);
    };

    let voti = punteggi(connection, &ammessi, contesto, adesso_ms)?;
    let mut migliore: Option<(&Voto, i64)> = None;
    for (brano, voto) in ammessi.iter().zip(voti.iter()) {
        let Some(voto) = voto.as_ref() else {
            continue;
        };
        // Strettamente maggiore: a parità vince il primo, cioè l'ordinamento che
        // quel passo aveva già scelto.
        if migliore.is_none_or(|(quanto, _)| voto.totale > quanto.totale) {
            migliore = Some((voto, brano.id));
        }
    }

    // Nessuno ha un punteggio: nessuno dei tre strati sapeva niente di nessuno
    // di loro. Vince il primo, che è la risposta di prima — e il motivo è quello
    // del gradino, che è l'unica cosa che si sa davvero.
    Ok(Some(migliore.map_or(
        Scelta {
            id: primo.id,
            motivo: ripiego,
        },
        |(voto, id)| Scelta {
            id,
            motivo: voto.motivo(contesto, adesso_ms, ripiego),
        },
    )))
}

/// Il punteggio di ogni candidato, nello stesso ordine in cui è arrivato.
///
/// `None` in una posizione vuol dire che di quel brano non si sapeva niente su
/// nessuno dei tre strati — che è diverso da «punteggio basso», e chi chiama lo
/// tratta diversamente: [`il_migliore`] lo salta, [`radio`] lo lascia dov'era.
///
/// Le due letture pesanti — impronte e cronologie — si fanno **una volta per
/// tutto il bacino** e non per brano: duecento interrogazioni separate su un filo
/// che sta preparando il brano successivo sono duecento occasioni di arrivare
/// tardi.
fn punteggi(
    connection: &Connection,
    ammessi: &[&TrackSummary],
    contesto: &Contesto,
    adesso_ms: i64,
) -> Result<Vec<Option<Voto>>, AppError> {
    let ids: Vec<i64> = ammessi.iter().map(|brano| brano.id).collect();
    let impronte = sonora::impronte(connection, &ids)?;
    let cronologie = ascolti_di(connection, &ids)?;

    let mut normalizzata: Vec<f32> = Vec::new();
    let mut fuori = Vec::with_capacity(ammessi.len());

    for brano in ammessi {
        let sonora = contesto.partenza.as_ref().and_then(|partenza| {
            let grezza = impronte.get(&brano.id)?;
            contesto
                .scala
                .normalizza_in(grezza, &mut normalizzata)
                .then(|| {
                    affinita::distanza2(partenza, &normalizzata, &RAGGRUPPAMENTO)
                        .map(affinita::somiglianza)
                })
                .flatten()
        });

        let ascolti = cronologie.get(&brano.id);
        let gusto = ascolti.and_then(|righe| affinita::gusto(righe, brano.duration_ms, adesso_ms));
        let ultimo = ascolti.and_then(|righe| righe.iter().map(|a| a.quando_ms).max());
        let vicinanza = contesto.vicini.get(&brano.id).copied();

        fuori.push(
            affinita::punteggio(
                sonora,
                vicinanza,
                gusto,
                affinita::ripetizione(ultimo, adesso_ms),
                &contesto.pesi,
            )
            .map(|totale| Voto {
                totale,
                sonora,
                vicinanza,
                gusto,
                ultimo,
            }),
        );
    }
    Ok(fuori)
}

/// Il punteggio di un candidato, coi tre strati ancora separati.
///
/// Il totale serve a scegliere; gli strati servono a **dire perché**. Tenerli
/// insieme costa quattro `f32` per candidato e toglie l'unica alternativa, che
/// sarebbe ricalcolarli per il vincitore dopo averli buttati per tutti.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Voto {
    /// Quanto vale in tutto, dopo la ridistribuzione dei pesi.
    totale: f32,
    /// Quanto somiglia nel suono, se lo si sa.
    sonora: Option<f32>,
    /// Quanto è vicino secondo gli ascolti altrui, se lo si sa.
    vicinanza: Option<f32>,
    /// Quanto è piaciuto qui, se lo si sa.
    gusto: Option<f32>,
    /// Quando lo si è sentito l'ultima volta.
    ultimo: Option<i64>,
}

impl Voto {
    /// Perché ha vinto questo, in una parola.
    ///
    /// # Come si sceglie fra tre strati che hanno tutti detto qualcosa
    ///
    /// Non col più alto in assoluto — i tre non sono sulla stessa scala: la
    /// somiglianza sonora vale 0,5 fra due brani estranei, la vicinanza vale
    /// zero per la stragrande maggioranza dei brani, e il gusto vale zero per
    /// tutto quel che non si è mai sentito. Confrontarli fra loro direbbe
    /// «suono» quasi sempre, e sarebbe una risposta senza contenuto.
    ///
    /// Ognuno ha quindi la **sua** soglia, e sopra quella soglia vince chi ha
    /// contribuito di più al totale — cioè il valore per il suo peso, che è
    /// esattamente il termine che ha spostato la classifica.
    ///
    /// Se nessuno arriva alla propria soglia resta il tempo: un brano che non si
    /// sente da mesi è una spiegazione vera e verificabile, e non richiede che
    /// nessuno dei tre strati abbia niente da dire. Se non c'è nemmeno quello,
    /// il motivo è il gradino della cascata — che è sempre onesto, perché è come
    /// il bacino è stato scelto.
    fn motivo(&self, contesto: &Contesto, adesso_ms: i64, ripiego: Motivo) -> Motivo {
        let mut vincitore: Option<(f32, Motivo)> = None;
        for (valore, soglia, peso, motivo) in [
            (
                self.sonora,
                SOGLIA_SUONO,
                contesto.pesi.sonora,
                Motivo::Suono,
            ),
            (
                self.vicinanza,
                SOGLIA_ASCOLTI,
                contesto.pesi.vicinanza,
                Motivo::Ascolti,
            ),
            (self.gusto, SOGLIA_GUSTO, contesto.pesi.gusto, Motivo::Gusto),
        ] {
            let Some(valore) = valore.filter(|v| *v >= soglia) else {
                continue;
            };
            let contributo = valore * peso;
            if vincitore.is_none_or(|(quanto, _)| contributo > quanto) {
                vincitore = Some((contributo, motivo));
            }
        }
        if let Some((_, motivo)) = vincitore {
            return motivo;
        }
        match self.ultimo {
            Some(quando) if adesso_ms.saturating_sub(quando) >= DA_TANTO_MS => Motivo::DaTanto,
            _ => ripiego,
        }
    }
}

/// La cronologia d'ascolto di un pugno di brani.
///
/// Una domanda sola per tutto il bacino, e non una per brano: duecento
/// interrogazioni separate su un filo che sta preparando il brano successivo
/// sono duecento occasioni di arrivare tardi.
///
/// # Cosa **non** c'è qui dentro
///
/// Gli abbandoni. `play_history` riceve una riga solo quando
/// [`aether_domain::listen::counts_as_play`] ha detto di sì, quindi chi salta un
/// brano dopo dieci secondi non lascia traccia. Non è una dimenticanza — è la
/// proprietà per cui `play_count` si può contare — ma va saputa prima di andare
/// a cercare lì una penalità per i salti: non c'è, e quel che si usa al suo
/// posto è quanto di un brano si è sentito quando lo si è sentito.
fn ascolti_di(
    connection: &Connection,
    ids: &[i64],
) -> Result<HashMap<i64, Vec<Ascolto>>, AppError> {
    let mut fuori: HashMap<i64, Vec<Ascolto>> = HashMap::new();
    if ids.is_empty() {
        return Ok(fuori);
    }
    let segnaposti = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT track_id, played_at, ms_played FROM play_history
         WHERE track_id IN ({segnaposti})"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| library::db_error("cronologia dei candidati", &err))?;
    let righe = statement
        .query_map(rusqlite::params_from_iter(ids.iter()), |riga| {
            Ok((
                riga.get::<_, i64>(0)?,
                Ascolto {
                    quando_ms: riga.get(1)?,
                    ms_ascoltati: riga.get(2)?,
                },
            ))
        })
        .map_err(|err| library::db_error("cronologia dei candidati", &err))?;
    for riga in righe {
        let (id, ascolto) =
            riga.map_err(|err| library::db_error("cronologia dei candidati", &err))?;
        fuori.entry(id).or_default().push(ascolto);
    }
    Ok(fuori)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un giorno in millisecondi, per le prove che parlano di mesi.
    const GIORNO: i64 = 24 * 60 * 60 * 1000;

    /// Chi verrebbe dopo, senza il perché.
    ///
    /// Quasi tutte le prove di questo modulo chiedono **quale** brano, non
    /// perché: passare da qui invece che da `prossimo` tiene le asserzioni sul
    /// fatto che interessa. Il motivo ha le sue prove, qui sotto.
    fn chi(connection: &Connection, corrente: i64, esclusi: &[i64], adesso_ms: i64) -> Option<i64> {
        prossimo(connection, corrente, esclusi, adesso_ms)
            .expect("scelta")
            .map(|scelta| scelta.id)
    }

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
        let scelto = chi(&connection, 1, &[1], 0);
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
        let scelto = chi(&connection, 3, &[1, 2, 3], 0);
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
        let scelto = chi(&connection, 1, &[1, 2], 0);
        assert_eq!(scelto, Some(3), "ha riproposto un brano già accodato");
    }

    /// Esaurito l'artista, si cambia: qui resta solo l'altro genere.
    #[test]
    fn esaurito_l_artista_si_scende_di_gradino() {
        let connection = libreria();
        let scelto = chi(&connection, 4, &[1, 2, 3, 4], 0);
        assert_eq!(scelto, Some(5), "non ha trovato l'unico brano rimasto");
    }

    /// Con tutto già in coda non c'è niente da proporre, e non è un errore.
    #[test]
    fn quando_non_resta_niente_lo_dice_invece_di_fallire() {
        let connection = libreria();
        let scelto = chi(&connection, 1, &[1, 2, 3, 4, 5], 0);
        assert_eq!(scelto, None, "ha proposto qualcosa che era già in coda");
    }

    /// Una libreria vuota non fa cadere niente.
    #[test]
    fn una_libreria_vuota_non_e_un_guasto() {
        let connection = crate::db::open_in_memory().expect("database").connection;
        let scelto = chi(&connection, 1, &[], 0);
        assert_eq!(scelto, None);
    }

    // ── il riordino per affinità ────────────────────────────────────────────

    /// Dà a `quali` un'impronta costruita attorno a `seme`, e taratura la scala.
    ///
    /// I brani con semi vicini si somigliano; quelli lontani no. Serve una
    /// libreria di almeno `MINIMO_BRANI` impronte perché una scala esista, e i
    /// riempitivi la fanno esistere senza entrare fra i candidati.
    fn con_affinita(connection: &mut Connection, quali: &[(i64, f32)]) {
        // I riempitivi: brani veri, con impronte sparse, che non appartengono
        // né all'artista né al genere dei cinque della libreria di prova.
        for i in 0..(sonora::MINIMO_BRANI as i64) {
            let id = 1000 + i;
            connection
                .execute(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                         album_key, genre, duration_ms, file_size,
                                         date_added, date_modified)
                     VALUES (?1, ?2, ?3, 'R', 'Riempitivo', 'Riempi', 'r|r', 'Ambient',
                             1000, 1, 1, 1)",
                    rusqlite::params![id, format!("r{id}.mp3"), format!("kr{id}")],
                )
                .expect("riempitivo");
        }

        let impronta_di = |seme: f32| -> Vec<f32> {
            (0..aether_play::impronta::DIMENSIONI)
                .map(|d| seme + d as f32 * 0.01)
                .collect()
        };

        let mut misurati: Vec<sonora::Misurato> = (0..(sonora::MINIMO_BRANI as i64))
            .map(|i| sonora::Misurato::Fatta {
                track_id: 1000 + i,
                vettore: impronta_di(i as f32),
            })
            .collect();
        for (id, seme) in quali {
            misurati.push(sonora::Misurato::Fatta {
                track_id: *id,
                vettore: impronta_di(*seme),
            });
        }

        let tx = connection.transaction().expect("transazione");
        sonora::registra(&tx, &misurati, 1_000).expect("registrazione");
        sonora::ritara(&tx, 1_000).expect("taratura");
        tx.commit().expect("commit");
    }

    /// Con una scala e delle impronte, fra tre candidati vince il più simile.
    ///
    /// È la prova che il riordino esiste davvero: senza, vincerebbe sempre lo
    /// stesso che il motore delle regole restituisce per primo.
    ///
    /// Il confronto avviene **dentro** il gradino che la cascata raggiunge per
    /// davvero. Il corrente è il 4, il suo album non ha altre tracce, e il
    /// gradino che risponde è quello dell'artista: il bacino è `{1, 2, 3}` e il
    /// 5 non ci entra, perché è di un altro artista e la cascata al genere non
    /// ci arriva nemmeno. Mettere il brano vicino fuori dal bacino non
    /// proverebbe che il riordino funziona: proverebbe che non è lui a
    /// scegliere il bacino, che è vero e non è la domanda.
    #[test]
    fn fra_due_candidati_vince_quello_che_somiglia_di_piu() {
        let mut connection = libreria();
        // Il 3 ha l'impronta identica al corrente, l'1 e il 2 lontanissime.
        con_affinita(
            &mut connection,
            &[(4, 0.0), (1, 900.0), (2, 900.0), (3, 0.0), (5, 900.0)],
        );
        let scelto = chi(&connection, 4, &[4], 0);
        assert_eq!(
            scelto,
            Some(3),
            "non ha scelto il brano sonoramente più vicino"
        );
    }

    /// E se il più simile è un altro, sceglie quell'altro.
    ///
    /// Lo stesso bacino con i semi spostati: se la prova sopra passasse per un
    /// accidente dell'ordinamento — cioè se a rispondere fosse sempre la stessa
    /// riga a prescindere dalle impronte — una delle due fallirebbe per forza.
    #[test]
    fn e_se_il_piu_simile_e_un_altro_sceglie_quello() {
        let mut connection = libreria();
        con_affinita(
            &mut connection,
            &[(4, 0.0), (1, 0.0), (2, 900.0), (3, 900.0), (5, 900.0)],
        );
        let scelto = chi(&connection, 4, &[4], 0);
        assert_eq!(scelto, Some(1), "non ha seguito le impronte");
    }

    /// Senza una scala non cambia niente rispetto a prima.
    ///
    /// La regressione che conta: la cascata è il pavimento, e su una libreria
    /// non ancora analizzata deve rispondere esattamente come rispondeva.
    #[test]
    fn senza_impronte_la_cascata_risponde_come_prima() {
        let connection = libreria();
        assert!(
            sonora::scala(&connection).expect("scala").is_none(),
            "non doveva esserci una scala"
        );
        assert_eq!(chi(&connection, 1, &[1], 0), Some(2));
        assert_eq!(chi(&connection, 3, &[1, 2, 3], 0), Some(4));
    }

    /// Un brano corrente senza impronta non impedisce di scegliere.
    ///
    /// Capita per tutta la durata della prima passata: la scala c'è già — l'ha
    /// fatta il resto della libreria — ma il brano che suona non è ancora stato
    /// misurato. Lo strato sonoro tace e gli altri due reggono.
    #[test]
    fn un_corrente_non_analizzato_non_ferma_la_scelta() {
        let mut connection = libreria();
        // Tutti tranne il 4, che è il corrente.
        con_affinita(&mut connection, &[(1, 1.0), (2, 2.0), (3, 3.0), (5, 4.0)]);
        let scelto = chi(&connection, 4, &[4], 0);
        assert!(scelto.is_some(), "si è fermato senza impronta di partenza");
    }

    /// Quel che si è appena sentito perde, a parità di tutto il resto.
    #[test]
    fn a_parita_di_suono_perde_quel_che_si_e_appena_sentito() {
        let mut connection = libreria();
        con_affinita(
            &mut connection,
            &[(4, 0.0), (1, 0.0), (2, 0.0), (3, 900.0), (5, 900.0)],
        );
        // Il brano 1 è stato sentito per intero pochi istanti fa; il 2 mai.
        let adesso = 10_000_000_i64;
        connection
            .execute(
                "INSERT INTO play_history (track_id, played_at, ms_played)
                 VALUES (1, ?1, 1000)",
                [adesso - 60_000],
            )
            .expect("cronologia");
        let scelto = chi(&connection, 4, &[4], adesso);
        assert_eq!(
            scelto,
            Some(2),
            "ha riproposto quel che si è appena sentito"
        );
    }

    /// Una libreria di `artisti` nomi, `per_artista` brani ciascuno.
    ///
    /// Serve alle prove del tetto per artista, che sulla libreria di prova non
    /// si vedrebbe: là ci sono quattro brani di un artista e uno di un altro.
    fn libreria_larga(artisti: i64, per_artista: i64) -> Connection {
        let connection = crate::db::open_in_memory().expect("database").connection;
        for a in 0..artisti {
            for n in 0..per_artista {
                let id = a * 100 + n + 1;
                connection
                    .execute(
                        "INSERT INTO tracks (id, path, track_key, title, artist, album,
                                             album_key, genre, duration_ms, file_size,
                                             date_added, date_modified)
                         VALUES (?1, ?2, ?3, 'T', ?4, 'D', ?5, 'Rock', 1000, 1, 1, 1)",
                        rusqlite::params![
                            id,
                            format!("f{id}.mp3"),
                            format!("kf{id}"),
                            format!("Art{a}"),
                            format!("art{a}|d"),
                        ],
                    )
                    .expect("brano");
            }
        }
        connection
    }

    /// Il seme non finisce nella propria radio: sta già suonando.
    #[test]
    fn la_radio_non_contiene_il_seme() {
        let connection = libreria();
        let coda = radio(&connection, 1, 4, 0).expect("radio");
        assert!(
            !coda.contains(&1),
            "il seme è finito nella sua stessa radio"
        );
    }

    /// La radio mette davanti quel che somiglia al seme.
    ///
    /// È la differenza fra una radio e un mescolamento: il 5 è di un altro
    /// artista e di un altro genere — la cascata di `prossimo` non ci
    /// arriverebbe mai passando per l'artista — e ha l'impronta identica al
    /// seme. Una radio deve portarci.
    #[test]
    fn la_radio_mette_davanti_quel_che_somiglia() {
        let mut connection = libreria();
        con_affinita(
            &mut connection,
            &[(1, 500.0), (5, 500.0), (2, 900.0), (3, 900.0), (4, 900.0)],
        );
        let coda = radio(&connection, 1, 3, 0).expect("radio");
        assert_eq!(
            coda.first(),
            Some(&5),
            "non ha messo davanti il brano sonoramente più vicino"
        );
    }

    /// Il brano corrente sparito non ferma la musica.
    ///
    /// Succede davvero: si ascolta, una riscansione trova il file rinominato o
    /// tolto, e la riga esce da `tracks` mentre l'audio sta ancora suonando.
    /// `stesso_genere` chiedeva il genere di quella riga e trattava «nessuna
    /// riga» come un guasto del database, così la scelta automatica si fermava
    /// con `db.queryFailed` invece di provare i criteri successivi — cioè la
    /// coda finiva lì, a metà ascolto, senza che niente fosse rotto.
    #[test]
    fn un_corrente_che_non_esiste_piu_non_e_un_guasto() {
        let connection = libreria();
        // Dritto su `stesso_genere`: è l'unico gradino che leggeva la riga del
        // brano corrente con una `query_row` e trattava la sua assenza come un
        // guasto. Passando da `prossimo` la prova non direbbe niente — i
        // gradini prima rispondono da soli e questo non lo si raggiunge.
        let scelta = stesso_genere(&connection, 9999, &[], 0, None);
        assert_eq!(
            scelta.expect("un brano corrente sparito non è un guasto del database"),
            None,
            "niente riga, niente genere, nessun candidato: si prova il criterio dopo"
        );
    }

    /// Una radio non è un disco solo.
    ///
    /// Senza il tetto, i brani più vicini a un brano sono quasi sempre gli altri
    /// dello stesso album: vero, e inutile.
    #[test]
    fn una_radio_non_e_un_disco_solo() {
        let connection = libreria_larga(4, 5);
        let coda = radio(&connection, 1, 8, 0).expect("radio");
        assert_eq!(coda.len(), 8);
        let mut per_artista: HashMap<String, usize> = HashMap::new();
        for id in &coda {
            let artista: String = connection
                .query_row("SELECT artist FROM tracks WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .expect("artista");
            *per_artista.entry(artista).or_insert(0) += 1;
        }
        assert!(
            per_artista.values().all(|quanti| *quanti <= TETTO_ARTISTA),
            "un artista solo si è preso più di {TETTO_ARTISTA} brani: {per_artista:?}"
        );
    }

    /// Quando non c'è altro, il tetto cede invece di accorciare la radio.
    ///
    /// Su una libreria di un artista solo, un tetto rispettato alla lettera
    /// darebbe tre brani. Meglio una radio monotona che una radio troncata.
    #[test]
    fn il_tetto_cede_quando_non_c_e_altro() {
        let connection = libreria_larga(1, 9);
        let coda = radio(&connection, 1, 6, 0).expect("radio");
        assert_eq!(coda.len(), 6, "il tetto ha accorciato la radio");
    }

    /// Senza impronte la radio è un mescolamento, non un guasto.
    ///
    /// È il pavimento: prima che l'analisi abbia coperto la libreria non c'è
    /// niente su cui riordinare, e quel che resta è il bacino così com'è.
    #[test]
    fn senza_impronte_la_radio_e_un_mescolamento() {
        let connection = libreria();
        let coda = radio(&connection, 1, 4, 0).expect("radio");
        assert_eq!(coda.len(), 4);
        assert!(coda.iter().all(|id| *id != 1));
    }

    /// Una libreria di un brano solo non ha una radio, e non è un errore.
    #[test]
    fn una_libreria_di_un_brano_non_ha_radio() {
        let connection = libreria_larga(1, 1);
        assert!(radio(&connection, 1, 10, 0).expect("radio").is_empty());
    }

    // ── il perché ───────────────────────────────────────────────────────────

    /// Il perché di una scelta, per esteso.
    fn perche(connection: &Connection, corrente: i64, esclusi: &[i64], adesso_ms: i64) -> Motivo {
        prossimo(connection, corrente, esclusi, adesso_ms)
            .expect("scelta")
            .expect("una scelta c'è")
            .motivo
    }

    /// Continuare un disco si dice «album», e non è un'affinità.
    ///
    /// Il primo gradino non passa dal riordino, quindi non ha nessuno strato da
    /// cui ricavare un motivo: il motivo è il gradino stesso, ed è quello vero.
    #[test]
    fn continuare_il_disco_si_chiama_col_suo_nome() {
        let connection = libreria();
        assert_eq!(perche(&connection, 1, &[1], 0), Motivo::Album);
    }

    /// Senza scala il motivo è il gradino della cascata.
    ///
    /// È il pavimento anche qui: prima della prima analisi non c'è niente da
    /// spiegare oltre a come è stato scelto il bacino, e dire «suona come quello
    /// di prima» senza averlo misurato sarebbe inventare.
    #[test]
    fn senza_impronte_il_motivo_e_il_gradino() {
        let connection = libreria();
        assert_eq!(perche(&connection, 3, &[1, 2, 3], 0), Motivo::Artista);
    }

    /// Quando è il suono a decidere, lo dice.
    #[test]
    fn quando_decide_il_suono_lo_dice() {
        let mut connection = libreria();
        // Il 3 ha l'impronta identica al corrente: somiglianza uno, ben sopra la
        // soglia. Gli altri due sono lontanissimi.
        con_affinita(
            &mut connection,
            &[(4, 0.0), (1, 900.0), (2, 900.0), (3, 0.0), (5, 900.0)],
        );
        assert_eq!(perche(&connection, 4, &[4], 0), Motivo::Suono);
    }

    /// Quando decide chi ascolta cosa, lo dice.
    ///
    /// Il 2 non somiglia al corrente per niente — la sua impronta è dall'altra
    /// parte della libreria — e viene scelto solo perché ListenBrainz dice che
    /// chi ascolta il 4 ascolta anche lui.
    #[test]
    fn quando_decidono_gli_altri_ascoltatori_lo_dice() {
        let mut connection = libreria();
        con_affinita(
            &mut connection,
            &[(4, 0.0), (1, 900.0), (2, 900.0), (3, 900.0), (5, 900.0)],
        );
        connection
            .execute(
                "INSERT INTO brano_vicino (track_id, vicino_id, punteggio, fonte, raccolto_at)
                 VALUES (4, 2, 1.0, 'lb-registrazione', 1)",
                [],
            )
            .expect("vicino");
        let scelta = prossimo(&connection, 4, &[4], 0)
            .expect("scelta")
            .expect("una scelta c'è");
        assert_eq!(scelta.id, 2, "non ha scelto il vicino");
        assert_eq!(scelta.motivo, Motivo::Ascolti);
    }

    /// Un brano fermo da mesi si spiega col tempo, che è la spiegazione più
    /// verificabile che ci sia.
    ///
    /// Nessuno strato arriva alla propria soglia — le impronte sono tutte
    /// lontane, non ci sono vicini, e un ascolto solo di dieci secondi non fa
    /// gusto — ma l'ultima volta è stata otto mesi fa, e quello si può dire.
    #[test]
    fn un_brano_fermo_da_mesi_si_spiega_col_tempo() {
        let mut connection = libreria();
        con_affinita(
            &mut connection,
            &[(4, 0.0), (1, 900.0), (2, 900.0), (3, 900.0), (5, 900.0)],
        );
        let adesso = DA_TANTO_MS * 3;
        // Otto mesi: due mesi oltre la soglia, senza dividere niente.
        let otto_mesi_fa = adesso - DA_TANTO_MS - 60 * GIORNO;
        connection
            .execute(
                "INSERT INTO play_history (track_id, played_at, ms_played)
                 VALUES (1, ?1, 10000), (2, ?1, 10000), (3, ?1, 10000)",
                [otto_mesi_fa],
            )
            .expect("cronologia");
        assert_eq!(perche(&connection, 4, &[4], adesso), Motivo::DaTanto);
    }

    /// Ogni motivo ha un codice suo: due motivi con lo stesso codice sarebbero
    /// due frasi che l'interfaccia non sa distinguere.
    #[test]
    fn i_codici_dei_motivi_sono_tutti_diversi() {
        let tutti = [
            Motivo::Album,
            Motivo::Artista,
            Motivo::Genere,
            Motivo::Preferito,
            Motivo::Caso,
            Motivo::Suono,
            Motivo::Ascolti,
            Motivo::Gusto,
            Motivo::DaTanto,
        ];
        let codici: std::collections::HashSet<&str> =
            tutti.iter().map(|m| Motivo::codice(*m)).collect();
        assert_eq!(
            codici.len(),
            tutti.len(),
            "due motivi hanno lo stesso codice"
        );
    }
}
