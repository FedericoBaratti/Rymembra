//! Le regole di sessione e il motore in sessione: quel che `riproduzione` chiede al ponte
//! dentro i suoi gesti, sul `Lettore` che ha già in mano.

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::queue::{Queue, QueueSnapshot};
use rymembra_insieme::messaggi::MASSIMO_VOCI;
use rymembra_play::tempo;

use crate::nota;
use crate::riproduzione::Lettore;
use crate::riproduzione::ospiti::{self, MANIGLIA_CALIBRAZIONE};
use crate::spegnimento::Emette as _;

use super::{ANTICIPO_NS, Programmata, Ruolo, SCADENZA_PROGRAMMATA_NS};

// ── le regole ───────────────────────────────────────────────────────────────

/// La dissolvenza da dare al motore, in millisecondi: zero in sessione, la
/// scelta dell'utente fuori. Pura, per poterla provare: vedi `prove`.
pub(super) const fn dissolvenza_per(in_sessione: bool, secondi: u64) -> u64 {
    if in_sessione {
        0
    } else {
        secondi.saturating_mul(1000)
    }
}

/// L'autoplay che vale: mai in sessione.
pub(super) const fn autoplay_per(in_sessione: bool, voluto: bool) -> bool {
    voluto && !in_sessione
}

/// La coda si scrive sul disco: mai in sessione.
pub(super) const fn coda_si_scrive_per(in_sessione: bool) -> bool {
    !in_sessione
}

/// La dissolvenza da dare al motore adesso, in millisecondi.
///
/// È l'unico posto da cui il motore la riceve: il comando, la coda di ieri,
/// la riapertura del dispositivo, l'ingresso e l'uscita. `Lettore::dissolvenza_s`
/// resta la scelta dell'utente — la mostra il cursore, e la rilegge l'uscita
/// dalla sessione — e la sessione decide solo quella **effettiva**.
pub(crate) fn dissolvenza_ms(lettore: &Lettore) -> u64 {
    dissolvenza_per(lettore.sessione.is_some(), lettore.dissolvenza_s)
}

/// L'autoplay che vale adesso: vedi [`autoplay_per`].
pub(crate) fn autoplay(lettore: &Lettore) -> bool {
    autoplay_per(lettore.sessione.is_some(), lettore.autoplay)
}

/// La coda si scrive sul disco adesso: vedi `coda::scrivi_coda`.
pub(crate) fn coda_si_scrive(lettore: &Lettore) -> bool {
    coda_si_scrive_per(lettore.sessione.is_some())
}

/// Questo lettore ospita una sessione.
pub(crate) fn ospita(lettore: &Lettore) -> bool {
    lettore
        .sessione
        .as_ref()
        .is_some_and(|sessione| sessione.ruolo == Ruolo::Ospita)
}

/// Questo lettore segue la sessione di un altro.
pub(crate) fn segue(lettore: &Lettore) -> bool {
    lettore
        .sessione
        .as_ref()
        .is_some_and(|sessione| sessione.ruolo == Ruolo::Segue)
}

/// La calibrazione è accesa.
pub(crate) fn in_calibrazione(lettore: &Lettore) -> bool {
    lettore
        .sessione
        .as_ref()
        .is_some_and(|sessione| sessione.calibrazione.is_some())
}

/// L'id è la voce di calibrazione.
pub(crate) const fn e_la_calibrazione(id: i64) -> bool {
    id == MANIGLIA_CALIBRAZIONE
}

/// Il brano da mostrare nella barra: quello della coda, o la voce di
/// calibrazione mentre è lei a suonare.
pub(crate) fn corrente_da_mostrare(lettore: &Lettore) -> Option<i64> {
    if lettore.motore.posizione().track_id == Some(MANIGLIA_CALIBRAZIONE) {
        return Some(MANIGLIA_CALIBRAZIONE);
    }
    lettore.coda.current()
}

/// Il punto da mostrare alla finestra al posto di quello del motore, in
/// sessione; `None` per quello del motore.
///
/// # Perché
///
/// Perché in sessione una voce nuova non parte subito: parte fra un quarto di
/// secondo (chi ospita) o quando il regolatore l'ha riaperta (chi segue), e
/// fino al primo campione il motore dice ancora il punto della voce di prima.
/// La coda invece è già sulla nuova, e la finestra disegnava il titolo nuovo
/// col cursore a 63 secondi su un brano appena cominciato. Fuori sessione la
/// stessa cosa dura qualche millisecondo, e la copre `manda_stato_con_posizione`;
/// qui dura abbastanza perché si veda, e passa anche dal tempo dell'orologio.
///
/// Due regole, nell'ordine:
///
/// - una partenza promessa per la voce che si mostra, finché il motore non
///   suona lei, finché l'istante non è passato, o finché il motore non è
///   davvero arrivato lì: il punto promesso (il salto, o l'inizio). Con le
///   cuffie Bluetooth il primo campione del salto arriva all'orecchio parecchio
///   dopo l'istante, e nel frattempo il motore dice ancora il punto di prima:
///   la barra tornava indietro per mezzo secondo e poi saltava avanti (visto
///   nelle prove a tre PC, a ogni salto chiesto da un membro);
/// - il motore su un'altra voce di quella che si mostra — lo specchio di chi
///   segue è già sulla voce dopo, il regolatore non l'ha ancora riaperta: zero,
///   l'inizio della voce che si vedrà.
pub(crate) fn posizione_da_mostrare(
    lettore: &Lettore,
    posizione: &rymembra_play::Posizione,
) -> Option<u64> {
    let sessione = lettore.sessione.as_ref()?;
    let corrente = corrente_da_mostrare(lettore);
    punto_da_mostrare(
        sessione.programmata.as_ref(),
        corrente,
        posizione.track_id,
        posizione.ms,
        tempo::adesso_ns(),
    )
}

/// Quanto il motore può stare lontano dal punto promesso, già fatto avanzare
/// col tempo, per dirsi arrivato: un secondo e mezzo, il ritardo di un primo
/// campione lento più l'arrotondamento al millisecondo.
pub(super) const ARRIVATO_ENTRO_MS: u64 = 1_500;

/// La regola di [`posizione_da_mostrare`], senza motore e senza orologio.
pub(super) fn punto_da_mostrare(
    promessa: Option<&Programmata>,
    corrente: Option<i64>,
    nel_motore: Option<i64>,
    motore_ms: u64,
    adesso_ns: u64,
) -> Option<u64> {
    // Una promessa vale al più fino alla sua scadenza: chi segue la fissa
    // riaprendo e non la toglie mai (la toglie l'ancora di chi ospita), e una
    // promessa vecchia di minuti teneva la barra di chi segue ferma sul punto
    // di quando era entrato, con la musica che andava.
    if let Some(promessa) = promessa
        && promessa.maniglia.is_some()
        && promessa.maniglia == corrente
        && adesso_ns <= promessa.quando_ns.saturating_add(SCADENZA_PROGRAMMATA_NS)
    {
        let promesso_ms = promessa.posizione_us.checked_div(1000).unwrap_or(0);
        let passati_ms = adesso_ns
            .saturating_sub(promessa.quando_ns)
            .checked_div(1_000_000)
            .unwrap_or(0);
        let arrivato = nel_motore == promessa.maniglia
            && adesso_ns >= promessa.quando_ns
            && motore_ms.abs_diff(promesso_ms.saturating_add(passati_ms)) <= ARRIVATO_ENTRO_MS;
        if !arrivato {
            return Some(promesso_ms);
        }
    }
    if corrente.is_some() && nel_motore.is_some() && nel_motore != corrente {
        return Some(0);
    }
    None
}

/// Chi segue: il cursore della coda sulla voce che il motore ha cominciato,
/// se è nello specchio. Vedi il ramo di chi segue in `iniziato`.
pub(crate) fn cursore_sulla_voce(lettore: &mut Lettore, maniglia: i64) {
    if lettore.coda.current() == Some(maniglia) {
        return;
    }
    if let Some(posto) = lettore
        .coda
        .in_play_order()
        .iter()
        .position(|id| *id == maniglia)
    {
        lettore.coda.play_at(posto);
    }
}

/// Quanti dei brani da accodare entrano nella coda di sessione, che ha un
/// tetto: [`MASSIMO_VOCI`]. Fuori sessione tutti.
///
/// # Perché il tetto si fa rispettare qui
///
/// Perché è un tetto **del protocollo**: lo stato della sessione con più voci
/// di così, ogni membro lo rifiuta con `insieme.protocollo` e chiude la
/// connessione. Un brano accodato in più costerebbe a tutti gli altri la
/// sessione. Quelli che entrano entrano, il gesto non fallisce, e per quelli
/// che restano fuori la finestra riceve un avviso: vedi [`avvisa_coda_piena`].
///
/// È la strada dei gesti di **chi ospita**, che l'avviso lo deve vedere.
/// L'aggiunta di un membro passa da [`accoda_maniglie`](super::accoda_maniglie), che usa lo stesso
/// conto senza avvisare questa finestra: il gesto non è suo.
pub(crate) fn entrano<'a>(
    app: &tauri::AppHandle,
    lettore: &Lettore,
    brani: &'a [i64],
) -> &'a [i64] {
    let quante = quante_entrano(lettore.sessione.is_some(), lettore.coda.len(), brani.len());
    if quante < brani.len() {
        // Sul filo del seguito: qui il lucchetto del lettore è in mano, e il
        // diario scrive sul disco.
        let fuori = brani.len().saturating_sub(quante);
        super::in_ordine(app, move |app| avvisa_coda_piena(app, fuori));
    }
    brani.get(..quante).unwrap_or_default()
}

/// Quanti di `chiesti` brani entrano in una coda che ne ha già `in_coda`: in
/// sessione fino a [`MASSIMO_VOCI`], fuori tutti. Vedi [`entrano`].
pub(super) const fn quante_entrano(in_sessione: bool, in_coda: usize, chiesti: usize) -> usize {
    if !in_sessione {
        return chiesti;
    }
    let posto = MASSIMO_VOCI.saturating_sub(in_coda);
    if chiesti < posto { chiesti } else { posto }
}

/// Dei brani sono rimasti fuori dalla coda di sessione: una riga nel diario
/// (il conto, non i brani) e un avviso alla finestra, con la chiave di
/// `insieme.codaPiena`. Senza lucchetti in mano.
fn avvisa_coda_piena(app: &tauri::AppHandle, fuori: usize) {
    nota!("[insieme] coda di sessione piena: {fuori} brani lasciati fuori");
    app.emetti(
        "insieme:avviso",
        crate::insieme::AvvisoIpc {
            chiave: "errors.insieme.codaPiena".to_owned(),
            nome: None,
        },
    );
}

/// Quel che entra in coda per un gesto che accoda dei brani, deciso **sotto il
/// lucchetto del lettore**: la lista, e le voci da dimenticare.
///
/// # La gara che chiude
///
/// La strada del gesto si chiede prima del lucchetto, e dal ruolo della
/// sessione: nell'istante fra il ruolo che diventa «ospita» e il lettore che
/// entra davvero in sessione, la strada crea già le voci e dà le maniglie
/// (`ConVoci`), ma la coda è ancora quella dell'utente. Messe lì, le maniglie
/// sarebbero tornate all'uscita come voci sparite, e una coda con una maniglia
/// non si salva più.
///
/// Qui si decide guardando il lettore, che è la verità: se ospita, le voci; se
/// no, i brani chiesti — il gesto era dell'utente, e fuori sessione è
/// esattamente quel che avrebbe fatto — e le voci create per niente si
/// dimenticano. Un errore ritentabile sarebbe stato l'altra scelta: avrebbe
/// fatto ripetere all'utente un gesto per una gara che non può vedere.
pub(crate) fn brani_da_mettere(
    ospita: bool,
    righe: Vec<i64>,
    voci: Option<Vec<i64>>,
) -> (Vec<i64>, Vec<i64>) {
    match voci {
        None => (righe, Vec::new()),
        Some(maniglie) if ospita => (maniglie, Vec::new()),
        Some(maniglie) => (righe, maniglie),
    }
}

/// Come [`brani_da_mettere`], per «suona», che porta anche il brano da cui
/// partire: la lista, il punto di partenza, e le voci da dimenticare.
///
/// # L'indice
///
/// Le voci non sono per forza tante quante le righe: [`voci_da_libreria`](super::voci_da_libreria)
/// salta quelle che la libreria non ha più. L'indice si ricalcola quindi sulla
/// voce del brano scelto — la stessa occorrenza, se il brano è in lista due
/// volte — e se quella manca, o se non c'è nessuna voce, il gesto si rifiuta
/// con `library.trackNotFound` **senza toccare** la coda di tutti: suonare un
/// altro brano, o svuotare la coda, sarebbe un gesto che nessuno ha fatto.
///
/// `righe_delle_voci` è la riga dietro ogni maniglia, dal registro.
pub(crate) fn lista_da_suonare(
    ospita: bool,
    righe: Vec<i64>,
    indice: usize,
    voci: Option<VociDaSuonare>,
) -> (Partenza, Vec<i64>) {
    match voci {
        None => (Ok((righe, indice)), Vec::new()),
        Some((maniglie, _)) if !ospita => (Ok((righe, indice)), maniglie),
        Some((maniglie, righe_delle_voci)) => {
            match indice_fra_le_voci(&righe, indice, &righe_delle_voci) {
                Some(da) => (Ok((maniglie, da)), Vec::new()),
                None => (
                    Err(AppError::new(ErrorCode::LibraryTrackNotFound {
                        track_id: righe.get(indice).copied(),
                    })
                    .with_cause("il brano scelto non è diventato una voce della sessione")),
                    maniglie,
                ),
            }
        }
    }
}

/// Le maniglie di un «suona» e, per ciascuna, la riga che apre.
pub(super) type VociDaSuonare = (Vec<i64>, Vec<Option<i64>>);

/// La lista da suonare e il punto da cui partire, o il rifiuto.
pub(super) type Partenza = Result<(Vec<i64>, usize), AppError>;

/// Dove sta, fra le voci, il brano che stava a `indice` fra le righe: la
/// stessa occorrenza della stessa riga. `None` se non è diventato una voce.
fn indice_fra_le_voci(
    righe: &[i64],
    indice: usize,
    righe_delle_voci: &[Option<i64>],
) -> Option<usize> {
    let scelta = *righe.get(indice)?;
    let occorrenza = righe
        .get(..=indice)?
        .iter()
        .filter(|riga| **riga == scelta)
        .count();
    righe_delle_voci
        .iter()
        .enumerate()
        .filter(|(_, riga)| **riga == Some(scelta))
        .nth(occorrenza.checked_sub(1)?)
        .map(|(posto, _)| posto)
}

/// La riga dietro ogni maniglia, dal registro: vedi [`lista_da_suonare`].
pub(crate) fn righe_delle_voci(app: &tauri::AppHandle, maniglie: &[i64]) -> Vec<Option<i64>> {
    ospiti::con_registro(app, |registro| {
        maniglie
            .iter()
            .map(|maniglia| {
                registro
                    .voce(*maniglia)
                    .and_then(|voce| voce.apertura_di_libreria())
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Toglie dal registro voci che nessuna coda nominerà. Il registro è un
/// lucchetto foglia: si può chiamare col lettore in mano.
pub(crate) fn dimentica_voci(app: &tauri::AppHandle, maniglie: &[i64]) {
    if maniglie.is_empty() {
        return;
    }
    ospiti::con_registro(app, |registro| registro.togli(maniglie));
}

/// La coda dell'utente senza maniglie: quelle che una gara d'ingresso ci
/// avesse lasciato. Il cursore resta dov'era, o scivola sul brano dopo come
/// per ogni brano tolto.
pub(super) fn ripulita(istantanea: QueueSnapshot) -> QueueSnapshot {
    let maniglie: Vec<i64> = istantanea
        .tracks
        .iter()
        .copied()
        .filter(|id| !ospiti::e_di_libreria(*id))
        .collect();
    if maniglie.is_empty() {
        return istantanea;
    }
    let mut coda = Queue::restore(istantanea);
    coda.remove_tracks(&maniglie);
    coda.snapshot()
}

/// La finestra di brani attorno a `indice`, se si è in sessione, per un gesto
/// che sostituisce la coda: vedi [`finestra`]. Fuori sessione tutti.
///
/// Si legge il ruolo della sessione e non il lettore: si chiama prima della
/// strada del gesto, senza lucchetti, e la strada stessa decide dal ruolo.
pub(crate) fn finestra_di_sessione(
    app: &tauri::AppHandle,
    brani: Vec<i64>,
    indice: usize,
) -> (Vec<i64>, usize) {
    let ruolo = crate::insieme::ruolo(app);
    if ruolo == crate::insieme::Ruolo::Fuori {
        return (brani, indice);
    }
    // L'avviso solo per chi ospita, che il gesto lo fa davvero: per chi segue
    // il gesto va all'host, o si rifiuta, e i suoi tetti sono altri.
    if ruolo == crate::insieme::Ruolo::Host && brani.len() > MASSIMO_VOCI {
        avvisa_coda_piena(app, brani.len().saturating_sub(MASSIMO_VOCI));
    }
    let (finestra, dove) = finestra(&brani, Some(indice), MASSIMO_VOCI);
    (finestra, dove.unwrap_or(0))
}

/// Al più `tetto` brani attorno al `posto`: qualcuno prima, perché «precedente»
/// abbia dove andare, e il resto dopo, che è quel che si ascolterà.
///
/// Un decimo del tetto prima, al più. Una coda da cinquantamila brani — «Tutti
/// i brani» — entra in una sessione con i mille che cominciano da quello che
/// suona, che è quel che chi la condivide si aspetta di sentire.
pub(super) fn finestra(
    ordine: &[i64],
    posto: Option<usize>,
    tetto: usize,
) -> (Vec<i64>, Option<usize>) {
    if ordine.len() <= tetto {
        return (ordine.to_vec(), posto);
    }
    let qui = posto.unwrap_or(0).min(ordine.len().saturating_sub(1));
    let indietro = qui.min(tetto.checked_div(10).unwrap_or(0));
    let fine = qui
        .saturating_sub(indietro)
        .saturating_add(tetto)
        .min(ordine.len());
    let inizio = fine.saturating_sub(tetto);
    let pezzo = ordine.get(inizio..fine).unwrap_or_default().to_vec();
    (pezzo, posto.map(|_| qui.saturating_sub(inizio)))
}

// ── il motore in sessione ───────────────────────────────────────────────────

/// Fa partire `brano` dal millisecondo `da_ms`: chi ospita fra [`ANTICIPO_NS`]
/// e annunciandolo, tutti gli altri subito. Vedi «Chi ospita parte fra un
/// quarto di secondo» nel `//!` del ponte (`insieme.rs`).
pub(crate) fn parti(lettore: &mut Lettore, brano: rymembra_play::BranoAperto, da_ms: u64) {
    if !ospita(lettore) {
        lettore.suona_da(brano, da_ms);
        return;
    }
    let etichetta = brano.etichetta();
    let maniglia = brano.track_id();
    lettore.apri_fermo(brano, da_ms);
    programma(
        lettore,
        Some(maniglia),
        etichetta,
        Some(da_ms.saturating_mul(1000)),
    );
}

/// Riprende: chi ospita fra [`ANTICIPO_NS`], dal punto in cui il motore
/// aspetta; tutti gli altri subito.
///
/// Solo da fermi: un motore che sta già suonando non ha niente da
/// programmare, e un avvio programmato chiesto lì aprirebbe un buco di un
/// quarto di secondo dove non serviva.
pub(crate) fn riprendi(lettore: &mut Lettore) {
    if !ospita(lettore) || !lettore.motore.posizione().in_pausa {
        lettore.motore.riprendi();
        return;
    }
    let maniglia = lettore.motore.posizione().track_id;
    let attesa = lettore.motore.in_attesa();
    programma(
        lettore,
        maniglia,
        attesa.and_then(|(etichetta, _)| etichetta),
        attesa.map(|(_, posizione_us)| posizione_us),
    );
}

/// Salta a `ms`: chi ospita, mentre suona, ferma, salta e riparte fra
/// [`ANTICIPO_NS`]; tutti gli altri — e chi ospita da fermo — saltano e
/// basta. Vedi «Chi ospita parte fra un quarto di secondo» nel `//!` del ponte
/// (`insieme.rs`).
///
/// Pausa, salto e ripresa programmata, in quest'ordine, sono la forma che il
/// motore vuole: un avvio programmato chiesto mentre suona non ferma niente, e
/// un salto a musica in corso farebbe sentire il punto d'arrivo prima degli
/// altri.
pub(crate) fn salta(lettore: &mut Lettore, ms: u64) {
    let suona = !lettore.motore.posizione().in_pausa;
    if !ospita(lettore) || !suona || lettore.motore.posizione().track_id.is_none() {
        lettore.motore.vai_a(ms);
        return;
    }
    // L'etichetta si legge prima della pausa, finché l'ancora la dice: dopo il
    // salto non la direbbe più nessuno fino al primo blocco nuovo.
    let etichetta = lettore
        .motore
        .ancora()
        .and_then(|ancora| ancora.etichetta)
        .or_else(|| {
            lettore
                .motore
                .in_attesa()
                .and_then(|(etichetta, _)| etichetta)
        });
    let maniglia = lettore.motore.posizione().track_id;
    lettore.motore.pausa();
    lettore.motore.vai_a(ms);
    programma(lettore, maniglia, etichetta, Some(ms.saturating_mul(1000)));
}

/// La ripresa programmata vera e propria, e l'annuncio per l'ancora.
///
/// Il flusso si sveglia **prima** di scegliere l'istante. In pausa riposa,
/// anche in sessione — è così che le cuffie Bluetooth tornano libere — e il
/// risveglio, che `riprendi_a` farebbe comunque, dopo la scelta si
/// mangerebbe il quarto di secondo che serve agli altri: su un'uscita
/// Bluetooth anche tutto. Dopo un `apri_fermo` o un salto il flusso è già
/// sveglio, e qui costa un giro di messaggio.
///
/// Se il motore rifiuta l'istante — non può, è nel futuro, ma la risposta
/// esiste — si riparte subito, come fuori sessione: meglio un brano che
/// arriva agli altri un quarto di secondo dopo che un brano che non parte.
fn programma(
    lettore: &mut Lettore,
    maniglia: Option<i64>,
    etichetta: Option<u64>,
    posizione_us: Option<u64>,
) {
    lettore.motore.sveglia();
    let quando_ns = tempo::adesso_ns().saturating_add(ANTICIPO_NS);
    match lettore.motore.riprendi_a(quando_ns) {
        Ok(()) => {
            if let Some(sessione) = lettore.sessione.as_mut() {
                sessione.programmata = posizione_us.map(|posizione_us| Programmata {
                    maniglia,
                    etichetta,
                    posizione_us,
                    quando_ns,
                });
            }
        }
        Err(rifiuto) => {
            nota!("[insieme] avvio programmato rifiutato ({rifiuto:?}): si parte subito");
            lettore.motore.riprendi();
        }
    }
}

/// La latenza dichiarata di quest'uscita è cambiata, in sessione: la musica
/// si sposta subito della stessa quantità, all'indietro.
///
/// # Perché qui e non dal regolatore
///
/// Perché la latenza dichiarata non sposta il suono, sposta l'istante in cui
/// il motore **crede** di essere sentito, e con lui le ancore. Lasciato al
/// regolatore, un millisecondo in più diventerebbe un errore di un
/// millisecondo, che la deriva toglie in una ventina di secondi (50 ppm per
/// ms): calibrare a orecchio, un millisecondo per volta, vorrebbe dire
/// aspettare venti secondi dopo ogni tacca del cursore per sentire che cosa ha
/// fatto. Un gradino uguale e contrario sposta il suono nell'istante, e lascia
/// l'errore misurato dov'era: posizione e istante si muovono insieme, il
/// regolatore non vede niente da correggere.
///
/// Il segno è quello del motore: dichiarare più latenza vuol dire che il
/// suono arriva più tardi di quel che si credeva, quindi si salta avanti
/// (gradino negativo). Da fermi la callback i gradini li lascia cadere, ed è
/// giusto: alla ripresa l'avvio programmato tiene già conto del numero nuovo.
pub(crate) fn latenza_cambiata(lettore: &Lettore, prima_ms: i64, dopo_ms: i64) {
    if lettore.sessione.is_none() || lettore.motore.posizione().in_pausa {
        return;
    }
    let differenza_ms = i128::from(dopo_ms) - i128::from(prima_ms);
    let fotogrammi = differenza_ms
        .saturating_mul(i128::from(lettore.motore.formato().frequenza))
        .checked_div(1000)
        .unwrap_or(0);
    if let Ok(fotogrammi) = i64::try_from(-fotogrammi) {
        lettore.motore.gradino(fotogrammi);
    }
}

/// Una pausa butta l'avvio programmato: l'annuncio parlerebbe di una musica
/// che non partirà.
pub(crate) fn dopo_una_pausa(lettore: &mut Lettore) {
    if let Some(sessione) = lettore.sessione.as_mut() {
        sessione.programmata = None;
    }
}

/// Un brano nuovo dato al motore, o il motore fermato: l'avvio programmato e
/// la calibrazione erano di quel che c'era prima. Lo chiama
/// `Lettore::dopo_un_avvio`, che è la strada di ogni partenza; chi programma
/// o calibra riscrive i suoi dopo.
pub(crate) fn dopo_un_avvio(lettore: &mut Lettore) {
    if let Some(sessione) = lettore.sessione.as_mut() {
        sessione.programmata = None;
        sessione.calibrazione = None;
    }
}

/// Il motore è stato sostituito — un'uscita nuova, un dispositivo tornato —
/// e l'avvio programmato era di quello di prima: si dimentica. La
/// dissolvenza della sessione la rimette già chi lo sostituisce, con
/// [`dissolvenza_ms`].
///
/// E la sessione lo deve sapere: l'uscita nuova è un'altra scheda con un
/// altro quarzo, e la deriva che il regolatore di chi segue aveva imparato
/// era di quella di prima (`Regolatore::azzera`). L'avviso non blocca, e si
/// può dare col lucchetto del lettore in mano: vedi
/// `crate::insieme::uscita_cambiata`.
pub(crate) fn motore_sostituito(app: &tauri::AppHandle, lettore: &mut Lettore) {
    let Some(sessione) = lettore.sessione.as_mut() else {
        return;
    };
    sessione.programmata = None;
    crate::insieme::uscita_cambiata(app);
}

/// Chi ospita: avvisa la sessione che il lettore è cambiato. Lo chiama
/// `manda_stato`, che è dove passa ogni cambiamento. Non blocca.
pub(crate) fn cambiato(app: &tauri::AppHandle, lettore: &Lettore) {
    if ospita(lettore) {
        crate::insieme::lettore_cambiato(app);
    }
}
