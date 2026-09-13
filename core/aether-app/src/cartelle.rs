//! Il pannello «Cartelle»: l'albero della libreria, non quello del disco.
//!
//! # Perché dal database e non dal filesystem
//!
//! Perché un pannello di navigazione deve rispondere adesso, e perché la sola
//! cartella che serve è quella che ha dentro dei brani indicizzati. Camminare
//! sul disco per disegnarlo vorrebbe dire, su una condivisione di rete, i
//! quaranta secondi di timeout di Windows a ogni espansione — e su una share
//! spenta, un pannello che non si apre affatto. Da qui non si tocca **mai**
//! SMB: le righe di `tracks` sono già in casa.
//!
//! Il prezzo è dichiarato: una cartella che sul disco esiste ma di cui nessun
//! brano è indicizzato qui non compare. È il prezzo giusto — un nodo che non si
//! può riprodurre, dentro un pannello che serve a riprodurre, è un nodo che si
//! impara a saltare.
//!
//! # Perché un trie in memoria e non `LIKE 'radice%'`
//!
//! Perché quella `LIKE` è una **scansione a ogni clic di espansione**, e non
//! per una svista di scrittura: non c'è modo di farle usare l'indice.
//!
//! `tracks.path` è `TEXT NOT NULL UNIQUE` (`001_baseline.sql:18`), quindi
//! l'indice implicito `sqlite_autoindex_tracks_1` ha collazione `BINARY`.
//! SQLite converte `colonna LIKE 'prefisso%'` in una ricerca per intervallo a
//! due condizioni: che il prefisso non contenga metacaratteri, **e** che il
//! confronto sia sensibile alle maiuscole — cioè o `PRAGMA case_sensitive_like`
//! acceso, o la colonna dichiarata `COLLATE NOCASE`. Qui non vale né l'una né
//! l'altra: il pragma è quello di serie (spento, cioè `LIKE` insensibile sugli
//! ASCII) e la colonna è `BINARY`. `EXPLAIN QUERY PLAN` lo dice in chiaro:
//!
//! ```text
//! path LIKE 'C:\Musica%'  ->  SCAN   tracks USING COVERING INDEX …
//! path GLOB 'C:\Musica*'  ->  SEARCH tracks USING COVERING INDEX … (path>? AND path<?)
//! ```
//!
//! **E `GLOB` non è la scappatoia**, anche se il piano lì è una ricerca vera:
//! `GLOB` cerca per intervallo *proprio perché* confronta byte per byte, e i
//! byte del prefisso non si conoscono. Lo stesso posto sta nel database scritto
//! `C:\Musica` o `C:/Musica`, con qualunque combinazione di maiuscole — è
//! precisamente la ragione per cui `paths.rs::path_key` esiste — e un prefisso
//! che dovesse coprire tutte le grafie non è un prefisso. Farlo con `GLOB`
//! vorrebbe dire una `OR` per ogni variante, e la prima `OR` riporta al `SCAN`.
//!
//! Resta quindi la scansione: sull'indice e non sulla tabella, quindi meno
//! cara di quanto sembri, ma pur sempre lineare nel numero di brani — **a ogni
//! clic**. Il trie invece si costruisce con **una** lettura di tutta la tabella
//! (`SELECT id, path, disc_number, track_number`), e da lì ogni espansione è un
//! accesso a una mappa.
//!
//! # Perché nessuna tabella `folders` materializzata
//!
//! Perché sarebbe una seconda verità. Le righe di `tracks` che cambiano la
//! forma dell'albero — un brano che nasce, uno che sparisce, uno che si sposta
//! — si scrivono in tre punti soli, tutti in `library.rs` (`insert_track`,
//! `update_track` col ramo «Sposta», e la cancellazione), e sono tre punti che
//! oggi non sanno niente di cartelle. Una tabella materializzata pretenderebbe
//! che tutti e tre restassero in passo per sempre, e il giorno in cui uno non
//! lo fa il pannello mostra cartelle che non ci sono più — senza che niente
//! segnali l'errore, perché una tabella non si accorge di essere vecchia.
//!
//! Il trie non può desincronizzarsi: è **derivato** da `tracks` e si
//! ricostruisce da capo in una trentina di millisecondi — 35 ms misurati in
//! rilascio su 18.534 brani, lettura del database compresa. Quando la libreria
//! cambia non c'è niente da aggiornare, c'è solo da buttarlo via.
//!
//! # Il costo in memoria è una condizione, non un dettaglio
//!
//! Questo albero vive in RAM, e l'albero di processi di Aether ha un tetto che
//! non deve crescere. Da qui due scelte visibili nel codice:
//!
//! - i brani di una cartella si conservano come **soli identificativi**, già
//!   ordinati alla costruzione. L'ordine dipende da `(disc_number,
//!   track_number, path_key)`, che non cambia finché il trie vive: tenersi
//!   quelle tre cose per riordinare a ogni richiesta costerebbe una decina di
//!   volte tanto per non decidere mai niente di diverso;
//! - oltre [`SOGLIA_IDENTIFICATIVI`] brani gli identificativi non si conservano
//!   affatto e restano i soli conteggi. Chi deve riprodurre una cartella su una
//!   libreria così grande passa da [`brani_sotto_dal_database`], che rilegge.
//!
//! Chi costruisce l'albero decide anche **quando**: non all'avvio, ma alla
//! prima apertura della vista, e lo lascia cadere quando la vista è chiusa da
//! un pezzo. Quella parte sta nel guscio desktop, che è l'unico a sapere se una
//! finestra è aperta.

use std::collections::HashMap;

use aether_domain::errors::AppError;
use aether_domain::paths::{PathRules, base_name, is_under, path_key};
use rusqlite::Connection;

/// Oltre questi brani il trie rinuncia agli identificativi.
///
/// Il numero è una condizione di accettazione, non una stima: su una libreria
/// di duecentomila brani gli identificativi da soli sono un megabyte e mezzo, e
/// i nomi delle cartelle che li contengono parecchi di più. Sopra la soglia si
/// tiene la forma dell'albero — che è quel che il pannello disegna — e si
/// rinuncia a quel che serve solo nel momento in cui si preme «riproduci», dove
/// una rilettura del database si può pagare perché è un gesto singolo.
pub const SOGLIA_IDENTIFICATIVI: usize = 200_000;

/// Una cartella dell'albero, nella forma che il pannello disegna.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodoCartella {
    /// Il percorso nella grafia originale, quella con cui i brani stanno sul
    /// disco. È anche l'identificativo del nodo: si rimanda a [`Albero::figlie`]
    /// e a [`Albero::brani_sotto`] così com'è.
    pub percorso: String,
    /// Quel che si scrive nella riga.
    ///
    /// L'ultimo segmento del percorso, tranne che per i nodi di primo livello,
    /// dove è il percorso intero: una radice che si annunciasse come «musica»
    /// non direbbe di quale disco sta parlando.
    pub nome: String,
    /// Quanti brani ci sono qui sotto, **contando le sottocartelle**.
    pub brani: u32,
    /// Quante sottocartelle dirette.
    pub sottocartelle: u32,
    /// È una delle cartelle sorvegliate.
    ///
    /// `false` sui nodi sintetici, cioè quelli nati per dare un posto a un
    /// brano che nel database c'è ma sotto nessuna radice sorvegliata non sta.
    /// La differenza conta: solo su una radice vera ha senso chiedersi se
    /// risponde ancora.
    pub radice: bool,
}

/// L'albero delle cartelle della libreria.
///
/// Si costruisce con [`Albero::costruisci`] e da lì è di sola lettura: non c'è
/// modo di aggiornarlo, e non è una mancanza — quando la libreria cambia lo si
/// butta e se ne fa un altro. Vedi il `//!` del modulo.
#[derive(Debug)]
pub struct Albero {
    /// Tutti i nodi in un solo vettore: i legami sono indici, non puntatori.
    nodi: Vec<Nodo>,
    /// Da [`chiave`] all'indice del nodo. È l'unica copia delle chiavi che
    /// sopravvive alla costruzione.
    per_chiave: HashMap<String, usize>,
    /// I nodi di primo livello, nell'ordine in cui vanno mostrati.
    cime: Vec<usize>,
    /// Gli identificativi dei brani sono stati conservati.
    con_identificativi: bool,
    /// Le regole con cui sono state calcolate le chiavi: servono a rispondere
    /// alle domande successive con lo stesso metro con cui si è costruito.
    regole: PathRules,
}

/// Un nodo dell'albero. Privato: quel che esce è [`NodoCartella`].
#[derive(Debug)]
struct Nodo {
    percorso: String,
    nome: String,
    /// Gli indici dei figli, già ordinati per chiave.
    figli: Vec<usize>,
    /// Gli identificativi dei brani **direttamente** in questa cartella, già
    /// nell'ordine dell'album. Vuoto quando gli identificativi non si
    /// conservano.
    brani: Vec<i64>,
    /// Quanti brani ci sono qui sotto, sottocartelle comprese.
    sotto: u32,
    radice: bool,
}

/// Una riga di `tracks`, per quel poco che all'albero serve.
#[derive(Debug)]
struct Riga {
    id: i64,
    percorso: String,
    disco: Option<i64>,
    traccia: Option<i64>,
}

/// Un brano in attesa di essere ordinato dentro la sua cartella.
///
/// Vive solo durante la costruzione: alla fine di ogni cartella resta il solo
/// `id`, e la chiave di ordinamento si butta.
#[derive(Debug)]
struct DaOrdinare {
    disco: i64,
    traccia: i64,
    nome: String,
    id: i64,
}

/// Una cartella sorvegliata, pronta per la ricerca del prefisso più lungo.
#[derive(Debug)]
struct Sorvegliata {
    percorso: String,
    profondita: usize,
    indice: usize,
}

/// I pezzi dell'albero mentre lo si costruisce.
///
/// Una struttura e non otto parametri sciolti: [`trova_o_crea`] li vuole tutti,
/// e otto argomenti in fila sono otto occasioni di scambiarne due.
#[derive(Debug)]
struct InCostruzione {
    nodi: Vec<Nodo>,
    /// Il padre di ogni nodo, `None` per quelli di primo livello. Vive solo
    /// qui: serve a far salire i conteggi una volta sola, e tenerlo dentro
    /// [`Nodo`] vorrebbe dire sedici byte per nodo mai più letti.
    padri: Vec<Option<usize>>,
    /// I brani di ogni nodo, ancora da ordinare.
    brani: Vec<Vec<DaOrdinare>>,
    per_chiave: HashMap<String, usize>,
    cime: Vec<usize>,
}

impl Albero {
    /// Costruisce l'albero leggendo `tracks` per intero.
    ///
    /// `radici` sono le cartelle sorvegliate, nella grafia con cui l'utente le
    /// ha date: diventano i nodi di primo livello **anche se vuote**, perché
    /// una cartella appena aggiunta e non ancora scansionata che non comparisse
    /// nel pannello sembrerebbe non essere stata aggiunta.
    ///
    /// Un brano che non sta sotto nessuna di esse non si perde: gli si dà un
    /// nodo sintetico di primo livello, che è l'unità del suo percorso — la
    /// lettera di unità, o la condivisione di rete per intero.
    ///
    /// # Errori
    ///
    /// Quel che risponde SQLite se la lettura fallisce, cioè `db.queryFailed` e
    /// i suoi fratelli.
    pub fn costruisci(
        connection: &Connection,
        radici: &[String],
        rules: PathRules,
    ) -> Result<Self, AppError> {
        let righe = leggi_righe(connection)?;
        let con_identificativi = righe.len() <= SOGLIA_IDENTIFICATIVI;
        Ok(Self::da_righe(righe, radici, rules, con_identificativi))
    }

    /// Le cartelle dentro `percorso`; `None` chiede quelle di primo livello.
    ///
    /// Un percorso che l'albero non conosce dà un elenco vuoto, non un errore:
    /// dall'ultima costruzione la libreria può essere cambiata, e un pannello
    /// che si svuota è meno peggio di un pannello che si rompe.
    #[must_use]
    pub fn figlie(&self, percorso: Option<&str>) -> Vec<NodoCartella> {
        let vuoto: &[usize] = &[];
        let indici = match percorso {
            None => self.cime.as_slice(),
            Some(dove) => self
                .per_chiave
                .get(&chiave(dove, self.regole))
                .and_then(|indice| self.nodi.get(*indice))
                .map_or(vuoto, |nodo| nodo.figli.as_slice()),
        };
        indici
            .iter()
            .filter_map(|indice| self.nodi.get(*indice))
            .map(|nodo| NodoCartella {
                percorso: nodo.percorso.clone(),
                nome: nodo.nome.clone(),
                brani: nodo.sotto,
                sottocartelle: u32::try_from(nodo.figli.len()).unwrap_or(u32::MAX),
                radice: nodo.radice,
            })
            .collect()
    }

    /// Gli identificativi dei brani sotto `percorso`, nell'ordine in cui si
    /// riproducono.
    ///
    /// L'ordine è quello che ci si aspetta da una cartella d'album, perché una
    /// cartella *è* un album nel caso normale: prima i brani che stanno
    /// direttamente qui, ordinati per `(disc_number, track_number, path_key)`,
    /// poi le sottocartelle in ordine di chiave, ciascuna con la stessa regola.
    /// Un brano senza numero di traccia va in coda ai numerati, non in testa:
    /// dentro un album vero i non numerati sono i bonus e le tracce nascoste.
    ///
    /// `None` sopra [`SOGLIA_IDENTIFICATIVI`], dove gli identificativi non si
    /// conservano: la via d'uscita è [`brani_sotto_dal_database`]. Una cartella
    /// che davvero non ha brani risponde `Some` di un elenco vuoto — sono due
    /// fatti diversi, e il tipo li tiene diversi.
    #[must_use]
    pub fn brani_sotto(&self, percorso: &str) -> Option<Vec<i64>> {
        if !self.con_identificativi {
            return None;
        }
        let Some(radice) = self.per_chiave.get(&chiave(percorso, self.regole)) else {
            // Una cartella che l'albero non conosce: gli identificativi ci
            // sono, e sotto quel percorso non c'è niente. È un `Some` vuoto, non
            // un «non lo so».
            return Some(Vec::new());
        };
        let mut esito = Vec::with_capacity(self.quanti_sotto(percorso) as usize);
        let mut pila = vec![*radice];
        // Una pila e non la ricorsione: la profondità di un albero di cartelle
        // la decide chi ha nominato le cartelle, e non c'è motivo di farla
        // decidere allo stack di questo processo.
        while let Some(indice) = pila.pop() {
            let Some(nodo) = self.nodi.get(indice) else {
                continue;
            };
            esito.extend_from_slice(&nodo.brani);
            // Al contrario, perché la pila restituisce l'ultimo che ha preso: è
            // così che il primo figlio esce prima del secondo.
            pila.extend(nodo.figli.iter().rev().copied());
        }
        Some(esito)
    }

    /// Quanti brani ci sono sotto `percorso`, sottocartelle comprese.
    ///
    /// Zero anche per un percorso che l'albero non conosce, e le due cose non
    /// si distinguono: per il pannello sono la stessa riga vuota.
    #[must_use]
    pub fn quanti_sotto(&self, percorso: &str) -> u32 {
        self.per_chiave
            .get(&chiave(percorso, self.regole))
            .and_then(|indice| self.nodi.get(*indice))
            .map_or(0, |nodo| nodo.sotto)
    }

    /// Gli identificativi dei brani ci sono.
    ///
    /// `false` sopra [`SOGLIA_IDENTIFICATIVI`]: allora [`Albero::brani_sotto`]
    /// risponde `None`, e chi deve riprodurre passa da
    /// [`brani_sotto_dal_database`]. Serve a chi vuole saperlo **prima** di
    /// chiedere — l'esito di `brani_sotto` lo dice già da sé.
    #[must_use]
    pub const fn con_identificativi(&self) -> bool {
        self.con_identificativi
    }

    /// Quanti nodi ha l'albero. Serve a chi misura quanto costa tenerlo.
    #[must_use]
    pub fn quanti_nodi(&self) -> usize {
        self.nodi.len()
    }

    /// Il cuore di [`Albero::costruisci`], senza il database.
    ///
    /// Separata perché è tutto quel che c'è da provare, e perché
    /// [`brani_sotto_dal_database`] la richiama su un sottoinsieme di righe.
    fn da_righe(
        righe: Vec<Riga>,
        radici: &[String],
        regole: PathRules,
        con_identificativi: bool,
    ) -> Self {
        let mut lavoro = InCostruzione {
            nodi: Vec::new(),
            padri: Vec::new(),
            brani: Vec::new(),
            per_chiave: HashMap::new(),
            cime: Vec::new(),
        };
        let mut sorvegliate: Vec<Sorvegliata> = Vec::new();

        for radice in radici {
            let ch = chiave(radice, regole);
            // Una radice vuota non contiene niente — è la stessa guardia di
            // `is_under` — e una ripetuta è già un nodo.
            if ch.is_empty() || lavoro.per_chiave.contains_key(&ch) {
                continue;
            }
            let indice = lavoro.nodi.len();
            lavoro.nodi.push(Nodo {
                percorso: radice.clone(),
                nome: radice.clone(),
                figli: Vec::new(),
                brani: Vec::new(),
                sotto: 0,
                radice: true,
            });
            lavoro.padri.push(None);
            lavoro.brani.push(Vec::new());
            lavoro.per_chiave.insert(ch, indice);
            lavoro.cime.push(indice);
            sorvegliate.push(Sorvegliata {
                percorso: radice.clone(),
                profondita: segmenti(radice).count(),
                indice,
            });
        }
        // Dalla più profonda alla meno: la radice di un brano è il **prefisso
        // più lungo** che lo contiene, e cercandole in quest'ordine ci si ferma
        // alla prima che risponde invece di provarle tutte.
        sorvegliate.sort_by_key(|s| std::cmp::Reverse(s.profondita));

        for riga in righe {
            let confini = confini_dei_segmenti(&riga.percorso);
            let totale = confini.len();
            let sotto_radice = sorvegliate
                .iter()
                .find(|s| is_under(&riga.percorso, &s.percorso, regole));
            let (mut corrente, profondita) = match sotto_radice {
                Some(s) => (s.indice, s.profondita),
                None => {
                    // Un brano fuori da ogni radice sorvegliata prende l'unità
                    // del suo percorso. **La condivisione di rete conta per
                    // una**: `\\server\condivisione` è un posto, non due
                    // segmenti, e spezzarla darebbe un nodo `server` che non si
                    // può aprire e che non contiene niente.
                    let profondita = if e_unc(&riga.percorso) { 2 } else { 1 };
                    let Some(indice) = trova_o_crea(
                        &mut lavoro,
                        &riga.percorso,
                        &confini,
                        profondita,
                        None,
                        regole,
                    ) else {
                        continue;
                    };
                    (indice, profondita)
                }
            };
            // Il percorso è la radice stessa, o più corto: non c'è nessun file
            // da appendere. Non capita su una libreria vera — `tracks.path` è
            // sempre il percorso di un file — e saltarlo costa meno che fidarsi.
            if totale <= profondita {
                continue;
            }
            // Dalla radice all'ultima cartella: l'ultimo segmento è il nome del
            // file, e un file non è un nodo.
            for livello in profondita..totale.saturating_sub(1) {
                let Some(indice) = trova_o_crea(
                    &mut lavoro,
                    &riga.percorso,
                    &confini,
                    livello.saturating_add(1),
                    Some(corrente),
                    regole,
                ) else {
                    break;
                };
                corrente = indice;
            }
            if let Some(nodo) = lavoro.nodi.get_mut(corrente) {
                nodo.sotto = nodo.sotto.saturating_add(1);
            }
            if con_identificativi && let Some(cartella) = lavoro.brani.get_mut(corrente) {
                cartella.push(DaOrdinare {
                    // Chi non ha numero va in coda, non in testa: dentro un
                    // album i non numerati sono i bonus.
                    disco: riga.disco.unwrap_or(i64::MAX),
                    traccia: riga.traccia.unwrap_or(i64::MAX),
                    nome: path_key(base_name(&riga.percorso), regole),
                    id: riga.id,
                });
            }
        }

        let InCostruzione {
            mut nodi,
            padri,
            brani,
            per_chiave,
            mut cime,
        } = lavoro;

        // I conteggi, dal basso. Un figlio nasce sempre dopo suo padre — lo si
        // crea scendendo — quindi il suo indice è più grande, e una passata al
        // contrario basta a far salire tutto.
        for indice in (0..nodi.len()).rev() {
            let quanti = nodi.get(indice).map_or(0, |nodo| nodo.sotto);
            let Some(Some(padre)) = padri.get(indice).copied() else {
                continue;
            };
            if let Some(nodo) = nodi.get_mut(padre) {
                nodo.sotto = nodo.sotto.saturating_add(quanti);
            }
        }

        // I fratelli in ordine di chiave. Fra figli dello stesso padre l'ordine
        // per chiave intera e quello per solo nome coincidono — il prefisso è
        // lo stesso — e il secondo si calcola su venti caratteri invece che su
        // centoventi.
        let ordine: Vec<String> = nodi.iter().map(|n| path_key(&n.nome, regole)).collect();
        for nodo in &mut nodi {
            nodo.figli
                .sort_by(|a, b| ordine.get(*a).cmp(&ordine.get(*b)));
        }
        cime.sort_by(|a, b| ordine.get(*a).cmp(&ordine.get(*b)));

        // I brani dentro ogni cartella, e poi via la chiave di ordinamento:
        // quel che resta sono otto byte per brano.
        for (indice, mut cartella) in brani.into_iter().enumerate() {
            cartella
                .sort_by(|a, b| (a.disco, a.traccia, &a.nome).cmp(&(b.disco, b.traccia, &b.nome)));
            if let Some(nodo) = nodi.get_mut(indice) {
                // `iter` e non `into_iter`, e la differenza vale un megabyte su
                // diciottomila brani: da un `Vec` consumato per valore la
                // collezione **riusa l'allocazione di partenza**, e quella è
                // dimensionata su `DaOrdinare` — quarantotto byte — invece che
                // sugli otto di un identificativo. Il vettore risultante
                // sarebbe corretto e sei volte più largo del necessario.
                nodo.brani = cartella.iter().map(|b| b.id).collect();
            }
        }

        Self {
            nodi,
            per_chiave,
            cime,
            con_identificativi,
            regole,
        }
    }

    /// Quanti byte di mucchio tiene l'albero, contati uno per uno.
    ///
    /// Non è una stima a spanne: somma la capacità vera di ogni vettore e di
    /// ogni stringa. Non conta quel che il gestore di memoria arrotonda per
    /// conto suo, quindi il numero è un limite inferiore stretto.
    #[cfg(test)]
    fn peso_in_byte(&self) -> usize {
        let mut peso = std::mem::size_of::<Self>();
        peso += self.nodi.capacity() * std::mem::size_of::<Nodo>();
        for nodo in &self.nodi {
            peso += nodo.percorso.capacity() + nodo.nome.capacity();
            peso += nodo.figli.capacity() * std::mem::size_of::<usize>();
            peso += nodo.brani.capacity() * std::mem::size_of::<i64>();
        }
        peso += self.cime.capacity() * std::mem::size_of::<usize>();
        // La tabella di una `HashMap` non si può interrogare per byte: la si
        // conta come una voce per posto riservato più il testo delle chiavi.
        for chiave in self.per_chiave.keys() {
            peso += chiave.capacity();
        }
        peso += self.per_chiave.capacity()
            * (std::mem::size_of::<String>() + std::mem::size_of::<usize>() + 1);
        peso
    }
}

/// Gli identificativi dei brani sotto `percorso`, rileggendo il database.
///
/// È la via per le librerie oltre [`SOGLIA_IDENTIFICATIVI`], dove il trie i
/// brani non se li ricorda. Costa una lettura intera di `tracks` — la stessa
/// che costa costruire l'albero — e si paga una volta, quando qualcuno preme
/// «riproduci» su una cartella.
///
/// L'ordine è **identico** a quello di [`Albero::brani_sotto`], e non per
/// coincidenza: costruisce un albero delle sole righe che stanno sotto
/// `percorso` e gli fa la stessa domanda. Due ordinamenti scritti due volte
/// sarebbero due ordinamenti che un giorno divergono.
///
/// # Errori
///
/// Quel che risponde SQLite se la lettura fallisce.
pub fn brani_sotto_dal_database(
    connection: &Connection,
    percorso: &str,
    rules: PathRules,
) -> Result<Vec<i64>, AppError> {
    let righe: Vec<Riga> = leggi_righe(connection)?
        .into_iter()
        .filter(|riga| is_under(&riga.percorso, percorso, rules))
        .collect();
    let radici = [percorso.to_owned()];
    let albero = Albero::da_righe(righe, &radici, rules, true);
    // `unwrap_or_default` e non un `expect`: l'albero è appena stato costruito
    // con gli identificativi, quindi `None` qui non può succedere — e se un
    // giorno potesse, un elenco vuoto è la risposta prudente, non un panico
    // dentro un pannello di navigazione.
    Ok(albero.brani_sotto(percorso).unwrap_or_default())
}

/// Legge da `tracks` le sole quattro colonne che l'albero guarda.
///
/// Quattro e non `SELECT *`: su diciottomila righe la differenza fra queste e
/// la riga intera — testi, commenti, impronte — sono due megabyte contro
/// parecchie decine.
fn leggi_righe(connection: &Connection) -> Result<Vec<Riga>, AppError> {
    let guasto =
        |err: &rusqlite::Error| crate::db::codice_da_sqlite("l'albero delle cartelle", err);
    let mut statement = connection
        .prepare("SELECT id, path, disc_number, track_number FROM tracks")
        .map_err(|err| guasto(&err))?;
    let mappate = statement
        .query_map([], |riga| {
            Ok(Riga {
                id: riga.get(0)?,
                percorso: riga.get(1)?,
                disco: riga.get(2)?,
                traccia: riga.get(3)?,
            })
        })
        .map_err(|err| guasto(&err))?;
    let mut righe = Vec::new();
    for riga in mappate {
        righe.push(riga.map_err(|err| guasto(&err))?);
    }
    Ok(righe)
}

/// Trova il nodo alla profondità data lungo `percorso`, o lo crea.
///
/// `profondita` si conta in segmenti non vuoti: 1 è `C:`, 2 è `C:\Musica`.
/// `padre` è `None` per i nodi di primo livello, che finiscono in
/// [`InCostruzione::cime`]. Restituisce `None` quando il percorso non arriva a
/// quella profondità, cioè quando non c'è niente da creare.
fn trova_o_crea(
    lavoro: &mut InCostruzione,
    percorso: &str,
    confini: &[(usize, usize)],
    profondita: usize,
    padre: Option<usize>,
    regole: PathRules,
) -> Option<usize> {
    let (inizio, fine) = *confini.get(profondita.checked_sub(1)?)?;
    let prefisso = percorso.get(..fine)?;
    let nome = percorso.get(inizio..fine)?;
    let ch = chiave(prefisso, regole);
    if let Some(indice) = lavoro.per_chiave.get(&ch) {
        return Some(*indice);
    }
    let indice = lavoro.nodi.len();
    lavoro.nodi.push(Nodo {
        percorso: prefisso.to_owned(),
        // Un nodo di primo livello dice il percorso intero: `C:` da solo si
        // legge, `condivisione` senza il server no.
        nome: if padre.is_none() {
            prefisso.to_owned()
        } else {
            nome.to_owned()
        },
        figli: Vec::new(),
        brani: Vec::new(),
        sotto: 0,
        radice: false,
    });
    lavoro.padri.push(padre);
    lavoro.brani.push(Vec::new());
    lavoro.per_chiave.insert(ch, indice);
    match padre {
        Some(padre) => {
            if let Some(nodo) = lavoro.nodi.get_mut(padre) {
                nodo.figli.push(indice);
            }
        }
        None => lavoro.cime.push(indice),
    }
    Some(indice)
}

/// I segmenti non vuoti di un percorso, con qualunque separatore.
fn segmenti(percorso: &str) -> impl Iterator<Item = &str> {
    percorso
        .split(['/', '\\'])
        .filter(|parte| !parte.is_empty())
}

/// Dove comincia e dove finisce ogni segmento non vuoto, in byte.
///
/// Serve a ritagliare dal percorso **originale** il prefisso di ogni cartella:
/// i nodi mostrano la grafia con cui il brano sta sul disco, e ricostruirla dai
/// segmenti la perderebbe — `C:/Musica` tornerebbe indietro come `C:\Musica`.
///
/// Gli scarti si contano sul percorso originale e non sulla chiave perché le
/// due non hanno gli stessi byte: `path_key` piega le maiuscole, e in UTF-8 una
/// maiuscola accentata e la sua minuscola non occupano sempre lo stesso spazio.
fn confini_dei_segmenti(percorso: &str) -> Vec<(usize, usize)> {
    let mut confini = Vec::new();
    let mut inizio: Option<usize> = None;
    for (posizione, carattere) in percorso.char_indices() {
        if carattere == '/' || carattere == '\\' {
            if let Some(da) = inizio.take() {
                confini.push((da, posizione));
            }
        } else if inizio.is_none() {
            inizio = Some(posizione);
        }
    }
    if let Some(da) = inizio {
        confini.push((da, percorso.len()));
    }
    confini
}

/// Il percorso comincia con due separatori, cioè è una condivisione di rete.
fn e_unc(percorso: &str) -> bool {
    let mut caratteri = percorso.chars();
    matches!(
        (caratteri.next(), caratteri.next()),
        (Some('/' | '\\'), Some('/' | '\\'))
    )
}

/// La chiave di un nodo: [`path_key`] più la fusione dei separatori ripetuti.
///
/// `path_key` è la forma canonica per l'appartenenza a un insieme, e fa già il
/// lavoro che conta: unifica le barre e piega le maiuscole dove il filesystem
/// non le distingue. Toglie però i separatori solo **in coda**, e qui serve
/// anche fondere quelli interni: le chiavi di questo albero si calcolano una
/// volta sul percorso della radice e una volta sul prefisso ritagliato dal
/// percorso di un brano, e `C:\\Musica` contro `C:\Musica` darebbe due nodi per
/// la stessa cartella. Il guasto sarebbe cosmetico — qui non si cancella
/// niente — ma costa cinque righe evitarlo.
///
/// Il doppio separatore di testa invece si conserva: è quel che distingue
/// `\\server\musica` da un percorso relativo che comincia con `server`.
fn chiave(percorso: &str, regole: PathRules) -> String {
    let mut disteso = String::with_capacity(percorso.len().saturating_add(2));
    if e_unc(percorso) {
        disteso.push_str("//");
    }
    let mut primo = true;
    for segmento in segmenti(percorso) {
        if !primo {
            disteso.push('/');
        }
        disteso.push_str(segmento);
        primo = false;
    }
    path_key(&disteso, regole)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: PathRules = PathRules {
        case_insensitive: true,
    };

    fn banco() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    fn riga(
        connection: &Connection,
        id: i64,
        percorso: &str,
        disco: Option<i64>,
        traccia: Option<i64>,
    ) {
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified, disc_number, track_number)
                 VALUES (?1, ?2, ?3, 'T', 'A', 'Al', 1000, 99999, 0, 0, ?4, ?5)",
                rusqlite::params![id, percorso, format!("a|t{id}|al"), disco, traccia],
            )
            .expect("riga di brano");
    }

    fn nomi(nodi: &[NodoCartella]) -> Vec<String> {
        nodi.iter().map(|n| n.nome.clone()).collect()
    }

    #[test]
    fn solo_le_cartelle_con_brani_indicizzati() {
        let c = banco();
        // Sul disco, accanto a «Rock», c'è anche «Jazz»: nessuno dei suoi file
        // è entrato in libreria, e nel pannello non compare. È la scelta
        // dichiarata nel `//!` — la sorgente è il database.
        riga(&c, 1, r"C:\Musica\Rock\a.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\Musica".to_owned()], WIN).expect("albero");
        assert_eq!(nomi(&albero.figlie(Some(r"C:\Musica"))), vec!["Rock"]);
        assert_eq!(albero.quanti_sotto(r"C:\Musica\Jazz"), 0);
    }

    #[test]
    fn il_confine_di_separatore_non_fonde_due_cartelle() {
        let c = banco();
        // Il difetto storico di `paths.rs`: `C:\Musica` comincia con `C:\Music`.
        riga(&c, 1, r"C:\Music\a.mp3", None, None);
        riga(&c, 2, r"C:\Musica\b.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\Music".to_owned()], WIN).expect("albero");
        assert_eq!(albero.quanti_sotto(r"C:\Music"), 1, "solo il suo");
        // L'altro non è sparito: ha preso un nodo sintetico sotto la sua unità.
        assert_eq!(albero.quanti_sotto(r"C:\Musica"), 1);
        assert!(
            !albero
                .figlie(Some(r"C:\Music"))
                .iter()
                .any(|n| n.nome == "Musica")
        );
    }

    #[test]
    fn i_conteggi_sono_ricorsivi() {
        let c = banco();
        riga(&c, 1, r"C:\M\A\1.mp3", None, None);
        riga(&c, 2, r"C:\M\A\B\2.mp3", None, None);
        riga(&c, 3, r"C:\M\A\B\3.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert_eq!(albero.quanti_sotto(r"C:\M"), 3);
        assert_eq!(albero.quanti_sotto(r"C:\M\A"), 3);
        assert_eq!(albero.quanti_sotto(r"C:\M\A\B"), 2);
        let figlie = albero.figlie(Some(r"C:\M"));
        assert_eq!(figlie.len(), 1);
        let prima = figlie.first().expect("la cartella A");
        assert_eq!(prima.brani, 3, "il conteggio conta anche B");
        assert_eq!(prima.sottocartelle, 1);
    }

    #[test]
    fn unc_e_lettera_restano_due_radici() {
        let c = banco();
        riga(&c, 1, r"\\srv\musica\a.mp3", None, None);
        riga(&c, 2, r"Z:\musica\b.mp3", None, None);
        let radici = vec![r"\\srv\musica".to_owned(), r"Z:\musica".to_owned()];
        let albero = Albero::costruisci(&c, &radici, WIN).expect("albero");
        let cime = albero.figlie(None);
        assert_eq!(cime.len(), 2, "due radici, non tre");
        // La condivisione è **un** nodo: nessun `srv` vuoto di mezzo.
        assert!(!cime.iter().any(|n| n.nome == "srv"));
        assert_eq!(albero.quanti_sotto(r"\\srv\musica"), 1);
        assert_eq!(albero.quanti_sotto(r"Z:\musica"), 1);
    }

    #[test]
    fn le_due_grafie_di_una_condivisione_sono_lo_stesso_posto() {
        let c = banco();
        // Come le scrive Esplora risorse, e come le scrive un M3U.
        riga(&c, 1, r"\\srv\musica\a.mp3", None, None);
        riga(&c, 2, "//SRV/Musica/b.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"\\srv\musica".to_owned()], WIN).expect("albero");
        assert_eq!(albero.figlie(None).len(), 1);
        assert_eq!(albero.quanti_sotto(r"\\srv\musica"), 2);
    }

    #[test]
    fn una_condivisione_senza_radice_resta_un_nodo_solo() {
        let c = banco();
        // Nessuna radice sorvegliata: il nodo sintetico deve fermarsi alla
        // condivisione, non spezzarsi in `srv` più `musica`.
        riga(&c, 1, r"\\srv\musica\Rock\a.mp3", None, None);
        let albero = Albero::costruisci(&c, &[], WIN).expect("albero");
        assert_eq!(nomi(&albero.figlie(None)), vec![r"\\srv\musica"]);
        assert_eq!(nomi(&albero.figlie(Some(r"\\srv\musica"))), vec!["Rock"]);
    }

    #[test]
    fn maiuscole_diverse_non_sdoppiano_un_nodo() {
        let c = banco();
        riga(&c, 1, r"C:\M\Rock\a.mp3", None, None);
        riga(&c, 2, r"C:\M\rock\b.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        let figlie = albero.figlie(Some(r"C:\M"));
        assert_eq!(figlie.len(), 1, "una cartella sola");
        assert_eq!(
            albero.quanti_sotto(r"C:\M\ROCK"),
            2,
            "e la si trova comunque"
        );
    }

    #[test]
    fn brani_sotto_ordina_per_disco_e_traccia() {
        let c = banco();
        riga(&c, 1, r"C:\M\Al\z.mp3", Some(1), Some(2));
        riga(&c, 2, r"C:\M\Al\a.mp3", Some(2), Some(1));
        riga(&c, 3, r"C:\M\Al\m.mp3", Some(1), Some(1));
        // Senza numero: in coda ai numerati, non in testa.
        riga(&c, 4, r"C:\M\Al\b.mp3", None, None);
        // E la sottocartella dopo tutto quel che sta nella cartella.
        riga(&c, 5, r"C:\M\Al\Bonus\x.mp3", Some(1), Some(1));
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert_eq!(albero.brani_sotto(r"C:\M\Al"), Some(vec![3, 1, 2, 4, 5]));
    }

    #[test]
    fn a_parita_di_numero_decide_il_nome() {
        let c = banco();
        riga(&c, 1, r"C:\M\Al\b.mp3", None, None);
        riga(&c, 2, r"C:\M\Al\A.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert_eq!(albero.brani_sotto(r"C:\M\Al"), Some(vec![2, 1]));
    }

    #[test]
    fn un_brano_fuori_dalle_radici_ha_il_suo_nodo() {
        let c = banco();
        riga(&c, 1, r"C:\M\a.mp3", None, None);
        riga(&c, 2, r"D:\altro\b.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        let cime = albero.figlie(None);
        assert_eq!(cime.len(), 2);
        let sintetico = cime.iter().find(|n| !n.radice).expect("il nodo sintetico");
        assert_eq!(sintetico.nome, "D:");
        assert_eq!(sintetico.brani, 1);
        assert_eq!(albero.brani_sotto("D:"), Some(vec![2]));
    }

    #[test]
    fn una_radice_senza_brani_resta_nel_pannello() {
        let c = banco();
        let radici = vec![r"C:\M".to_owned(), r"E:\vuota".to_owned()];
        riga(&c, 1, r"C:\M\a.mp3", None, None);
        let albero = Albero::costruisci(&c, &radici, WIN).expect("albero");
        assert_eq!(albero.figlie(None).len(), 2);
        assert_eq!(albero.quanti_sotto(r"E:\vuota"), 0);
    }

    #[test]
    fn la_radice_e_il_prefisso_piu_lungo() {
        let c = banco();
        riga(&c, 1, r"C:\M\Live\a.mp3", None, None);
        riga(&c, 2, r"C:\M\Studio\b.mp3", None, None);
        let radici = vec![r"C:\M".to_owned(), r"C:\M\Live".to_owned()];
        let albero = Albero::costruisci(&c, &radici, WIN).expect("albero");
        // «Live» è una radice sua, quindi non conta dentro «C:\M» e non compare
        // due volte.
        assert_eq!(albero.quanti_sotto(r"C:\M"), 1);
        assert_eq!(albero.quanti_sotto(r"C:\M\Live"), 1);
        assert_eq!(nomi(&albero.figlie(Some(r"C:\M"))), vec!["Studio"]);
    }

    #[test]
    fn i_separatori_ripetuti_non_sdoppiano_un_nodo() {
        let c = banco();
        riga(&c, 1, r"C:\M\Rock\a.mp3", None, None);
        riga(&c, 2, r"C:\M\\Rock\b.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert_eq!(albero.figlie(Some(r"C:\M")).len(), 1);
        assert_eq!(albero.quanti_sotto(r"C:\M\Rock"), 2);
    }

    #[test]
    fn senza_identificativi_restano_i_conteggi() {
        let righe = vec![
            Riga {
                id: 1,
                percorso: r"C:\M\A\a.mp3".to_owned(),
                disco: None,
                traccia: None,
            },
            Riga {
                id: 2,
                percorso: r"C:\M\A\b.mp3".to_owned(),
                disco: None,
                traccia: None,
            },
        ];
        let albero = Albero::da_righe(righe, &[r"C:\M".to_owned()], WIN, false);
        assert!(!albero.con_identificativi());
        assert_eq!(albero.quanti_sotto(r"C:\M\A"), 2, "la forma resta");
        // `None` e non un elenco vuoto: sopra la soglia la risposta è «non lo
        // so», e va distinta da «questa cartella non ha brani». Le due si
        // confondevano finché il tipo era un `Vec`.
        assert_eq!(albero.brani_sotto(r"C:\M\A"), None, "gli id no");
        assert_eq!(
            albero.brani_sotto(r"Q:\niente"),
            None,
            "e nemmeno qui si sa"
        );
    }

    #[test]
    fn la_rilettura_dal_database_da_lo_stesso_ordine() {
        let c = banco();
        riga(&c, 1, r"C:\M\Al\z.mp3", Some(1), Some(2));
        riga(&c, 2, r"C:\M\Al\a.mp3", Some(2), Some(1));
        riga(&c, 3, r"C:\M\Al\m.mp3", Some(1), Some(1));
        riga(&c, 4, r"C:\M\Al\Bonus\x.mp3", Some(1), Some(1));
        riga(&c, 5, r"C:\M\Altro\y.mp3", Some(1), Some(1));
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        let dal_trie = albero
            .brani_sotto(r"C:\M\Al")
            .expect("gli identificativi ci sono");
        let dal_db = brani_sotto_dal_database(&c, r"C:\M\Al", WIN).expect("rilettura");
        assert_eq!(dal_trie, dal_db);
        // «Altro» non entra: il confine di separatore vale anche qui.
        assert_eq!(dal_db, vec![3, 1, 2, 4]);
    }

    #[test]
    fn un_percorso_sconosciuto_non_rompe_niente() {
        let c = banco();
        riga(&c, 1, r"C:\M\a.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert!(albero.figlie(Some(r"Q:\niente")).is_empty());
        // Un `Some` vuoto: gli identificativi ci sono, e sotto quel percorso
        // non c'è niente. Non è il vuoto della soglia superata.
        assert_eq!(albero.brani_sotto(r"Q:\niente"), Some(Vec::new()));
        assert_eq!(albero.quanti_sotto(r"Q:\niente"), 0);
    }

    /// Quanto costa l'albero sulla libreria dell'autore: 18.534 brani.
    ///
    /// Non è una curiosità: il vincolo di questa release è che l'occupazione di
    /// memoria non peggiori, e un albero che si tiene in RAM va misurato prima
    /// di prometterlo. La libreria finta ha la forma di quella vera — artista,
    /// album, tracce — perché il costo sta nelle stringhe dei percorsi, e un
    /// albero di percorsi corti direbbe un numero che non vale per nessuno.
    /// Una libreria finta con la forma di una vera: artista, album, tracce.
    ///
    /// Il costo del trie sta quasi tutto nelle stringhe dei percorsi, e un
    /// albero di percorsi corti direbbe un numero che non vale per nessuno.
    fn libreria_finta(brani: usize) -> Vec<Riga> {
        let mut righe = Vec::with_capacity(brani);
        let mut fatti = 0usize;
        let mut artista = 0usize;
        while fatti < brani {
            for album in 0..8 {
                for traccia in 1..=14i64 {
                    if fatti >= brani {
                        break;
                    }
                    righe.push(Riga {
                        percorso: format!(
                            r"C:\Musica\Artista Numero {artista:04}\{album} - Un Titolo D'Album Abbastanza Lungo\{traccia:02} - Il Titolo Della Traccia.flac"
                        ),
                        id: i64::try_from(fatti).unwrap_or(0),
                        disco: Some(1),
                        traccia: Some(traccia),
                    });
                    fatti += 1;
                }
            }
            artista += 1;
        }
        righe
    }

    /// Quanto costa l'albero sulla libreria dell'autore: 18.534 brani.
    ///
    /// Non è una curiosità: il vincolo di questa release è che l'occupazione di
    /// memoria non peggiori, e un albero che si tiene in RAM va misurato prima
    /// di prometterlo.
    #[test]
    fn il_trie_sta_nel_suo_budget() {
        const BRANI: usize = 18_534;
        let radici = [r"C:\Musica".to_owned()];
        let albero = Albero::da_righe(libreria_finta(BRANI), &radici, WIN, true);
        let peso = albero.peso_in_byte();
        assert_eq!(albero.quanti_sotto(r"C:\Musica"), 18_534);
        // Tre megabyte è il tetto dichiarato nel piano della release.
        assert!(
            peso < 3 * 1024 * 1024,
            "il trie costa {peso} byte su {} nodi: sopra il tetto di 3 MB",
            albero.quanti_nodi()
        );
        // Quanto costa la parte a cui si rinuncia oltre la soglia: è il numero
        // che dice se quella rinuncia vale la pena, e va letto insieme all'altro.
        let senza = Albero::da_righe(libreria_finta(BRANI), &radici, WIN, false);
        let peso_senza = senza.peso_in_byte();
        assert!(peso_senza < peso);
        // I numeri misurati si stampano: `cargo test -- --nocapture` li mostra,
        // e il rapporto della release li cita.
        println!(
            "trie: {peso} byte con gli identificativi, {peso_senza} senza, {} nodi, {BRANI} brani",
            albero.quanti_nodi()
        );
    }
}
