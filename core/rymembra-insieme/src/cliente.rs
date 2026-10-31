//! Il cliente: chi legge i byte dei gettoni di un altro PC.
//!
//! # Le connessioni di lettura
//!
//! Al più [`CONNESSIONI_DI_LETTURA`] verso lo stesso host, aperte quando
//! servono e tenute per la lettura dopo ([`Vivaio`]). Due perché sono due i
//! lettori che chiedono byte nello stesso momento: il brano che suona e quello
//! che si prepara in canna. Separate da quella di controllo perché due
//! megabyte in coda davanti a un'ancora la farebbero arrivare tardi.
//!
//! # L'annullo
//!
//! Una lettura annullata torna **subito**, e la sua connessione si butta: la
//! risposta che stava arrivando è ormai di nessuno, e leggerla fino in fondo
//! per tenere la connessione costerebbe più che aprirne un'altra. Chi aspetta
//! guarda la bandiera ogni [`RESPIRO_LETTURA`], su un canale e non sul socket.
//!
//! Anche mentre la connessione si apre: il collegamento TCP (fino a tre
//! secondi) e la stretta (fino a cinque) li fa un filo a parte
//! ([`Vivaio::apri`]), e chi aspetta guarda la bandiera allo stesso modo.
//! Fino alla revisione del 26 ottobre li faceva lui, e un annullo arrivato
//! durante la stretta aspettava la fine della stretta.
//!
//! # La riconnessione
//!
//! Una connessione che cade prima della risposta (l'host l'ha chiusa per il
//! silenzio, l'ha mandata via per far posto a una nuova, o si è riavviato)
//! non fa fallire la lettura: se ne apre una nuova e si chiede di nuovo,
//! **una** volta. Una lettura è idempotente, e chiederla due volte non cambia
//! niente. Fino alla revisione del 26 ottobre si richiedeva solo su una
//! connessione riusata, e una appena aperta che cadeva faceva fallire la
//! lettura. Non si richiede dopo un silenzio intero senza risposta: un host
//! che non risponde da quindici secondi non risponderà nei quindici dopo.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCodeKind};

use crate::collegamento::{
    Atteso, Collegamento, Modo, RESPIRO_LETTURA, Uscita, Verso, json, leggi_gettone, prepara, vivi,
};
use crate::condiviso::Condiviso;
use crate::errori::{annullata, chiave, errore};
use crate::identita::ChiavePubblica;
use crate::messaggi::Gettone;
use crate::servente::{Chiesto, LETTURA_MASSIMA, Risposta};
use crate::trasporto::{Trasporto, Uso};

/// Quante connessioni di lettura verso lo stesso host. Vedi il `//!`.
pub(crate) const CONNESSIONI_DI_LETTURA: usize = 2;

/// La bandiera alzata?
fn annullato(annullo: Option<&AtomicBool>) -> bool {
    annullo.is_some_and(|a| a.load(Ordering::SeqCst))
}

/// Le connessioni di lettura verso un host.
pub(crate) struct Vivaio {
    rete: Arc<Condiviso>,
    ospite: SocketAddr,
    pari: ChiavePubblica,
    stato: Mutex<StatoVivaio>,
    cambiato: Condvar,
}

/// Le connessioni libere, e quante ce ne sono in tutto (libere, al lavoro e
/// in apertura).
#[derive(Default)]
struct StatoVivaio {
    libere: Vec<Arc<Collegamento>>,
    quante: usize,
}

impl std::fmt::Debug for Vivaio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vivaio").finish_non_exhaustive()
    }
}

/// Com'è andata una richiesta su una connessione.
enum Andata {
    /// La risposta.
    Risposta(Risposta),
    /// Chi aspettava ha smesso di volerla.
    Annullata,
    /// La connessione è caduta prima della risposta: si può chiedere di
    /// nuovo. Vedi «La riconnessione».
    Caduta,
    /// L'host non ha risposto per un silenzio intero: non si richiede.
    Muta,
}

impl Vivaio {
    /// Un vivaio vuoto: nessuna connessione finché non serve.
    pub(crate) fn nuovo(rete: Arc<Condiviso>, ospite: SocketAddr, pari: ChiavePubblica) -> Self {
        Self {
            rete,
            ospite,
            pari,
            stato: Mutex::new(StatoVivaio::default()),
            cambiato: Condvar::new(),
        }
    }

    /// Una richiesta, con un secondo tentativo su un'altra connessione se
    /// quella su cui è partita cade prima della risposta, riusata o appena
    /// aperta che fosse. Vedi «La riconnessione».
    ///
    /// Il secondo tentativo rispetta l'annullo come il primo; e se non riesce
    /// nemmeno ad aprire la connessione, vale il suo perché — di solito
    /// `insieme.irraggiungibile`: l'host non c'è più.
    pub(crate) fn chiedi(
        &self,
        gettone: &Gettone,
        chiesto: Chiesto,
        annullo: Option<&AtomicBool>,
    ) -> Result<Risposta, AppError> {
        for tentativo in 0..2 {
            let collegamento = self.prendi(annullo)?;
            match self.su(&collegamento, gettone, chiesto, annullo) {
                Andata::Risposta(risposta) => {
                    self.rendi(collegamento);
                    return Ok(risposta);
                }
                Andata::Annullata => {
                    self.butta(&collegamento);
                    return Err(annullata());
                }
                Andata::Caduta if tentativo == 0 => self.butta(&collegamento),
                Andata::Caduta | Andata::Muta => {
                    self.butta(&collegamento);
                    return Err(errore(ErrorCodeKind::InsiemeConnessioneCaduta));
                }
            }
        }
        Err(errore(ErrorCodeKind::InsiemeConnessioneCaduta))
    }

    /// Una connessione libera, o una nuova se ce n'è posto, o aspetta che se
    /// ne liberi una.
    fn prendi(&self, annullo: Option<&AtomicBool>) -> Result<Arc<Collegamento>, AppError> {
        let mut stato = chiave(&self.stato);
        loop {
            if annullato(annullo) {
                return Err(annullata());
            }
            if self.rete.fermo() {
                return Err(errore(ErrorCodeKind::InsiemeConnessioneCaduta));
            }
            while let Some(collegamento) = stato.libere.pop() {
                if !collegamento.chiusa() {
                    return Ok(collegamento);
                }
                stato.quante = stato.quante.saturating_sub(1);
            }
            if stato.quante < CONNESSIONI_DI_LETTURA {
                stato.quante = stato.quante.saturating_add(1);
                drop(stato);
                // Fuori dal lucchetto: aprire vuol dire una stretta, e chi
                // vuole rendere una connessione nel frattempo non deve
                // aspettarla.
                return match self.apri(annullo) {
                    Ok(collegamento) => Ok(collegamento),
                    Err(err) => {
                        let mut stato = chiave(&self.stato);
                        stato.quante = stato.quante.saturating_sub(1);
                        drop(stato);
                        self.cambiato.notify_one();
                        Err(err)
                    }
                };
            }
            stato = self
                .cambiato
                .wait_timeout(stato, RESPIRO_LETTURA)
                .map_or_else(|e| e.into_inner().0, |(s, _)| s);
        }
    }

    /// Apre una connessione di lettura e lancia i suoi fili.
    ///
    /// Collegamento e stretta li fa un filo a parte ([`apri_qui`]), e qui si
    /// aspetta il suo esito su un canale, guardando l'annullo e la rete ogni
    /// [`RESPIRO_LETTURA`]: vedi «L'annullo». Un annullo torna subito; il
    /// filo finisce per conto suo, e la connessione che apre dopo, che non
    /// aspetta più nessuno, la taglia.
    fn apri(&self, annullo: Option<&AtomicBool>) -> Result<Arc<Collegamento>, AppError> {
        let recapito = Arc::new(Recapito::default());
        let per_il_filo = Arc::clone(&recapito);
        let (ospite, pari) = (self.ospite, self.pari);
        let partito = self.rete.filo("apre", move |rete| {
            per_il_filo.consegna(apri_qui(&rete, ospite, pari));
        });
        if !partito {
            return Err(crate::errori::inatteso("il filo che apre non parte"));
        }
        // Collegamento e stretta hanno le loro scadenze, e il filo finisce
        // entro quelle: questa c'è solo perché un filo caduto senza
        // consegnare non lasci qui chi aspetta per sempre.
        let entro = Instant::now()
            + self.rete.tempi.collegamento
            + self.rete.tempi.stretta
            + Duration::from_secs(1);
        let mut posto = chiave(&recapito.posto);
        loop {
            if matches!(*posto, Posto::Arrivato(_))
                && let Posto::Arrivato(esito) = std::mem::replace(&mut *posto, Posto::Abbandonato)
            {
                return esito;
            }
            let lascia = if annullato(annullo) {
                Some(annullata())
            } else if self.rete.fermo() {
                Some(errore(ErrorCodeKind::InsiemeConnessioneCaduta))
            } else if Instant::now() >= entro {
                Some(crate::errori::inatteso("il filo che apre non ha risposto"))
            } else {
                None
            };
            if let Some(err) = lascia {
                // Sotto lo stesso lucchetto della consegna: quel che il filo
                // aprirà da qui in poi, lo taglia lui.
                *posto = Posto::Abbandonato;
                return Err(err);
            }
            posto = recapito
                .arrivato
                .wait_timeout(posto, RESPIRO_LETTURA)
                .map_or_else(|e| e.into_inner().0, |(p, _)| p);
        }
    }

    /// Rende una connessione che ha finito il suo lavoro.
    fn rendi(&self, collegamento: Arc<Collegamento>) {
        let mut stato = chiave(&self.stato);
        if collegamento.chiusa() {
            stato.quante = stato.quante.saturating_sub(1);
        } else {
            stato.libere.push(collegamento);
        }
        drop(stato);
        self.cambiato.notify_one();
    }

    /// Butta una connessione: vedi «L'annullo».
    fn butta(&self, collegamento: &Collegamento) {
        collegamento.chiudi(None, Modo::Taglio);
        let mut stato = chiave(&self.stato);
        stato.quante = stato.quante.saturating_sub(1);
        drop(stato);
        self.cambiato.notify_one();
    }

    /// Una richiesta su una connessione, fino alla risposta.
    fn su(
        &self,
        collegamento: &Collegamento,
        gettone: &Gettone,
        chiesto: Chiesto,
        annullo: Option<&AtomicBool>,
    ) -> Andata {
        let lettura = self.rete.prossima_lettura.fetch_add(1, Ordering::Relaxed);
        let (dai, prendi) = sync_channel(1);
        chiave(&collegamento.attese).insert(lettura, dai);
        let parola = match chiesto {
            Chiesto::Byte { da, quanti } => Trasporto::Leggi {
                lettura,
                gettone: *gettone,
                da,
                quanti,
            },
            Chiesto::Copertina => Trasporto::Copertina {
                lettura,
                gettone: *gettone,
            },
        };
        if !collegamento.metti(Uscita::Json(json(&parola))) {
            chiave(&collegamento.attese).remove(&lettura);
            return Andata::Caduta;
        }
        let inizio = Instant::now();
        loop {
            if annullato(annullo) {
                return Andata::Annullata;
            }
            match prendi.recv_timeout(RESPIRO_LETTURA) {
                Ok(risposta) => return Andata::Risposta(risposta),
                Err(RecvTimeoutError::Disconnected) => return Andata::Caduta,
                Err(RecvTimeoutError::Timeout) => {
                    // Un host che non risponde per un silenzio intero è un
                    // host caduto, anche se i battiti arrivano: quei battiti
                    // vengono da un filo che non è quello che legge il disco.
                    if self.rete.fermo() {
                        chiave(&collegamento.attese).remove(&lettura);
                        return Andata::Caduta;
                    }
                    if inizio.elapsed() > self.rete.tempi.silenzio {
                        chiave(&collegamento.attese).remove(&lettura);
                        return Andata::Muta;
                    }
                }
            }
        }
    }
}

/// L'esito di [`apri_qui`], dal filo che apre a chi aspetta in
/// [`Vivaio::apri`].
///
/// Un posto sotto lucchetto e non un canale perché chi se ne va (un annullo)
/// e il filo che consegna devono mettersi d'accordo su chi chiude la
/// connessione: con un canale, una consegna arrivata un istante prima che chi
/// aspetta lo butti resterebbe dentro, aperta e di nessuno — un posto
/// occupato sull'host finché non la manda via lui.
#[derive(Default)]
struct Recapito {
    posto: Mutex<Posto>,
    arrivato: Condvar,
}

/// Dov'è l'esito.
#[derive(Default)]
enum Posto {
    /// Il filo sta ancora aprendo.
    #[default]
    Attesa,
    /// Aperta, o no; da prendere.
    Arrivato(Result<Arc<Collegamento>, AppError>),
    /// Chi aspettava se n'è andato, o l'ha già preso.
    Abbandonato,
}

impl Recapito {
    /// Il filo che apre consegna: a chi aspetta, o — se non aspetta più
    /// nessuno — alla chiusura.
    fn consegna(&self, esito: Result<Arc<Collegamento>, AppError>) {
        let mut posto = chiave(&self.posto);
        if matches!(*posto, Posto::Abbandonato) {
            drop(posto);
            if let Ok(collegamento) = esito {
                collegamento.chiudi(None, Modo::Taglio);
            }
            return;
        }
        *posto = Posto::Arrivato(esito);
        drop(posto);
        self.arrivato.notify_all();
    }
}

/// Il corpo del filo di [`Vivaio::apri`]: collegamento TCP, stretta e saluto,
/// e il filo che legge.
fn apri_qui(
    rete: &Arc<Condiviso>,
    ospite: SocketAddr,
    pari: ChiavePubblica,
) -> Result<Arc<Collegamento>, AppError> {
    let presa = std::net::TcpStream::connect_timeout(&ospite, rete.tempi.collegamento)
        .map_err(|_| errore(ErrorCodeKind::InsiemeIrraggiungibile))?;
    let verso = Verso::Uscente {
        id: rete.nuovo_id(),
        uso: Uso::Lettura,
        atteso: Some(Atteso::Chiave(pari)),
    };
    let pronto = prepara(rete, presa, ospite, verso)
        .map_err(|codice| errore(codice.unwrap_or(ErrorCodeKind::InsiemeConnessioneCaduta)))?;
    let collegamento = Arc::clone(&pronto.collegamento);
    if !rete.filo("legge", move |rete| vivi(&rete, pronto)) {
        return Err(crate::errori::inatteso("il filo di lettura non parte"));
    }
    Ok(collegamento)
}

impl Drop for Vivaio {
    /// L'ultimo `LettoreRemoto` che se ne va chiude le sue connessioni:
    /// tenerle aperte vorrebbe dire due posti occupati sull'host per niente.
    fn drop(&mut self) {
        let libere = std::mem::take(&mut chiave(&self.stato).libere);
        for collegamento in libere {
            collegamento.chiudi(None, Modo::Cortese);
        }
    }
}

/// Fino a `quanti` byte da `da`, a finestre di al più [`LETTURA_MASSIMA`].
///
/// Una richiesta più grande di una finestra si spezza qui, e non sull'host:
/// così una risposta più corta di quella chiesta vuol dire sempre «fine del
/// file», come promette `LettoreRemoto::pezzo`, e mai «l'host ne manda al più
/// due mega».
pub(crate) fn a_finestre(
    da: u64,
    quanti: u64,
    mut una: impl FnMut(u64, u64) -> Result<Risposta, AppError>,
) -> Result<(Vec<u8>, u64), AppError> {
    let mut fuori: Vec<u8> = Vec::new();
    let mut posto = da;
    let mut resta = quanti;
    loop {
        let passo = resta.min(LETTURA_MASSIMA);
        let (byte, totale) = match una(posto, passo)? {
            Risposta::Byte { totale, byte } => (byte, totale),
            Risposta::Rifiutata(codice) => return Err(errore(codice)),
        };
        let letti = u64::try_from(byte.len()).unwrap_or(u64::MAX);
        if letti > passo {
            // Più di quel che si è chiesto: chi risponde non sa contare, e i
            // byte in più non sono di nessuno.
            return Err(errore(ErrorCodeKind::InsiemeProtocollo));
        }
        if fuori.is_empty() {
            fuori = byte;
        } else {
            fuori.extend_from_slice(&byte);
        }
        resta = resta.saturating_sub(letti);
        posto = posto.saturating_add(letti);
        if letti < passo || resta == 0 {
            return Ok((fuori, totale));
        }
    }
}

/// Una lettura dei gettoni di questo PC senza passare dalla rete. Vedi
/// `Rete::lettore_locale`.
pub(crate) fn qui(
    rete: &Arc<Condiviso>,
    gettone: &Gettone,
    chiesto: Chiesto,
    annullo: Option<&AtomicBool>,
) -> Result<Risposta, AppError> {
    if annullato(annullo) {
        return Err(annullata());
    }
    let per_ancora = Arc::clone(rete);
    let (risposta, _) = leggi_gettone(
        rete,
        gettone,
        chiesto,
        &|| !annullato(annullo) && !per_ancora.fermo(),
        true,
    );
    // Annullata mentre aspettava il proprietario: vale l'annullo, non il
    // rifiuto che l'attesa interrotta ha prodotto.
    if annullato(annullo) {
        return Err(annullata());
    }
    Ok(risposta)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// La finestra intera: `M`.
    const M: u64 = LETTURA_MASSIMA;

    /// I byte di una risposta, tutti uguali a `segno`.
    fn byte(quanti: u64, segno: u8) -> Vec<u8> {
        vec![segno; usize::try_from(quanti).expect("sta")]
    }

    #[test]
    fn a_finestre_spezza_e_ricompone_nell_ordine() {
        let mut chiamate = Vec::new();
        let (letti, totale) = a_finestre(7, 2 * M + 5, |da, quanti| {
            chiamate.push((da, quanti));
            let segno = u8::try_from(chiamate.len()).expect("sta");
            Ok(Risposta::Byte {
                totale: 99 * M,
                byte: byte(quanti, segno),
            })
        })
        .expect("tre finestre");
        assert_eq!(chiamate, [(7, M), (7 + M, M), (7 + 2 * M, 5)]);
        assert_eq!(totale, 99 * M);
        assert_eq!(u64::try_from(letti.len()).expect("sta"), 2 * M + 5);
        // Le tre finestre una dopo l'altra, nell'ordine in cui sono arrivate.
        let m = usize::try_from(M).expect("sta");
        assert_eq!(letti.first(), Some(&1));
        assert_eq!(letti.get(m - 1), Some(&1));
        assert_eq!(letti.get(m), Some(&2));
        assert_eq!(letti.get(2 * m), Some(&3));
        assert_eq!(letti.last(), Some(&3));
    }

    #[test]
    fn una_risposta_corta_e_la_fine_del_file() {
        let mut chiamate = Vec::new();
        let (letti, totale) = a_finestre(0, 3 * M, |da, quanti| {
            chiamate.push((da, quanti));
            let quanti = if da == 0 { quanti } else { 10 };
            Ok(Risposta::Byte {
                totale: M + 10,
                byte: byte(quanti, 1),
            })
        })
        .expect("fine del file");
        assert_eq!(chiamate, [(0, M), (M, M)], "niente terza finestra");
        assert_eq!(u64::try_from(letti.len()).expect("sta"), M + 10);
        assert_eq!(totale, M + 10);
    }

    #[test]
    fn piu_byte_del_chiesto_e_un_errore_di_protocollo() {
        // L'unica guardia del membro contro un host che manda più del
        // chiesto: `comincia` controlla un `Pezzo` solo contro
        // `LETTURA_MASSIMA`, non contro la richiesta. Senza, i byte in più
        // finirebbero al decodificatore come se fossero a quell'offset.
        let err = a_finestre(0, 100, |_, quanti| {
            Ok(Risposta::Byte {
                totale: 1_000,
                byte: byte(quanti + 1, 1),
            })
        })
        .expect_err("uno di troppo");
        assert_eq!(err.code().kind().code(), "insieme.protocollo");
    }

    #[test]
    fn un_rifiuto_a_meta_vale_il_suo_codice() {
        let err = a_finestre(0, 2 * M, |da, quanti| {
            if da == 0 {
                Ok(Risposta::Byte {
                    totale: 2 * M,
                    byte: byte(quanti, 1),
                })
            } else {
                Ok(Risposta::Rifiutata(ErrorCodeKind::InsiemeFileCambiato))
            }
        })
        .expect_err("rifiutata");
        assert_eq!(err.code().kind().code(), "insieme.fileCambiato");
    }
}
