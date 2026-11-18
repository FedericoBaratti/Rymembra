//! «Scarica qui»: i brani di un altro dispositivo copiati in questa libreria.
//!
//! # Come
//!
//! Un filo solo, un file alla volta, a finestre da [`MASSIMO_PEZZO`] (2 MiB):
//! quel che l'altro concede per domanda. Il file si scrive accanto a dove
//! starà, come `<nome>.<ext>.part`, e si rinomina solo intero e scritto su
//! disco: la scansione non vede mai un brano a metà, perché `.part` non è
//! un'estensione che legge.
//!
//! # Dove
//!
//! Nella cartella dei download, la stessa dei brani di catalogo
//! (`procura::cartella_download`): `<Artista>/<Album>/<NN - Titolo>.<ext>`, coi
//! nomi ripuliti come lì ([`segmento_sicuro`]). Un file che c'è già con la
//! stessa dimensione è lo stesso brano, e si salta; uno diverso con lo stesso
//! nome resta dov'è, e il nuovo prende ` (2)`.
//!
//! Alla fine della coda si scansiona la cartella dei download, come dopo un
//! prelievo di catalogo: i brani entrano in libreria, e da lì sono di questo
//! dispositivo — i suoi ascolti, le sue playlist.
//!
//! # Cosa non scende
//!
//! I brani di un'immagine CD (un `.cue` con un file solo): scaricarli vorrebbe
//! dire tagliare il file, o portare l'immagine intera per un brano. In questa
//! versione si saltano, e la finestra spegne il tasto dicendo perché.
//!
//! Un file che non è musica: si scrive solo con un'estensione audio che la
//! scansione legge ([`estensione_audio`]). Il resto non scende, ed è un
//! guasto che la finestra dice (`fs.extensionRefused`).
//!
//! # La rete
//!
//! Come lo streaming: «Solo Wi-Fi» vale (`dati_permessi`, guardato a ogni
//! file, perché il Wi-Fi può andarsene a metà coda), e finché scende qualcosa
//! il nodo del telefono non va a dormire (`InVolo`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rymembra_app::library::condivisa::BranoRemoto;
use rymembra_app::specchio::scarico::righe_da_scaricare;
use rymembra_domain::destinazione::segmento_sicuro;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::SUPPORTED_EXTENSIONS;
use rymembra_lontano::protocollo::MASSIMO_PEZZO;
use serde::Serialize;
use tauri::AppHandle;

use super::filo::{leggi_a_parte, prendi};
use super::sfoglia::{InVolo, Pezzi, ViaNodo, dati_permessi, estensione_buona};
use super::{comune, in_disparte};
use crate::errore::Esito;
use crate::nota;
use crate::spegnimento::Emette as _;

/// Quanti brani al più in una richiesta: un album intero, o una ricerca.
const MASSIMI_PER_VOLTA: usize = 5000;

/// Quante varianti ` (2)`, ` (3)`… si provano prima di arrendersi.
const MASSIME_VARIANTI: u32 = 20;

/// A che punto è «Scarica qui», per la finestra (`lontano:scarico`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoScaricoIpc {
    /// Il brano che sta scendendo; `None` a coda ferma.
    pub titolo: Option<String>,
    /// I byte del brano in corso, e quanti sono in tutto.
    pub byte: u64,
    pub totale: u64,
    /// I brani che aspettano dopo quello in corso.
    pub in_coda: usize,
    /// Da quando la coda è ripartita: scritti, saltati (c'erano già, o sono di
    /// un'immagine CD), non riusciti.
    pub fatti: u32,
    pub saltati: u32,
    pub falliti: u32,
    /// Il codice dell'ultimo guasto, per la finestra.
    pub errore: Option<String>,
}

impl StatoScaricoIpc {
    const FERMO: Self = Self {
        titolo: None,
        byte: 0,
        totale: 0,
        in_coda: 0,
        fatti: 0,
        saltati: 0,
        falliti: 0,
        errore: None,
    };
}

/// Un brano da scaricare, e da chi.
struct Lavoro {
    dispositivo: String,
    brano: BranoRemoto,
}

/// Le code di «Scarica qui»: una per dispositivo, ognuna col suo filo.
///
/// # Perché una per dispositivo
///
/// Con un filo solo per tutti, un PC lento — o raggiunto via relay — con un
/// album in coda faceva aspettare ogni altro dispositivo finché non finiva
/// (§7.2.5). Adesso ogni dispositivo ha il suo posto: un filo, una coda. Quel
/// che è comune sta qui — lo stato che la finestra vede, «Annulla», i conti, e
/// la scansione finale dei download, che tocca all'ultimo filo che finisce.
///
/// La parte pura sta in questi metodi, e si prova da sola.
struct Coda {
    /// I lavori in attesa, per dispositivo.
    attese: BTreeMap<String, VecDeque<Lavoro>>,
    /// I dispositivi che hanno un filo che gira.
    fili: BTreeSet<String>,
    /// Il titolo del brano che ogni filo sta scaricando.
    in_corso: BTreeMap<String, String>,
    /// Di quale dispositivo la finestra vede il brano e i byte: uno solo alla
    /// volta, perché una barra che ballasse fra due brani non direbbe niente.
    primo_piano: Option<String>,
    /// Cresce a ogni «Annulla»: il brano in corso lo confronta fra una finestra
    /// e l'altra, e si ferma.
    giro: u64,
    /// Brani scritti da quando la coda è ripartita: se ce ne sono, alla fine si
    /// scansiona la cartella dei download.
    scritti: u32,
    stato: StatoScaricoIpc,
}

/// Cosa fa un filo dopo aver finito un brano.
enum Prossimo {
    /// Un altro brano del suo dispositivo.
    Lavoro {
        lavoro: Lavoro,
        giro: u64,
        /// Il numero del brano nella coda e quanti sono in tutto, per la notifica.
        numero: u32,
        di: u32,
    },
    /// Il suo dispositivo non ha altro: il filo si ferma. `ultimo` se non ne
    /// gira nessun altro, e allora la coda è finita; `scritti` sono i brani
    /// scesi da quando è ripartita.
    Finito { ultimo: bool, scritti: u32 },
}

impl Coda {
    const fn nuova() -> Self {
        Self {
            attese: BTreeMap::new(),
            fili: BTreeSet::new(),
            in_corso: BTreeMap::new(),
            primo_piano: None,
            giro: 0,
            scritti: 0,
            stato: StatoScaricoIpc::FERMO,
        }
    }

    /// Quanti brani aspettano, di tutti i dispositivi.
    fn in_attesa(&self) -> usize {
        self.attese.values().map(VecDeque::len).sum()
    }

    /// Mette in coda i brani di `dispositivo`. `true` se per lui serve un filo
    /// nuovo: ce l'ha già se uno ne gira, e quello di un altro non conta.
    fn aggiungi(&mut self, dispositivo: &str, brani: Vec<BranoRemoto>) -> bool {
        if self.fili.is_empty() {
            // Una coda nuova: i conti ricominciano.
            self.stato = StatoScaricoIpc::FERMO;
            self.scritti = 0;
            self.in_corso.clear();
            self.primo_piano = None;
        }
        for brano in brani {
            if brano.segmento.is_some() {
                self.stato.saltati = self.stato.saltati.saturating_add(1);
                continue;
            }
            self.attese
                .entry(dispositivo.to_owned())
                .or_default()
                .push_back(Lavoro {
                    dispositivo: dispositivo.to_owned(),
                    brano,
                });
        }
        self.stato.in_coda = self.in_attesa();
        let ha_lavori = self.attese.get(dispositivo).is_some_and(|c| !c.is_empty());
        let parte = ha_lavori && !self.fili.contains(dispositivo);
        if parte {
            self.fili.insert(dispositivo.to_owned());
        }
        parte
    }

    /// Il prossimo brano di `dispositivo`, o la fine del suo filo.
    ///
    /// Il filo esce dall'elenco nello stesso passo in cui vede la sua coda
    /// vuota: un `aggiungi` che arriva un istante dopo ne apre uno nuovo, e uno
    /// che arriva un istante prima è già in coda per questo.
    fn prendi(&mut self, dispositivo: &str) -> Prossimo {
        let Some(lavoro) = self
            .attese
            .get_mut(dispositivo)
            .and_then(VecDeque::pop_front)
        else {
            self.attese.remove(dispositivo);
            self.fili.remove(dispositivo);
            let ultimo = self.fili.is_empty();
            if ultimo {
                self.stato.titolo = None;
                self.stato.byte = 0;
                self.stato.totale = 0;
                self.stato.in_coda = 0;
                self.primo_piano = None;
                self.in_corso.clear();
            }
            return Prossimo::Finito {
                ultimo,
                scritti: self.scritti,
            };
        };
        self.stato.in_coda = self.in_attesa();
        self.in_corso
            .insert(dispositivo.to_owned(), lavoro.brano.title.clone());
        if self.primo_piano.is_none() {
            self.primo_piano = Some(dispositivo.to_owned());
        }
        if self.primo_piano.as_deref() == Some(dispositivo) {
            self.stato.titolo = Some(lavoro.brano.title.clone());
            self.stato.byte = 0;
            self.stato.totale = u64::try_from(lavoro.brano.dimensione).unwrap_or(0);
        }
        let numero = self
            .stato
            .fatti
            .saturating_add(self.stato.saltati)
            .saturating_add(self.stato.falliti)
            .saturating_add(1);
        let di = numero.saturating_add(u32::try_from(self.in_attesa()).unwrap_or(u32::MAX));
        Prossimo::Lavoro {
            lavoro,
            giro: self.giro,
            numero,
            di,
        }
    }

    /// Il brano di `dispositivo` è arrivato a `byte` su `totale`. `true` se la
    /// finestra deve saperlo: solo il brano in primo piano muove la barra.
    fn avanza(&mut self, dispositivo: &str, byte: u64, totale: u64) -> bool {
        if self.primo_piano.is_none()
            && let Some(titolo) = self.in_corso.get(dispositivo)
        {
            self.stato.titolo = Some(titolo.clone());
            self.primo_piano = Some(dispositivo.to_owned());
        }
        if self.primo_piano.as_deref() != Some(dispositivo) {
            return false;
        }
        self.stato.byte = byte;
        self.stato.totale = totale;
        true
    }

    /// Il brano di `dispositivo` è finito, comunque sia andata: se era in
    /// primo piano, la barra passa a un altro brano che sta scendendo.
    fn finito(&mut self, dispositivo: &str) {
        self.in_corso.remove(dispositivo);
        if self.primo_piano.as_deref() == Some(dispositivo) {
            self.primo_piano = self.in_corso.keys().next().cloned();
            self.stato.titolo = self
                .primo_piano
                .as_ref()
                .and_then(|d| self.in_corso.get(d))
                .cloned();
            self.stato.byte = 0;
            self.stato.totale = 0;
        }
    }

    /// Un brano è stato scritto.
    fn scritto(&mut self, _dispositivo: &str) {
        self.stato.fatti = self.stato.fatti.saturating_add(1);
        self.scritti = self.scritti.saturating_add(1);
    }

    /// Svuota ogni coda: «Annulla», o l'uscita dal programma.
    fn svuota(&mut self) {
        self.attese.clear();
        self.stato.in_coda = 0;
    }

    /// «Annulla»: svuota ogni coda, e ferma i brani in corso.
    fn annulla(&mut self) {
        self.svuota();
        self.giro = self.giro.wrapping_add(1);
    }
}

static CODA: Mutex<Coda> = Mutex::new(Coda::nuova());

/// Com'è finito un brano.
#[derive(Debug, PartialEq, Eq)]
enum Fatto {
    Scritto(PathBuf),
    /// C'era già, con la stessa dimensione.
    Uguale,
}

/// Il nome da dare al file: libero, o già occupato dallo stesso brano.
#[derive(Debug, PartialEq, Eq)]
enum Posto {
    Libero(PathBuf),
    Uguale,
}

fn annuncia(app: &AppHandle) {
    let stato = prendi(&CODA).stato.clone();
    app.emetti("lontano:scarico", stato);
}

/// Il guasto di un'operazione sul disco, nella forma del catalogo.
fn guasto_io(percorso: &Path, err: &std::io::Error) -> AppError {
    let path = percorso.to_string_lossy().into_owned();
    match err.kind() {
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => {
            AppError::new(ErrorCode::FsDiskFull { path: Some(path) })
        }
        ErrorKind::PermissionDenied => AppError::new(ErrorCode::FsPermissionDenied { path }),
        _ => AppError::new(ErrorCode::FsWriteFailed {
            path,
            detail: Some(err.to_string()),
        }),
    }
}

/// La cartella e il nome base di un brano, relativi alla cartella dei download.
fn dove_va(brano: &BranoRemoto) -> (PathBuf, String) {
    let artista = if brano.artist.trim().is_empty() {
        "Sconosciuto"
    } else {
        brano.artist.as_str()
    };
    // Senza album è un singolo: la stessa regola dei brani di catalogo.
    let album = if brano.album.trim().is_empty() {
        "Singoli"
    } else {
        brano.album.as_str()
    };
    let nome = match brano.track_number.filter(|n| *n > 0) {
        Some(numero) => format!("{numero:02} - {}", brano.title),
        None => brano.title.clone(),
    };
    (
        PathBuf::from(segmento_sicuro(artista)).join(segmento_sicuro(album)),
        segmento_sicuro(&nome),
    )
}

/// Il primo nome libero in `cartella`, o `Uguale` se lo stesso brano — stesso
/// nome, stessa dimensione — c'è già.
fn posto_libero(cartella: &Path, base: &str, estensione: &str, dimensione: u64) -> Posto {
    for variante in 1..=MASSIME_VARIANTI {
        let nome = if variante == 1 {
            format!("{base}.{estensione}")
        } else {
            format!("{base} ({variante}).{estensione}")
        };
        let percorso = cartella.join(nome);
        match fs::metadata(&percorso) {
            Ok(dati) if dati.is_file() && dati.len() == dimensione => return Posto::Uguale,
            Ok(_) => {}
            Err(_) => return Posto::Libero(percorso),
        }
    }
    // Venti file diversi con lo stesso nome: il ventunesimo sovrascriverebbe
    // qualcosa. Si dice «c'è già», che è il meno sbagliato.
    Posto::Uguale
}

/// L'estensione del file da scrivere: solo una di quelle che la scansione
/// riconosce come musica ([`SUPPORTED_EXTENSIONS`]).
///
/// L'estensione la dice l'altro dispositivo, e un brano «`Intro.exe`», un
/// `.lnk`, un `.url` o un `desktop.ini` nella cartella dei download sono un
/// programma da un clic o un'icona che Esplora risorse va a prendere da sé
/// (anche su `\\un-altro-pc\…`). La scansione poi li ignorerebbe, e
/// resterebbero lì senza che nessuno ne parli. Un brano di un formato che
/// questa versione non legge non entrerebbe comunque in libreria.
fn estensione_audio(estensione: &str) -> Result<String, AppError> {
    estensione_buona(estensione)
        .filter(|buona| SUPPORTED_EXTENSIONS.contains(&buona.as_str()))
        .ok_or_else(|| {
            AppError::new(ErrorCode::FsExtensionRefused {
                path: format!(
                    ".{}",
                    estensione
                        .chars()
                        .filter(char::is_ascii_alphanumeric)
                        .take(16)
                        .collect::<String>()
                ),
                allowed: SUPPORTED_EXTENSIONS
                    .iter()
                    .map(|e| (*e).to_owned())
                    .collect(),
            })
        })
}

/// La coda è stata annullata dopo che il brano `giro` è partito.
fn annullato(giro: u64) -> bool {
    prendi(&CODA).giro != giro || crate::spegnimento::in_uscita()
}

fn interrotto() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("scaricamento annullato".to_owned()),
    })
}

/// Copia i byte di un brano in `parte`, a finestre, e lo scrive su disco.
/// `avanza` riceve i byte scritti e il totale.
///
/// La dimensione e la versione sono quelle della prima risposta, non quelle
/// dell'elenco: il database dell'altro può essere indietro rispetto al disco
/// (vedi `sfoglia::DaLontano::versione`). Da lì in poi non devono cambiare.
fn copia(
    pezzi: &impl Pezzi,
    brano: &BranoRemoto,
    parte: &Path,
    giro: u64,
    avanza: &mut dyn FnMut(u64, u64),
) -> Result<(), AppError> {
    let mut file = File::create(parte).map_err(|err| guasto_io(parte, &err))?;
    let mut fissati: Option<(u64, String)> = None;
    let mut da = 0_u64;
    loop {
        if fissati.as_ref().is_some_and(|(totale, _)| da >= *totale) {
            break;
        }
        if annullato(giro) {
            return Err(interrotto());
        }
        let quanti = fissati
            .as_ref()
            .map_or(MASSIMO_PEZZO, |(totale, _)| totale.saturating_sub(da))
            .min(MASSIMO_PEZZO);
        let (byte, dimensione, versione) = pezzi.pezzo(brano.id, da, quanti)?;
        // Un file cambiato sotto la copia darebbe un brano fatto di due: si
        // ferma, come lo streaming.
        match &fissati {
            Some((totale, prima)) if *totale != dimensione || *prima != versione => {
                return Err(AppError::new(ErrorCode::LontanoFileCambiato));
            }
            Some(_) => {}
            None => fissati = Some((dimensione, versione)),
        }
        if da >= dimensione {
            break;
        }
        if byte.is_empty() {
            return Err(AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause("una lettura prima della fine non ha portato byte"));
        }
        let quanti = usize::try_from(quanti)
            .unwrap_or(usize::MAX)
            .min(byte.len());
        let utili = byte.get(..quanti).unwrap_or(&byte);
        file.write_all(utili)
            .map_err(|err| guasto_io(parte, &err))?;
        da = da.saturating_add(u64::try_from(utili.len()).unwrap_or(0));
        avanza(da, dimensione);
    }
    file.sync_all().map_err(|err| guasto_io(parte, &err))
}

/// Scarica un brano: dal posto libero al file rinominato.
fn scarica_uno(app: &AppHandle, lavoro: &Lavoro, giro: u64) -> Result<Fatto, AppError> {
    let brano = &lavoro.brano;
    if brano.segmento.is_some() {
        return Ok(Fatto::Uguale);
    }
    let comune = comune(app)?;
    if !comune.associato(&lavoro.dispositivo) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    dati_permessi(&comune)?;
    let cartella = crate::procura::cartella_download(app).ok_or_else(|| {
        AppError::new(ErrorCode::FsNotFound {
            path: "cartella di scaricamento".to_owned(),
        })
    })?;
    let estensione = estensione_audio(&brano.estensione)?;
    let (relativa, base) = dove_va(brano);
    let cartella = cartella.join(relativa);
    let totale = u64::try_from(brano.dimensione).unwrap_or(0);
    let finale = match posto_libero(&cartella, &base, &estensione, totale) {
        Posto::Uguale => return Ok(Fatto::Uguale),
        Posto::Libero(percorso) => percorso,
    };
    fs::create_dir_all(&cartella).map_err(|err| guasto_io(&cartella, &err))?;
    let mut parte = finale.clone().into_os_string();
    parte.push(".part");
    let parte = PathBuf::from(parte);

    let _in_volo = InVolo::nuovo(Arc::clone(&comune));
    let pezzi = ViaNodo {
        comune,
        chiave: lavoro.dispositivo.clone(),
    };
    let mut ultimo = 0_u64;
    let mut avanza = |byte: u64, totale: u64| {
        let in_primo_piano = prendi(&CODA).avanza(&lavoro.dispositivo, byte, totale);
        // Ogni finestra è 2 MiB: abbastanza di rado per un evento ciascuna.
        if in_primo_piano && byte != ultimo {
            ultimo = byte;
            annuncia(app);
        }
    };
    if let Err(err) = copia(&pezzi, brano, &parte, giro, &mut avanza) {
        let _ = fs::remove_file(&parte);
        return Err(err);
    }
    if let Err(err) = fs::rename(&parte, &finale) {
        let _ = fs::remove_file(&parte);
        return Err(guasto_io(&finale, &err));
    }
    Ok(Fatto::Scritto(finale))
}

/// Sul telefono, la notifica del servizio in primo piano che tiene vivo il
/// processo finché la coda non è vuota (`ScaricoInCorso`): il brano `numero`
/// di `di`, o spenta.
#[cfg(target_os = "android")]
fn notifica(acceso: bool, numero: u32, di: u32, titolo: &str) {
    if let Err(err) = rymembra_android::sistema::scarico(acceso, numero, di, titolo) {
        nota!("[lontano] la notifica dello scaricamento non si cambia: {err}");
    }
}

#[cfg(not(target_os = "android"))]
const fn notifica(_: bool, _: u32, _: u32, _: &str) {}

/// Il filo di un dispositivo: svuota la sua coda. L'ultimo che finisce
/// scansiona quel che è arrivato.
fn gira(app: &AppHandle, dispositivo: &str) {
    let (ultimo, scritti) = loop {
        let (lavoro, giro, numero, di) = match prendi(&CODA).prendi(dispositivo) {
            Prossimo::Finito { ultimo, scritti } => break (ultimo, scritti),
            Prossimo::Lavoro {
                lavoro,
                giro,
                numero,
                di,
            } => (lavoro, giro, numero, di),
        };
        notifica(true, numero, di, &lavoro.brano.title);
        annuncia(app);
        let esito = scarica_uno(app, &lavoro, giro);
        let mut coda = prendi(&CODA);
        coda.finito(dispositivo);
        match esito {
            Ok(Fatto::Scritto(_)) => coda.scritto(dispositivo),
            Ok(Fatto::Uguale) => coda.stato.saltati = coda.stato.saltati.saturating_add(1),
            // Annullato: non è un guasto, e il resto della coda è già vuoto.
            Err(_) if coda.giro != giro => {}
            Err(err) => {
                let codice = err.code().kind().code();
                nota!("[lontano] un brano non è sceso codice={codice}");
                coda.stato.falliti = coda.stato.falliti.saturating_add(1);
                coda.stato.errore = Some(codice.to_owned());
            }
        }
        drop(coda);
        if crate::spegnimento::in_uscita() {
            prendi(&CODA).svuota();
        }
    };
    annuncia(app);
    if !ultimo {
        return;
    }
    notifica(false, 0, 0, "");
    if scritti > 0 && !crate::spegnimento::in_uscita() {
        nota!("[lontano] scesi {scritti} brani: si scansiona la cartella dei download");
        crate::procura::scansiona_i_download(app);
    }
}

/// Mette in coda i brani di `chiave`, e fa partire il filo del suo dispositivo
/// se non gira già.
fn accoda(
    app: &AppHandle,
    chiave: &str,
    brani: Vec<BranoRemoto>,
) -> Result<StatoScaricoIpc, AppError> {
    let comune = comune(app)?;
    if !comune.associato(chiave) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    if comune.nodo().is_none() {
        return Err(AppError::new(ErrorCode::LontanoSpento));
    }
    dati_permessi(&comune)?;
    if crate::procura::cartella_download(app).is_none() {
        return Err(AppError::new(ErrorCode::FsNotFound {
            path: "cartella di scaricamento".to_owned(),
        }));
    }

    let (parte, stato) = {
        let mut coda = prendi(&CODA);
        let parte = coda.aggiungi(chiave, brani);
        (parte, coda.stato.clone())
    };

    if parte {
        let per_il_filo = app.clone();
        let del_dispositivo = chiave.to_owned();
        let partito = std::thread::Builder::new()
            .name("rymembra-lontano-scarico".to_owned())
            .spawn(move || gira(&per_il_filo, &del_dispositivo));
        if let Err(err) = partito {
            // Il filo non c'è: la sua coda non ha chi la svuoti.
            let mut coda = prendi(&CODA);
            coda.fili.remove(chiave);
            coda.attese.remove(chiave);
            coda.stato.in_coda = coda.in_attesa();
            return Err(AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("il filo dello scaricamento non parte".to_owned()),
            })
            .with_cause(err.to_string()));
        }
    }
    annuncia(app);
    Ok(stato)
}

/// «Scarica qui»: le righe della libreria che stanno su un altro dispositivo,
/// copiate nella cartella dei download di questo.
///
/// Le righe di questa macchina e quelle che il proprietario non manda più si
/// saltano: la finestra offre il gesto su una selezione, e una selezione può
/// essere mista. Ogni dispositivo ha la sua coda e il suo filo (vedi [`Coda`]).
///
/// # Errori
///
/// `lontano.nonAssociato`, `lontano.spento`, `lontano.reteAConsumo`,
/// `fs.notFound` senza una cartella dove scrivere, `ipc.payloadInvalid` per
/// una lista troppo lunga. Se un dispositivo non si può chiedere e un altro sì,
/// questo scende e l'errore è quello del primo: il resto lo dice la finestra
/// con `lontano:scarico`. I guasti dei singoli brani arrivano lì.
#[tauri::command]
pub async fn lontano_scarica_righe(app: AppHandle, righe: Vec<i64>) -> Esito<StatoScaricoIpc> {
    in_disparte(move || {
        if righe.len() > MASSIMI_PER_VOLTA {
            return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
                channel: "lontano".to_owned(),
                detail: Some(format!("al più {MASSIMI_PER_VOLTA} brani per volta")),
            }));
        }
        let gruppi = leggi_a_parte(&app, |c| righe_da_scaricare(c, &righe))?;
        let mut primo_errore: Option<AppError> = None;
        let mut partito = false;
        for (dispositivo, brani) in gruppi {
            match accoda(&app, &dispositivo, brani) {
                Ok(_) => partito = true,
                Err(err) => {
                    primo_errore.get_or_insert(err);
                }
            }
        }
        match primo_errore {
            Some(err) if !partito => Err(err),
            _ => Ok(prendi(&CODA).stato.clone()),
        }
    })
    .await
}

/// Svuota le code e ferma i brani in corso, i cui `.part` si cancellano.
#[tauri::command]
pub fn lontano_scarico_annulla(app: AppHandle) -> Esito<StatoScaricoIpc> {
    let stato = {
        let mut coda = prendi(&CODA);
        coda.annulla();
        coda.stato.clone()
    };
    annuncia(&app);
    Ok(stato)
}

/// A che punto è, adesso: per la finestra che si apre a coda avviata.
#[tauri::command]
pub fn lontano_scarico_stato() -> Esito<StatoScaricoIpc> {
    Ok(prendi(&CODA).stato.clone())
}

#[cfg(test)]
mod prove {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn brano(titolo: &str) -> BranoRemoto {
        BranoRemoto {
            id: 7,
            title: titolo.to_owned(),
            artist: "AC/DC".to_owned(),
            album: "Back in Black".to_owned(),
            album_key: None,
            track_number: Some(3),
            disc_number: None,
            duration_ms: 1_000,
            year: None,
            cover_art_hash: None,
            play_count: 0,
            liked: false,
            rating: 0,
            estensione: "flac".to_owned(),
            dimensione: 10,
            versione: "v1".to_owned(),
            segmento: None,
        }
    }

    #[test]
    fn il_brano_va_sotto_artista_e_album_coi_nomi_ripuliti() {
        let (cartella, base) = dove_va(&brano("Shoot to Thrill"));
        assert_eq!(cartella, PathBuf::from("AC DC").join("Back in Black"));
        assert_eq!(base, "03 - Shoot to Thrill");

        let mut senza = brano("Solo");
        senza.album = String::new();
        senza.artist = " ".to_owned();
        senza.track_number = None;
        let (cartella, base) = dove_va(&senza);
        assert_eq!(cartella, PathBuf::from("Sconosciuto").join("Singoli"));
        assert_eq!(base, "Solo");
    }

    #[test]
    fn scende_solo_un_estensione_audio() {
        assert_eq!(estensione_audio("FLAC").unwrap(), "flac");
        assert_eq!(estensione_audio(".mp3").unwrap(), "mp3");
        assert_eq!(estensione_audio("ape").unwrap(), "ape");
        // Quel che l'altro dispositivo potrebbe chiamare «brano» e che
        // Esplora risorse eseguirebbe, o aprirebbe da sé.
        for cattiva in [
            "exe", "lnk", "url", "scf", "ini", "hta", "js", "bat", "part", "",
        ] {
            let err = estensione_audio(cattiva).unwrap_err();
            assert_eq!(err.code().kind().code(), "fs.extensionRefused", "{cattiva}");
        }
    }

    #[test]
    fn lo_stesso_brano_si_salta_e_un_altro_prende_una_variante() {
        let cartella = tempfile::tempdir().unwrap();
        assert_eq!(
            posto_libero(cartella.path(), "01 - X", "flac", 10),
            Posto::Libero(cartella.path().join("01 - X.flac"))
        );
        fs::write(cartella.path().join("01 - X.flac"), [0_u8; 10]).unwrap();
        assert_eq!(
            posto_libero(cartella.path(), "01 - X", "flac", 10),
            Posto::Uguale
        );
        assert_eq!(
            posto_libero(cartella.path(), "01 - X", "flac", 11),
            Posto::Libero(cartella.path().join("01 - X (2).flac"))
        );
    }

    /// Un file in memoria, servito a pezzi come farebbe l'altro. Dopo
    /// `cambia_dopo` domande la sua versione cambia: riscritto a metà copia.
    #[derive(Debug)]
    struct InMemoria {
        byte: Vec<u8>,
        versione: String,
        domande: AtomicUsize,
        cambia_dopo: Option<usize>,
    }

    impl Pezzi for InMemoria {
        fn pezzo(
            &self,
            _brano: i64,
            da: u64,
            quanti: u64,
        ) -> Result<(Vec<u8>, u64, String), AppError> {
            let fatte = self.domande.fetch_add(1, Ordering::Relaxed);
            let da = usize::try_from(da).unwrap();
            let fino = (da + usize::try_from(quanti).unwrap()).min(self.byte.len());
            let versione = if self.cambia_dopo.is_some_and(|n| fatte >= n) {
                format!("{}-riscritto", self.versione)
            } else {
                self.versione.clone()
            };
            Ok((
                self.byte[da..fino].to_vec(),
                u64::try_from(self.byte.len()).unwrap(),
                versione,
            ))
        }
    }

    #[test]
    fn la_copia_scrive_tutti_i_byte() {
        let cartella = tempfile::tempdir().unwrap();
        let parte = cartella.path().join("x.flac.part");
        let byte: Vec<u8> = (0..=255).cycle().take(5 * 1024 * 1024 + 3).collect();
        // L'elenco è vecchio — un'altra dimensione, un'altra versione — e
        // conta il disco.
        let b = brano("X");
        let pezzi = InMemoria {
            byte: byte.clone(),
            versione: "v7".to_owned(),
            domande: AtomicUsize::new(0),
            cambia_dopo: None,
        };
        let giro = prendi(&CODA).giro;
        let mut visti = Vec::new();
        copia(&pezzi, &b, &parte, giro, &mut |n, totale| {
            visti.push(n);
            assert_eq!(totale, u64::try_from(byte.len()).unwrap());
        })
        .unwrap();
        assert_eq!(fs::read(&parte).unwrap(), byte);
        // Tre finestre da 2 MiB per cinque mebibyte e tre byte.
        assert_eq!(pezzi.domande.load(Ordering::Relaxed), 3);
        assert_eq!(
            visti.last().copied(),
            Some(u64::try_from(byte.len()).unwrap())
        );
    }

    #[test]
    fn un_file_cambiato_ferma_la_copia() {
        let cartella = tempfile::tempdir().unwrap();
        let parte = cartella.path().join("x.flac.part");
        let b = brano("X");
        let pezzi = InMemoria {
            byte: vec![1; 5 * 1024 * 1024],
            versione: "v2".to_owned(),
            domande: AtomicUsize::new(0),
            cambia_dopo: Some(1),
        };
        let giro = prendi(&CODA).giro;
        let err = copia(&pezzi, &b, &parte, giro, &mut |_, _| {}).unwrap_err();
        assert_eq!(err.code().kind().code(), "lontano.fileCambiato");
        assert_eq!(pezzi.domande.load(Ordering::Relaxed), 2);
    }
    // ── una coda per dispositivo ────────────────────────────────────────────

    /// Il titolo del lavoro preso, o `None` se il filo è finito.
    fn preso(coda: &mut Coda, dispositivo: &str) -> Option<String> {
        match coda.prendi(dispositivo) {
            Prossimo::Lavoro { lavoro, .. } => Some(lavoro.brano.title),
            Prossimo::Finito { .. } => None,
        }
    }

    /// §7.2.5: ogni dispositivo ha il suo posto. Un PC lento — o via relay — con
    /// tre brani in coda non fa aspettare un altro dispositivo: il filo del
    /// secondo parte subito e prende il suo brano, mentre quello del primo è
    /// ancora in mezzo ai suoi.
    #[test]
    fn un_dispositivo_lento_non_fa_aspettare_gli_altri() {
        let mut coda = Coda::nuova();
        assert!(coda.aggiungi("lento", vec![brano("a1"), brano("a2"), brano("a3")]));
        assert_eq!(preso(&mut coda, "lento").as_deref(), Some("a1"));
        // Il filo di «lento» è occupato col suo primo brano: l'altro parte lo stesso.
        assert!(
            coda.aggiungi("svelto", vec![brano("b1")]),
            "il secondo dispositivo ha il suo filo"
        );
        assert_eq!(preso(&mut coda, "svelto").as_deref(), Some("b1"));
        // Un brano in più per il primo non apre un secondo filo: ce l'ha già.
        assert!(!coda.aggiungi("lento", vec![brano("a4")]));
        assert_eq!(coda.stato.in_coda, 3, "a2, a3 e a4 aspettano");
        // Ognuno finisce la sua coda, e nessuna prende il lavoro dell'altra.
        assert_eq!(preso(&mut coda, "svelto"), None);
        assert_eq!(preso(&mut coda, "lento").as_deref(), Some("a2"));
        assert_eq!(preso(&mut coda, "lento").as_deref(), Some("a3"));
        assert_eq!(preso(&mut coda, "lento").as_deref(), Some("a4"));
    }

    /// La scansione dei download e la notifica toccano all'ultimo filo che
    /// finisce, non al primo: finché uno scende, la coda non è finita.
    #[test]
    fn solo_l_ultimo_filo_dice_che_la_coda_e_finita() {
        let mut coda = Coda::nuova();
        coda.aggiungi("a", vec![brano("a1")]);
        coda.aggiungi("b", vec![brano("b1")]);
        assert_eq!(preso(&mut coda, "a").as_deref(), Some("a1"));
        assert_eq!(preso(&mut coda, "b").as_deref(), Some("b1"));
        coda.scritto("a");
        coda.finito("a");
        coda.scritto("b");
        coda.finito("b");
        assert!(matches!(
            coda.prendi("a"),
            Prossimo::Finito {
                ultimo: false,
                scritti: 2
            }
        ));
        assert!(matches!(
            coda.prendi("b"),
            Prossimo::Finito {
                ultimo: true,
                scritti: 2
            }
        ));
        assert_eq!(coda.stato.titolo, None);
        assert_eq!(coda.stato.in_coda, 0);
        // Una coda nuova ricomincia i conti.
        assert!(coda.aggiungi("a", vec![brano("a2")]));
        assert_eq!(coda.stato.fatti, 0);
    }

    /// Lo stato che la finestra vede è uno solo: il byte in corso è di un brano
    /// solo, e quando quello finisce lo prende l'altro filo, invece di far
    /// ballare la barra fra due brani.
    #[test]
    fn la_barra_segue_un_brano_alla_volta() {
        let mut coda = Coda::nuova();
        coda.aggiungi("a", vec![brano("a1")]);
        coda.aggiungi("b", vec![brano("b1")]);
        assert_eq!(preso(&mut coda, "a").as_deref(), Some("a1"));
        assert_eq!(preso(&mut coda, "b").as_deref(), Some("b1"));
        assert_eq!(
            coda.stato.titolo.as_deref(),
            Some("a1"),
            "il primo in primo piano"
        );
        assert!(coda.avanza("a", 5, 10));
        assert!(
            !coda.avanza("b", 9, 10),
            "b va avanti, ma la barra non è sua"
        );
        assert_eq!((coda.stato.byte, coda.stato.totale), (5, 10));
        coda.finito("a");
        assert_eq!(coda.stato.titolo.as_deref(), Some("b1"), "adesso tocca a b");
        assert!(coda.avanza("b", 9, 10));
        assert_eq!(coda.stato.byte, 9);
    }

    /// «Annulla» svuota **tutte** le code e ferma i brani in corso.
    #[test]
    fn annullare_svuota_ogni_coda() {
        let mut coda = Coda::nuova();
        coda.aggiungi("a", vec![brano("a1"), brano("a2")]);
        coda.aggiungi("b", vec![brano("b1"), brano("b2")]);
        let giro = coda.giro;
        coda.annulla();
        assert_ne!(coda.giro, giro, "i brani in corso se ne accorgono");
        assert_eq!(coda.stato.in_coda, 0);
        assert_eq!(preso(&mut coda, "a"), None);
        assert_eq!(preso(&mut coda, "b"), None);
    }

    /// Un brano d'immagine CD non scende: si conta fra i saltati e non apre un
    /// filo che non avrebbe niente da fare.
    #[test]
    fn un_brano_d_immagine_non_apre_un_filo() {
        let mut coda = Coda::nuova();
        let mut cd = brano("cd");
        cd.segmento = Some(rymembra_app::library::condivisa::Segmento {
            inizio_cd: 0,
            fine_cd: None,
        });
        assert!(!coda.aggiungi("a", vec![cd]));
        assert_eq!(coda.stato.saltati, 1);
    }
}
