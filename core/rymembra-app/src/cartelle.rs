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
//! Non per il costo della singola domanda. Dalla 026 `tracks.path` è `TEXT
//! UNIQUE` (nullabile, per i brani di catalogo), e dalla 030 c'è
//! `idx_tracks_percorso_piegato ON tracks(path COLLATE NOCASE)`. Con il pragma
//! `case_sensitive_like` di serie (spento, cioè `LIKE` insensibile alle
//! maiuscole degli ASCII) e una colonna indicizzata `NOCASE`, SQLite converte
//! `path LIKE 'prefisso%'` in una ricerca per intervallo su quell'indice, come
//! converte `GLOB` sull'indice `BINARY` della chiave unica. Sul SQLite incluso
//! (la prova `il_like_sul_percorso_usa_l_indice_piegato` lo tiene d'occhio):
//!
//! ```text
//! path LIKE 'C:\Musica\%'  ->  SEARCH tracks USING COVERING INDEX idx_tracks_percorso_piegato (path>? AND path<?)
//! path GLOB 'C:\Musica\*'  ->  SEARCH tracks USING COVERING INDEX sqlite_autoindex_tracks_1 (path>? AND path<?)
//! ```
//!
//! **Il problema sono i byte del prefisso**, che non si conoscono. Lo stesso
//! posto sta nel database scritto `C:\Musica` o `C:/Musica` — è precisamente la
//! ragione per cui `paths.rs::path_key` esiste — e il `LIKE` piega le maiuscole
//! ma non i separatori: un prefisso che dovesse coprire tutte le grafie non è
//! un prefisso. Farlo vorrebbe dire una `OR` per ogni variante dei separatori,
//! a ogni livello dell'albero, **a ogni clic di espansione**, e ancora non
//! direbbe quali sottocartelle ci sono sotto.
//!
//! Il trie invece si costruisce con **una** lettura di tutta la tabella
//! (`SELECT id, path, disc_number, track_number`), con i percorsi passati da
//! `path_key`, e da lì ogni espansione è un accesso a una mappa.
//!
//! # Perché nessuna tabella `folders` materializzata
//!
//! Perché sarebbe una seconda verità. Le righe di `tracks` che cambiano la
//! forma dell'albero — un brano che nasce, uno che sparisce, uno che si sposta
//! — si scrivono in più di un posto: la scansione in `library/righe.rs`
//! (`insert_track`, `update_track` col ramo «Sposta», la cancellazione), la
//! rimappatura delle radici del profilo (`profilo/radici.rs`), e altri ancora;
//! nessuno di loro sa niente di cartelle. Una tabella materializzata
//! pretenderebbe che tutti restassero in passo per sempre, e il giorno in cui
//! uno non lo fa il pannello mostra cartelle che non ci sono più — senza che
//! niente segnali l'errore, perché una tabella non si accorge di essere
//! vecchia.
//!
//! Il trie non può desincronizzarsi: è **derivato** da `tracks` e si
//! ricostruisce da capo in una trentina di millisecondi — 35 ms misurati in
//! rilascio su 18.534 brani, lettura del database compresa. Quando la libreria
//! cambia non c'è niente da aggiornare, c'è solo da buttarlo via.
//!
//! # Il costo in memoria è una condizione, non un dettaglio
//!
//! Questo albero vive in RAM, e l'albero di processi di Rymembra ha un tetto che
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
//!
//! # I file sono foglie, ma non stanno nel trie
//!
//! Il pannello mostra anche i brani di una cartella aperta, come le foglie del
//! pannello «Folders» di foobar2000: titolo e durata sotto le sottocartelle. Il
//! trie però sa dei brani soltanto l'identificativo, e per una ragione che è
//! la condizione del paragrafo qui sopra: titoli e nomi di file di diciottomila
//! brani sono un altro paio di megabyte, cioè il trie raddoppiato per mostrare
//! le foglie di una cartella alla volta. Quindi [`file_della_cartella`] prende
//! dal trie gli identificativi **diretti** — già pronti, senza scendere — e
//! legge dal database le sole righe di quella cartella: una decina di righe per
//! un album, mai la tabella. Resta una lettura del database e non del disco: le
//! foglie di una share spenta si aprono come le altre.
//!
//! # Il filtro guarda tutto l'albero, non solo quel che è aperto
//!
//! Il pannello conosce le sole cartelle che ha aperto, e un filtro scritto lì
//! troverebbe soltanto quelle — cioè quasi niente, perché si filtra proprio per
//! non dover aprire a mano. [`filtra`] cerca nel trie intero e, per i file, in
//! una lettura di `tracks`, e restituisce l'albero **potato**: ogni corrispondenza
//! con i suoi antenati, ognuno con dentro soltanto i figli che portano a
//! qualcosa. La piegatura è quella della ricerca ([`fold_text`]): maiuscole e
//! accenti non contano.
//!
//! # L'ordine delle sorelle è quello di chi legge
//!
//! «CD 2» prima di «CD 10». Le cartelle sorelle si ordinano con
//! [`confronta_naturale`], che legge le cifre come numeri, e lo stesso vale per
//! i nomi dei file quando un album non ha i numeri di traccia: l'ordine in cui
//! una cartella suona è quello in cui la si vede. I file del pannello li ordina
//! la finestra, che li disegna: dentro una cartella l'ordine di riproduzione
//! resta quello dell'album.

use rymembra_domain::nuvola;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use rymembra_domain::errors::AppError;
use rymembra_domain::paths::{PathRules, base_name, is_under, path_key};
use rymembra_domain::text::fold_text;

/// Oltre questi brani il trie rinuncia agli identificativi.
///
/// Il numero è una condizione di accettazione, non una stima: su una libreria
/// di duecentomila brani gli identificativi da soli sono un megabyte e mezzo, e
/// i nomi delle cartelle che li contengono parecchi di più. Sopra la soglia si
/// tiene la forma dell'albero — che è quel che il pannello disegna — e si
/// rinuncia a quel che serve solo nel momento in cui si preme «riproduci», dove
/// una rilettura del database si può pagare perché è un gesto singolo.
pub const SOGLIA_IDENTIFICATIVI: usize = 200_000;

/// Quante corrispondenze al massimo restituisce un filtro.
///
/// Duemila. Il filtro si scrive una lettera alla volta, e con una lettera sola
/// corrisponde quasi tutto: diciottomila foglie mandate alla finestra per la
/// «a» che precede «abba» sarebbero un megabyte e mezzo di risposta buttato via
/// trecento millisecondi dopo. Duemila righe riempiono già parecchi schermi, e
/// chi ne vuole di più sta cercando con una parola troppo corta — il pannello
/// glielo dice.
pub const LIMITE_FILTRO: usize = 2_000;

/// Quanti identificativi per ogni `IN (…)` della lettura dei file.
///
/// Sotto il limite dei parametri di SQLite con un margine largo, e abbastanza
/// per leggere in un colpo solo qualunque cartella che sia un album.
const PEZZO_LETTURA: usize = 500;

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
    /// Quanti brani stanno **direttamente** qui, senza scendere.
    ///
    /// Sono le foglie che il pannello disegna aprendo la cartella. Il numero
    /// arriva con il nodo perché la freccia va disegnata subito — una cartella
    /// d'album non ha sottocartelle ma si apre lo stesso — e perché la finestra
    /// non chieda le foglie di una cartella che non ne ha.
    pub file: u32,
    /// È una delle cartelle sorvegliate.
    ///
    /// `false` sui nodi sintetici, cioè quelli nati per dare un posto a un
    /// brano che nel database c'è ma sotto nessuna radice sorvegliata non sta.
    /// La differenza conta: solo su una radice vera ha senso chiedersi se
    /// risponde ancora.
    pub radice: bool,
    /// Il servizio, sui nodi di primo livello del cloud (`onedrive`,
    /// `gdrive`…): è quel che fa disegnare la sua icona accanto al nome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servizio: Option<String>,
}

/// Il nome di un nodo come lo si legge, e il suo servizio se è una cima del
/// cloud.
///
/// Un percorso del cloud ha i segmenti codificati (`Rock%7CPop` per un nome
/// con la barra verticale: vedi `rymembra_domain::nuvola`), e un nodo di primo
/// livello dice il percorso intero — che per il cloud sarebbe
/// `nuvola://onedrive-k3x7q2ab/Musica`. Qui diventano «Rock|Pop» e
/// «OneDrive · Musica»: la grafia del percorso resta l'identità del nodo, il
/// nome è per chi guarda.
fn per_chi_guarda(percorso: &str, nome: &str) -> (String, Option<String>) {
    let Some(conto) = nuvola::conto_di(percorso) else {
        return (nome.to_owned(), None);
    };
    if nome != percorso {
        return (nuvola::decodifica_segmento(nome), None);
    }
    let servizio = nuvola::Servizio::del_conto(conto);
    let del_servizio = servizio.map_or("", nuvola::Servizio::nome);
    let nomi = nuvola::segmenti(percorso);
    let testo = if nomi.is_empty() {
        del_servizio.to_owned()
    } else {
        format!("{del_servizio} · {}", nomi.join(" › "))
    };
    (testo, servizio.map(|s| s.chiave().to_owned()))
}

/// Un file dentro una cartella: una foglia dell'albero.
///
/// Quel che la riga disegna e quel che serve a riprodurla, e nient'altro: le
/// colonne del brano intero le chiede il pannello dei brani, che le mostra.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCartella {
    /// L'identificativo del brano: è quel che va in coda.
    pub brano: i64,
    /// Il percorso del file, nella grafia del database. È anche l'identità
    /// della riga nel pannello, accanto a quella delle cartelle.
    pub percorso: String,
    /// Il nome del file, senza la cartella: è la chiave con cui la finestra
    /// ordina le foglie.
    pub nome: String,
    /// Il titolo del brano, come lo dicono i tag.
    pub titolo: String,
    /// La durata in millisecondi.
    pub durata_ms: i64,
}

/// Un ramo dell'albero potato da [`filtra`]: una cartella aperta e quel che le
/// resta dentro.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RamoFiltrato {
    /// La cartella aperta; `None` per il primo livello.
    pub percorso: Option<String>,
    /// Le sottocartelle che portano a una corrispondenza, o che lo sono,
    /// nell'ordine dell'albero.
    pub cartelle: Vec<NodoCartella>,
    /// I file di questa cartella che corrispondono.
    pub file: Vec<FileCartella>,
}

/// L'esito di un filtro: l'albero potato e quanto è grande quel che si è trovato.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartelleFiltrate {
    /// Il primo livello e ogni cartella da mostrare aperta. Una cartella che
    /// corrisponde e non contiene altre corrispondenze **non** è fra queste: si
    /// mostra chiusa, e aprendola la si vede intera.
    pub rami: Vec<RamoFiltrato>,
    /// I percorsi delle cartelle il cui nome corrisponde: la finestra le
    /// distingue dagli antenati, che stanno lì solo per portarci.
    pub trovate: Vec<String>,
    /// Quante corrispondenze c'erano in tutto, cartelle e file, prima del tetto.
    pub corrispondenze: u32,
    /// Le corrispondenze erano più di quelle restituite: vedi [`LIMITE_FILTRO`].
    pub troncato: bool,
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
    /// Il padre di ogni nodo, `None` per quelli di primo livello.
    ///
    /// Sedici byte per nodo — su diciottomila brani un migliaio e mezzo di
    /// cartelle, cioè ventiquattro kilobyte — tenuti per il filtro, che da ogni
    /// corrispondenza deve risalire fino alla cima per aprire la strada.
    /// Ricavarlo dal percorso togliendo un segmento alla volta funzionerebbe
    /// lo stesso, ma vorrebbe dire rifare a mano la regola del prefisso più
    /// lungo che la costruzione ha già applicato.
    padri: Vec<Option<usize>>,
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
    /// Quanti brani stanno direttamente qui. Non è `brani.len()`: sopra la
    /// soglia gli identificativi non ci sono, e il conteggio serve lo stesso.
    diretti: u32,
    radice: bool,
}

/// Una riga di `tracks` per le foglie: quel che [`FileCartella`] porta.
#[derive(Debug)]
struct RigaFile {
    id: i64,
    percorso: String,
    titolo: String,
    durata_ms: i64,
}

impl RigaFile {
    /// Il nome della foglia: quello del file, o il titolo per un brano
    /// d'immagine CD.
    ///
    /// Il «nome del file» di un brano d'immagine sarebbe `Album.cue|03`, che non
    /// è il nome di niente: il brano un file suo non ce l'ha, e quello che il
    /// pannello deve dire è il titolo. Se il titolo manca resta il percorso
    /// virtuale, e la finestra, che lo riconosce, scrive «Traccia 03 · Album».
    fn nome(&self) -> &str {
        if rymembra_domain::cue::brano_d_immagine(&self.percorso).is_some()
            && !self.titolo.trim().is_empty()
        {
            return &self.titolo;
        }
        base_name(&self.percorso)
    }

    fn in_foglia(self) -> FileCartella {
        // Il nome di un file del cloud, se non c'è un titolo, è un segmento
        // codificato: si legge decodificato.
        let nome = if nuvola::e_nuvola(&self.percorso) {
            nuvola::decodifica_segmento(self.nome())
        } else {
            self.nome().to_owned()
        };
        FileCartella {
            brano: self.id,
            nome,
            percorso: self.percorso,
            titolo: self.titolo,
            durata_ms: self.durata_ms,
        }
    }
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
    /// Il padre di ogni nodo, `None` per quelli di primo livello. Serve qui a
    /// far salire i conteggi una volta sola, e passa poi ad [`Albero::padri`]
    /// per il filtro. Un vettore a parte e non un campo di [`Nodo`] perché la
    /// costruzione lo vuole prima che i nodi siano finiti.
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
            .filter_map(|indice| self.nodo_cartella(*indice))
            .collect()
    }

    /// Il nodo all'indice dato, nella forma che esce.
    fn nodo_cartella(&self, indice: usize) -> Option<NodoCartella> {
        self.nodi.get(indice).map(|nodo| {
            let (nome, servizio) = per_chi_guarda(&nodo.percorso, &nodo.nome);
            NodoCartella {
                percorso: nodo.percorso.clone(),
                nome,
                brani: nodo.sotto,
                sottocartelle: u32::try_from(nodo.figli.len()).unwrap_or(u32::MAX),
                file: nodo.diretti,
                radice: nodo.radice,
                servizio,
            }
        })
    }

    /// L'indice della cartella che contiene il file a questo percorso.
    ///
    /// Si cerca il percorso senza l'ultimo segmento, ed è la stessa cartella in
    /// cui la costruzione ha messo il brano: i nodi nascono ritagliando i
    /// percorsi dei brani segmento per segmento, quindi la cartella di un file
    /// che sta in libreria ha sempre la sua chiave. `None` per un percorso che
    /// non ha una cartella nell'albero.
    fn cartella_di(&self, percorso: &str) -> Option<usize> {
        let taglio = percorso.rfind(['/', '\\'])?;
        let cartella = percorso.get(..taglio)?;
        self.per_chiave.get(&chiave(cartella, self.regole)).copied()
    }

    /// Gli identificativi dei brani che stanno **direttamente** in `percorso`,
    /// senza le sottocartelle, nell'ordine dell'album.
    ///
    /// È la metà di [`Albero::brani_sotto`] che non scende: serve al pannello
    /// dei brani quando l'interruttore «includi sottocartelle» è spento, e alle
    /// foglie. `None` sopra [`SOGLIA_IDENTIFICATIVI`], con lo stesso
    /// significato di là — «non lo so» — e la stessa via d'uscita:
    /// [`brani_diretti_dal_database`].
    #[must_use]
    pub fn brani_diretti(&self, percorso: &str) -> Option<Vec<i64>> {
        if !self.con_identificativi {
            return None;
        }
        Some(
            self.per_chiave
                .get(&chiave(percorso, self.regole))
                .and_then(|indice| self.nodi.get(*indice))
                .map_or_else(Vec::new, |nodo| nodo.brani.clone()),
        )
    }

    /// Quanti brani stanno direttamente in `percorso`. Zero per un percorso
    /// che l'albero non conosce, come [`Albero::quanti_sotto`].
    #[must_use]
    pub fn quanti_diretti(&self, percorso: &str) -> u32 {
        self.per_chiave
            .get(&chiave(percorso, self.regole))
            .and_then(|indice| self.nodi.get(*indice))
            .map_or(0, |nodo| nodo.diretti)
    }

    /// Segna da mostrare e da aprire ogni antenato di `indice`, fino alla cima:
    /// è la strada che [`filtra`] apre verso ogni corrispondenza.
    ///
    /// Si ferma al primo antenato già aperto: quello ha già percorso la sua
    /// strada, e rifarla per ogni corrispondenza sotto lo stesso disco vorrebbe
    /// dire risalire mille volte gli stessi tre gradini.
    fn apri_la_strada(
        &self,
        indice: usize,
        mostrate: &mut HashSet<usize>,
        aperte: &mut HashSet<usize>,
    ) {
        let mut qui = self.padri.get(indice).copied().flatten();
        while let Some(padre) = qui {
            mostrate.insert(padre);
            if !aperte.insert(padre) {
                break;
            }
            qui = self.padri.get(padre).copied().flatten();
        }
    }

    /// Gli identificativi dei brani sotto `percorso`, nell'ordine in cui si
    /// riproducono.
    ///
    /// L'ordine è quello che ci si aspetta da una cartella d'album, perché una
    /// cartella *è* un album nel caso normale: prima i brani che stanno
    /// direttamente qui, ordinati per `(disc_number, track_number, path_key)`,
    /// poi le sottocartelle nell'ordine in cui il pannello le mostra, ciascuna
    /// con la stessa regola. I nomi, dei file e delle cartelle, si confrontano
    /// con [`confronta_naturale`]: «2» prima di «10».
    ///
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
                diretti: 0,
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
                // Qui e solo qui: più sotto i conteggi salgono verso i padri, e
                // quel che sale non sta «direttamente» da nessuna parte.
                nodo.diretti = nodo.diretti.saturating_add(1);
            }
            if con_identificativi && let Some(cartella) = lavoro.brani.get_mut(corrente) {
                // Un brano d'immagine CD si ordina per foglio e poi per
                // numero, e tutti e due si leggono dal percorso: il nome
                // `album.cue|03`, confrontato come lo legge chi guarda, dà
                // proprio quell'ordine. Disco e numero di traccia non entrano,
                // e apposta: due fogli nella stessa cartella — `CD1.cue` e
                // `CD2.cue`, quasi sempre senza `REM DISCNUMBER` — ripartono
                // tutti e due da 1, e per numero di traccia si mescolerebbero.
                // È anche l'ordine in cui la finestra disegna le foglie
                // (`albero.ts::ordinaFile`), così quel che suona è quel che si
                // vede.
                let d_immagine = rymembra_domain::cue::brano_d_immagine(&riga.percorso).is_some();
                cartella.push(DaOrdinare {
                    // Chi non ha numero va in coda, non in testa: dentro un
                    // album i non numerati sono i bonus.
                    disco: riga.disco.filter(|_| !d_immagine).unwrap_or(i64::MAX),
                    traccia: riga.traccia.filter(|_| !d_immagine).unwrap_or(i64::MAX),
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

        // I fratelli in ordine di chiave, letta come la legge chi guarda: «CD 2»
        // prima di «CD 10». Fra figli dello stesso padre l'ordine per chiave
        // intera e quello per solo nome coincidono — il prefisso è lo stesso —
        // e il secondo si calcola su venti caratteri invece che su centoventi.
        let ordine: Vec<String> = nodi.iter().map(|n| path_key(&n.nome, regole)).collect();
        let per_nome = |a: &usize, b: &usize| match (ordine.get(*a), ordine.get(*b)) {
            (Some(x), Some(y)) => confronta_naturale(x, y),
            (x, y) => x.cmp(&y),
        };
        for nodo in &mut nodi {
            nodo.figli.sort_by(per_nome);
        }
        cime.sort_by(per_nome);

        // I brani dentro ogni cartella, e poi via la chiave di ordinamento:
        // quel che resta sono otto byte per brano.
        for (indice, mut cartella) in brani.into_iter().enumerate() {
            cartella.sort_by(|a, b| {
                (a.disco, a.traccia)
                    .cmp(&(b.disco, b.traccia))
                    .then_with(|| confronta_naturale(&a.nome, &b.nome))
            });
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
            padri,
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
        peso += self.padri.capacity() * std::mem::size_of::<Option<usize>>();
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
    // `unwrap_or_default` e non un `expect`: l'albero è appena stato costruito
    // con gli identificativi, quindi `None` qui non può succedere — e se un
    // giorno potesse, un elenco vuoto è la risposta prudente, non un panico
    // dentro un pannello di navigazione.
    Ok(albero_di(connection, percorso, rules)?
        .brani_sotto(percorso)
        .unwrap_or_default())
}

/// Gli identificativi dei brani che stanno direttamente in `percorso`,
/// rileggendo il database.
///
/// La metà di [`brani_sotto_dal_database`] che non scende, con lo stesso
/// costo e la stessa ragione d'essere: le librerie oltre
/// [`SOGLIA_IDENTIFICATIVI`]. L'ordine è quello di [`Albero::brani_diretti`],
/// per la stessa costruzione.
///
/// # Errori
///
/// Quel che risponde SQLite se la lettura fallisce.
pub fn brani_diretti_dal_database(
    connection: &Connection,
    percorso: &str,
    rules: PathRules,
) -> Result<Vec<i64>, AppError> {
    Ok(albero_di(connection, percorso, rules)?
        .brani_diretti(percorso)
        .unwrap_or_default())
}

/// L'albero delle sole righe sotto `percorso`, con gli identificativi.
fn albero_di(
    connection: &Connection,
    percorso: &str,
    rules: PathRules,
) -> Result<Albero, AppError> {
    let righe: Vec<Riga> = leggi_righe(connection)?
        .into_iter()
        .filter(|riga| is_under(&riga.percorso, percorso, rules))
        .collect();
    let radici = [percorso.to_owned()];
    Ok(Albero::da_righe(righe, &radici, rules, true))
}

/// I file che stanno direttamente in `percorso`: le foglie della cartella.
///
/// Gli identificativi vengono dal trie, già pronti, e dal database si leggono
/// le sole righe di quei brani — un album sono dieci righe, e nessuna
/// espansione legge la tabella intera. Sopra [`SOGLIA_IDENTIFICATIVI`], dove il
/// trie gli identificativi non li ha, si legge `tracks` e si tengono le righe la
/// cui cartella è questa: costa quel che costa costruire l'albero, e su una
/// libreria così è il prezzo dichiarato di ogni gesto che vuole i brani.
///
/// L'ordine non è una promessa: le foglie le ordina la finestra, per nome di
/// file, perché è lei a disegnarle. Quando gli identificativi ci sono escono
/// nell'ordine dell'album, che è gratis.
///
/// # Errori
///
/// Quel che risponde SQLite se la lettura fallisce.
pub fn file_della_cartella(
    connection: &Connection,
    albero: &Albero,
    percorso: &str,
) -> Result<Vec<FileCartella>, AppError> {
    if albero.quanti_diretti(percorso) == 0 {
        return Ok(Vec::new());
    }
    if let Some(ids) = albero.brani_diretti(percorso) {
        return righe_per_id(connection, &ids);
    }
    let cartella = albero
        .per_chiave
        .get(&chiave(percorso, albero.regole))
        .copied();
    Ok(leggi_file(connection)?
        .into_iter()
        .filter(|riga| cartella.is_some() && albero.cartella_di(&riga.percorso) == cartella)
        .map(RigaFile::in_foglia)
        .collect())
}

/// L'albero potato dal filtro `testo`: ogni cartella e ogni file il cui nome
/// lo contiene, con gli antenati che portano fin lì.
///
/// # Cosa corrisponde
///
/// Una cartella, se il **nome** che il pannello le scrive contiene il testo —
/// per le cime è il percorso intero, come nella riga. Un file, se lo contiene
/// il nome del file **o** il titolo: la foglia mostra il titolo e ha il nome
/// nel suggerimento, e un filtro che non trovasse quel che c'è scritto nella
/// riga sembrerebbe rotto. Il confronto passa da [`fold_text`] da entrambe le
/// parti, cioè la piegatura della ricerca: «Ö» e «o» sono la stessa lettera, e
/// le maiuscole non contano.
///
/// # Quel che esce
///
/// Un [`RamoFiltrato`] per il primo livello e uno per ogni cartella da tenere
/// aperta, cioè ogni cartella che ha sotto di sé qualcosa da mostrare. Dentro
/// ciascuno restano soltanto i figli che sono una corrispondenza o portano a
/// una: le sorelle che non c'entrano non si vedono, che è il punto di un
/// filtro.
///
/// Oltre `limite` corrispondenze ci si ferma — prima le cartelle, che danno la
/// forma, poi i file — e [`CartelleFiltrate::troncato`] lo dice.
///
/// # Errori
///
/// Quel che risponde SQLite se la lettura dei file fallisce.
pub fn filtra(
    connection: &Connection,
    albero: &Albero,
    testo: &str,
    limite: usize,
) -> Result<CartelleFiltrate, AppError> {
    let ago = fold_text(testo.trim());
    if ago.is_empty() {
        return Ok(CartelleFiltrate::default());
    }

    let mut cartelle: Vec<usize> = albero
        .nodi
        .iter()
        .enumerate()
        .filter(|(_, nodo)| fold_text(&nodo.nome).contains(&ago))
        .map(|(indice, _)| indice)
        .collect();
    let mut file: Vec<(usize, FileCartella)> = Vec::new();
    for riga in leggi_file(connection)? {
        // Il nome della foglia e non quello del percorso: per un brano
        // d'immagine il secondo è `Album.cue|03`, e cercando «album» ogni
        // traccia del disco si accenderebbe per un nome che nessuna riga
        // scrive.
        let combacia =
            fold_text(riga.nome()).contains(&ago) || fold_text(&riga.titolo).contains(&ago);
        if !combacia {
            continue;
        }
        // Un file senza cartella nell'albero non ha dove essere mostrato: è un
        // percorso che la costruzione ha saltato, e qui si salta allo stesso
        // modo invece di inventargli un posto.
        if let Some(cartella) = albero.cartella_di(&riga.percorso) {
            file.push((cartella, riga.in_foglia()));
        }
    }

    let corrispondenze = cartelle.len().saturating_add(file.len());
    cartelle.truncate(limite);
    file.truncate(limite.saturating_sub(cartelle.len()));

    // Da mostrare: le corrispondenze e i loro antenati. Da aprire: tutto quel
    // che ha sotto qualcosa da mostrare. Una cartella trovata che non contiene
    // altre corrispondenze resta chiusa — aprendola si vede intera.
    let mut mostrate: HashSet<usize> = HashSet::new();
    let mut aperte: HashSet<usize> = HashSet::new();
    for indice in &cartelle {
        mostrate.insert(*indice);
        albero.apri_la_strada(*indice, &mut mostrate, &mut aperte);
    }
    let mut per_cartella: HashMap<usize, Vec<FileCartella>> = HashMap::new();
    for (cartella, foglia) in file {
        mostrate.insert(cartella);
        if aperte.insert(cartella) {
            albero.apri_la_strada(cartella, &mut mostrate, &mut aperte);
        }
        per_cartella.entry(cartella).or_default().push(foglia);
    }

    let mut rami = vec![RamoFiltrato {
        percorso: None,
        cartelle: albero
            .cime
            .iter()
            .filter(|indice| mostrate.contains(indice))
            .filter_map(|indice| albero.nodo_cartella(*indice))
            .collect(),
        file: Vec::new(),
    }];
    // In ordine di indice: la risposta non dipende dall'ordine di un insieme,
    // e due filtri uguali danno due risposte uguali byte per byte.
    let mut da_aprire: Vec<usize> = aperte.into_iter().collect();
    da_aprire.sort_unstable();
    for indice in da_aprire {
        let Some(nodo) = albero.nodi.get(indice) else {
            continue;
        };
        rami.push(RamoFiltrato {
            percorso: Some(nodo.percorso.clone()),
            cartelle: nodo
                .figli
                .iter()
                .filter(|figlio| mostrate.contains(figlio))
                .filter_map(|figlio| albero.nodo_cartella(*figlio))
                .collect(),
            file: per_cartella.remove(&indice).unwrap_or_default(),
        });
    }

    Ok(CartelleFiltrate {
        rami,
        trovate: cartelle
            .iter()
            .filter_map(|indice| albero.nodi.get(*indice))
            .map(|nodo| nodo.percorso.clone())
            .collect(),
        corrispondenze: u32::try_from(corrispondenze).unwrap_or(u32::MAX),
        troncato: corrispondenze > limite,
    })
}

/// `percorso` sta dentro una delle cartelle sorvegliate, o è una di loro.
///
/// È la guardia del comando che apre una cartella in Esplora risorse: la
/// finestra nomina un percorso, e un comando che aprisse il gestore file su un
/// percorso qualunque sarebbe una porta su tutto il disco. Le radici le legge
/// chi chiama, dal database — **mai** dalla finestra, che altrimenti si
/// sceglierebbe da sola il recinto.
///
/// Il confronto è [`is_under`], che conosce il confine di separatore (`C:\Music`
/// non contiene `C:\Musica`). In più si rifiuta ogni percorso con un segmento
/// `.` o `..`: `is_under` confronta le grafie e non risolve niente, quindi
/// `C:\Musica\..\Windows` gli sembrerebbe dentro `C:\Musica` — ed è
/// precisamente il percorso che un comando così non deve accettare.
#[must_use]
pub fn sotto_una_radice(percorso: &str, radici: &[String], regole: PathRules) -> bool {
    if percorso.trim().is_empty() || percorso.contains('\0') {
        return false;
    }
    if segmenti(percorso).any(|parte| parte == "." || parte == "..") {
        return false;
    }
    radici
        .iter()
        .any(|radice| is_under(percorso, radice, regole))
}

/// Confronta due nomi come li ordina chi li legge: le cifre come numeri.
///
/// «2» prima di «10», «CD 9» prima di «CD 10». Una sequenza di cifre si
/// confronta per valore — prima per quante cifre restano senza gli zeri in
/// testa, poi cifra per cifra, così nessun nome troppo lungo per un `u64`
/// trabocca — e tutto il resto carattere per carattere. A parità di lettura
/// («01» e «1») decide il confronto dei byte: due nomi diversi non risultano
/// mai uguali, e l'ordine resta lo stesso a ogni costruzione.
///
/// Le chiavi che arrivano qui sono già passate da [`path_key`]: le maiuscole le
/// ha piegate lei, dove il filesystem non le distingue.
#[must_use]
pub fn confronta_naturale(a: &str, b: &str) -> Ordering {
    let mut x = a.chars().peekable();
    let mut y = b.chars().peekable();
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let numero_x = cifre(&mut x);
                let numero_y = cifre(&mut y);
                let senza_x = numero_x.trim_start_matches('0');
                let senza_y = numero_y.trim_start_matches('0');
                let ordine = senza_x
                    .len()
                    .cmp(&senza_y.len())
                    .then_with(|| senza_x.cmp(senza_y));
                if ordine != Ordering::Equal {
                    return ordine;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Consuma le cifre ASCII consecutive e le restituisce.
fn cifre(caratteri: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut numero = String::new();
    while let Some(cifra) = caratteri.next_if(char::is_ascii_digit) {
        numero.push(cifra);
    }
    numero
}

/// Le righe dei file per identificativo, a pezzi, nell'ordine chiesto.
///
/// L'ordine chiesto è quello dell'album; una `IN (…)` restituisce le righe come
/// fa comodo a SQLite, quindi si riordina con una mappa. Un identificativo che
/// non c'è più — il brano tolto fra la costruzione del trie e questa lettura —
/// semplicemente non compare.
fn righe_per_id(connection: &Connection, ids: &[i64]) -> Result<Vec<FileCartella>, AppError> {
    let guasto = |err: &rusqlite::Error| crate::db::codice_da_sqlite("i file di una cartella", err);
    let mut trovate: HashMap<i64, FileCartella> = HashMap::with_capacity(ids.len());
    for pezzo in ids.chunks(PEZZO_LETTURA) {
        let segnaposto = std::iter::repeat_n("?", pezzo.len())
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = connection
            .prepare(&format!(
                "SELECT id, path, title, duration_ms FROM tracks
                 WHERE path IS NOT NULL AND id IN ({segnaposto})"
            ))
            .map_err(|err| guasto(&err))?;
        let mappate = statement
            .query_map(rusqlite::params_from_iter(pezzo), riga_file)
            .map_err(|err| guasto(&err))?;
        for riga in mappate {
            let riga = riga.map_err(|err| guasto(&err))?;
            trovate.insert(riga.id, riga.in_foglia());
        }
    }
    Ok(ids.iter().filter_map(|id| trovate.remove(id)).collect())
}

/// Tutti i file della libreria, per il filtro e per le cartelle oltre la soglia.
///
/// Quattro colonne, come [`leggi_righe`], e per la stessa ragione.
fn leggi_file(connection: &Connection) -> Result<Vec<RigaFile>, AppError> {
    let guasto = |err: &rusqlite::Error| crate::db::codice_da_sqlite("i file delle cartelle", err);
    let mut statement = connection
        .prepare("SELECT id, path, title, duration_ms FROM tracks WHERE path IS NOT NULL")
        .map_err(|err| guasto(&err))?;
    let mappate = statement
        .query_map([], riga_file)
        .map_err(|err| guasto(&err))?;
    let mut righe = Vec::new();
    for riga in mappate {
        righe.push(riga.map_err(|err| guasto(&err))?);
    }
    Ok(righe)
}

/// Una riga di `SELECT id, path, title, duration_ms`.
fn riga_file(riga: &rusqlite::Row<'_>) -> rusqlite::Result<RigaFile> {
    Ok(RigaFile {
        id: riga.get(0)?,
        percorso: riga.get(1)?,
        titolo: riga.get(2)?,
        durata_ms: riga.get(3)?,
    })
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
        // `WHERE path IS NOT NULL`: un brano di catalogo non sta in nessuna
        // cartella, e metterlo in questo albero vorrebbe dire inventargli una
        // cartella di nome «https:» — che è quel che si vedrebbe, perché è
        // questa query a decidere cosa appare nella schermata Cartelle.
        .prepare("SELECT id, path, disc_number, track_number FROM tracks WHERE path IS NOT NULL")
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
        diretti: 0,
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

    /// Il piano di una domanda sui percorsi, riga per riga.
    fn piano(connection: &Connection, sql: &str) -> Vec<String> {
        let mut s = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("piano");
        let righe = s.query_map([], |r| r.get::<_, String>(3)).expect("piano");
        righe.collect::<Result<_, _>>().expect("righe del piano")
    }

    #[test]
    fn il_like_sul_percorso_usa_l_indice_piegato() {
        // Il `//!` in testa al modulo racconta perché il pannello non chiede
        // `LIKE 'radice%'` al database: questa prova tiene quel racconto
        // ancorato al SQLite incluso. Dalla 030 il `LIKE` su `path` è una
        // ricerca per intervallo sull'indice piegato, e il trie resta per
        // un'altra ragione (le grafie dei separatori). Se il piano cambia,
        // cambia anche il commento.
        let connection = banco();
        let like = piano(
            &connection,
            r"SELECT id FROM tracks WHERE path LIKE 'C:\Musica\%'",
        );
        assert!(
            like.iter()
                .any(|r| r.starts_with("SEARCH") && r.contains("idx_tracks_percorso_piegato")),
            "{like:?}"
        );
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

    /// Una riga con un titolo suo, per le prove delle foglie e del filtro.
    fn brano_titolato(connection: &Connection, id: i64, percorso: &str, titolo: &str) {
        connection
            .execute(
                "INSERT INTO tracks
                     (id, path, track_key, title, artist, album, duration_ms,
                      file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, ?4, 'A', 'Al', 241000, 99999, 0, 0)",
                rusqlite::params![id, percorso, format!("a|t{id}|al"), titolo],
            )
            .expect("riga di brano");
    }

    fn brani_delle_foglie(foglie: &[FileCartella]) -> Vec<i64> {
        let mut brani: Vec<i64> = foglie.iter().map(|f| f.brano).collect();
        brani.sort_unstable();
        brani
    }

    /// Il ramo filtrato di una cartella, o `None` se il filtro la lascia chiusa.
    fn ramo<'a>(filtro: &'a CartelleFiltrate, percorso: Option<&str>) -> Option<&'a RamoFiltrato> {
        filtro
            .rami
            .iter()
            .find(|r| r.percorso.as_deref() == percorso)
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
    fn i_brani_d_immagine_stanno_col_loro_cue_per_foglio_e_numero() {
        let c = banco();
        // Due fogli nella stessa cartella, numerati tutti e due da 1: per
        // numero di traccia si mescolerebbero. E uno con un numero di disco,
        // che per un brano d'immagine non conta.
        riga(&c, 1, r"C:\M\Al\CD2.cue|01", None, Some(1));
        riga(&c, 2, r"C:\M\Al\CD1.cue|02", None, Some(2));
        riga(&c, 3, r"C:\M\Al\CD1.cue|10", None, Some(10));
        riga(&c, 4, r"C:\M\Al\CD1.cue|01", Some(2), Some(1));
        riga(&c, 5, r"C:\M\Al\CD2.cue|02", None, Some(2));
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        // Nella cartella del `.cue`, e il `.cue` non diventa una cartella.
        assert_eq!(nomi(&albero.figlie(Some(r"C:\M"))), vec!["Al"]);
        assert!(albero.figlie(Some(r"C:\M\Al")).is_empty());
        assert_eq!(albero.quanti_diretti(r"C:\M\Al"), 5);
        // L'ordine della finestra: foglio, poi numero.
        assert_eq!(albero.brani_diretti(r"C:\M\Al"), Some(vec![4, 2, 3, 1, 5]));
        assert_eq!(
            brani_diretti_dal_database(&c, r"C:\M\Al", WIN).expect("dal database"),
            vec![4, 2, 3, 1, 5]
        );
    }

    #[test]
    fn una_foglia_d_immagine_si_chiama_col_suo_titolo() {
        let c = banco();
        brano_titolato(&c, 1, r"C:\M\Al\Album.cue|01", "Overture");
        brano_titolato(&c, 2, r"C:\M\Al\Album.cue|02", "");
        brano_titolato(&c, 3, r"C:\M\Al\album bonus.mp3", "Bonus");
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        let foglie = file_della_cartella(&c, &albero, r"C:\M\Al").expect("foglie");
        let nome_di = |brano: i64| {
            foglie
                .iter()
                .find(|f| f.brano == brano)
                .map(|f| f.nome.clone())
        };
        assert_eq!(nome_di(1).as_deref(), Some("Overture"));
        // Senza titolo resta il percorso virtuale: la finestra lo riconosce e
        // scrive «Traccia 02 · Album».
        assert_eq!(nome_di(2).as_deref(), Some("Album.cue|02"));
        assert_eq!(nome_di(3).as_deref(), Some("album bonus.mp3"));

        // Il filtro guarda quel che la riga scrive: «album» non accende la
        // traccia che si chiama «Overture» solo perché il suo foglio si chiama
        // così.
        let filtro = filtra(&c, &albero, "album", LIMITE_FILTRO).expect("filtro");
        let al = ramo(&filtro, Some(r"C:\M\Al")).expect("Al è aperta");
        assert_eq!(brani_delle_foglie(&al.file), vec![2, 3]);
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

    #[test]
    fn le_foglie_sono_i_soli_file_diretti() {
        let c = banco();
        brano_titolato(&c, 1, r"C:\M\Al\01 - Uno.mp3", "Uno");
        brano_titolato(&c, 2, r"C:\M\Al\02 - Due.mp3", "Due");
        brano_titolato(&c, 3, r"C:\M\Al\CD2\01 - Tre.mp3", "Tre");
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");

        // Il nodo dice quante foglie ha prima che qualcuno le chieda: è quel
        // che fa disegnare la freccia su una cartella senza sottocartelle.
        let al = albero
            .figlie(Some(r"C:\M"))
            .into_iter()
            .next()
            .expect("la cartella Al");
        assert_eq!((al.brani, al.sottocartelle, al.file), (3, 1, 2));

        let foglie = file_della_cartella(&c, &albero, r"C:\M\Al").expect("foglie");
        assert_eq!(brani_delle_foglie(&foglie), vec![1, 2], "CD2 non entra");
        let prima = foglie
            .iter()
            .find(|f| f.brano == 1)
            .expect("la prima foglia");
        assert_eq!(prima.nome, "01 - Uno.mp3");
        assert_eq!(prima.titolo, "Uno");
        assert_eq!(prima.durata_ms, 241_000);
        assert_eq!(prima.percorso, r"C:\M\Al\01 - Uno.mp3");

        // Una cartella di passaggio non ha foglie, e non si legge niente.
        assert!(
            file_della_cartella(&c, &albero, r"C:\M")
                .expect("foglie")
                .is_empty()
        );
        assert_eq!(albero.quanti_diretti(r"C:\M"), 0);
    }

    #[test]
    fn i_brani_diretti_non_scendono() {
        let c = banco();
        riga(&c, 1, r"C:\M\Al\a.mp3", Some(1), Some(2));
        riga(&c, 2, r"C:\M\Al\b.mp3", Some(1), Some(1));
        riga(&c, 3, r"C:\M\Al\Bonus\c.mp3", Some(1), Some(1));
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        // Con le sottocartelle e senza: l'interruttore del pannello dei brani.
        assert_eq!(albero.brani_sotto(r"C:\M\Al"), Some(vec![2, 1, 3]));
        assert_eq!(albero.brani_diretti(r"C:\M\Al"), Some(vec![2, 1]));
        // E la rilettura dal database dice lo stesso, nello stesso ordine.
        assert_eq!(
            brani_diretti_dal_database(&c, r"C:\M\Al", WIN).expect("rilettura"),
            vec![2, 1]
        );
        assert_eq!(albero.brani_diretti(r"Q:\niente"), Some(Vec::new()));
    }

    #[test]
    fn sopra_la_soglia_le_foglie_si_rileggono() {
        let c = banco();
        brano_titolato(&c, 1, r"C:\M\Al\a.mp3", "A");
        brano_titolato(&c, 2, r"C:\M\Al\b.mp3", "B");
        brano_titolato(&c, 3, r"C:\M\Al\Sotto\c.mp3", "C");
        brano_titolato(&c, 4, r"C:\M\Altro\d.mp3", "D");
        let senza = Albero::da_righe(
            leggi_righe(&c).expect("righe"),
            &[r"C:\M".to_owned()],
            WIN,
            false,
        );
        assert_eq!(senza.brani_diretti(r"C:\M\Al"), None, "gli id non ci sono");
        // Le foglie ci sono lo stesso: si rilegge la tabella, e restano le
        // sole righe di quella cartella — non la sottocartella, non la sorella
        // che comincia allo stesso modo.
        let foglie = file_della_cartella(&c, &senza, r"C:\M\Al").expect("foglie");
        assert_eq!(brani_delle_foglie(&foglie), vec![1, 2]);
    }

    #[test]
    fn le_cartelle_sorelle_in_ordine_naturale() {
        let c = banco();
        riga(&c, 1, r"C:\M\CD 10\a.mp3", None, None);
        riga(&c, 2, r"C:\M\CD 2\a.mp3", None, None);
        riga(&c, 3, r"C:\M\CD 1\a.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert_eq!(
            nomi(&albero.figlie(Some(r"C:\M"))),
            vec!["CD 1", "CD 2", "CD 10"]
        );
        // E la cartella suona nell'ordine in cui la si vede.
        assert_eq!(albero.brani_sotto(r"C:\M"), Some(vec![3, 2, 1]));
    }

    #[test]
    fn senza_numeri_di_traccia_i_nomi_si_leggono_come_numeri() {
        let c = banco();
        riga(&c, 1, r"C:\M\Al\10.mp3", None, None);
        riga(&c, 2, r"C:\M\Al\2.mp3", None, None);
        riga(&c, 3, r"C:\M\Al\1.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert_eq!(albero.brani_sotto(r"C:\M\Al"), Some(vec![3, 2, 1]));
    }

    #[test]
    fn il_confronto_naturale_legge_le_cifre_come_numeri() {
        use std::cmp::Ordering::{Greater, Less};
        assert_eq!(confronta_naturale("2", "10"), Less);
        assert_eq!(confronta_naturale("cd 9", "cd 10"), Less);
        assert_eq!(confronta_naturale("track10b", "track10a"), Greater);
        assert_eq!(confronta_naturale("a", "a1"), Less);
        // Gli zeri in testa non contano per il valore, ma due nomi diversi non
        // sono mai uguali: decide il confronto dei byte.
        assert_eq!(confronta_naturale("007", "7"), Less);
        assert_ne!(confronta_naturale("01", "1"), std::cmp::Ordering::Equal);
        // Un numero più lungo di un `u64` non trabocca: si confronta per
        // lunghezza prima che per cifre.
        assert_eq!(
            confronta_naturale("x99999999999999999999999", "x100000000000000000000000"),
            Less
        );
    }

    #[test]
    fn il_filtro_apre_gli_antenati_e_pota_le_sorelle() {
        let c = banco();
        brano_titolato(&c, 1, r"C:\M\Rock\Queen\Bohemian.mp3", "Bohemian Rhapsody");
        brano_titolato(&c, 2, r"C:\M\Rock\Altri\x.mp3", "X");
        brano_titolato(&c, 3, r"C:\M\Jazz\y.mp3", "Y");
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");

        let filtro = filtra(&c, &albero, "QUEEN", LIMITE_FILTRO).expect("filtro");
        assert_eq!(filtro.trovate, vec![r"C:\M\Rock\Queen"]);
        assert_eq!(filtro.corrispondenze, 1);
        assert!(!filtro.troncato);
        assert_eq!(
            nomi(&ramo(&filtro, None).expect("le cime").cartelle),
            vec![r"C:\M"]
        );
        // La strada è aperta, e lungo la strada ci sono solo i passi che portano
        // alla corrispondenza: niente Jazz, niente Altri.
        assert_eq!(
            nomi(&ramo(&filtro, Some(r"C:\M")).expect("la radice").cartelle),
            vec!["Rock"]
        );
        assert_eq!(
            nomi(&ramo(&filtro, Some(r"C:\M\Rock")).expect("Rock").cartelle),
            vec!["Queen"]
        );
        // La cartella trovata resta chiusa: aprendola la si vede intera.
        assert!(ramo(&filtro, Some(r"C:\M\Rock\Queen")).is_none());
    }

    #[test]
    fn il_filtro_trova_i_file_per_nome_e_per_titolo_senza_accenti() {
        let c = banco();
        brano_titolato(&c, 1, r"C:\M\A\cafe.mp3", "Brano");
        brano_titolato(&c, 2, r"C:\M\B\x.mp3", "Café del Mar");
        brano_titolato(&c, 3, r"C:\M\B\y.mp3", "Altro");
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");

        let filtro = filtra(&c, &albero, "  CAFÉ ", LIMITE_FILTRO).expect("filtro");
        assert!(filtro.trovate.is_empty(), "nessuna cartella si chiama così");
        assert_eq!(filtro.corrispondenze, 2);
        assert_eq!(
            nomi(&ramo(&filtro, Some(r"C:\M")).expect("la radice").cartelle),
            vec!["A", "B"]
        );
        let a = ramo(&filtro, Some(r"C:\M\A")).expect("A è aperta");
        assert_eq!(brani_delle_foglie(&a.file), vec![1], "per nome del file");
        let b = ramo(&filtro, Some(r"C:\M\B")).expect("B è aperta");
        assert_eq!(brani_delle_foglie(&b.file), vec![2], "per titolo, e y no");
    }

    #[test]
    fn il_filtro_si_ferma_al_limite_e_lo_dice() {
        let c = banco();
        for id in 1..=5 {
            brano_titolato(&c, id, &format!(r"C:\M\Al\pezzo {id}.mp3"), "T");
        }
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        let filtro = filtra(&c, &albero, "pezzo", 3).expect("filtro");
        assert!(filtro.troncato);
        assert_eq!(filtro.corrispondenze, 5);
        let foglie: usize = filtro.rami.iter().map(|r| r.file.len()).sum();
        assert_eq!(foglie, 3);
    }

    #[test]
    fn un_filtro_vuoto_non_trova_niente() {
        let c = banco();
        riga(&c, 1, r"C:\M\a.mp3", None, None);
        let albero = Albero::costruisci(&c, &[r"C:\M".to_owned()], WIN).expect("albero");
        assert_eq!(
            filtra(&c, &albero, "   ", LIMITE_FILTRO).expect("filtro"),
            CartelleFiltrate::default()
        );
    }

    #[test]
    fn sotto_una_radice_vale_solo_dentro_le_cartelle_sorvegliate() {
        let radici = vec![r"C:\M".to_owned(), r"\\srv\musica".to_owned()];
        assert!(sotto_una_radice(r"C:\M\Rock\Queen", &radici, WIN));
        assert!(sotto_una_radice(r"C:\M", &radici, WIN), "la radice stessa");
        assert!(
            sotto_una_radice(r"c:/m/rock", &radici, WIN),
            "un'altra grafia"
        );
        assert!(sotto_una_radice(r"\\srv\musica\Jazz", &radici, WIN));
        // Il confine di separatore: `C:\Musica` comincia come `C:\M`, e non ci
        // sta dentro.
        assert!(!sotto_una_radice(r"C:\Musica\Rock", &radici, WIN));
        assert!(!sotto_una_radice(r"D:\altro", &radici, WIN));
        assert!(!sotto_una_radice(r"\\srv\altro", &radici, WIN));
    }

    #[test]
    fn sotto_una_radice_non_si_fa_aggirare_dai_punti() {
        let radici = vec![r"C:\M".to_owned()];
        // Lessicalmente «dentro», in realtà fuori: è il percorso per cui la
        // guardia esiste.
        assert!(!sotto_una_radice(r"C:\M\..\Windows", &radici, WIN));
        assert!(!sotto_una_radice(r"C:/M/Rock/../../Windows", &radici, WIN));
        assert!(!sotto_una_radice(r"C:\M\.\Rock", &radici, WIN));
        assert!(!sotto_una_radice("", &radici, WIN));
        assert!(!sotto_una_radice("C:\\M\\a\0b", &radici, WIN));
        // E senza radici non c'è niente da mostrare.
        assert!(!sotto_una_radice(r"C:\M\Rock", &[], WIN));
        assert!(!sotto_una_radice(r"C:\M\Rock", &[String::new()], WIN));
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
