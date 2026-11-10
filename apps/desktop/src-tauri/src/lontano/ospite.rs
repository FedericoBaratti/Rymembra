//! Quel che il nodo chiede all'app: il cancello, le conferme, le associazioni
//! fatte, le domande degli altri dispositivi.
//!
//! Il nodo chiama questi metodi da un filo bloccante del suo runtime (vedi
//! `rymembra_lontano::nodo`), tranne [`Ospite::evento`], che arriva da un filo
//! della rete: quello passa la notizia al filo del collegamento e torna
//! subito.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rymembra_app::dispositivi::{self, Nuovo};
use rymembra_domain::errors::AppError;
use rymembra_lontano::nodo::ATTESA_CONFERMA;
use rymembra_lontano::{Domanda, Evento, Presentazione, Risposta};
use tauri::AppHandle;

use super::filo::{Comune, Posta, in_libreria, prendi};
use super::ipc::RichiestaIpc;
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::adesso_ms;

/// Quanto si aspetta la libreria per scrivere un'associazione: è il momento
/// che la persona sta guardando, e un no per una libreria occupata la
/// costringerebbe a rifare il QR.
const ATTESA_SCRITTURA: Duration = Duration::from_secs(10);

/// L'ospite del nodo.
pub struct Ospite {
    app: AppHandle,
    comune: Arc<Comune>,
    /// Le chiavi a cui chi guarda ha appena detto di sì: la loro associazione
    /// (`nuovo_pari`) si annuncia anche qui, dove si mostrava il QR. Chi l'ha
    /// letto lo sa già dal suo comando.
    confermati: Mutex<HashSet<String>>,
}

impl Ospite {
    pub(super) fn nuovo(app: AppHandle, comune: Arc<Comune>) -> Self {
        Self {
            app,
            comune,
            confermati: Mutex::new(HashSet::new()),
        }
    }
}

impl Ospite {
    /// Un dispositivo collegato col QR è anche un PC fidato di «Ascolta
    /// insieme», se ha dato la sua chiave: chiederlo di nuovo in casa sarebbe
    /// una seconda associazione degli stessi due dispositivi. Ma la chiave la
    /// dichiara l'altro, e niente prova che sia sua: un PC che la persona
    /// aveva bloccato resta bloccato, e uno che c'è già tiene il suo nome
    /// (`rymembra_app::insieme::fidato_dal_qr`). «Dimentica» la toglie.
    /// Un guasto qui non disfa il collegamento: in casa resta il codice.
    fn fidato_anche_insieme(&self, chi: &Presentazione) {
        let Some(chiave) = chi.chiave_insieme.as_deref() else {
            return;
        };
        let scritto = in_libreria(&self.app, ATTESA_SCRITTURA, |c| {
            rymembra_app::insieme::fidato_dal_qr(c, chiave, &chi.nome, adesso_ms())
        });
        match scritto {
            Ok(Some(_)) => crate::insieme::fidati_cambiati(&self.app),
            Ok(None) => nota!(
                "[lontano] la chiave di «Ascolta insieme» del dispositivo collegato è di un PC bloccato: resta bloccato"
            ),
            Err(err) => nota!(
                "[lontano] la chiave di «Ascolta insieme» non si scrive codice={}",
                err.code().kind().code()
            ),
        }
    }
}

impl rymembra_lontano::Ospite for Ospite {
    fn associato(&self, chiave: &str) -> bool {
        self.comune.associato(chiave)
    }

    fn conferma(&self, chiave: &str, chi: &Presentazione) -> bool {
        let id = self.comune.prossima.fetch_add(1, Ordering::Relaxed);
        let (risposta, attesa) = channel();
        let richiesta = RichiestaIpc {
            id,
            nome: rymembra_app::insieme::ripulisci_nome(&chi.nome),
            piattaforma: chi
                .piattaforma
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(16)
                .collect(),
            // La chiave l'ha provata QUIC: è sua, a differenza del nome.
            impronta: rymembra_lontano::collegamento::impronta_di(chiave).unwrap_or_default(),
            chiave: chiave.to_owned(),
        };
        prendi(&self.comune.richieste).push((richiesta.clone(), risposta));
        self.comune.sveglia(Posta::Pubblica);
        self.app.emetti("lontano:richiesta", richiesta);
        // Un po' meno del nodo, che dopo chiude: la risposta deve arrivare
        // all'altro prima.
        let si = attesa
            .recv_timeout(ATTESA_CONFERMA.saturating_sub(Duration::from_secs(5)))
            .unwrap_or(false);
        prendi(&self.comune.richieste).retain(|(r, _)| r.id != id);
        self.comune.sveglia(Posta::Pubblica);
        nota!(
            "[lontano] richiesta di collegamento {}",
            if si { "accolta" } else { "respinta" }
        );
        if si {
            prendi(&self.confermati).insert(chiave.to_owned());
        }
        si
    }

    fn ritirata(&self, chiave: &str) {
        // Il no a chi aspetta: `conferma` torna, e toglie la domanda.
        if let Some((_, risposta)) = prendi(&self.comune.richieste)
            .iter()
            .find(|(r, _)| r.chiave == chiave)
        {
            let _ = risposta.send(false);
        }
        nota!("[lontano] chi aveva letto il QR ha rinunciato");
    }

    fn nuovo_pari(&self, chiave: &str, chi: &Presentazione, indirizzi: &[SocketAddr]) {
        // Il cancello si apre subito: la connessione che arriva adesso è di
        // questo dispositivo.
        self.comune.ammetti(chiave, true);
        // Chi ha il QR davanti non lo deve rifare: il QR è consumato.
        *prendi(&self.comune.invito) = None;
        let indirizzi: Vec<String> = indirizzi.iter().map(ToString::to_string).collect();
        let scritto = in_libreria(&self.app, ATTESA_SCRITTURA, |c| {
            dispositivi::associa(
                c,
                Nuovo {
                    chiave,
                    nome: &chi.nome,
                    piattaforma: &chi.piattaforma,
                    dispositivo: &chi.dispositivo,
                    chiave_insieme: chi.chiave_insieme.as_deref(),
                    indirizzi: &indirizzi,
                },
                adesso_ms(),
            )
        });
        match scritto {
            Ok(_) => nota!("[lontano] dispositivo collegato ({})", chi.piattaforma),
            Err(err) => nota!(
                "[lontano] il dispositivo collegato non si scrive codice={}",
                err.code().kind().code()
            ),
        }
        self.fidato_anche_insieme(chi);
        if prendi(&self.confermati).remove(chiave) {
            self.app.emetti(
                "lontano:collegato",
                rymembra_app::insieme::ripulisci_nome(&chi.nome),
            );
        }
        self.comune.sveglia(Posta::Rileggi {
            risposta: None,
            subito: false,
        });
    }

    fn rispondi(&self, da: &str, domanda: Domanda) -> Result<Risposta, AppError> {
        super::servizio::rispondi(&self.app, da, domanda)
    }

    fn dimenticato(&self, da: &str) {
        self.comune.ammetti(da, false);
        // I suoi brani in coda non si aprono più: l'altro non ci risponde.
        crate::riproduzione::togli_lontane(&self.app, da);
        match in_libreria(&self.app, ATTESA_SCRITTURA, |c| dispositivi::revoca(c, da)) {
            // La sua chiave di «Ascolta insieme» non è più fidata: la rete di
            // «Ascolta insieme» rilegge e chiude le sue connessioni.
            Ok(true) => crate::insieme::fidati_cambiati(&self.app),
            Ok(false) => {}
            Err(err) => nota!(
                "[lontano] un dispositivo che ci ha dimenticati non si segna codice={}",
                err.code().kind().code()
            ),
        }
        self.comune.sveglia(Posta::Rileggi {
            risposta: None,
            subito: false,
        });
    }

    fn evento(&self, evento: Evento) {
        self.comune.sveglia(Posta::Evento(evento));
    }

    fn insieme(&self, da: &str, flusso: rymembra_lontano::Flusso) {
        crate::insieme::accogli_lontano(
            &self.app,
            da,
            Box::new(super::insieme::PresaLontana::nuova(flusso)),
        );
    }
}
