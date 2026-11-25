//! «Scarica qui»: i brani di un altro dispositivo copiati in questa libreria.
//!
//! # Come
//!
//! Un filo solo per dispositivo, un file alla volta, a finestre da
//! [`MASSIMO_PEZZO`] (2 MiB): quel che l'altro concede per domanda. Il file si
//! scrive accanto a dove starà, come `<nome>.<ext>.<dispositivo>.part` — il
//! marchio è di chi lo manda, perché due code non scrivano sullo stesso file —,
//! e si rinomina solo intero e scritto su disco: la scansione non vede mai un
//! brano a metà, perché `.part` non è un'estensione che legge. Il nome finale si
//! prende alla fine con una creazione esclusiva, e un file che c'è già non si
//! sostituisce ([`consegna`]); le parti che un arresto lascia si tolgono quando
//! il filo di quel dispositivo torna in quella cartella.
//!
//! # Quel che l'altro dichiara non si crede sulla parola
//!
//! La dimensione della prima risposta deve stare vicina a quella del catalogo, non
//! superare il tetto per file, e non essere zero ([`dimensione_ammessa`]); lo
//! spazio si riserva a quella lunghezza prima del primo byte, e un file che finisce
//! prima del totale non conta come scritto (S2-4).
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
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::nucleo::Nucleo;
use rymembra_app::library::condivisa::BranoRemoto;
use rymembra_app::specchio::scarico::righe_da_scaricare;
use rymembra_domain::destinazione::segmento_sicuro;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::SUPPORTED_EXTENSIONS;
use rymembra_lontano::protocollo::MASSIMO_PEZZO;
use serde::Serialize;

use super::filo::{leggi_a_parte, prendi};
use super::sfoglia::{InVolo, Pezzi, ViaNodo, dati_permessi, estensione_buona};
use super::{comune, in_disparte};
use crate::errore::Esito;
use crate::nota;

/// Quanti brani al più in una richiesta: un album intero, o una ricerca.
const MASSIMI_PER_VOLTA: usize = 5000;

/// Quante varianti ` (2)`, ` (3)`… si provano prima di arrendersi.
const MASSIME_VARIANTI: u32 = 20;

/// Il tetto di un file: otto gibibyte.
///
/// Un brano che li supera — un'ora di DSD256 ne pesa cinque — non è quel che un
/// «Scarica qui» porta, e senza un tetto è l'altro a dire quanto scrivere: un
/// totale a quaranta bit non avrebbe mai fine (S2-4). Vale anche quando il
/// catalogo non sa la dimensione, ed è il limite che nessuna tolleranza sposta.
const TETTO_PER_FILE: u64 = 8 * 1024 * 1024 * 1024;

/// Di quanto la dimensione dichiarata può superare quella del catalogo,
/// anche per un brano piccolo: un file ritaggato con una copertina nuova cresce di
/// qualche mebibyte. Sopra, conta metà della dimensione del catalogo
/// ([`dimensione_ammessa`]).
const MARGINE_MINIMO: u64 = 16 * 1024 * 1024;

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
        lavoro: Box<Lavoro>,
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
            lavoro: Box::new(lavoro),
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

    /// Il filo di `dispositivo` è caduto in un panico: il suo posto si libera, e il
    /// brano che aveva in mano non è più in corso. La coda che aspettava resta: il
    /// prossimo «Scarica qui» per quel dispositivo fa partire un filo nuovo, che la
    /// trova.
    fn filo_caduto(&mut self, dispositivo: &str) {
        self.fili.remove(dispositivo);
        self.finito(dispositivo);
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

/// Il nome del filo di un dispositivo, nel diario dei panici e nella salute dei fili.
const FILO_SCARICO: &str = "rymembra-lontano-scarico";

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

fn annuncia(app: &Nucleo) {
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

/// La dimensione che l'altro dichiara nella prima risposta, contro quella del
/// suo catalogo: si può credere?
///
/// L'altro è un dispositivo collegato, non per questo uno di cui fidarsi sulla
/// parola: un totale inventato — mille volte l'elenco, o zero — faceva scrivere
/// finestre da 2 MiB finché il disco era pieno, o rinominare un file vuoto come
/// se fosse un brano (S2-4).
///
/// - **Vuoto** non è un brano: `lontano.fileCambiato`, come per un file che non
///   è più quello.
/// - **Oltre [`TETTO_PER_FILE`]**: `lontano.protocollo`, anche quando il catalogo
///   non sa la dimensione (zero) e non c'è niente con cui confrontarla.
/// - **Lontano da quella del catalogo**: `lontano.fileCambiato`. In più, di oltre
///   metà e comunque di oltre [`MARGINE_MINIMO`]; in meno, di oltre metà. Il
///   margine in più è per un file ritaggato dopo l'ultimo giro del catalogo, che
///   cambia di qualche mebibyte, e per un elenco indietro rispetto al disco (vedi
///   `sfoglia::DaLontano::versione`): per questo la dimensione che conta resta
///   quella della prima risposta, e il catalogo serve a escludere l'assurdo, non
///   a imporre l'uguaglianza. In meno non c'è margine fisso: un brano da pochi
///   mebibyte che ne dichiara mille byte è un file tagliato, non ritaggato.
fn dimensione_ammessa(dichiarata: u64, catalogo: i64) -> Result<(), AppError> {
    if dichiarata == 0 {
        return Err(AppError::new(ErrorCode::LontanoFileCambiato)
            .with_cause("l'altro dispositivo dichiara un file vuoto"));
    }
    if dichiarata > TETTO_PER_FILE {
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("la dimensione dichiarata supera il tetto per file"));
    }
    if let Ok(atteso) = u64::try_from(catalogo)
        && atteso > 0
        && (dichiarata < atteso.div_euclid(2)
            || dichiarata > atteso.saturating_add(atteso.div_euclid(2).max(MARGINE_MINIMO)))
    {
        return Err(AppError::new(ErrorCode::LontanoFileCambiato)
            .with_cause("la dimensione dichiarata non è quella del catalogo"));
    }
    Ok(())
}

/// Copia i byte di un brano in `parte`, a finestre, e lo scrive su disco.
/// `avanza` riceve i byte scritti e il totale. Restituisce quanti ne ha scritti.
///
/// La dimensione e la versione sono quelle della prima risposta, non quelle
/// dell'elenco: il database dell'altro può essere indietro rispetto al disco
/// (vedi `sfoglia::DaLontano::versione`). Da lì in poi non devono cambiare. La
/// prima risposta, però, deve avere un senso ([`dimensione_ammessa`]) **prima** di
/// scrivere un byte.
///
/// # Lo spazio si riserva, non si chiede
///
/// Subito dopo la dimensione ammessa il file si porta a quella lunghezza
/// (`set_len`). Su NTFS questo riserva davvero i cluster: un disco senza posto
/// risponde `fs.diskFull` **prima** di cominciare, e non a metà di un brano. Su
/// Android — ext4 e f2fs, dove `ftruncate` crea un file rado — la riserva non
/// c'è e un disco pieno si scopre alla scrittura, che dice lo stesso errore e
/// toglie la parte. Chiedere lo spazio libero al sistema vorrebbe un `unsafe`
/// (`GetDiskFreeSpaceExW`) o un ponte verso Kotlin (`StatFs`).
///
/// La parte si crea **nuova** (`create_new`): se c'è già, è di un'altra coda o di
/// un arresto di prima, e riaprirla troncandola la guasterebbe.
fn copia(
    pezzi: &impl Pezzi,
    brano: &BranoRemoto,
    parte: &Path,
    giro: u64,
    avanza: &mut dyn FnMut(u64, u64),
) -> Result<u64, AppError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(parte)
        .map_err(|err| guasto_io(parte, &err))?;
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
            None => {
                dimensione_ammessa(dimensione, brano.dimensione)?;
                file.set_len(dimensione)
                    .map_err(|err| guasto_io(parte, &err))?;
                fissati = Some((dimensione, versione));
            }
        }
        if byte.is_empty() {
            return Err(AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause("una lettura prima della fine non ha portato byte"));
        }
        // Mai oltre quel che l'altro ha dichiarato: una risposta più lunga del
        // totale non fa crescere il file.
        let restano = dimensione.saturating_sub(da);
        let quanti = usize::try_from(quanti.min(restano))
            .unwrap_or(usize::MAX)
            .min(byte.len());
        let utili = byte.get(..quanti).unwrap_or(&byte);
        file.write_all(utili)
            .map_err(|err| guasto_io(parte, &err))?;
        da = da.saturating_add(u64::try_from(utili.len()).unwrap_or(0));
        avanza(da, dimensione);
    }
    // Un file che finisce senza byte, o prima del totale, non è un brano.
    if da == 0 || fissati.as_ref().is_some_and(|(totale, _)| da != *totale) {
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("la copia è finita prima del totale dichiarato"));
    }
    file.sync_all().map_err(|err| guasto_io(parte, &err))?;
    Ok(da)
}

/// Il nome della parte di un brano: `<finale>.<dispositivo>.part`.
///
/// Con otto cifre della chiave di chi lo manda: due code che scaricano lo
/// stesso brano — stesso artista, album, titolo ed estensione, da due
/// dispositivi — non scrivono più sullo stesso file (S2-10). `.part` in fondo,
/// perché è l'unica estensione che la scansione e la sorveglianza non leggono.
fn nome_della_parte(finale: &Path, dispositivo: &str) -> PathBuf {
    let mut parte = finale.to_path_buf().into_os_string();
    parte.push(format!(".{}.part", marchio(dispositivo)));
    PathBuf::from(parte)
}

/// Le otto cifre della chiave di un dispositivo che finiscono nel nome di una
/// parte: solo caratteri da nome di file, mai vuoto.
fn marchio(dispositivo: &str) -> String {
    let cifre: String = dispositivo
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect();
    if cifre.is_empty() {
        "0".to_owned()
    } else {
        cifre
    }
}

/// Toglie da `cartella` le parti rimaste di **questo dispositivo**: un arresto a
/// metà brano le lascia, e nessuno le rinomina più. Quante ne ha tolte.
///
/// Solo i file col nome di una parte nostra ([`nome_della_parte`]): quella di un
/// altro dispositivo può essere in corso adesso, e un `.part` che non ha il
/// nostro marchio — un programma di torrent, un browser — non è affar nostro. Una
/// cartella che non c'è non è un guasto. Il filo di un dispositivo è uno solo
/// (`Coda::fili`), quindi fra un brano e l'altro nessuna parte sua sta
/// scendendo: quelle che ci sono sono orfane.
fn togli_le_parti_orfane(cartella: &Path, dispositivo: &str) -> usize {
    let Ok(voci) = fs::read_dir(cartella) else {
        return 0;
    };
    let coda = format!(".{}.part", marchio(dispositivo));
    let mut tolte = 0;
    for voce in voci.filter_map(Result::ok) {
        let nome = voce.file_name();
        let nome = nome.to_string_lossy();
        // Più lungo del solo marchio: il nome base di un brano ci deve essere.
        if nome.len() <= coda.len() || !nome.ends_with(&coda) {
            continue;
        }
        if !voce.file_type().is_ok_and(|tipo| tipo.is_file()) {
            continue;
        }
        match fs::remove_file(voce.path()) {
            Ok(()) => tolte += 1,
            Err(err) => nota!(
                "[lontano] una parte rimasta di un arresto non si toglie: {:?}",
                err.kind()
            ),
        }
    }
    tolte
}

/// Mette la parte al suo posto: il nome finale lo prende chi arriva prima, e un
/// file che c'è già non si sostituisce mai.
///
/// Fra la scelta del nome, all'inizio, e la fine dello scaricamento passano
/// minuti: l'utente può aver messo lì un file con quel nome, o un'altra coda lo
/// stesso brano. Il nome si sceglie **di nuovo** adesso ([`posto_libero`]):
///
/// - se c'è lo stesso brano — stessa dimensione — la parte si butta e l'esito è
///   [`Fatto::Uguale`]: la seconda coda non lascia un doppione;
/// - altrimenti il primo nome libero, che si **prende** con una creazione
///   esclusiva (`create_new`) prima di rinominarci sopra: `rename` da solo,
///   su Windows come su Unix, sostituisce un file che c'è. Se qualcuno lo
///   prende in quell'istante si riprova col nome dopo.
///
/// `dimensione` sono i byte scritti, non quelli dell'elenco.
fn consegna(
    parte: &Path,
    cartella: &Path,
    base: &str,
    estensione: &str,
    dimensione: u64,
) -> Result<Fatto, AppError> {
    for _ in 0..MASSIME_VARIANTI {
        let finale = match posto_libero(cartella, base, estensione, dimensione) {
            Posto::Uguale => {
                let _ = fs::remove_file(parte);
                return Ok(Fatto::Uguale);
            }
            Posto::Libero(percorso) => percorso,
        };
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&finale)
        {
            Ok(segnaposto) => {
                // Chiuso prima di rinominare: Windows non sostituisce un file aperto.
                drop(segnaposto);
                if let Err(err) = fs::rename(parte, &finale) {
                    let _ = fs::remove_file(&finale);
                    let _ = fs::remove_file(parte);
                    return Err(guasto_io(&finale, &err));
                }
                return Ok(Fatto::Scritto(finale));
            }
            // L'ha preso qualcuno un attimo fa: si guarda di nuovo.
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => {
                let _ = fs::remove_file(parte);
                return Err(guasto_io(&finale, &err));
            }
        }
    }
    let _ = fs::remove_file(parte);
    Err(AppError::new(ErrorCode::FsWriteFailed {
        path: cartella.to_string_lossy().into_owned(),
        detail: Some("nessun nome libero per il brano".to_owned()),
    }))
}

/// Scarica un brano: dal posto libero al file rinominato.
fn scarica_uno(app: &Nucleo, lavoro: &Lavoro, giro: u64) -> Result<Fatto, AppError> {
    let brano = &lavoro.brano;
    if brano.segmento.is_some() {
        return Ok(Fatto::Uguale);
    }
    let comune = comune(app)?;
    if !comune.associato(&lavoro.dispositivo) {
        return Err(AppError::new(ErrorCode::LontanoNonAssociato));
    }
    dati_permessi(&comune)?;
    let cartella = app
        .con_finestra(crate::procura::cartella_download)
        .flatten()
        .ok_or_else(|| {
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
    // Le parti di un arresto di prima, di questo dispositivo, in questa cartella:
    // il filo di un dispositivo è uno solo, e fra un brano e l'altro nessuna sua
    // parte sta scendendo.
    let tolte = togli_le_parti_orfane(&cartella, &lavoro.dispositivo);
    if tolte > 0 {
        nota!("[lontano] {tolte} parti rimaste di uno scaricamento interrotto, tolte");
    }
    let parte = nome_della_parte(&finale, &lavoro.dispositivo);

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
    let scritti = match copia(&pezzi, brano, &parte, giro, &mut avanza) {
        Ok(scritti) => scritti,
        Err(err) => {
            let _ = fs::remove_file(&parte);
            return Err(err);
        }
    };
    consegna(&parte, &cartella, &base, &estensione, scritti)
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
fn gira(app: &Nucleo, dispositivo: &str) {
    // Alla fine normale il filo è già uscito da `fili` nello stesso passo in cui ha
    // visto la sua coda vuota (`Coda::prendi`), e un filo nuovo può averne preso il
    // posto: la guardia agisce solo se questo cade in un panico. Altrimenti il
    // dispositivo restava in `fili`, «Scarica qui» accodava e non scaricava mai, e
    // la barra restava ferma fino al riavvio (S7-3).
    let _posto = crate::disparte::AllaFine::solo_se_cade(|| {
        prendi(&CODA).filo_caduto(dispositivo);
    });
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
        // Un brano che fa cadere `scarica_uno` è un brano fallito, e il filo
        // continua: `finito` lo conta comunque e gli altri brani scendono.
        let esito = crate::stato::sorvegliato(FILO_SCARICO, || scarica_uno(app, &lavoro, giro))
            .unwrap_or_else(|| {
                Err(AppError::new(ErrorCode::InternalUnexpected {
                    detail: Some("lo scaricamento di un brano è caduto".to_owned()),
                }))
            });
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
        app.con_finestra(crate::procura::scansiona_i_download);
    }
}

/// Mette in coda i brani di `chiave`, e fa partire il filo del suo dispositivo
/// se non gira già.
fn accoda(
    app: &Nucleo,
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
    if app
        .con_finestra(crate::procura::cartella_download)
        .flatten()
        .is_none()
    {
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
            .name(FILO_SCARICO.to_owned())
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

/// Quanto è grande un «Scarica qui», per chiedere prima di farlo (FR2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnteprimaScarico {
    /// Quanti brani scenderebbero davvero.
    pub brani: usize,
    /// La somma delle dimensioni che si sanno, in byte.
    pub byte: u64,
    /// Quanti di quei brani non hanno una dimensione nota (zero o meno: il
    /// catalogo dell'altro non l'aveva).
    pub senza_dimensione: usize,
}

impl AnteprimaScarico {
    /// Questa anteprima e `altra`, sommate: i pezzi di un insieme lungo.
    const fn con(self, altra: Self) -> Self {
        Self {
            brani: self.brani.saturating_add(altra.brani),
            byte: self.byte.saturating_add(altra.byte),
            senza_dimensione: self.senza_dimensione.saturating_add(altra.senza_dimensione),
        }
    }
}

/// Quel che la coda chiederebbe, dai gruppi di [`righe_da_scaricare`].
///
/// Conta come conta `Coda::aggiungi`: i brani di un'immagine CD si saltano, e
/// non sono né «brani» né byte. La somma si ferma al massimo invece di
/// traboccare: è un numero da dire a chi guarda, non un conto da quadrare.
fn anteprima_di(gruppi: &[(String, Vec<BranoRemoto>)]) -> AnteprimaScarico {
    let mut anteprima = AnteprimaScarico::default();
    for brano in gruppi
        .iter()
        .flat_map(|(_, brani)| brani)
        .filter(|brano| brano.segmento.is_none())
    {
        anteprima.brani = anteprima.brani.saturating_add(1);
        match u64::try_from(brano.dimensione) {
            Ok(dimensione) if dimensione > 0 => {
                anteprima.byte = anteprima.byte.saturating_add(dimensione);
            }
            _ => anteprima.senza_dimensione = anteprima.senza_dimensione.saturating_add(1),
        }
    }
    anteprima
}

/// Prima di «Scarica qui» su un insieme grande: quanti brani scenderebbero e
/// quanto spazio prendono, perché la finestra lo chieda. **Non accoda niente.**
///
/// Legge dalla stessa fonte di [`lontano_scarica_righe`] (`righe_da_scaricare`,
/// su una connessione a parte): vale la stessa regola sulle righe — quelle di
/// questa macchina e quelle che il proprietario non manda più non contano — e il
/// numero detto a chi conferma è quello che la coda poi prende.
///
/// A differenza di quello, non rifiuta una lista lunga: l'artista con la
/// discografia intera è proprio il caso per cui si chiede, e sono sola lettura.
/// Si legge a pezzi di [`MASSIMI_PER_VOLTA`] righe, perché ogni riga letta porta
/// con sé titolo, artista e album, e sessantamila insieme sono decine di
/// mebibyte per sapere un numero.
///
/// # Errori
///
/// Quelli del database.
#[tauri::command]
pub async fn lontano_scarica_anteprima(app: Nucleo, righe: Vec<i64>) -> Esito<AnteprimaScarico> {
    in_disparte(move || {
        let mut tutto = AnteprimaScarico::default();
        for pezzo in righe.chunks(MASSIMI_PER_VOLTA) {
            let gruppi = leggi_a_parte(&app, |c| righe_da_scaricare(c, pezzo))?;
            tutto = tutto.con(anteprima_di(&gruppi));
        }
        Ok(tutto)
    })
    .await
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
pub async fn lontano_scarica_righe(app: Nucleo, righe: Vec<i64>) -> Esito<StatoScaricoIpc> {
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
pub async fn lontano_scarico_annulla(app: Nucleo) -> Esito<StatoScaricoIpc> {
    in_disparte(move || {
        let stato = {
            let mut coda = prendi(&CODA);
            coda.annulla();
            coda.stato.clone()
        };
        annuncia(&app);
        Ok(stato)
    })
    .await
}

/// A che punto è, adesso: per la finestra che si apre a coda avviata.
#[tauri::command]
pub async fn lontano_scarico_stato() -> Esito<StatoScaricoIpc> {
    in_disparte(|| Ok(prendi(&CODA).stato.clone())).await
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

    /// FR2: la domanda prima di «Scarica qui» conta quel che la coda chiederebbe
    /// davvero — non i brani di un'immagine CD, che `Coda::aggiungi` salta — e
    /// somma solo le dimensioni che si sanno.
    #[test]
    fn l_anteprima_conta_quel_che_si_chiederebbe_e_somma_le_dimensioni_note() {
        let con = |titolo: &str, dimensione: i64| BranoRemoto {
            dimensione,
            ..brano(titolo)
        };
        let mut immagine = con("Traccia di un CD", 9_000_000);
        immagine.segmento = Some(rymembra_app::library::condivisa::Segmento {
            inizio_cd: 750,
            fine_cd: None,
        });
        let gruppi = vec![
            (
                "pc".to_owned(),
                vec![con("Uno", 5_000_000), con("Due", 0), immagine],
            ),
            (
                "telefono".to_owned(),
                vec![con("Tre", 1_000_000), con("Quattro", -1)],
            ),
        ];
        assert_eq!(
            anteprima_di(&gruppi),
            AnteprimaScarico {
                brani: 4,
                byte: 6_000_000,
                senza_dimensione: 2,
            },
            "i quattro che scendono; la traccia del CD no; due non hanno una dimensione"
        );
        assert_eq!(
            anteprima_di(&[]),
            AnteprimaScarico {
                brani: 0,
                byte: 0,
                senza_dimensione: 0,
            }
        );
    }

    /// FR2: la somma non gira intorno — quel che non ci sta si ferma al
    /// massimo — e i pezzi di un insieme lungo si sommano come un insieme solo.
    #[test]
    fn la_somma_dell_anteprima_non_trabocca_e_i_pezzi_si_sommano() {
        let enorme = |t: &str| BranoRemoto {
            dimensione: i64::MAX,
            ..brano(t)
        };
        let gruppi = vec![("pc".to_owned(), vec![enorme("A"), enorme("B"), enorme("C")])];
        let anteprima = anteprima_di(&gruppi);
        assert_eq!(anteprima.brani, 3);
        assert_eq!(
            anteprima.byte,
            u64::MAX,
            "tre volte i64::MAX non ci sta in u64"
        );

        let uno = AnteprimaScarico {
            brani: 5000,
            byte: 7,
            senza_dimensione: 1,
        };
        let due = AnteprimaScarico {
            brani: 12,
            byte: 3,
            senza_dimensione: 0,
        };
        assert_eq!(
            uno.con(due),
            AnteprimaScarico {
                brani: 5012,
                byte: 10,
                senza_dimensione: 1,
            }
        );
        assert_eq!(uno.con(AnteprimaScarico::default()), uno);
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
        // conta il disco, finché sta dentro la tolleranza ([`dimensione_ammessa`]).
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

    // ── S2-4: quel che l'altro dichiara non si crede sulla parola ───────────

    const MIB: u64 = 1024 * 1024;

    /// Un altro che **dichiara** `totale` byte, vero o no, e ne manda quanti gliene
    /// si chiedono (`generoso`: anche oltre quel che ha dichiarato). Dopo
    /// `al_massimo` domande smette con un errore suo: una copia che non lo ferma
    /// da sé non scrive gigabyte per dimostrarlo.
    #[derive(Debug)]
    struct Dichiara {
        totale: u64,
        generoso: bool,
        al_massimo: usize,
        domande: AtomicUsize,
    }

    impl Dichiara {
        fn nuovo(totale: u64) -> Self {
            Self {
                totale,
                generoso: false,
                al_massimo: 3,
                domande: AtomicUsize::new(0),
            }
        }
    }

    impl Pezzi for Dichiara {
        fn pezzo(
            &self,
            _brano: i64,
            da: u64,
            quanti: u64,
        ) -> Result<(Vec<u8>, u64, String), AppError> {
            if self.domande.fetch_add(1, Ordering::Relaxed) >= self.al_massimo {
                return Err(AppError::new(ErrorCode::InternalAborted {
                    what: Some("la prova ha visto abbastanza".to_owned()),
                }));
            }
            let quanti = if self.generoso {
                quanti
            } else {
                quanti.min(self.totale.saturating_sub(da))
            };
            Ok((
                vec![7; usize::try_from(quanti).unwrap()],
                self.totale,
                "v1".to_owned(),
            ))
        }
    }

    /// Un brano di cui il catalogo dice `dimensione` byte.
    fn con_dimensione(dimensione: i64) -> BranoRemoto {
        BranoRemoto {
            dimensione,
            ..brano("X")
        }
    }

    fn copia_di_prova(
        pezzi: &impl Pezzi,
        brano: &BranoRemoto,
        parte: &Path,
    ) -> Result<u64, AppError> {
        let giro = prendi(&CODA).giro;
        copia(pezzi, brano, parte, giro, &mut |_, _| {})
    }

    /// L'altro dice, nella prima risposta, un totale che con quello del suo
    /// catalogo non c'entra: mille volte tanto. Prima la copia scriveva finestre
    /// da 2 MiB finché il disco non era pieno, o l'utente premeva «Annulla».
    #[test]
    fn una_dimensione_che_non_e_quella_del_catalogo_ferma_la_copia() {
        let cartella = tempfile::tempdir().unwrap();
        for dichiarato in [8_000_000_u64 * 1000, 1000] {
            let parte = cartella.path().join(format!("{dichiarato}.flac.part"));
            let pezzi = Dichiara::nuovo(dichiarato);
            let err = copia_di_prova(&pezzi, &con_dimensione(8_000_000), &parte).unwrap_err();
            assert_eq!(
                err.code().kind().code(),
                "lontano.fileCambiato",
                "dichiarati {dichiarato} byte contro 8000000 del catalogo"
            );
            assert_eq!(
                pezzi.domande.load(Ordering::Relaxed),
                1,
                "si ferma alla prima risposta"
            );
            assert_eq!(
                fs::metadata(&parte).unwrap().len(),
                0,
                "né un byte né spazio riservato: {dichiarato}"
            );
        }
    }

    /// Un totale pari a zero non è un brano: il file `.part` vuoto veniva
    /// rinominato e contato come «scritto», e la scansione lo prendeva per un
    /// `.mp3` illeggibile.
    #[test]
    fn un_file_dichiarato_vuoto_non_e_riuscito() {
        let cartella = tempfile::tempdir().unwrap();
        for catalogo in [8_000_000, 0] {
            let parte = cartella.path().join(format!("{catalogo}.flac.part"));
            let err =
                copia_di_prova(&Dichiara::nuovo(0), &con_dimensione(catalogo), &parte).unwrap_err();
            assert_eq!(
                err.code().kind().code(),
                "lontano.fileCambiato",
                "catalogo {catalogo}"
            );
        }
    }

    /// Un catalogo che non sa la dimensione (zero) non lascia l'altro libero di
    /// dire quel che vuole: c'è un tetto per file.
    #[test]
    fn un_file_oltre_il_tetto_non_si_scrive_nemmeno_senza_una_dimensione_nel_catalogo() {
        let cartella = tempfile::tempdir().unwrap();
        let parte = cartella.path().join("x.flac.part");
        let enorme = TETTO_PER_FILE + MIB;
        let err = copia_di_prova(&Dichiara::nuovo(enorme), &con_dimensione(0), &parte).unwrap_err();
        assert_eq!(err.code().kind().code(), "lontano.protocollo");
        assert_eq!(fs::metadata(&parte).unwrap().len(), 0);

        // E lo stesso quando è il catalogo a dire l'enorme e l'altro è d'accordo.
        let parte = cartella.path().join("y.flac.part");
        let pezzi = Dichiara::nuovo(enorme);
        let err = copia_di_prova(
            &pezzi,
            &con_dimensione(i64::try_from(enorme).unwrap()),
            &parte,
        )
        .unwrap_err();
        assert_eq!(err.code().kind().code(), "lontano.protocollo");
        assert_eq!(pezzi.domande.load(Ordering::Relaxed), 1);
    }

    /// La tolleranza è larga — un file ritaggato dopo l'ultimo giro del catalogo
    /// cambia di qualche mebibyte — e il controllo di prova: una copia che sta
    /// nel margine passa, e i byte sono quelli del disco, non quelli dell'elenco.
    #[test]
    fn una_dimensione_poco_diversa_dal_catalogo_passa() {
        let cartella = tempfile::tempdir().unwrap();
        let parte = cartella.path().join("x.flac.part");
        let vera = 8_100_000_u64;
        let scritti = copia_di_prova(
            &Dichiara {
                al_massimo: 50,
                ..Dichiara::nuovo(vera)
            },
            &con_dimensione(8_000_000),
            &parte,
        )
        .unwrap();
        assert_eq!(scritti, vera);
        assert_eq!(fs::metadata(&parte).unwrap().len(), vera);
    }

    /// Un altro che manda più byte di quanti ne ha dichiarati non fa crescere il
    /// file oltre il totale: si scrive fino a lì, e non un byte di più.
    #[test]
    fn una_risposta_piu_lunga_del_dichiarato_non_scrive_oltre() {
        let cartella = tempfile::tempdir().unwrap();
        let parte = cartella.path().join("x.flac.part");
        let pezzi = Dichiara {
            generoso: true,
            ..Dichiara::nuovo(1000)
        };
        let scritti = copia_di_prova(&pezzi, &con_dimensione(1000), &parte).unwrap();
        assert_eq!(scritti, 1000);
        assert_eq!(fs::metadata(&parte).unwrap().len(), 1000);
    }

    /// Un altro che si ferma prima della fine — tre byte e poi niente — non è un
    /// brano scritto: la copia dice che non è riuscita.
    #[test]
    fn un_file_che_si_ferma_prima_della_fine_non_riesce() {
        #[derive(Debug)]
        struct SiFerma(AtomicUsize);
        impl Pezzi for SiFerma {
            fn pezzo(
                &self,
                _brano: i64,
                _da: u64,
                _quanti: u64,
            ) -> Result<(Vec<u8>, u64, String), AppError> {
                let prima = self.0.fetch_add(1, Ordering::Relaxed) == 0;
                Ok((
                    if prima { vec![1, 2, 3] } else { Vec::new() },
                    10_000,
                    "v1".to_owned(),
                ))
            }
        }
        let cartella = tempfile::tempdir().unwrap();
        let parte = cartella.path().join("x.flac.part");
        let err = copia_di_prova(
            &SiFerma(AtomicUsize::new(0)),
            &con_dimensione(10_000),
            &parte,
        )
        .unwrap_err();
        assert_eq!(err.code().kind().code(), "lontano.protocollo");
    }

    // ── S2-10: il `.part` è di una coda, e il nome finale non si sovrascrive ─

    fn chiave(c: char) -> String {
        c.to_string().repeat(64)
    }

    /// Due dispositivi che scaricano lo stesso brano — stesso artista, album,
    /// titolo ed estensione — sceglievano lo stesso `.part` e lo scrivevano
    /// insieme: un file fatto di due.
    #[test]
    fn le_parti_di_due_dispositivi_hanno_nomi_diversi() {
        let finale = Path::new("musica").join("x.flac");
        let del_pc = nome_della_parte(&finale, &chiave('a'));
        let del_tablet = nome_della_parte(&finale, &chiave('b'));
        assert_ne!(del_pc, del_tablet);
        for parte in [&del_pc, &del_tablet] {
            let nome = parte.file_name().unwrap().to_string_lossy();
            assert!(nome.starts_with("x.flac."), "{nome}");
            assert!(nome.ends_with(".part"), "la scansione non lo legge: {nome}");
        }
        assert_eq!(del_pc, nome_della_parte(&finale, &chiave('a')), "è stabile");
    }

    /// Una parte che c'è già non si riapre troncandola: la sua è un'altra coda,
    /// o un arresto di prima che nessuno ha ripulito.
    #[test]
    fn la_parte_non_si_apre_sopra_una_parte_che_c_e() {
        let cartella = tempfile::tempdir().unwrap();
        let parte = cartella.path().join("x.flac.aaaaaaaa.part");
        fs::write(&parte, b"di un altro").unwrap();
        let err = copia_di_prova(&Dichiara::nuovo(10), &con_dimensione(10), &parte).unwrap_err();
        assert_eq!(err.code().kind().code(), "fs.writeFailed");
        assert_eq!(fs::read(&parte).unwrap(), b"di un altro");
    }

    /// Il nome finale lo prende chi arriva prima: un file che è comparso durante
    /// lo scaricamento — fatto dall'utente, o dall'altra coda — non si
    /// sostituisce, e il brano nuovo prende ` (2)`.
    #[test]
    fn la_consegna_non_sovrascrive_un_file_che_c_e() {
        let cartella = tempfile::tempdir().unwrap();
        fs::write(cartella.path().join("x.flac"), b"quello mio").unwrap();
        let parte = cartella.path().join("x.flac.aaaaaaaa.part");
        fs::write(&parte, b"nove byte").unwrap();
        let esito = consegna(&parte, cartella.path(), "x", "flac", 9).unwrap();
        assert_eq!(
            esito,
            Fatto::Scritto(cartella.path().join("x (2).flac")),
            "il brano nuovo prende la variante"
        );
        assert_eq!(
            fs::read(cartella.path().join("x.flac")).unwrap(),
            b"quello mio"
        );
        assert_eq!(
            fs::read(cartella.path().join("x (2).flac")).unwrap(),
            b"nove byte"
        );
        assert!(!parte.exists());
    }

    /// Se invece il file che c'è è lo stesso brano — la stessa dimensione —, la
    /// consegna lo dice e toglie la sua parte: la seconda coda non lascia un
    /// doppione.
    #[test]
    fn la_consegna_di_un_brano_che_c_e_gia_lo_dice_e_toglie_la_parte() {
        let cartella = tempfile::tempdir().unwrap();
        fs::write(cartella.path().join("x.flac"), b"nove byte").unwrap();
        let parte = cartella.path().join("x.flac.bbbbbbbb.part");
        fs::write(&parte, b"nove byte").unwrap();
        let esito = consegna(&parte, cartella.path(), "x", "flac", 9).unwrap();
        assert_eq!(esito, Fatto::Uguale);
        assert!(!parte.exists(), "la parte non resta");
        assert!(!cartella.path().join("x (2).flac").exists());
    }

    /// Un arresto a metà lascia una `.part` che nessuno rinominerà mai. All'avvio
    /// della coda si tolgono **quelle del dispositivo** — e solo quelle: un
    /// `.part` di un altro dispositivo può essere in corso, e uno che non è nostro
    /// (un programma di torrent, un browser) non si tocca.
    #[test]
    fn le_parti_orfane_di_un_dispositivo_si_tolgono_e_solo_quelle() {
        let cartella = tempfile::tempdir().unwrap();
        let dir = cartella.path();
        for nome in [
            "a.flac.aaaaaaaa.part",
            "b.mp3.aaaaaaaa.part",
            "a.flac.bbbbbbbb.part",
            "scarico.part",
            "a.flac",
        ] {
            fs::write(dir.join(nome), b"x").unwrap();
        }
        // Una cartella col nome di una parte non è un file nostro.
        fs::create_dir(dir.join("Album.aaaaaaaa.part")).unwrap();

        assert_eq!(togli_le_parti_orfane(dir, &chiave('a')), 2);

        let mut resta: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|voce| voce.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        resta.sort();
        assert_eq!(
            resta,
            [
                "Album.aaaaaaaa.part",
                "a.flac",
                "a.flac.bbbbbbbb.part",
                "scarico.part"
            ]
        );
        // Una cartella che non c'è non è un guasto.
        assert_eq!(togli_le_parti_orfane(&dir.join("non-c-e"), &chiave('a')), 0);
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

    /// S7-3: un filo caduto non lascia la coda del suo dispositivo senza chi la
    /// svuoti: dopo `filo_caduto` un brano nuovo fa ripartire un filo, e i brani che
    /// aspettavano ci sono ancora.
    #[test]
    fn un_filo_caduto_lascia_ripartire_la_coda_del_dispositivo() {
        let mut coda = Coda::nuova();
        assert!(
            coda.aggiungi("d", vec![brano("a")]),
            "il primo brano fa partire il filo"
        );
        assert!(
            !coda.aggiungi("d", vec![brano("b")]),
            "con un filo vivo ne basta uno"
        );
        coda.filo_caduto("d");
        assert!(
            coda.aggiungi("d", vec![brano("c")]),
            "dopo la caduta il dispositivo non ha più un filo, e un brano nuovo ne fa partire uno"
        );
        assert_eq!(
            coda.in_attesa(),
            3,
            "i brani che aspettavano ci sono ancora"
        );
    }

    /// S7-3: il filo di un dispositivo libera il suo posto in `fili` se cade in
    /// un panico, e un brano che cade non lo porta via: `finito` conta comunque
    /// quel brano. Senza, «Scarica qui» accodava e non scaricava mai, e la barra
    /// restava ferma fino al riavvio.
    #[test]
    fn il_filo_dello_scarico_libera_il_posto_anche_in_un_panico() {
        let sorgente = include_str!("scarico.rs");
        let sorgente = sorgente
            .split("\n#[cfg(test)]\nmod prove")
            .next()
            .unwrap_or(sorgente);
        let inizio = sorgente.find("fn gira(").expect("gira");
        let resto = &sorgente[inizio..];
        let corpo = &resto[..resto.find("\n}\n").expect("fine")];
        assert!(
            corpo.contains("AllaFine::solo_se_cade("),
            "il filo dello scarico non libera `fili` in un panico"
        );
        assert!(
            corpo.contains("sorvegliato("),
            "un brano che cade porta via il filo e la sua coda"
        );
    }
}
