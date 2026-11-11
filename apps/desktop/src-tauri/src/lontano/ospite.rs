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

use rymembra_domain::errors::AppError;
use rymembra_lontano::nodo::ATTESA_CONFERMA;
use rymembra_lontano::{Domanda, Evento, Presentazione, Risposta};
use tauri::AppHandle;

use super::filo::{Collegato, Comune, Modifica, Posta, prendi, scrivi_il_registro};
use super::ipc::RichiestaIpc;
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::adesso_ms;

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
        // Il cancello si apre subito — la connessione che arriva adesso è di
        // questo dispositivo — e il collegamento resta nel registro finché la
        // libreria non l'ha scritto, anche se una scansione la tiene
        // (decisione D-24): prima una scrittura che non arrivava in dieci
        // secondi si perdeva, e la prima lettura chiudeva fuori il dispositivo
        // appena accolto (reperto R01-2). Con lui la fiducia di «Ascolta
        // insieme» per la chiave che ha dato: vedi `filo::associa`.
        self.comune.registra(
            chiave,
            Modifica::Associa(Collegato {
                nome: chi.nome.clone(),
                piattaforma: chi.piattaforma.clone(),
                dispositivo: chi.dispositivo.clone(),
                chiave_insieme: chi.chiave_insieme.clone(),
                indirizzi: indirizzi.iter().map(ToString::to_string).collect(),
                associato_ms: adesso_ms(),
            }),
        );
        // Chi ha il QR davanti non lo deve rifare: il QR è consumato.
        *prendi(&self.comune.invito) = None;
        // La piattaforma la dichiara l'altro, e senza limiti: nel diario ne va
        // solo quanto basta a riconoscerla, come in `filo.rs`.
        nota!(
            "[lontano] dispositivo collegato ({})",
            chi.piattaforma
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(16)
                .collect::<String>()
        );
        scrivi_il_registro(&self.app, &self.comune);
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
        // Fuori subito, e nel registro finché la libreria non l'ha scritto:
        // prima un «Dimenticami» arrivato con la libreria occupata oltre dieci
        // secondi si perdeva, e il dispositivo restava collegato qui per
        // sempre (reperto R01-2, decisione D-24). La fiducia di «Ascolta
        // insieme» resta: la toglie la persona, con «Dimentica» (vedi
        // `dispositivi::revoca`).
        self.comune.registra(da, Modifica::Revoca);
        // I suoi brani in coda non si aprono più: l'altro non ci risponde.
        crate::riproduzione::togli_lontane(&self.app, da);
        scrivi_il_registro(&self.app, &self.comune);
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
