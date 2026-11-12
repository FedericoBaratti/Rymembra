//! Il client: due richieste, e la traduzione dei guasti che meritano un nome.
//!
//! # Perché uno solo per quattro fornitori
//!
//! Perché parlano tutti lo stesso dialetto — vedi [`crate::fornitore`] — e un
//! tratto con quattro implementazioni sarebbe quattro file per non scrivere due
//! `if`. Quel che cambia davvero sono l'indirizzo, la chiave e due intestazioni
//! di cortesia; tutto il resto è identico fino al byte.
//!
//! # Cosa si traduce, e cosa si lascia stare
//!
//! Quasi tutto passa da [`rymembra_net::http::Rete::stato_a_errore`], che è la
//! traduzione normale e sa già cosa fare di un 429 o di un 503. Qui si toccano
//! solo i tre casi in cui quella traduzione direbbe una cosa vera e inutile:
//!
//! - un 401 o un 403, che è «la chiave è sbagliata» e non «errore 401»;
//! - un 404 che nomina il modello, che è «quel modello non c'è» e non «errore
//!   404» — perché un 404 su un indirizzo di base sbagliato e un 404 su un nome
//!   di modello sbagliato hanno due rimedi diversi;
//! - una connessione rifiutata verso un servizio **locale**, che non è «non c'è
//!   rete»: è Ollama che non è acceso, e mandare a controllare il router chi
//!   deve premere un bottone in un'altra finestra è la peggiore di tutte.
//!
//! Il limite di frequenza non è in questo elenco di proposito: esce già come
//! `net.rateLimited` con il tempo che il servizio ha chiesto e la ritentabilità
//! giusta, e un secondo codice per la stessa cosa sarebbe la tabella doppia che
//! il catalogo degli errori esiste per non avere.
//!
//! # Cosa non finisce mai in un errore, né in un `Debug`
//!
//! La chiave. Il tipo [`Cliente`] ha un `Debug` scritto a mano proprio per
//! questo: un `derive` la stamperebbe la prima volta che qualcuno mette un
//! `dbg!` per capire perché una richiesta non parte, e da lì finirebbe in un
//! diario che l'utente allega a una segnalazione.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_net::http::{Corpo, Metodo, Rete, Richiesta};
use serde_json::{Value, json};

use crate::fornitore::Fornitore;
use crate::messaggi::{Fine, Messaggio, Motivo, Voce};
use crate::profilo::Profilo;
use crate::sse::{Evento, Sse};

/// Il nome del servizio negli errori.
const SERVIZIO: &str = "ia";

/// Quanto può durare una generazione.
///
/// Dieci minuti, e non è una scadenza pensata per scattare: un modello locale
/// da trenta miliardi di parametri su una macchina senza scheda video scrive
/// più lento di quanto si legga, e una scadenza stretta taglierebbe la risposta
/// a metà frase proprio nei casi in cui è costata di più. Quel che protegge
/// davvero da un servizio fermo è l'annullamento, che risponde entro un
/// [`RESPIRO`] anche quando il servizio non ha ancora mandato niente (vedi
/// [`Cliente::conversa`]); questa è la rete sotto, per il caso in cui nessuno
/// stia guardando, e il tempo massimo per cui una richiesta abbandonata resta
/// aperta.
const SCADENZA_LUNGA: Duration = Duration::from_secs(600);

/// Ogni quanto, mentre il servizio tace, si guarda se qualcuno ha premuto
/// «Ferma».
///
/// Un decimo di secondo: sotto la soglia in cui un bottone sembra non aver
/// risposto, e abbastanza raro da non costare niente a un filo che aspetta.
const RESPIRO: Duration = Duration::from_millis(100);

/// Quanti blocchi possono aspettare nel canale fra il filo della richiesta e
/// chi li legge.
///
/// Un canale con un fondo, e non senza: se chi legge rallenta, il filo smette
/// di leggere dalla rete invece di accumulare in memoria. Trentadue blocchi
/// sono molto più di quanto un modello scriva fra due respiri.
const BLOCCHI_IN_VOLO: usize = 32;

/// Quel che il filo della richiesta passa a [`Cliente::conversa`].
enum Arrivo {
    /// Un blocco di byte, così come è arrivato dalla rete.
    Blocco(Vec<u8>),
    /// Il flusso è finito, con il suo esito.
    Fine(Result<(), AppError>),
}

/// Come una richiesta di [`Cliente::conversa`] è andata male.
enum Guasto {
    /// Il servizio ha rifiutato il nome del tetto dei gettoni, prima di
    /// rispondere: si riprova con l'altro. L'errore è quello della rete, non
    /// ancora tradotto.
    TettoRifiutato(AppError),
    /// Qualunque altra cosa, già tradotta.
    Altro(AppError),
}

/// Il nome del tetto dei gettoni che parlano tutti.
const CAMPO_DEL_TETTO: &str = "max_tokens";

/// Il nome che OpenAI vuole al suo posto per i modelli che ragionano.
const CAMPO_DEL_TETTO_NUOVO: &str = "max_completion_tokens";

/// Il servizio ha rifiutato `campo` come nome del tetto: un 400 che nomina il
/// campo, e che porta l'altro nome come rimedio — è la frase di OpenAI
/// («Unsupported parameter: 'max_tokens' … Use 'max_completion_tokens'
/// instead»). Si guarda la causa, che porta il corpo della risposta.
fn tetto_rifiutato(err: &AppError, campo: &str) -> bool {
    let ErrorCode::NetHttp { status: 400, .. } = err.code() else {
        return false;
    };
    let causa = err.cause().unwrap_or_default();
    causa.contains(campo) && causa.contains(CAMPO_DEL_TETTO_NUOVO)
}

/// Quanto si aspetta per l'elenco dei modelli.
///
/// Quindici secondi. È una richiesta che serve a **sapere se il servizio c'è**:
/// se non risponde in quindici secondi la risposta utile è già «non c'è», e
/// farla aspettare dieci minuti dietro la stessa scadenza della generazione
/// renderebbe il bottone «Prova» indistinguibile da un blocco.
const SCADENZA_CORTA: Duration = Duration::from_secs(15);

/// Quanto di un corpo di errore si tiene per raccontarlo.
const DETTAGLIO_MASSIMO: usize = 300;

/// Quanto testo — risposta e ragionamento insieme — una risposta può
/// consegnare (D-64).
///
/// Quattro mebibyte: più di quel che il tetto dei gettoni lascia scrivere a
/// qualunque modello vero, e quindi un tetto che scatta solo davanti a un
/// servizio rotto od ostile. Senza, la finestra riceveva e ricomponeva una
/// stringa che cresceva per dieci minuti. Arrivati qui la risposta si ferma
/// come [`Motivo::Tagliato`]: quel che c'è resta leggibile, e un blocco di
/// modifiche a metà non si applica.
const TESTO_MASSIMO: usize = 4 * 1024 * 1024;

/// Quanti byte si leggono dalla rete per una risposta, battiti e righe che non
/// si capiscono compresi.
///
/// Sessantaquattro mebibyte. Un evento porta qualche carattere dentro un
/// centinaio di byte di JSON, quindi il conto grezzo cresce molto più del
/// testo: duecentomila gettoni ne fanno una trentina. Un servizio che manda
/// solo `: keep-alive` o righe che non sono eventi non consegna testo, e
/// [`TESTO_MASSIMO`] non lo fermerebbe mai.
const GREZZO_MASSIMO: usize = 64 * 1024 * 1024;

/// Quanto può tacere un servizio **dopo** aver cominciato a rispondere.
///
/// Due minuti. Prima del primo byte vale solo [`SCADENZA_LUNGA`]: un modello
/// locale che valuta un prompt lungo su CPU può metterci minuti, e non manda
/// niente mentre lo fa. Dopo, un modello che scrive manda qualcosa di
/// continuo, e i servizi remoti mandano un `: keep-alive` ogni pochi secondi:
/// due minuti senza un byte sono una connessione che non porterà più niente,
/// e aspettare gli otto minuti che restano della scadenza voleva dire una chat
/// occupata per niente.
const SILENZIO_MASSIMO: Duration = Duration::from_secs(120);

/// Quanto si tiene di una risposta che non è un flusso.
///
/// Un megabyte, come l'avanzo del decodificatore: una risposta intera da
/// sessantacinquemila gettoni ci sta dentro abbondantemente, e una pagina di
/// errore pure. Oltre, non è né l'una né l'altra.
const CRUDO_MASSIMO: usize = 1024 * 1024;

/// Quanto costa un modello, per quel che il servizio ne dichiara.
///
/// Tre valori e non un `bool`, perché «non lo so» e «si paga» non sono la stessa
/// cosa: Ollama e LM Studio non mandano nessun prezzo — girano su questa
/// macchina, il prezzo è la corrente — e marcarli «a pagamento» sarebbe una
/// bugia scritta accanto a ogni riga del menù.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Prezzo {
    /// Il servizio non dichiara prezzi.
    Sconosciuto,
    /// Dichiara zero da tutte e due le parti.
    Gratis,
    /// Dichiara un prezzo.
    APagamento,
}

/// Un modello, come lo dichiara chi lo serve.
///
/// # Perché non basta più l'identificativo
///
/// Perché con OpenRouter l'elenco è di centinaia di righe e lo slug da solo non
/// dice niente di quel che serve a sceglierne una: quanto costa, e quanto
/// contesto regge. Chi lo scriveva a mano lo scopriva dopo, da un 404 che dice
/// «This model is unavailable for free» — cioè dal posto peggiore.
///
/// Restano fuori le altre venti chiavi che OpenRouter manda: qui c'è quel che
/// una riga di menù mostra, e non una copia del loro catalogo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Modello {
    /// Lo slug, che è quel che va scritto nel profilo.
    pub id: String,
    /// Il nome leggibile, quando il servizio ne dichiara uno diverso dall'id.
    pub nome: Option<String>,
    /// Quanti gettoni di contesto, quando lo dichiara.
    pub contesto: Option<u32>,
    /// Quanto costa.
    pub prezzo: Prezzo,
    /// Legge le immagini, quando il servizio lo dichiara.
    ///
    /// `None` non è `false`: Ollama e LM Studio non dicono niente delle
    /// modalità, e un modello locale con la visione esiste. Chi mostra la
    /// scelta lascia decidere a chi usa il modello, invece di negargliela.
    pub vede: Option<bool>,
}

/// Un modello raggiungibile.
pub struct Cliente {
    /// Per la generazione: scadenza lunga.
    rete: Rete,
    /// Per l'elenco dei modelli: scadenza corta.
    rete_corta: Rete,
    profilo: Profilo,
    /// Il segreto, per il tempo di una richiesta.
    chiave: Option<String>,
    /// Quanto può tacere il servizio a risposta cominciata: vedi
    /// [`SILENZIO_MASSIMO`]. Un campo e non la costante, perché le prove lo
    /// accorciano.
    silenzio: Duration,
}

/// Scritto a mano: un `derive` stamperebbe la chiave.
impl std::fmt::Debug for Cliente {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cliente")
            .field("profilo", &self.profilo.id)
            .field("modello", &self.profilo.modello)
            .field("url_base", &self.profilo.url_base)
            .field("chiave", &self.chiave.as_ref().map(|_| "‹presente›"))
            .finish()
    }
}

impl Cliente {
    /// Un client per questo profilo.
    ///
    /// La chiave si passa qui e non si tiene da nessun'altra parte: chi chiama
    /// la prende dal portachiavi un istante prima, e questo oggetto vive quanto
    /// una conversazione.
    ///
    /// # La chiave parte solo verso l'origine per cui è stata scritta
    ///
    /// Chi salva un profilo la toglie già quando l'indirizzo cambia origine
    /// (vedi [`Profilo::chiave_vale_per`]), ma il profilo sta in `settings`, e
    /// `settings` è un file che si può aprire con altri strumenti. Il controllo
    /// si rifà qui, sull'ultimo metro prima della rete: una chiave che arriva
    /// insieme a un indirizzo che non è il suo non costruisce un client.
    ///
    /// # Nessun rimando
    ///
    /// Le due reti vengono da [`Rete::verso`], che non ne segue nessuno: la
    /// chiave e la conversazione vanno all'indirizzo del profilo e basta. Un
    /// 3xx diventa un errore che dice dove il servizio voleva mandarle.
    ///
    /// # Errori
    ///
    /// `ia.profileIncomplete` se manca l'indirizzo o il modello,
    /// `ia.keyMissing` se manca la chiave di un fornitore che la vuole,
    /// `ia.keyForOtherOrigin` se la chiave è stata scritta per un'altra
    /// origine: tre codici e non uno, perché «nessun modello è configurato»
    /// era falso per tutti e tre (R10-6). `ia.notLoopback` per un indirizzo in
    /// chiaro che non è questa macchina.
    pub fn nuovo(profilo: &Profilo, chiave: Option<String>) -> Result<Self, AppError> {
        // Il dettaglio va nella causa, cioè nel diario: la frase la dà il
        // codice, nella lingua di chi legge.
        let manca = |codice: ErrorCode, cosa: &str| {
            Err(AppError::new(codice)
                .with_cause(format!("il profilo «{}» non ha {cosa}", profilo.id)))
        };
        if profilo.url_base.trim().is_empty() {
            return manca(ErrorCode::IaProfileIncomplete, "un indirizzo");
        }
        if profilo.modello.trim().is_empty() {
            return manca(ErrorCode::IaProfileIncomplete, "un modello");
        }
        let chiave = chiave.filter(|k| !k.trim().is_empty());
        if profilo.fornitore.vuole_chiave() && chiave.is_none() {
            return manca(ErrorCode::IaKeyMissing, "una chiave");
        }
        if chiave.is_some() && !profilo.chiave_vale_per(&profilo.url_base) {
            // Si nomina l'origine per cui era scritta, non la chiave: è quel che
            // serve a capire perché, e non è un segreto.
            return Err(AppError::new(ErrorCode::IaKeyForOtherOrigin).with_cause(format!(
                "la chiave del profilo «{}» è stata scritta per {}, non per l'indirizzo di adesso: \
                 va scritta di nuovo",
                profilo.id,
                profilo.origine_chiave.as_deref().unwrap_or("un altro indirizzo")
            )));
        }
        crate::profilo::controlla_indirizzo(&profilo.url_base)?;

        Ok(Self {
            rete: Rete::verso(SERVIZIO, SCADENZA_LUNGA, &profilo.url_base)?,
            rete_corta: Rete::verso(SERVIZIO, SCADENZA_CORTA, &profilo.url_base)?,
            profilo: profilo.clone(),
            chiave,
            silenzio: SILENZIO_MASSIMO,
        })
    }

    /// I modelli che questo servizio offre.
    ///
    /// # Perché è anche la prova di connessione
    ///
    /// Perché un servizio che risponde a questa risponde a tutto: l'indirizzo è
    /// giusto, il processo è acceso, la chiave passa. Un bottone «prova» che
    /// facesse una richiesta finta oltre a questa proverebbe una cosa in meno e
    /// costerebbe una richiesta in più.
    ///
    /// # L'ordine è quello con cui si sceglie
    ///
    /// Prima i gratuiti, poi gli altri, e dentro ogni gruppo per nome. Con
    /// OpenRouter l'elenco è di centinaia di righe, e l'ordine in cui arriva —
    /// il loro — non è un ordine per chi sta scegliendo.
    ///
    /// # Errori
    ///
    /// `ia.unauthorized`, `ia.localServerDown`, `ia.badResponse` se la risposta
    /// non ha la forma dichiarata, e i `net.*` di sempre.
    pub fn modelli(&self) -> Result<Vec<Modello>, AppError> {
        let url = self.profilo.punto("/models");
        let intestazioni = self.intestazioni();
        let riferimenti: Vec<(&str, &str)> =
            intestazioni.iter().map(|(n, v)| (*n, v.as_str())).collect();

        let risposta = self
            .rete_corta
            .esegui(Richiesta {
                metodo: Metodo::Get,
                url: &url,
                intestazioni: &riferimenti,
                corpo: Corpo::Niente,
            })
            .map_err(|err| self.traduci(err))?;
        if !risposta.e_andata() {
            return Err(self.traduci(self.rete_corta.stato_a_errore(&risposta, &url)));
        }

        let documento: Value = serde_json::from_slice(&risposta.corpo)
            .map_err(|err| self.forma_sbagliata(&format!("l'elenco dei modelli: {err}")))?;
        // `data[].id` è la forma di OpenAI, e la copiano tutti. Quel che non ha
        // un `id` testuale si salta invece di far fallire l'elenco: un servizio
        // che aggiunge una riga strana non deve rendere inservibile il menù.
        let mut modelli: Vec<Modello> = documento
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| self.forma_sbagliata("l'elenco dei modelli non ha un campo «data»"))?
            .iter()
            .filter_map(voce_di_modello)
            .collect();
        modelli.sort_by(|a, b| {
            ordine_prezzo(a.prezzo)
                .cmp(&ordine_prezzo(b.prezzo))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(modelli)
    }

    /// Manda la conversazione e consegna la risposta mentre arriva.
    ///
    /// `pezzo` riceve il testo nuovo, non il testo intero: accumularlo è
    /// compito di chi lo mostra, e passare ogni volta tutto quel che è arrivato
    /// finora renderebbe quadratico il costo di una risposta lunga. Insieme al
    /// testo riceve la [`Voce`] da cui arriva: chi lo mostra ha bisogno di
    /// sapere se sta guardando la risposta o il ragionamento, perché una delle
    /// due si interpreta e l'altra no.
    ///
    /// # L'annullamento è una domanda
    ///
    /// Come in tutto il resto dell'albero: una chiusura interrogata spesso. Un
    /// annullamento non è un guasto e non esce come `Err`: esce come
    /// [`Motivo::Fermato`], perché chi ha premuto «Ferma» sa già cos'è successo
    /// e non ha bisogno di leggerlo in rosso.
    ///
    /// «Spesso» vuol dire anche mentre il servizio **tace**. La richiesta parte
    /// da un filo suo ([`Self::avvia_flusso`]) che passa i blocchi su un canale,
    /// e qui si aspetta a respiri di [`RESPIRO`], guardando la chiusura a ogni
    /// respiro. Prima la si guardava solo fra un blocco e l'altro, e un servizio
    /// che non manda il primo byte — un modello locale che si carica, un prompt
    /// lungo valutato su CPU, un server bloccato — teneva la chat occupata fino
    /// al primo byte o ai dieci minuti di [`SCADENZA_LUNGA`]: «Ferma» non
    /// fermava niente, e una domanda nuova riceveva `ia.busy`.
    ///
    /// Un limite da sapere: la richiesta abbandonata resta aperta finché il
    /// servizio non manda il primo byte (o finché non scade), e poi il filo
    /// smette da sé. Un server locale con un posto solo — Ollama, col
    /// parallelismo di serie — continua intanto a lavorare su quella, e la
    /// domanda dopo gli aspetta dietro: «Ferma» libera la chat, non il modello.
    ///
    /// # Ogni risposta ha una misura
    ///
    /// Tre tetti, perché fino alla 3.2.0 l'unico limite era la scadenza di
    /// dieci minuti (R08-3):
    ///
    /// - la richiesta dice quanti gettoni il modello può scrivere
    ///   ([`crate::GETTONI_DI_SERIE`], o quel che chiede il profilo): è quel che
    ///   decide il costo, e con OpenRouter anche **se** la richiesta parte;
    /// - il testo consegnato si ferma a [`TESTO_MASSIMO`], e i byte letti a
    ///   [`GREZZO_MASSIMO`]: un servizio rotto od ostile che scrive senza fine
    ///   non riempie la finestra, e la risposta esce come
    ///   [`Motivo::Tagliato`];
    /// - a risposta cominciata, un silenzio oltre [`SILENZIO_MASSIMO`] è una
    ///   scadenza (`net.timeout`), e non dieci minuti di chat occupata.
    ///
    /// # Errori
    ///
    /// `ia.unauthorized`, `ia.modelUnknown`, `ia.localServerDown`,
    /// `ia.noAnswer`, `ia.badResponse`, i `net.*` di sempre, e
    /// `internal.unexpected` se il filo della richiesta non parte.
    pub fn conversa(
        &self,
        messaggi: &[Messaggio],
        annullato: &dyn Fn() -> bool,
        pezzo: &mut dyn FnMut(Voce, &str),
    ) -> Result<Fine, AppError> {
        // `max_tokens` è il nome che parlano tutti: OpenRouter, Ollama, LM
        // Studio. OpenAI lo rifiuta coi modelli che ragionano, con un 400 che
        // chiede `max_completion_tokens`; un profilo personalizzato verso di
        // loro funzionava prima che il tetto ci fosse, e deve continuare. Il
        // rifiuto arriva prima di un solo byte di risposta, e non si paga:
        // ripetere la richiesta con l'altro nome costa un giro e basta.
        let mut campo = CAMPO_DEL_TETTO;
        loop {
            match self.ascolta(messaggi, campo, annullato, pezzo) {
                Ok(fine) => return Ok(fine),
                Err(Guasto::TettoRifiutato(err)) => {
                    if campo == CAMPO_DEL_TETTO_NUOVO {
                        return Err(self.traduci(err));
                    }
                    campo = CAMPO_DEL_TETTO_NUOVO;
                }
                Err(Guasto::Altro(err)) => return Err(err),
            }
        }
    }

    /// Il corpo della richiesta, col tetto dei gettoni sotto il nome `campo`.
    fn richiesta(&self, messaggi: &[Messaggio], campo: &str) -> Result<Vec<u8>, AppError> {
        let mut richiesta = json!({
            "model": self.profilo.modello,
            "messages": messaggi
                .iter()
                .map(|m| json!({ "role": m.ruolo.nome(), "content": m.contenuto() }))
                .collect::<Vec<_>>(),
            "stream": true,
        });
        if let Some(oggetto) = richiesta.as_object_mut() {
            oggetto.insert(campo.to_owned(), json!(self.profilo.gettoni()));
        }
        // I gettoni, in streaming, li manda solo chi li chiede. OpenRouter ha
        // la sua forma; gli altri quella del protocollo, `stream_options`, che
        // Ollama e LM Studio documentano. Senza, il conto resta `None` — e la
        // chat dello Studio non può dire quanto è costata una conversazione.
        if let Some(oggetto) = richiesta.as_object_mut() {
            if self.profilo.fornitore == Fornitore::OpenRouter {
                oggetto.insert("usage".to_owned(), json!({ "include": true }));
            } else {
                oggetto.insert(
                    "stream_options".to_owned(),
                    json!({ "include_usage": true }),
                );
            }
        }
        serde_json::to_vec(&richiesta)
            .map_err(|err| self.forma_sbagliata(&format!("la richiesta: {err}")))
    }

    /// Una richiesta di [`Self::conversa`], col tetto dei gettoni chiamato
    /// `campo`, e la sua risposta fino in fondo.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::conversa`], già tradotti in [`Guasto::Altro`];
    /// [`Guasto::TettoRifiutato`], non tradotto, se il servizio rifiuta il
    /// nome del tetto prima di rispondere.
    fn ascolta(
        &self,
        messaggi: &[Messaggio],
        campo: &str,
        annullato: &dyn Fn() -> bool,
        pezzo: &mut dyn FnMut(Voce, &str),
    ) -> Result<Fine, Guasto> {
        let url = self.profilo.punto("/chat/completions");
        let intestazioni = self.intestazioni();
        let corpo = self.richiesta(messaggi, campo).map_err(Guasto::Altro)?;

        let mut sse = Sse::nuovo();
        let mut fine = Fine::per(Motivo::Finito);
        let mut visto_qualcosa = false;
        // Il testo consegnato finora, e se il tetto l'ha fermato: vedi
        // [`TESTO_MASSIMO`].
        let mut consegnati = 0_usize;
        let tagliato = std::cell::Cell::new(false);
        // I byte letti dalla rete finora: vedi [`GREZZO_MASSIMO`].
        let mut grezzi = 0_usize;
        // I byte com'erano, finché il flusso non ha detto niente.
        //
        // Serve a due cose che l'avanzo del decodificatore non può fare. La
        // prima: un servizio che ignora `stream: true` e risponde con la
        // risposta intera — succede, e la risposta c'è: buttarla vorrebbe dire
        // dare un errore su una richiesta andata bene, e già pagata. La
        // seconda: raccontare cosa è arrivato quando non era un flusso, anche
        // se andava a capo — l'avanzo a quel punto è già stato consumato riga
        // per riga, e resterebbe vuoto proprio nel caso in cui serve.
        //
        // Smette di crescere appena il primo evento arriva: su una risposta
        // vera questo `Vec` resta di zero byte.
        let mut crudo: Vec<u8> = Vec::new();

        // Quel che arriva a `pezzo`, dentro il tetto del testo. L'ultimo pezzo
        // si taglia al tetto, su un confine di carattere; da lì niente più.
        let mut consegna = |voce: Voce, testo: &str| {
            if tagliato.get() {
                return;
            }
            let resta = TESTO_MASSIMO.saturating_sub(consegnati);
            if testo.len() <= resta {
                consegnati = consegnati.saturating_add(testo.len());
                pezzo(voce, testo);
                return;
            }
            let mut taglio = resta;
            while taglio > 0 && !testo.is_char_boundary(taglio) {
                taglio -= 1;
            }
            if let Some(entra) = testo.get(..taglio).filter(|t| !t.is_empty()) {
                pezzo(voce, entra);
            }
            consegnati = TESTO_MASSIMO;
            tagliato.set(true);
        };

        // `Ok(true)` se si va avanti, `Ok(false)` se un tetto ha detto basta.
        let mut mangia = |blocco: &[u8]| -> Result<bool, AppError> {
            grezzi = grezzi.saturating_add(blocco.len());
            if !visto_qualcosa && crudo.len() < CRUDO_MASSIMO {
                crudo.extend_from_slice(blocco);
            }
            let eventi = sse.mangia(blocco);
            // Il decodificatore ha rinunciato: una riga oltre il mebibyte non è
            // un evento, è una pagina o un file arrivati con lo stato giusto.
            // Da qui non uscirebbe più niente, e aspettare la fine del corpo
            // voleva dire leggere la rete per dieci minuti buttando tutto
            // (R08-5).
            if sse.sfondato() {
                return Err(self.forma_sbagliata(&format!(
                    "una riga di più di un mebibyte, non un flusso di eventi: {}",
                    accorcia(&String::from_utf8_lossy(&crudo), DETTAGLIO_MASSIMO)
                )));
            }
            for evento in eventi {
                // Anche un `[DONE]` senza contenuto conta: una risposta
                // vuota è una risposta, e trattarla come «non era un
                // flusso» direbbe che il servizio è rotto quando ha solo
                // taciuto.
                visto_qualcosa = true;
                let Evento::Dati(carico) = evento else {
                    continue;
                };
                // Un evento che non si capisce si salta: il flusso continua,
                // e un servizio che infila una riga di suo non deve poter
                // buttare via la risposta che sta arrivando.
                let Ok(documento) = serde_json::from_str::<Value>(&carico) else {
                    continue;
                };
                // Un `error` dentro un flusso già iniziato: OpenRouter lo fa
                // quando il modello a valle cade a metà generazione.
                if let Some(guasto) = documento.get("error") {
                    return Err(self.dal_documento(guasto));
                }
                leggi_evento(&documento, &mut fine, &mut consegna);
                if tagliato.get() {
                    return Ok(false);
                }
            }
            // Tanti byte e poco testo: battiti, righe che non sono eventi. Se
            // del testo è arrivato, la risposta è quella, tagliata; se non è
            // arrivato niente, dall'altra parte non c'era un flusso.
            if grezzi > GREZZO_MASSIMO {
                if !visto_qualcosa {
                    return Err(self.forma_sbagliata(&format!(
                        "più di {} MiB senza un evento",
                        GREZZO_MASSIMO >> 20
                    )));
                }
                tagliato.set(true);
                return Ok(false);
            }
            Ok(true)
        };

        // Il flusso gira su un filo suo, e qui si aspetta a respiri brevi: vedi
        // «L'annullamento è una domanda» sopra. Il ciclo esce con l'esito del
        // flusso, o con `None` se chi guarda ha smesso di volerlo.
        let (manda, ricevi) = mpsc::sync_channel(BLOCCHI_IN_VOLO);
        let smesso = Arc::new(AtomicBool::new(false));
        let indirizzo = url.clone();
        self.avvia_flusso(url, intestazioni, corpo, manda, Arc::clone(&smesso))
            .map_err(Guasto::Altro)?;
        // Quando è arrivato l'ultimo blocco; `None` finché non ne è arrivato
        // nessuno. Vedi [`SILENZIO_MASSIMO`].
        let mut ultimo: Option<std::time::Instant> = None;
        let esito = loop {
            if annullato() {
                break None;
            }
            match ricevi.recv_timeout(RESPIRO) {
                Ok(Arrivo::Blocco(blocco)) => {
                    ultimo = Some(std::time::Instant::now());
                    match mangia(&blocco) {
                        Ok(true) => {}
                        Ok(false) => break Some(Ok(())),
                        Err(err) => break Some(Err(err)),
                    }
                }
                Ok(Arrivo::Fine(esito)) => break Some(esito),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if ultimo.is_some_and(|quando| quando.elapsed() >= self.silenzio) {
                        break Some(Err(AppError::new(ErrorCode::NetTimeout {
                            url: Some(indirizzo),
                            timeout_ms: u64::try_from(self.silenzio.as_millis()).ok(),
                        })
                        .with_cause(format!(
                            "a risposta cominciata, nessun byte da {} s",
                            self.silenzio.as_secs()
                        ))));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Some(Err(AppError::new(ErrorCode::InternalUnexpected {
                        detail: Some("il filo del flusso è finito senza dire come".to_owned()),
                    })));
                }
            }
        };
        // Il filo, se c'è ancora, smette al primo blocco che arriva: la bandiera
        // glielo dice fra un blocco e l'altro, e il canale chiuso — `ricevi`
        // cade all'uscita di qui — fa fallire la consegna di quel che resta.
        smesso.store(true, Ordering::Relaxed);

        match esito {
            Some(Ok(())) => {}
            // L'annullamento: chi ha premuto «Ferma» non ha causato un guasto,
            // e la risposta a metà che ha già letto resta valida. Dal flusso
            // arriverebbe come `internal.aborted`, che va riconosciuto
            // **prima** di tradurre.
            None => {
                return Ok(Fine {
                    motivo: Motivo::Fermato,
                    ..fine
                });
            }
            Some(Err(err)) if matches!(err.code(), ErrorCode::InternalAborted { .. }) => {
                return Ok(Fine {
                    motivo: Motivo::Fermato,
                    ..fine
                });
            }
            Some(Err(err)) if !visto_qualcosa && tetto_rifiutato(&err, campo) => {
                return Err(Guasto::TettoRifiutato(err));
            }
            Some(Err(err)) => return Err(Guasto::Altro(self.traduci(err))),
        }
        // Un tetto ha detto basta: quel che è arrivato resta, e dice perché si
        // è fermato. Un blocco di modifiche a metà non si applica.
        if tagliato.get() {
            return Ok(Fine {
                motivo: Motivo::Tagliato,
                ..fine
            });
        }

        self.senza_un_flusso(visto_qualcosa, &sse, &crudo, fine, pezzo)
            .map_err(Guasto::Altro)
    }

    /// Com'è finita una risposta arrivata in fondo, o perché non era una
    /// risposta: il seguito di [`Self::ascolta`].
    fn senza_un_flusso(
        &self,
        visto_qualcosa: bool,
        sse: &Sse,
        crudo: &[u8],
        mut fine: Fine,
        pezzo: &mut dyn FnMut(Voce, &str),
    ) -> Result<Fine, AppError> {
        if !visto_qualcosa {
            // Prima di dare la colpa a qualcuno: forse la risposta c'è, e non è
            // un flusso. Un servizio che ignora `stream: true` risponde con il
            // documento intero, e dentro c'è `choices[0].message.content` invece
            // di una sequenza di `delta`. È la stessa risposta, arrivata tutta
            // in una volta: leggerla costa dieci righe, e l'alternativa è
            // rifiutare qualcosa che ha funzionato.
            if let Ok(documento) = serde_json::from_slice::<Value>(crudo) {
                if let Some(guasto) = documento.get("error") {
                    return Err(self.dal_documento(guasto));
                }
                if let Some(fatto) = leggi_intero(&documento, pezzo) {
                    return Ok(fatto);
                }
            }

            // Stato 2xx e nessun dato: due guasti diversi che si somigliano.
            //
            // Se non è passata nemmeno una riga, dall'altra parte non c'era un
            // flusso — un indirizzo di base che punta a una pagina web — e quel
            // che è arrivato è l'informazione utile, perché contiene la pagina.
            //
            // Se invece le righe sono passate, il protocollo era giusto e il
            // servizio ha solo taciuto: succede sui modelli gratuiti quando la
            // coda a valle scade dopo un po' di `: keep-alive`. Dirgli «non era
            // un flusso» manderebbe a controllare l'indirizzo, che è l'unica
            // cosa che va bene.
            if sse.righe() == 0 {
                return Err(self.forma_sbagliata(&format!(
                    "la risposta non era un flusso di eventi: {}",
                    accorcia(&String::from_utf8_lossy(crudo), DETTAGLIO_MASSIMO)
                )));
            }
            return Err(AppError::new(ErrorCode::IaNoAnswer)
                .with_message(format!("da «{}»", self.profilo.id))
                .with_cause(format!(
                    "flusso ben formato, {} righe, nessun dato: modello «{}»",
                    sse.righe(),
                    self.profilo.modello
                )));
        }
        if !sse.finito() && fine.motivo == Motivo::Finito {
            fine.motivo = Motivo::Troncato;
        }
        Ok(fine)
    }

    /// Fa partire la richiesta di [`Self::conversa`] su un filo suo.
    ///
    /// # Perché un filo staccato, e cosa si porta dietro
    ///
    /// Staccato e non dentro uno `std::thread::scope`: lo scope aspetterebbe la
    /// fine del filo prima di restituire, e si tornerebbe a un «Ferma» che
    /// aspetta il servizio. Quindi il filo **possiede** tutto quel che usa — un
    /// clone della rete (costa un `Arc`), l'indirizzo, le intestazioni con la
    /// chiave, il corpo — e la chiusura di annullamento non è quella di chi
    /// chiama, che non è `Send`: è `smesso`, che chi chiama alza quando smette
    /// di aspettare. La chiave non esce dal filo e non finisce in nessun
    /// `Debug`: [`Arrivo`] porta solo byte e l'esito.
    ///
    /// Il filo finisce da sé: al primo blocco dopo che `smesso` è alto, o al
    /// primo invio su un canale che nessuno legge più, o alla scadenza della
    /// rete.
    ///
    /// # Errori
    ///
    /// `internal.unexpected` se il sistema non concede il filo.
    fn avvia_flusso(
        &self,
        url: String,
        intestazioni: Vec<(&'static str, String)>,
        corpo: Vec<u8>,
        manda: mpsc::SyncSender<Arrivo>,
        smesso: Arc<AtomicBool>,
    ) -> Result<(), AppError> {
        let rete = self.rete.clone();
        std::thread::Builder::new()
            .name("rymembra-ia-flusso".to_owned())
            .spawn(move || {
                let riferimenti: Vec<(&str, &str)> =
                    intestazioni.iter().map(|(n, v)| (*n, v.as_str())).collect();
                let esito = rete.flusso(
                    &url,
                    &riferimenti,
                    Corpo::Byte {
                        tipo: "application/json",
                        dati: &corpo,
                    },
                    &|| smesso.load(Ordering::Relaxed),
                    &mut |blocco: &[u8]| {
                        manda.send(Arrivo::Blocco(blocco.to_vec())).map_err(|_| {
                            AppError::new(ErrorCode::InternalAborted {
                                what: Some("flusso".to_owned()),
                            })
                        })
                    },
                );
                // Se nessuno ascolta più, l'esito non serve a nessuno.
                let _ = manda.send(Arrivo::Fine(esito));
            })
            .map(|_| ())
            .map_err(|err| {
                AppError::new(ErrorCode::InternalUnexpected {
                    detail: Some(format!("il filo della richiesta non parte: {err}")),
                })
            })
    }

    /// Le intestazioni di questa richiesta, chiave compresa.
    ///
    /// Le due di OpenRouter sono facoltative e servono a loro per dire da quale
    /// applicazione arriva una richiesta. Si mandano perché è la stessa
    /// cortesia dello `User-Agent` — un servizio pubblico che vede un nome può
    /// scrivere a qualcuno invece di limitarsi a bloccare — e perché non
    /// contengono niente dell'utente: il nome del programma e il suo indirizzo,
    /// che sono pubblici.
    fn intestazioni(&self) -> Vec<(&'static str, String)> {
        let mut fuori = Vec::with_capacity(3);
        if let Some(chiave) = &self.chiave {
            fuori.push(("authorization", format!("Bearer {chiave}")));
        }
        if self.profilo.fornitore == Fornitore::OpenRouter {
            fuori.push((
                "http-referer",
                "https://github.com/FedericoBaratti/Rymembra".to_owned(),
            ));
            fuori.push(("x-title", "Rymembra".to_owned()));
        }
        fuori
    }

    /// Il codice giusto per un guasto che ne merita uno suo, e le parole che il
    /// servizio ci ha messo dentro.
    ///
    /// Vedi il preambolo per quali casi si toccano. Quel che è cambiato è che
    /// **niente** perde più la frase del fornitore: anche i codici che passano
    /// com'erano la portano via in `message`, perché è l'unico campo che
    /// attraversa l'IPC e finisce sotto gli occhi di chi legge. Senza,
    /// «l'elenco è cambiato, usa questo slug» restava scritto in un `cause` che
    /// va nel diario.
    fn traduci(&self, err: AppError) -> AppError {
        // Due volte, e non per scrupolo: `tradotto` accorcia la frase del
        // servizio, e una chiave che cade sul taglio non si riconosce più per
        // intero. Ripulire prima vuol dire tagliare un testo che non la porta.
        self.senza_segreti(self.tradotto(self.senza_segreti(err)))
    }

    /// Un errore con il messaggio e la causa ripuliti dai segreti.
    ///
    /// # Perché qui, e su tutti e due i campi
    ///
    /// Le parole che un servizio scrive arrivano in `message`, che va a schermo,
    /// e in `cause`, che va nel diario — e il diario l'utente lo allega alle
    /// segnalazioni. Un proxy compatibile con OpenAI che risponde 401 con
    /// «Invalid API key: sk-or-v1-…» ci metteva la chiave, intera o mascherata
    /// a metà. Questo oggetto è l'unico che la conosce, quindi è il posto in cui
    /// toglierla: ogni errore che esce dalla rete passa da [`Self::traduci`] o
    /// da [`Self::forma_sbagliata`], e tutti e due passano di qui.
    fn senza_segreti(&self, err: AppError) -> AppError {
        let pulisci = |testo: &str| senza_segreti(testo, self.chiave.as_deref());
        let messaggio = err.message().map(pulisci);
        let causa = err.cause().map(pulisci);
        let mut pulito = err;
        if let Some(messaggio) = messaggio {
            pulito = pulito.with_message(messaggio);
        }
        if let Some(causa) = causa {
            pulito = pulito.with_cause(causa);
        }
        pulito
    }

    /// Quel che fa [`Self::traduci`], prima di togliere i segreti.
    fn tradotto(&self, err: AppError) -> AppError {
        let causa = err.cause().unwrap_or_default().to_owned();
        let detto = messaggio_del_servizio(&causa);
        // Il ripiego sul corpo grezzo tiene in piedi i servizi che non
        // rispondono JSON: Ollama scrive `{"error":"model 'x' not found"}`, ma
        // un proxy davanti può rispondere testo.
        let parla_di_modello = detto.as_deref().is_some_and(parla_di_modello)
            || causa.to_lowercase().contains("model");

        // Un rimando che questo client non segue (vedi `Rete::verso`). Il
        // corpo di un 3xx non dice niente di utile; la causa che
        // `stato_a_errore` ha scritto sì — dove il servizio voleva mandare la
        // richiesta — ed è la frase da mostrare, perché quel che va corretto è
        // l'indirizzo di base che l'utente ha scritto nel profilo.
        if let ErrorCode::NetHttp { status, .. } = err.code()
            && (300..400).contains(status)
        {
            return err.with_message(causa);
        }

        let nuovo = match err.code() {
            ErrorCode::NetHttp {
                status: 401 | 403, ..
            } => AppError::new(ErrorCode::IaUnauthorized {
                provider: self.profilo.fornitore.chiave().to_owned(),
            }),
            // Un 404 o un 400 che parlano di modelli. Un 404 secco è un
            // indirizzo di base sbagliato, e dirgli «quel modello non esiste»
            // manderebbe a cambiare la cosa giusta nel posto sbagliato; un 400
            // che nomina lo slug invece è esattamente questo caso, e prima
            // usciva come «il server ha risposto con un errore».
            ErrorCode::NetHttp {
                status: 400 | 404, ..
            } if parla_di_modello => AppError::new(ErrorCode::IaModelUnknown {
                model: self.profilo.modello.clone(),
            }),
            ErrorCode::NetOffline { .. } | ErrorCode::NetHostUnknown { .. }
                if self.profilo.fornitore.locale() =>
            {
                AppError::new(ErrorCode::IaLocalServerDown {
                    url: self.profilo.url_base.clone(),
                })
            }
            // Tutto il resto tiene il suo codice: quel che si aggiunge sotto è
            // soltanto la frase.
            _ => {
                return match detto {
                    None => err,
                    Some(frase) => err.with_message(frase),
                };
            }
        }
        .with_cause(err.to_string());

        match detto {
            None => nuovo,
            Some(frase) => nuovo.with_message(frase),
        }
    }

    /// Un `ia.badResponse` con il suo dettaglio.
    ///
    /// Il dettaglio va **due volte**: nel codice, dove appartiene, e nella
    /// causa, che è l'unica delle due che arriva da qualche parte. I campi di un
    /// [`ErrorCode`] non attraversano l'IPC — `ErroreIpc` porta codice,
    /// dominio, gravità, chiave, messaggio e causa — e il diario scrive il
    /// codice e la causa. Senza questa riga la frase che il servizio ha scritto
    /// per spiegare cosa non andava si ferma dentro un campo che nessuno legge,
    /// e nel diario resta `causa=—`: cioè il guasto perfettamente registrato e
    /// perfettamente muto.
    ///
    /// I segreti si tolgono **prima** di accorciare: una chiave tagliata a metà
    /// dal tetto resterebbe una mezza chiave, e la regola che la riconosce non
    /// la riconoscerebbe più.
    fn forma_sbagliata(&self, dettaglio: &str) -> AppError {
        let pulito = senza_segreti(dettaglio, self.chiave.as_deref());
        let corto = accorcia(&pulito, DETTAGLIO_MASSIMO);
        AppError::new(ErrorCode::IaBadResponse {
            detail: Some(corto.clone()),
        })
        .with_message(format!("da «{}»", self.profilo.id))
        .with_cause(corto)
    }

    /// L'errore che il servizio ha scritto dentro il flusso.
    fn dal_documento(&self, guasto: &Value) -> AppError {
        let messaggio = guasto
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("il servizio ha interrotto la generazione");
        self.forma_sbagliata(messaggio)
    }
}

/// In che ordine i prezzi si mostrano: prima quel che non costa.
const fn ordine_prezzo(prezzo: Prezzo) -> u8 {
    match prezzo {
        Prezzo::Gratis => 0,
        Prezzo::Sconosciuto => 1,
        Prezzo::APagamento => 2,
    }
}

/// Un prezzo scritto come stringa, che è come lo manda OpenRouter.
///
/// `"0"`, `"0.0000005"`, `"-1"` per i modelli a instradamento variabile. Zero è
/// zero comunque sia scritto; quel che non si legge come numero non è un prezzo
/// e non fa una promessa.
fn e_zero(valore: Option<&Value>) -> Option<bool> {
    let numero = match valore? {
        Value::String(testo) => testo.trim().parse::<f64>().ok()?,
        Value::Number(numero) => numero.as_f64()?,
        _ => return None,
    };
    numero.is_finite().then_some(numero == 0.0)
}

/// Una riga di `data[]`, letta per quel che serve a sceglierla.
///
/// Fuori da [`Cliente`] perché non ha bisogno di niente di suo: così si prova
/// senza un indirizzo, una chiave e un servizio acceso — la stessa ragione per
/// cui sta fuori [`leggi_evento`].
fn voce_di_modello(voce: &Value) -> Option<Modello> {
    let id = voce.get("id").and_then(Value::as_str)?.to_owned();
    // Un nome uguale allo slug non è un nome: sarebbe la stessa parola due
    // volte nella stessa riga di menù.
    let nome = voce
        .get("name")
        .and_then(Value::as_str)
        .filter(|scritto| !scritto.is_empty() && *scritto != id)
        .map(ToOwned::to_owned);
    let contesto = voce
        .get("context_length")
        .and_then(Value::as_u64)
        .and_then(|quanti| u32::try_from(quanti).ok());

    // Gratis solo quando **entrambe** le direzioni dicono zero: un modello che
    // legge gratis e scrive a pagamento è a pagamento, e chiamarlo gratis
    // sarebbe la mezza verità che costa qualcosa a chi ci crede.
    let prezzi = voce.get("pricing");
    let prezzo = match (
        e_zero(prezzi.and_then(|p| p.get("prompt"))),
        e_zero(prezzi.and_then(|p| p.get("completion"))),
    ) {
        (Some(true), Some(true)) => Prezzo::Gratis,
        (Some(_), Some(_)) => Prezzo::APagamento,
        // Ollama e LM Studio non mandano `pricing` affatto.
        _ => Prezzo::Sconosciuto,
    };

    // OpenRouter dichiara quel che un modello legge in
    // `architecture.input_modalities`; chi non lo dichiara non ha detto di no.
    let vede = voce
        .get("architecture")
        .and_then(|a| a.get("input_modalities"))
        .and_then(Value::as_array)
        .map(|modi| modi.iter().any(|m| m.as_str() == Some("image")));

    Some(Modello {
        id,
        nome,
        contesto,
        prezzo,
        vede,
    })
}

/// La frase che il servizio ha scritto dentro il corpo di una risposta storta.
///
/// # Perché va cercata, invece di essere già lì
///
/// Perché `stato_a_errore` mette nel `cause` i primi cinquecento caratteri del
/// corpo **così com'è**: per un servizio che risponde JSON è
/// `{"error":{"message":"API key expired.","code":401,…}}`, cioè la frase utile
/// dentro tre livelli di impalcatura. E `cause` è il campo per la diagnostica —
/// finisce nel diario, non sotto gli occhi di chi legge.
///
/// Portarla in `message` è quel che la fa arrivare a schermo, ed è l'unica metà
/// che dice **cosa fare**: «use this slug instead: minimax/minimax-m3» è una
/// correzione da copiare, mentre «questo fornitore non conosce quel modello» è
/// una constatazione.
///
/// Le tre forme sono quelle che si incontrano davvero: OpenAI e OpenRouter
/// annidano sotto `error`, Ollama scrive `{"error":"model 'x' not found"}`, e
/// qualcuno mette `message` in cima.
fn messaggio_del_servizio(causa: &str) -> Option<String> {
    let inizio = causa.find('{')?;
    let documento: Value = serde_json::from_str(causa.get(inizio..)?).ok()?;
    let dentro = documento.get("error");
    let frase = match dentro {
        Some(Value::String(testo)) => Some(testo.as_str()),
        Some(oggetto) => oggetto.get("message").and_then(Value::as_str),
        None => documento.get("message").and_then(Value::as_str),
    }?;
    let pulita = frase.trim();
    (!pulita.is_empty()).then(|| accorcia(pulita, DETTAGLIO_MASSIMO))
}

/// Al posto di un segreto tolto da un testo.
const SEGRETO: &str = "‹chiave›";

/// Sotto questa lunghezza una «chiave» non si cerca nel testo per esteso.
///
/// Una chiave vera è lunga decine di caratteri. Un server locale ne accetta una
/// qualunque, anche di una lettera, e cercare dappertutto la lettera «k»
/// renderebbe illeggibile la frase senza proteggere niente.
const CHIAVE_MINIMA: usize = 8;

/// Un testo arrivato dal servizio, senza la chiave in uso e senza le forme in
/// cui una chiave si riconosce: `sk-…` e `Bearer …`.
///
/// Le due forme valgono anche quando non sono la chiave di questo profilo: un
/// proxy che ripete l'intestazione che ha ricevuto, o la chiave di un altro
/// conto, è un segreto lo stesso, e nel diario non ha niente da fare.
fn senza_segreti(testo: &str, chiave: Option<&str>) -> String {
    let senza_la_chiave = match chiave.filter(|k| k.chars().count() >= CHIAVE_MINIMA) {
        Some(chiave) => senza_la_coda(&testo.replace(chiave, SEGRETO), chiave),
        None => testo.to_owned(),
    };
    let di_una_chiave = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let di_un_gettone = |c: char| c.is_ascii_alphanumeric() || "-._~+/=".contains(c);
    let senza_sk = togli_dopo(&senza_la_chiave, "sk-", 8, false, di_una_chiave);
    togli_dopo(&senza_sk, "bearer ", 1, true, di_un_gettone)
}

/// Quanti caratteri dell'inizio di una chiave, rimasti prima di un taglio,
/// bastano per toglierli. Meno di così sono lettere qualunque.
const CODA_MINIMA: usize = 4;

/// Toglie l'inizio della chiave lasciato prima di un taglio.
///
/// # Perché
///
/// La rete accorcia il corpo di una risposta d'errore a cinquecento caratteri
/// prima che arrivi qui (`stato_a_errore`), e una chiave che cade sul taglio
/// arriva già a metà: «Invalid key: gsk_Q7w2Ee9Rr4…». Per intero non c'è più,
/// e la metà rimasta è quasi tutta la chiave per chi la cerca. Un taglio è
/// sempre in coda, ed è segnato dai puntini (`…`): lì si guarda se il testo
/// finisce con un pezzo iniziale della chiave, e si toglie il più lungo.
fn senza_la_coda(testo: &str, chiave: &str) -> String {
    // Gli inizi della chiave, dal più lungo: lo scarto in byte dove finiscono.
    let inizi: Vec<usize> = chiave
        .char_indices()
        .map(|(posto, _)| posto)
        .chain(std::iter::once(chiave.len()))
        .skip(CODA_MINIMA)
        .collect();
    let mut tagli: Vec<usize> = testo.match_indices('…').map(|(posto, _)| posto).collect();
    tagli.push(testo.len());
    let mut fuori = testo.to_owned();
    // Da destra, così gli scarti dei tagli a sinistra restano validi.
    for taglio in tagli.into_iter().rev() {
        let prima = fuori.get(..taglio).unwrap_or_default();
        let Some(lungo) = inizi.iter().rev().copied().find(|&fine| {
            chiave
                .get(..fine)
                .is_some_and(|pezzo| prima.ends_with(pezzo))
        }) else {
            continue;
        };
        fuori.replace_range(taglio.saturating_sub(lungo)..taglio, SEGRETO);
    }
    fuori
}

/// Toglie quel che segue `prefisso` (cercato senza badare alle maiuscole),
/// quando ne segue almeno `minimo` caratteri `ammesso`.
///
/// Il prefisso conta solo dove comincia una parola: «task-…» non è una chiave.
/// `tieni` lascia il prefisso davanti al segnaposto — «Bearer ‹chiave›» dice
/// ancora che cosa c'era.
fn togli_dopo(
    testo: &str,
    prefisso: &str,
    minimo: usize,
    tieni: bool,
    ammesso: impl Fn(char) -> bool,
) -> String {
    // Le minuscole ASCII non cambiano la lunghezza in byte di niente: gli scarti
    // trovati qui valgono anche nel testo vero.
    let basso = testo.to_ascii_lowercase();
    let mut fuori = String::with_capacity(testo.len());
    let mut copiato = 0;
    let mut da = 0;
    while let Some(trovato) = basso.get(da..).and_then(|resto| resto.find(prefisso)) {
        let inizio = da.saturating_add(trovato);
        let dopo = inizio.saturating_add(prefisso.len());
        let a_inizio_di_parola = testo
            .get(..inizio)
            .and_then(|prima| prima.chars().next_back())
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let lungo: usize = testo
            .get(dopo..)
            .unwrap_or_default()
            .chars()
            .take_while(|c| ammesso(*c))
            .map(char::len_utf8)
            .sum();
        da = dopo;
        if !a_inizio_di_parola || lungo < minimo {
            continue;
        }
        let fino = if tieni { dopo } else { inizio };
        fuori.push_str(testo.get(copiato..fino).unwrap_or_default());
        fuori.push_str(SEGRETO);
        copiato = dopo.saturating_add(lungo);
        da = copiato;
    }
    fuori.push_str(testo.get(copiato..).unwrap_or_default());
    fuori
}

/// Se questa frase parla di un modello sbagliato invece che di altro.
///
/// # Perché non basta più `contains("model")` sul corpo grezzo
///
/// Perché il corpo grezzo di un 404 di OpenRouter è «No endpoints found for
/// deepseek/deepseek-r1.» — nessun «model» dentro, quindi il caso più comune
/// di tutti restava un `net.http` muto, «il server ha risposto con un errore».
/// E il loro «… is not a valid model ID» arriva con un **400**, che il ramo non
/// guardava affatto.
fn parla_di_modello(frase: &str) -> bool {
    let minuscolo = frase.to_lowercase();
    ["model", "endpoints found", "slug", "no such model"]
        .iter()
        .any(|ago| minuscolo.contains(ago))
}

/// Quel che un evento del flusso aggiunge alla risposta.
///
/// Fuori da [`Cliente`] perché non ha bisogno di niente di suo, e perché così
/// si prova senza costruire un client — cioè senza un indirizzo, una chiave e
/// un servizio acceso.
fn leggi_evento(documento: &Value, fine: &mut Fine, pezzo: &mut dyn FnMut(Voce, &str)) {
    if let Some(scelta) = documento
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
    {
        let delta = scelta.get("delta");
        // Il ragionamento **prima** della risposta, quando un evento porta tutti
        // e due: è l'ordine in cui il modello li ha scritti, e invertirlo
        // metterebbe la conclusione prima del pensiero che la giustifica.
        //
        // Due nomi per la stessa cosa perché il protocollo non ne ha uno solo:
        // `reasoning` è quello di OpenRouter e di vLLM, `reasoning_content`
        // quello di DeepSeek, di LM Studio e di parte di Ollama. Guardarne uno
        // solo vuol dire un pannello muto sulla metà dei modelli che pensano.
        if let Some(pensato) = delta
            .and_then(|d| {
                d.get("reasoning")
                    .and_then(Value::as_str)
                    .or_else(|| d.get("reasoning_content").and_then(Value::as_str))
            })
            .filter(|p| !p.is_empty())
        {
            pezzo(Voce::Pensiero, pensato);
        }
        if let Some(testo) = delta.and_then(|d| d.get("content")).and_then(Value::as_str)
            && !testo.is_empty()
        {
            pezzo(Voce::Risposta, testo);
        }
        // `length` vuol dire che il tetto dei gettoni è arrivato prima della
        // fine: un blocco di operazioni troncato non si applica, e chi legge ha
        // il diritto di sapere che la colpa è della lunghezza.
        if let Some(motivo) = scelta.get("finish_reason").and_then(Value::as_str) {
            fine.motivo = match motivo {
                "length" => Motivo::Tagliato,
                _ => Motivo::Finito,
            };
        }
    }
    // Il conteggio arriva nell'ultimo evento, e solo da chi lo manda: Ollama e
    // LM Studio spesso non lo fanno, e un `None` è più onesto di uno zero.
    if let Some(uso) = documento.get("usage").filter(|u| !u.is_null()) {
        let quanti = |campo: &str| {
            uso.get(campo)
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
        };
        if let Some(n) = quanti("prompt_tokens") {
            fine.gettoni_in = Some(n);
        }
        if let Some(n) = quanti("completion_tokens") {
            fine.gettoni_out = Some(n);
        }
    }
}

/// La risposta di un servizio che ha ignorato `stream: true`.
///
/// `None` quando il documento non è una risposta di chat: allora era davvero
/// un'altra cosa, e chi chiama ha un errore da dare.
///
/// Il testo esce dal solito `pezzo`, in un colpo solo: chi lo mostra non ha
/// bisogno di sapere che non è arrivato a rate, e il ragionamento — che qui sta
/// dentro `message` invece che dentro `delta` — resta separato come sempre.
fn leggi_intero(documento: &Value, pezzo: &mut dyn FnMut(Voce, &str)) -> Option<Fine> {
    let scelta = documento
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())?;
    let messaggio = scelta.get("message")?;
    let testo = messaggio.get("content").and_then(Value::as_str)?;

    if let Some(pensato) = messaggio
        .get("reasoning")
        .and_then(Value::as_str)
        .or_else(|| messaggio.get("reasoning_content").and_then(Value::as_str))
        .filter(|p| !p.is_empty())
    {
        pezzo(Voce::Pensiero, pensato);
    }
    if !testo.is_empty() {
        pezzo(Voce::Risposta, testo);
    }

    let mut fine = Fine::per(match scelta.get("finish_reason").and_then(Value::as_str) {
        Some("length") => Motivo::Tagliato,
        _ => Motivo::Finito,
    });
    // Il conteggio si legge con le righe del flusso: `usage` sta allo stesso
    // posto, e una seconda copia di quelle righe sarebbe una seconda occasione
    // di scriverle diverse.
    leggi_evento(
        &json!({ "usage": documento.get("usage") }),
        &mut fine,
        &mut |_, _| {},
    );
    Some(fine)
}

/// I primi `quanti` caratteri, senza spaccare un carattere a metà.
fn accorcia(testo: &str, quanti: usize) -> String {
    match testo.char_indices().nth(quanti) {
        None => testo.to_owned(),
        Some((fine, _)) => format!("{}…", testo.get(..fine).unwrap_or_default()),
    }
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::fornitore::Fornitore;

    fn profilo(fornitore: Fornitore, url: &str) -> Profilo {
        Profilo {
            id: "prova".to_owned(),
            nome: "Prova".to_owned(),
            fornitore,
            url_base: url.to_owned(),
            modello: "qwen3".to_owned(),
            con_chiave: false,
            origine_chiave: None,
            gettoni_massimi: None,
        }
    }

    /// La chiave e la conversazione vanno all'indirizzo del profilo e basta:
    /// nessuna delle due reti segue un rimando.
    #[test]
    fn il_client_dei_modelli_non_segue_rimandi() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("sk-prova".to_owned()),
        )
        .expect("un profilo completo costruisce");
        assert!(matches!(
            cliente.rete.rimandi(),
            rymembra_net::Rimandi::Nessuno
        ));
        assert!(matches!(
            cliente.rete_corta.rimandi(),
            rymembra_net::Rimandi::Nessuno
        ));
    }

    /// Un profilo modificato a mano in `settings`, con la chiave rimasta e
    /// l'indirizzo cambiato: la chiave non parte.
    #[test]
    fn una_chiave_scritta_per_un_altra_origine_non_parte() {
        let mut spostato = profilo(Fornitore::OpenRouter, "https://esempio.com/api/v1");
        spostato.con_chiave = true;
        spostato.origine_chiave = Some("https://openrouter.ai:443".to_owned());
        let err = Cliente::nuovo(&spostato, Some("sk-prova".to_owned())).unwrap_err();
        // Il suo codice, non «nessun modello è configurato»: il modello c'è, e
        // la pagina dei modelli lo mostra (R10-6).
        assert_eq!(err.code().kind().code(), "ia.keyForOtherOrigin");
        assert!(
            !format!("{err:?}").contains("sk-prova"),
            "la chiave non finisce nell'errore"
        );

        let mut al_suo_posto = spostato.clone();
        al_suo_posto.url_base = "https://openrouter.ai/api/v1".to_owned();
        assert!(Cliente::nuovo(&al_suo_posto, Some("sk-prova".to_owned())).is_ok());
    }

    #[test]
    fn senza_chiave_un_fornitore_che_la_vuole_non_parte() {
        let err = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            None,
        )
        .unwrap_err();
        assert_eq!(
            err.code().kind().code(),
            "ia.keyMissing",
            "una chiave mancante non è un modello mancante (R10-6)"
        );
    }

    /// Senza indirizzo o senza modello il profilo è incompleto: un codice suo,
    /// e non quello di chi non ha mai configurato niente (R10-6).
    #[test]
    fn un_profilo_senza_modello_e_incompleto() {
        let mut senza_modello = profilo(Fornitore::Ollama, "http://localhost:11434/v1");
        senza_modello.modello = "  ".to_owned();
        let err = Cliente::nuovo(&senza_modello, None).unwrap_err();
        assert_eq!(err.code().kind().code(), "ia.profileIncomplete");

        let senza_indirizzo = profilo(Fornitore::Ollama, "");
        let err = Cliente::nuovo(&senza_indirizzo, None).unwrap_err();
        assert_eq!(err.code().kind().code(), "ia.profileIncomplete");
    }

    #[test]
    fn senza_chiave_un_fornitore_locale_parte() {
        assert!(
            Cliente::nuovo(
                &profilo(Fornitore::Ollama, "http://localhost:11434/v1"),
                None
            )
            .is_ok()
        );
    }

    /// La riga che tiene in piedi la frase di `PRIVACY.md`: un indirizzo in
    /// chiaro che non è questa macchina non arriva nemmeno a costruire un
    /// client.
    #[test]
    fn un_indirizzo_in_chiaro_fuori_di_casa_non_costruisce_niente() {
        let err = Cliente::nuovo(
            &profilo(Fornitore::Personalizzato, "http://esempio.com/v1"),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(err.code(), ErrorCode::IaNotLoopback { host } if host == "esempio.com"),
            "{err:?}"
        );
    }

    /// Il `Debug` è scritto a mano proprio perché questo non succeda.
    #[test]
    fn la_chiave_non_esce_da_un_debug() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("sk-or-v1-segretissimo".to_owned()),
        )
        .unwrap();
        let stampato = format!("{cliente:?}");
        assert!(!stampato.contains("segretissimo"), "{stampato}");
        assert!(stampato.contains("presente"), "{stampato}");
    }

    #[test]
    fn le_intestazioni_di_cortesia_le_ha_solo_chi_le_vuole() {
        let con = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("k".to_owned()),
        )
        .unwrap();
        let nomi: Vec<&str> = con.intestazioni().iter().map(|(n, _)| *n).collect();
        assert_eq!(nomi, ["authorization", "http-referer", "x-title"]);

        let senza = Cliente::nuovo(
            &profilo(Fornitore::Ollama, "http://localhost:11434/v1"),
            None,
        )
        .unwrap();
        assert!(senza.intestazioni().is_empty());
    }

    /// Raccoglie quel che un evento consegna, tenendo separate le due voci.
    fn raccogli(eventi: &[Value]) -> (String, String, Fine) {
        let mut fine = Fine::per(Motivo::Finito);
        let mut risposta = String::new();
        let mut pensiero = String::new();
        for evento in eventi {
            leggi_evento(evento, &mut fine, &mut |voce, testo| match voce {
                Voce::Risposta => risposta.push_str(testo),
                Voce::Pensiero => pensiero.push_str(testo),
            });
        }
        (risposta, pensiero, fine)
    }

    #[test]
    fn un_evento_normale_consegna_il_testo() {
        let (risposta, pensiero, fine) = raccogli(&[
            json!({"choices": [{"delta": {"content": "ciao"}, "finish_reason": null}]}),
        ]);
        assert_eq!(risposta, "ciao");
        assert_eq!(pensiero, "");
        assert_eq!(fine.motivo, Motivo::Finito);
    }

    /// Il difetto per cui questa distinzione esiste: un modello che ragiona
    /// scrive per un minuto in un campo che, se si legge solo `content`, non
    /// esiste.
    #[test]
    fn il_ragionamento_arriva_e_non_si_mescola_alla_risposta() {
        let (risposta, pensiero, _) = raccogli(&[
            json!({"choices": [{"delta": {"reasoning": "penso", "content": null}}]}),
            json!({"choices": [{"delta": {"reasoning_content": " ancora"}}]}),
            json!({"choices": [{"delta": {"reasoning": null, "content": "ecco"}}]}),
        ]);
        assert_eq!(risposta, "ecco");
        assert_eq!(pensiero, "penso ancora");
    }

    /// Quando un evento porta tutti e due, il pensiero viene prima: è l'ordine
    /// in cui il modello li ha scritti.
    #[test]
    fn in_un_evento_solo_il_pensiero_precede_la_risposta() {
        let mut ordine = Vec::new();
        let mut fine = Fine::per(Motivo::Finito);
        leggi_evento(
            &json!({"choices": [{"delta": {"reasoning": "prima", "content": "dopo"}}]}),
            &mut fine,
            &mut |voce, testo| ordine.push((voce, testo.to_owned())),
        );
        assert_eq!(
            ordine,
            vec![
                (Voce::Pensiero, "prima".to_owned()),
                (Voce::Risposta, "dopo".to_owned()),
            ]
        );
    }

    #[test]
    fn il_tetto_dei_gettoni_si_distingue_dalla_fine() {
        let mut fine = Fine::per(Motivo::Finito);
        leggi_evento(
            &json!({"choices": [{"delta": {}, "finish_reason": "length"}]}),
            &mut fine,
            &mut |_, _| {},
        );
        assert_eq!(fine.motivo, Motivo::Tagliato);
    }

    #[test]
    fn il_conteggio_arriva_solo_se_lo_mandano() {
        let mut fine = Fine::per(Motivo::Finito);
        leggi_evento(
            &json!({"choices": [], "usage": null}),
            &mut fine,
            &mut |_, _| {},
        );
        assert_eq!(fine.gettoni_in, None);
        leggi_evento(
            &json!({"usage": {"prompt_tokens": 120, "completion_tokens": 34}}),
            &mut fine,
            &mut |_, _| {},
        );
        assert_eq!(fine.gettoni_in, Some(120));
        assert_eq!(fine.gettoni_out, Some(34));
    }

    #[test]
    fn un_evento_senza_contenuto_non_chiama_nessuno() {
        let mut fine = Fine::per(Motivo::Finito);
        let mut quante = 0_u32;
        for evento in [
            json!({"choices": [{"delta": {"role": "assistant"}}]}),
            json!({"choices": [{"delta": {"content": "", "reasoning": ""}}]}),
            json!({}),
        ] {
            leggi_evento(&evento, &mut fine, &mut |_, _| quante += 1);
        }
        assert_eq!(quante, 0);
    }

    /// Un servizio che ignora `stream: true` risponde tutto in una volta: la
    /// risposta c'è, ed è la stessa. Rifiutarla vorrebbe dire dare un errore su
    /// una richiesta che ha funzionato — e, con un fornitore a pagamento, su una
    /// richiesta già pagata.
    #[test]
    fn una_risposta_intera_si_legge_come_se_fosse_arrivata_a_pezzi() {
        let mut risposta = String::new();
        let mut pensiero = String::new();
        let fine = leggi_intero(
            &json!({
                "choices": [{
                    "message": {"content": "eccolo", "reasoning": "ci penso"},
                    "finish_reason": "length"
                }],
                "usage": {"prompt_tokens": 9, "completion_tokens": 2}
            }),
            &mut |voce, testo| match voce {
                Voce::Risposta => risposta.push_str(testo),
                Voce::Pensiero => pensiero.push_str(testo),
            },
        )
        .expect("è una risposta di chat");
        assert_eq!(risposta, "eccolo");
        assert_eq!(pensiero, "ci penso");
        assert_eq!(fine.motivo, Motivo::Tagliato);
        assert_eq!(fine.gettoni_in, Some(9));
        assert_eq!(fine.gettoni_out, Some(2));
    }

    /// Quel che non è una risposta di chat resta un guasto: senza questo, una
    /// pagina di errore con dentro un JSON qualunque passerebbe per una risposta
    /// vuota, e il pannello direbbe che il modello non ha niente da dire.
    #[test]
    fn quel_che_non_e_una_risposta_non_diventa_una_risposta() {
        for documento in [
            json!({"error": {"message": "no"}}),
            json!({"choices": []}),
            json!({"choices": [{"delta": {"content": "ciao"}}]}),
            json!({"messaggio": "un servizio qualunque"}),
        ] {
            assert!(
                leggi_intero(&documento, &mut |_, _| {}).is_none(),
                "{documento} non è una risposta di chat"
            );
        }
    }

    /// Le due risposte che il diario aveva registrato, parola per parola. In
    /// tutte e due il fornitore aveva scritto la frase che diceva cosa fare, e
    /// in tutte e due quella frase si fermava dentro `cause` — cioè nel diario,
    /// dove la legge chi sviluppa, e non a schermo, dove serviva.
    #[test]
    fn la_chiave_non_esce_in_un_errore() {
        // Un proxy compatibile con OpenAI che ripete la chiave nella frase
        // d'errore — succede, a volte mascherata solo in parte. La frase va a
        // schermo e la causa nel diario, che l'utente allega a una segnalazione.
        let chiave = "sk-or-v1-0123456789abcdef0123456789abcdef";
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some(chiave.to_owned()),
        )
        .unwrap();
        // E le forme comuni, anche quando non sono la chiave di questo profilo.
        let altra = "sk-proj-ZZZZyyyyXXXXwwww1234";
        let portatore = "Bearer abc.def-ghi_jkl";
        let corpo = format!(
            r#"{{"error":{{"message":"Invalid API key: {chiave} (Authorization: {portatore}, also {altra})","code":401}}}}"#
        );
        let http = |status: u16, causa: &str| {
            AppError::new(ErrorCode::NetHttp { status, url: None }).with_cause(causa.to_owned())
        };
        let guasti = [
            cliente.traduci(http(401, &corpo)),
            cliente.traduci(http(500, &corpo)),
            cliente.traduci(http(404, &format!("model not found for key {chiave}"))),
            cliente.dal_documento(&json!({ "message": format!("bad key {chiave}") })),
            cliente.forma_sbagliata(&format!("la risposta non era un flusso: {corpo}")),
        ];
        for err in &guasti {
            let tutto = format!("{err:?} {:?} {:?}", err.message(), err.cause());
            for segreto in [chiave, "0123456789abcdef", altra, "abc.def-ghi_jkl"] {
                assert!(!tutto.contains(segreto), "«{segreto}» in {tutto}");
            }
        }
        // La frase resta, senza il segreto: è quella che dice cosa fare.
        assert!(
            guasti[0]
                .message()
                .is_some_and(|m| m.contains("Invalid API key: ‹chiave›")),
            "{:?}",
            guasti[0]
        );
        // Quel che somiglia a una chiave solo a metà parola resta com'è, e una
        // «chiave» di una lettera — un server locale ne accetta una qualunque —
        // non si cerca dentro le parole.
        let frase = "task-force-1234567890: API key expired";
        assert_eq!(senza_segreti(frase, Some("k")), frase);
    }

    #[test]
    fn una_chiave_tagliata_a_meta_non_esce_lo_stesso() {
        // Una chiave che non comincia per `sk-` — `gsk_…`, `AIza…`, quella che
        // accetta un server locale — e una frase lunga abbastanza da farla
        // cadere su un taglio. Due tagli: quello del messaggio qui dentro
        // (`DETTAGLIO_MASSIMO`) e quello della rete, che arriva già fatto.
        let chiave = "gsk_Q7w2Ee9Rr4Tt6Yy8Uu1Ii3Oo5Pp0AaSsDdFf";
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Personalizzato, "https://proxy.esempio.it/v1"),
            Some(chiave.to_owned()),
        )
        .unwrap();
        let http = |causa: String| {
            AppError::new(ErrorCode::NetHttp {
                status: 401,
                url: None,
            })
            .with_cause(causa)
        };
        // La chiave a cavallo del trecentesimo carattere della frase.
        let prima = "x".repeat(DETTAGLIO_MASSIMO - 30);
        let corpo = format!(r#"{{"error":{{"message":"{prima} {chiave} fine"}}}}"#);
        // E la rete che ha già tagliato a metà della chiave: i primi
        // cinquecento caratteri del corpo, e i puntini.
        let lungo = format!(
            r#"{{"error":{{"message":"{} {chiave} fine"}}}}"#,
            "y".repeat(470)
        );
        let tagliato = format!("{}…", lungo.chars().take(500).collect::<String>());
        for err in [
            cliente.traduci(http(corpo)),
            cliente.traduci(http(tagliato)),
        ] {
            let tutto = format!("{err:?} {:?} {:?}", err.message(), err.cause());
            // Nemmeno la metà: i primi sedici caratteri di una chiave sono
            // già quasi tutta la chiave per chi la cerca.
            assert!(!tutto.contains(&chiave[..16]), "{tutto}");
        }
    }

    #[test]
    fn quel_che_openrouter_ha_scritto_arriva_a_schermo() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("k".to_owned()),
        )
        .unwrap();

        let scaduta = cliente.traduci(
            AppError::new(ErrorCode::NetHttp {
                status: 401,
                url: None,
            })
            .with_cause(r#"{"error":{"message":"API key expired.","code":401,"metadata":{}}}"#),
        );
        assert!(
            matches!(scaduta.code(), ErrorCode::IaUnauthorized { .. }),
            "{scaduta:?}"
        );
        assert_eq!(scaduta.message(), Some("API key expired."));

        // Il 404 del diario: dice **quale** slug usare al posto di quello.
        let a_pagamento = cliente.traduci(
            AppError::new(ErrorCode::NetHttp {
                status: 404,
                url: None,
            })
            .with_cause(
                r#"{"error":{"message":"This model is unavailable for free. The paid version is available now - use this slug instead: minimax/minimax-m3","code":404}}"#,
            ),
        );
        assert!(
            matches!(a_pagamento.code(), ErrorCode::IaModelUnknown { .. }),
            "{a_pagamento:?}"
        );
        assert!(
            a_pagamento
                .message()
                .is_some_and(|m| m.contains("minimax/minimax-m3")),
            "{a_pagamento:?}"
        );
    }

    /// Le due risposte di OpenRouter che prima non arrivavano da nessuna parte:
    /// un 404 che non contiene la parola «model», e un 400 che il ramo non
    /// guardava affatto. Uscivano tutte e due come `net.http`, cioè «il server
    /// ha risposto con un errore».
    #[test]
    fn un_modello_che_non_esiste_si_riconosce_anche_senza_la_parola_model() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("k".to_owned()),
        )
        .unwrap();

        let senza_rotte = cliente.traduci(
            AppError::new(ErrorCode::NetHttp {
                status: 404,
                url: None,
            })
            .with_cause(r#"{"error":{"message":"No endpoints found for qualcosa/che-non-ce."}}"#),
        );
        assert!(
            matches!(senza_rotte.code(), ErrorCode::IaModelUnknown { .. }),
            "{senza_rotte:?}"
        );

        let slug_storto = cliente.traduci(
            AppError::new(ErrorCode::NetHttp {
                status: 400,
                url: None,
            })
            .with_cause(r#"{"error":{"message":"gpt5 is not a valid model ID"}}"#),
        );
        assert!(
            matches!(slug_storto.code(), ErrorCode::IaModelUnknown { .. }),
            "{slug_storto:?}"
        );

        // E un 400 che parla d'altro resta quel che era: non tutto quel che il
        // servizio rifiuta è un modello sbagliato.
        let altro = cliente.traduci(
            AppError::new(ErrorCode::NetHttp {
                status: 400,
                url: None,
            })
            .with_cause(r#"{"error":{"message":"messages: field required"}}"#),
        );
        assert!(
            matches!(altro.code(), ErrorCode::NetHttp { status: 400, .. }),
            "{altro:?}"
        );
        assert_eq!(
            altro.message(),
            Some("messages: field required"),
            "anche quel che tiene il suo codice porta via la frase"
        );
    }

    /// Un nome che non si risolve verso un servizio locale è lo stesso caso di
    /// una connessione rifiutata: LM Studio non è acceso. Prima
    /// `net.hostUnknown` non esisteva e quel guasto usciva come `net.badSchema`.
    #[test]
    fn un_nome_che_non_si_risolve_in_casa_e_il_server_spento() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Bionic, "http://localhost:1234/v1"),
            None,
        )
        .unwrap();
        let tradotto = cliente.traduci(AppError::new(ErrorCode::NetHostUnknown { host: None }));
        assert!(
            matches!(tradotto.code(), ErrorCode::IaLocalServerDown { .. }),
            "{tradotto:?}"
        );
    }

    #[test]
    fn una_riga_dellelenco_dice_quanto_costa_e_quanto_regge() {
        let gratis = voce_di_modello(&json!({
            "id": "deepseek/deepseek-r1:free",
            "name": "DeepSeek R1 (free)",
            "context_length": 163_840,
            "pricing": {"prompt": "0", "completion": "0"}
        }))
        .expect("una riga buona");
        assert_eq!(gratis.prezzo, Prezzo::Gratis);
        assert_eq!(gratis.contesto, Some(163_840));
        assert_eq!(gratis.nome.as_deref(), Some("DeepSeek R1 (free)"));
        assert_eq!(gratis.vede, None, "senza `architecture` non ha detto di no");

        let vista = voce_di_modello(&json!({
            "id": "x/vede",
            "architecture": {"input_modalities": ["text", "image"]}
        }))
        .expect("una riga buona");
        assert_eq!(vista.vede, Some(true));
        let cieco = voce_di_modello(&json!({
            "id": "x/cieco",
            "architecture": {"input_modalities": ["text"]}
        }))
        .expect("una riga buona");
        assert_eq!(cieco.vede, Some(false));

        // Legge gratis e scrive a pagamento: è a pagamento. La mezza verità qui
        // costa soldi a chi ci crede.
        let mezzo = voce_di_modello(&json!({
            "id": "x/y",
            "pricing": {"prompt": "0", "completion": "0.0000004"}
        }))
        .expect("una riga buona");
        assert_eq!(mezzo.prezzo, Prezzo::APagamento);

        // Ollama non manda nessun prezzo, e non è «a pagamento».
        let locale = voce_di_modello(&json!({"id": "qwen3:8b"})).expect("una riga buona");
        assert_eq!(locale.prezzo, Prezzo::Sconosciuto);
        assert_eq!(locale.nome, None, "un nome uguale all'id non è un nome");
        assert_eq!(locale.contesto, None);

        // Una riga senza `id` testuale si salta invece di far fallire l'elenco.
        assert!(voce_di_modello(&json!({"object": "model"})).is_none());
    }

    #[test]
    fn un_401_diventa_la_chiave_sbagliata() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("k".to_owned()),
        )
        .unwrap();
        let tradotto = cliente.traduci(AppError::new(ErrorCode::NetHttp {
            status: 401,
            url: None,
        }));
        assert!(
            matches!(tradotto.code(), ErrorCode::IaUnauthorized { provider } if provider == "openrouter"),
            "{tradotto:?}"
        );
    }

    /// Un 404 secco è un indirizzo sbagliato, non un modello sbagliato: la
    /// differenza è fra due rimedi diversi, e confonderli manda a cambiare la
    /// cosa giusta nel posto sbagliato.
    #[test]
    fn un_404_diventa_il_modello_solo_se_parla_di_modelli() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, "http://localhost:11434/v1"),
            None,
        )
        .unwrap();
        let secco = cliente.traduci(AppError::new(ErrorCode::NetHttp {
            status: 404,
            url: None,
        }));
        assert!(
            matches!(secco.code(), ErrorCode::NetHttp { .. }),
            "{secco:?}"
        );

        let parlante = cliente.traduci(
            AppError::new(ErrorCode::NetHttp {
                status: 404,
                url: None,
            })
            .with_cause("{\"error\":\"model 'qwen3' not found\"}"),
        );
        assert!(
            matches!(parlante.code(), ErrorCode::IaModelUnknown { model } if model == "qwen3"),
            "{parlante:?}"
        );
    }

    #[test]
    fn un_locale_spento_non_e_la_rete_che_manca() {
        let locale = Cliente::nuovo(
            &profilo(Fornitore::Ollama, "http://localhost:11434/v1"),
            None,
        )
        .unwrap();
        let tradotto = locale.traduci(AppError::new(ErrorCode::NetOffline { url: None }));
        assert!(
            matches!(tradotto.code(), ErrorCode::IaLocalServerDown { .. }),
            "{tradotto:?}"
        );

        // Lo stesso guasto verso un servizio remoto resta quel che è.
        let remoto = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("k".to_owned()),
        )
        .unwrap();
        let intatto = remoto.traduci(AppError::new(ErrorCode::NetOffline { url: None }));
        assert!(matches!(intatto.code(), ErrorCode::NetOffline { .. }));
    }

    /// Un rimando non seguito si dice con la frase che nomina dove portava:
    /// è l'indirizzo di base del profilo quel che va corretto.
    #[test]
    fn un_rimando_non_seguito_dice_dove_portava() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Personalizzato, "https://proxy.esempio/v1"),
            None,
        )
        .unwrap();
        let rete = Rete::nuova("ia", Duration::from_secs(1));
        let rimando = rymembra_net::Risposta {
            stato: 307,
            corpo: Vec::new(),
            riprova_fra_ms: None,
            posizione: Some("https://nuovo.esempio/v1/models?firma=x".to_owned()),
            url_finale: "https://proxy.esempio/v1/models".to_owned(),
            intervallo: None,
        };
        let tradotto =
            cliente.traduci(rete.stato_a_errore(&rimando, "https://proxy.esempio/v1/models"));
        assert_eq!(tradotto.code().kind().code(), "net.http");
        assert!(!tradotto.is_retryable());
        let frase = tradotto.message().unwrap_or_default();
        assert!(frase.contains("nuovo.esempio/v1/models"), "{frase}");
        assert!(!frase.contains("firma"), "la query non si mostra: {frase}");
    }

    /// Un servitore su questa macchina che accetta una connessione, legge la
    /// richiesta **intera** — testa e corpo — e poi fa quel che `poi` dice.
    ///
    /// La richiesta si legge tutta prima di rispondere: su Windows un socket
    /// chiuso con byte ancora da leggere manda un RST, e il client perderebbe la
    /// risposta.
    fn servitore(poi: impl FnOnce(&mut std::net::TcpStream) + Send + 'static) -> u16 {
        let poi = std::sync::Mutex::new(Some(poi));
        servitore_per(1, move |connessione, _| {
            if let Some(poi) = poi.lock().ok().and_then(|mut p| p.take()) {
                poi(connessione);
            }
        })
    }

    /// Come [`servitore`], per `volte` connessioni una dopo l'altra, e `poi`
    /// riceve anche il corpo della richiesta: per guardare cosa il client ha
    /// chiesto.
    fn servitore_per(
        volte: usize,
        poi: impl Fn(&mut std::net::TcpStream, &[u8]) + Send + 'static,
    ) -> u16 {
        use std::io::Read as _;
        let ascolto =
            std::net::TcpListener::bind("127.0.0.1:0").expect("una porta su questa macchina");
        let porta = ascolto.local_addr().expect("la porta scelta").port();
        std::thread::spawn(move || {
            for _ in 0..volte {
                let Ok((mut connessione, _)) = ascolto.accept() else {
                    return;
                };
                let mut testa = Vec::new();
                let mut byte = [0_u8; 1];
                while !testa.ends_with(b"\r\n\r\n") {
                    match connessione.read(&mut byte) {
                        Ok(1) => testa.push(byte[0]),
                        _ => return,
                    }
                }
                let lunghezza = String::from_utf8_lossy(&testa)
                    .lines()
                    .find_map(|riga| {
                        let (nome, valore) = riga.split_once(':')?;
                        nome.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then(|| valore.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                let mut corpo = vec![0_u8; lunghezza];
                if connessione.read_exact(&mut corpo).is_err() {
                    return;
                }
                poi(&mut connessione, &corpo);
            }
        });
        porta
    }

    /// Le intestazioni di un flusso di eventi che finisce con la connessione.
    const TESTA_FLUSSO: &[u8] =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

    /// `conversa` su un altro filo, con al più `entro` di attesa: le prove dei
    /// tetti non devono poter restare appese a un servitore che tace.
    fn conversa_entro(cliente: Cliente, entro: Duration) -> (Result<Fine, AppError>, usize) {
        let (esito_tx, esito_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut arrivati = 0_usize;
            let esito = cliente.conversa(&[Messaggio::utente("ciao")], &|| false, &mut |_, t| {
                arrivati += t.len();
            });
            let _ = esito_tx.send((esito, arrivati));
        });
        esito_rx
            .recv_timeout(entro)
            .expect("la conversazione deve finire senza aspettare il servitore")
    }

    /// Un corpo enorme senza un a capo — un file, una pagina, un servizio che
    /// sgocciola byte — fa rinunciare il decodificatore al primo mebibyte: da
    /// lì la conversazione finisce subito, e non quando il servitore chiude o
    /// fra dieci minuti (R08-5).
    #[test]
    fn un_flusso_senza_a_capo_si_chiude_subito() {
        let porta = servitore(|connessione| {
            use std::io::Write as _;
            let _ = connessione.write_all(TESTA_FLUSSO);
            let blocco = vec![b'x'; 64 * 1024];
            for _ in 0..32 {
                if connessione.write_all(&blocco).is_err() {
                    return;
                }
            }
            let _ = connessione.flush();
            std::thread::sleep(Duration::from_secs(30));
        });
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, &format!("http://127.0.0.1:{porta}/v1")),
            None,
        )
        .expect("un profilo locale costruisce");
        let (esito, _) = conversa_entro(cliente, Duration::from_secs(5));
        let guasto = esito.expect_err("due mebibyte senza a capo non sono una risposta");
        assert_eq!(guasto.code().kind().code(), "ia.badResponse");
    }

    /// La richiesta dice quanti gettoni può scrivere il modello: di serie
    /// [`crate::GETTONI_DI_SERIE`], o quel che il profilo chiede (R08-3, D-64). Senza,
    /// un modello che entra in un ciclo scrive fino al massimo del modello, e
    /// OpenRouter rifiuta la richiesta a chi ha poco credito.
    #[test]
    fn la_richiesta_porta_un_tetto_di_gettoni() {
        let (corpo_tx, corpo_rx) = std::sync::mpsc::channel();
        let porta = servitore_per(1, move |connessione, corpo| {
            use std::io::Write as _;
            let _ = corpo_tx.send(corpo.to_vec());
            let _ = connessione.write_all(TESTA_FLUSSO);
            let _ = connessione.write_all(b"data: [DONE]\n\n");
        });
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, &format!("http://127.0.0.1:{porta}/v1")),
            None,
        )
        .expect("un profilo locale costruisce");
        let _ = conversa_entro(cliente, Duration::from_secs(5));
        let corpo = corpo_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("il servitore ha ricevuto la richiesta");
        let richiesta: Value = serde_json::from_slice(&corpo).expect("un JSON");
        assert_eq!(
            richiesta.get("max_tokens").and_then(Value::as_u64),
            Some(4096),
            "{richiesta}"
        );

        // Il numero del profilo, quando c'è; dentro i bordi anche se qualcuno
        // ha scritto `settings` a mano.
        let mut suo = profilo(Fornitore::Ollama, "http://localhost:11434/v1");
        suo.gettoni_massimi = Some(1000);
        let cliente = Cliente::nuovo(&suo, None).expect("un profilo locale costruisce");
        let corpo = cliente
            .richiesta(&[Messaggio::utente("ciao")], CAMPO_DEL_TETTO)
            .expect("il corpo");
        let richiesta: Value = serde_json::from_slice(&corpo).expect("un JSON");
        assert_eq!(
            richiesta.get("max_tokens").and_then(Value::as_u64),
            Some(1000)
        );
        suo.gettoni_massimi = Some(3);
        let cliente = Cliente::nuovo(&suo, None).expect("un profilo locale costruisce");
        let corpo = cliente
            .richiesta(&[Messaggio::utente("ciao")], CAMPO_DEL_TETTO)
            .expect("il corpo");
        let richiesta: Value = serde_json::from_slice(&corpo).expect("un JSON");
        assert_eq!(
            richiesta.get("max_tokens").and_then(Value::as_u64),
            Some(64)
        );
    }

    /// Una risposta che non finisce — un servizio rotto o ostile che manda
    /// testo a raffica, senza `[DONE]` — si ferma al tetto del testo,
    /// [`TESTO_MASSIMO`], come «tagliata», senza aspettare che il servitore
    /// chiuda (R08-3).
    #[test]
    fn una_risposta_oltre_il_tetto_si_ferma_tagliata() {
        let porta = servitore(|connessione| {
            use std::io::Write as _;
            let _ = connessione.write_all(TESTA_FLUSSO);
            let parola = "x".repeat(4000);
            let evento =
                format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{parola}\"}}}}]}}\n\n");
            // Più di cinque mebibyte di testo.
            for _ in 0..1400 {
                if connessione.write_all(evento.as_bytes()).is_err() {
                    return;
                }
            }
            let _ = connessione.flush();
            std::thread::sleep(Duration::from_secs(30));
        });
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, &format!("http://127.0.0.1:{porta}/v1")),
            None,
        )
        .expect("un profilo locale costruisce");
        let (esito, arrivati) = conversa_entro(cliente, Duration::from_secs(10));
        let fine = esito.expect("una risposta tagliata non è un guasto");
        assert_eq!(fine.motivo, Motivo::Tagliato);
        assert_eq!(
            arrivati,
            4 * 1024 * 1024,
            "si consegna il tetto, non di più"
        );
    }

    /// Un servizio che a metà risposta smette di mandare — il modello a valle
    /// che si pianta, una connessione mezza morta — non tiene la chat occupata
    /// per i dieci minuti della scadenza: dopo [`SILENZIO_MASSIMO`] senza un
    /// byte è una scadenza, e lo si dice come tale, non come «il server ha
    /// risposto con un errore» (R08-3).
    #[test]
    fn un_silenzio_a_meta_risposta_e_una_scadenza() {
        let porta = servitore(|connessione| {
            use std::io::Write as _;
            let _ = connessione.write_all(TESTA_FLUSSO);
            let _ = connessione
                .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"Ci\"}}]}\n\n");
            let _ = connessione.flush();
            std::thread::sleep(Duration::from_secs(30));
        });
        let mut cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, &format!("http://127.0.0.1:{porta}/v1")),
            None,
        )
        .expect("un profilo locale costruisce");
        cliente.silenzio = Duration::from_millis(400);
        let (esito, arrivati) = conversa_entro(cliente, Duration::from_secs(5));
        let guasto = esito.expect_err("un servizio muto a metà non ha finito");
        assert_eq!(guasto.code().kind().code(), "net.timeout");
        assert_eq!(arrivati, 2, "quel che era arrivato è arrivato");
    }

    /// Un endpoint che parla come OpenAI e vuole `max_completion_tokens` (i
    /// modelli che ragionano, su `api.openai.com`) risponde 400 a
    /// `max_tokens`: si richiede una volta con l'altro nome, invece di rompere
    /// un profilo che prima del tetto funzionava.
    #[test]
    fn chi_rifiuta_max_tokens_riceve_max_completion_tokens() {
        let (corpo_tx, corpo_rx) = std::sync::mpsc::channel();
        let porta = servitore_per(2, move |connessione, corpo| {
            use std::io::Write as _;
            let richiesta: Value = serde_json::from_slice(corpo).unwrap_or_default();
            let _ = corpo_tx.send(richiesta.clone());
            if richiesta.get("max_tokens").is_some() {
                let rifiuto = r#"{"error":{"message":"Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.","type":"invalid_request_error","param":"max_tokens","code":"unsupported_parameter"}}"#;
                let _ = connessione.write_all(
                    format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{rifiuto}",
                        rifiuto.len()
                    )
                    .as_bytes(),
                );
                return;
            }
            let _ = connessione.write_all(TESTA_FLUSSO);
            let _ = connessione.write_all(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            );
        });
        let cliente = Cliente::nuovo(
            &profilo(
                Fornitore::Personalizzato,
                &format!("http://127.0.0.1:{porta}/v1"),
            ),
            None,
        )
        .expect("un profilo locale costruisce");
        let (esito, arrivati) = conversa_entro(cliente, Duration::from_secs(5));
        let fine = esito.expect("la seconda richiesta passa");
        assert_eq!(fine.motivo, Motivo::Finito);
        assert_eq!(arrivati, 2);
        let prima = corpo_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("la prima richiesta");
        let seconda = corpo_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("la seconda richiesta");
        assert!(prima.get("max_tokens").is_some(), "{prima}");
        assert_eq!(
            seconda.get("max_completion_tokens").and_then(Value::as_u64),
            Some(4096),
            "{seconda}"
        );
        assert!(seconda.get("max_tokens").is_none(), "{seconda}");
    }

    /// Il servizio che non manda un byte — un modello che si carica, un prompt
    /// lungo valutato su CPU, un server bloccato — si ferma entro un respiro da
    /// quando si preme «Ferma», e non al primo byte o ai dieci minuti della
    /// scadenza.
    #[test]
    fn un_servizio_muto_si_ferma_subito() {
        let porta = servitore(|_| std::thread::sleep(Duration::from_secs(30)));
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, &format!("http://127.0.0.1:{porta}/v1")),
            None,
        )
        .expect("un profilo locale costruisce");
        let (esito_tx, esito_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let partenza = std::time::Instant::now();
            let annullato = || partenza.elapsed() >= Duration::from_millis(200);
            let esito = cliente.conversa(&[Messaggio::utente("ciao")], &annullato, &mut |_, _| {});
            let _ = esito_tx.send(esito);
        });
        let esito = esito_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("«Ferma» deve rispondere senza aspettare il servizio");
        let fine = esito.expect("un annullamento non è un guasto");
        assert_eq!(fine.motivo, Motivo::Fermato);
    }

    /// La risposta che arriva a pezzi passa ancora tutta, nell'ordine, e il
    /// motivo della fine è quello che il servizio ha dichiarato.
    #[test]
    fn una_risposta_in_flusso_arriva_intera() {
        let porta = servitore(|connessione| {
            use std::io::Write as _;
            let _ = connessione.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            );
            for evento in [
                r#"{"choices":[{"delta":{"reasoning":"penso"}}]}"#,
                r#"{"choices":[{"delta":{"content":"Ciao"}}]}"#,
                r#"{"choices":[{"delta":{"content":" mondo"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#,
                "[DONE]",
            ] {
                let _ = connessione.write_all(format!("data: {evento}\n\n").as_bytes());
                let _ = connessione.flush();
            }
        });
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, &format!("http://127.0.0.1:{porta}/v1")),
            None,
        )
        .expect("un profilo locale costruisce");
        let mut risposta = String::new();
        let mut pensiero = String::new();
        let fine = cliente
            .conversa(
                &[Messaggio::utente("ciao")],
                &|| false,
                &mut |voce, testo| match voce {
                    Voce::Risposta => risposta.push_str(testo),
                    Voce::Pensiero => pensiero.push_str(testo),
                },
            )
            .expect("una risposta ben formata");
        assert_eq!(risposta, "Ciao mondo");
        assert_eq!(pensiero, "penso");
        assert_eq!(fine.motivo, Motivo::Finito);
        assert_eq!((fine.gettoni_in, fine.gettoni_out), (Some(3), Some(2)));
    }

    /// Un `error` dentro un flusso già cominciato resta un errore, con le
    /// parole del servizio.
    #[test]
    fn un_errore_dentro_il_flusso_resta_un_errore() {
        let porta = servitore(|connessione| {
            use std::io::Write as _;
            let _ = connessione.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
                  data: {\"choices\":[{\"delta\":{\"content\":\"Ci\"}}]}\n\n\
                  data: {\"error\":{\"message\":\"il modello a valle e caduto\"}}\n\n",
            );
        });
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::Ollama, &format!("http://127.0.0.1:{porta}/v1")),
            None,
        )
        .expect("un profilo locale costruisce");
        let guasto = cliente
            .conversa(&[Messaggio::utente("ciao")], &|| false, &mut |_, _| {})
            .expect_err("un errore dichiarato non è una risposta");
        assert_eq!(guasto.code().kind().code(), "ia.badResponse");
        assert!(
            guasto.cause().is_some_and(|c| c.contains("a valle")),
            "{guasto:?}"
        );
    }

    /// Il limite di frequenza non ha un codice suo, e non deve prenderselo.
    #[test]
    fn un_429_resta_quello_che_era() {
        let cliente = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            Some("k".to_owned()),
        )
        .unwrap();
        let tradotto = cliente.traduci(AppError::new(ErrorCode::NetRateLimited {
            service: Some("ia".to_owned()),
            retry_after_ms: Some(3000),
        }));
        assert!(matches!(tradotto.code(), ErrorCode::NetRateLimited { .. }));
        assert!(tradotto.is_retryable());
    }

    #[test]
    fn un_testo_lungo_si_accorcia_senza_spaccare_un_carattere() {
        assert_eq!(accorcia("àèìòù", 3), "àèì…");
        assert_eq!(accorcia("corto", 99), "corto");
    }
}
