//! La rete del filo: accenderla, spegnerla, mandare, e quel che il trasporto
//! racconta (connessioni aperte e chiuse, associazioni, vicini da collegare).

use std::net::SocketAddr;
use std::time::Instant;

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_insieme::messaggi::{MASSIMO_NOME, Messaggio};
use rymembra_insieme::{ChiavePubblica, Dove, IdConnessione, Impostazioni, Rete};
use rymembra_play::sincronia::orologio::Scambio;

use super::{
    ATTESA_LIBRERIA, ATTESA_LIBRERIA_BREVE, ATTESA_RETE, Accesa, Filo, LENTO, OGNI_COLLEGAMENTO_MS,
    OGNI_SCOPERTA_MS, Posta, chiave_per_la_finestra, lega, ora, riga_della_rete, senza_causa,
};
use crate::insieme::fasi::{Connessione, Evento};
use crate::insieme::segui::EsitoScambio;
use crate::insieme::{fiducia, voci};
use crate::nota;
use crate::stato::adesso_ms;

impl Filo {
    // ── la rete ─────────────────────────────────────────────────────────────

    pub(super) fn accendi(&mut self) -> Result<(), AppError> {
        if self.rete.is_some() {
            return Ok(());
        }
        // Mai con dei fidati mai letti: ogni PC associato sarebbe un estraneo
        // per il trasporto, che chiuderebbe la sua connessione con «non
        // associato», e dall'altra parte si leggerebbe «non riconosce più
        // questo PC».
        if !self.fidati_letti {
            self.ricarica_fidati();
            if !self.fidati_letti {
                return Err(AppError::new(ErrorCode::DbLocked)
                    .with_cause("i PC associati non si sono potuti leggere: la rete aspetta"));
            }
        }
        let dispositivo = self.in_libreria(ATTESA_LIBRERIA, crate::nuvola::dispositivo)?;
        let identita = fiducia::identita(&rymembra_oauth::portachiavi::DiSistema, &dispositivo)?;
        let mia = identita.pubblica();
        let (rete, eventi) = Rete::avvia(Impostazioni {
            identita,
            nome: self.nome.clone(),
            lega: lega(),
            porta: rymembra_insieme::PORTA,
            adesso_ns: rymembra_play::tempo::adesso_ns,
        })
        .map_err(|e| senza_causa(&e))?;
        self.generazione = self.generazione.wrapping_add(1);
        let generazione = self.generazione;
        let posta = self.posta.clone();
        let girato = std::thread::Builder::new()
            .name("rymembra-insieme-rete".to_owned())
            .spawn(move || {
                // Finisce quando la rete smette di mandare (fermata) o quando
                // il filo della sessione non c'è più.
                for evento in eventi {
                    if posta
                        .send(Posta::Rete {
                            generazione,
                            evento,
                            arrivato: Instant::now(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        if let Err(err) = girato {
            rete.ferma();
            return Err(
                AppError::new(ErrorCode::InternalUnexpected { detail: None })
                    .with_cause(format!("il filo degli eventi della rete non parte: {err}")),
            );
        }
        let (fidate, bloccate) = self.fidati.per_la_rete();
        rete.pari(fidate, bloccate);
        // Fuori casa: i dispositivi associati col QR si raggiungono dal
        // collegamento via internet (`lontano::insieme`).
        rete.strada(Some(crate::lontano::insieme::strada(&self.app)));
        self.rete = Some(Accesa { rete, mia });
        // Su Android i pacchetti mandati a tutta la Wi-Fi — quelli della
        // scoperta — arrivano solo con il lucchetto del multicast preso: senza,
        // il telefono li scarta per risparmiare batteria, e gli altri Rymembra
        // non si vedono. Si tiene finché la rete è accesa.
        #[cfg(target_os = "android")]
        if let Err(err) = rymembra_android::sistema::multicast(true) {
            nota!("[insieme] il lucchetto del multicast non si prende: {err}");
        }
        // E da Android 17 la rete di casa, che senza il permesso si ferma nel
        // sistema in silenzio. Qui non lo si chiede — la rete si accende anche
        // all'avvio, senza nessuno davanti — ma lo si dice: la domanda la fa
        // il gesto (`crate::telefono::rete_locale_o_chiedila`).
        if !crate::telefono::rete_locale_concessa() {
            nota!(
                "[insieme] Android non concede la rete di casa: manca «Dispositivi nelle vicinanze»"
            );
            self.avvisa(ErrorCodeKind::NetLocalNetwork.i18n_key(), None);
        }
        nota!(
            "[insieme] rete accesa, {} fidati{}",
            self.fidati.elenco().len(),
            if lega().is_loopback() {
                ", solo locale"
            } else {
                ""
            }
        );
        self.prossima_scoperta = 0;
        self.prossimo_collegamento = 0;
        self.nutri(Evento::Acceso);
        Ok(())
    }

    pub(super) fn ferma_la_rete(&mut self) {
        if let Some(accesa) = self.rete.take() {
            accesa.rete.ferma();
            #[cfg(target_os = "android")]
            if let Err(err) = rymembra_android::sistema::multicast(false) {
                nota!("[insieme] il lucchetto del multicast non si lascia: {err}");
            }
            let tutti = accesa.rete.aspetta_fili(ATTESA_RETE);
            nota!(
                "[insieme] rete spenta{}",
                if tutti {
                    ""
                } else {
                    ", qualche filo ancora in uscita"
                }
            );
        }
        // Gli eventi ancora in volo della rete vecchia si buttano.
        self.generazione = self.generazione.wrapping_add(1);
        self.vicinato.svuota();
        self.capi.clear();
    }

    pub(super) fn manda(&self, a: IdConnessione, messaggio: &Messaggio) {
        let Some(accesa) = &self.rete else {
            return;
        };
        // Quel che parte deve passare i tetti di chi lo riceve: un messaggio
        // che non li passa chiude la connessione dall'altra parte. Qui non
        // deve succedere mai; se succede, meglio non mandarlo e dirlo.
        if let Err(fuori) = messaggio.valida() {
            nota!(
                "[insieme] messaggio per la connessione #{} fuori dai tetti: {}",
                a.0,
                fuori.0
            );
            return;
        }
        if let Err(err) = accesa.rete.manda(a, messaggio) {
            // La chiusura arriverà come evento, e la sessione la tratterà lì.
            nota!(
                "[insieme] messaggio non partito per la connessione #{} codice={}",
                a.0,
                err.code().kind().code()
            );
        }
    }

    fn mia(&self) -> Option<ChiavePubblica> {
        self.rete.as_ref().map(|a| a.mia)
    }

    pub(super) fn dalla_rete(&mut self, evento: rymembra_insieme::Evento) {
        use rymembra_insieme::Evento as DallaRete;
        // La riga di una chiusura la decide il vicinato: un fidato spento si
        // riprova ogni minuto, e non si scrive ogni volta (vedi
        // `Vicinato::chiusa`).
        let riga = riga_della_rete(&evento, &self.fidati);
        if !matches!(evento, DallaRete::Chiusa { .. })
            && let Some(riga) = &riga
        {
            nota!("{riga}");
        }
        match evento {
            DallaRete::Trovato {
                indirizzo,
                impronta,
            } => {
                self.vicinato.trovato(Dove::Lan(indirizzo), impronta, ora());
                // L'ultimo indirizzo su cui un fidato ha dimostrato la chiave,
                // anche in un'altra accensione: vale come verificato. Vedi
                // `Vicinato::ricordato` (reperto R26-1).
                if let Some(ricordato) = self
                    .fidati
                    .per_impronta(&impronta)
                    .and_then(|p| self.ricordati.get(&p.chiave))
                {
                    self.vicinato.ricordato(impronta, Dove::Lan(*ricordato));
                }
            }
            DallaRete::Aperta {
                id,
                pari,
                nome,
                ospita,
                in_entrata,
                dove,
            } => {
                // Due istanze con la stessa identità non si parlano: sarebbe
                // un PC che si associa con sé stesso.
                if Some(pari) == self.mia() {
                    if let Some(accesa) = &self.rete {
                        accesa.rete.chiudi(id, None);
                    }
                    return;
                }
                let dichiarato = voci::testo(&nome, MASSIMO_NOME);
                self.capi.insert(id, (dove.clone(), pari));
                if !in_entrata {
                    let taciute = self.vicinato.aperta(id, &dichiarato);
                    if taciute > 0 {
                        nota!(
                            "[insieme] connessione #{} di nuovo raggiungibile dopo {taciute} tentativi non scritti",
                            id.0
                        );
                    }
                }
                let fidato = self
                    .fidati
                    .per_chiave(&pari)
                    .filter(|p| !p.bloccato)
                    .cloned();
                // Un dettaglio per il pannello («visto alle…»): se la libreria
                // è occupata si salta, e lo scriverà la prossima connessione.
                if let Some(p) = &fidato {
                    let _ = self.in_libreria(ATTESA_LIBRERIA_BREVE, |c| {
                        rymembra_app::insieme::visto(c, p.id, adesso_ms())
                    });
                }
                // L'indirizzo di un host fidato che ha risposto: la prossima
                // volta lo si cerca anche lì, dove il broadcast non arriva. Solo
                // per le connessioni aperte da qui — l'indirizzo di chi bussa è
                // la sua porta d'uscita, che non ascolta — e solo se è cambiato.
                if !in_entrata
                    && fidato.is_some()
                    && let Dove::Lan(indirizzo) = dove
                {
                    let chiave = pari.in_testo();
                    if self.ricordati.get(&chiave) != Some(&indirizzo) {
                        let scritto = self.in_libreria(ATTESA_LIBRERIA_BREVE, |c| {
                            rymembra_app::insieme::ricorda_indirizzo(
                                c,
                                &chiave,
                                &indirizzo.to_string(),
                                adesso_ms(),
                            )
                        });
                        if scritto.is_ok() {
                            self.ricordati.insert(chiave, indirizzo);
                        }
                    }
                }
                self.nutri(Evento::Aperta {
                    id,
                    connessione: Connessione {
                        chiave: pari,
                        nome: fidato
                            .as_ref()
                            .map_or_else(|| dichiarato.clone(), |p| p.nome.clone()),
                        dichiarato,
                        ospita,
                        in_entrata,
                        pari: fidato.map(|p| p.id),
                    },
                });
            }
            // Il posto nella coda d'entrata della connessione se ne va con
            // `evento`, alla fine di questa funzione: trattato il messaggio,
            // quella connessione può leggere il prossimo.
            DallaRete::Ricevuto { id, messaggio, .. } => {
                if let Messaggio::Esito { id: numero, codice } = &messaggio {
                    self.esito_arrivato(id, *numero, codice.as_deref());
                }
                self.nutri(Evento::Ricevuto { id, messaggio });
            }
            DallaRete::AssociazioneChiesta { id } => {
                self.nutri(Evento::AssociazioneChiesta { id });
            }
            DallaRete::Codice { id, codice } => self.nutri(Evento::Codice { id, codice }),
            // L'host sta associando un altro PC: niente codice per ora, e la
            // connessione resta. Si dice perché, e «Associa» torna premibile.
            DallaRete::AssociazioneRimandata { id, codice } => {
                let nome = Some(self.nome_della_connessione(id));
                self.nutri(Evento::AssociazioneFinita { id });
                self.avvisa(codice.i18n_key(), nome);
            }
            DallaRete::Associato { id, pari } => self.associato(id, pari),
            DallaRete::Orologio { id, campione } => {
                if let Some(seguendo) = &self.seguendo
                    && seguendo.host == id
                {
                    let esito = seguendo.condivisa.scambio(Scambio {
                        t1: campione.t1,
                        t2: campione.t2,
                        t3: campione.t3,
                        t4: campione.t4,
                    });
                    // La stima dell'orologio dell'host è ricominciata da un
                    // punto solo: si torna ai ping svelti, che ne rifanno una in
                    // pochi secondi invece di aspettarne cinque al ritmo di uno
                    // al secondo (reperto R05-3). Intanto la pausa dell'host si
                    // segue lo stesso (vedi `segui`).
                    if esito == EsitoScambio::Ricominciato {
                        nota!("[insieme] l'orologio dell'host è cambiato: la stima ricomincia");
                        if let Some(accesa) = &self.rete {
                            accesa.rete.sincronizza(id, true);
                        }
                    }
                }
            }
            DallaRete::Chiusa { id, codice } => {
                self.capi.remove(&id);
                if self.vicinato.chiusa(id, ora(), codice)
                    && let Some(riga) = riga
                {
                    nota!("{riga}");
                }
                self.nutri(Evento::Chiusa { id, codice });
            }
        }
    }

    /// Chi segue: la risposta a un suo gesto. Nel diario se è un no, o se è
    /// arrivata tardi: è il tempo che la persona ha aspettato.
    fn esito_arrivato(&mut self, da: IdConnessione, numero: u32, codice: Option<&str>) {
        let Some(seguendo) = self.seguendo.as_mut().filter(|s| s.host == da) else {
            return;
        };
        let Some(posto) = seguendo.in_volo.iter().position(|(n, ..)| *n == numero) else {
            return;
        };
        let (_, partito, nome) = seguendo.in_volo.remove(posto);
        let attesa = partito.elapsed();
        if codice.is_some() || attesa >= LENTO {
            nota!(
                "[insieme] gesto «{nome}» risposto dall'host in {} ms codice={}",
                attesa.as_millis(),
                codice.unwrap_or("—")
            );
        }
    }

    fn associato(&mut self, id: IdConnessione, chiave: ChiavePubblica) {
        if Some(chiave) == self.mia() {
            return;
        }
        let nome = self
            .s
            .connessioni
            .get(&id)
            .map(|c| c.dichiarato.clone())
            .unwrap_or_default();
        let esito = self.in_libreria(ATTESA_LIBRERIA, |c| {
            rymembra_app::insieme::associa(c, &chiave.in_testo(), &nome, adesso_ms())
        });
        match esito {
            Ok(pari) => {
                nota!("[insieme] associato: pari #{pari}");
                self.ricarica_fidati();
                self.nutri(Evento::Associato { id, pari });
            }
            Err(err) => {
                nota!(
                    "[insieme] associazione non salvata codice={}",
                    err.code().kind().code()
                );
                // «Non si è salvato niente»: è la frase di `codiceRifiutato`, ed
                // è esattamente quel che è successo.
                if let Some(chiave) =
                    chiave_per_la_finestra(&err, Some(ErrorCodeKind::InsiemeCodiceRifiutato))
                {
                    self.avvisa(chiave, None);
                }
                self.nutri(Evento::AssociazioneFinita { id });
            }
        }
    }

    pub(super) fn cerca(&mut self, adesso: u64) {
        self.prossima_scoperta = adesso.saturating_add(OGNI_SCOPERTA_MS);
        self.vicinato.pulisci(adesso, &self.fidati);
        // I dispositivi collegati via internet che ospitano: vicini anche
        // loro, raggiungibili da lì, se la scoperta non li vede in casa.
        for (chiave, chiave_insieme) in crate::lontano::insieme::ospitano(&self.app) {
            if let Some(pubblica) = ChiavePubblica::da_testo(&chiave_insieme) {
                self.vicinato
                    .trovato_lontano(&chiave, pubblica.impronta(), adesso);
            }
        }
        // Gli scritti a mano, e gli ultimi indirizzi dei PC fidati: una domanda
        // a ciascuno, oltre al broadcast. Vedi la 040.
        let mut a_mano: Vec<SocketAddr> = self
            .impostazioni
            .indirizzi
            .iter()
            .filter_map(|i| i.parse().ok())
            .collect();
        for indirizzo in self.ricordati.values() {
            if !a_mano.contains(indirizzo) {
                a_mano.push(*indirizzo);
            }
        }
        if let Some(accesa) = &self.rete {
            accesa.rete.cerca(&a_mano);
        }
    }

    pub(super) fn collega_vicini(&mut self, adesso: u64) {
        self.prossimo_collegamento = adesso.saturating_add(OGNI_COLLEGAMENTO_MS);
        let Some(accesa) = &self.rete else {
            return;
        };
        for (impronta, indirizzo) in
            self.vicinato
                .da_collegare(adesso, accesa.mia.impronta(), &self.fidati)
        {
            // Con l'impronta che la scoperta ha dato: la risposta viaggia in
            // chiaro e la può scrivere chiunque, e la stretta deve dimostrare
            // che dall'altra parte c'è proprio quella chiave.
            let id = accesa.rete.collega_atteso(indirizzo, impronta);
            self.vicinato.collegando(impronta, id);
        }
    }
}
