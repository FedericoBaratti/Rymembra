//! Il permesso di un conto: l'access token in memoria, il refresh token nel
//! portachiavi, e un rinfresco solo alla volta.
//!
//! # Perché un rinfresco solo
//!
//! Perché un conto ha più fili che lo usano insieme — la riproduzione, la
//! scansione con otto letture in volo, il filo dei cambiamenti — e un token
//! scade per tutti nello stesso istante. Senza una regola, otto `401` di fila
//! sarebbero otto rinfreschi: otto richieste a Microsoft, e con un refresh
//! token che **ruota** a ogni rinfresco, sette refresh token buttati e una gara
//! su quale finisce nel portachiavi.
//!
//! La regola è un giro: ogni permesso dato porta il numero del rinfresco da cui
//! viene ([`Permesso`]). Chi riceve un `401` lo riporta con [`Accesso::rifiutato`],
//! e se nel frattempo qualcun altro ha già rinfrescato riceve il token nuovo
//! senza chiederne un altro. Il rinfresco avviene tenendo il lucchetto di
//! questo tipo — non quello della libreria, che qui non arriva — quindi chi
//! arriva mentre è in corso lo aspetta invece di rifarlo.
//!
//! # Il refresh token ruotato
//!
//! Si salva nel portachiavi appena arriva, sotto
//! [`crate::portachiavi::nuvola_refresh`]. Se il portachiavi in quel momento
//! non risponde, il token nuovo resta in memoria e si usa lui per il rinfresco
//! dopo, e si riprova a salvarlo: perderlo vorrebbe dire continuare con quello
//! vecchio, che Microsoft prima o poi smette di accettare.
//!
//! # Spento
//!
//! Un `Accesso` vive dentro ogni [`Permesso`] in volo e ogni sorgente aperta:
//! togliere il deposito dalla mappa non lo ferma. Chi scollega un conto, o ne
//! cambia l'app, chiama prima [`Accesso::spegni`] e **poi** cancella il
//! portachiavi. Lo spegnimento prende lo stesso lucchetto del rinfresco, quindi
//! aspetta quello in corso — col suo salvataggio — e da lì in poi nessun
//! rinfresco parte e nessun token viene riscritto: senza, un brano che suona
//! da un conto appena scollegato rimetterebbe nel portachiavi il refresh token
//! che lo scollegamento aveva appena tolto.
//!
//! # Un rifiuto definitivo si ricorda
//!
//! Quando il punto di scambio dice che il refresh token non vale più
//! (`cloud.authExpired`: consenso revocato, un'app di Google in «Testing» che
//! scade dopo sette giorni) o che l'app non è accettata (`cloud.appRejected`),
//! riprovare con lo stesso token e la stessa app darà la stessa risposta. Ma
//! chi chiede un permesso non lo sa: la scansione ne chiede uno per ogni tag
//! da leggere, e senza memoria tremila file in attesa erano tremila richieste
//! rifiutate a Microsoft o a Google in pochi minuti. Il rifiuto si ricorda
//! per [`RICORDO_DEL_RIFIUTO`], e intanto chi chiede riceve quello senza
//! andare in rete.
//!
//! Non per sempre: il filo del conto riprova di rado anche un guasto da
//! ricollegare, e un'app rimessa a posto sul servizio (un client ripristinato
//! nella console) deve tornare da sé, come tornava prima. Il rimedio vero —
//! ricollegare, o cambiare l'app — crea comunque un `Accesso` nuovo, senza
//! ricordi. I guasti passeggeri (la rete, un limite di frequenza, un
//! portachiavi che non risponde) non si ricordano mai.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_oauth::ancora_valido;

use crate::http::Rete;
use crate::oauth::{self, Credenziali, Fornitore, Token, Uso};
use crate::portachiavi::{Portachiavi, nuvola_refresh};

/// Quanto si ricorda un rifiuto definitivo del punto di scambio: vedi il
/// `//!`. Dieci minuti, quanto il filo del conto aspetta fra un tentativo e
/// l'altro dopo un guasto da ricollegare (`PAUSA_MASSIMA` in
/// `apps/desktop/src-tauri/src/nuvola_musica.rs`: se cambia quella, va
/// rivista questa): la memoria non aggiunge attese a chi riprova a quel ritmo,
/// e toglie le richieste di tutti gli altri.
const RICORDO_DEL_RIFIUTO: Duration = Duration::from_secs(10 * 60);

/// Un access token da usare, e il giro di rinfresco da cui viene.
///
/// [`Debug`] scritto a mano: dentro c'è il token.
#[derive(Clone, PartialEq, Eq)]
pub struct Permesso {
    token: String,
    giro: u64,
}

impl Permesso {
    /// Il valore dell'intestazione `Authorization`.
    #[must_use]
    pub fn intestazione(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

impl std::fmt::Debug for Permesso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Permesso")
            .field("token", &"…")
            .field("giro", &self.giro)
            .finish()
    }
}

/// Quel che il lucchetto protegge.
struct Stato {
    /// Il conto, quando si sa: subito dopo il consenso non lo si sa ancora.
    conto: Option<String>,
    /// L'ultimo token ricevuto.
    token: Option<Token>,
    /// Quanti rinfreschi sono riusciti finora.
    giro: u64,
    /// Il refresh token più recente che è passato di qui: arrivato col
    /// consenso, o ruotato da un rinfresco. `None` finché si usa quello del
    /// portachiavi senza che nessuno l'abbia cambiato.
    refresh: Option<String>,
    /// `refresh` non è ancora nel portachiavi.
    da_salvare: bool,
    /// Il conto è stato scollegato o la sua app è cambiata: niente più
    /// permessi, niente più scritture. Vedi il `//!`.
    spento: bool,
    /// L'ultimo rifiuto definitivo del punto di scambio, e fino a quando vale:
    /// chi chiede un permesso intanto riceve questo. Vedi il `//!`.
    respinto: Option<(AppError, Instant)>,
}

/// Il permesso di un conto, condiviso da tutti i fili che lo usano.
pub struct Accesso {
    rete: Rete,
    fornitore: Fornitore,
    credenziali: Credenziali,
    portachiavi: Arc<dyn Portachiavi>,
    stato: Mutex<Stato>,
}

impl std::fmt::Debug for Accesso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accesso")
            .field("fornitore", &self.fornitore.servizio.chiave)
            .finish_non_exhaustive()
    }
}

impl Accesso {
    /// Il permesso di un conto già collegato: il refresh token sta nel
    /// portachiavi, e il primo access token arriva alla prima richiesta.
    #[must_use]
    pub fn del_conto(
        rete: Rete,
        fornitore: Fornitore,
        credenziali: Credenziali,
        conto: &str,
        portachiavi: Arc<dyn Portachiavi>,
    ) -> Self {
        Self::con_stato(
            rete,
            fornitore,
            credenziali,
            portachiavi,
            Stato {
                conto: Some(conto.to_owned()),
                token: None,
                giro: 0,
                refresh: None,
                da_salvare: false,
                spento: false,
                respinto: None,
            },
        )
    }

    /// Il permesso appena dato nel browser: i token in mano, il conto ancora
    /// da sapere. Lo si sa chiedendo al servizio chi è l'account, con questo
    /// stesso permesso; poi [`Self::battezza`].
    #[must_use]
    pub fn appena_collegato(
        rete: Rete,
        fornitore: Fornitore,
        credenziali: Credenziali,
        token: Token,
        portachiavi: Arc<dyn Portachiavi>,
    ) -> Self {
        let refresh = token.refresh_token.clone();
        let da_salvare = refresh.is_some();
        Self::con_stato(
            rete,
            fornitore,
            credenziali,
            portachiavi,
            Stato {
                conto: None,
                token: Some(token),
                giro: 0,
                refresh,
                da_salvare,
                spento: false,
                respinto: None,
            },
        )
    }

    fn con_stato(
        rete: Rete,
        fornitore: Fornitore,
        credenziali: Credenziali,
        portachiavi: Arc<dyn Portachiavi>,
        stato: Stato,
    ) -> Self {
        Self {
            rete,
            fornitore,
            credenziali,
            portachiavi,
            stato: Mutex::new(stato),
        }
    }

    /// Dà il nome al conto, e mette il refresh token nel portachiavi.
    ///
    /// # Errori
    ///
    /// `settings.secretUnavailable` se il portachiavi non risponde: un conto
    /// collegato il cui refresh token non è da nessuna parte sparirebbe al
    /// primo riavvio, ed è meglio dirlo adesso che scoprirlo domani.
    ///
    /// `cloud.authExpired` se il consenso non ha portato un refresh token
    /// (`offline_access` non concesso, un'app Dropbox coi token di un altro
    /// tipo): il collegamento funzionerebbe un'ora e poi smetterebbe in
    /// silenzio. È la stessa guardia che il backup ha per Google.
    pub fn battezza(&self, conto: &str) -> Result<(), AppError> {
        let mut stato = self.blocca();
        if stato.spento {
            return Err(self.spento());
        }
        stato.conto = Some(conto.to_owned());
        let Some(refresh) = stato.refresh.clone() else {
            return Err(AppError::new(ErrorCode::CloudAuthExpired {
                servizio: self.nome_servizio(),
            })
            .with_cause("il servizio non ha mandato un refresh token"));
        };
        self.portachiavi.scrivi(&nuvola_refresh(conto), &refresh)?;
        stato.da_salvare = false;
        Ok(())
    }

    /// Spegne il permesso: aspetta il rinfresco in corso, se ce n'è uno, poi
    /// dimentica i token in memoria e non ne chiede né ne salva più. Vedi il
    /// `//!`: va chiamato **prima** di cancellare il portachiavi.
    pub fn spegni(&self) {
        let mut stato = self.blocca();
        stato.spento = true;
        stato.token = None;
        stato.refresh = None;
        stato.da_salvare = false;
    }

    /// Il conto, se lo si sa.
    #[must_use]
    pub fn conto(&self) -> Option<String> {
        self.blocca().conto.clone()
    }

    /// Un permesso valido: quello in mano, o uno nuovo se sta per scadere.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::rinfresca`].
    pub fn permesso(&self) -> Result<Permesso, AppError> {
        let mut stato = self.blocca();
        if stato.spento {
            return Err(self.spento());
        }
        if let Some(token) = stato.token.as_ref()
            && ancora_valido(token.scade_ms)
        {
            return Ok(Permesso {
                token: token.access_token.clone(),
                giro: stato.giro,
            });
        }
        self.rinfresca(&mut stato)
    }

    /// Il servizio ha rifiutato `usato` con un `401`: un permesso nuovo.
    ///
    /// Se intanto qualcun altro ha già rinfrescato, si riceve il suo e non si
    /// chiede niente. Vedi il `//!`.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::rinfresca`].
    pub fn rifiutato(&self, usato: &Permesso) -> Result<Permesso, AppError> {
        let mut stato = self.blocca();
        if stato.spento {
            return Err(self.spento());
        }
        if stato.giro != usato.giro
            && let Some(token) = stato.token.as_ref()
        {
            return Ok(Permesso {
                token: token.access_token.clone(),
                giro: stato.giro,
            });
        }
        self.rinfresca(&mut stato)
    }

    /// Chiede un access token nuovo, col lucchetto in mano.
    ///
    /// # Errori
    ///
    /// `cloud.notConnected` se non c'è un refresh token da nessuna parte (il
    /// conto è arrivato con la sincronia, e su questo PC non è collegato);
    /// `cloud.authExpired` se il servizio non lo accetta più, e
    /// `cloud.appRejected` se non accetta l'app — tutti e due, per
    /// [`RICORDO_DEL_RIFIUTO`], anche senza chiederlo di nuovo (vedi il
    /// `//!`); `settings.secretUnavailable` se il portachiavi non si legge;
    /// `net.*`.
    fn rinfresca(&self, stato: &mut Stato) -> Result<Permesso, AppError> {
        if let Some((rifiuto, fino)) = stato.respinto.as_ref()
            && Instant::now() < *fino
        {
            return Err(rifiuto.clone());
        }
        let refresh = match stato.refresh.clone() {
            Some(refresh) => Some(refresh),
            None => match stato.conto.as_deref() {
                Some(conto) => self.portachiavi.leggi(&nuvola_refresh(conto))?,
                None => None,
            },
        };
        let Some(refresh) = refresh else {
            return Err(AppError::new(ErrorCode::CloudNotConnected {
                servizio: self.nome_servizio(),
            })
            .with_cause("nessun refresh token per questo conto"));
        };
        let token =
            match oauth::rinfresca_presso(&self.rete, &self.fornitore, &self.credenziali, &refresh)
            {
                Ok(token) => token,
                Err(err) => {
                    if matches!(
                        err.code(),
                        ErrorCode::CloudAuthExpired { .. } | ErrorCode::CloudAppRejected { .. }
                    ) {
                        stato.respinto = Instant::now()
                            .checked_add(RICORDO_DEL_RIFIUTO)
                            .map(|fino| (err.clone(), fino));
                    }
                    return Err(err);
                }
            };
        stato.respinto = None;
        if let Some(ruotato) = token.refresh_token.clone() {
            stato.refresh = Some(ruotato);
            stato.da_salvare = true;
        }
        if stato.da_salvare
            && let (Some(conto), Some(refresh)) = (stato.conto.as_deref(), stato.refresh.as_deref())
            && self
                .portachiavi
                .scrivi(&nuvola_refresh(conto), refresh)
                .is_ok()
        {
            // Salvato. La copia in memoria resta: è la stessa, e un
            // portachiavi che domani non si lasciasse leggere non deve fermare
            // un conto che il suo refresh token ce l'ha in mano.
            stato.da_salvare = false;
        }
        stato.giro = stato.giro.wrapping_add(1);
        let permesso = Permesso {
            token: token.access_token.clone(),
            giro: stato.giro,
        };
        stato.token = Some(token);
        Ok(permesso)
    }

    /// L'errore di chi chiede un permesso a un accesso spento: il conto, per
    /// questo PC, non è più collegato così.
    fn spento(&self) -> AppError {
        AppError::new(ErrorCode::CloudNotConnected {
            servizio: self.nome_servizio(),
        })
        .with_cause("accesso spento: conto scollegato o app cambiata")
    }

    /// Il nome del servizio, per il diario degli errori.
    fn nome_servizio(&self) -> String {
        match self.fornitore.uso {
            Uso::Musica(servizio) => servizio.nome().to_owned(),
            Uso::Backup => self.fornitore.servizio.nome.to_owned(),
        }
    }

    /// Il lucchetto. Uno avvelenato non è un guasto da propagare: lo stato è
    /// un token e un contatore, e il peggio che può succedere è un rinfresco
    /// in più.
    fn blocca(&self) -> std::sync::MutexGuard<'_, Stato> {
        self.stato.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod prove {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::finto::{Finto, Vista, json};
    use crate::oauth::FORNITORE_MICROSOFT;
    use crate::portachiavi::{Guasto, InMemoria};

    const CONTO: &str = "onedrive-k3x7q2ab";

    fn pubblico() -> Credenziali {
        Credenziali {
            client_id: "app".to_owned(),
            client_secret: None,
        }
    }

    /// Un punto di scambio che conta i rinfreschi e dà ogni volta un token
    /// e un refresh token nuovi, numerati. Ci mette un po', così due fili che
    /// arrivano insieme si sovrappongono davvero.
    fn punto(contati: Arc<AtomicUsize>, pausa: Duration) -> Finto {
        Finto::avvia(move |_: &Vista, _: &str| {
            let n = contati.fetch_add(1, Ordering::SeqCst) + 1;
            std::thread::sleep(pausa);
            json(
                200,
                &serde_json::json!({
                    "access_token": format!("accesso-{n}"),
                    "refresh_token": format!("refresh-{n}"),
                    "expires_in": 3600,
                }),
            )
        })
    }

    fn fornitore(finto: &Finto) -> Fornitore {
        let scambio: &'static str = format!("{}/token", finto.base).leak();
        Fornitore {
            scambio,
            ..FORNITORE_MICROSOFT
        }
    }

    fn token(accesso: &str, scade_fra_ms: i64) -> Token {
        Token {
            access_token: accesso.to_owned(),
            refresh_token: Some("refresh-0".to_owned()),
            scade_ms: rymembra_oauth::adesso_ms() + scade_fra_ms,
            email: None,
        }
    }

    #[test]
    fn due_rifiuti_insieme_fanno_un_rinfresco_solo() {
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto(Arc::clone(&contati), Duration::from_millis(300));
        let portachiavi = Arc::new(InMemoria::nuovo());
        let accesso = Arc::new(Accesso::appena_collegato(
            finto.rete(),
            fornitore(&finto),
            pubblico(),
            token("vecchio", 3_600_000),
            Arc::clone(&portachiavi) as Arc<dyn Portachiavi>,
        ));
        accesso.battezza(CONTO).expect("portachiavi");
        let usato = accesso.permesso().expect("permesso");
        assert_eq!(usato.intestazione(), "Bearer vecchio");

        let barriera = Arc::new(std::sync::Barrier::new(2));
        let fili: Vec<_> = (0..2)
            .map(|_| {
                let accesso = Arc::clone(&accesso);
                let barriera = Arc::clone(&barriera);
                let usato = usato.clone();
                std::thread::spawn(move || {
                    barriera.wait();
                    accesso.rifiutato(&usato).expect("nuovo permesso")
                })
            })
            .collect();
        let nuovi: Vec<Permesso> = fili.into_iter().map(|f| f.join().expect("filo")).collect();

        assert_eq!(contati.load(Ordering::SeqCst), 1, "un rinfresco solo");
        assert!(nuovi.iter().all(|p| p.intestazione() == "Bearer accesso-1"));
        assert_eq!(
            portachiavi.leggi(&nuvola_refresh(CONTO)),
            Ok(Some("refresh-1".to_owned())),
            "il refresh token ruotato è nel portachiavi"
        );
        let vista = finto.viste().remove(0);
        assert_eq!(vista.campo("refresh_token").as_deref(), Some("refresh-0"));
    }

    #[test]
    fn un_token_che_sta_per_scadere_si_rinfresca_prima_di_usarlo() {
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto(Arc::clone(&contati), Duration::ZERO);
        let portachiavi = Arc::new(InMemoria::nuovo());
        portachiavi
            .scrivi(&nuvola_refresh(CONTO), "refresh-salvato")
            .expect("scritto");
        let accesso = Accesso::del_conto(
            finto.rete(),
            fornitore(&finto),
            pubblico(),
            CONTO,
            Arc::clone(&portachiavi) as Arc<dyn Portachiavi>,
        );
        let primo = accesso.permesso().expect("permesso");
        assert_eq!(primo.intestazione(), "Bearer accesso-1");
        let di_nuovo = accesso.permesso().expect("permesso");
        assert_eq!(di_nuovo, primo, "ancora buono: nessuna richiesta");
        assert_eq!(contati.load(Ordering::SeqCst), 1);
        assert_eq!(
            finto.viste().remove(0).campo("refresh_token").as_deref(),
            Some("refresh-salvato"),
            "il primo rinfresco usa quello del portachiavi"
        );
    }

    #[test]
    fn un_rifiuto_di_un_permesso_vecchio_non_rinfresca_di_nuovo() {
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto(Arc::clone(&contati), Duration::ZERO);
        let accesso = Accesso::appena_collegato(
            finto.rete(),
            fornitore(&finto),
            pubblico(),
            token("vecchio", 3_600_000),
            Arc::new(InMemoria::nuovo()) as Arc<dyn Portachiavi>,
        );
        let vecchio = accesso.permesso().expect("permesso");
        let nuovo = accesso.rifiutato(&vecchio).expect("rinfrescato");
        let ancora = accesso.rifiutato(&vecchio).expect("già rinfrescato");
        assert_eq!(nuovo, ancora);
        assert_eq!(contati.load(Ordering::SeqCst), 1);
        // Ma un rifiuto del permesso **nuovo** è un rifiuto vero.
        accesso.rifiutato(&nuovo).expect("ancora");
        assert_eq!(contati.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn senza_refresh_token_il_conto_non_e_collegato() {
        let accesso = Accesso::del_conto(
            Rete::nuova("prova", Duration::from_secs(1)),
            FORNITORE_MICROSOFT,
            pubblico(),
            CONTO,
            Arc::new(InMemoria::nuovo()) as Arc<dyn Portachiavi>,
        );
        let err = accesso.permesso().expect_err("niente da rinfrescare");
        assert_eq!(
            err.code(),
            &ErrorCode::CloudNotConnected {
                servizio: "OneDrive".to_owned()
            }
        );
    }

    #[test]
    fn con_il_portachiavi_giu_il_refresh_ruotato_resta_in_memoria() {
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto(Arc::clone(&contati), Duration::ZERO);
        let accesso = Accesso::appena_collegato(
            finto.rete(),
            fornitore(&finto),
            pubblico(),
            token("vecchio", 0),
            Arc::new(Guasto) as Arc<dyn Portachiavi>,
        );
        assert!(
            accesso.battezza(CONTO).is_err(),
            "un conto senza refresh salvato va detto subito"
        );
        accesso.permesso().expect("rinfresco con quello in memoria");
        let primo = accesso.permesso().expect("ancora buono");
        accesso.rifiutato(&primo).expect("secondo rinfresco");
        let viste = finto.viste();
        assert_eq!(viste.len(), 2);
        assert_eq!(
            viste[0].campo("refresh_token").as_deref(),
            Some("refresh-0")
        );
        assert_eq!(
            viste[1].campo("refresh_token").as_deref(),
            Some("refresh-1"),
            "il secondo rinfresco usa quello ruotato dal primo, non il vecchio"
        );
    }

    #[test]
    fn un_consenso_senza_refresh_token_non_battezza_un_conto() {
        // Senza refresh token il collegamento varrebbe un'ora e poi
        // sparirebbe al primo riavvio: va detto subito, non scoperto domani.
        let portachiavi = Arc::new(InMemoria::nuovo());
        let accesso = Accesso::appena_collegato(
            Rete::nuova("prova", Duration::from_secs(1)),
            FORNITORE_MICROSOFT,
            pubblico(),
            Token {
                refresh_token: None,
                ..token("a", 3_600_000)
            },
            Arc::clone(&portachiavi) as Arc<dyn Portachiavi>,
        );
        let err = accesso.battezza(CONTO).expect_err("niente da salvare");
        assert_eq!(
            err.code(),
            &ErrorCode::CloudAuthExpired {
                servizio: "OneDrive".to_owned()
            }
        );
        assert_eq!(portachiavi.leggi(&nuvola_refresh(CONTO)), Ok(None));
    }

    #[test]
    fn un_accesso_spento_non_chiede_e_non_riscrive_niente() {
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto(Arc::clone(&contati), Duration::ZERO);
        let portachiavi = Arc::new(InMemoria::nuovo());
        let accesso = Accesso::appena_collegato(
            finto.rete(),
            fornitore(&finto),
            pubblico(),
            // Già scaduto: il primo permesso vorrebbe un rinfresco.
            token("vecchio", -1),
            Arc::clone(&portachiavi) as Arc<dyn Portachiavi>,
        );
        accesso.battezza(CONTO).expect("portachiavi");
        accesso.spegni();
        portachiavi
            .cancella(&nuvola_refresh(CONTO))
            .expect("cancellato");

        let err = accesso.permesso().expect_err("spento");
        assert_eq!(err.code().kind().code(), "cloud.notConnected");
        let usato = Permesso {
            token: "vecchio".to_owned(),
            giro: 0,
        };
        assert!(accesso.rifiutato(&usato).is_err());
        assert!(accesso.battezza(CONTO).is_err());
        assert_eq!(contati.load(Ordering::SeqCst), 0, "nessun rinfresco");
        assert_eq!(
            portachiavi.leggi(&nuvola_refresh(CONTO)),
            Ok(None),
            "il refresh token tolto non rinasce"
        );
    }

    #[test]
    fn lo_spegnimento_aspetta_il_rinfresco_in_corso() {
        // Un rinfresco lento parte; lo spegnimento arriva a metà. Quando lo
        // spegnimento torna, il salvataggio del rinfresco è già avvenuto: chi
        // cancella il portachiavi dopo non viene smentito.
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto(Arc::clone(&contati), Duration::from_millis(300));
        let portachiavi = Arc::new(InMemoria::nuovo());
        let accesso = Arc::new(Accesso::appena_collegato(
            finto.rete(),
            fornitore(&finto),
            pubblico(),
            token("vecchio", -1),
            Arc::clone(&portachiavi) as Arc<dyn Portachiavi>,
        ));
        accesso.battezza(CONTO).expect("portachiavi");
        let in_volo = {
            let accesso = Arc::clone(&accesso);
            std::thread::spawn(move || accesso.permesso())
        };
        while contati.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        accesso.spegni();
        portachiavi
            .cancella(&nuvola_refresh(CONTO))
            .expect("cancellato");
        assert!(
            in_volo.join().expect("filo").is_ok(),
            "quello in volo finisce"
        );
        assert_eq!(portachiavi.leggi(&nuvola_refresh(CONTO)), Ok(None));
        assert!(accesso.permesso().is_err());
    }

    /// Un punto di scambio che conta le richieste e risponde sempre con
    /// `stato` e questo `error`.
    fn punto_che_rifiuta(contati: Arc<AtomicUsize>, stato: u16, motivo: &'static str) -> Finto {
        Finto::avvia(move |_: &Vista, _: &str| {
            contati.fetch_add(1, Ordering::SeqCst);
            json(stato, &serde_json::json!({ "error": motivo }))
        })
    }

    /// Un conto collegato, col refresh token nel portachiavi.
    fn del_conto(finto: &Finto) -> Accesso {
        let portachiavi = Arc::new(InMemoria::nuovo());
        portachiavi
            .scrivi(&nuvola_refresh(CONTO), "refresh-salvato")
            .expect("scritto");
        Accesso::del_conto(
            finto.rete(),
            fornitore(finto),
            pubblico(),
            CONTO,
            portachiavi as Arc<dyn Portachiavi>,
        )
    }

    #[test]
    fn un_rifiuto_definitivo_non_si_richiede_a_ogni_permesso() {
        // Il consenso revocato, o l'app di Google in «Testing» scaduta: la
        // scansione chiede un permesso per ogni file da leggere, e ognuno era
        // una richiesta al punto di scambio, rifiutata uguale.
        for (motivo, atteso) in [
            ("invalid_grant", "cloud.authExpired"),
            ("invalid_client", "cloud.appRejected"),
        ] {
            let contati = Arc::new(AtomicUsize::new(0));
            let finto = punto_che_rifiuta(Arc::clone(&contati), 400, motivo);
            let accesso = del_conto(&finto);
            for _ in 0..5 {
                let err = accesso.permesso().expect_err("rifiutato");
                assert_eq!(err.code().kind().code(), atteso, "{motivo}");
            }
            let usato = Permesso {
                token: "vecchio".to_owned(),
                giro: 0,
            };
            let err = accesso.rifiutato(&usato).expect_err("rifiutato");
            assert_eq!(err.code().kind().code(), atteso, "{motivo}");
            assert_eq!(
                contati.load(Ordering::SeqCst),
                1,
                "{motivo}: una richiesta sola, poi il rifiuto ricordato"
            );
        }
    }

    #[test]
    fn un_rifiuto_ricordato_scade() {
        // Passato il ricordo si richiede: il filo del conto riprova di rado, e
        // un'app rimessa a posto sul servizio deve tornare da sé.
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto_che_rifiuta(Arc::clone(&contati), 400, "invalid_client");
        let accesso = del_conto(&finto);
        accesso.permesso().expect_err("rifiutato");
        accesso.permesso().expect_err("ricordato");
        assert_eq!(contati.load(Ordering::SeqCst), 1);
        // Il ricordo scaduto adesso, senza aspettare dieci minuti.
        if let Some((_, fino)) = accesso.blocca().respinto.as_mut() {
            *fino = Instant::now();
        }
        accesso.permesso().expect_err("ancora rifiutato");
        assert_eq!(contati.load(Ordering::SeqCst), 2, "si è richiesto");
        accesso.permesso().expect_err("di nuovo ricordato");
        assert_eq!(contati.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn un_rinfresco_riuscito_dimentica_il_rifiuto() {
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto(Arc::clone(&contati), Duration::ZERO);
        let accesso = del_conto(&finto);
        accesso.blocca().respinto = Some((
            AppError::new(ErrorCode::CloudAppRejected {
                servizio: "OneDrive".to_owned(),
            }),
            Instant::now(),
        ));
        accesso.permesso().expect("il servizio adesso accetta");
        assert!(accesso.blocca().respinto.is_none());
    }

    #[test]
    fn un_rifiuto_passeggero_si_richiede_ogni_volta() {
        // Un servizio giù per un momento non è un consenso morto: ogni
        // permesso riprova.
        let contati = Arc::new(AtomicUsize::new(0));
        let finto = punto_che_rifiuta(Arc::clone(&contati), 503, "temporarily_unavailable");
        let accesso = del_conto(&finto);
        for _ in 0..5 {
            let err = accesso.permesso().expect_err("giù");
            assert_ne!(err.code().kind().code(), "cloud.authExpired");
        }
        assert_eq!(contati.load(Ordering::SeqCst), 5, "nessun ricordo");
    }

    #[test]
    fn il_permesso_non_si_stampa() {
        let permesso = Permesso {
            token: "segretissimo".to_owned(),
            giro: 3,
        };
        assert!(!format!("{permesso:?}").contains("segretissimo"));
    }
}
