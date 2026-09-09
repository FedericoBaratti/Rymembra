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
//! Quasi tutto passa da [`aether_net::http::Rete::stato_a_errore`], che è la
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

use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_net::http::{Corpo, Metodo, Rete, Richiesta};
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
/// davvero da un servizio fermo è l'annullamento, che risponde entro un blocco;
/// questa è la rete sotto, per il caso in cui nessuno stia guardando.
const SCADENZA_LUNGA: Duration = Duration::from_secs(600);

/// Quanto si aspetta per l'elenco dei modelli.
///
/// Quindici secondi. È una richiesta che serve a **sapere se il servizio c'è**:
/// se non risponde in quindici secondi la risposta utile è già «non c'è», e
/// farla aspettare dieci minuti dietro la stessa scadenza della generazione
/// renderebbe il bottone «Prova» indistinguibile da un blocco.
const SCADENZA_CORTA: Duration = Duration::from_secs(15);

/// Quanto di un corpo di errore si tiene per raccontarlo.
const DETTAGLIO_MASSIMO: usize = 300;

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
    /// # Errori
    ///
    /// `ia.notConfigured` se manca l'indirizzo, il modello, o la chiave di un
    /// fornitore che la vuole. `ia.notLoopback` per un indirizzo in chiaro che
    /// non è questa macchina.
    pub fn nuovo(profilo: &Profilo, chiave: Option<String>) -> Result<Self, AppError> {
        let manca = |cosa: &str| {
            Err(AppError::new(ErrorCode::IaNotConfigured)
                .with_message(format!("il profilo «{}» non ha {cosa}", profilo.id)))
        };
        if profilo.url_base.trim().is_empty() {
            return manca("un indirizzo");
        }
        if profilo.modello.trim().is_empty() {
            return manca("un modello");
        }
        let chiave = chiave.filter(|k| !k.trim().is_empty());
        if profilo.fornitore.vuole_chiave() && chiave.is_none() {
            return manca("una chiave");
        }
        crate::profilo::controlla_indirizzo(&profilo.url_base)?;

        Ok(Self {
            rete: Rete::verso(SERVIZIO, SCADENZA_LUNGA, &profilo.url_base)?,
            rete_corta: Rete::verso(SERVIZIO, SCADENZA_CORTA, &profilo.url_base)?,
            profilo: profilo.clone(),
            chiave,
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
    /// Come in tutto il resto dell'albero: una chiusura interrogata fra un
    /// blocco e l'altro. Un annullamento non è un guasto e non esce come
    /// `Err`: esce come [`Motivo::Fermato`], perché chi ha premuto «Ferma» sa
    /// già cos'è successo e non ha bisogno di leggerlo in rosso.
    ///
    /// # Errori
    ///
    /// `ia.unauthorized`, `ia.modelUnknown`, `ia.localServerDown`,
    /// `ia.noAnswer`, `ia.badResponse`, e i `net.*` di sempre.
    pub fn conversa(
        &self,
        messaggi: &[Messaggio],
        annullato: &dyn Fn() -> bool,
        pezzo: &mut dyn FnMut(Voce, &str),
    ) -> Result<Fine, AppError> {
        let url = self.profilo.punto("/chat/completions");
        let intestazioni = self.intestazioni();
        let riferimenti: Vec<(&str, &str)> =
            intestazioni.iter().map(|(n, v)| (*n, v.as_str())).collect();
        let mut richiesta = json!({
            "model": self.profilo.modello,
            "messages": messaggi
                .iter()
                .map(|m| json!({ "role": m.ruolo.nome(), "content": m.testo }))
                .collect::<Vec<_>>(),
            "stream": true,
        });
        // OpenRouter i gettoni li conta anche in streaming, ma li manda solo a
        // chi li chiede. Senza questa riga il conto resta `None` proprio dove
        // costa qualcosa — cioè sull'unico fornitore dei quattro che fa pagare.
        if self.profilo.fornitore == Fornitore::OpenRouter
            && let Some(oggetto) = richiesta.as_object_mut()
        {
            oggetto.insert("usage".to_owned(), json!({ "include": true }));
        }
        let corpo = serde_json::to_vec(&richiesta)
            .map_err(|err| self.forma_sbagliata(&format!("la richiesta: {err}")))?;

        let mut sse = Sse::nuovo();
        let mut fine = Fine::per(Motivo::Finito);
        let mut visto_qualcosa = false;
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

        let esito = self.rete.flusso(
            &url,
            &riferimenti,
            Corpo::Byte {
                tipo: "application/json",
                dati: &corpo,
            },
            annullato,
            &mut |blocco: &[u8]| {
                if !visto_qualcosa && crudo.len() < CRUDO_MASSIMO {
                    crudo.extend_from_slice(blocco);
                }
                for evento in sse.mangia(blocco) {
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
                    leggi_evento(&documento, &mut fine, pezzo);
                }
                Ok(())
            },
        );

        match esito {
            Ok(()) => {}
            // L'annullamento arriva come `internal.aborted`, e va riconosciuto
            // **prima** di tradurre: chi ha premuto «Ferma» non ha causato un
            // guasto, e la risposta a metà che ha già letto resta valida.
            Err(err) if matches!(err.code(), ErrorCode::InternalAborted { .. }) => {
                return Ok(Fine {
                    motivo: Motivo::Fermato,
                    ..fine
                });
            }
            Err(err) => return Err(self.traduci(err)),
        }

        if !visto_qualcosa {
            // Prima di dare la colpa a qualcuno: forse la risposta c'è, e non è
            // un flusso. Un servizio che ignora `stream: true` risponde con il
            // documento intero, e dentro c'è `choices[0].message.content` invece
            // di una sequenza di `delta`. È la stessa risposta, arrivata tutta
            // in una volta: leggerla costa dieci righe, e l'alternativa è
            // rifiutare qualcosa che ha funzionato.
            if let Ok(documento) = serde_json::from_slice::<Value>(&crudo) {
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
                    accorcia(&String::from_utf8_lossy(&crudo), DETTAGLIO_MASSIMO)
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
                "https://github.com/FedericoBaratti/Aether".to_owned(),
            ));
            fuori.push(("x-title", "Aether".to_owned()));
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
        let causa = err.cause().unwrap_or_default().to_owned();
        let detto = messaggio_del_servizio(&causa);
        // Il ripiego sul corpo grezzo tiene in piedi i servizi che non
        // rispondono JSON: Ollama scrive `{"error":"model 'x' not found"}`, ma
        // un proxy davanti può rispondere testo.
        let parla_di_modello = detto.as_deref().is_some_and(parla_di_modello)
            || causa.to_lowercase().contains("model");

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
    fn forma_sbagliata(&self, dettaglio: &str) -> AppError {
        let corto = accorcia(dettaglio, DETTAGLIO_MASSIMO);
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

    Some(Modello {
        id,
        nome,
        contesto,
        prezzo,
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
        }
    }

    #[test]
    fn senza_chiave_un_fornitore_che_la_vuole_non_parte() {
        let err = Cliente::nuovo(
            &profilo(Fornitore::OpenRouter, "https://openrouter.ai/api/v1"),
            None,
        )
        .unwrap_err();
        assert!(matches!(err.code(), ErrorCode::IaNotConfigured));
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
