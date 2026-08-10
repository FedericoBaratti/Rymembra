//! La passata di backup e quella di ripristino, messe in fila.
//!
//! Qui si decide **cosa** caricare e **in che ordine**; il come lo sanno
//! [`crate::drive`] e [`crate::pacchetti`], e il cosa significhi
//! [`aether_app::backup`].
//!
//! # Tre tipi di file, non un archivio unico
//!
//! ```text
//! appDataFolder/
//!   aether-libreria.v1.json.gz     ~70 KB per 1 400 brani, cambia in continuazione
//!   aether-skin-<id>.aeskin        fino a 20 MB, non cambia quasi mai
//!   aether-bozza-<id>.json.gz      il solo manifest della bozza
//! ```
//!
//! Un archivio solo sarebbe più semplice da scrivere e rispedirebbe venti
//! megabyte **a ogni cuoricino**. Separati, un voto messo costa settanta
//! chilobyte e una skin caricata una volta resta caricata.
//!
//! # «Non è cambiato niente», in una chiamata sola
//!
//! Ogni file porta la sua impronta in `appProperties`. Una sola `files.list`
//! restituisce tutto quel che possediamo con l'impronta che **noi** abbiamo
//! calcolato: confrontarla con quella di adesso dice, senza scaricare niente, se
//! c'è qualcosa da fare. Su una libreria ferma, una passata è una richiesta HTTP
//! e nient'altro.
//!
//! # Il file remoto non regredisce mai
//!
//! Prima di sovrascrivere si guarda **chi** l'ha scritto. Se è stato un altro
//! dispositivo, si scarica il suo, lo si fonde in memoria con il nostro e si
//! carica l'unione: gli ascolti dell'altro computer non si perdono per il solo
//! fatto che questo ha salvato per ultimo.
//!
//! Il database locale, invece, **non si tocca**. Per vedere qui quegli ascolti
//! bisogna premere «Ripristina», ed è deliberato: una sincronia che riscrive la
//! libreria da sola è una sincronia di cui non ci si accorge finché non ha
//! cancellato qualcosa.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::Path;

use aether_app::backup::{self, Contenuto, Salvataggio, StatoLocale};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::restore::RestorePlan;

use crate::drive::{Drive, FileRemoto};
use crate::pacchetti;

/// Il nome del file dei metadati.
///
/// La versione sta nel **nome** e non solo dentro: il giorno in cui il formato
/// cambiasse in modo non leggibile all'indietro, una versione vecchia di Aether
/// continuerebbe a trovare e a scrivere il suo `.v1`, invece di litigare con un
/// file che non capisce.
pub const NOME_LIBRERIA: &str = "aether-libreria.v1.json.gz";

/// Il prefisso dei pacchetti di skin.
const PREFISSO_SKIN: &str = "aether-skin-";

/// Il prefisso dei manifest di bozza.
const PREFISSO_BOZZA: &str = "aether-bozza-";

/// Quanto si accetta che un file compresso diventi una volta aperto.
///
/// Sessantaquattro mebibyte: molto più di qualunque backup vero, molto meno di
/// quel che serve a una bomba di decompressione per riempire la memoria. Il file
/// viene dalla rete, e «l'abbiamo scritto noi» non è una garanzia su cosa ci sia
/// dentro adesso.
const LIMITE_DECOMPRESSO: u64 = 64 * 1024 * 1024;

/// Il nome su Drive del pacchetto di una skin.
#[must_use]
pub fn nome_skin(id: &str) -> String {
    format!("{PREFISSO_SKIN}{id}.aeskin")
}

/// Il nome su Drive del manifest di una bozza.
#[must_use]
pub fn nome_bozza(id: &str) -> String {
    format!("{PREFISSO_BOZZA}{id}.json.gz")
}

/// L'identificatore dentro il nome di un pacchetto di skin.
#[must_use]
pub fn id_da_nome_skin(nome: &str) -> Option<&str> {
    nome.strip_prefix(PREFISSO_SKIN)?.strip_suffix(".aeskin")
}

/// L'identificatore dentro il nome di un manifest di bozza.
#[must_use]
pub fn id_da_nome_bozza(nome: &str) -> Option<&str> {
    nome.strip_prefix(PREFISSO_BOZZA)?.strip_suffix(".json.gz")
}

/// A che punto è una passata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cosa {
    /// Il file dei metadati.
    Metadati,
    /// I pacchetti di skin.
    Skin,
    /// I manifest delle bozze.
    Bozze,
}

impl Cosa {
    /// Il nome che attraversa l'IPC.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Metadati => "metadati",
            Self::Skin => "skin",
            Self::Bozze => "bozze",
        }
    }
}

/// Un passo avanti, da riferire a chi guarda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Avanzamento {
    /// Quanti ne sono stati fatti.
    pub fatti: usize,
    /// Quanti in tutto.
    pub totale: usize,
    /// Di cosa si tratta.
    pub cosa: Cosa,
}

/// Com'è andata una passata di salvataggio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passata {
    /// Quanti file sono stati caricati.
    pub caricati: usize,
    /// Quanti erano già lassù identici.
    pub saltati: usize,
    /// L'identificativo del file dei metadati su Drive.
    pub file_id: String,
    /// L'impronta del contenuto appena caricato.
    ///
    /// Chi chiama la scrive in `settings`: è ciò che permette alla passata
    /// successiva di sapere cosa aveva lasciato lassù.
    pub impronta: String,
}

/// Il backup scaricato, pronto per essere pianificato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaRipristinare {
    /// Quel che il file dei metadati dice.
    pub contenuto: Contenuto,
    /// Quando è stato scritto, in millisecondi.
    pub generato_ms: i64,
    /// Da quale dispositivo.
    pub generato_da: String,
    /// I pacchetti di skin disponibili, per identificatore.
    pub skin: BTreeMap<String, FileRemoto>,
    /// I manifest di bozza disponibili, per identificatore.
    pub bozze: BTreeMap<String, FileRemoto>,
}

/// Quanti file di skin e di bozza sono stati scritti sul disco.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scritti {
    /// Le skin installate adesso.
    pub skin: usize,
    /// Le bozze scritte adesso.
    pub bozze: usize,
    /// Gli identificatori che il piano chiedeva e che lassù non c'erano.
    ///
    /// Succede quando una passata di salvataggio si è interrotta fra il
    /// caricamento dei metadati e quello dei pacchetti. Non è un guasto del
    /// ripristino: è un pezzo che manca, e va detto invece che far fallire tutto
    /// il resto.
    pub mancanti: Vec<String>,
}

/// Salva la libreria e i pacchetti su Drive.
///
/// **Nessuna `Connection` attraversa questa funzione**, ed è la ragione per cui
/// il crate esiste separato: chi chiama legge il database sotto il lucchetto,
/// lo rilascia, e solo dopo arriva qui. Tutto ciò che segue dura secondi o
/// minuti, e il lettore continua a suonare.
///
/// # Errori
///
/// `net.*` per la rete; `net.badSchema` se lassù c'è un backup scritto da una
/// versione di Aether più nuova — nel qual caso **non si sovrascrive niente**.
pub fn salva(
    drive: &Drive,
    data_dir: &Path,
    stato: &StatoLocale,
    dispositivo: &str,
    adesso_ms: i64,
    mut avanzamento: impl FnMut(Avanzamento),
) -> Result<Passata, AppError> {
    let skin_locali = pacchetti::elenca_skin(data_dir);
    let bozze_locali = pacchetti::elenca_bozze(data_dir);
    let mut contenuto = backup::snapshot(stato, skin_locali.clone(), bozze_locali.clone());

    let remoti = per_nome(drive.elenca()?);
    let mut caricati = 0usize;
    let mut saltati = 0usize;

    // ── i metadati ──────────────────────────────────────────────────────────
    avanzamento(Avanzamento {
        fatti: 0,
        totale: 1,
        cosa: Cosa::Metadati,
    });
    let esistente = remoti.get(NOME_LIBRERIA);
    let mut nostra = backup::impronta(&backup::canonico(&contenuto)?);

    let file_id = match esistente {
        Some(remoto) if remoto.impronta.as_deref() == Some(nostra.as_str()) => {
            // La scorciatoia che rende gratuita una passata su una libreria
            // ferma: né scaricare né caricare, una sola richiesta in tutto.
            saltati = saltati.saturating_add(1);
            remoto.id.clone()
        }
        Some(remoto) => {
            if let Some(loro) = leggi_remoto(drive, remoto)? {
                if loro.generated_by != dispositivo {
                    // L'ha scritto un altro computer: si fonde in memoria e si
                    // carica l'unione. Il database locale resta intatto.
                    contenuto = backup::fondi(&contenuto, &loro.content);
                    nostra = backup::impronta(&backup::canonico(&contenuto)?);
                }
            }
            let salvataggio = Salvataggio::nuovo(contenuto, adesso_ms, dispositivo.to_owned());
            let byte = comprimi(&backup::serializza(&salvataggio)?)?;
            caricati = caricati.saturating_add(1);
            drive
                .carica(
                    NOME_LIBRERIA,
                    Some(&remoto.id),
                    "application/gzip",
                    &byte,
                    &nostra,
                )?
                .id
        }
        None => {
            let salvataggio = Salvataggio::nuovo(contenuto, adesso_ms, dispositivo.to_owned());
            let byte = comprimi(&backup::serializza(&salvataggio)?)?;
            caricati = caricati.saturating_add(1);
            drive
                .carica(NOME_LIBRERIA, None, "application/gzip", &byte, &nostra)?
                .id
        }
    };
    avanzamento(Avanzamento {
        fatti: 1,
        totale: 1,
        cosa: Cosa::Metadati,
    });

    // ── le skin ─────────────────────────────────────────────────────────────
    let totale = skin_locali.len();
    for (fatti, (id, impronta)) in skin_locali.iter().enumerate() {
        let nome = nome_skin(id);
        let remoto = remoti.get(&nome);
        if remoto.is_some_and(|voce| voce.impronta.as_deref() == Some(impronta.as_str())) {
            saltati = saltati.saturating_add(1);
        } else {
            let byte = pacchetti::leggi_skin(data_dir, id)?;
            drive.carica(
                &nome,
                remoto.map(|voce| voce.id.as_str()),
                "application/zip",
                &byte,
                impronta,
            )?;
            caricati = caricati.saturating_add(1);
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Skin,
        );
    }

    // ── le bozze ────────────────────────────────────────────────────────────
    let totale = bozze_locali.len();
    for (fatti, (id, impronta)) in bozze_locali.iter().enumerate() {
        let nome = nome_bozza(id);
        let remoto = remoti.get(&nome);
        if remoto.is_some_and(|voce| voce.impronta.as_deref() == Some(impronta.as_str())) {
            saltati = saltati.saturating_add(1);
        } else {
            let byte = comprimi(&pacchetti::leggi_bozza(data_dir, id)?)?;
            drive.carica(
                &nome,
                remoto.map(|voce| voce.id.as_str()),
                "application/gzip",
                &byte,
                impronta,
            )?;
            caricati = caricati.saturating_add(1);
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Bozze,
        );
    }

    Ok(Passata {
        caricati,
        saltati,
        file_id,
        impronta: nostra,
    })
}

/// Scarica il backup, senza applicare niente.
///
/// `None` se lassù non c'è ancora nessun file di metadati: è la condizione di
/// chi ha appena collegato l'account, non un guasto.
///
/// I pacchetti di skin **non** si scaricano qui: pesano fino a venti megabyte
/// l'uno, e il piano ha bisogno solo di sapere quali esistono e quanto pesano —
/// cose che stanno già nell'elenco. Si scaricano al momento di applicare.
///
/// # Errori
///
/// `net.*`; `sync.remoteCorrupt` se il file lassù non si legge, dopo averlo
/// messo da parte con un nome che dice cos'era.
pub fn scarica_salvataggio(drive: &Drive) -> Result<Option<DaRipristinare>, AppError> {
    let remoti = per_nome(drive.elenca()?);
    let Some(file) = remoti.get(NOME_LIBRERIA) else {
        return Ok(None);
    };
    let Some(salvataggio) = leggi_remoto(drive, file)? else {
        // Era corrotto ed è stato messo da parte: per chi ripristina è come se
        // non ci fosse, ed è la verità.
        return Ok(None);
    };

    let mut skin = BTreeMap::new();
    let mut bozze = BTreeMap::new();
    for (nome, voce) in &remoti {
        if let Some(id) = id_da_nome_skin(nome) {
            skin.insert(id.to_owned(), voce.clone());
        } else if let Some(id) = id_da_nome_bozza(nome) {
            bozze.insert(id.to_owned(), voce.clone());
        }
    }

    Ok(Some(DaRipristinare {
        contenuto: salvataggio.content,
        generato_ms: salvataggio.generated_at,
        generato_da: salvataggio.generated_by,
        skin,
        bozze,
    }))
}

/// Scarica e scrive le skin e le bozze che il piano propone.
///
/// **Prima della transazione sul database**, e non dopo: se si scrivesse prima
/// la transazione e poi un download fallisse, resterebbe committato uno
/// `skin.active` che punta a una skin che non esiste, e Aether si riaprirebbe
/// senza sapersi disegnare.
///
/// Un pacchetto che non si scarica o non si valida non ferma gli altri: finisce
/// in [`Scritti::mancanti`]. Un ripristino parziale è meglio di nessun
/// ripristino, purché si dica quale parte manca.
///
/// # Errori
///
/// `net.*` solo per i guasti che riguardano l'intera connessione.
pub fn scarica_pacchetti(
    drive: &Drive,
    data_dir: &Path,
    da: &DaRipristinare,
    piano: &RestorePlan,
    mut avanzamento: impl FnMut(Avanzamento),
) -> Result<Scritti, AppError> {
    let mut scritti = Scritti::default();

    let totale = piano.skins_to_install.len();
    for (fatti, id) in piano.skins_to_install.iter().enumerate() {
        match da.skin.get(id) {
            None => scritti.mancanti.push(nome_skin(id)),
            Some(remoto) => {
                let byte = drive.scarica(&remoto.id)?;
                match pacchetti::scrivi_skin(data_dir, id, &byte) {
                    Ok(true) => scritti.skin = scritti.skin.saturating_add(1),
                    // `false` è «c'era già»: non è un lavoro svolto né un
                    // guasto. `Err` è un pacchetto che non si valida, e vale
                    // come mancante: la skin non c'è, e dirlo è più utile che
                    // far fallire tutto il ripristino per un tema.
                    Ok(false) => {}
                    Err(_) => scritti.mancanti.push(nome_skin(id)),
                }
            }
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Skin,
        );
    }

    let totale = piano.drafts_to_write.len();
    for (fatti, id) in piano.drafts_to_write.iter().enumerate() {
        match da.bozze.get(id) {
            None => scritti.mancanti.push(nome_bozza(id)),
            Some(remoto) => {
                let byte = decomprimi(&drive.scarica(&remoto.id)?)?;
                match pacchetti::scrivi_bozza(data_dir, id, &byte) {
                    Ok(true) => scritti.bozze = scritti.bozze.saturating_add(1),
                    Ok(false) => {}
                    Err(_) => scritti.mancanti.push(nome_bozza(id)),
                }
            }
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Bozze,
        );
    }

    Ok(scritti)
}

/// I file remoti, per nome.
///
/// Drive **permette** due file con lo stesso nome nella stessa cartella. Se
/// succede — un caricamento interrotto a metà, due versioni che si sono
/// incrociate — vince l'ultimo che compare nell'elenco, e la passata successiva
/// sovrascriverà quello. Non è una scelta profonda: è che una libreria non può
/// avere due backup e bisogna pur sceglierne uno.
fn per_nome(file: Vec<FileRemoto>) -> BTreeMap<String, FileRemoto> {
    file.into_iter()
        .map(|voce| (voce.nome.clone(), voce))
        .collect()
}

/// Scarica e interpreta il file dei metadati.
///
/// `None` quando il file c'era ma era illeggibile: in quel caso lo si
/// **rinomina** invece di sovrascriverlo, e chi chiama procede come se non ci
/// fosse.
///
/// # Perché rinominare e non cancellare
///
/// Perché nella cartella privata dell'applicazione il cestino non esiste
/// (`notSupportedForAppDataFolderFiles`), quindi le uniche due possibilità sono
/// rinominare e cancellare davvero. E perché un backup illeggibile è l'unica
/// copia rimasta di qualcosa: se un giorno si scoprisse *perché* si era rotto,
/// averlo buttato via sarebbe irrimediabile. Costa settanta chilobyte tenerlo.
fn leggi_remoto(drive: &Drive, file: &FileRemoto) -> Result<Option<Salvataggio>, AppError> {
    let byte = drive.scarica(&file.id)?;
    let esito = decomprimi(&byte).and_then(|aperto| backup::interpreta(&aperto));
    match esito {
        Ok(salvataggio) => Ok(Some(salvataggio)),
        Err(err) if e_corrotto(&err) => {
            // Il rinomino è best-effort: se anche fallisse, la cosa importante è
            // non aver sovrascritto in silenzio.
            let messo_da_parte = format!("{NOME_LIBRERIA}.corrotto-{}", adesso_grezzo());
            drop(drive.rinomina(&file.id, &messo_da_parte));
            Ok(None)
        }
        // Una versione più nuova non è una corruzione, e **non** si mette da
        // parte: sovrascrivere il backup di un Aether più recente sarebbe
        // l'unico modo in cui questa funzione può perdere dati per davvero.
        Err(err) => Err(err),
    }
}

/// L'errore dice che il file remoto è illeggibile?
fn e_corrotto(err: &AppError) -> bool {
    matches!(err.code(), ErrorCode::SyncRemoteCorrupt)
}

/// Un numero crescente per distinguere i file messi da parte.
fn adesso_grezzo() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |quanto| quanto.as_millis())
}

/// Riferisce l'avanzamento, con la stessa strozzatura del resto dell'app.
///
/// Ogni venticinque elementi, e **sempre l'ultimo**. Senza la strozzatura, una
/// libreria con quaranta skin manderebbe quaranta eventi alla finestra; senza
/// «sempre l'ultimo», una barra di avanzamento si fermerebbe al 96%.
fn segnala(avanzamento: &mut impl FnMut(Avanzamento), fatti: usize, totale: usize, cosa: Cosa) {
    if fatti % 25 == 0 || fatti == totale {
        avanzamento(Avanzamento {
            fatti,
            totale,
            cosa,
        });
    }
}

/// Comprime con gzip.
///
/// # Errori
///
/// `internal.unexpected`: comprimere in memoria non ha un modo di fallire che
/// non sia un guasto nostro.
pub fn comprimi(dati: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut compressore = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    compressore
        .write_all(dati)
        .and_then(|()| compressore.finish())
        .map_err(|err| {
            AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("compressione del backup".to_owned()),
            })
            .with_cause(err.to_string())
        })
}

/// Apre un gzip, con un tetto a quanto può diventare.
///
/// # Errori
///
/// `sync.remoteCorrupt` se non è un gzip valido o se sfora il tetto. Sono due
/// cose diverse che si dicono nello stesso modo apposta: per chi riceve il file
/// sono la stessa situazione, cioè «lassù c'è qualcosa che non si può usare».
pub fn decomprimi(dati: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut fuori = Vec::new();
    let mut lettore = flate2::read::GzDecoder::new(dati).take(LIMITE_DECOMPRESSO + 1);
    lettore.read_to_end(&mut fuori).map_err(|err| {
        AppError::new(ErrorCode::SyncRemoteCorrupt).with_cause(format!("gzip illeggibile: {err}"))
    })?;
    if fuori.len() as u64 > LIMITE_DECOMPRESSO {
        return Err(AppError::new(ErrorCode::SyncRemoteCorrupt)
            .with_cause("il file compresso si apre oltre il limite"));
    }
    Ok(fuori)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_nomi_fanno_andata_e_ritorno() {
        assert_eq!(nome_skin("notte"), "aether-skin-notte.aeskin");
        assert_eq!(id_da_nome_skin(&nome_skin("notte")), Some("notte"));
        assert_eq!(nome_bozza("in-corso"), "aether-bozza-in-corso.json.gz");
        assert_eq!(id_da_nome_bozza(&nome_bozza("in-corso")), Some("in-corso"));
    }

    #[test]
    fn i_nomi_non_si_confondono_fra_loro() {
        // Il file dei metadati non deve essere scambiato per una bozza: hanno
        // entrambi il suffisso `.json.gz`, e una classificazione sbagliata lo
        // farebbe scrivere come manifest dentro `skin\bozze\`.
        assert_eq!(id_da_nome_bozza(NOME_LIBRERIA), None);
        assert_eq!(id_da_nome_skin(NOME_LIBRERIA), None);
        assert_eq!(id_da_nome_skin(&nome_bozza("x")), None);
        assert_eq!(id_da_nome_bozza(&nome_skin("x")), None);
    }

    #[test]
    fn la_versione_sta_nel_nome() {
        // Così una versione precedente continua a trovare il suo file invece di
        // litigare con uno che non capisce.
        assert!(NOME_LIBRERIA.contains(".v1."));
    }

    #[test]
    fn il_gzip_fa_andata_e_ritorno() {
        let dati = b"{\"tracks\":[]}".repeat(500);
        let stretto = comprimi(&dati).expect("compresso");
        assert!(stretto.len() < dati.len(), "deve pur comprimere");
        assert_eq!(decomprimi(&stretto), Ok(dati));
    }

    #[test]
    fn quel_che_non_e_gzip_e_remoto_corrotto() {
        // E non un `internal`: per chi riceve il file la situazione è «lassù c'è
        // qualcosa che non si può usare», ed è il codice che autorizza a metterlo
        // da parte e ricominciare.
        let err = decomprimi(b"non e gzip").expect_err("illeggibile");
        assert_eq!(err.code().kind().code(), "sync.remoteCorrupt");
    }

    #[test]
    fn una_bomba_di_decompressione_non_riempie_la_memoria() {
        // Un gzip di zeri: cento megabyte si comprimono in pochi chilobyte, e
        // senza il tetto verrebbero aperti tutti.
        let zeri = vec![0u8; usize::try_from(LIMITE_DECOMPRESSO + 1024).unwrap_or(usize::MAX)];
        let bomba = comprimi(&zeri).expect("compressa");
        assert!(bomba.len() < 200_000, "deve essere piccola per contare");
        let err = decomprimi(&bomba).expect_err("oltre il limite");
        assert_eq!(err.code().kind().code(), "sync.remoteCorrupt");
    }

    #[test]
    fn l_avanzamento_si_strozza_ma_non_perde_l_ultimo() {
        // Senza la strozzatura, quaranta skin sarebbero quaranta eventi; senza
        // «sempre l'ultimo», la barra si fermerebbe prima della fine.
        let mut visti = Vec::new();
        let mut riferisci = |passo: Avanzamento| visti.push(passo.fatti);
        for fatti in 1..=53usize {
            segnala(&mut riferisci, fatti, 53, Cosa::Skin);
        }
        assert_eq!(visti, [25, 50, 53]);
    }

    #[test]
    fn due_file_con_lo_stesso_nome_ne_lasciano_uno() {
        let doppio = vec![
            FileRemoto {
                id: "vecchio".to_owned(),
                nome: NOME_LIBRERIA.to_owned(),
                byte: 10,
                impronta: None,
            },
            FileRemoto {
                id: "nuovo".to_owned(),
                nome: NOME_LIBRERIA.to_owned(),
                byte: 20,
                impronta: Some("abc".to_owned()),
            },
        ];
        let per_nome = per_nome(doppio);
        assert_eq!(per_nome.len(), 1);
        assert_eq!(
            per_nome.get(NOME_LIBRERIA).map(|voce| voce.id.as_str()),
            Some("nuovo"),
            "vince l'ultimo, e la passata dopo sovrascrive quello"
        );
    }

    #[test]
    fn i_nomi_delle_cose_attraversano_l_ipc_come_stringhe_stabili() {
        // La finestra ci fa uno `switch` sopra: cambiarli è un cambio di
        // protocollo, non un ritocco di stile.
        assert_eq!(Cosa::Metadati.nome(), "metadati");
        assert_eq!(Cosa::Skin.nome(), "skin");
        assert_eq!(Cosa::Bozze.nome(), "bozze");
    }
}
