//! Il testo della canzone di un brano di un altro dispositivo, lato rete.
//!
//! Le regole — cosa si dà, cosa si accetta, come si fondono le correzioni — sono
//! in `rymembra_app::testi::condiviso`; qui c'è quel che ha una rete.
//!
//! # Chi chiede
//!
//! [`testo_del_proprietario`] chiede il testo al dispositivo che possiede il
//! brano di una riga specchio, e lo dà già convalidato. **Non fallisce mai**:
//! un dispositivo che non risponde, che è di una versione di prima (non dichiara
//! `testi`), che non ha più quel brano o che manda un testo fuori dai tetti vale
//! «nessun testo dal PC», e chi chiama continua con la catena di qui — il
//! catalogo dei testi, da qui, col pannello aperto. Mai una fascia rossa per
//! un testo che non è arrivato. Solo un guasto che non è di rete lascia una riga
//! nel diario, col solo codice.
//!
//! Non si chiede in due casi:
//!
//! - il dispositivo non risponde **adesso** (non c'è nel registro delle
//!   presenze): chiedere vorrebbe dire aspettare la scadenza con il pannello in
//!   «leggo»;
//! - una correzione a quel brano **aspetta in posta**: il testo di qui è quello
//!   giusto, e quel che dice il proprietario — che non l'ha ancora — non lo
//!   deve scavalcare.
//!
//! # La posta
//!
//! [`consegna`] manda a un dispositivo la correzione che aspetta per un brano:
//! prima il testo, poi lo scarto, ognuno con la sua domanda e nell'ordine in
//! cui vanno applicati. È innocua se si ripete — dice come deve restare il testo
//! e non cosa aggiungere — quindi una consegna a metà, interrotta dal
//! dispositivo che sparisce, si rifà per intero al giro dopo.

use std::time::Duration;

use rymembra_app::dispositivi::{SCADENZA_POSTA_MS, VocePosta};
use rymembra_app::testi::Remoto;
use rymembra_app::testi::condiviso::{
    self, Correzione, DalProprietario, NuovoTesto, Scrittura, TestoCondiviso,
};
use rymembra_domain::errors::{AppError, Domain, ErrorCode, ErrorCodeKind};
use rymembra_lontano::protocollo::{CAPACITA_TESTI, ScritturaTesto};
use rymembra_lontano::{Domanda, Risposta};
use tauri::AppHandle;

use super::specchio::{DallApp, Dati};
use crate::nota;
use crate::stato::adesso_ms;

/// Quanto si aspetta il testo dal proprietario: una risposta di qualche
/// kilobyte, anche via relay. Oltre, si va avanti senza.
const ATTESA_TESTO: Duration = Duration::from_secs(8);

/// Quanto si aspetta l'altro per una correzione in posta.
const ATTESA_POSTA: Duration = Duration::from_secs(10);

// ── fra il nucleo e il filo ─────────────────────────────────────────────────

/// Una scrittura del nucleo, nella forma del protocollo.
pub(super) fn in_protocollo(scrittura: &Scrittura) -> ScritturaTesto {
    match scrittura {
        Scrittura::Scarto(scarto_ms) => ScritturaTesto::Scarto {
            scarto_ms: *scarto_ms,
        },
        Scrittura::Testo(NuovoTesto::Mano { lrc }) => ScritturaTesto::Mano { lrc: lrc.clone() },
        Scrittura::Testo(NuovoTesto::Scelta {
            id_catalogo,
            piatto,
            sincronizzato,
            strumentale,
        }) => ScritturaTesto::Scelta {
            id_catalogo: *id_catalogo,
            piatto: piatto.clone(),
            sincronizzato: sincronizzato.clone(),
            strumentale: *strumentale,
        },
        Scrittura::Testo(NuovoTesto::Rifiuta) => ScritturaTesto::Rifiuta,
    }
}

/// Una scrittura del protocollo, nella forma del nucleo; `None` per una
/// correzione di una versione più nuova, che questa non sa fare.
pub(super) fn dal_protocollo(scrittura: ScritturaTesto) -> Option<Scrittura> {
    Some(match scrittura {
        ScritturaTesto::Scarto { scarto_ms } => Scrittura::Scarto(scarto_ms),
        ScritturaTesto::Mano { lrc } => Scrittura::Testo(NuovoTesto::Mano { lrc }),
        ScritturaTesto::Scelta {
            id_catalogo,
            piatto,
            sincronizzato,
            strumentale,
        } => Scrittura::Testo(NuovoTesto::Scelta {
            id_catalogo,
            piatto,
            sincronizzato,
            strumentale,
        }),
        ScritturaTesto::Rifiuta => Scrittura::Testo(NuovoTesto::Rifiuta),
        ScritturaTesto::Sconosciuta => return None,
    })
}

// ── chi chiede ──────────────────────────────────────────────────────────────

/// Il testo che il proprietario del brano ha, o `None`: vedi il `//!`.
pub(super) fn testo_del_proprietario<D: Dati>(
    dati: &D,
    remoto: &Remoto,
) -> Option<DalProprietario> {
    let chiave = remoto.dispositivo.as_str();
    // Finché una correzione aspetta in posta, vale il testo di qui.
    match dati.leggi(|c| condiviso::in_attesa(c, remoto)) {
        Ok(false) => {}
        Ok(true) => return None,
        Err(err) => {
            nota!(
                "[lontano] la posta dei testi non si legge codice={}",
                err.code().kind().code()
            );
            return None;
        }
    }
    if !dati.associato(chiave) || !dati.risponde(chiave) || !dati.dichiara(chiave, CAPACITA_TESTI) {
        return None;
    }
    let domanda = Domanda::Testo {
        brano: remoto.id_remoto,
    };
    let esito = dati
        .chiedi(chiave, domanda, ATTESA_TESTO)
        .and_then(|risposta| match risposta {
            Risposta::Fatto => Ok(None),
            Risposta::Dati(valore) => serde_json::from_value::<TestoCondiviso>(valore)
                .map_err(|err| {
                    AppError::new(ErrorCode::LontanoProtocollo).with_cause(err.to_string())
                })
                .and_then(condiviso::convalida_ricevuto)
                .map(Some),
            _ => Err(AppError::new(ErrorCode::LontanoProtocollo)
                .with_cause("a una domanda di testo non si risponde con dei byte")),
        });
    match esito {
        Ok(dal) => dal,
        Err(err) => {
            let kind = err.code().kind();
            // Fuori campo, spento, non più condiviso: è «non ce l'ha», non un
            // guasto. Il resto lascia il codice, e non la causa: vedi `nota!`.
            let di_rete =
                kind.domain() == Domain::Lontano && kind != ErrorCodeKind::LontanoProtocollo;
            if !di_rete {
                nota!(
                    "[lontano] il testo di un altro dispositivo non si usa codice={}",
                    kind.code()
                );
            }
            None
        }
    }
}

/// Il testo del proprietario di una riga specchio, per chi ha un'app: vedi
/// [`testo_del_proprietario`].
pub(crate) fn dal_proprietario(app: &AppHandle, remoto: &Remoto) -> Option<DalProprietario> {
    let dati = DallApp::nuovo(app).ok()?;
    testo_del_proprietario(&dati, remoto)
}

// ── la posta ────────────────────────────────────────────────────────────────

/// Com'è andata la consegna di una voce di posta dei testi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Consegna {
    /// Arrivata: si toglie.
    Fatta,
    /// Non ha più senso — il brano non è più condiviso, o la voce non si legge:
    /// si toglie.
    Persa,
    /// Il dispositivo non la prende (non conosce i testi, o la rifiuta): resta
    /// in posta, e si passa alla voce dopo. Oltre [`SCADENZA_POSTA_MS`] si
    /// lascia.
    Tieni,
    /// Il dispositivo non risponde: basta per adesso.
    Ferma,
}

/// Manda a `chiave` la correzione che aspetta in `voce`.
pub(super) fn consegna<D: Dati>(dati: &D, chiave: &str, voce: &VocePosta) -> Consegna {
    let Some(brano) = voce.chiave.parse::<i64>().ok().filter(|id| *id > 0) else {
        return Consegna::Persa;
    };
    let Some(correzione) = Correzione::da_corpo(&voce.corpo) else {
        // Una voce che non si legge non diventerà leggibile.
        return Consegna::Persa;
    };
    for scrittura in correzione.scritture() {
        let domanda = Domanda::TestoScrivi {
            brano,
            cosa: in_protocollo(&scrittura),
        };
        let Err(err) = dati.chiedi(chiave, domanda, ATTESA_POSTA) else {
            continue;
        };
        return match err.code().kind() {
            // Il brano non c'è più, o non è più condiviso: non c'è più niente
            // da dirgli.
            ErrorCodeKind::LontanoFileCambiato => Consegna::Persa,
            // Una versione che non lo capisce, o non lo dichiara ancora: si
            // tiene un po', senza fermare le altre voci, poi si lascia.
            ErrorCodeKind::LontanoProtocollo => {
                if adesso_ms().saturating_sub(voce.quando_ms) > SCADENZA_POSTA_MS {
                    Consegna::Persa
                } else {
                    Consegna::Tieni
                }
            }
            _ => Consegna::Ferma,
        };
    }
    Consegna::Fatta
}
