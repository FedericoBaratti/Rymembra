//! Le cartelle di musica che stanno su un servizio cloud.
//!
//! Il backup (il resto di questo crate) scrive nella cartella privata di
//! Rymembra su Drive; questo modulo **legge** le cartelle dell'utente: le
//! elenca, ne segue i cambiamenti e ne serve i byte a chi suona. Non scrive
//! niente da nessuna parte — gli scope che chiede sono di sola lettura — e non
//! vede il database: le voci che produce sono [`VoceRemota`] del dominio, e lo
//! specchio di `rymembra-app` le mette dove vanno.
//!
//! # Il tratto
//!
//! [`Deposito`] è la forma comune dei servizi: chi è l'account, quali cartelle
//! ha, l'elenco di una cartella, i cambiamenti da un cursore, i byte di un
//! file. Le implementazioni saranno cinque (OneDrive, Dropbox, Google Drive,
//! WebDAV, MEGA): non è un'astrazione costruita per un caso solo, ed è il
//! motivo per cui le scelte di ciascun servizio stanno nel suo file e non qui.
//!
//! # Le convenzioni che i servizi rispettano
//!
//! - **La cima del drive** ha `genitore = None` e **nome vuoto**. Non è un
//!   segmento del percorso: `componi` salta i segmenti vuoti, e così
//!   `nuvola://onedrive-…/Musica/…` non diventa `nuvola://onedrive-…/root/Musica/…`
//!   qualunque cosa lo specchio decida di fare con la cima.
//! - **Una cima sola per conto.** Una voce senza una madre visibile che non è
//!   la cima — una cartella che altri condividono con un account Google — va
//!   sotto una cartella virtuale del servizio, figlia della cima
//!   ([`rymembra_domain::nuvola::e_virtuale`]): due cime avrebbero lo stesso
//!   percorso. Nell'elenco e nei cambiamenti.
//! - **[`Deposito::elenca`]** comincia con la catena degli antenati della
//!   cartella scelta, dalla cima del drive in giù, e può lasciare fuori i file
//!   che non interessano ([`rymembra_domain::nuvola::interessa`]).
//!   **[`Deposito::cambiamenti`]** no: un file rinominato in qualcosa che non
//!   interessa è un'informazione (lo specchio lo toglie), e va consegnato.
//! - **La versione** è quel che cambia quando cambia il contenuto (il `cTag` di
//!   Graph, la `rev` di Dropbox, l'md5): decide se una copia in cache vale
//!   ancora. Un rinomina non deve cambiarla.
//! - **Il cursore scaduto** è un errore `net.http` con stato `410`, comunque il
//!   servizio lo dica (Graph con `410 resyncRequired`, Dropbox con un `409
//!   reset`, Drive con un token non valido): vedi [`cursore_scaduto`]. Chi lo
//!   riceve rifà l'elenco completo e lo confronta con lo specchio; non svuota
//!   mai per riempire di nuovo. Un `410` non si ritenta da solo (vedi
//!   `rymembra_net::http`), quindi non può nascondersi dietro un tentativo.
//!
//! # I segreti
//!
//! Il refresh token di un conto sta nel portachiavi, sotto
//! [`crate::portachiavi::nuvola_refresh`]; l'access token solo in memoria, in
//! [`Accesso`]. Nessuna delle due cose finisce in un errore.

pub mod accesso;
pub mod archivio;
pub mod cache;
pub mod dropbox;
pub mod gdrive;
pub mod lettura;
pub mod onedrive;
pub mod webdav;

use std::ops::ControlFlow;
use std::time::Duration;

use rymembra_app::files::Limiti;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola::{self, Cambio, Servizio, VoceRemota};
use rymembra_domain::nuvola_app::Piattaforma;
use rymembra_net::flusso::Sorgente;

pub use accesso::{Accesso, Permesso};
pub use archivio::{ArchivioNuvola, CopieComplete, Depositi};
pub use cache::{Cache, Chiave, ConCache, TETTO_DI_SERIE};
pub use dropbox::Dropbox;
pub use gdrive::GoogleDrive;
pub use lettura::{LettoreTag, TETTO_LETTURA, errore_remoto};
pub use onedrive::OneDrive;
pub use webdav::WebDav;

/// Chi è l'account dall'altra parte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identita {
    /// L'identificativo che non cambia mai per quell'account: l'id del drive
    /// per Graph, l'`account_id` per Dropbox, il `permissionId` per Google.
    /// Da qui viene il conto ([`conto_di`]).
    pub id_stabile: String,
    /// Come mostrarlo: l'email, o il nome dell'account.
    pub nome: String,
}

/// Un servizio da cui si leggono cartelle di musica.
///
/// Tutto bloccante, come il resto di Rymembra: ogni conto ha il suo filo. Nessun
/// metodo riceve un lucchetto della libreria, e nessuno deve riceverlo: le
/// risposte di un servizio possono metterci decine di secondi.
pub trait Deposito: Send + Sync {
    /// Quale servizio.
    fn servizio(&self) -> Servizio;

    /// Spegne il permesso del conto, prima che il suo segreto si cancelli o
    /// che la sua app cambi: vedi [`Accesso::spegni`]. Chi non ha un permesso
    /// da rinfrescare (WebDAV, con la password in ogni richiesta) non fa
    /// niente.
    fn spegni(&self) {}

    /// Chi è l'account collegato.
    ///
    /// # Errori
    ///
    /// `cloud.authExpired` se il consenso non vale più, `net.*` per la rete.
    fn chi(&self) -> Result<Identita, AppError>;

    /// Le cartelle dentro `dentro` (la cima del drive se `None`), per il
    /// selettore. Solo cartelle, e solo un livello.
    ///
    /// # Errori
    ///
    /// Come [`Self::chi`], e `cloud.notFound` per una cartella che non c'è più.
    fn cartelle(&self, dentro: Option<&str>) -> Result<Vec<VoceRemota>, AppError>;

    /// Il cursore dei cambiamenti **adesso**.
    ///
    /// Si prende **prima** dell'elenco iniziale: quel che cambia mentre
    /// l'elenco scorre arriva poi coi cambiamenti, e al peggio due volte —
    /// mai zero.
    ///
    /// # Errori
    ///
    /// Come [`Self::chi`].
    fn cursore_adesso(&self) -> Result<String, AppError>;

    /// L'elenco di tutto quel che sta sotto `cima`, a pagine.
    ///
    /// La prima pagina comincia con la catena degli antenati di `cima` — dalla
    /// cima del drive in giù, `cima` compresa — perché lo specchio possa
    /// comporre i percorsi. `pagina` può dire basta (`Break`): l'elenco si
    /// ferma e il risultato è `Ok(false)`, «non completo». `Ok(true)` vuol
    /// dire che si è arrivati in fondo, ed è l'unico caso in cui chi chiama
    /// può togliere dallo specchio quel che non è arrivato.
    ///
    /// # Errori
    ///
    /// Come [`Self::cartelle`]. Un errore a metà vuol dire «non completo»: le
    /// pagine già consegnate restano buone, quelle mancanti non dicono niente.
    fn elenca(
        &self,
        cima: &str,
        pagina: &mut dyn FnMut(Vec<VoceRemota>) -> ControlFlow<()>,
    ) -> Result<bool, AppError>;

    /// I cambiamenti da `cursore`, e il cursore nuovo.
    ///
    /// `attesa` è quanto si può restare in ascolto se il servizio sa
    /// aspettare i cambiamenti invece di farsi chiedere (Dropbox, MEGA). Chi
    /// non sa farlo (Graph) risponde subito, e il ritmo lo tiene chi chiama.
    ///
    /// # Errori
    ///
    /// Un cursore che il servizio non accetta più è l'errore di
    /// [`cursore_scaduto`]; il resto come [`Self::chi`].
    fn cambiamenti(
        &self,
        cursore: &str,
        attesa: Duration,
    ) -> Result<(Vec<Cambio>, String), AppError>;

    /// I byte di un file, come [`Sorgente`]: per un `FlussoHttp`, una
    /// [`ConCache`] o un [`LettoreTag`].
    ///
    /// La sorgente gestisce da sé la scadenza dei permessi: un token scaduto
    /// si rinfresca, un indirizzo firmato scaduto si chiede di nuovo, una volta
    /// sola ciascuno.
    ///
    /// # Errori
    ///
    /// Quelli della costruzione (di solito nessuno: la prima richiesta parte
    /// alla prima lettura).
    fn sorgente(&self, voce: &VoceRemota) -> Result<Box<dyn Sorgente>, AppError>;

    /// Come la scansione legge da questo servizio. Vedi [`limiti_di`].
    fn limiti(&self) -> Limiti {
        limiti_di(self.servizio())
    }

    /// Dove vedere questa voce sul sito del servizio, per «Apri in OneDrive».
    ///
    /// `None` quando non si sa comporre senza chiederlo alla rete.
    fn indirizzo_web(&self, voce: &VoceRemota) -> Option<String>;
}

/// Come la scansione legge da un servizio.
///
/// # I numeri, e perché non quelli del disco
///
/// - **Otto letture insieme** (quattro per MEGA e WebDAV): una lettura dei tag
///   è una o due richieste HTTPS da qualche centinaio di millisecondi, e la
///   latenza si nasconde solo tenendone più in volo. MEGA e WebDAV ne reggono
///   meno: il primo limita per indirizzo, il secondo è spesso un server di casa.
/// - **Sessanta secondi per file**: un `429` chiede di aspettare fino a mezzo
///   minuto, e i venticinque del disco abbandonerebbero la radice al primo
///   rallentamento.
/// - **Lotti da cinquanta**: le righe arrivano a gocce, e la finestra si
///   aggiorna a ogni lotto.
/// - **Niente «uno alla volta finché non risponde»**: è la prudenza di una share
///   appena svegliata, e un servizio cloud è sempre sveglio.
/// - **Prima le righe**: i brani entrano dall'elenco, prima della lettura dei
///   tag.
/// - **Duecento letture per giro**: la scansione cede il turno, e una cartella
///   locale non aspetta dietro ventimila file remoti.
#[must_use]
pub const fn limiti_di(servizio: Servizio) -> Limiti {
    let in_volo = match servizio {
        Servizio::Mega | Servizio::MegaLink | Servizio::WebDav => 4,
        Servizio::OneDrive | Servizio::GoogleDrive | Servizio::Dropbox => 8,
    };
    Limiti {
        in_volo,
        per_file: Duration::from_secs(60),
        scadenze_di_fila: 3,
        lotto: 50,
        prima_uno_alla_volta: false,
        prima_le_righe: true,
        letture_per_giro: Some(200),
    }
}

/// Il conto di un account: `<servizio>-<impronta>`.
///
/// L'impronta sono i primi quaranta bit di BLAKE3 dell'id stabile, in base32
/// minuscola (l'alfabeto della RFC 4648, `a-z2-7`): otto caratteri. Perché
/// un'impronta e non l'id stesso lo spiega `rymembra_domain::nuvola` — il conto
/// viaggia coi percorsi, e deve essere lo stesso su ogni PC dove quell'account
/// si collega; perché BLAKE3 è che è già quella del contenuto, e un secondo
/// algoritmo di impronta nello stesso programma è un secondo modo di sbagliare.
///
/// L'impronta non nasconde niente a chi la vuole rovesciare (l'id di un drive
/// non è un segreto): serve a essere corta, stabile e senza caratteri che un
/// percorso non ammette.
#[must_use]
pub fn conto_di(servizio: Servizio, id_stabile: &str) -> String {
    let impronta_intera = blake3::hash(id_stabile.as_bytes());
    let impronta = base32_minuscola(impronta_intera.as_bytes().get(..5).unwrap_or_default());
    // Otto caratteri dell'alfabeto giusto, sempre: il ripiego non scatta mai,
    // ed è scritto solo perché il dominio risponde con un `Option`.
    nuvola::conto(servizio, &impronta)
        .unwrap_or_else(|| format!("{}-{impronta}", servizio.chiave()))
}

/// L'identificativo di un'app nuova dell'utente: `<piattaforma>-<impronta>`.
///
/// Casuale e non progressivo: l'identificativo dà il nome alla voce del
/// portachiavi col segreto dell'app ([`crate::portachiavi::nuvola_app_segreto`]),
/// e un numero riusato dopo aver tolto un'app darebbe a quella nuova il segreto
/// rimasto della vecchia. Quaranta bit della casualità del sistema, nella
/// stessa base32 dei conti, così si legge allo stesso modo in un diario.
///
/// # Errori
///
/// `internal.unexpected` se il sistema non fornisce casualità.
pub fn nuova_app(piattaforma: Piattaforma) -> Result<String, AppError> {
    let casuale = rymembra_oauth::identificativo()?;
    let impronta = base32_minuscola(
        blake3::hash(casuale.as_bytes())
            .as_bytes()
            .get(..5)
            .unwrap_or_default(),
    );
    Ok(format!("{}-{impronta}", piattaforma.chiave()))
}

/// L'alfabeto della base32 della RFC 4648, in minuscolo.
const ALFABETO_BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Byte in base32 minuscola, senza riempimento.
///
/// Scritta a mano perché ne servono otto caratteri, e una dipendenza per otto
/// caratteri è più lunga di queste righe.
fn base32_minuscola(byte: &[u8]) -> String {
    let mut fuori = String::new();
    let mut accumulo: u32 = 0;
    let mut bit: u32 = 0;
    for b in byte {
        accumulo = (accumulo << 8) | u32::from(*b);
        bit += 8;
        while bit >= 5 {
            bit -= 5;
            let indice = usize::try_from((accumulo >> bit) & 0x1F).unwrap_or(0);
            fuori.push(char::from(
                ALFABETO_BASE32.get(indice).copied().unwrap_or(b'a'),
            ));
        }
        // Tiene solo i bit non ancora scritti: senza, l'accumulo crescerebbe
        // fino a perdere quelli in testa.
        accumulo &= (1 << bit) - 1;
    }
    if bit > 0 {
        let indice = usize::try_from((accumulo << (5 - bit)) & 0x1F).unwrap_or(0);
        fuori.push(char::from(
            ALFABETO_BASE32.get(indice).copied().unwrap_or(b'a'),
        ));
    }
    fuori
}

/// È l'errore di un cursore che il servizio non accetta più?
///
/// Il segnale è uno solo per tutti i servizi: `net.http` con stato `410`. Vedi
/// il `//!`.
#[must_use]
pub fn cursore_scaduto(err: &AppError) -> bool {
    matches!(err.code(), ErrorCode::NetHttp { status: 410, .. })
}

/// L'errore di un cursore che il servizio non accetta più.
///
/// Senza indirizzo: il cursore di Graph è un indirizzo intero, e quel che
/// conta dire è *perché* lo si butta, non dove stava.
#[must_use]
pub fn errore_cursore_scaduto(perche: &str) -> AppError {
    AppError::new(ErrorCode::NetHttp {
        status: 410,
        url: None,
    })
    .with_cause(format!("il cursore dei cambiamenti non vale più: {perche}"))
}

/// Il rielenco periodico di un servizio che non ha un flusso dei cambiamenti
/// (WebDAV): lo stesso segnale del cursore scaduto, con la causa che il diario
/// mostra — è un ripasso previsto, non un guasto.
#[must_use]
pub fn ripasso_periodico() -> AppError {
    AppError::new(ErrorCode::NetHttp {
        status: 410,
        url: None,
    })
    .with_cause("ripasso periodico")
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_base32_e_quella_della_rfc() {
        // I vettori della RFC 4648, § 10, in minuscolo e senza riempimento.
        assert_eq!(base32_minuscola(b""), "");
        assert_eq!(base32_minuscola(b"f"), "my");
        assert_eq!(base32_minuscola(b"fo"), "mzxq");
        assert_eq!(base32_minuscola(b"foo"), "mzxw6");
        assert_eq!(base32_minuscola(b"foob"), "mzxw6yq");
        assert_eq!(base32_minuscola(b"fooba"), "mzxw6ytb");
        assert_eq!(base32_minuscola(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn un_app_nuova_ha_un_nome_nuovo() {
        let una = nuova_app(Piattaforma::Google).expect("casualità");
        let altra = nuova_app(Piattaforma::Google).expect("casualità");
        assert_ne!(una, altra);
        let impronta = una.strip_prefix("google-").expect("la piattaforma davanti");
        assert_eq!(impronta.len(), nuvola::LUNGHEZZA_IMPRONTA);
        assert!(impronta.bytes().all(|b| ALFABETO_BASE32.contains(&b)));
        assert!(
            nuova_app(Piattaforma::Dropbox)
                .expect("casualità")
                .starts_with("dropbox-")
        );
    }

    #[test]
    fn il_conto_e_stabile_corto_e_del_servizio() {
        let conto = conto_di(Servizio::OneDrive, "d3e1f2a3b4c5");
        assert_eq!(
            conto,
            conto_di(Servizio::OneDrive, "d3e1f2a3b4c5"),
            "lo stesso account dà lo stesso conto, su ogni PC"
        );
        assert!(nuvola::e_conto(&conto), "{conto}");
        assert_eq!(Servizio::del_conto(&conto), Some(Servizio::OneDrive));
        assert_eq!(conto.len(), "onedrive-".len() + nuvola::LUNGHEZZA_IMPRONTA);
        assert_ne!(conto, conto_di(Servizio::OneDrive, "d3e1f2a3b4c6"));
        let dropbox = conto_di(Servizio::Dropbox, "d3e1f2a3b4c5");
        assert!(dropbox.starts_with("dropbox-"));
        assert_eq!(
            dropbox.strip_prefix("dropbox-"),
            conto.strip_prefix("onedrive-"),
            "l'impronta è dell'id: il servizio sta nel prefisso"
        );
        // L'impronta è proprio BLAKE3: i primi cinque byte in base32.
        let attesa = base32_minuscola(
            blake3::hash(b"d3e1f2a3b4c5")
                .as_bytes()
                .get(..5)
                .unwrap_or_default(),
        );
        assert_eq!(conto, format!("onedrive-{attesa}"));
    }

    #[test]
    fn i_limiti_del_cloud_non_sono_quelli_del_disco() {
        let onedrive = limiti_di(Servizio::OneDrive);
        assert_eq!(onedrive.in_volo, 8);
        assert_eq!(onedrive.per_file, Duration::from_secs(60));
        assert_eq!(onedrive.scadenze_di_fila, 3);
        assert_eq!(onedrive.lotto, 50);
        assert!(!onedrive.prima_uno_alla_volta);
        assert!(onedrive.prima_le_righe);
        assert_eq!(onedrive.letture_per_giro, Some(200));
        assert_eq!(limiti_di(Servizio::Mega).in_volo, 4);
        assert_eq!(limiti_di(Servizio::MegaLink).in_volo, 4);
        assert_eq!(limiti_di(Servizio::WebDav).in_volo, 4);
        assert_eq!(limiti_di(Servizio::Dropbox).in_volo, 8);
        assert_ne!(onedrive, Limiti::DISCO);
    }

    #[test]
    fn il_cursore_scaduto_si_riconosce_e_non_si_ritenta() {
        let err = errore_cursore_scaduto("resyncRequired");
        assert!(cursore_scaduto(&err));
        assert!(!err.is_retryable(), "un cursore scaduto non torna buono");
        assert!(!cursore_scaduto(&AppError::new(ErrorCode::NetHttp {
            status: 404,
            url: None
        })));
        assert!(!cursore_scaduto(&AppError::new(ErrorCode::CloudNotFound)));
    }
}
