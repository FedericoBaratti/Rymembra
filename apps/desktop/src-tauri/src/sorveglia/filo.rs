//! Il filo della sorveglianza: l'avvio, il risveglio delle cartelle di rete, gli
//! osservatori, i riallineamenti e le scansioni. Fa parte di
//! [`crate::sorveglia`]: il perché e i tempi stanno là.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant, SystemTime};

use notify::Watcher as _;
use rymembra_app::library::{Cambiamento, Deposito, Natura, Scadenze, Scan, piu_esterne};
use rymembra_app::volumi::{Collocazione, Esistenza};
use rymembra_barra::{Avviso, Registrazione};
use rymembra_domain::errors::AppError;
use rymembra_domain::paths::{PathRules, is_under, path_key};
use tauri::{AppHandle, Manager as _};

use crate::nota;
use crate::stato::{DepositoStato, Stato, TurnoDiScansione, con_libreria};

use super::accorpamento::{cartelle_da_guardare, natura_di};
use super::{
    Accorpatore, CHIAVE_SORVEGLIANZA, CHIUSURA_ATTESA, CONTROLLO, CartellaSorvegliata, Come,
    MANCATE_PER_TORNARE, Messaggio, Modo, PARTENZA_MASSIMA, Pianificatore, QuadroSorveglianza,
    RAMO_PERSO_TENTATIVI, RISPOSTA_AL_VOLUME, Riallineamento, Ritmo, SONDAGGIO, StatoSorveglianza,
    UltimaPassata, cambiamenti_dell_avviso, come_sorvegliare,
};

// ── il filo ─────────────────────────────────────────────────────────────────

/// Registra lo stato e avvia il filo. Da chiamare nel `setup`, dopo la
/// libreria.
pub fn avvia(app: &AppHandle) {
    // La scelta si legge subito, così la finestra che chiede il quadro nei
    // primi secondi vede quella vera e non quella di serie. E con lei l'elenco
    // delle cartelle com'è adesso, prima che la finestra possa cambiarlo: solo
    // queste hanno il riallineamento dell'avvio (vedi
    // `Pianificatore::fotografa`). Un elenco che non si legge non toglie la
    // scelta.
    let letto = app.try_state::<Stato>().and_then(|stato| {
        con_libreria(&stato, |l| {
            let modo = rymembra_app::settings::read(&l.connection, CHIAVE_SORVEGLIANZA)?;
            let radici = rymembra_app::settings::read_json::<Vec<String>>(
                &l.connection,
                rymembra_app::settings::CHIAVE_CARTELLE,
            )
            .ok()
            .map(Option::unwrap_or_default);
            Ok((modo, radici))
        })
        .ok()
    });
    let (modo, dell_avvio) = letto.unwrap_or_default();
    // Il riallineamento dell'avvio è per le cartelle del disco: quelle del
    // cloud le riallinea il filo del loro conto, che sa cosa è cambiato.
    let dell_avvio = dell_avvio.map(|radici| rymembra_domain::nuvola::solo_locali(&radici));
    let modo = Modo::dal_salvato(modo.as_deref());
    let (posta, orecchio) = channel();
    app.manage(StatoSorveglianza {
        posta: Mutex::new(Some(posta.clone())),
        quadro: Mutex::new(QuadroSorveglianza {
            modo: modo.come_testo().to_owned(),
            ..QuadroSorveglianza::default()
        }),
        dove: Mutex::new(Vec::new()),
    });
    let mano = app.clone();
    let del_filo = posta.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-sorveglia".to_owned())
        .spawn(move || filo(&mano, &del_filo, &orecchio, dell_avvio));
    if let Err(err) = avviato {
        nota!("[sorveglia] il filo non è partito: {err}");
    }
    let mano = app.clone();
    let avviato = std::thread::Builder::new()
        .name("rymembra-risveglio".to_owned())
        .spawn(move || {
            let _fine = FineDelRisveglio(mano.clone());
            risveglia(&mano);
        });
    if let Err(err) = avviato {
        nota!("[sorveglia] il risveglio delle cartelle di rete non è partito: {err}");
        // Senza risveglio non c'è niente da aspettare: il primo riallineamento
        // proverà da sé.
        let _ = posta.send(Messaggio::Svegliate);
    }
}

/// Dice al filo che il risveglio è finito, comunque sia finito — anche
/// cadendo in panico: il primo riallineamento aspetta questo messaggio, e senza
/// non partirebbe mai.
struct FineDelRisveglio(AppHandle);

impl Drop for FineDelRisveglio {
    fn drop(&mut self) {
        if let Some(stato) = self.0.try_state::<StatoSorveglianza>() {
            stato.manda(Messaggio::Svegliate);
        }
    }
}

/// Quanto si concede al risveglio di una cartella di rete, all'avvio.
///
/// Mezzo minuto: un NAS addormentato fa girare i dischi in otto, quindici
/// secondi, e Windows ricollega una lettera mappata quando qualcuno ci bussa —
/// non prima. Nessuno aspetta questa risposta: serve a bussare.
const RISVEGLIO_SCADENZA: Duration = Duration::from_secs(30);

/// Bussa alle cartelle di rete appena Rymembra si apre, perché siano sveglie
/// quando qualcuno preme play.
///
/// # Perché
///
/// Windows ricollega le lettere mappate **in modo pigro**: all'accesso, non al
/// login. E un NAS che dorme fa girare i dischi solo quando gli si chiede un
/// file. Il primo a chiederlo era il primo play: cinque secondi di scadenza
/// contro un minuto di riconnessione, e la finestra diceva «la cartella di rete
/// non risponde» a chi aveva appena acceso il computer — tre volte, una per
/// ogni clic. Bussare all'avvio fa partire il risveglio mentre la finestra si
/// sta ancora disegnando.
///
/// Riempie anche la mappa di [`StatoSorveglianza::dove`]: subito con «non si
/// sa» per tutte, così [`forse_in_rete`] risponde «forse» invece di «no» nei
/// primi secondi; poi con la risposta vera.
///
/// Solo il diario: niente avvisi, niente scritture. Una cartella che non si
/// sveglia lo dirà chi la usa, quando la usa. Quando ha finito, il filo della
/// sorveglianza comincia i riallineamenti dell'avvio (vedi
/// [`FineDelRisveglio`]): su un NAS che dorme, chi cammina prima di aver bussato
/// trova le cartelle «non raggiungibili».
fn risveglia(app: &AppHandle) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let radici = con_libreria(&stato, |l| {
        rymembra_app::settings::read_json::<Vec<String>>(
            &l.connection,
            rymembra_app::settings::CHIAVE_CARTELLE,
        )
    });
    // Si svegliano i dischi e le share: una cartella del cloud non dorme, e
    // chiedere a Windows dove sta `nuvola://…` non ha risposta.
    let radici = match radici {
        Ok(Some(radici)) if !radici.is_empty() => rymembra_domain::nuvola::solo_locali(&radici),
        _ => return,
    };
    if radici.is_empty() {
        return;
    }
    for radice in &radici {
        ricorda_dove(app, radice, Collocazione::Ignota);
    }
    let dove =
        rymembra_app::volumi::collocazioni(&radici, rymembra_app::volumi::SCADENZA_RISOLUZIONE);
    for (radice, dove) in radici.iter().zip(&dove) {
        ricorda_dove(app, radice, *dove);
    }
    // Anche le UNC: `collocazioni` le riconosce dalla forma senza toccarle, e
    // da sole non si sveglierebbero.
    let da_svegliare: Vec<String> = radici
        .iter()
        .zip(&dove)
        .filter(|(_, dove)| **dove != Collocazione::Locale)
        .map(|(radice, _)| radice.clone())
        .collect();
    if da_svegliare.is_empty() {
        return;
    }
    let inizio = Instant::now();
    let risposte = rymembra_app::volumi::esistenze(da_svegliare.clone(), RISVEGLIO_SCADENZA);
    let sveglie = risposte
        .iter()
        .filter(|risposta| **risposta == Esistenza::Presente)
        .count();
    nota!(
        "[sorveglia] risveglio delle cartelle di rete: {sveglie} su {} rispondono, in {} ms",
        da_svegliare.len(),
        inizio.elapsed().as_millis()
    );
}

/// Ricorda dove sta una cartella sorvegliata. Vedi [`StatoSorveglianza::dove`].
fn ricorda_dove(app: &AppHandle, radice: &str, dove: Collocazione) {
    let Some(stato) = app.try_state::<StatoSorveglianza>() else {
        return;
    };
    let mut elenco = stato.dove();
    match elenco.iter_mut().find(|(gia, _)| gia == radice) {
        Some(voce) => voce.1 = dove,
        None => elenco.push((radice.to_owned(), dove)),
    }
}

/// Questo percorso potrebbe stare su una cartella di rete?
///
/// La risposta che serve a chi apre un brano per decidere quanto aspettarlo:
/// un disco locale risponde in microsecondi, un NAS che si sveglia in quindici
/// secondi, e cinque erano pochi per il secondo. Dalla forma quando basta — un
/// UNC è di rete — e altrimenti dalla cartella sorvegliata più lunga che lo
/// contiene, com'era l'ultima volta che lo si è chiesto al disco. «Non si sa»
/// vale «forse»: è il caso della lettera mappata che non risponde ancora, cioè
/// proprio quello da aspettare di più.
///
/// Non tocca il disco, mai: la chiama un filo che sta per aprire un file, e un
/// controllo che si bloccasse sulla share sarebbe la stessa attesa che si sta
/// cercando di misurare.
pub fn forse_in_rete(app: &AppHandle, percorso: &str) -> bool {
    if rymembra_app::volumi::dalla_forma(Path::new(percorso)).is_some() {
        return true;
    }
    let Some(stato) = app.try_state::<StatoSorveglianza>() else {
        return false;
    };
    let regole = PathRules::for_current_platform();
    let elenco = stato.dove();
    elenco
        .iter()
        .filter(|(radice, _)| is_under(percorso, radice, regole))
        .max_by_key(|(radice, _)| radice.len())
        .is_some_and(|(_, dove)| *dove != Collocazione::Locale)
}

/// Quanto aspettare dopo l'avvio prima di guardare le cartelle.
///
/// Dieci secondi: l'avvio è il momento in cui la finestra, la riproduzione e
/// il database vogliono tutti il disco, e risolvere le lettere mappate di una
/// libreria su un NAS può chiederne qualcuno. Una sorveglianza che arriva
/// dieci secondi dopo non perde niente: il riallineamento dell'avvio guarda
/// comunque tutto.
const ATTESA_AVVIO: Duration = Duration::from_secs(10);

/// Un osservatore di `notify` che sa dire quando ha lasciato la cartella
/// davvero.
///
/// Su Windows lasciar cadere il `RecommendedWatcher` non chiude la cartella:
/// manda un «fermati» al filo di `notify`, che chiude l'handle un attimo dopo,
/// su di sé. Per rispondere a Windows «ho lasciato» serve la chiusura vera, e
/// `notify` non la annuncia. La si legge dalla funzione che gli si è data: la
/// tiene lui, e la lascia cadere **dopo** aver chiuso l'handle — quando il filo
/// e l'ultima richiesta in corso hanno finito. La guardia che sta dentro la
/// funzione cade con lei, e il ricevitore che la aspetta lo vede.
pub(super) struct Osservatore {
    /// Cadendo smette.
    dentro: notify::RecommendedWatcher,
    /// Il lato che aspetta: non riceve mai niente, si disconnette quando la
    /// funzione di `notify` — e con lei la guardia — è caduta.
    chiuso: Receiver<()>,
}

impl Osservatore {
    /// Lo lascia cadere, e restituisce il ricevitore che dirà quando `notify`
    /// ha chiuso la cartella: vedi [`aspetta_la_chiusura`].
    fn lascia(self) -> Receiver<()> {
        let Self { dentro, chiuso } = self;
        drop(dentro);
        chiuso
    }
}

/// Quante delle `chiusure` non sono state confermate entro `fino`.
///
/// Una chiusura è confermata quando il ricevitore si disconnette. Non confermata
/// vuol dire che `notify` non ha finito in tempo — o non finirà, se l'annullo
/// della sua richiesta è fallito — e chi risponde a Windows lo dice nel
/// diario: la richiesta ritorna lo stesso, perché non può aspettare per
/// sempre.
pub(super) fn aspetta_la_chiusura(chiusure: Vec<Receiver<()>>, fino: Instant) -> usize {
    chiusure
        .into_iter()
        .filter(|chiuso| {
            loop {
                match chiuso.recv_timeout(fino.saturating_duration_since(Instant::now())) {
                    Err(RecvTimeoutError::Disconnected) => break false,
                    Err(RecvTimeoutError::Timeout) => break true,
                    // Nessuno manda mai niente: se arrivasse, si riaspetta.
                    Ok(()) => {}
                }
            }
        })
        .count()
}

/// Una cartella sorvegliata, dal lato del filo.
pub(super) struct Sorvegliata {
    /// Come la si guarda.
    pub(super) come: Come,
    /// Dove sta, quando la si è armata.
    pub(super) dove: Collocazione,
    /// L'osservatore, quando la si guarda con gli avvisi. Cadendo smette.
    pub(super) osservatore: Option<Osservatore>,
    /// Il volume su cui sta (`rymembra_barra::volume_di`), quando la si è armata
    /// con gli avvisi e Windows lo sa dire: è quello che Windows nomina quando
    /// chiede di espellerlo. `None` per le cartelle di rete e di altri
    /// sistemi, e per quelle senza osservatore. Vedi `Motore::volume_in_partenza`.
    pub(super) volume: Option<String>,
    /// Le domande «c'è ancora?» di fila rimaste senza risposta. Conta solo per
    /// le cartelle di rete con gli avvisi: vedi [`lascia_cadere_le_morte`].
    pub(super) senza_risposta: u32,
}

impl Sorvegliata {
    /// Il passo dei suoi riallineamenti.
    const fn ritmo(&self) -> Ritmo {
        Ritmo::di(self.come, self.dove)
    }
}

/// Quanto si aspetta la risposta a «la cartella c'è ancora?» per una cartella
/// con gli avvisi, a ogni [`CONTROLLO`].
///
/// Due secondi: un disco di questo computer risponde in microsecondi, e un NAS
/// vivo in millisecondi. Uno che non risponde in due secondi non è dato per
/// perso — vedi [`MANCATE_PER_TORNARE`].
const VIVA_SCADENZA: Duration = Duration::from_secs(2);

/// Quel che si è lasciato quando Windows ha chiesto di espellere un volume.
pub(super) struct Lasciate {
    /// Le cartelle del volume, con o senza osservatore.
    pub(super) cartelle: usize,
    /// Quanti osservatori non hanno confermato in tempo la chiusura della loro
    /// cartella: vedi [`aspetta_la_chiusura`].
    pub(super) non_confermate: usize,
}

/// Lo stato del filo fra un giro e l'altro.
pub(super) struct Motore {
    /// La scelta in vigore.
    modo: Modo,
    /// Le cartelle nell'ordine dell'elenco: è quello in cui la finestra le
    /// mostra.
    pub(super) elenco: Vec<String>,
    /// Le cartelle, con la grafia con cui stanno nell'elenco.
    pub(super) cartelle: BTreeMap<String, Sorvegliata>,
    /// Le registrazioni a Windows per «il volume se ne va», una per volume, per
    /// chiave. Esistono finché una cartella del volume ha il suo osservatore, o
    /// il volume sta partendo: vedi [`Motore::pota_le_registrazioni`].
    registrazioni: BTreeMap<String, Registrazione>,
    /// I volumi che Windows ha chiesto di espellere e di cui non si è ancora
    /// saputo se se ne sono andati o no, con la scadenza di questa attesa.
    /// Le loro cartelle non si riarmano: vedi [`Motore::volume_in_partenza`].
    in_partenza: BTreeMap<String, Instant>,
    /// Gli avvisi in attesa.
    pub(super) attesa: Accorpatore,
    /// Quando riallineare.
    pub(super) piano: Pianificatore,
    /// Il prossimo controllo dell'elenco.
    pub(super) prossimo_controllo: Instant,
    /// L'elenco è stato letto almeno una volta. Prima le cartelle da
    /// riguardare non si possono giudicare: fuori dalle cartelle sorvegliate
    /// si scarterebbero tutte.
    pub(super) configurata: bool,
    /// Chi aspetta la fine del prossimo controllo.
    chi_aspetta: Vec<Sender<()>>,
    /// Dopo un giro caduto in panico, fino a quando non si lavora. La posta
    /// intanto si legge e si conserva.
    pausa: Option<Instant>,
}

/// Le cartelle e non i percorsi, ormai sono decise; e da cartelle «nuove», che
/// si riguardano da sé — o da quella che le conteneva, se intanto sono sparite.
fn indietro(cartelle: &[String]) -> Vec<Cambiamento> {
    cartelle
        .iter()
        .map(|cartella| Cambiamento {
            percorso: cartella.clone(),
            nuovo: true,
        })
        .collect()
}

impl Motore {
    /// Un motore appena partito: niente letto, niente in attesa.
    pub(super) fn nuovo(adesso: Instant) -> Self {
        Self {
            modo: Modo::default(),
            elenco: Vec::new(),
            cartelle: BTreeMap::new(),
            registrazioni: BTreeMap::new(),
            in_partenza: BTreeMap::new(),
            attesa: Accorpatore::default(),
            piano: Pianificatore::default(),
            // Il primo controllo dopo `ATTESA_AVVIO`; una riconfigurazione lo
            // anticipa.
            prossimo_controllo: adesso + ATTESA_AVVIO,
            configurata: false,
            chi_aspetta: Vec::new(),
            pausa: None,
        }
    }

    /// La scelta appena letta; `true` se è cambiata.
    ///
    /// Spenta, quel che aspettava un rinvio cade: è sorveglianza, e da spenta
    /// niente si riprova da sé. Senza, i rinviati continuavano a girare ogni
    /// [`SONDAGGIO`] anche a sorveglianza spenta — contro un NAS spento, per
    /// tutta la sessione. La raffica resta: vedi
    /// [`Accorpatore::svuota_rinviati`].
    pub(super) fn imposta_modo(&mut self, modo: Modo) -> bool {
        let cambiato = modo != self.modo;
        self.modo = modo;
        if modo == Modo::Spenta {
            let caduti = self.attesa.svuota_rinviati();
            if caduti > 0 {
                nota!(
                    "[sorveglia] sorveglianza spenta: {caduti} cartelle in attesa di riprova lasciate stare"
                );
            }
        }
        cambiato
    }

    /// Il volume di `radice` è uno che Windows sta per espellere?
    pub(super) fn e_in_partenza(&self, radice: &str) -> bool {
        self.cartelle
            .get(radice)
            .and_then(|sorvegliata| sorvegliata.volume.as_deref())
            .is_some_and(|volume| self.in_partenza.contains_key(volume))
    }

    /// Windows chiede di espellere `volume`: le sue cartelle si lasciano.
    ///
    /// Gli osservatori cadono, e si aspetta che `notify` abbia chiuso davvero le
    /// cartelle (vedi [`Osservatore`]): chi risponde a Windows lo fa solo dopo,
    /// altrimenti l'espulsione verrebbe rifiutata lo stesso. Le cartelle
    /// diventano «non raggiungibili» come quelle di un disco staccato, e il volume
    /// resta «in partenza» fino a [`Self::volume_rifiutato`],
    /// [`Self::volume_andato`] o [`PARTENZA_MASSIMA`]: intanto il controllo non le
    /// riarma, che vorrebbe dire riaprire la cartella dietro le spalle di Windows.
    ///
    /// Il passo dei riallineamenti **non** cambia. Se l'espulsione è rifiutata, un
    /// cambio di passo vorrebbe dire una camminata dell'intero volume al
    /// riarmo, che terrebbe aperte le cartelle proprio quando l'utente riprova a
    /// espellerlo; il periodico, che c'è comunque, copre quel che è passato nel
    /// frattempo. La registrazione resta: Windows continua a mandarle
    /// «rifiutato» e «andato».
    pub(super) fn volume_in_partenza(&mut self, volume: &str, adesso: Instant) -> Lasciate {
        self.in_partenza
            .insert(volume.to_owned(), adesso + PARTENZA_MASSIMA);
        let mut cartelle = 0_usize;
        let mut chiusure = Vec::new();
        for sorvegliata in self
            .cartelle
            .values_mut()
            .filter(|sorvegliata| sorvegliata.volume.as_deref() == Some(volume))
        {
            cartelle = cartelle.saturating_add(1);
            sorvegliata.come = Come::NonRaggiungibile;
            sorvegliata.senza_risposta = 0;
            if let Some(osservatore) = sorvegliata.osservatore.take() {
                chiusure.push(osservatore.lascia());
            }
        }
        // Prima si lasciano cadere tutti, poi si aspetta: le chiusure di un
        // volume con molte cartelle corrono insieme.
        let non_confermate = aspetta_la_chiusura(chiusure, Instant::now() + CHIUSURA_ATTESA);
        Lasciate {
            cartelle,
            non_confermate,
        }
    }

    /// Un altro programma ha rifiutato l'espulsione di `volume`: si riprende.
    ///
    /// Il controllo corre subito, e riarma le cartelle come per un osservatore
    /// guastato. La registrazione c'è ancora e vale ancora. `true` se il volume
    /// era davvero in partenza: un rifiuto di cui non si sapeva niente non muove
    /// nulla.
    pub(super) fn volume_rifiutato(&mut self, volume: &str, adesso: Instant) -> bool {
        let era = self.in_partenza.remove(volume).is_some();
        if era {
            self.prossimo_controllo = adesso;
        }
        era
    }

    /// `volume` non c'è più: le sue cartelle sono assenti. Quante.
    ///
    /// Come per un disco staccato che si scopre da sé (vedi
    /// [`lascia_cadere_le_morte`]): niente osservatore, niente riallineamenti —
    /// il passo si azzera —, e quando il volume torna il controllo di ogni minuto
    /// le riarma, il cambio di passo le riallinea, e una registrazione nuova
    /// prende il posto di quella che qui cade: Windows non la rivedrà.
    pub(super) fn volume_andato(&mut self, volume: &str, adesso: Instant) -> usize {
        self.in_partenza.remove(volume);
        self.registrazioni.remove(volume);
        let mut radici = Vec::new();
        for (radice, sorvegliata) in self
            .cartelle
            .iter_mut()
            .filter(|(_, sorvegliata)| sorvegliata.volume.as_deref() == Some(volume))
        {
            sorvegliata.osservatore = None;
            sorvegliata.come = Come::NonRaggiungibile;
            sorvegliata.senza_risposta = 0;
            radici.push(radice.clone());
        }
        for radice in &radici {
            self.piano.imposta(radice, Ritmo::Nessuno, adesso);
        }
        radici.len()
    }

    /// I volumi «in partenza» senza notizie da [`PARTENZA_MASSIMA`] tornano
    /// com'erano: si riarmano come ogni altra cartella, con una registrazione
    /// nuova — di quella vecchia non ci si fida più. Quanti.
    pub(super) fn scadute_le_partenze(&mut self, adesso: Instant) -> usize {
        let scadute: Vec<String> = self
            .in_partenza
            .iter()
            .filter(|(_, fino)| **fino <= adesso)
            .map(|(volume, _)| volume.clone())
            .collect();
        for volume in &scadute {
            self.in_partenza.remove(volume);
            self.registrazioni.remove(volume);
        }
        scadute.len()
    }

    /// Toglie le registrazioni che non servono più: quelle di un volume senza
    /// cartelle con l'osservatore, che non sia in partenza.
    ///
    /// Cade così anche la registrazione di una cartella tolta dall'elenco, di
    /// una sorveglianza spenta, di un osservatore morto con il suo disco: senza
    /// osservatore non c'è niente da lasciare. Quando il volume torna, la
    /// registrazione la rifà [`Self::registra_il_volume`] dopo il riarmo.
    fn pota_le_registrazioni(&mut self) {
        let cartelle = &self.cartelle;
        let in_partenza = &self.in_partenza;
        self.registrazioni.retain(|volume, _| {
            in_partenza.contains_key(volume)
                || cartelle.values().any(|sorvegliata| {
                    sorvegliata.osservatore.is_some()
                        && sorvegliata.volume.as_deref() == Some(volume.as_str())
                })
        });
    }

    /// Dopo che `radice` è stata armata con gli avvisi: se il suo volume non
    /// ha ancora una registrazione a Windows, la si fa.
    ///
    /// Non la si fa per un volume che sta partendo, né per una cartella senza
    /// osservatore o senza volume (di rete, del cloud, spenta: niente da
    /// lasciare). Se Windows non la accetta la sorveglianza resta com'era:
    /// niente va perso, solo l'espulsione sarà rifiutata come prima.
    fn registra_il_volume(&mut self, radice: &str, posta: &Sender<Messaggio>) {
        let Some(sorvegliata) = self.cartelle.get(radice) else {
            return;
        };
        let Some(volume) = sorvegliata
            .volume
            .clone()
            .filter(|_| sorvegliata.osservatore.is_some())
        else {
            return;
        };
        if self.registrazioni.contains_key(&volume) || self.in_partenza.contains_key(&volume) {
            return;
        }
        let chi_risponde = chi_avvisa(volume.clone(), posta.clone(), RISPOSTA_AL_VOLUME);
        match rymembra_barra::osserva_volume(&volume, chi_risponde) {
            Ok(registrazione) => {
                self.registrazioni.insert(volume, registrazione);
            }
            Err(guasto) => nota!(
                "[sorveglia] {} non avviserà quando il suo volume sta per essere espulso: {guasto}",
                nel_diario(radice, &self.elenco)
            ),
        }
    }

    /// Rimanda `cartelle` di `quanto` — se la sorveglianza non è spenta.
    fn rinvia(&mut self, cartelle: Vec<Cambiamento>, adesso: Instant, quanto: Duration) {
        if self.modo == Modo::Spenta {
            return;
        }
        self.attesa
            .rinvia(cartelle, adesso, quanto, PathRules::for_current_platform());
    }

    /// I rami persi di una passata che ha guardato `guardate`: tornano in
    /// attesa fra poco, finché non sono troppe volte (vedi
    /// [`Accorpatore::rami`]). Spenta, niente.
    ///
    /// Non quelli sotto una delle `vuote`: la guardia per radice ha lasciato
    /// stare quella cartella sorvegliata per intero, e riguardarne un ramo da
    /// solo — una sottocartella, che la guardia per radice non protegge — la
    /// giudicherebbe a pezzi.
    fn rami_persi(
        &mut self,
        guardate: &[String],
        persi: &[String],
        vuote: &[String],
        adesso: Instant,
    ) {
        if self.modo == Modo::Spenta {
            return;
        }
        let regole = PathRules::for_current_platform();
        let persi: Vec<String> = persi
            .iter()
            .filter(|ramo| !vuote.iter().any(|vuota| is_under(ramo, vuota, regole)))
            .cloned()
            .collect();
        let lasciati = self.attesa.rami(guardate, &persi, adesso, regole);
        for ramo in lasciati {
            nota!(
                "[sorveglia] {} non si legge da {RAMO_PERSO_TENTATIVI} tentativi: la si lascia stare, la riguarderanno i riallineamenti",
                nel_diario(&ramo, &self.elenco)
            );
        }
    }

    /// Il turno, e le cartelle da riguardare: in quest'ordine.
    ///
    /// Il turno prima: con «Scansiona» o la coda dei download in corso si
    /// riprova ogni [`RESPIRO`], e prima chiedere al disco cos'erano i percorsi
    /// voleva dire chiederlo ogni due secondi — sul NAS un giro di rete per
    /// cartella — per tutta la durata dell'altra scansione, e buttare la
    /// risposta. Il prezzo è il turno tenuto mentre si chiede: qualche
    /// millisecondo, al più [`NATURA_SCADENZA`] su una share che muore in quel
    /// momento, e chi lo vuole intanto aspetta.
    ///
    /// `None` col turno occupato — l'attesa resta com'è, e si riprova dopo un
    /// respiro — o con niente da guardare.
    ///
    /// [`RESPIRO`]: super::RESPIRO
    /// [`NATURA_SCADENZA`]: super::NATURA_SCADENZA
    pub(super) fn prepara_la_riscansione<'a>(
        &mut self,
        stato: &'a Stato,
        adesso: Instant,
        natura: fn(&Path) -> Natura,
    ) -> Option<(TurnoDiScansione<'a>, Vec<String>)> {
        let Some(turno) = stato.turno_di_scansione() else {
            self.piano.aspetta_un_respiro(adesso);
            return None;
        };
        let cambiati = self.attesa.prendi(adesso);
        // Tutte le cartelle dell'elenco e non solo quelle guardate: un avviso
        // arriva solo da una cartella guardata, e una richiesta di [`riguarda`]
        // vale con qualunque scelta.
        let da_guardare = cartelle_da_guardare(
            cambiati,
            self.elenco.clone(),
            PathRules::for_current_platform(),
            natura,
        );
        if da_guardare.is_empty() {
            return None;
        }
        Some((turno, da_guardare))
    }

    /// Quel che resta da fare dopo il riallineamento di `radice`: dirlo al
    /// pianificatore, e rimettere quel che aspettava sotto di lei
    /// (`assorbiti`) se non l'ha guardato.
    ///
    /// - Fermato a metà: la cartella si riprova, gli assorbiti tornano in
    ///   raffica.
    /// - Non ha risposto: la cartella si riprova, gli assorbiti col rinvio.
    ///   Lo stesso `adesso` per tutti e due, così su un NAS spento il
    ///   riallineamento e i suoi rinviati scadono insieme, e il primo si porta
    ///   via i secondi: una sonda ogni [`SONDAGGIO`], non due.
    /// - Ha risposto vuota: per il pianificatore ha risposto (vedi
    ///   [`Pianificatore::fatto`]), e gli assorbiti non tornano: li riguarda il
    ///   suo prossimo riallineamento, che ogni cartella guardata ha.
    /// - Fallito: come non ha risposto, ma gli assorbiti fra [`CONTROLLO`].
    ///
    /// E in ogni caso i suoi rami persi tornano in attesa per conto loro.
    pub(super) fn dopo_il_riallineamento(
        &mut self,
        radice: &str,
        assorbiti: Vec<Cambiamento>,
        passata: Passata,
        adesso: Instant,
    ) {
        match passata {
            Passata::Fatta { fermata: true, .. } => {
                nota!(
                    "[sorveglia] il riallineamento di {} si è fermato a metà: si riprova più tardi",
                    nel_diario(radice, &self.elenco)
                );
                self.piano.fatto(radice, false, adesso);
                self.attesa.rimetti(assorbiti, adesso);
            }
            Passata::Fatta {
                saltate,
                vuote,
                rami_persi,
                ..
            } => {
                let risposto = saltate.iter().all(|saltata| vuote.contains(saltata));
                if !risposto {
                    nota!(
                        "[sorveglia] il riallineamento di {} non è andato in porto: si riprova più tardi",
                        nel_diario(radice, &self.elenco)
                    );
                    self.rinvia(assorbiti, adesso, SONDAGGIO);
                } else if !vuote.is_empty() {
                    nota!(
                        "[sorveglia] {} risponde vuota: niente è stato tolto, la si riguarda al suo giro",
                        nel_diario(radice, &self.elenco)
                    );
                }
                self.piano.fatto(radice, risposto, adesso);
                let guardate: Vec<String> = if saltate.is_empty() {
                    vec![radice.to_owned()]
                } else {
                    Vec::new()
                };
                self.rami_persi(&guardate, &rami_persi, &vuote, adesso);
            }
            Passata::Fallita => {
                nota!(
                    "[sorveglia] il riallineamento di {} non è andato in porto: si riprova più tardi",
                    nel_diario(radice, &self.elenco)
                );
                self.piano.fatto(radice, false, adesso);
                self.rinvia(assorbiti, adesso, CONTROLLO);
            }
        }
    }

    /// Quel che resta da fare dopo la scansione di `da_guardare`.
    ///
    /// Quel che si è preso e non si è potuto guardare torna in attesa: con la
    /// scansione fermata a metà fra [`ACCORPAMENTO`]; con la scansione fallita
    /// fra [`CONTROLLO`]; con una cartella che non ha risposto fra
    /// [`SONDAGGIO`], allargata alla cartella sorvegliata che la contiene. Una
    /// sottocartella che non risponde è un disco spento, o una cartella sparita
    /// fra l'avviso e la camminata — un album spostato via per intero — e
    /// riprovarla identica, se è sparita, sarebbe un rinvio senza fine: la
    /// cartella sorvegliata intera è dove la si ritrova.
    ///
    /// Una cartella sorvegliata trovata vuota dalla guardia non torna: ha
    /// risposto, e la riguarda il suo riallineamento (vedi
    /// [`Pianificatore::fatto`]). I rami persi tornano per conto loro.
    ///
    /// [`ACCORPAMENTO`]: super::ACCORPAMENTO
    pub(super) fn dopo_la_riscansione(
        &mut self,
        da_guardare: &[String],
        passata: Passata,
        adesso: Instant,
    ) {
        let regole = PathRules::for_current_platform();
        self.piano.aspetta_un_respiro(adesso);
        match passata {
            Passata::Fatta { fermata: true, .. } => {
                self.attesa.rimetti(indietro(da_guardare), adesso);
            }
            Passata::Fatta {
                saltate,
                vuote,
                rami_persi,
                ..
            } => {
                for vuota in &vuote {
                    nota!(
                        "[sorveglia] {} risponde vuota: niente è stato tolto, la si riguarda al suo giro",
                        nel_diario(vuota, &self.elenco)
                    );
                }
                let allargate = piu_esterne(
                    saltate
                        .iter()
                        .filter(|saltata| !vuote.contains(*saltata))
                        .filter_map(|saltata| {
                            self.elenco
                                .iter()
                                .find(|radice| is_under(saltata, radice, regole))
                                .cloned()
                        })
                        .collect(),
                    regole,
                );
                self.rinvia(indietro(&allargate), adesso, SONDAGGIO);
                let guardate: Vec<String> = da_guardare
                    .iter()
                    .filter(|cartella| !saltate.contains(*cartella))
                    .cloned()
                    .collect();
                self.rami_persi(&guardate, &rami_persi, &vuote, adesso);
            }
            Passata::Fallita => self.rinvia(indietro(da_guardare), adesso, CONTROLLO),
        }
    }
}

fn filo(
    app: &AppHandle,
    posta: &Sender<Messaggio>,
    orecchio: &Receiver<Messaggio>,
    dell_avvio: Option<Vec<String>>,
) {
    let mut motore = Motore::nuovo(Instant::now());
    if let Some(radici) = dell_avvio {
        motore.piano.fotografa(radici);
    }
    let mut cadute_di_fila = 0_u32;
    loop {
        if crate::spegnimento::in_uscita() {
            return;
        }
        let attesa = prossima_attesa(&motore, Instant::now());
        // Il sospeso si misura qui e solo qui: attorno all'attesa a vuoto, mai
        // attorno a una scansione. Vedi `Pianificatore::sveglio`.
        motore.piano.dormo(attesa, SystemTime::now());
        let ricevuto = orecchio.recv_timeout(attesa);
        if motore.piano.sveglio(SystemTime::now()) {
            nota!("[sorveglia] risveglio dal sospeso: le cartelle si riallineano");
        }
        match ricevuto {
            Ok(Messaggio::Fermati) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(messaggio) => ricevi(&mut motore, messaggio, Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
        }
        // Dopo una caduta la posta si legge lo stesso — il risveglio finito,
        // una richiesta di riguardare, chi aspetta un controllo: niente va
        // perso — e il lavoro aspetta la fine della pausa.
        if motore.pausa.is_some_and(|fino| Instant::now() < fino) {
            continue;
        }
        motore.pausa = None;
        // Ogni giro dentro `sorvegliato`: un panico — un percorso che il
        // sistema consegna storto, un parser sotto la scansione — costa un
        // giro, non la sorveglianza per il resto della sessione.
        let giro = crate::stato::sorvegliato("sorveglia", || lavora(app, posta, &mut motore));
        if giro.is_some() {
            cadute_di_fila = 0;
        } else {
            cadute_di_fila = cadute_di_fila.saturating_add(1);
            let pausa = crate::stato::attesa_dopo_la_caduta(cadute_di_fila).min(CONTROLLO);
            motore.pausa = Some(Instant::now() + pausa);
        }
    }
}

/// Fino a quando dormire: il primo fra il prossimo controllo, gli avvisi
/// pronti e il prossimo riallineamento.
pub(super) fn prossima_attesa(motore: &Motore, adesso: Instant) -> Duration {
    // Mai un'attesa nulla in un ciclo: un decimo di secondo è il passo minimo.
    let minimo = Duration::from_millis(100);
    if let Some(fino) = motore.pausa {
        return fino.saturating_duration_since(adesso).max(minimo);
    }
    let mut attesa = motore.prossimo_controllo.saturating_duration_since(adesso);
    if motore.configurata
        && let Some(manca) = motore.attesa.manca(adesso)
    {
        // Gli avvisi pronti aspettano anche il respiro dopo l'ultima scansione.
        attesa = attesa.min(manca.max(motore.piano.fra_quanto_libero(adesso)));
    }
    if let Some(manca) = motore.piano.manca(adesso) {
        attesa = attesa.min(manca);
    }
    attesa.max(minimo)
}

/// Un messaggio arrivato: solo stato in memoria, niente disco.
fn ricevi(motore: &mut Motore, messaggio: Messaggio, adesso: Instant) {
    match messaggio {
        Messaggio::Avvisi(cambiati) => motore.attesa.avvisati(cambiati, adesso),
        Messaggio::Riguarda(cartelle) => motore.attesa.da_riguardare(cartelle, adesso),
        Messaggio::Guasto(radice) => {
            // Con `notify` 8.2 su Windows non arriva mai (vedi
            // `RIALLINEAMENTO_LOCALE`): quel che protegge davvero sono i
            // riallineamenti periodici. Se arriva, l'osservatore se ne va
            // subito, e si rifà al prossimo controllo e non adesso: un
            // osservatore che si guasta di continuo — una radice che il sistema
            // rifiuta — costa così un riallineamento al minuto, invece di un
            // giro senza fine fra guasto e riavvio. Rifatto, il cambio di passo
            // lo fa riallineare: quel che è successo intanto non l'ha visto
            // nessuno.
            nota!(
                "[sorveglia] l'osservatore di {} si è guastato: si rifà e la si riallinea",
                nel_diario(&radice, &motore.elenco)
            );
            if let Some(sorvegliata) = motore.cartelle.get_mut(&radice) {
                sorvegliata.osservatore = None;
                sorvegliata.come = Come::NonRaggiungibile;
                motore.piano.imposta(&radice, Ritmo::Nessuno, adesso);
            }
        }
        Messaggio::Riconfigura(chi) => {
            motore.chi_aspetta.extend(chi);
            motore.prossimo_controllo = adesso;
        }
        Messaggio::Svegliate => motore.piano.avvio(),
        Messaggio::VolumeVaVia { volume, risposta } => {
            let lasciate = motore.volume_in_partenza(&volume, adesso);
            nota!(
                "[sorveglia] Windows chiede di espellere un volume: {} cartelle lasciate",
                lasciate.cartelle
            );
            if lasciate.non_confermate > 0 {
                nota!(
                    "[sorveglia] la chiusura di {} cartelle non è stata confermata in {} secondi: l'espulsione potrebbe essere rifiutata",
                    lasciate.non_confermate,
                    CHIUSURA_ATTESA.as_secs()
                );
            }
            // Solo ora Windows può andare avanti. Chi aspettava può essersene
            // andato — la sua scadenza è passata — e non è un guasto.
            let _ = risposta.send(());
        }
        Messaggio::VolumeRifiutato(volume) => {
            if motore.volume_rifiutato(&volume, adesso) {
                nota!(
                    "[sorveglia] l'espulsione del volume è stata rifiutata: si riprende a guardarne le cartelle"
                );
            }
        }
        Messaggio::VolumeAndato(volume) => {
            let assenti = motore.volume_andato(&volume, adesso);
            nota!(
                "[sorveglia] il volume è stato espulso o staccato: {assenti} cartelle assenti, si riarmano quando torna"
            );
        }
        Messaggio::Fermati => {}
    }
}

/// Quel che si risponde a Windows quando ci avvisa di un volume: i messaggi
/// per il filo, e — per «sta per andarsene» — l'attesa della sua risposta.
///
/// Gira su un filo del sistema, e Windows aspetta il suo ritorno: l'attesa ha
/// la sua `scadenza`, e il filo — che può essere in mezzo a una scansione — non
/// può trattenerlo per sempre. Se il filo non c'è più (si esce) il messaggio
/// non parte, o cade senza risposta: la richiesta ritorna subito.
pub(super) fn chi_avvisa(
    volume: String,
    posta: Sender<Messaggio>,
    scadenza: Duration,
) -> impl Fn(Avviso) + Send + Sync + 'static {
    move |avviso| match avviso {
        Avviso::StaPerAndarsene => {
            let (risposto, aspetta) = channel();
            let messaggio = Messaggio::VolumeVaVia {
                volume: volume.clone(),
                risposta: risposto,
            };
            if posta.send(messaggio).is_ok() {
                // Scaduta, o con il lato che risponde caduto: si ritorna lo
                // stesso, vedi `RISPOSTA_AL_VOLUME`.
                let _ = aspetta.recv_timeout(scadenza);
            }
        }
        Avviso::Rifiutato => {
            let _ = posta.send(Messaggio::VolumeRifiutato(volume.clone()));
        }
        Avviso::Andato => {
            let _ = posta.send(Messaggio::VolumeAndato(volume.clone()));
        }
    }
}

/// Un giro di lavoro: il controllo se è ora, e al più una scansione.
fn lavora(app: &AppHandle, posta: &Sender<Messaggio>, motore: &mut Motore) {
    if Instant::now() >= motore.prossimo_controllo {
        controlla(app, posta, motore);
        motore.prossimo_controllo = Instant::now() + CONTROLLO;
        for chi in motore.chi_aspetta.drain(..) {
            let _ = chi.send(());
        }
    }
    if !motore.configurata {
        return;
    }
    // Prima il riallineamento: guarda tutta la cartella, e quel che è in
    // attesa sotto di lei se lo porta via.
    if let Some(voce) = motore.piano.prossima(Instant::now()) {
        riallinea(app, posta, motore, voce);
        return;
    }
    // Gli avvisi aspettano il respiro ma non il risveglio delle cartelle di
    // rete: un album copiato sul disco di casa non ha motivo di aspettare che
    // il NAS si svegli, e quel che arriva da lì intanto se lo porterà via il
    // riallineamento dell'avvio. Con la stessa condizione di
    // `prossima_attesa`, se no il filo girerebbe a vuoto ogni decimo di
    // secondo fino al risveglio.
    let adesso = Instant::now();
    if motore.piano.fra_quanto_libero(adesso).is_zero() && motore.attesa.pronto(adesso) {
        if motore.in_partenza.is_empty() {
            riscansiona(app, motore);
        } else {
            // Windows sta espellendo un volume, e una camminata delle sue
            // cartelle — non si sa quali: gli avvisi in attesa sono percorsi —
            // le terrebbe aperte. L'espulsione dura un secondo o due, e si
            // aspetta un respiro alla volta come col turno occupato; il tetto
            // è `PARTENZA_MASSIMA`.
            motore.piano.aspetta_un_respiro(adesso);
        }
    }
}

/// Rilegge cartelle e scelta, e rimette in riga gli osservatori.
fn controlla(app: &AppHandle, posta: &Sender<Messaggio>, motore: &mut Motore) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let letto = con_libreria(&stato, |l| {
        let radici: Vec<String> = rymembra_app::settings::read_json(
            &l.connection,
            rymembra_app::settings::CHIAVE_CARTELLE,
        )?
        .unwrap_or_default();
        let modo = rymembra_app::settings::read(&l.connection, CHIAVE_SORVEGLIANZA)?;
        Ok((radici, Modo::dal_salvato(modo.as_deref())))
    });
    let (radici, modo) = match letto {
        Ok(letto) => letto,
        Err(guasto) => {
            nota!("[sorveglia] cartelle non lette: {guasto}");
            return;
        }
    };
    let modo_cambiato = motore.imposta_modo(modo);
    let scadute = motore.scadute_le_partenze(Instant::now());
    if scadute > 0 {
        nota!(
            "[sorveglia] {scadute} volumi non hanno dato notizie dopo la richiesta di espulsione: si riprende a guardarne le cartelle"
        );
    }
    // Gli osservatori e i riallineamenti sono per le cartelle del disco e
    // della rete di casa. Una cartella del cloud non ha un `notify` da
    // armare né un `canonicalize` da chiedere: i suoi cambiamenti li porta il
    // filo del suo conto, che chiede le scansioni con [`riguarda`]. Resta però
    // nell'elenco qui sotto, perché è dall'elenco che una richiesta di
    // `riguarda` viene accettata.
    let locali = rymembra_domain::nuvola::solo_locali(&radici);
    // Via quel che non è più nell'elenco: l'osservatore cade con la voce.
    motore
        .cartelle
        .retain(|cartella, _| locali.contains(cartella));
    motore.piano.tieni_solo(&locali);
    if let Some(stato) = app.try_state::<StatoSorveglianza>() {
        stato.dove().retain(|(radice, _)| locali.contains(radice));
    }
    lascia_cadere_le_morte(motore);
    for radice in &locali {
        // Un volume che Windows sta per espellere non si riarma: riaprire la
        // cartella adesso farebbe rifiutare l'espulsione. Lo si riprende al
        // «rifiutato», o al ritorno del volume, o alla scadenza.
        if motore.e_in_partenza(radice) {
            continue;
        }
        // Si rifà quel che è nuovo, quel che non si è potuto guardare, e — con
        // la scelta cambiata — tutto. Una cartella col solo riallineamento
        // ritenta l'osservatore a ogni controllo: senza, una cartella che
        // all'avvio non rispondeva — un disco che girava ancora a vuoto —
        // restava così per tutta la sessione anche dopo essere tornata normale.
        let rifare = motore.cartelle.get(radice).is_none_or(|sorvegliata| {
            modo_cambiato || matches!(sorvegliata.come, Come::NonRaggiungibile | Come::Sondaggio)
        });
        if !rifare {
            continue;
        }
        let prima = motore
            .cartelle
            .get(radice)
            .map(|sorvegliata| sorvegliata.come);
        let sorvegliata = arma(app, radice, modo, posta, prima, &radici);
        motore
            .piano
            .imposta(radice, sorvegliata.ritmo(), Instant::now());
        motore.cartelle.insert(radice.clone(), sorvegliata);
        motore.registra_il_volume(radice, posta);
    }
    motore.pota_le_registrazioni();
    motore.elenco = radici;
    motore.configurata = true;
    motore.piano.elenco_letto();
    pubblica_quadro(app, motore);
}

/// Arma una cartella com'è giusto per la scelta: chiede dove sta, e se la si
/// vuole con gli avvisi prova l'osservatore.
///
/// Dove sta si chiede fuori da ogni lucchetto e con la scadenza: su una
/// lettera mappata che non risponde è un'attesa.
///
/// `prima` è come la si guardava finora: un osservatore che non si arma si
/// annota solo quando la cosa è nuova, se no un NAS spento per una giornata
/// scriverebbe la stessa riga ogni minuto.
fn arma(
    app: &AppHandle,
    radice: &str,
    modo: Modo,
    posta: &Sender<Messaggio>,
    prima: Option<Come>,
    elenco: &[String],
) -> Sorvegliata {
    let dove =
        rymembra_app::volumi::collocazione(radice, rymembra_app::volumi::SCADENZA_RISOLUZIONE);
    ricorda_dove(app, radice, dove);
    // Il volume si chiede solo per un disco di questo computer: per una
    // condivisione, una lettera che non risponde o il cloud non c'è un
    // volume da lasciare, e la registrazione a Windows non si fa.
    let con_volume = dove == Collocazione::Locale;
    let (come, osservatore, volume) = match come_sorvegliare(modo, dove) {
        Come::Avvisi => match osserva_entro(radice, posta, con_volume) {
            Ok((osservatore, volume)) => (Come::Avvisi, Some(osservatore), volume),
            Err(guasto) => {
                // Con «tutte» resta il solo riallineamento: è la strada che
                // funziona anche dove gli avvisi no. Con «locali», dove questa
                // è per forza un disco di questo computer, si riprova al
                // prossimo controllo.
                let ripiego = if modo == Modo::Tutte {
                    Come::Sondaggio
                } else {
                    Come::NonRaggiungibile
                };
                if prima != Some(ripiego) {
                    nota!(
                        "[sorveglia] {} non si può osservare: {guasto}",
                        nel_diario(radice, elenco)
                    );
                }
                (ripiego, None, None)
            }
        },
        altro => (altro, None, None),
    };
    Sorvegliata {
        come,
        dove,
        osservatore,
        volume,
        senza_risposta: 0,
    }
}

/// Scrive il quadro per la finestra.
fn pubblica_quadro(app: &AppHandle, motore: &Motore) {
    let Some(stato) = app.try_state::<StatoSorveglianza>() else {
        return;
    };
    let mut quadro = stato.quadro();
    quadro.modo = motore.modo.come_testo().to_owned();
    quadro.cartelle = motore
        .elenco
        .iter()
        .map(|radice| CartellaSorvegliata {
            percorso: radice.clone(),
            come: motore
                .cartelle
                .get(radice)
                .map_or(Come::Ferma, |sorvegliata| sorvegliata.come),
        })
        .collect();
}

/// Le cartelle con gli avvisi che non ci sono più, e quelle di rete che
/// tornano a rispondere.
///
/// # Perché bisogna andarle a cercare
///
/// Perché il loro osservatore muore **in silenzio**. In `notify` 8.2 su Windows
/// un completamento fallito di `ReadDirectoryChangesW` — il disco USB staccato,
/// la lettera che sparisce, la rete che cade — toglie la sorveglianza e lo
/// scrive in un registro che Rymembra non ascolta: il gestore qui sotto non
/// riceve nessun errore, il ramo `Guasto` non scatta, e la cartella resta «con
/// gli avvisi» senza che nessun avviso arrivi più.
///
/// Una domanda a ogni [`CONTROLLO`] — «c'è ancora?» — con [`VIVA_SCADENZA`].
///
/// - Un «non c'è» detto dal sistema fa cadere l'osservatore. Quando la
///   cartella torna, [`controlla`] la riarma, e il cambio di passo la fa
///   riallineare.
/// - Un «non si sa» (un disco che si sta svegliando, una rete giù) lo lascia
///   stare: farlo cadere e rifarlo costerebbe un riallineamento per un disco
///   lento. Ma una cartella di rete che è rimasta muta per
///   [`MANCATE_PER_TORNARE`] domande di fila e poi risponde è tornata: il suo
///   osservatore è con ogni probabilità morto con la rete, e la si riarma e
///   riallinea subito invece che al prossimo giro di [`RIALLINEAMENTO`].
///
/// [`RIALLINEAMENTO`]: super::RIALLINEAMENTO
fn lascia_cadere_le_morte(motore: &mut Motore) {
    let con_avvisi: Vec<String> = motore
        .cartelle
        .iter()
        .filter(|(_, sorvegliata)| sorvegliata.come == Come::Avvisi)
        .map(|(radice, _)| radice.clone())
        .collect();
    if con_avvisi.is_empty() {
        return;
    }
    let risposte = rymembra_app::volumi::esistenze(con_avvisi.clone(), VIVA_SCADENZA);
    let adesso = Instant::now();
    for (radice, risposta) in con_avvisi.iter().zip(risposte) {
        let Some(sorvegliata) = motore.cartelle.get_mut(radice) else {
            continue;
        };
        match risposta {
            Esistenza::Assente => {
                nota!(
                    "[sorveglia] {} non c'è più: la si riarma quando torna",
                    nel_diario(radice, &motore.elenco)
                );
                sorvegliata.osservatore = None;
                sorvegliata.come = Come::NonRaggiungibile;
                sorvegliata.senza_risposta = 0;
                motore.piano.imposta(radice, Ritmo::Nessuno, adesso);
            }
            Esistenza::Ignota => {
                sorvegliata.senza_risposta = sorvegliata.senza_risposta.saturating_add(1);
            }
            Esistenza::Presente => {
                if sorvegliata.senza_risposta >= MANCATE_PER_TORNARE
                    && sorvegliata.ritmo() == Ritmo::Rete
                {
                    nota!(
                        "[sorveglia] {} risponde di nuovo: la si riarma e riallinea",
                        nel_diario(radice, &motore.elenco)
                    );
                    motore.piano.tornata(radice);
                }
                sorvegliata.senza_risposta = 0;
            }
        }
    }
}

/// [`osserva`], con una scadenza.
///
/// `notify` chiede al disco se la cartella è un file o una cartella prima di
/// osservarla, e lo chiede senza scadenza: su una lettera mappata che Windows
/// sta ricollegando la risposta arrivava dopo un minuto — e per quel minuto il
/// filo della sorveglianza non faceva nient'altro, una volta al minuto finché il
/// NAS restava giù. Scaduto il tempo, la risposta che arriva dopo trova il
/// canale chiuso: l'osservatore nato tardi cade col filo che l'aspettava, e il
/// suo `Drop` chiude la sorveglianza. Niente resta appeso.
///
/// Con `con_volume` chiede anche a Windows il volume della cartella, sullo stesso
/// filo e con la stessa scadenza: per trovarlo Windows guarda le cartelle del
/// percorso, cioè il disco. `None` per una cartella di cui non si sa il volume:
/// resta sorvegliata, senza la registrazione.
fn osserva_entro(
    radice: &str,
    posta: &Sender<Messaggio>,
    con_volume: bool,
) -> Result<(Osservatore, Option<String>), String> {
    let percorso = radice.to_owned();
    let posta = posta.clone();
    rymembra_app::scadenza::con_scadenza(
        "osserva",
        rymembra_app::volumi::SCADENZA_RISOLUZIONE,
        // Il guasto senza i suoi percorsi: il `Display` di `notify` li
        // aggiunge in coda («… about ["D:\\Musica\\…"]»), e questa frase
        // finisce nel diario. Vedi [`nel_diario`].
        move || {
            let osservatore = osserva(&percorso, &posta)
                .map_err(|guasto| notify::Error::new(guasto.kind).to_string())?;
            let volume = if con_volume {
                rymembra_barra::volume_di(Path::new(&percorso))
            } else {
                None
            };
            Ok((osservatore, volume))
        },
    )
    .unwrap_or_else(|| {
        Err(format!(
            "la cartella non ha risposto entro {} secondi",
            rymembra_app::volumi::SCADENZA_RISOLUZIONE.as_secs()
        ))
    })
}

/// Mette un osservatore ricorsivo su una cartella, che scrive al filo.
pub(super) fn osserva(radice: &str, posta: &Sender<Messaggio>) -> notify::Result<Osservatore> {
    let posta = posta.clone();
    let nome = radice.to_owned();
    // La guardia sta dentro la funzione che `notify` tiene: cade quando `notify`
    // ha finito con la cartella, e il ricevitore se ne accorge. Vedi
    // [`Osservatore`].
    let (guardia, chiuso) = channel::<()>();
    let mut osservatore =
        notify::recommended_watcher(move |esito: notify::Result<notify::Event>| {
            // Letta, perché la funzione la catturi: non serve a altro.
            let _guardia = &guardia;
            let messaggio = match esito {
                // Il sistema ha perso degli avvisi — il suo buffer si è
                // riempito durante una copia enorme — e lo dice: non si sa più
                // cosa è cambiato, quindi si riguarda la radice intera.
                // L'osservatore funziona, e resta: quel che arriva dopo lo
                // vede. Questo e l'`Err` qui sotto, con `notify` 8.2 su
                // Windows, non arrivano mai: l'osservatore muore senza dirlo,
                // e lo rifanno i riallineamenti periodici (vedi
                // `RIALLINEAMENTO_LOCALE`). Restano per chi lo dicesse.
                Ok(evento) if evento.need_rescan() => Messaggio::Riguarda(vec![nome.clone()]),
                Ok(evento) => {
                    let cambiati = cambiamenti_dell_avviso(&evento, &natura_di);
                    if cambiati.is_empty() {
                        return;
                    }
                    Messaggio::Avvisi(cambiati)
                }
                Err(_) => Messaggio::Guasto(nome.clone()),
            };
            let _ = posta.send(messaggio);
        })?;
    osservatore.watch(Path::new(radice), notify::RecursiveMode::Recursive)?;
    Ok(Osservatore {
        dentro: osservatore,
        chiuso,
    })
}

/// Un riallineamento: l'osservatore rifatto se serve, e la cartella intera
/// scansionata, se il turno è libero.
fn riallinea(
    app: &AppHandle,
    posta: &Sender<Messaggio>,
    motore: &mut Motore,
    voce: Riallineamento,
) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let radice = voce.radice;
    if motore.e_in_partenza(&radice) {
        // Windows sta per espellere il volume: né riarmare né camminare, che
        // riaprirebbe le sue cartelle. La cartella resta dovuta, e si riprova
        // dopo un respiro — se l'espulsione è rifiutata la si trova ancora
        // lì, se è riuscita è assente.
        motore.piano.aspetta_un_respiro(Instant::now());
        return;
    }
    if voce.riarma && motore.cartelle.contains_key(&radice) {
        // Il nuovo prima della camminata — vedi `Riallineamento::riarma` — e
        // il vecchio cade quando il nuovo prende il suo posto. Se il nuovo non
        // si arma, la cartella passa al solo riallineamento, e si cammina lo
        // stesso.
        let prima = motore.cartelle.get(&radice).map(|vecchia| vecchia.come);
        let nuova = arma(app, &radice, motore.modo, posta, prima, &motore.elenco);
        let ritmo = nuova.ritmo();
        motore.piano.imposta(&radice, ritmo, Instant::now());
        let cambiata = motore
            .cartelle
            .get(&radice)
            .is_none_or(|vecchia| vecchia.come != nuova.come);
        motore.cartelle.insert(radice.clone(), nuova);
        motore.registra_il_volume(&radice, posta);
        if cambiata {
            pubblica_quadro(app, motore);
        }
        if !ritmo.guardata() {
            return;
        }
    }
    let Some(turno) = stato.turno_di_scansione() else {
        // Il turno è di qualcun altro: la cartella resta dovuta, e si riprova
        // dopo un respiro.
        motore.piano.aspetta_un_respiro(Instant::now());
        return;
    };
    if crate::spegnimento::in_uscita() {
        return;
    }
    stato.riprendi_scansioni();
    // Quel che aspettava sotto questa cartella lo guarda il riallineamento:
    // va via adesso, prima della camminata, e torna se la camminata non va in
    // porto (vedi `Motore::dopo_il_riallineamento`). Quel che arriva durante
    // resta in attesa per conto suo.
    let assorbiti = motore
        .attesa
        .togli_sotto(&radice, PathRules::for_current_platform());
    let passata = esegui(
        app,
        &stato,
        std::slice::from_ref(&radice),
        turno,
        "riallineamento",
    );
    motore.dopo_il_riallineamento(&radice, assorbiti, passata, Instant::now());
}

/// La scansione delle cartelle in attesa, se il turno è libero.
///
/// Il turno si chiede **prima** di decidere le cartelle, e col turno occupato
/// non si tocca né l'attesa né il disco: vedi
/// [`Motore::prepara_la_riscansione`]. Quel che si è preso e non si è potuto
/// guardare torna in attesa come dice [`Motore::dopo_la_riscansione`].
fn riscansiona(app: &AppHandle, motore: &mut Motore) {
    let Some(stato) = app.try_state::<Stato>() else {
        return;
    };
    let Some((turno, da_guardare)) =
        motore.prepara_la_riscansione(&stato, Instant::now(), natura_di)
    else {
        return;
    };
    if crate::spegnimento::in_uscita() {
        motore
            .attesa
            .rimetti(indietro(&da_guardare), Instant::now());
        return;
    }
    stato.riprendi_scansioni();
    let passata = esegui(app, &stato, &da_guardare, turno, "sorveglianza");
    motore.dopo_la_riscansione(&da_guardare, passata, Instant::now());
}

/// Com'è andata una scansione automatica.
pub(super) enum Passata {
    /// È arrivata in fondo, o si è fermata a metà perché qualcuno l'ha chiesto.
    Fatta {
        /// Le cartelle che non hanno risposto: niente di loro è stato giudicato.
        /// Comprende le `vuote`.
        saltate: Vec<String>,
        /// Fra le `saltate`, le cartelle sorvegliate che hanno risposto vuote
        /// e la guardia per radice ha lasciato stare: hanno risposto, e non si
        /// riprovano come quelle che non l'hanno fatto.
        vuote: Vec<String>,
        /// Le sottocartelle che non si sono lette, dentro cartelle che
        /// rispondevano: il resto è stato giudicato, loro no.
        rami_persi: Vec<String>,
        /// Si è fermata a metà: quel che ha fatto è scritto, il resto no.
        fermata: bool,
    },
    /// Non è partita, o è fallita: niente di quel che doveva guardare si può
    /// dare per guardato.
    Fallita,
}

/// La scansione vera, con il turno già in mano. Il turno cade alla fine, e
/// sveglia la manutenzione se la scansione ha cambiato qualcosa.
fn esegui(
    app: &AppHandle,
    stato: &Stato,
    cartelle: &[String],
    turno: TurnoDiScansione<'_>,
    origine: &'static str,
) -> Passata {
    let copertine = match stato.copertine() {
        Ok(copertine) => copertine,
        Err(guasto) => {
            nota!("[sorveglia] la scansione non parte: {guasto}");
            return Passata::Fallita;
        }
    };
    let fermati = || stato.scansione_fermata() || crate::spegnimento::in_uscita();
    // Le cartelle sorvegliate, per la guardia per radice: vale su quelle e non
    // sulle sottocartelle che si riguardano qui. Se non si leggono, tutte le
    // cartelle di questa passata valgono sorvegliate — la guardia scatta di
    // più, e nel dubbio non si toglie.
    let radici_della_libreria: Vec<String> = con_libreria(stato, |l| {
        rymembra_app::settings::read_json::<Vec<String>>(
            &l.connection,
            rymembra_app::settings::CHIAVE_CARTELLE,
        )
    })
    .ok()
    .flatten()
    .unwrap_or_else(|| cartelle.to_vec());
    let scan = Scan {
        // Il disco e il cloud: le radici di OneDrive camminano sullo specchio.
        files: crate::nuvola_musica::archivi_per_la_scansione(app),
        covers: copertine,
        roots: cartelle,
        radici_della_libreria: &radici_della_libreria,
        rules: PathRules::for_current_platform(),
        // Nessuno la sta guardando: vedi il preambolo di `sorveglia.rs`.
        prudente: true,
        scadenze: Scadenze::default(),
        fermati: Some(&fermati),
    };
    let mut deposito = DepositoCheGuarda {
        dentro: DepositoStato::nuovo(stato),
        scritto: false,
    };
    let report = match scan.run_su(&mut deposito, |_, _| std::ops::ControlFlow::Continue(())) {
        Ok(report) => report,
        Err(guasto) => {
            nota!(
                "[sorveglia] la scansione di {} cartelle ({origine}) è fallita: {guasto}",
                cartelle.len()
            );
            // Fallita dopo aver scritto: i lotti chiusi sono in libreria anche
            // se l'errore non porta i conti, e la finestra deve rileggerli.
            if deposito.scritto {
                libreria_cambiata(app, &turno);
            }
            return Passata::Fallita;
        }
    };
    crate::comandi::racconta_scansione(origine, &report);
    // I brani che questa passata ha tolto escono anche dalla coda, come per
    // «Togli» (R23-8): il lettore tiene la scheda di quello che suona, e la
    // coda non lo ripropone. Dopo la scansione, e fuori dal lucchetto della
    // libreria: sgomberare la coda la salva, e salvarla lo prende.
    if report.removed > 0 {
        let candidati: Vec<i64> = report.plan.to_remove.iter().map(|r| r.track.id).collect();
        match con_libreria(stato, |l| spariti(&l.connection, &candidati)) {
            Ok(tolti) if !tolti.is_empty() => crate::cancellazione::sgombera(app, &tolti, false),
            Ok(_) => {}
            Err(guasto) => {
                nota!("[sorveglia] i brani tolti non si ritrovano per la coda: {guasto}")
            }
        }
    }
    crate::nuvola_musica::rimetti_in_fila(app, cartelle, report.rimasti);
    // Le vuote stanno anche fra le saltate, ma hanno risposto: le annuncia
    // chi decide se riprovarle.
    let mute = report
        .radici_saltate
        .len()
        .saturating_sub(report.radici_vuote.len());
    if mute > 0 {
        nota!(
            "[sorveglia] {mute} cartelle non hanno risposto, i loro brani non sono stati giudicati"
        );
    }
    if report.rimozioni_rinviate > 0 {
        nota!(
            "[sorveglia] {} rimozioni trattenute dalla guardia, niente è stato tolto",
            report.rimozioni_rinviate
        );
    }
    let aggiornati = report.updated.saturating_add(report.moved);
    if let Some(sorveglianza) = app.try_state::<StatoSorveglianza>() {
        sorveglianza.quadro().ultima = Some(UltimaPassata {
            quando_ms: crate::stato::adesso_ms(),
            cartelle: cartelle.len(),
            inseriti: report.inserted,
            aggiornati,
            tolti: report.removed,
            rinviate: report.rimozioni_rinviate,
        });
    }
    let cambiato = report
        .inserted
        .saturating_add(aggiornati)
        .saturating_add(report.removed)
        > 0;
    if cambiato {
        libreria_cambiata(app, &turno);
    }
    Passata::Fatta {
        saltate: report.radici_saltate,
        vuote: report.radici_vuote,
        rami_persi: report.rami_persi,
        fermata: report.cancelled,
    }
}

/// I brani di `candidati` che la libreria non ha più.
///
/// Il piano dice quali righe la passata **voleva** togliere; la guardia ne può
/// aver trattenute, e una riga tolta solo se il percorso era ancora quello. Si
/// chiede al database, così dalla coda esce solo quel che dalla libreria è
/// uscito davvero.
pub(super) fn spariti(
    conn: &rusqlite::Connection,
    candidati: &[i64],
) -> Result<Vec<i64>, AppError> {
    let errore = |err: rusqlite::Error| {
        rymembra_app::db::codice_da_sqlite("brani tolti dalla sorveglianza", &err)
    };
    let mut c_e = conn
        .prepare_cached("SELECT 1 FROM tracks WHERE id = ?1")
        .map_err(errore)?;
    let mut fuori = Vec::new();
    for &id in candidati {
        if !c_e.exists([id]).map_err(errore)? {
            fuori.push(id);
        }
    }
    Ok(fuori)
}

/// Quel che si fa quando una scansione automatica ha cambiato la libreria.
///
/// Il turno lo sa — cadendo sveglierà la manutenzione — e la finestra riceve
/// **un** evento: `cartelle:cambiate`, che le fa rileggere viste e albero.
/// Fino alla 3.0.1 ne riceveva due, perché si mandava anche quello dei
/// download (`scarico:in_libreria`), che resta ai download. Poi i brani nuovi
/// vanno arricchiti, misurati e salvati. Una scansione che non ha cambiato
/// niente non sveglia nessuno di questi.
fn libreria_cambiata(app: &AppHandle, turno: &TurnoDiScansione<'_>) {
    turno.segna_cambiamenti();
    crate::cartelle::cambiate(app);
    crate::arricchimento::sporca(app);
    crate::analisi::sporca(app);
    crate::nuvola::sporca(app);
}

/// Il deposito della libreria, che in più si accorge se qualcosa è stato
/// scritto.
///
/// Serve al caso in cui la scansione fallisce **dopo** aver scritto: l'errore
/// non porta i conti, ma i lotti già chiusi sono in libreria. Si guarda il
/// contatore delle modifiche di SQLite prima e dopo ogni presa, con il
/// lucchetto in mano: conta solo quel che ha scritto la scansione, non gli
/// altri che usano la connessione fra una presa e l'altra.
struct DepositoCheGuarda<'a> {
    /// Il deposito vero.
    dentro: DepositoStato<'a>,
    /// Almeno una presa ha cambiato qualcosa.
    scritto: bool,
}

impl Deposito for DepositoCheGuarda<'_> {
    fn con_connessione<T, F>(&mut self, azione: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> Result<T, AppError>,
    {
        let mut scritto = false;
        let esito = self.dentro.con_connessione(|connessione| {
            let prima = connessione.total_changes();
            let esito = azione(connessione);
            scritto = connessione.total_changes() != prima;
            esito
        });
        self.scritto |= scritto;
        esito
    }
}

/// Come una cartella della musica si nomina nel diario: mai per nome.
///
/// `PRIVACY.md` e la testa di `diario.rs` promettono che nel diario non
/// finiscono i percorsi della musica, e questo modulo era l'unico che ce li
/// scriveva: un ramo che non si legge sul NAS è
/// `\\NAS\Musica\Fabrizio De André\Anime salve`, e una cartella sorvegliata
/// porta spesso il nome di chi la tiene. Chi allega il diario a una
/// segnalazione allegava nomi d'artista e d'album.
///
/// Al posto del nome: la posizione nell'elenco delle cartelle sorvegliate
/// («cartella 2»), quanti livelli sotto, e un'impronta corta. La posizione
/// cambia se l'elenco cambia; l'impronta no, finché il processo vive — è lei
/// che permette di riconoscere la stessa cartella fra due righe. Viene da una
/// chiave casuale presa all'avvio, quindi non si confronta fra due sessioni
/// né si ricava provando nomi di cartelle note. Si calcola sulla chiave della
/// scansione (`path_key`), così due grafie dello stesso posto danno la stessa.
pub(super) fn nel_diario(percorso: &str, elenco: &[String]) -> String {
    use std::hash::BuildHasher as _;
    static CHIAVE: std::sync::OnceLock<std::collections::hash_map::RandomState> =
        std::sync::OnceLock::new();
    let regole = PathRules::for_current_platform();
    let chiave = path_key(percorso, regole);
    let impronta = CHIAVE
        .get_or_init(std::collections::hash_map::RandomState::new)
        .hash_one(&chiave);
    let impronta = format!("{:08x}", impronta & 0xFFFF_FFFF);
    let dentro = elenco
        .iter()
        .enumerate()
        .find(|(_, radice)| is_under(percorso, radice, regole));
    match dentro {
        Some((indice, radice)) => {
            let livelli = chiave
                .get(path_key(radice, regole).len()..)
                .unwrap_or_default()
                .split('/')
                .filter(|pezzo| !pezzo.is_empty())
                .count();
            let quale = indice.saturating_add(1);
            if livelli == 0 {
                format!("cartella {quale} [{impronta}]")
            } else {
                format!("una sottocartella a {livelli} livelli dalla cartella {quale} [{impronta}]")
            }
        }
        None => format!("una cartella [{impronta}]"),
    }
}
