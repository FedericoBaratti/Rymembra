//! Quel che la finestra riceve del collegamento via internet: il pannello
//! (comando `lontano_stato`, evento `lontano:stato`), il QR, le domande di
//! conferma.
//!
//! Le chiavi viaggiano intere: la finestra le usa come nomi dei dispositivi
//! nei comandi, e sono pubbliche. Gli indirizzi no: alla finestra non
//! servono, e il diario è il posto dove un indirizzo non deve finire.

use rymembra_app::dispositivi::DatiMobili;
use serde::Serialize;

/// Il pannello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatoLontanoIpc {
    /// «Raggiungibile da internet».
    pub attivo: bool,
    /// Il nodo è acceso adesso. Su Android può essere spento con
    /// l'interruttore acceso: l'app è in secondo piano da un po'.
    pub acceso: bool,
    /// La chiave pubblica di questo dispositivo, quando il nodo è acceso.
    pub chiave: Option<String>,
    /// La sua impronta (`rymembra_lontano::collegamento::impronta`), quando
    /// il nodo è acceso: accanto al QR, e mentre si aspetta una conferma,
    /// perché sull'altro schermo la si confronti.
    pub impronta: Option<String>,
    /// Il relay proprio, se ce n'è uno.
    pub relay: Option<String>,
    /// «Solo Wi-Fi».
    pub dati_mobili: DatiMobili,
    /// Perché il nodo non si accende: la chiave del testo (`errors.…`).
    pub errore: Option<String>,
    /// I dispositivi collegati.
    pub dispositivi: Vec<DispositivoIpc>,
    /// Il QR mostrato adesso.
    pub invito: Option<InvitoIpc>,
    /// Chi aspetta un sì o un no per collegarsi.
    pub richieste: Vec<RichiestaIpc>,
}

/// Un dispositivo collegato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispositivoIpc {
    /// La sua chiave: il suo nome nei comandi.
    pub chiave: String,
    /// Come lo si mostra.
    pub nome: String,
    /// `windows`, `android`.
    pub piattaforma: String,
    /// L'id che ha nella sincronia, per unire le voci della pagina.
    pub dispositivo: String,
    /// Risponde adesso.
    pub online: bool,
    /// L'ultima volta che ha risposto, ms dall'epoca.
    pub visto_ms: Option<i64>,
    /// Quando ci si è collegati.
    pub associato_ms: i64,
    /// Dall'altra parte l'hanno dimenticato: si tiene, senza fidarsene.
    pub revocato: bool,
    /// La versione di Rymembra che dice di avere, quando risponde.
    pub versione: Option<String>,
    /// Ha «Ascolta insieme» acceso.
    pub insieme_acceso: bool,
    /// E sta ospitando.
    pub insieme_ospita: bool,
    /// La riga di `insieme_pari` con la sua chiave di «Ascolta insieme», se
    /// c'è: per unire le due voci nella pagina «Dispositivi».
    pub insieme: Option<i64>,
    /// Da dove passa adesso la connessione: `diretto` o `relay`; `None` senza
    /// una connessione viva.
    pub via: Option<String>,
}

/// Il QR: il link, e la matrice da disegnare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitoIpc {
    /// `rymembra://collega/…`, da copiare.
    pub link: String,
    /// Moduli per lato.
    pub lato: usize,
    /// I moduli riga per riga, `1` scuro e `0` chiaro: una stringa e non un
    /// elenco di booleani, che in JSON pesa sei volte tanto.
    pub moduli: String,
    /// Quando non vale più, ms dall'epoca.
    pub scade_ms: i64,
}

/// Un link di collegamento, prima di collegarsi: con chi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkIpc {
    /// Il link, da ripetere a `lontano_collega`.
    pub link: String,
    /// Il nome che chi mostra il QR dichiara, ripulito; `None` se il link non
    /// ne porta uno (una versione di prima).
    pub nome: Option<String>,
    /// L'impronta della sua chiave: la stessa che lui mostra accanto al QR.
    pub impronta: String,
}

/// Qualcuno ha letto il QR e aspetta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichiestaIpc {
    /// Da ripetere in `lontano_rispondi`.
    pub id: u64,
    /// Come si presenta: il nome lo sceglie **lui**, e la finestra lo dice
    /// come dichiarato.
    pub nome: String,
    /// `windows`, `android`.
    pub piattaforma: String,
    /// L'impronta della sua chiave: quel che chi decide confronta con l'altro
    /// schermo, dove il dispositivo la mostra mentre aspetta.
    pub impronta: String,
    /// La chiave di chi aspetta: per togliere la domanda se rinuncia. Non va
    /// alla finestra, che non ne ha bisogno.
    #[serde(skip)]
    pub chiave: String,
}
