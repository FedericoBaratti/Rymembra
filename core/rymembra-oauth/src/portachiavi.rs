//! Dove stanno i token di aggiornamento: nel portachiavi del sistema, non nel
//! database.
//!
//! Lo dice lo schema — `001_baseline.sql`, sezione «impostazioni»: *«I SEGRETI
//! non stanno qui»* — e in questo modulo la regola smette di essere igiene e
//! diventa concreta. `rymembra.db` sta in `%APPDATA%`, leggibile da qualunque
//! processo giri come l'utente. E c'è una funzione di **backup**: il giorno in
//! cui a qualcuno venisse in mente di salvare anche il database, un token
//! scritto lì partirebbe per Drive dentro il backup di sé stesso.
//!
//! # Perché un tratto e non `keyring` e basta
//!
//! Il precedente diretto è `MusicFiles` in `rymembra_app::files`, indirezione nata
//! per lo stesso motivo: quel che su Windows è il Credential Manager, su Android
//! è il Keystore, e su una macchina di prova non deve essere niente. Con il
//! tratto, chi fa OAuth si prova per intero senza toccare il portachiavi vero —
//! che chiederebbe una password all'utente nel bel mezzo di `cargo test`.
//!
//! # Cosa non si fa mai
//!
//! Ripiegare sulla tabella `settings` quando il portachiavi non risponde.
//! Sarebbe la scelta comoda: il collegamento continuerebbe a funzionare e
//! nessuno se ne accorgerebbe. È precisamente il problema — un declassamento
//! silenzioso di una proprietà di sicurezza è peggio di una funzione che dice di
//! non poter partire. Se il portachiavi non c'è, il collegamento non parte e lo
//! dice.
//!
//! # Le voci della versione di prima
//!
//! Chi aveva già collegato un account prima del cambio di nome ha i suoi
//! segreti sotto un altro servizio, `rymembra_domain::eredita::IDENTIFICATORE`:
//! cambiare il nome del programma non sposta le voci del Credential Manager, e
//! ritrovarsi scollegati da Google, da Spotify e dall'IA per una questione di
//! etichetta sarebbe il modo peggiore di accorgersene. [`DiSistema`] allora
//! **legge** anche da lì, quando la voce nuova manca, e la porta nel servizio
//! di adesso. La regola è una sola, e vale per ogni chiave (anche quelle che
//! nascono a runtime, come `ia.<id>.chiave`): un segreto non si perde mai.
//!
//! - **Leggere**: la voce nuova, se c'è. Se manca, quella di prima: la si copia
//!   nel servizio nuovo e **solo se la scrittura riesce** si toglie la vecchia.
//!   Se non riesce la vecchia resta dov'è, e il valore si restituisce lo
//!   stesso: alla lettura dopo si riprova.
//! - **Scrivere**: nel servizio nuovo; poi la voce di prima, se c'è, si toglie
//!   (se non si riesce poco male: la nuova vince comunque).
//! - **Cancellare**: da **tutti e due**. Se si togliesse solo la nuova, un
//!   account appena scollegato risorgerebbe dalla vecchia alla lettura dopo.
//!
//! # Un turno alla volta
//!
//! Il travaso sono tre passi — leggi la voce di prima, scrivila nella nuova,
//! togli quella di prima — e [`DiSistema`] lo chiamano più fili insieme: il
//! backup, lo scrobbling, i comandi della finestra. Se i passi di due chiamate
//! si intrecciano, ogni intreccio ha un esito sbagliato:
//!
//! - **un aggiornamento perso**: si ricollega un altro account Google fra la
//!   lettura e la scrittura del travaso, e la scrittura del travaso rimette il
//!   token vecchio sopra quello nuovo;
//! - **un account che risorge**: uno «scollega» arriva fra la lettura e la
//!   scrittura, toglie tutte e due le voci, e il travaso le riscrive subito
//!   dopo;
//! - **un falso «non collegato»**: due letture insieme, la seconda trova la
//!   voce nuova ancora vuota e, quando va a guardare quella di prima, la prima
//!   l'ha già tolta.
//!
//! Perciò ogni operazione di [`DiSistema`] tiene un blocco unico di processo
//! dal primo all'ultimo passo, e le tre cose diventano impossibili. Non c'è
//! rientranza da temere: dentro il blocco si parla soltanto con i singoli
//! servizi, mai con [`DiSistema`] stesso, quindi nessuna operazione ne aspetta
//! un'altra mentre ne sta già facendo parte. Il blocco vale per il processo:
//! fra due processi non c'è niente, ma due Rymembra insieme sulla stessa
//! cartella dati non ci sono, per via dell'istanza unica.
//!
//! # Quando la versione di prima è ancora aperta
//!
//! Le voci di prima non sono solo un'eredità: finché quella versione gira
//! sono le **sue**, e portargliele via vuol dire che perde i suoi account
//! mentre l'utente la sta usando. Chi scopre all'avvio che è aperta, e per
//! questo rinuncia a prendere i suoi dati, chiama [`sospendi_il_travaso`]: da
//! quel momento e fino alla fine del processo [`DiSistema`] legge, scrive e
//! cancella **solo** nel servizio di adesso, e il servizio di prima non lo
//! tocca in nessun modo. Un utente che aveva i segreti soltanto lì li vede
//! mancare, in questo processo, e va bene così: è un processo che sta per
//! uscire, e al prossimo avvio il travaso riparte da sé.

use rymembra_domain::errors::{AppError, ErrorCode};

/// Il servizio sotto cui stanno le voci di Rymembra.
///
/// Lo stesso identificativo del bundle dell'applicazione: nel Credential
/// Manager di Windows le voci compaiono con questo nome, e un utente che va a
/// guardare cosa gli è finito nel portachiavi deve poter riconoscere chi ce
/// l'ha messo.
///
/// Uno solo per tutta l'applicazione: le voci si distinguono per **chiave**
/// (`google.refresh_token`, `spotify.refresh_token`), non per servizio, o
/// nell'elenco del Credential Manager comparirebbero tre «Rymembra» diversi senza
/// che nessuno sappia dire quale sia quale.
pub const SERVIZIO: &str = "io.github.federicobaratti.rymembra";

/// Un posto dove tenere i segreti.
///
/// Tutte e tre le operazioni sono idempotenti: leggere una voce che non c'è dà
/// `Ok(None)`, cancellarne una che non c'è dà `Ok(())`. È quel che serve a chi
/// scollega un account senza sapere se era collegato.
pub trait Portachiavi: Send + Sync {
    /// Legge un segreto. `Ok(None)` se non c'è.
    ///
    /// # Errori
    ///
    /// `settings.secretUnavailable` se il portachiavi non risponde. Una voce
    /// assente **non** è un errore: è la condizione di chi non ha mai collegato
    /// niente.
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError>;

    /// Scrive un segreto, sostituendo quello che c'era.
    ///
    /// # Errori
    ///
    /// `settings.secretUnavailable` se il portachiavi non risponde.
    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError>;

    /// Cancella un segreto. Cancellarne uno che non c'è non è un errore.
    ///
    /// # Errori
    ///
    /// `settings.secretUnavailable` se il portachiavi non risponde.
    fn cancella(&self, chiave: &str) -> Result<(), AppError>;
}

/// Il guasto del portachiavi, nominando la voce.
fn non_raggiungibile(chiave: &str, causa: &dyn std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::SettingsSecretUnavailable {
        key: chiave.to_owned(),
    })
    .with_cause(causa.to_string())
}

/// Un servizio del portachiavi di sistema: tutte le voci che stanno sotto un
/// nome solo, distinte per chiave.
///
/// È il mattone di [`DiSistema`], che ne usa due (quello di adesso e quello di
/// prima) e li compone con [`ConRipiego`]. Da solo non ripiega su niente.
#[cfg(feature = "portachiavi")]
#[derive(Debug, Clone, Copy)]
struct Servizio(&'static str);

#[cfg(feature = "portachiavi")]
impl Portachiavi for Servizio {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        let voce =
            keyring::Entry::new(self.0, chiave).map_err(|err| non_raggiungibile(chiave, &err))?;
        match voce.get_password() {
            Ok(valore) => Ok(Some(valore)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(non_raggiungibile(chiave, &err)),
        }
    }

    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
        let voce =
            keyring::Entry::new(self.0, chiave).map_err(|err| non_raggiungibile(chiave, &err))?;
        voce.set_password(valore)
            .map_err(|err| non_raggiungibile(chiave, &err))
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        let voce =
            keyring::Entry::new(self.0, chiave).map_err(|err| non_raggiungibile(chiave, &err))?;
        match voce.delete_credential() {
            // Cancellare quel che non c'è è già la situazione voluta. Un errore
            // qui farebbe fallire uno «scollega» su un account mai collegato,
            // cioè proprio la strada che qualcuno prende quando qualcosa è già
            // andato storto.
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(non_raggiungibile(chiave, &err)),
        }
    }
}

/// Un portachiavi che guarda prima nel posto di adesso e poi in quello di prima.
///
/// Sta sul tratto e non sul Credential Manager perché la regola «un segreto non
/// si perde mai» si deve poter provare, e con il portachiavi vero di mezzo
/// `cargo test` toccherebbe le credenziali di chi lo lancia. Le prove lo
/// montano su due [`InMemoria`]; [`DiSistema`] su due [`Servizio`].
///
/// Non sa nulla di turni né di sospensioni: chi lo monta decide se il posto di
/// prima c'è (vedi [`Accesso`]), e a lui spetta solo la regola.
#[cfg(any(feature = "portachiavi", test))]
struct ConRipiego<'a> {
    /// Dove stanno le voci di adesso, e dove si scrive.
    nuovo: &'a dyn Portachiavi,
    /// Dove stavano quelle di prima: si legge e si svuota, non si riempie.
    /// `None` quando il travaso è sospeso: allora non si fa nessuna delle tre
    /// cose, e questo posto è come se non esistesse.
    vecchio: Option<&'a dyn Portachiavi>,
}

#[cfg(any(feature = "portachiavi", test))]
impl Portachiavi for ConRipiego<'_> {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        if let Some(valore) = self.nuovo.leggi(chiave)? {
            return Ok(Some(valore));
        }
        let Some(vecchio) = self.vecchio else {
            return Ok(None);
        };
        // Un errore sulla voce di prima si propaga, non si scambia per «non
        // c'è»: non sapere se il segreto esiste e dire che non esiste porta
        // l'utente a ricollegare un account che era ancora lì.
        let Some(valore) = vecchio.leggi(chiave)? else {
            return Ok(None);
        };
        // Si porta nel servizio di adesso, e la vecchia si toglie solo con la
        // copia buona già al sicuro: se la scrittura non riesce il segreto
        // resta dov'era e si riprova alla lettura dopo. Lo stesso ordine di
        // `travasa_chiave_lastfm`, nel verso in cui qui serve.
        if self.nuovo.scrivi(chiave, &valore).is_ok() {
            // Best effort: una vecchia che non se ne va non fa male a nessuno,
            // la nuova vince e un `cancella` toglie tutte e due.
            let _ = vecchio.cancella(chiave);
        }
        Ok(Some(valore))
    }

    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
        // Se la scrittura nuova fallisce l'errore esce qui, e la vecchia non si
        // tocca: meglio un segreto in due posti che in nessuno.
        self.nuovo.scrivi(chiave, valore)?;
        // Quella di prima ormai è superata. Se non se ne va la nuova vince lo
        // stesso a ogni lettura.
        if let Some(vecchio) = self.vecchio {
            let _ = vecchio.cancella(chiave);
        }
        Ok(())
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        // Tutte e due, sempre, anche se la prima fallisce: fermarsi al primo
        // errore lascerebbe l'altra voce a far risorgere l'account alla lettura
        // dopo. L'errore che si riporta è il primo.
        let dal_nuovo = self.nuovo.cancella(chiave);
        let Some(vecchio) = self.vecchio else {
            return dal_nuovo;
        };
        dal_nuovo.and(vecchio.cancella(chiave))
    }
}

/// Quel che le chiamate a [`DiSistema`] si dividono: il turno, e l'interruttore
/// del travaso.
///
/// Sta in una struttura, e non in due `static` sciolti, perché le due regole che
/// porta — «una operazione alla volta» e «da sospeso il posto di prima non
/// esiste» — devono potersi provare con un'istanza di prova, senza toccare
/// quella del processo né il Credential Manager. [`DiSistema`] ne usa una sola,
/// [`ACCESSO`].
#[cfg(any(feature = "portachiavi", test))]
struct Accesso {
    /// Lo tiene chi sta facendo una operazione sul portachiavi, dal primo
    /// all'ultimo passo. Non protegge dati: protegge la **sequenza**, che è
    /// tutto quello che serve perché il travaso non si intrecci con niente.
    turno: std::sync::Mutex<()>,
    /// Il travaso è sospeso per il resto del processo? Si alza una volta e non
    /// si abbassa più.
    travaso_sospeso: std::sync::atomic::AtomicBool,
}

#[cfg(any(feature = "portachiavi", test))]
impl Accesso {
    /// Un accesso con il travaso acceso e nessuno dentro.
    const fn nuovo() -> Self {
        Self {
            turno: std::sync::Mutex::new(()),
            travaso_sospeso: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Fa una operazione sui due servizi, da sola e con il travaso com'è.
    ///
    /// Il turno si prende **prima** di guardare l'interruttore, e si tiene fino
    /// a operazione finita: quel che l'operazione vede dell'interruttore è
    /// quello che c'era quando è entrata, e [`Accesso::sospendi`], che il turno
    /// lo aspetta, non lo cambia a metà.
    ///
    /// Un turno avvelenato — un filo caduto con il turno in mano — si
    /// recupera: non protegge dati, e un portachiavi che non risponde più per
    /// sempre perché un filo è caduto sarebbe peggio del guaio che ha fatto
    /// cadere il filo.
    fn esegui<T>(
        &self,
        nuovo: &dyn Portachiavi,
        vecchio: &dyn Portachiavi,
        operazione: impl FnOnce(&ConRipiego<'_>) -> T,
    ) -> T {
        let _turno = self
            .turno
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sospeso = self
            .travaso_sospeso
            .load(std::sync::atomic::Ordering::SeqCst);
        operazione(&ConRipiego {
            nuovo,
            vecchio: (!sospeso).then_some(vecchio),
        })
    }

    /// Sospende il travaso per il resto del processo.
    ///
    /// Prende il turno anche lei: quando ritorna nessuna operazione è in corso
    /// sul posto di prima, e nessuna ci tornerà.
    fn sospendi(&self) {
        let _turno = self
            .turno
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.travaso_sospeso
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Il servizio di adesso, per [`DiSistema`].
#[cfg(feature = "portachiavi")]
static SERVIZIO_NUOVO: Servizio = Servizio(SERVIZIO);

/// Il servizio di prima, per [`DiSistema`]: serve solo a ritrovare le voci
/// vecchie.
#[cfg(feature = "portachiavi")]
static SERVIZIO_PRECEDENTE: Servizio = Servizio(rymembra_domain::eredita::IDENTIFICATORE);

/// Il turno e l'interruttore di tutto il processo, per [`DiSistema`].
#[cfg(feature = "portachiavi")]
static ACCESSO: Accesso = Accesso::nuovo();

/// Smette di toccare le voci della versione di prima, per il resto del processo.
///
/// La chiama chi ha scoperto all'avvio che la versione di prima è ancora aperta
/// e, per questo, ha rinunciato a prenderne i dati: quelle voci del portachiavi
/// sono ancora le sue, e un [`DiSistema`] che le travasasse la lascerebbe senza
/// gli account mentre è in uso. Da qui in poi [`DiSistema`] legge, scrive e
/// cancella soltanto sotto [`SERVIZIO`].
///
/// Non si torna indietro: l'interruttore non si riabbassa. Se la versione di
/// prima viene chiusa mentre Rymembra gira, il travaso riparte al prossimo
/// avvio, che rifà tutti i controlli.
///
/// Quando ritorna, nessuna operazione è più in corso sul servizio di prima. Si
/// può chiamare da qualunque filo, ma una chiamata che arrivi a operazioni già
/// iniziate le aspetta: conviene farla prima che parta chiunque usi il
/// portachiavi.
#[cfg(feature = "portachiavi")]
pub fn sospendi_il_travaso() {
    ACCESSO.sospendi();
}

/// Il portachiavi del sistema operativo.
///
/// Su Windows è il Credential Manager, raggiunto da `keyring` attraverso
/// `windows-sys`: nessun demone da avviare, nessuna catena di compilazione C.
///
/// Scrive sempre sotto [`SERVIZIO`]. In lettura ripiega sul servizio della
/// versione di prima e ne porta le voci in quello nuovo: vedi «Le voci della
/// versione di prima», in testa al modulo. Ogni operazione ha il portachiavi
/// per sé dal primo all'ultimo passo («Un turno alla volta»), e dopo
/// [`sospendi_il_travaso`] non guarda più nel servizio di prima.
#[cfg(feature = "portachiavi")]
#[derive(Debug, Clone, Copy, Default)]
pub struct DiSistema;

#[cfg(feature = "portachiavi")]
impl DiSistema {
    /// Una operazione sui due servizi, sotto il turno di tutto il processo.
    fn con_ripiego<T>(operazione: impl FnOnce(&ConRipiego<'_>) -> T) -> T {
        ACCESSO.esegui(&SERVIZIO_NUOVO, &SERVIZIO_PRECEDENTE, operazione)
    }
}

#[cfg(feature = "portachiavi")]
impl Portachiavi for DiSistema {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        Self::con_ripiego(|servizi| servizi.leggi(chiave))
    }

    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
        Self::con_ripiego(|servizi| servizi.scrivi(chiave, valore))
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        Self::con_ripiego(|servizi| servizi.cancella(chiave))
    }
}

/// Un portachiavi in memoria, per le prove.
///
/// Non è un ripiego per la produzione — non c'entra niente con la nota in testa
/// al modulo: non tocca il disco, e sparisce con il processo. Esiste perché chi
/// fa OAuth si possa provare senza che `cargo test` chieda una password.
#[derive(Debug, Default)]
pub struct InMemoria {
    voci: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
}

impl InMemoria {
    /// Un portachiavi vuoto.
    #[must_use]
    pub fn nuovo() -> Self {
        Self::default()
    }
}

impl Portachiavi for InMemoria {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        let voci = self
            .voci
            .lock()
            .map_err(|err| non_raggiungibile(chiave, &err))?;
        Ok(voci.get(chiave).cloned())
    }

    fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
        let mut voci = self
            .voci
            .lock()
            .map_err(|err| non_raggiungibile(chiave, &err))?;
        voci.insert(chiave.to_owned(), valore.to_owned());
        Ok(())
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        let mut voci = self
            .voci
            .lock()
            .map_err(|err| non_raggiungibile(chiave, &err))?;
        voci.remove(chiave);
        Ok(())
    }
}

/// Un portachiavi che non funziona, per provare cosa succede quando non c'è.
///
/// La strada che conta davvero: se il portachiavi è irraggiungibile il
/// collegamento deve **restare spento** e dirlo, non ripiegare da qualche altra
/// parte.
#[derive(Debug, Clone, Copy, Default)]
pub struct Guasto;

impl Portachiavi for Guasto {
    fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
        Err(non_raggiungibile(chiave, &"portachiavi non disponibile"))
    }

    fn scrivi(&self, chiave: &str, _valore: &str) -> Result<(), AppError> {
        Err(non_raggiungibile(chiave, &"portachiavi non disponibile"))
    }

    fn cancella(&self, chiave: &str) -> Result<(), AppError> {
        Err(non_raggiungibile(chiave, &"portachiavi non disponibile"))
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn una_voce_mai_scritta_non_e_un_errore() {
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.leggi("mai.scritta"), Ok(None));
    }

    #[test]
    fn scrivere_due_volte_sostituisce() {
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.scrivi("t", "prima"), Ok(()));
        assert_eq!(portachiavi.scrivi("t", "dopo"), Ok(()));
        assert_eq!(portachiavi.leggi("t"), Ok(Some("dopo".to_owned())));
    }

    #[test]
    fn cancellare_quel_che_non_c_e_va_bene_lo_stesso() {
        // È la strada di chi preme «scollega» su un account mai collegato,
        // oppure di chi lo preme due volte perché la prima non sembrava aver
        // fatto niente.
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.cancella("mai.scritta"), Ok(()));
        assert_eq!(portachiavi.scrivi("t", "x"), Ok(()));
        assert_eq!(portachiavi.cancella("t"), Ok(()));
        assert_eq!(portachiavi.cancella("t"), Ok(()));
        assert_eq!(portachiavi.leggi("t"), Ok(None));
    }

    #[test]
    fn un_portachiavi_guasto_lo_dice_col_nome_della_voce() {
        // Il codice porta la chiave perché il messaggio all'utente possa dire
        // *cosa* non si è potuto salvare, invece di «errore del portachiavi».
        let err = Guasto.leggi("un.segreto").unwrap_err();
        assert_eq!(err.code().kind().code(), "settings.secretUnavailable");
        assert_eq!(
            err.code(),
            &ErrorCode::SettingsSecretUnavailable {
                key: "un.segreto".to_owned()
            }
        );
    }

    #[test]
    fn due_voci_non_si_pestano_i_piedi() {
        // Un solo `SERVIZIO` per tutta l'applicazione: quel che distingue il
        // token di Google da quello di Spotify è la chiave, e deve bastare.
        let portachiavi = InMemoria::nuovo();
        assert_eq!(portachiavi.scrivi("google.refresh_token", "g"), Ok(()));
        assert_eq!(portachiavi.scrivi("spotify.refresh_token", "s"), Ok(()));
        assert_eq!(
            portachiavi.leggi("google.refresh_token"),
            Ok(Some("g".to_owned()))
        );
        assert_eq!(portachiavi.cancella("google.refresh_token"), Ok(()));
        assert_eq!(
            portachiavi.leggi("spotify.refresh_token"),
            Ok(Some("s".to_owned())),
            "scollegare un account non ne scollega un altro"
        );
    }

    // ── il ripiego sulle voci di prima ──────────────────────────────────────
    //
    // Mai sul portachiavi vero: `ConRipiego` sta sul tratto, e qui ci stanno
    // due `InMemoria` — il servizio di adesso e quello di prima.

    /// Un portachiavi in memoria a cui si può vietare una operazione, per
    /// provare cosa succede quando il Credential Manager rifiuta proprio quella.
    #[derive(Default)]
    struct ConDivieti {
        dentro: InMemoria,
        niente_scrittura: bool,
        niente_cancellazione: bool,
    }

    impl Portachiavi for ConDivieti {
        fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
            self.dentro.leggi(chiave)
        }

        fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
            if self.niente_scrittura {
                return Err(non_raggiungibile(chiave, &"scrittura rifiutata"));
            }
            self.dentro.scrivi(chiave, valore)
        }

        fn cancella(&self, chiave: &str) -> Result<(), AppError> {
            if self.niente_cancellazione {
                return Err(non_raggiungibile(chiave, &"cancellazione rifiutata"));
            }
            self.dentro.cancella(chiave)
        }
    }

    /// Una voce con un valore, per non ripetere il giro ogni volta.
    fn con(chiave: &str, valore: &str) -> InMemoria {
        let portachiavi = InMemoria::nuovo();
        portachiavi.scrivi(chiave, valore).unwrap();
        portachiavi
    }

    const GOOGLE: &str = "google.refresh_token";

    #[test]
    fn la_voce_di_prima_si_travasa_e_si_toglie() {
        let nuovo = InMemoria::nuovo();
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
        assert_eq!(
            nuovo.leggi(GOOGLE),
            Ok(Some("di-prima".to_owned())),
            "la copia è nel servizio di adesso"
        );
        assert_eq!(
            vecchio.leggi(GOOGLE),
            Ok(None),
            "e quella di prima non c'è più"
        );
        // Alla lettura dopo non c'è più niente da travasare, e il segreto c'è.
        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
    }

    #[test]
    fn la_voce_nuova_vince() {
        let nuovo = con(GOOGLE, "di-adesso");
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-adesso".to_owned())));
        assert_eq!(
            nuovo.leggi(GOOGLE),
            Ok(Some("di-adesso".to_owned())),
            "quella di prima non ne prende il posto"
        );
    }

    #[test]
    fn cancellare_toglie_entrambe() {
        // Se `cancella` togliesse solo la voce nuova, un account appena
        // scollegato risorgerebbe dalla vecchia alla lettura dopo.
        let nuovo = con(GOOGLE, "di-adesso");
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        assert_eq!(insieme.cancella(GOOGLE), Ok(()));
        assert_eq!(nuovo.leggi(GOOGLE), Ok(None));
        assert_eq!(vecchio.leggi(GOOGLE), Ok(None));
        assert_eq!(insieme.leggi(GOOGLE), Ok(None));

        // E vale anche per chi aveva solo la voce di prima, mai letta.
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &InMemoria::nuovo(),
            vecchio: Some(&vecchio),
        };
        assert_eq!(insieme.cancella(GOOGLE), Ok(()));
        assert_eq!(insieme.leggi(GOOGLE), Ok(None));
    }

    #[test]
    fn se_la_scrittura_nuova_fallisce_la_vecchia_resta() {
        let nuovo = ConDivieti {
            niente_scrittura: true,
            ..ConDivieti::default()
        };
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        // Il valore si restituisce lo stesso: l'utente resta collegato.
        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
        // Ma il segreto non si perde: l'unica copia buona è ancora lì.
        assert_eq!(vecchio.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
        assert_eq!(nuovo.leggi(GOOGLE), Ok(None));
        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
    }

    #[test]
    fn chiave_dinamica_ia() {
        // Le voci dell'IA nascono a runtime, una per profilo: il ripiego non ne
        // conosce i nomi, e funziona perché tutto passa per la chiave.
        let chiave = "ia.mio-profilo.chiave";
        let nuovo = InMemoria::nuovo();
        let vecchio = con(chiave, "sk-di-prima");
        let altra = "ia.altro-profilo.chiave";
        vecchio.scrivi(altra, "sk-altro").unwrap();
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        assert_eq!(insieme.leggi(chiave), Ok(Some("sk-di-prima".to_owned())));
        assert_eq!(nuovo.leggi(chiave), Ok(Some("sk-di-prima".to_owned())));
        assert_eq!(vecchio.leggi(chiave), Ok(None));
        assert_eq!(
            vecchio.leggi(altra),
            Ok(Some("sk-altro".to_owned())),
            "il travaso di una chiave non porta via le altre"
        );
        assert_eq!(nuovo.leggi(altra), Ok(None));
    }

    #[test]
    fn una_voce_che_non_c_e_da_nessuna_parte_non_e_un_errore() {
        let nuovo = InMemoria::nuovo();
        let vecchio = InMemoria::nuovo();
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };
        assert_eq!(insieme.leggi(GOOGLE), Ok(None));
        assert_eq!(insieme.cancella(GOOGLE), Ok(()));
    }

    #[test]
    fn scrivere_scrive_il_nuovo_e_toglie_il_vecchio() {
        let nuovo = InMemoria::nuovo();
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        assert_eq!(insieme.scrivi(GOOGLE, "di-adesso"), Ok(()));
        assert_eq!(nuovo.leggi(GOOGLE), Ok(Some("di-adesso".to_owned())));
        assert_eq!(vecchio.leggi(GOOGLE), Ok(None));
        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-adesso".to_owned())));
    }

    #[test]
    fn scrivere_con_il_nuovo_che_rifiuta_non_tocca_il_vecchio() {
        let nuovo = ConDivieti {
            niente_scrittura: true,
            ..ConDivieti::default()
        };
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        let err = insieme.scrivi(GOOGLE, "di-adesso").unwrap_err();
        assert_eq!(err.code().kind().code(), "settings.secretUnavailable");
        assert_eq!(vecchio.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
    }

    #[test]
    fn se_la_vecchia_non_se_ne_va_la_nuova_vince_lo_stesso() {
        // Il portachiavi rifiuta di togliere la voce di prima: né il travaso né
        // la scrittura devono per questo fallire, né far tornare indietro il
        // valore.
        let nuovo = InMemoria::nuovo();
        let vecchio = ConDivieti {
            niente_cancellazione: true,
            ..ConDivieti::default()
        };
        vecchio.dentro.scrivi(GOOGLE, "di-prima").unwrap();
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
        assert_eq!(insieme.scrivi(GOOGLE, "di-adesso"), Ok(()));
        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-adesso".to_owned())));
    }

    #[test]
    fn cancellare_prova_anche_la_seconda_se_la_prima_rifiuta() {
        // L'errore esce, perché l'utente deve saperlo, ma la voce di prima si
        // toglie comunque: non c'è motivo di lasciarla a far risorgere l'account.
        let nuovo = ConDivieti {
            niente_cancellazione: true,
            ..ConDivieti::default()
        };
        nuovo.dentro.scrivi(GOOGLE, "di-adesso").unwrap();
        let vecchio = con(GOOGLE, "di-prima");
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&vecchio),
        };

        let err = insieme.cancella(GOOGLE).unwrap_err();
        assert_eq!(err.code().kind().code(), "settings.secretUnavailable");
        assert_eq!(vecchio.leggi(GOOGLE), Ok(None));
    }

    #[test]
    fn una_vecchia_che_non_risponde_non_si_scambia_per_assente() {
        let nuovo = InMemoria::nuovo();
        let insieme = ConRipiego {
            nuovo: &nuovo,
            vecchio: Some(&Guasto),
        };

        // Non sapere se il segreto c'è non è «non c'è»: lo si dice.
        let err = insieme.leggi(GOOGLE).unwrap_err();
        assert_eq!(err.code().kind().code(), "settings.secretUnavailable");

        // Ma con la voce nuova a posto non si va nemmeno a guardare.
        nuovo.scrivi(GOOGLE, "di-adesso").unwrap();
        assert_eq!(insieme.leggi(GOOGLE), Ok(Some("di-adesso".to_owned())));
    }

    #[test]
    fn il_servizio_di_prima_non_e_quello_di_adesso() {
        // Se coincidessero, «scrivi il nuovo e togli il vecchio» toglierebbe la
        // voce appena scritta, e ogni segreto durerebbe il tempo di una
        // scrittura.
        assert_ne!(SERVIZIO, rymembra_domain::eredita::IDENTIFICATORE);
    }

    // ── la versione di prima è aperta: il travaso è sospeso ────────────────
    //
    // Né qui né sotto si tocca l'`ACCESSO` del processo: ogni prova ha il suo
    // `Accesso`, montato sulle solite `InMemoria`.

    /// Una voce con un'altra chiave, per vedere che sospendere non fa
    /// differenze fra una chiave e l'altra.
    const SPOTIFY: &str = "spotify.refresh_token";

    /// Un portachiavi in memoria che conta ogni accesso, di qualunque tipo: per
    /// provare che qualcuno **non** ci ha messo le mani, che con un contatore a
    /// zero si dice meglio che con un'asserzione sul contenuto.
    #[derive(Default)]
    struct Sentinella {
        dentro: InMemoria,
        accessi: std::sync::atomic::AtomicUsize,
    }

    impl Sentinella {
        fn accessi(&self) -> usize {
            self.accessi.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn conta(&self) {
            self.accessi
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl Portachiavi for Sentinella {
        fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
            self.conta();
            self.dentro.leggi(chiave)
        }

        fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
            self.conta();
            self.dentro.scrivi(chiave, valore)
        }

        fn cancella(&self, chiave: &str) -> Result<(), AppError> {
            self.conta();
            self.dentro.cancella(chiave)
        }
    }

    #[test]
    fn da_sospeso_il_servizio_di_prima_non_si_tocca_mai() {
        let accesso = Accesso::nuovo();
        accesso.sospendi();
        let nuovo = InMemoria::nuovo();
        let vecchio = Sentinella::default();
        vecchio.dentro.scrivi(GOOGLE, "di-prima").unwrap();

        // Leggere: la voce nuova non c'è, e di là non si va a guardare. Per chi
        // aveva i segreti solo lì vuol dire «non collegato», in questo processo.
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)),
            Ok(None)
        );
        // Scrivere: solo la nuova, e la vecchia non si toglie.
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.scrivi(GOOGLE, "di-adesso")),
            Ok(())
        );
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)),
            Ok(Some("di-adesso".to_owned()))
        );
        // Cancellare: solo la nuova.
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.cancella(GOOGLE)),
            Ok(())
        );
        assert_eq!(nuovo.leggi(GOOGLE), Ok(None));

        assert_eq!(vecchio.accessi(), 0, "nemmeno uno sguardo");
        assert_eq!(
            vecchio.dentro.leggi(GOOGLE),
            Ok(Some("di-prima".to_owned())),
            "e la voce di prima è dov'era, com'era"
        );
    }

    #[test]
    fn il_travaso_va_finche_non_si_sospende() {
        let accesso = Accesso::nuovo();
        let nuovo = InMemoria::nuovo();
        let vecchio = Sentinella::default();
        vecchio.dentro.scrivi(GOOGLE, "google-di-prima").unwrap();
        vecchio.dentro.scrivi(SPOTIFY, "spotify-di-prima").unwrap();

        // Prima: la regola di sempre. Anche la sentinella lo vede.
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)),
            Ok(Some("google-di-prima".to_owned()))
        );
        assert!(vecchio.accessi() > 0);
        assert_eq!(vecchio.dentro.leggi(GOOGLE), Ok(None), "travasata");

        // Dopo: le voci che non erano ancora passate restano dove sono.
        accesso.sospendi();
        let accessi = vecchio.accessi();
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.leggi(SPOTIFY)),
            Ok(None)
        );
        assert_eq!(vecchio.accessi(), accessi);
        assert_eq!(
            vecchio.dentro.leggi(SPOTIFY),
            Ok(Some("spotify-di-prima".to_owned()))
        );
        // E quella già passata resta nel servizio di adesso.
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)),
            Ok(Some("google-di-prima".to_owned()))
        );
    }

    #[test]
    fn da_sospeso_un_errore_del_servizio_di_prima_non_conta() {
        // Non lo si interroga, quindi non può nemmeno rispondere male: la
        // lettura che sul posto di prima trova un portachiavi guasto e
        // propaga l'errore, da sospesa non lo incontra.
        let accesso = Accesso::nuovo();
        accesso.sospendi();
        let nuovo = InMemoria::nuovo();
        assert_eq!(
            accesso.esegui(&nuovo, &Guasto, |s| s.leggi(GOOGLE)),
            Ok(None)
        );
        assert_eq!(
            accesso.esegui(&nuovo, &Guasto, |s| s.scrivi(GOOGLE, "x")),
            Ok(())
        );
        assert_eq!(
            accesso.esegui(&nuovo, &Guasto, |s| s.cancella(GOOGLE)),
            Ok(())
        );
    }

    // ── un turno alla volta ────────────────────────────────────────────────
    //
    // Le tre corse che il turno rende impossibili, ognuna ferma a metà con un
    // portachiavi che aspetta il via dopo la sua prima lettura. Con il turno
    // l'altro filo aspetta, e l'esito è quello in fila; senza, passa in mezzo
    // e l'esito è sbagliato.

    /// Quanto si lascia al filo che dovrebbe passare in mezzo per provarci:
    /// con il turno non cambia niente, senza è più che sufficiente.
    const PAUSA: std::time::Duration = std::time::Duration::from_millis(150);

    /// Quanto si aspetta al massimo un via o un arrivo: una prova che fallisce
    /// deve fallire, non restare appesa.
    const LIMITE: std::time::Duration = std::time::Duration::from_secs(10);

    /// Un portachiavi in memoria che, alla **prima** lettura, avvisa di essere
    /// arrivato e aspetta il via *dopo* aver letto: tiene un filo a metà di
    /// un'operazione. Dalla seconda lettura in poi non si ferma più.
    struct Fermo {
        dentro: InMemoria,
        sosta:
            std::sync::Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    }

    impl Fermo {
        /// Il portachiavi, il canale su cui avvisa e quello su cui aspetta.
        fn alla_prima_lettura() -> (
            Self,
            std::sync::mpsc::Receiver<()>,
            std::sync::mpsc::Sender<()>,
        ) {
            let (arrivato, aspetta_l_arrivo) = std::sync::mpsc::channel();
            let (via, aspetta_il_via) = std::sync::mpsc::channel();
            let fermo = Self {
                dentro: InMemoria::nuovo(),
                sosta: std::sync::Mutex::new(Some((arrivato, aspetta_il_via))),
            };
            (fermo, aspetta_l_arrivo, via)
        }
    }

    impl Portachiavi for Fermo {
        fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
            let letto = self.dentro.leggi(chiave);
            let sosta = self
                .sosta
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some((arrivato, via)) = sosta {
                let _ = arrivato.send(());
                let _ = via.recv_timeout(LIMITE);
            }
            letto
        }

        fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
            self.dentro.scrivi(chiave, valore)
        }

        fn cancella(&self, chiave: &str) -> Result<(), AppError> {
            self.dentro.cancella(chiave)
        }
    }

    #[test]
    fn un_aggiornamento_non_viene_sovrascritto_dal_travaso() {
        let accesso = Accesso::nuovo();
        let nuovo = InMemoria::nuovo();
        let (vecchio, arrivato, via) = Fermo::alla_prima_lettura();
        vecchio.dentro.scrivi(GOOGLE, "di-prima").unwrap();

        std::thread::scope(|filo| {
            // Il travaso: ha letto la voce di prima, e si ferma lì.
            let lettura = filo.spawn(|| accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)));
            arrivato.recv_timeout(LIMITE).unwrap();
            // Un ricollegamento arriva a metà del travaso.
            let scrittura =
                filo.spawn(|| accesso.esegui(&nuovo, &vecchio, |s| s.scrivi(GOOGLE, "di-adesso")));
            std::thread::sleep(PAUSA);
            via.send(()).unwrap();

            assert_eq!(lettura.join().unwrap(), Ok(Some("di-prima".to_owned())));
            assert_eq!(scrittura.join().unwrap(), Ok(()));
        });

        assert_eq!(
            nuovo.leggi(GOOGLE),
            Ok(Some("di-adesso".to_owned())),
            "il valore appena scritto non lo rimette a posto quello di prima"
        );
        assert_eq!(vecchio.dentro.leggi(GOOGLE), Ok(None));
    }

    #[test]
    fn uno_scollega_a_meta_del_travaso_non_fa_risorgere_l_account() {
        let accesso = Accesso::nuovo();
        let nuovo = InMemoria::nuovo();
        let (vecchio, arrivato, via) = Fermo::alla_prima_lettura();
        vecchio.dentro.scrivi(GOOGLE, "di-prima").unwrap();

        std::thread::scope(|filo| {
            let lettura = filo.spawn(|| accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)));
            arrivato.recv_timeout(LIMITE).unwrap();
            let scollega = filo.spawn(|| accesso.esegui(&nuovo, &vecchio, |s| s.cancella(GOOGLE)));
            std::thread::sleep(PAUSA);
            via.send(()).unwrap();

            // Il travaso finisce per primo, e lo scollega viene dopo.
            assert_eq!(lettura.join().unwrap(), Ok(Some("di-prima".to_owned())));
            assert_eq!(scollega.join().unwrap(), Ok(()));
        });

        assert_eq!(
            nuovo.leggi(GOOGLE),
            Ok(None),
            "l'account è rimasto scollegato"
        );
        assert_eq!(vecchio.dentro.leggi(GOOGLE), Ok(None));
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)),
            Ok(None)
        );
    }

    #[test]
    fn due_letture_insieme_non_dicono_non_collegato_a_chi_lo_e() {
        let accesso = Accesso::nuovo();
        // Qui si ferma la voce **nuova**: la prima lettura la trova vuota e
        // aspetta, prima ancora di guardare quella di prima.
        let (nuovo, arrivato, via) = Fermo::alla_prima_lettura();
        let vecchio = con(GOOGLE, "di-prima");

        std::thread::scope(|filo| {
            let prima = filo.spawn(|| accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)));
            arrivato.recv_timeout(LIMITE).unwrap();
            let seconda = filo.spawn(|| accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)));
            std::thread::sleep(PAUSA);
            via.send(()).unwrap();

            // Senza il turno la seconda travasa tutto mentre la prima aspetta, e
            // la prima, ripartendo, trova la voce di prima già tolta.
            let valore = Ok(Some("di-prima".to_owned()));
            assert_eq!(prima.join().unwrap(), valore);
            assert_eq!(seconda.join().unwrap(), valore);
        });

        assert_eq!(nuovo.dentro.leggi(GOOGLE), Ok(Some("di-prima".to_owned())));
        assert_eq!(vecchio.leggi(GOOGLE), Ok(None));
    }

    #[test]
    fn il_turno_si_tiene_per_tutta_l_operazione() {
        let accesso = Accesso::nuovo();
        let nuovo = InMemoria::nuovo();
        let vecchio = InMemoria::nuovo();

        let preso = accesso.esegui(&nuovo, &vecchio, |servizi| {
            // Dentro, il turno è preso: chiunque altro dovrebbe aspettare.
            let occupato = accesso.turno.try_lock().is_err();
            // E l'operazione si fa lo stesso, sul turno che ha.
            assert_eq!(servizi.scrivi(GOOGLE, "x"), Ok(()));
            occupato
        });

        assert!(preso, "il turno era libero in mezzo all'operazione");
        assert!(accesso.turno.try_lock().is_ok(), "e dopo si rilascia");
    }

    #[test]
    fn sospendere_aspetta_chi_sta_operando() {
        let accesso = Accesso::nuovo();
        let nuovo = InMemoria::nuovo();
        let (vecchio, arrivato, via) = Fermo::alla_prima_lettura();
        vecchio.dentro.scrivi(GOOGLE, "di-prima").unwrap();
        let sospeso = std::sync::atomic::AtomicBool::new(false);

        std::thread::scope(|filo| {
            // Un travaso in corso, fermo a metà.
            let lettura = filo.spawn(|| accesso.esegui(&nuovo, &vecchio, |s| s.leggi(GOOGLE)));
            arrivato.recv_timeout(LIMITE).unwrap();
            // La sospensione arriva, e non può essere già finita: il servizio
            // di prima è ancora in mano al travaso.
            let sospensione = filo.spawn(|| {
                accesso.sospendi();
                sospeso.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            std::thread::sleep(PAUSA);
            assert!(!sospeso.load(std::sync::atomic::Ordering::SeqCst));
            via.send(()).unwrap();

            assert_eq!(lettura.join().unwrap(), Ok(Some("di-prima".to_owned())));
            sospensione.join().unwrap();
        });

        // Quando la sospensione ritorna, il travaso è finito e non ne parte altri.
        assert!(sospeso.load(std::sync::atomic::Ordering::SeqCst));
        vecchio.dentro.scrivi(SPOTIFY, "altro").unwrap();
        assert_eq!(
            accesso.esegui(&nuovo, &vecchio, |s| s.leggi(SPOTIFY)),
            Ok(None)
        );
        assert_eq!(vecchio.dentro.leggi(SPOTIFY), Ok(Some("altro".to_owned())));
    }
}
