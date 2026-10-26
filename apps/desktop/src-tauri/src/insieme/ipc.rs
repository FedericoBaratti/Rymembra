//! Le forme che la finestra riceve da «Ascolta insieme».
//!
//! Gemelle, campo per campo, dei tipi di `src/ipc.ts`: chi cambia una delle
//! due cambia l'altra nello stesso momento. Sono tutte fotografie: la finestra
//! non tiene uno stato suo della sessione, ridisegna quel che arriva con
//! l'evento `insieme:stato`.
//!
//! # Cosa non c'è
//!
//! Chiavi intere, gettoni, codici d'errore dell'altro PC sotto forma di frase.
//! Gli indirizzi ci sono solo dove la persona li ha scritti a mano o deve
//! poterli leggere per capire un guasto: la finestra è di questo PC.

use rymembra_insieme::messaggi::Diritti;
use serde::Serialize;

/// Il pannello «Ascolta insieme». Evento `insieme:stato`, comando
/// `insieme_stato`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoInsiemeIpc {
    /// L'interruttore delle impostazioni. Spento di serie.
    pub attivo: bool,
    /// Il nome di questo PC, come lo vedono gli altri.
    pub nome: String,
    /// In che fase è questo PC.
    pub fase: FaseIpc,
    /// La porta su cui si ospita, quando si ospita.
    pub porta: Option<u16>,
    /// Il dialogo che spiega la richiesta del firewall è già stato visto.
    pub firewall_spiegato: bool,
    /// La sessione in corso, se ce n'è una.
    pub sessione: Option<SessioneIpc>,
    /// I PC che ospitano, visti in rete adesso.
    pub vicini: Vec<VicinoIpc>,
    /// I PC associati, visti o no.
    pub fidati: Vec<FidatoIpc>,
    /// L'associazione in corso, se ce n'è una.
    pub associazione: Option<AssociazioneIpc>,
    /// Gli inviti ricevuti e non ancora risposti.
    pub inviti: Vec<InvitoIpc>,
    /// Chi ospita: le richieste d'ingresso non ancora risposte.
    pub richieste: Vec<RichiestaIpc>,
    /// Gli indirizzi scritti a mano (`ip:porta`), per le reti dove la
    /// scoperta non passa.
    pub indirizzi: Vec<String>,
    /// Mentre si ospita: gli indirizzi `ip:porta` di questo PC che un collega
    /// può scrivere a mano, il migliore per primo (quello della strada
    /// predefinita), le schede virtuali in fondo. Vuoto se non si ospita.
    pub indirizzi_di_qui: Vec<String>,
    /// Chi segue: com'è agganciato all'host.
    pub sincronia: Option<SincroniaIpc>,
}

/// La fase di questo PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FaseIpc {
    /// L'interruttore è spento: niente rete.
    Spenta,
    /// Acceso, fuori da ogni sessione: si cercano gli host e si ricevono inviti.
    Libera,
    /// Si sta entrando: invito accettato o richiesta mandata, in attesa.
    Entrando,
    /// Si ospita una sessione.
    Ospita,
    /// Si segue la sessione di un altro.
    Membro,
    /// Si sta uscendo.
    Uscendo,
}

/// La sessione in corso.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessioneIpc {
    /// Il nome del PC che ospita.
    pub host: String,
    /// Questo PC è l'host.
    pub mia: bool,
    /// Chi c'è, l'host per primo.
    pub persone: Vec<PersonaIpc>,
    /// I clic di calibrazione stanno suonando.
    pub calibrazione: bool,
    /// Le voci della coda che un **membro** ha aggiunto, e che non sono di
    /// questo PC: la finestra scrive «di Marco» accanto alla riga. Per
    /// maniglia, cioè per l'id che la finestra ha in coda. Le voci di chi
    /// ospita non ci sono: sono la coda stessa, e la fascia della sessione dice
    /// già di chi è.
    pub proprietari: Vec<ProprietarioIpc>,
}

/// Di chi è una voce della coda. Vedi [`SessioneIpc::proprietari`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProprietarioIpc {
    /// La maniglia della voce: l'id in coda su questo PC.
    pub id: i64,
    /// Il nome del PC a cui appartiene.
    pub nome: String,
}

/// Una persona nella sessione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaIpc {
    /// Il suo numero nella sessione; `0` è l'host.
    pub membro: u32,
    /// Il nome del suo PC.
    pub nome: String,
    /// È questo PC.
    pub io: bool,
    /// Che cosa può fare: chi ospita lo cambia da qui. L'host è `tutto`.
    pub diritti: Diritti,
}

/// Un PC che ospita, visto in rete.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VicinoIpc {
    /// La chiave per i comandi (`insieme_associa`): l'impronta in esadecimale.
    pub chiave: String,
    /// L'impronta come la si legge, a gruppi: `3fa2 91c0 …`. Serve a
    /// distinguere due PC con lo stesso nome.
    pub impronta: String,
    /// Il nome che dichiara, quando la connessione ce l'ha detto.
    pub nome: Option<String>,
    /// L'id in `insieme_pari`, se è un PC associato.
    pub pari: Option<i64>,
    /// È bloccato: si vede, ma non ci si collega.
    pub bloccato: bool,
}

/// Un PC associato.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FidatoIpc {
    /// L'id in `insieme_pari`.
    pub id: i64,
    /// Il nome: quello scelto qui, o quello che il PC dichiarava.
    pub nome: String,
    /// L'impronta, a gruppi.
    pub impronta: String,
    /// Bloccato.
    pub bloccato: bool,
    /// Quando è stato associato, ms dall'epoca.
    pub associato_ms: i64,
    /// L'ultima volta che si è visto, ms dall'epoca.
    pub visto_ms: Option<i64>,
    /// È collegato adesso (connessione di controllo aperta).
    pub presente: bool,
    /// Sta ospitando.
    pub ospita: bool,
    /// È nella sessione di questo PC, o questo PC è nella sua.
    pub in_sessione: bool,
}

/// L'associazione in corso.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociazioneIpc {
    /// Il suo numero, per `insieme_codice`.
    pub id: u64,
    /// Il nome dell'altro PC.
    pub nome: String,
    /// Le quattro cifre, quando ci sono. `None`: si sta ancora calcolando.
    pub codice: Option<String>,
    /// Questo PC ha già detto che coincide, e aspetta l'altro.
    pub confermato: bool,
}

/// Un invito ricevuto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitoIpc {
    /// Il suo numero, per `insieme_rispondi`.
    pub id: u64,
    /// Il nome del PC che invita.
    pub da: String,
    /// L'id in `insieme_pari` di chi invita.
    pub pari: i64,
    /// Quando scade, ms dall'epoca su questo PC.
    pub scade_ms: i64,
}

/// Una richiesta d'ingresso ricevuta da chi ospita.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichiestaIpc {
    /// Il suo numero, per `insieme_ammetti`.
    pub id: u64,
    /// Il nome del PC che chiede.
    pub da: String,
    /// L'id in `insieme_pari` di chi chiede.
    pub pari: i64,
    /// Quando scade, ms dall'epoca su questo PC.
    pub scade_ms: i64,
}

/// Com'è agganciato chi segue.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SincroniaIpc {
    /// La correzione è a regime: l'errore è sotto la soglia da qualche secondo.
    pub agganciato: bool,
    /// L'errore filtrato, in ms, positivo se questo PC è avanti.
    pub errore_ms: Option<f64>,
    /// Quanto la stima dell'orologio è incerta, in ms.
    pub incertezza_ms: Option<f64>,
}

/// Un avviso di passaggio. Evento `insieme:avviso`.
///
/// La **chiave** della frase e non la frase: `together.notice.joined`, o
/// `errors.insieme.connessioneCaduta`. La finestra la traduce con `nome`
/// come parametro.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvvisoIpc {
    /// La chiave i18n.
    pub chiave: String,
    /// Il nome da metterci dentro, se la frase ne ha uno.
    pub nome: Option<String>,
}

/// «Ascolta insieme» visto dal lettore: il campo `insieme` di
/// `StatoRiproduzione`. `None` fuori sessione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsiemeNelLettore {
    /// Questo PC ospita (`true`) o segue (`false`).
    pub ospita: bool,
    /// Il nome del PC che ospita.
    pub host: String,
    /// Quante persone, l'host compreso.
    pub persone: u32,
    /// Di chi è la voce che suona: `None` se è di questo PC.
    pub proprietario: Option<String>,
    /// La voce che suona si legge da un file di questo PC (sua, o una copia
    /// identica trovata qui), e non dalla rete.
    pub locale: bool,
    /// Chi segue: la sincronia è agganciata. Chi ospita: sempre `true`.
    pub agganciato: bool,
    /// Che cosa fanno i gesti di trasporto e di coda: chi ospita tutto, chi
    /// segue quel che chi ospita gli ha concesso.
    pub diritti: Diritti,
    /// Suona la voce di calibrazione: la finestra ne scrive il titolo nella
    /// lingua di chi guarda, invece di quello che la voce porta.
    pub calibrazione: bool,
    /// Cresce ogni volta che le schede delle voci della sessione cambiano
    /// (una copertina arrivata, voci nuove registrate): la coda della finestra
    /// rilegge le sue righe, che altrimenti tiene per id e non richiede più.
    pub revisione: u64,
}
