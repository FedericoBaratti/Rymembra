//! Gli avvisi del sistema, accorpati in raffiche e rinvii, e le cartelle che ne
//! vengono da riguardare. Fa parte di [`crate::sorveglia`]: i tempi stanno là.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use rymembra_app::library::{Cambiamento, Natura, cartelle_da_riguardare};
use rymembra_domain::paths::{PathRules, extension_of, is_supported_audio_path, is_under};

use crate::nota;

use super::{ACCORPAMENTO, ATTESA_MASSIMA, NATURA_SCADENZA, RAMO_PERSO, RAMO_PERSO_TENTATIVI};

// ── l'accorpamento ──────────────────────────────────────────────────────────

/// Gli avvisi arrivati e non ancora scansionati.
///
/// Puro, con l'orologio passato da fuori: le prove gli danno degli istanti
/// inventati invece di aspettare due secondi veri.
#[derive(Debug, Default)]
pub struct Accorpatore {
    /// La raffica in corso: i percorsi che il sistema ha nominato — file,
    /// cartelle, vecchi nomi — e le cartelle che qualcuno ha chiesto di
    /// riguardare.
    cambiati: Vec<Cambiamento>,
    /// Il primo avviso della raffica in corso.
    primo: Option<Instant>,
    /// L'ultimo.
    ultimo: Option<Instant>,
    /// Quel che una scansione non ha potuto guardare perché il disco non ha
    /// risposto, perché una sottocartella non si è letta, o perché è fallita;
    /// ognuno con l'istante in cui riprovarlo.
    ///
    /// Aspetta il suo rinvio e non la raffica: riprovarlo a ogni raffica
    /// vorrebbe dire bussare a un NAS spento ogni volta che si copia un album
    /// sul disco di casa. E il **suo**, non uno per tutti: con un rinvio
    /// comune, il NAS spento che torna indietro ogni cinque minuti spostava
    /// avanti anche l'album rinviato sul disco di casa — per sempre.
    rinviati: Vec<(Cambiamento, Instant)>,
    /// I rami persi che si stanno riguardando da sé, e quante volte di fila si
    /// sono persi: vedi [`Self::rami`].
    rami: Vec<(String, u32)>,
}

impl Accorpatore {
    /// Cambiamenti nominati da un avviso.
    pub fn avvisati(&mut self, cambiati: impl IntoIterator<Item = Cambiamento>, adesso: Instant) {
        let prima = self.cambiati.len();
        self.cambiati.extend(cambiati);
        if self.cambiati.len() > prima {
            self.primo.get_or_insert(adesso);
            self.ultimo = Some(adesso);
        }
    }

    /// Cartelle da riguardare per intero.
    ///
    /// Entrano come cambiamenti **nuovi**, cioè «se è una cartella, guardala da
    /// sé»: è esattamente quel che si chiede, e se nel frattempo la cartella è
    /// sparita si risale a quella che la conteneva invece di saltarla.
    pub fn da_riguardare(&mut self, cartelle: impl IntoIterator<Item = String>, adesso: Instant) {
        self.avvisati(
            cartelle.into_iter().map(|percorso| Cambiamento {
                percorso,
                nuovo: true,
            }),
            adesso,
        );
    }

    /// La raffica tace da [`ACCORPAMENTO`], o dura da [`ATTESA_MASSIMA`].
    fn raffica_pronta(&self, adesso: Instant) -> bool {
        match (self.primo, self.ultimo) {
            (Some(primo), Some(ultimo)) => {
                adesso.saturating_duration_since(ultimo) >= ACCORPAMENTO
                    || adesso.saturating_duration_since(primo) >= ATTESA_MASSIMA
            }
            _ => false,
        }
    }

    /// Il rinvio di almeno un rinviato è passato.
    fn rinvio_scaduto(&self, adesso: Instant) -> bool {
        self.rinviati.iter().any(|(_, quando)| *quando <= adesso)
    }

    /// È il momento di scansionare: la raffica è finita, o il rinvio è passato.
    #[must_use]
    pub fn pronto(&self, adesso: Instant) -> bool {
        self.raffica_pronta(adesso) || self.rinvio_scaduto(adesso)
    }

    /// Quanto manca a [`Self::pronto`]. `None` se non c'è niente in attesa.
    #[must_use]
    pub fn manca(&self, adesso: Instant) -> Option<Duration> {
        let per_la_raffica = match (self.primo, self.ultimo) {
            (Some(primo), Some(ultimo)) => {
                let per_silenzio = (ultimo + ACCORPAMENTO).saturating_duration_since(adesso);
                let per_tetto = (primo + ATTESA_MASSIMA).saturating_duration_since(adesso);
                Some(per_silenzio.min(per_tetto))
            }
            _ => None,
        };
        let per_il_rinvio = self
            .rinviati
            .iter()
            .map(|(_, quando)| quando.saturating_duration_since(adesso))
            .min();
        match (per_la_raffica, per_il_rinvio) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Quel che è pronto, e da lì l'attesa ricomincia.
    ///
    /// Solo quel che è pronto: una raffica ancora in corso aspetta la sua fine
    /// anche se intanto è scaduto un rinvio, e viceversa; e dei rinviati, solo
    /// quelli il cui rinvio è passato.
    pub fn prendi(&mut self, adesso: Instant) -> Vec<Cambiamento> {
        let mut presi = Vec::new();
        if self.raffica_pronta(adesso) {
            presi.append(&mut self.cambiati);
            self.primo = None;
            self.ultimo = None;
        }
        if self.rinvio_scaduto(adesso) {
            let (scaduti, restano): (Vec<_>, Vec<_>) = std::mem::take(&mut self.rinviati)
                .into_iter()
                .partition(|(_, quando)| *quando <= adesso);
            self.rinviati = restano;
            presi.extend(scaduti.into_iter().map(|(cambiamento, _)| cambiamento));
        }
        presi
    }

    /// Rimette in attesa quel che non si è potuto scansionare per una ragione
    /// che passa presto — una scansione fermata a metà — e ci si riprova fra
    /// [`ACCORPAMENTO`]. Il turno di qualcun altro invece non tocca l'attesa:
    /// vedi [`Motore::prepara_la_riscansione`].
    ///
    /// [`Motore::prepara_la_riscansione`]: super::filo::Motore::prepara_la_riscansione
    pub fn rimetti(&mut self, cambiati: Vec<Cambiamento>, adesso: Instant) {
        if cambiati.is_empty() {
            return;
        }
        self.cambiati.extend(cambiati);
        // Da capo tutte e due: con il primo vecchio il tetto sarebbe già
        // superato, e si girerebbe a vuoto contro un turno occupato.
        self.primo = Some(adesso);
        self.ultimo = Some(adesso);
    }

    /// Rimanda di `quanto` quel che non si è potuto scansionare perché il disco
    /// non ha risposto, o perché la scansione è fallita.
    ///
    /// Ognuno col suo rinvio: quel che sta su un disco spento non trattiene
    /// quel che sta su un altro. Lo stesso percorso già in attesa resta una
    /// voce sola, e vince il rinvio più lontano: è fermo sullo stesso disco di
    /// quel che è appena tornato indietro, e riprovarlo prima sarebbe bussare
    /// due volte.
    pub fn rinvia(
        &mut self,
        cambiati: Vec<Cambiamento>,
        adesso: Instant,
        quanto: Duration,
        regole: PathRules,
    ) {
        let quando = adesso + quanto;
        for cambiamento in cambiati {
            let stesso = |gia: &Cambiamento| {
                gia.nuovo == cambiamento.nuovo
                    && is_under(&gia.percorso, &cambiamento.percorso, regole)
                    && is_under(&cambiamento.percorso, &gia.percorso, regole)
            };
            match self.rinviati.iter_mut().find(|(gia, _)| stesso(gia)) {
                Some((_, gia_quando)) => *gia_quando = (*gia_quando).max(quando),
                None => self.rinviati.push((cambiamento, quando)),
            }
        }
    }

    /// Lascia cadere tutti i rinviati, e il conto dei rami persi, e dice quanti
    /// rinviati erano: la sorveglianza è stata spenta, e da spenta niente si
    /// riprova da sé. La raffica resta: può portare una richiesta di
    /// [`riguarda`], che vale anche da spenta.
    ///
    /// [`riguarda`]: super::riguarda
    pub fn svuota_rinviati(&mut self) -> usize {
        self.rami.clear();
        std::mem::take(&mut self.rinviati).len()
    }

    /// Com'è andata coi rami persi una passata che ha guardato `guardate` — le
    /// sue cartelle che hanno risposto — e non ha potuto leggere `persi`.
    ///
    /// Un ramo perso torna in attesa fra [`RAMO_PERSO`], e fra il doppio ogni
    /// volta che si riperde, per [`RAMO_PERSO_TENTATIVI`] volte; poi lo si
    /// lascia stare, e lo si restituisce — una volta sola — perché finisca nel
    /// diario. Lasciato stare resta ricordato: ogni riallineamento della sua
    /// cartella sorvegliata lo riperderebbe, e senza memoria ricomincerebbe il
    /// giro da un minuto, per sempre. Si dimentica quando una passata lo guarda
    /// e lo legge: sta sotto una cartella guardata, e non sotto un ramo perso
    /// adesso.
    pub fn rami(
        &mut self,
        guardate: &[String],
        persi: &[String],
        adesso: Instant,
        regole: PathRules,
    ) -> Vec<String> {
        self.rami.retain(|(ramo, _)| {
            !guardate
                .iter()
                .any(|cartella| is_under(ramo, cartella, regole))
                || persi.iter().any(|perso| is_under(ramo, perso, regole))
        });
        let mut lasciati = Vec::new();
        for perso in persi {
            let posto = self.rami.iter().position(|(ramo, _)| {
                is_under(ramo, perso, regole) && is_under(perso, ramo, regole)
            });
            let volte = match posto.and_then(|posto| self.rami.get_mut(posto)) {
                Some((_, volte)) => {
                    *volte = volte.saturating_add(1);
                    *volte
                }
                None => {
                    self.rami.push((perso.clone(), 1));
                    1
                }
            };
            if volte > RAMO_PERSO_TENTATIVI {
                if volte == RAMO_PERSO_TENTATIVI.saturating_add(1) {
                    lasciati.push(perso.clone());
                }
                continue;
            }
            // Uno, due, quattro... minuti: `volte` non supera il tetto, e
            // il doppio del doppio resta piccolo.
            let attesa = RAMO_PERSO.saturating_mul(2_u32.saturating_pow(volte.saturating_sub(1)));
            self.rinvia(
                vec![Cambiamento {
                    percorso: perso.clone(),
                    nuovo: true,
                }],
                adesso,
                attesa,
                regole,
            );
        }
        lasciati
    }

    /// Toglie dall'attesa e restituisce tutto quel che sta sotto `radice`, in
    /// raffica o in rinvio: un riallineamento della radice lo guarda già.
    ///
    /// Chi lo chiama lo rimette se il riallineamento non va in porto.
    pub fn togli_sotto(&mut self, radice: &str, regole: PathRules) -> Vec<Cambiamento> {
        let sotto = |cambiamento: &Cambiamento| is_under(&cambiamento.percorso, radice, regole);
        let (mut tolti, restano): (Vec<_>, Vec<_>) = std::mem::take(&mut self.cambiati)
            .into_iter()
            .partition(&sotto);
        self.cambiati = restano;
        if self.cambiati.is_empty() {
            self.primo = None;
            self.ultimo = None;
        }
        let (rinviati_sotto, restano): (Vec<_>, Vec<_>) = std::mem::take(&mut self.rinviati)
            .into_iter()
            .partition(|(cambiamento, _)| sotto(cambiamento));
        self.rinviati = restano;
        tolti.extend(
            rinviati_sotto
                .into_iter()
                .map(|(cambiamento, _)| cambiamento),
        );
        tolti
    }
}

/// I file che non sono musica e cambiano spesso accanto alla musica: le
/// copertine, i testi che Rymembra stesso può scrivere accanto ai brani, le
/// liste, i file temporanei di una copia o di un download.
///
/// Un avviso su uno di questi non vale una scansione. Un nome sconosciuto
/// invece sì: è meglio una camminata in più che un album che non compare.
///
/// Il `.cue` non c'è più dalla 3.0.2: è il foglio di un'immagine CD, e
/// cambiarlo cambia i brani che l'immagine diventa in libreria.
const NON_MUSICA: &[&str] = &[
    "jpg",
    "jpeg",
    "png",
    "gif",
    "bmp",
    "webp",
    "lrc",
    "txt",
    "nfo",
    "log",
    "m3u",
    "m3u8",
    "pls",
    "xspf",
    "ini",
    "db",
    "tmp",
    "part",
    "crdownload",
    "partial",
    "sfv",
    "md5",
    "accurip",
];

/// Un avviso su questo percorso vale una scansione?
#[must_use]
pub fn conta(percorso: &Path) -> bool {
    let Some(estensione) = percorso.extension().and_then(|e| e.to_str()) else {
        return true;
    };
    !NON_MUSICA
        .iter()
        .any(|non| estensione.eq_ignore_ascii_case(non))
}

/// I cambiamenti che un avviso del sistema racconta.
///
/// - Creato, o rinominato **verso** questo nome: nuovo. Se è una cartella la si
///   guarderà da sé — un album copiato è la sua cartella, non quella
///   dell'artista.
/// - Una modifica di contenuto, di metadati o generica su una **cartella** si
///   salta. Lo stato di una cartella cambia perché è cambiato un figlio, e il
///   figlio ha già il suo avviso: tenerla vorrebbe dire riguardare il genitore
///   per un file che sta un piano più giù. Sul NAS dell'utente arriva a ogni
///   file copiato.
/// - Tutto il resto — tolto, rinominato **da**, una modifica di un file — non
///   è nuovo: si guarda dalla cartella che lo contiene.
///
/// `natura` è chiesta solo per le modifiche, cioè dove serve a decidere.
pub fn cambiamenti_dell_avviso(
    evento: &notify::Event,
    natura: &dyn Fn(&Path) -> Natura,
) -> Vec<Cambiamento> {
    use notify::EventKind;
    use notify::event::{ModifyKind, RenameMode};
    let cambiamento = |percorso: &Path, nuovo: bool| Cambiamento {
        percorso: percorso.to_string_lossy().into_owned(),
        nuovo,
    };
    let tutti = |nuovo: bool| -> Vec<Cambiamento> {
        evento
            .paths
            .iter()
            .filter(|percorso| conta(percorso.as_path()))
            .map(|percorso| cambiamento(percorso.as_path(), nuovo))
            .collect()
    };
    match evento.kind {
        EventKind::Access(_) => Vec::new(),
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => tutti(true),
        // Un nome solo per due percorsi: il primo è quello vecchio.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => evento
            .paths
            .iter()
            .enumerate()
            .filter(|(_, percorso)| conta(percorso.as_path()))
            .map(|(posto, percorso)| cambiamento(percorso.as_path(), posto > 0))
            .collect(),
        EventKind::Modify(
            ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Other,
        ) => evento
            .paths
            .iter()
            .filter(|percorso| {
                conta(percorso.as_path()) && natura(percorso.as_path()) != Natura::Cartella
            })
            .map(|percorso| cambiamento(percorso.as_path(), false))
            .collect(),
        _ => tutti(false),
    }
}

/// Che cos'è un percorso, chiesto al disco — ma non per i file di musica.
///
/// Un nome con l'estensione di un brano o di un foglio `.cue` si prende per un
/// file senza chiedere: è la quasi totalità degli avvisi di una copia, e sul NAS
/// ogni domanda è un giro di rete. Una cartella che si chiamasse `Disco.flac`
/// verrebbe presa per un file, e si guarderebbe dalla cartella che la contiene:
/// una scansione un po' più larga, non un brano perso.
///
/// Senza seguire i collegamenti, come la scansione: un collegamento non è una
/// cartella da camminare.
pub(super) fn natura_di(percorso: &Path) -> Natura {
    let testo = percorso.to_string_lossy();
    if is_supported_audio_path(&testo) || extension_of(&testo) == "cue" {
        return Natura::File;
    }
    match std::fs::symlink_metadata(rymembra_app::files::percorso_lungo(percorso)) {
        Ok(metadati) if metadati.is_dir() => Natura::Cartella,
        Ok(_) => Natura::File,
        Err(_) => Natura::Assente,
    }
}

/// Le cartelle da scansionare per dei cambiamenti, col disco interrogato
/// sotto una scadenza.
///
/// Fuori dal filo della sorveglianza, su un filo che si può abbandonare: una
/// condivisione che muore fra l'avviso e la domanda risponderebbe dopo il
/// timeout di SMB, per ogni percorso. Scaduta [`NATURA_SCADENZA`], ogni
/// cambiamento diventa la cartella sorvegliata che lo contiene — la scelta
/// prudente: una camminata più larga, e se il disco è davvero spento la
/// radice risulterà saltata e tornerà in attesa.
///
/// `natura` è [`natura_di`]; le prove ne passano una che conta le domande.
pub(super) fn cartelle_da_guardare(
    cambiati: Vec<Cambiamento>,
    radici: Vec<String>,
    regole: PathRules,
    natura: fn(&Path) -> Natura,
) -> Vec<String> {
    if cambiati.is_empty() {
        return Vec::new();
    }
    let (per_il_filo, radici_del_filo) = (cambiati.clone(), radici.clone());
    let decise = rymembra_app::scadenza::con_scadenza("natura", NATURA_SCADENZA, move || {
        // Una domanda per cartella e non per avviso: i venti file di un album
        // chiedono tutti della stessa.
        let memoria: RefCell<HashMap<String, Natura>> = RefCell::default();
        let ricordata = |percorso: &str| {
            *memoria
                .borrow_mut()
                .entry(percorso.to_owned())
                .or_insert_with(|| natura(Path::new(percorso)))
        };
        cartelle_da_riguardare(&per_il_filo, &radici_del_filo, regole, &ricordata)
    });
    decise.unwrap_or_else(|| {
        nota!(
            "[sorveglia] il disco non ha detto in tempo cosa è cambiato: si riguardano le cartelle sorvegliate intere"
        );
        cartelle_da_riguardare(&cambiati, &radici, regole, &|_| Natura::Assente)
    })
}
