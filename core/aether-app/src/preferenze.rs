//! Le preferenze della finestra che stavano fuori dal nucleo.
//!
//! # Il tema, e perché torna dentro
//!
//! `aether.tema` viveva in `localStorage` (`apps/desktop/src/tema.ts`), e la
//! ragione era buona quando è stato scritto: la scelta fra chiaro e scuro non è
//! un dato della libreria, è una preferenza della finestra, e il nucleo non
//! aveva niente da decidere.
//!
//! Quel che è cambiato è che adesso esistono due cose che leggono *tutte* le
//! preferenze: il **backup su Drive** e il **profilo**. Una preferenza in
//! `localStorage` non finisce in nessuno dei due — quindi chi ripristina un
//! backup si ritrova la skin giusta e il tema sbagliato, e chi porta il proprio
//! profilo su un altro computer si porta tutto tranne quello. Non è un difetto
//! di `localStorage`: è che il confine era stato tracciato prima che esistesse
//! qualcosa che lo attraversa.
//!
//! Il ripiego resta: la prima apertura dopo l'aggiornamento non trova la chiave
//! e legge quella vecchia. È il motivo per cui [`tema`] restituisce un
//! `Option` — «mai scelto» e «scelto sistema» sono due stati diversi, e
//! confonderli vorrebbe dire buttare via la scelta di chi aveva fissato il tema
//! chiaro.
//!
//! # Le scorciatoie, e perché il nucleo non le capisce
//!
//! Si conservano come JSON **opaco**: qui si controlla che sia JSON valido e
//! nient'altro. Non è pigrizia — è che i nomi dei comandi (`alterna`, `cerca`,
//! `inRiproduzione`) appartengono alla finestra, e un nucleo che li validasse
//! andrebbe ricompilato per aggiungere una scorciatoia. La stessa disciplina di
//! `player.eq.presets`, che è un elenco di curve di cui il database non ha
//! nessuna opinione.
//!
//! Quel che invece si controlla è che il testo sia JSON: una stringa storta
//! scritta qui dentro non darebbe nessun sintomo finché la finestra non prova a
//! leggerla, cioè al riavvio successivo — e a quel punto tutte le scorciatoie
//! sparirebbero insieme, senza che niente colleghi le due cose.
//!
//! # La lingua, e perché non è un `enum`
//!
//! Stessa disciplina delle scorciatoie, per la stessa ragione. Le lingue
//! disponibili sono i file dentro `apps/desktop/src/lingue/`, e l'intero
//! impianto esiste perché aggiungerne una costi **un file solo**: un `enum` qui
//! vorrebbe dire ricompilare il nucleo per far comparire il tedesco. Si conserva
//! quindi il codice ISO com'è, controllando soltanto che *sia* un codice di
//! lingua — vedi [`imposta_lingua`].
//!
//! # Le cartelle aperte, e perché **non** escono da questa macchina
//!
//! Il pannello «Cartelle» ricorda quali nodi erano aperti e su quale stava il
//! fuoco. Sono preferenze come le altre e stanno in `settings` come le altre,
//! ma con una differenza che va detta qui, perché è il primo posto in cui
//! qualcuno la cercherà: **non entrano nel profilo**, e la ragione è la stessa
//! per cui non ci entra `player.output`. Un percorso è un fatto di *questo*
//! computer. Riaprirlo su un altro vorrebbe dire, nel caso buono, un albero
//! che riapre nodi che non esistono; nel caso normale, un pannello che al primo
//! disegno chiede al nucleo una dozzina di espansioni tutte vuote e poi si
//! presenta chiuso — cioè peggio che se non avesse ricordato niente.
//!
//! Il tetto di [`MASSIMO_CARTELLE_APERTE`] non è prudenza generica. L'elenco si
//! riscrive intero a ogni apertura, e chiudere un nodo nel pannello **non**
//! butta quel che si sa di lui: è il patto dichiarato lassù — riaprire dev'essere
//! immediato — e vale anche per quel che si ricorda fra due avvii. Senza un
//! tetto, quindi, questa riga cresce e non cala mai.
//!
//! # Lo zoom, e perché **non** esce da questa macchina
//!
//! `Ctrl++` ingrandisce l'interfaccia e `Ctrl+-` la rimpicciolisce; il gradino
//! scelto si ricorda, e sta qui per la ragione del preambolo — una riga in
//! `localStorage` non finirebbe né nel backup su Drive né nel profilo.
//!
//! Nel profilo però **non** ci va, ed è la differenza che va detta qui perché
//! è qui che qualcuno la cercherà. Non è il caso di
//! [`CHIAVE_MOVIMENTO_RIDOTTO`], che viaggia e deve viaggiare: «meno
//! movimento» vuol dire la stessa cosa dappertutto, mentre «tutto una volta e
//! mezza» vuol dire una cosa sola su *questo* monitor, a *questa* risoluzione,
//! con *questa* scalatura di Windows già applicata sotto. Lo zoom di Aether è
//! la correzione che resta **dopo** quella del sistema operativo, e il resto di
//! un conto fatto su un altro schermo non è un gusto di chi ascolta: è la
//! famiglia di `player.output` e di `player.spectrum.quality`.
//!
//! E il danno sarebbe di quelli che si vedono, non di quelli muti: chi arriva
//! su un portatile con 1,75 in tabella se ne accorge al primo fotogramma e lo
//! toglie con un tasto. È il motivo per cui l'omissione qui pesa meno di
//! quella della qualità dello spettro, non il motivo per cui non sarebbe
//! un'omissione.
//!
//! # Il giro guidato, e perché si ricorda una **versione**
//!
//! [`CHIAVE_GIRO_VISTO`] non dice «il giro è stato fatto»: dice **quale** giro
//! è stato fatto. Un booleano avrebbe risposto alla domanda di oggi e a
//! nessun'altra: la release che aggiunge tre schermate nuove vorrebbe poterle
//! mostrare a chi il giro l'ha già visto, e con un `true` in tabella l'unico
//! modo sarebbe cancellare la riga a tutti — cioè rifare il giro intero anche a
//! chi non ha niente da imparare. Con una versione, chi decide è il copione:
//! confronta quel che c'è scritto con la versione che dichiara, e se è la
//! stessa tace.
//!
//! Il testo non si interpreta qui, e non è pigrizia: il numero di versione che
//! ci finisce è quello del **copione**, che vive nella finestra
//! (`apps/desktop/src-tauri/src/giro.rs`) perché è là che si sa se il giro ha
//! qualcosa di nuovo da dire. Un `semver` confrontato dal nucleo direbbe
//! «maggiore di», che è la domanda sbagliata: un copione riscritto più corto
//! non è una versione più bassa, è un copione diverso.
//!
//! # Perché **non** entra nel profilo
//!
//! Perché il profilo porta *gusti* — il tema, le skin, le playlist, cosa piace
//! — e «cose già viste» non è un gusto: è la storia di questa installazione.
//! Portarla su un altro computer vorrebbe dire arrivare su una finestra mai
//! aperta con dentro la memoria di averla già guardata, cioè togliere il giro
//! proprio a chi ne ha più bisogno. È la stessa ragione per cui nel catalogo
//! del profilo non c'è `aggiornamenti.saltata`, e quell'assenza è giusta.

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::Connection;

use crate::settings;

/// Chiaro, scuro, o quel che dice il sistema.
pub const CHIAVE_TEMA: &str = "ui.theme";

/// Le associazioni fra tasti e comandi, come JSON.
pub const CHIAVE_SCORCIATOIE: &str = "ui.shortcuts";

/// La lingua dell'interfaccia, come codice ISO: `it`, `en`, `de`.
pub const CHIAVE_LINGUA: &str = "ui.language";

/// Se la X chiude davvero, o manda in secondo piano.
pub const CHIAVE_SECONDO_PIANO: &str = "ui.closeToTray";

/// Se chi guarda ha chiesto meno movimento di quanto la skin ne dichiari.
pub const CHIAVE_MOVIMENTO_RIDOTTO: &str = "ui.reducedMotion";

/// Quanto si ingrandisce l'interfaccia, come fattore sulla misura di serie.
///
/// Vedi il preambolo: si ricorda come le altre, ma non entra nel profilo.
pub const CHIAVE_ZOOM: &str = "ui.zoom";

/// Nessun ingrandimento: la misura che il foglio dichiara.
///
/// Sta in [`SCALA_ZOOM`], e ci deve stare — è il gradino da cui si parte e
/// quello a cui `Ctrl+0` riporta. La prova `il_neutro_e_un_gradino` lo tiene
/// vero il giorno in cui qualcuno cambia la scala.
pub const ZOOM_NEUTRO: f64 = 1.0;

/// I gradini dello zoom, in ordine crescente.
///
/// # Perché una scala di valori fissi e non un fattore continuo
///
/// Perché moltiplicare per 1,1 a ogni pressione dà numeri che nessuno
/// riconosce — 1,331 — e soprattutto non torna mai esattamente a 1: chi ha
/// premuto tre volte in su e tre in giù si ritrova un'interfaccia che *quasi*
/// è quella di prima, e non ha nessun modo di accorgersi di quanto le manca.
/// Con una scala, il ritorno è un gradino con un nome.
///
/// Sono i sette che ogni browser offre fra il 90% e il 200%, e non è pigrizia:
/// chi preme `Ctrl++` qui dentro l'ha già premuto altrove, e trovare gli stessi
/// numeri vuol dire sapere già quante pressioni servono. I passi crescono
/// invece di essere uguali perché lo fa anche la percezione: un dieci per cento
/// in più a 2,0 è un cambio che non si vede, e chiederebbe sei pressioni per
/// arrivare dove adesso ne bastano due.
///
/// # Perché i due estremi non sono simmetrici
///
/// Perché le due direzioni non chiedono la stessa cosa.
///
/// **In giù** si chiede «fammene stare di più», e il pavimento è duro: il
/// testo più piccolo che nel foglio si legge davvero è 10,5 px — le
/// spiegazioni, i contatori, le righe secondarie, trentasei dichiarazioni — e
/// a 0,9 diventa 9,45. Un gradino più in giù lo porterebbe a 8,4, e
/// un'interfaccia illeggibile non la può rimettere a posto chi la guarda,
/// perché per rimetterla a posto bisogna leggerla. Un gradino in giù, e basta.
///
/// **In su** si chiede «non ci vedo», e il soffitto è morbido:
/// l'impaginazione si stringe, il cursore del volume si ritira da sé
/// (`@container lettore (max-width: 760px)` in `stile.css`), e più su ancora
/// la barra del lettore trabocca dai propri minimi di contenuto. Sono difetti
/// che si vedono, e che `Ctrl+0` annulla in un tasto. Per questo in su i
/// gradini sono cinque.
///
/// # Il conto che rimane scoperto, e va detto invece che nascosto
///
/// Ingrandire riduce lo spazio in pixel CSS esattamente come stringere la
/// finestra: a fattore *z* la pagina vede `larghezza / z`. La finestra non
/// scende sotto 880 (`minWidth` in `tauri.conf.json`), quindi già a 1,25 la
/// pagina disegna in 704 pixel CSS un'impaginazione che nessuna finestra ha mai
/// potuto mostrare. Il primo a soffrirne è il lettore, che ha una sola
/// tappa di ritiro. Alzare il minimo della finestra insieme allo zoom
/// sarebbe la cura ovvia ed è peggio del male: a 1,75 il minimo diventerebbe
/// 1540 pixel logici, cioè più largo dell'area di lavoro di metà dei portatili,
/// e la finestra crescerebbe fuori dallo schermo. La cura vera è una seconda
/// tappa nel foglio, che appartiene a chi possiede il foglio.
pub const SCALA_ZOOM: &[f64] = &[0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// Quanto un valore può discostarsi da un gradino ed essere ancora quello.
///
/// Serve solo a non confrontare due `f64` con `==`. I gradini si scrivono e si
/// rileggono attraverso JSON, che li riporta identici, e questa è la rete per
/// il giorno in cui non fosse più vero; è cinque ordini di grandezza sotto la
/// distanza minima fra due gradini, che è 0,1, quindi non può far scambiare un
/// gradino per il suo vicino.
const TOLLERANZA_ZOOM: f64 = 1e-6;

/// I nodi lasciati aperti nel pannello «Cartelle», come JSON.
///
/// Percorsi di questa macchina: vedi il preambolo sul perché non entrano nel
/// profilo.
pub const CHIAVE_CARTELLE_APERTE: &str = "ui.folders.open";

/// Il nodo su cui stava il fuoco nel pannello «Cartelle».
///
/// Uno solo, e non una selezione: il pannello è una navigazione, e quel che si
/// riprende riaprendolo è «dov'ero», non «cosa avevo scelto».
pub const CHIAVE_CARTELLA_SCELTA: &str = "ui.folders.selected";

/// Quale giro guidato è già stato visto, come versione del copione.
///
/// Vedi il preambolo: una versione e non un booleano, e fuori dal profilo.
pub const CHIAVE_GIRO_VISTO: &str = "ui.tour.seenVersion";

/// Quanti percorsi aperti si conservano.
///
/// Duecento sono molti più dei nodi che stanno su uno schermo e molti meno di
/// quelli che una sessione lunga apre: è il numero che rende il ricordo utile
/// senza farne una lista che cresce per sempre.
pub const MASSIMO_CARTELLE_APERTE: usize = 200;

/// Le tre scelte del tema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tema {
    /// Sempre scuro.
    Scuro,
    /// Sempre chiaro.
    Chiaro,
    /// Quel che dice il sistema operativo.
    Sistema,
}

impl Tema {
    /// Il nome stabile, quello che finisce nel database.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Scuro => "scuro",
            Self::Chiaro => "chiaro",
            Self::Sistema => "sistema",
        }
    }

    /// Dal nome stabile.
    ///
    /// `None` per qualunque altra cosa: una riga scritta a mano con `sqlite3` o
    /// arrivata da un profilo di una versione futura non deve poter far
    /// comparire un tema che non esiste.
    #[must_use]
    pub fn dal_nome(nome: &str) -> Option<Self> {
        match nome {
            "scuro" => Some(Self::Scuro),
            "chiaro" => Some(Self::Chiaro),
            "sistema" => Some(Self::Sistema),
            _ => None,
        }
    }
}

/// Il tema scelto, o `None` se non è mai stato scelto.
///
/// La distinzione conta: chi non ha mai scelto va servito con la preferenza che
/// era rimasta in `localStorage`, chi ha scelto «sistema» no.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn tema(connection: &Connection) -> Result<Option<Tema>, AppError> {
    Ok(settings::read(connection, CHIAVE_TEMA)?.and_then(|nome| Tema::dal_nome(&nome)))
}

/// Scrive il tema.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_tema(connection: &Connection, tema: Tema) -> Result<(), AppError> {
    settings::write(connection, CHIAVE_TEMA, tema.nome())
}

/// Un codice di lingua plausibile: `it`, `en`, `pt-BR`.
///
/// # Perché qui non c'è un `enum`
///
/// Il tema ha tre valori e li conosce il nucleo; le lingue no. L'elenco delle
/// lingue disponibili è il **contenuto di una cartella** nella finestra
/// (`apps/desktop/src/lingue/`), e il punto di tutto l'impianto è che
/// aggiungerne una costi un file solo. Un `enum` qui vorrebbe dire ricompilare
/// il nucleo per aggiungere `de.json`, cioè esattamente la cosa che si sta
/// evitando — la stessa disciplina delle scorciatoie, che si conservano opache
/// perché i nomi dei comandi appartengono alla finestra.
///
/// Quel che resta da controllare non è quindi *quale* lingua, ma che il testo
/// **sia** un codice di lingua: senza, una riga qualunque scritta a mano
/// diventerebbe il nome di un file da cercare.
fn codice_valido(codice: &str) -> bool {
    let mut parti = codice.split('-');
    let Some(lingua) = parti.next() else {
        return false;
    };
    let lingua_ok =
        (2..=3).contains(&lingua.len()) && lingua.bytes().all(|b| b.is_ascii_lowercase());
    lingua_ok
        && parti.all(|parte| {
            (2..=8).contains(&parte.len()) && parte.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// La lingua scelta, o `None` se non è mai stata scelta.
///
/// La distinzione conta come per il tema, e per una ragione ancora più visibile:
/// «mai scelto» è ciò che fa rilevare la lingua dal sistema operativo. Se
/// diventasse una lingua qualsiasi, un'installazione tedesca partirebbe in
/// inglese per sempre senza che nessuno abbia deciso niente.
///
/// Un codice storto vale come mai scelto: una riga arrivata da un profilo di una
/// versione futura non deve poter far cercare un file che non esiste.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn lingua(connection: &Connection) -> Result<Option<String>, AppError> {
    Ok(settings::read(connection, CHIAVE_LINGUA)?.filter(|codice| codice_valido(codice)))
}

/// Scrive la lingua. Una stringa vuota rimette il rilevamento dal sistema.
///
/// Si **toglie** la riga invece di scrivere `""`, per la stessa ragione della
/// cartella dei download: una stringa vuota in tabella è un terzo stato oltre
/// «scelta» e «mai scelta», e ogni lettore dovrebbe ricordarsi di filtrarlo.
///
/// # Errori
///
/// `settings.corrupt` se il testo non è un codice di lingua. `db.queryFailed`
/// per il resto.
pub fn imposta_lingua(connection: &Connection, codice: &str) -> Result<(), AppError> {
    let codice = codice.trim();
    if codice.is_empty() {
        settings::forget(connection, CHIAVE_LINGUA)?;
        return Ok(());
    }
    if !codice_valido(codice) {
        return Err(AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!("«{codice}» non è un codice di lingua")));
    }
    settings::write(connection, CHIAVE_LINGUA, codice)
}

/// Le scorciatoie come sono state scritte, o `None` se sono quelle di serie.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn scorciatoie(connection: &Connection) -> Result<Option<String>, AppError> {
    settings::read(connection, CHIAVE_SCORCIATOIE)
}

/// Scrive le scorciatoie. Una stringa vuota rimette quelle di serie.
///
/// # Errori
///
/// `settings.corrupt` se il testo non è JSON — vedi il preambolo sul perché il
/// controllo è questo e non di più. `db.queryFailed` per il resto.
pub fn imposta_scorciatoie(connection: &Connection, json: &str) -> Result<(), AppError> {
    let json = json.trim();
    if json.is_empty() {
        // Cancellare invece di scrivere `""`: assente vuol dire «quelle di
        // serie», e una stringa vuota vorrebbe dire «nessuna scorciatoia».
        settings::forget(connection, CHIAVE_SCORCIATOIE)?;
        return Ok(());
    }
    serde_json::from_str::<serde_json::Value>(json).map_err(|err| {
        AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!("le scorciatoie non sono JSON: {err}"))
    })?;
    settings::write(connection, CHIAVE_SCORCIATOIE, json)
}

/// Se il tasto di chiusura manda in secondo piano invece di spegnere.
///
/// # Perché assente vuol dire `false`
///
/// Perché è la stessa ragione di `player.autoplay`, vista dall'altro verso: un
/// aggiornamento che accendesse questa da sé farebbe sparire nel vassoio un
/// programma che qualcuno credeva di aver chiuso, e la prova che è ancora
/// acceso arriverebbe dalla musica che non smette. Chi la vuole la accende.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn secondo_piano(connection: &Connection) -> Result<bool, AppError> {
    Ok(settings::read_json::<bool>(connection, CHIAVE_SECONDO_PIANO)?.unwrap_or(false))
}

/// Scrive se la X manda in secondo piano.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_secondo_piano(connection: &Connection, attivo: bool) -> Result<(), AppError> {
    settings::write_json(connection, CHIAVE_SECONDO_PIANO, &attivo)
}

/// Se chi guarda ha chiesto di ridurre il movimento.
///
/// # Perché è un booleano e non tre valori come il tema
///
/// Perché da qui si può soltanto **ridurre**. La skin dichiara la sua
/// `motion.intensity` e il sistema operativo dichiara il suo
/// `prefers-reduced-motion`; questa preferenza si compone con le altre due
/// abbassando, e un terzo livello che *alzasse* direbbe l'opposto della frase
/// che l'interfaccia mostra sopra il comando — una preferenza di accessibilità
/// che qualcosa può sovrascrivere non è una preferenza. Le due voci del
/// segmentato («come il sistema», «riduci tutto») sono quindi il vero e il
/// falso di questa riga, non due terzi di un `enum` a cui manca il terzo.
///
/// # Perché sta qui e non in `localStorage`
///
/// Per la ragione scritta nel preambolo, e a maggior ragione nella release che
/// promette un profilo con «tutto» dentro: una preferenza in `localStorage`
/// non finisce né nel backup su Drive né nel profilo. Il tema ci era finito e
/// ne è stato tolto proprio per questo, e ripetere l'errore su una preferenza
/// **di accessibilità** — cioè quella che più di tutte deve ritrovarsi
/// identica su un altro computer — sarebbe peggio che averlo fatto la prima
/// volta.
///
/// # Perché assente vuol dire «no»
///
/// Stessa forma di [`secondo_piano`], e stessa ragione vista dall'altro verso:
/// accenderla d'ufficio spegnerebbe il movimento a chi non l'ha chiesto,
/// facendogli credere che l'aggiornamento abbia rotto le animazioni. Chi ha
/// una condizione la dichiara al sistema operativo, e quella strada resta
/// aperta e vince comunque: questa riga serve a chi vuole meno movimento *in
/// Aether* senza cambiare tutto Windows.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn movimento_ridotto(connection: &Connection) -> Result<bool, AppError> {
    Ok(settings::read_json::<bool>(connection, CHIAVE_MOVIMENTO_RIDOTTO)?.unwrap_or(false))
}

/// Scrive se chi guarda ha chiesto meno movimento.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_movimento_ridotto(connection: &Connection, ridotto: bool) -> Result<(), AppError> {
    settings::write_json(connection, CHIAVE_MOVIMENTO_RIDOTTO, &ridotto)
}

/// Il fattore com'è, se è un gradino di [`SCALA_ZOOM`]; [`ZOOM_NEUTRO`] se no.
///
/// # Perché non si arrotonda al gradino più vicino
///
/// Perché un valore fuori scala non arriva mai da un gesto: i gesti scrivono
/// gradini, ed è [`imposta_zoom`] a garantirlo. Arriva da una riga scritta a
/// mano con `sqlite3`, o da una versione che aveva una scala diversa — e in
/// tutti e due i casi «il più vicino» è un'ipotesi su che cosa voleva
/// qualcun altro. [`ZOOM_NEUTRO`] non è un'ipotesi: si vede al primo
/// fotogramma, e da lì ogni gradino è a poche pressioni.
///
/// Non c'è nessun caso per `NaN` e per gli infiniti, e non è una dimenticanza:
/// il confronto qui sotto è falso per tutti e tre, quindi cadono nel ripiego da
/// sé. Aggiungere un `is_finite` scriverebbe due volte la stessa regola.
#[must_use]
pub fn zoom_valido(fattore: f64) -> f64 {
    SCALA_ZOOM
        .iter()
        .copied()
        .find(|gradino| (gradino - fattore).abs() < TOLLERANZA_ZOOM)
        .unwrap_or(ZOOM_NEUTRO)
}

/// Il gradino accanto, in su o in giù. Ai due estremi si resta dov'è.
///
/// Si resta invece di girare: una scala che dal 200% torna al 90% farebbe
/// rimpicciolire una pressione che chiedeva di ingrandire, e chi tiene premuto
/// il tasto per arrivare in cima si ritroverebbe in fondo senza capire perché.
/// La fine della scala è una cosa che si sente perché non succede più niente,
/// ed è la risposta giusta.
#[must_use]
pub fn zoom_al_gradino(fattore: f64, su: bool) -> f64 {
    let corrente = zoom_valido(fattore);
    // `zoom_valido` restituisce sempre un gradino, quindi la ricerca riesce
    // sempre: il ripiego a zero è lì perché `position` dà un `Option` e in
    // questo workspace `unwrap` è vietato, non perché il caso esista.
    let dove = SCALA_ZOOM
        .iter()
        .position(|gradino| (gradino - corrente).abs() < TOLLERANZA_ZOOM)
        .unwrap_or(0);
    let vicino = if su {
        Some(dove + 1)
    } else {
        dove.checked_sub(1)
    };
    vicino
        .and_then(|indice| SCALA_ZOOM.get(indice).copied())
        .unwrap_or(corrente)
}

/// Il gradino di zoom ricordato, [`ZOOM_NEUTRO`] se non ce n'è uno buono.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn zoom(connection: &Connection) -> Result<f64, AppError> {
    Ok(zoom_valido(
        settings::read_json::<f64>(connection, CHIAVE_ZOOM)?.unwrap_or(ZOOM_NEUTRO),
    ))
}

/// Scrive il gradino di zoom, e restituisce quello che ha davvero scritto.
///
/// Restituisce invece di tacere perché quel che si scrive non è sempre quel
/// che arriva: fuori scala si scrive [`ZOOM_NEUTRO`], e chi ha chiamato deve
/// applicare alla finestra **lo stesso** numero che sta in tabella. Due
/// letture successive che dessero due risposte diverse sarebbero un
/// disallineamento fra quel che si vede e quel che si ricorda, cioè il difetto
/// che si scopre solo al riavvio dopo.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_zoom(connection: &Connection, fattore: f64) -> Result<f64, AppError> {
    let valido = zoom_valido(fattore);
    settings::write_json(connection, CHIAVE_ZOOM, &valido)?;
    Ok(valido)
}

/// I nodi che il pannello «Cartelle» aveva aperti l'ultima volta.
///
/// Elenco vuoto quando non c'è niente da ricordare, e **anche** quando quel che
/// c'è non si interpreta: è la regola di [`settings::read_json`], e qui vale a
/// maggior ragione — un pannello che si apre tutto chiuso è il caso normale del
/// primo avvio, mentre un pannello che rifiuta di aprirsi perché una riga di
/// impostazione è storta non lascia all'utente nessun modo di correggerla.
///
/// I percorsi non si controllano contro il disco né contro la libreria. Non è
/// una dimenticanza: controllarli qui vorrebbe dire toccare SMB dentro una
/// lettura di preferenze, che è precisamente quel che il pannello esiste per
/// non fare. Un percorso che non c'è più semplicemente non compare nell'albero,
/// e la riga muore da sé alla prima riscrittura.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn cartelle_aperte(connection: &Connection) -> Result<Vec<String>, AppError> {
    Ok(settings::read_json::<Vec<String>>(connection, CHIAVE_CARTELLE_APERTE)?.unwrap_or_default())
}

/// Scrive i nodi aperti, tenendo i più recenti.
///
/// `aperte` arriva **dal meno recente al più recente**: è l'ordine in cui il
/// pannello li ha aperti, ed è l'unico che permette di sapere quali buttare
/// quando sono troppi. Oltre [`MASSIMO_CARTELLE_APERTE`] si taglia dalla testa.
///
/// Tagliare un antenato lasciando dentro un suo discendente è possibile e
/// innocuo: l'albero disegna i figli solo dei nodi aperti, quindi la voce
/// rimasta non si vede finché il padre non viene riaperto a mano — e allora
/// torna utile invece che sbagliata.
///
/// Un elenco vuoto **toglie** la riga invece di scrivere `[]`, come fa la
/// cartella dei download con la stringa vuota: «mai aperto niente» e «aperto
/// niente» sono lo stesso stato, e due modi di scriverlo sono due modi di
/// leggerlo.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_cartelle_aperte(connection: &Connection, aperte: &[String]) -> Result<(), AppError> {
    if aperte.is_empty() {
        settings::forget(connection, CHIAVE_CARTELLE_APERTE)?;
        return Ok(());
    }
    let da = aperte.len().saturating_sub(MASSIMO_CARTELLE_APERTE);
    let tenute = aperte.get(da..).unwrap_or(aperte);
    settings::write_json(connection, CHIAVE_CARTELLE_APERTE, &tenute)
}

/// Il nodo su cui stava il fuoco, o `None` se non ce n'era uno.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn cartella_scelta(connection: &Connection) -> Result<Option<String>, AppError> {
    Ok(settings::read(connection, CHIAVE_CARTELLA_SCELTA)?.filter(|dove| !dove.trim().is_empty()))
}

/// Scrive il nodo col fuoco. Una stringa vuota dimentica quello di prima.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_cartella_scelta(connection: &Connection, percorso: &str) -> Result<(), AppError> {
    if percorso.trim().is_empty() {
        settings::forget(connection, CHIAVE_CARTELLA_SCELTA)?;
        return Ok(());
    }
    settings::write(connection, CHIAVE_CARTELLA_SCELTA, percorso)
}

/// La versione del giro guidato già visto, o `None` se non se n'è visto nessuno.
///
/// Una stringa vuota vale come `None`, per la stessa disciplina di
/// [`cartella_scelta`]: se una riga vuota valesse «visto», basterebbe una
/// scrittura andata a metà per togliere il giro a chi non l'ha mai fatto — e
/// il sintomo sarebbe l'assenza di qualcosa, cioè niente da notare.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn giro_visto(connection: &Connection) -> Result<Option<String>, AppError> {
    Ok(settings::read(connection, CHIAVE_GIRO_VISTO)?.filter(|v| !v.trim().is_empty()))
}

/// Segna quale giro si è visto. Una stringa vuota dimentica.
///
/// Dimenticare è quel che fa «Rifai il giro» quando lo si vuole rivedere da
/// capo alla prossima apertura, e si **toglie** la riga invece di scrivere
/// `""`: «mai visto» e «visto niente» sono lo stesso stato, e due modi di
/// scriverlo sono due modi di leggerlo.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_giro_visto(connection: &Connection, versione: &str) -> Result<(), AppError> {
    let versione = versione.trim();
    if versione.is_empty() {
        settings::forget(connection, CHIAVE_GIRO_VISTO)?;
        return Ok(());
    }
    settings::write(connection, CHIAVE_GIRO_VISTO, versione)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn libreria() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    #[test]
    fn mai_scelto_non_e_sistema() {
        let c = libreria();
        assert_eq!(
            tema(&c),
            Ok(None),
            "senza questa distinzione la scelta rimasta in localStorage andrebbe persa"
        );
        imposta_tema(&c, Tema::Sistema).expect("scrittura");
        assert_eq!(tema(&c), Ok(Some(Tema::Sistema)));
    }

    #[test]
    fn i_tre_temi_vanno_e_tornano() {
        let c = libreria();
        for atteso in [Tema::Scuro, Tema::Chiaro, Tema::Sistema] {
            imposta_tema(&c, atteso).expect("scrittura");
            assert_eq!(tema(&c), Ok(Some(atteso)));
        }
    }

    #[test]
    fn un_tema_che_non_esiste_vale_come_mai_scelto() {
        let c = libreria();
        settings::write(&c, CHIAVE_TEMA, "seppia").expect("scrittura");
        assert_eq!(
            tema(&c),
            Ok(None),
            "una riga arrivata da una versione futura non deve inventare un tema"
        );
    }

    #[test]
    fn le_scorciatoie_devono_essere_json() {
        let c = libreria();
        let err = imposta_scorciatoie(&c, "alterna=Space").expect_err("non è JSON");
        assert_eq!(err.code().kind().code(), "settings.corrupt");
        assert_eq!(
            scorciatoie(&c),
            Ok(None),
            "e quel che non è JSON non viene scritto"
        );

        imposta_scorciatoie(&c, r#"{"alterna":[" "]}"#).expect("JSON valido");
        assert_eq!(scorciatoie(&c), Ok(Some(r#"{"alterna":[" "]}"#.to_owned())));
    }

    #[test]
    fn mai_scelta_e_cio_che_fa_rilevare_la_lingua() {
        let c = libreria();
        assert_eq!(
            lingua(&c),
            Ok(None),
            "senza «mai scelta» un'installazione tedesca partirebbe in inglese per sempre"
        );
        imposta_lingua(&c, "de").expect("scrittura");
        assert_eq!(lingua(&c), Ok(Some("de".to_owned())));
    }

    #[test]
    fn una_lingua_qualunque_va_e_torna() {
        let c = libreria();
        // Nessuna di queste è nell'elenco della finestra oggi, ed è il punto:
        // il nucleo non ha un elenco.
        for codice in ["it", "en", "de", "sv", "pt-BR"] {
            imposta_lingua(&c, codice).expect("scrittura");
            assert_eq!(lingua(&c), Ok(Some(codice.to_owned())));
        }
    }

    #[test]
    fn quel_che_non_e_un_codice_non_si_scrive() {
        let c = libreria();
        for storto in ["../it", "italiano bello", "I", "IT", "it_IT!"] {
            let err = imposta_lingua(&c, storto).expect_err("non è un codice");
            assert_eq!(err.code().kind().code(), "settings.corrupt");
        }
        assert_eq!(lingua(&c), Ok(None));
    }

    #[test]
    fn una_lingua_storta_gia_in_tabella_vale_come_mai_scelta() {
        let c = libreria();
        settings::write(&c, CHIAVE_LINGUA, "../../etc").expect("scrittura");
        assert_eq!(
            lingua(&c),
            Ok(None),
            "una riga arrivata da un profilo futuro non deve far cercare un file"
        );
    }

    #[test]
    fn svuotare_la_lingua_rimette_il_rilevamento() {
        let c = libreria();
        imposta_lingua(&c, "en").expect("scrittura");
        imposta_lingua(&c, "  ").expect("svuotamento");
        assert_eq!(lingua(&c), Ok(None));
    }

    #[test]
    fn svuotare_rimette_quelle_di_serie() {
        let c = libreria();
        imposta_scorciatoie(&c, r#"{"cerca":["k"]}"#).expect("scrittura");
        imposta_scorciatoie(&c, "  ").expect("svuotamento");
        assert_eq!(
            scorciatoie(&c),
            Ok(None),
            "assente vuol dire «quelle di serie»; una stringa vuota vorrebbe dire «nessuna»"
        );
    }

    #[test]
    fn il_secondo_piano_parte_spento() {
        let c = libreria();
        assert_eq!(
            secondo_piano(&c),
            Ok(false),
            "un aggiornamento non deve far sparire nel vassoio un programma che si credeva chiuso"
        );
        imposta_secondo_piano(&c, true).expect("scrittura");
        assert_eq!(secondo_piano(&c), Ok(true));
        imposta_secondo_piano(&c, false).expect("scrittura");
        assert_eq!(secondo_piano(&c), Ok(false));
    }

    #[test]
    fn il_movimento_ridotto_parte_spento() {
        let c = libreria();
        assert_eq!(
            movimento_ridotto(&c),
            Ok(false),
            "accenderla d'ufficio spegnerebbe il movimento a chi non l'ha chiesto"
        );
        imposta_movimento_ridotto(&c, true).expect("scrittura");
        assert_eq!(movimento_ridotto(&c), Ok(true));
        imposta_movimento_ridotto(&c, false).expect("scrittura");
        assert_eq!(movimento_ridotto(&c), Ok(false));
    }

    #[test]
    fn un_movimento_ridotto_storto_vale_come_spento() {
        let c = libreria();
        settings::write(&c, CHIAVE_MOVIMENTO_RIDOTTO, "abbastanza").expect("scrittura");
        assert_eq!(
            movimento_ridotto(&c),
            Ok(false),
            "una riga arrivata da un profilo futuro non deve poter spegnere il movimento"
        );
    }

    #[test]
    fn lo_zoom_parte_neutro() {
        let c = libreria();
        assert_eq!(
            zoom(&c),
            Ok(ZOOM_NEUTRO),
            "chi non ha mai premuto Ctrl+ deve vedere la misura che il foglio dichiara"
        );
    }

    #[test]
    fn il_neutro_e_un_gradino() {
        assert!(
            SCALA_ZOOM
                .iter()
                .any(|gradino| (gradino - ZOOM_NEUTRO).abs() < TOLLERANZA_ZOOM),
            "è il gradino da cui si parte e quello a cui Ctrl+0 riporta: fuori scala,              `zoom_al_gradino` non saprebbe da dove muoversi"
        );
    }

    #[test]
    fn uno_zoom_storto_vale_come_neutro() {
        let c = libreria();
        settings::write(&c, CHIAVE_ZOOM, "abbastanza").expect("scrittura");
        assert_eq!(
            zoom(&c),
            Ok(ZOOM_NEUTRO),
            "una riga che non è nemmeno un numero non deve poter ingrandire niente"
        );
        settings::write(&c, CHIAVE_ZOOM, "3.7").expect("scrittura");
        assert_eq!(
            zoom(&c),
            Ok(ZOOM_NEUTRO),
            "un numero fuori scala è un'ipotesi di qualcun altro: si torna al vero"
        );
        settings::write(&c, CHIAVE_ZOOM, "1.05").expect("scrittura");
        assert_eq!(
            zoom(&c),
            Ok(ZOOM_NEUTRO),
            "e vale anche per un numero dentro la scala ma fra due gradini"
        );
    }

    #[test]
    fn lo_zoom_va_e_torna() {
        let c = libreria();
        assert_eq!(imposta_zoom(&c, 1.5), Ok(1.5), "un gradino si scrive com'è");
        assert_eq!(zoom(&c), Ok(1.5));
        assert_eq!(
            imposta_zoom(&c, 3.7),
            Ok(ZOOM_NEUTRO),
            "quel che si scrive dev'essere quel che si applica, o si scoprirebbe al riavvio dopo"
        );
        assert_eq!(zoom(&c), Ok(ZOOM_NEUTRO));
    }

    #[test]
    fn i_gradini_non_escono_dalla_scala() {
        let primo = SCALA_ZOOM.first().copied().unwrap_or(ZOOM_NEUTRO);
        let ultimo = SCALA_ZOOM.last().copied().unwrap_or(ZOOM_NEUTRO);
        assert_eq!(
            zoom_al_gradino(primo, false),
            primo,
            "in fondo alla scala si resta dov'è: girare farebbe ingrandire chi chiedeva il contrario"
        );
        assert_eq!(zoom_al_gradino(ultimo, true), ultimo, "e in cima uguale");
        // E i due versi sono davvero l'uno l'inverso dell'altro, gradino per
        // gradino: senza questo giro una scala scritta storta — due valori
        // uguali, o uno fuori ordine — passerebbe le due prove qui sopra.
        for coppia in SCALA_ZOOM.windows(2) {
            let (basso, alto) = match coppia {
                [basso, alto] => (*basso, *alto),
                _ => continue,
            };
            assert!(basso < alto, "la scala dev'essere crescente");
            assert_eq!(zoom_al_gradino(basso, true), alto);
            assert_eq!(zoom_al_gradino(alto, false), basso);
        }
    }

    #[test]
    fn uno_zoom_storto_riparte_dal_neutro_e_non_dal_fondo() {
        assert_eq!(
            zoom_al_gradino(f64::NAN, true),
            1.1,
            "un valore che non si interpreta vale come neutro, e da lì si sale di uno"
        );
        assert_eq!(zoom_al_gradino(f64::INFINITY, false), 0.9);
    }

    #[test]
    fn le_cartelle_aperte_vanno_e_tornano() {
        let c = libreria();
        assert_eq!(cartelle_aperte(&c), Ok(Vec::new()), "il primo avvio");
        let aperte = vec![r"C:\M".to_owned(), r"C:\M\Rock".to_owned()];
        imposta_cartelle_aperte(&c, &aperte).expect("scrittura");
        assert_eq!(cartelle_aperte(&c), Ok(aperte));
    }

    #[test]
    fn oltre_il_tetto_si_tengono_le_piu_recenti() {
        let c = libreria();
        let tante: Vec<String> = (0..MASSIMO_CARTELLE_APERTE + 50)
            .map(|n| format!(r"C:\M\{n}"))
            .collect();
        imposta_cartelle_aperte(&c, &tante).expect("scrittura");
        let lette = cartelle_aperte(&c).expect("lettura");
        assert_eq!(lette.len(), MASSIMO_CARTELLE_APERTE);
        // Si taglia dalla testa: la più vecchia se ne va, l'ultima aperta resta.
        assert_eq!(lette.first().map(String::as_str), Some(r"C:\M\50"));
        assert_eq!(lette.last(), tante.last());
    }

    #[test]
    fn svuotare_toglie_la_riga_invece_di_scrivere_una_lista_vuota() {
        let c = libreria();
        imposta_cartelle_aperte(&c, &[r"C:\M".to_owned()]).expect("scrittura");
        imposta_cartelle_aperte(&c, &[]).expect("svuotamento");
        assert_eq!(
            settings::read(&c, CHIAVE_CARTELLE_APERTE),
            Ok(None),
            "«mai aperto niente» e «aperto niente» sono lo stesso stato"
        );
    }

    #[test]
    fn una_lista_storta_vale_come_nessuna() {
        let c = libreria();
        settings::write(&c, CHIAVE_CARTELLE_APERTE, "{non json").expect("scrittura");
        assert_eq!(
            cartelle_aperte(&c),
            Ok(Vec::new()),
            "un pannello che non si apre per una riga storta non si può correggere"
        );
    }

    #[test]
    fn la_cartella_scelta_va_e_torna_e_si_dimentica() {
        let c = libreria();
        assert_eq!(cartella_scelta(&c), Ok(None));
        imposta_cartella_scelta(&c, r"C:\M\Rock").expect("scrittura");
        assert_eq!(cartella_scelta(&c), Ok(Some(r"C:\M\Rock".to_owned())));
        imposta_cartella_scelta(&c, "  ").expect("svuotamento");
        assert_eq!(cartella_scelta(&c), Ok(None));
    }

    #[test]
    fn il_giro_non_visto_e_none() {
        let c = libreria();
        assert_eq!(
            giro_visto(&c),
            Ok(None),
            "al primo avvio non c'è nessun giro alle spalle, ed è quel che lo fa partire"
        );
    }

    #[test]
    fn il_giro_segnato_si_rilegge() {
        let c = libreria();
        imposta_giro_visto(&c, "2.3.1").expect("scrittura");
        assert_eq!(giro_visto(&c), Ok(Some("2.3.1".to_owned())));
        // Il punto della versione: un copione nuovo trova scritto quello
        // vecchio e sa di avere qualcosa da dire, senza che nessuno abbia
        // dovuto cancellare niente a nessuno.
        assert_ne!(giro_visto(&c), Ok(Some("2.4.0".to_owned())));
    }

    #[test]
    fn il_giro_azzerato_torna_none() {
        let c = libreria();
        imposta_giro_visto(&c, "2.3.1").expect("scrittura");
        imposta_giro_visto(&c, "  ").expect("azzeramento");
        assert_eq!(
            settings::read(&c, CHIAVE_GIRO_VISTO),
            Ok(None),
            "si toglie la riga invece di scrivere una stringa vuota"
        );
        assert_eq!(giro_visto(&c), Ok(None));
    }
}
