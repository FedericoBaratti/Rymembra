//! Chi ospita: lo stato e le ancore ai membri, le voci servite, i gesti dei
//! membri eseguiti dall'esecutore e le voci di questo PC preparate dal loro
//! filo.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_insieme::messaggi::{
    AncoraSulFilo, Azione, Dove, MASSIMO_AGGIUNTE, Messaggio, VoceSulFilo,
};
use rymembra_insieme::{ChiavePubblica, Gettone, IdConnessione};
use tauri::{AppHandle, Manager as _};

use super::{
    DalFilo, Dettagli, Filo, ImprontaStato, OGNI_ANCORA_MS, OGNI_DESCRIZIONI_MS, OGNI_STATO_MS,
    Posta, UID_DEI_MEMBRI, lega, nel_lettore_di_chi_ospita, ora, proprietari,
    proprietario_di_chi_ospita, senza_causa,
};
use crate::insieme::fasi::{self, Evento, Fase};
use crate::insieme::gesti::{self, Passo};
use crate::insieme::preparazione;
use crate::insieme::strada::Ruolo;
use crate::insieme::voci::{self, Servizio};
use crate::nota;
use crate::riproduzione::insieme as ponte;

impl Filo {
    // ── chi ospita ──────────────────────────────────────────────────────────

    pub(super) fn ospita(&mut self) {
        let Some(accesa) = &self.rete else {
            self.errore
                .get_or_insert(AppError::new(ErrorCode::InsiemeSpenta));
            return;
        };
        let porta = match accesa.rete.ospita() {
            Ok(porta) => porta,
            Err(err) => {
                self.errore.get_or_insert(senza_causa(&err));
                return;
            }
        };
        // Gli indirizzi da dettare al collega, una volta per sessione:
        // enumerare le schede a ogni pubblicazione costerebbe una chiamata di
        // sistema per niente.
        let qui = rymembra_insieme::indirizzi_di_qui(lega());
        self.indirizzi_di_qui = qui
            .principale
            .into_iter()
            .chain(qui.altri)
            .map(|ip| format!("{ip}:{porta}"))
            .collect();
        // Il ruolo prima: vedi il `//!` di `filo`.
        self.ruolo(Ruolo::Host);
        if let Err(err) = ponte::entra_come_host(&self.app) {
            self.ruolo(Ruolo::Fuori);
            accesa.rete.smetti_di_ospitare();
            self.errore.get_or_insert(err);
            return;
        }
        // Le voci dei membri (fase 2) si leggono senza rete: i loro gettoni
        // sono inoltrati, e `lettore_locale` li chiede alla connessione di
        // servizio del membro che li possiede.
        let lettore = accesa.rete.lettore_locale();
        self.dal_filo = Some(DalFilo::nuovo(lettore, self.posta.clone()));
        nota!("[insieme] si ospita");
        self.sessione = self.sessione.wrapping_add(1);
        self.sessione_viva.store(self.sessione, Ordering::Release);
        self.preparazione.svuota();
        self.servite.svuota();
        self.versione = 0;
        self.uid_dei_membri = 0;
        self.impronta_stato = None;
        self.nutri(Evento::Ospitato { porta });
        self.campanello();
    }

    /// Il lettore di chi ospita è cambiato: voci nuove da descrivere, gettoni
    /// da revocare, uno stato e un'ancora da mandare.
    pub(super) fn campanello(&mut self) {
        if !matches!(self.s.fase, Fase::Ospita(_)) {
            return;
        }
        let Some(istantanea) = ponte::istantanea(&self.app) else {
            return;
        };
        let mut tenere: HashSet<u64> = voci::finestra(&istantanea.coda, istantanea.corrente)
            .iter()
            .copied()
            .collect();
        if let Some(corrente) = istantanea.corrente {
            tenere.insert(corrente);
        }
        // Le voci dei membri restano finché sono in coda, anche fuori dalla
        // finestra: una volta dimenticate non le si saprebbe più descrivere,
        // perché i loro byte non sono di questo PC.
        let altrui: HashSet<u64> = self.servite.altrui().map(|(uid, _)| uid).collect();
        tenere.extend(istantanea.coda.iter().filter(|uid| altrui.contains(uid)));
        // Le voci di questo PC ancora da preparare si chiedono al loro filo,
        // in blocco e nell'ordine in cui serviranno: qui non si aspetta mai il
        // disco. Vedi `preparazione`.
        let candidate: Vec<u64> = preparazione::in_ordine(
            voci::finestra(&istantanea.coda, istantanea.corrente),
            istantanea.corrente,
        )
        .into_iter()
        .filter(|uid| !self.servite.ha(*uid) && *uid != ponte::UID_CALIBRAZIONE)
        .collect();
        let chiedere = self.preparazione.da_chiedere(&candidate, ora());
        if let Some(preparatore) = &self.preparatore {
            // La voce che suona, se non è ancora descritta, passa davanti ai
            // blocchi già in fila: chi segue la aspetta per suonare, e senza
            // di lei le ancore non partono. I blocchi possono essere quelli di
            // una coda appena sostituita, mille voci da un NAS.
            if let Some(corrente) = istantanea.corrente
                && corrente != ponte::UID_CALIBRAZIONE
                && !self.servite.ha(corrente)
                && self.chiesta_subito != Some((self.sessione, corrente))
            {
                self.chiesta_subito = Some((self.sessione, corrente));
                let _ = preparatore.send(DaPreparare::Subito {
                    sessione: self.sessione,
                    uid: corrente,
                });
            }
            for blocco in chiedere.chunks(preparazione::BLOCCO) {
                let _ = preparatore.send(DaPreparare::Blocco {
                    sessione: self.sessione,
                    uids: blocco.to_vec(),
                });
            }
        }
        self.preparazione.tieni(&tenere);
        let conto = self.preparazione.non_mandabili(&tenere);
        if let Some(riga) = self.preparazione.riga_se_cambia(conto) {
            nota!("{riga}");
        }
        let tolte = self.servite.fuori(&tenere);
        let servizi = self.servite.tieni(&tenere);
        self.esegui_servizi(servizi, false);
        if let Some(dal) = &mut self.dal_filo {
            dal.registrate.retain(|uid, _| tenere.contains(uid));
        }
        // Con le voci che `servite` ha tolto davvero, non con `tenere`: così
        // la pulizia ha la stessa finestra della loro, e non ne apre una sua.
        self.dimentica_le_copertine(&tolte);
        self.proprietari_di_chi_ospita(&istantanea);
        self.corrente = istantanea.corrente;
        // La calibrazione si legge dal lettore: la chiude anche un «prossimo»
        // o una voce scelta, e una copia tenuta qui resterebbe accesa sui clic
        // che non suonano più. Chiusa così, lo si dice anche a chi ospita.
        let calibrando = ponte::calibrando(&self.app);
        if self.calibrazione && !calibrando {
            self.avvisa(fasi::AVVISO_CALIBRAZIONE_SPENTA, None);
        }
        self.calibrazione = calibrando;
        self.locale = ponte::voce_corrente_locale(&self.app);
        self.fotografa_host();
        self.manda_stato(&istantanea, false);
        self.manda_ancora();
    }

    /// Esegue i servizi di una `Servite`: quella di chi ospita, o quella delle
    /// voci proprie di chi segue (`proprie`), che serve le stesse cose.
    pub(super) fn esegui_servizi(&mut self, servizi: Vec<Servizio>, proprie: bool) {
        let mut senza = Vec::new();
        {
            let Some(accesa) = &self.rete else {
                return;
            };
            let store = self.libreria().and_then(|s| s.copertine()).ok();
            for servizio in servizi {
                match servizio {
                    Servizio::File(gettone, file) => match lettore_del_cloud(&self.app, &file) {
                        Some(lettore) => accesa.rete.servi_lettore(gettone, lettore),
                        None => accesa.rete.servi(gettone, file),
                    },
                    Servizio::Copertina(gettone, impronta) => {
                        let byte = store
                            .as_ref()
                            .and_then(|s| std::fs::read(s.path_for(&impronta)).ok())
                            .filter(|b| !b.is_empty() && b.len() <= voci::BYTE_MASSIMI_COPERTINA);
                        match byte {
                            Some(byte) => accesa.rete.servi_copertina(gettone, Arc::from(byte)),
                            None => senza.push(impronta),
                        }
                    }
                    Servizio::Revoca(gettone) => accesa.rete.revoca(&gettone),
                    // Gli inoltri li fa `aggiungi_del_membro`, che sa rifiutare
                    // l'aggiunta quando il trasporto dice di no; qui arrivano
                    // solo per completezza.
                    Servizio::Inoltra(gettone, membro) => {
                        let fatto = self
                            .chiave_del_membro(membro)
                            .is_some_and(|chiave| accesa.rete.inoltra(gettone, chiave));
                        if !fatto {
                            nota!("[insieme] gettone del membro {membro} non inoltrato");
                        }
                    }
                    Servizio::Descrivi { file, track_id } => {
                        if let Some(coda) = &self.descrittori {
                            let _ = coda.send((file, track_id));
                        }
                    }
                }
            }
        }
        for impronta in senza {
            let revoche = if proprie {
                self.seguendo
                    .as_mut()
                    .map(|s| s.proprie.senza_copertina(&impronta))
                    .unwrap_or_default()
            } else {
                self.servite.senza_copertina(&impronta)
            };
            for servizio in revoche {
                if let (Servizio::Revoca(gettone), Some(accesa)) = (servizio, &self.rete) {
                    accesa.rete.revoca(&gettone);
                }
            }
        }
    }

    /// La chiave del membro con quel numero, dalla sua connessione.
    fn chiave_del_membro(&self, membro: u32) -> Option<ChiavePubblica> {
        let Fase::Ospita(o) = &self.s.fase else {
            return None;
        };
        let conn = o.membri.iter().find(|m| m.membro == membro)?.conn;
        self.capi.get(&conn).map(|(_, chiave)| *chiave)
    }

    /// Lo stato ai membri, se è cambiato (o `forzato`), al più ogni
    /// [`OGNI_STATO_MS`] (salvo `forzato`).
    pub(super) fn manda_stato(&mut self, istantanea: &ponte::Istantanea, forzato: bool) {
        let membri = fasi::connessioni_dei_membri(&self.s);
        if membri.is_empty() {
            self.stato_in_sospeso = false;
            return;
        }
        let persone = fasi::persone_di_qui(&self.s, &self.nome);
        let impronta = ImprontaStato {
            coda: istantanea.coda.clone(),
            descritte: istantanea
                .coda
                .iter()
                .filter(|uid| self.servite.ha(**uid))
                .count(),
            copie: self.copie,
            corrente: istantanea.corrente,
            corrente_descritta: istantanea.corrente.is_some_and(|uid| self.servite.ha(uid)),
            in_pausa: istantanea.in_pausa,
            ripeti: istantanea.ripeti,
            persone: persone.clone(),
        };
        if !forzato && self.impronta_stato.as_ref() == Some(&impronta) {
            self.stato_in_sospeso = false;
            return;
        }
        let adesso = ora();
        let ogni = if self
            .impronta_stato
            .as_ref()
            .is_some_and(|prima| impronta.solo_descrizioni(prima))
        {
            OGNI_DESCRIZIONI_MS
        } else {
            OGNI_STATO_MS
        };
        let dovuto = self.stato_mandato.saturating_add(ogni);
        if !forzato && adesso < dovuto {
            self.stato_in_sospeso = true;
            self.stato_dovuto = dovuto;
            return;
        }
        self.versione = self.versione.saturating_add(1);
        let stato = voci::stato_sessione(self.versione, persone, istantanea, &self.servite);
        let messaggio = Messaggio::Stato(stato);
        for membro in membri {
            self.manda(membro, &messaggio);
        }
        self.impronta_stato = Some(impronta);
        self.stato_mandato = adesso;
        self.stato_in_sospeso = false;
    }

    /// Chi ospita: le descrizioni sono cambiate senza che il lettore lo
    /// sappia (un descrittore di copia): lo stato partirà col giro lento,
    /// salvo che uno più urgente sia già in sospeso.
    pub(super) fn descrizioni_da_mandare(&mut self) {
        if !matches!(self.s.fase, Fase::Ospita(_)) {
            return;
        }
        let dovuto = self.stato_mandato.saturating_add(OGNI_DESCRIZIONI_MS);
        if !self.stato_in_sospeso || self.stato_dovuto > dovuto {
            self.stato_dovuto = dovuto;
        }
        self.stato_in_sospeso = true;
    }

    /// Uno stato in sospeso che non è solo di descrizioni: chi segue potrebbe
    /// non conoscere ancora la voce che l'ancora nominerebbe.
    fn stato_strutturale_in_sospeso(&self) -> bool {
        self.stato_in_sospeso
            && self.stato_dovuto < self.stato_mandato.saturating_add(OGNI_DESCRIZIONI_MS)
    }

    pub(super) fn manda_ancora(&mut self) {
        self.prossima_ancora = ora().saturating_add(OGNI_ANCORA_MS);
        let membri = fasi::connessioni_dei_membri(&self.s);
        if membri.is_empty() {
            return;
        }
        let Some(a) = ponte::ancora_di_questo(&self.app) else {
            // Il motore di chi ospita non ha niente in mano — un'apertura da
            // un disco di rete andata male, la coda finita — ma la voce
            // corrente c'è: «ferma» detto esplicitamente. Senza ancora, chi
            // segue continuava a far avanzare l'ultima che aveva e suonava da
            // solo, mentre l'host taceva.
            if let Some(corrente) = self.corrente.filter(|uid| self.servite.ha(*uid))
                && ponte::motore_senza_brano(&self.app)
            {
                let ferma = Messaggio::Ancora(AncoraSulFilo {
                    etichetta: corrente,
                    posizione_us: 0,
                    udito_ns: rymembra_play::tempo::adesso_ns(),
                    in_corso: false,
                    programmata: false,
                });
                for membro in membri {
                    self.manda(membro, &ferma);
                }
            }
            return;
        };
        // Un'ancora per una voce che nessuno stato ha ancora descritto non
        // serve a chi segue: non saprebbe aprirla, e il suo regolatore
        // insisterebbe a riaprire una voce che non ha
        // (`playback.sourceUnavailable`, visto nella prima prova fra due PC).
        // Arriva appena la voce è pronta, ed è la prima che si prepara.
        if a.etichetta != ponte::UID_CALIBRAZIONE && !self.servite.ha(a.etichetta) {
            return;
        }
        // Con uno stato in sospeso, un'ancora che nomina una voce che l'ultimo
        // stato mandato non dava come corrente aspetta quello stato: chi segue
        // non la conoscerebbe ancora. Le altre partono subito — una pausa, una
        // ripresa, un salto nella stessa voce — perché arrivavano dietro allo
        // stato, che chi segue impiega un po' a leggere: una pausa si sentiva
        // mezzo secondo dopo.
        let gia_detta = self
            .impronta_stato
            .as_ref()
            .is_some_and(|i| i.corrente == Some(a.etichetta) && i.corrente_descritta);
        if self.stato_strutturale_in_sospeso()
            && !gia_detta
            && a.etichetta != ponte::UID_CALIBRAZIONE
        {
            return;
        }
        let messaggio = Messaggio::Ancora(AncoraSulFilo {
            etichetta: a.etichetta,
            posizione_us: a.posizione_us,
            udito_ns: a.udito_ns,
            in_corso: a.in_corso,
            programmata: a.programmata,
        });
        for membro in membri {
            self.manda(membro, &messaggio);
        }
    }

    /// Chi ospita: «di Marco» sulle voci dei membri che sono in coda.
    fn proprietari_di_chi_ospita(&mut self, istantanea: &ponte::Istantanea) {
        let persone = fasi::persone_di_qui(&self.s, &self.nome);
        let di: HashMap<u64, u32> = self.servite.altrui().collect();
        let Some(dal) = &self.dal_filo else {
            self.proprietari.clear();
            return;
        };
        self.proprietari = proprietari(&istantanea.coda, &persone, |uid| {
            let membro = di.get(&uid)?;
            let maniglia = dal.registrate.get(&uid)?.1?;
            Some((maniglia, *membro))
        });
    }

    pub(super) fn fotografa_host(&self) {
        let persone = fasi::persone_di_qui(&self.s, &self.nome);
        let foto = nel_lettore_di_chi_ospita(
            &self.nome,
            persone.len(),
            proprietario_di_chi_ospita(self.corrente, &self.servite, &persone),
            Dettagli {
                agganciato: true,
                locale: self.locale,
                calibrazione: self.calibrazione,
                revisione: self.revisione_voci,
            },
        );
        self.metti_nel_lettore(Some(foto));
    }

    // ── fase 2: chi ospita esegue i gesti dei membri ────────────────────────

    /// Un gesto di un membro: all'esecutore, che lo fa negli stessi percorsi
    /// dei gesti di chi ospita e poi fa partire la risposta. Qui si prepara
    /// solo quel che è della sessione (le voci di un `Aggiungi`), che non
    /// aspetta né il disco né il lettore.
    pub(super) fn esegui_comando(
        &mut self,
        da: IdConnessione,
        membro: u32,
        comando: u32,
        azione: Azione,
    ) {
        let lavoro = match azione {
            Azione::Aggiungi { voci, dove, suona } => {
                match self.aggiungi_del_membro(membro, voci, dove, suona) {
                    Ok(lavoro) => lavoro,
                    Err(codice) => {
                        self.ricevi(Posta::Eseguito {
                            risposta: Some((da, comando)),
                            membro,
                            codice: Some(codice),
                            nome: "aggiungi",
                            attesa: Duration::ZERO,
                            durata: Duration::ZERO,
                            fuori: Vec::new(),
                            sessione: self.sessione,
                        });
                        return;
                    }
                }
            }
            altra => Lavoro::Azione(altra),
        };
        self.affida(Incarico {
            lavoro,
            risposta: Some((da, comando)),
            membro,
            arrivato: Instant::now(),
            sessione: self.sessione,
        });
    }

    /// Un incarico all'esecutore. Se l'esecutore non c'è, il gesto si rifiuta
    /// subito: meglio un no che un membro ad aspettare.
    fn affida(&mut self, incarico: Incarico) {
        let rimandato = match &self.esecutore {
            Some(esecutore) => esecutore.send(incarico).err().map(|err| err.0),
            None => Some(incarico),
        };
        if let Some(incarico) = rimandato {
            self.ricevi(Posta::Eseguito {
                risposta: incarico.risposta,
                membro: incarico.membro,
                codice: Some(ErrorCodeKind::InsiemeRifiutato.code().to_owned()),
                nome: incarico.lavoro.nome(),
                attesa: Duration::ZERO,
                durata: Duration::ZERO,
                fuori: Vec::new(),
                sessione: incarico.sessione,
            });
        }
    }

    /// `Aggiungi` di un membro: le voci si rinumerano e si registrano coi loro
    /// gettoni inoltrati al membro. Metterle in coda, e farne partire la prima,
    /// tocca all'esecutore: è il lavoro che apre.
    fn aggiungi_del_membro(
        &mut self,
        membro: u32,
        voci: Vec<VoceSulFilo>,
        dove: Dove,
        suona: bool,
    ) -> Result<Lavoro, String> {
        let rifiutato = || ErrorCodeKind::InsiemeRifiutato.code().to_owned();
        let (Some(chiave), Some(_)) = (self.chiave_del_membro(membro), &self.dal_filo) else {
            return Err(rifiutato());
        };
        let mut nuove = Vec::with_capacity(voci.len());
        let mut servizi = Vec::new();
        let mut altrui = false;
        for mut v in voci.into_iter().take(MASSIMO_AGGIUNTE) {
            // Gli `uid` li sceglie l'host: quelli del membro valgono solo per
            // lui, e due membri sceglierebbero gli stessi. `di` lo mette
            // l'host: un membro non può dire che una voce è di un altro.
            self.uid_dei_membri = self.uid_dei_membri.saturating_add(1);
            v.uid = UID_DEI_MEMBRI.saturating_add(self.uid_dei_membri);
            v.di = membro;
            match self.servite.ospita_voce(v.clone()) {
                Ok(s) => {
                    servizi.extend(s);
                    nuove.push(v);
                }
                Err(voci::GettoneAltrui) => {
                    altrui = true;
                    break;
                }
            }
        }
        let uids: Vec<u64> = nuove.iter().map(|v| v.uid).collect();
        // Gli inoltri, uno per uno: il trasporto rifiuta un gettone già in
        // uso, e allora l'aggiunta intera si rifiuta (un membro che sceglie il
        // gettone di un brano altrui non deve farsene dare i byte).
        let mut respinti = HashSet::new();
        if !altrui && let Some(accesa) = &self.rete {
            for servizio in servizi {
                if let Servizio::Inoltra(gettone, _) = servizio
                    && !accesa.rete.inoltra(gettone, chiave)
                {
                    respinti.insert(gettone);
                }
            }
        }
        if altrui || !respinti.is_empty() {
            nota!("[insieme] aggiunta del membro {membro} rifiutata: gettone già in uso");
            self.togli_l_aggiunta(&uids, &respinti);
            return Err(rifiutato());
        }
        self.registra_dal_filo(nuove);
        let registrate: Vec<(u64, i64)> = self
            .dal_filo
            .as_ref()
            .map(|dal| {
                uids.iter()
                    .filter_map(|uid| {
                        let maniglia = dal.registrate.get(uid).and_then(|(_, m)| *m)?;
                        Some((*uid, maniglia))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if registrate.is_empty() {
            self.togli_l_aggiunta(&uids, &HashSet::new());
            return Err(rifiutato());
        }
        nota!("[insieme] il membro {membro} aggiunge {} voci", uids.len());
        // La prima a suonare è la prima che va in coda: `esito_dell_aggiunta`
        // la fa partire solo se è entrata.
        let prima = registrate.first().map(|(uid, _)| *uid);
        Ok(Lavoro::Accoda {
            voci: registrate,
            dopo: dove == Dove::Dopo,
            suona: prima.filter(|_| suona),
        })
    }

    /// Un'aggiunta rifiutata a metà: le voci già contate si dimenticano, e i
    /// loro inoltri si revocano — tranne quelli che il trasporto ha respinto,
    /// che sono di qualcun altro e restano suoi.
    pub(super) fn togli_l_aggiunta(&mut self, uids: &[u64], respinti: &HashSet<Gettone>) {
        let revoche: Vec<Servizio> = self
            .servite
            .dimentica(uids)
            .into_iter()
            .filter(|s| !matches!(s, Servizio::Revoca(g) if respinti.contains(g)))
            .collect();
        self.esegui_servizi(revoche, false);
        if let Some(dal) = &mut self.dal_filo {
            for uid in uids {
                dal.registrate.remove(uid);
            }
        }
        self.dimentica_le_copertine(uids);
    }

    /// Chi ospita: le copertine di queste voci dei membri, che non ci sono
    /// più; i byte che nessuna voce usa si liberano (vedi
    /// [`voci::Copertine::dimentica`]), e i loro gettoni escono dalle chieste,
    /// così una voce che torna li richiede.
    fn dimentica_le_copertine(&mut self, uids: &[u64]) {
        if uids.is_empty() {
            return;
        }
        let liberati = self
            .comune
            .copertine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .dimentica(uids);
        if let Some(dal) = &mut self.dal_filo {
            for gettone in liberati {
                dal.chieste.remove(&gettone);
            }
        }
    }

    /// Chi ospita: le chiavi che possono leggere i gettoni sono quelle dei
    /// membri della sessione, e solo loro. Si rifà a ogni ingresso e uscita
    /// (con `PersoneCambiate`), e diventa vuota quando non si ospita: di
    /// partenza il trasporto non lascia leggere a nessuno.
    pub(super) fn aggiorna_ammessi(&self) {
        let Some(accesa) = &self.rete else {
            return;
        };
        let chiavi: Vec<ChiavePubblica> = fasi::connessioni_dei_membri(&self.s)
            .iter()
            .filter_map(|conn| self.capi.get(conn).map(|(_, chiave)| *chiave))
            .collect();
        accesa.rete.ammessi(chiavi);
    }

    /// Un membro non c'è più: le sue voci escono dalla coda, e i suoi gettoni
    /// inoltrati si dimenticano.
    pub(super) fn membro_uscito(&mut self, membro: u32) {
        let uids = self.servite.di_membro(membro);
        if uids.is_empty() {
            return;
        }
        // Togliere la voce che suona vuol dire aprire la dopo: all'esecutore.
        self.affida(Incarico {
            lavoro: Lavoro::Togli(uids.clone()),
            risposta: None,
            membro,
            arrivato: Instant::now(),
            sessione: self.sessione,
        });
        let servizi = self.servite.dimentica(&uids);
        self.esegui_servizi(servizi, false);
        if let Some(dal) = &mut self.dal_filo {
            for uid in &uids {
                dal.registrate.remove(uid);
            }
        }
        self.dimentica_le_copertine(&uids);
        nota!(
            "[insieme] il membro {membro} è uscito: {} voci tolte",
            uids.len()
        );
    }

    // ── chi ospita: le voci preparate dal loro filo ─────────────────────────

    /// Un blocco di voci preparate: le pronte si descrivono e si servono, le
    /// mancate aspettano il loro tentativo dopo. Poi lo stato si ripubblica,
    /// con le voci nuove dentro.
    pub(super) fn preparate(
        &mut self,
        sessione: u64,
        esiti: Vec<(u64, Result<ponte::BranoDiQuesto, ponte::Mancanza>)>,
    ) {
        if sessione != self.sessione || !matches!(self.s.fase, Fase::Ospita(_)) {
            return;
        }
        let adesso = ora();
        let mut servizi = Vec::new();
        let mut nuovo_gettone = Gettone::nuovo;
        for (uid, brano) in esiti {
            let brano = match brano {
                Ok(brano) => brano,
                Err(mancanza) => {
                    // Solo un disco che non ha risposto può rispondere la
                    // volta dopo: il resto si lascia subito.
                    self.preparazione
                        .mancata(uid, adesso, preparazione::definitiva(mancanza));
                    continue;
                }
            };
            match self.servite.aggiungi(uid, &brano, &mut nuovo_gettone) {
                Ok(mut nuovi) => {
                    servizi.append(&mut nuovi);
                    if self.servite.ha(uid) {
                        self.preparazione.arrivata(uid);
                    } else {
                        // Un brano che non si può mandare (niente file e niente
                        // riferimento): non cambierà riprovando.
                        self.preparazione.mancata(uid, adesso, true);
                    }
                }
                Err(err) => {
                    nota!(
                        "[insieme] gettone non creato codice={}",
                        err.code().kind().code()
                    );
                    self.preparazione.mancata(uid, adesso, false);
                }
            }
        }
        self.esegui_servizi(servizi, false);
        self.campanello();
    }
}

/// Chi ospita: il filo che prepara le voci di questo PC, fuori dal filo della
/// sessione. Vedi `preparazione`.
///
/// Un blocco per chiamata a `ponte::voci_di_questo`: il registro guardato una
/// volta, la libreria con una presa sola, il disco su un filo solo con una
/// scadenza per tutto il blocco. Il costo è suo, non delle ancore: ogni blocco
/// torna appena è fatto, e lo stato si ripubblica a ogni blocco.
pub(super) fn avvia_il_preparatore(
    app: AppHandle,
    posta: Sender<Posta>,
) -> Option<Sender<DaPreparare>> {
    let (coda, orecchio) = channel::<DaPreparare>();
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme-preparazione".to_owned())
        .spawn(move || {
            let mut in_fila: VecDeque<DaPreparare> = VecDeque::new();
            loop {
                if in_fila.is_empty() {
                    match orecchio.recv() {
                        Ok(richiesta) => in_fila.push_back(richiesta),
                        Err(_) => return,
                    }
                }
                in_fila.extend(orecchio.try_iter());
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let Some((sessione, uids)) = prossima_da_preparare(&mut in_fila) else {
                    continue;
                };
                let esiti = ponte::voci_di_questo(&app, &uids);
                if posta.send(Posta::Preparate { sessione, esiti }).is_err() {
                    return;
                }
            }
        });
    match avviato {
        Ok(_) => Some(coda),
        Err(err) => {
            nota!("[insieme] il filo della preparazione non parte: {err}");
            None
        }
    }
}

/// Una richiesta al filo che prepara le voci di chi ospita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DaPreparare {
    /// Un blocco di voci, nell'ordine in cui serviranno.
    Blocco { sessione: u64, uids: Vec<u64> },
    /// La voce che suona: davanti a tutto.
    Subito { sessione: u64, uid: u64 },
}

impl DaPreparare {
    const fn sessione(&self) -> u64 {
        match self {
            Self::Blocco { sessione, .. } | Self::Subito { sessione, .. } => *sessione,
        }
    }
}

/// Che cosa preparare adesso fra le richieste in fila, che toglie: le
/// richieste di una sessione vecchia si buttano (i suoi `uid` sono altre voci
/// in quella nuova), e una voce `Subito` passa davanti ai blocchi. Per il
/// resto, l'ordine d'arrivo.
pub(super) fn prossima_da_preparare(
    in_fila: &mut VecDeque<DaPreparare>,
) -> Option<(u64, Vec<u64>)> {
    let ultima = in_fila.iter().map(DaPreparare::sessione).max()?;
    in_fila.retain(|richiesta| richiesta.sessione() == ultima);
    let posto = in_fila
        .iter()
        .position(|richiesta| matches!(richiesta, DaPreparare::Subito { .. }))
        .unwrap_or(0);
    match in_fila.remove(posto)? {
        DaPreparare::Blocco { sessione, uids } => Some((sessione, uids)),
        DaPreparare::Subito { sessione, uid } => Some((sessione, vec![uid])),
    }
}

/// Un lavoro dell'esecutore: vedi [`avvia_l_esecutore`].
pub(super) enum Lavoro {
    /// Un gesto di un membro (non `Aggiungi`).
    Azione(Azione),
    /// Le voci di un `Aggiungi`, già registrate, in coda — `uid` e maniglia,
    /// nell'ordine — e la prima a suonare, se il membro l'ha chiesto.
    Accoda {
        voci: Vec<(u64, i64)>,
        dopo: bool,
        suona: Option<u64>,
    },
    /// Le voci di un membro uscito, fuori dalla coda.
    Togli(Vec<u64>),
}

impl Lavoro {
    /// Il nome per il diario.
    const fn nome(&self) -> &'static str {
        match self {
            Self::Azione(azione) => gesti::nome_dell_azione(azione),
            Self::Accoda { .. } => "aggiungi",
            Self::Togli(_) => "togli le voci di chi esce",
        }
    }
}

/// Un lavoro, e a chi rispondere.
pub(super) struct Incarico {
    lavoro: Lavoro,
    risposta: Option<(IdConnessione, u32)>,
    membro: u32,
    /// Quando è arrivato al filo della sessione: l'attesa in coda si conta da
    /// qui.
    arrivato: Instant,
    /// Di che sessione è: vedi `Filo::sessione_viva`.
    sessione: u64,
}

/// Chi ospita: il filo che esegue i gesti dei membri.
///
/// Sono i comandi della finestra — `pausa`, `prossimo`, `coda_vai`… — e
/// possono aprire una voce: da un disco di rete fino a cinque secondi, dalla
/// rete di un membro fino a quindici. Sul filo della sessione vorrebbe dire
/// niente ancore e niente stato per tutto quel tempo, e una pausa dell'host che
/// non arriva a nessuno. Uno alla volta, nell'ordine d'arrivo: per pausa e
/// salto vince l'ultimo arrivato.
pub(super) fn avvia_l_esecutore(
    app: AppHandle,
    posta: Sender<Posta>,
    sessione_viva: Arc<AtomicU64>,
) -> Option<Sender<Incarico>> {
    let (coda, orecchio) = channel::<Incarico>();
    let avviato = std::thread::Builder::new()
        .name("rymembra-insieme-esecutore".to_owned())
        .spawn(move || {
            for incarico in orecchio {
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let attesa = incarico.arrivato.elapsed();
                let inizio = Instant::now();
                let nome = incarico.lavoro.nome();
                let (codice, fuori) = if incarico.sessione == sessione_viva.load(Ordering::Acquire)
                {
                    crate::stato::sorvegliato("rymembra-insieme-esecutore", || {
                        esegui(&app, incarico.lavoro)
                    })
                    .unwrap_or_else(|| {
                        (
                            Some(ErrorCodeKind::InsiemeRifiutato.code().to_owned()),
                            Vec::new(),
                        )
                    })
                } else {
                    // Di una sessione finita: non si fa, e il membro (se c'è
                    // ancora) sa che non è stato fatto.
                    (
                        Some(ErrorCodeKind::InsiemeSuperato.code().to_owned()),
                        Vec::new(),
                    )
                };
                let fatto = Posta::Eseguito {
                    risposta: incarico.risposta,
                    membro: incarico.membro,
                    codice,
                    nome,
                    attesa,
                    durata: inizio.elapsed(),
                    fuori,
                    sessione: incarico.sessione,
                };
                if posta.send(fatto).is_err() {
                    return;
                }
            }
        });
    match avviato {
        Ok(_) => Some(coda),
        Err(err) => {
            nota!("[insieme] il filo dei gesti dei membri non parte: {err}");
            None
        }
    }
}

/// Un lavoro dell'esecutore, coi comandi del lettore così come sono: la stessa
/// strada dei gesti di chi ospita, e del tasto multimediale. Il codice è
/// `None` se è andato (o non c'era niente da fare), altrimenti quello del
/// rifiuto; accanto, gli `uid` delle voci di un `Aggiungi` rimaste fuori dalla
/// coda, che la sessione deve dimenticare (vedi [`esito_dell_aggiunta`]).
fn esegui(app: &AppHandle, lavoro: Lavoro) -> (Option<String>, Vec<u64>) {
    let codice_di = |err: &AppError| codice_sul_filo(err.code().kind().code());
    match lavoro {
        Lavoro::Azione(azione) => (fai(app, &azione), Vec::new()),
        Lavoro::Accoda { voci, dopo, suona } => {
            let maniglie: Vec<i64> = voci.iter().map(|(_, maniglia)| *maniglia).collect();
            let (entrate, esito) = ponte::accoda_maniglie(app, &maniglie, dopo);
            esito_dell_aggiunta(
                &voci,
                entrate,
                esito.map_err(|err| codice_di(&err)),
                suona,
                |uid| fai(app, &Azione::Vai { uid }),
            )
        }
        Lavoro::Togli(uids) => (
            ponte::togli_voci(app, &uids)
                .err()
                .map(|err| codice_di(&err)),
            Vec::new(),
        ),
    }
}

/// Com'è finito l'`Aggiungi` di un membro, sapendo quante delle sue `voci`
/// sono entrate in coda (dalla prima) e l'esito dell'accoda: il codice da
/// mandargli, e gli `uid` rimasti fuori.
///
/// # La coda piena
///
/// La coda di sessione ha un tetto ([`rymembra_insieme::messaggi::MASSIMO_VOCI`]),
/// e una coda che ci arriva — «Tutti i brani», una playlist lunga — è normale.
/// Prima l'esecutore rispondeva «fatto» a un'aggiunta rimasta tutta fuori: il
/// membro non vedeva niente, la fascia rossa la vedeva chi ospita, e con un
/// doppio clic il «vai» sulla voce che non c'era diceva al membro che la coda
/// era cambiata nel frattempo. Adesso: se ne è rimasta fuori anche una, il
/// membro riceve `insieme.codaPiena`, la sua frase («quelli in più sono
/// rimasti fuori»); la prima suona solo se è entrata davvero.
pub(super) fn esito_dell_aggiunta(
    voci: &[(u64, i64)],
    entrate: usize,
    esito: Result<(), String>,
    suona: Option<u64>,
    vai: impl FnOnce(u64) -> Option<String>,
) -> (Option<String>, Vec<u64>) {
    let fuori: Vec<u64> = voci
        .get(entrate..)
        .unwrap_or_default()
        .iter()
        .map(|(uid, _)| *uid)
        .collect();
    let codice = match esito {
        Err(codice) => Some(codice),
        Ok(()) => {
            let suonata = suona.filter(|_| entrate > 0).and_then(vai);
            if fuori.is_empty() {
                suonata
            } else {
                Some(ErrorCodeKind::InsiemeCodaPiena.code().to_owned())
            }
        }
    };
    (codice, fuori)
}

/// Un'azione diversa da `Aggiungi`, con la coda di adesso: vedi
/// [`gesti::piano`].
fn fai(app: &AppHandle, azione: &Azione) -> Option<String> {
    let rifiutato = || Some(ErrorCodeKind::InsiemeRifiutato.code().to_owned());
    let Some(adesso) = ponte::istantanea(app) else {
        return rifiutato();
    };
    let Some(stato) = app.try_state::<crate::riproduzione::StatoLettore>() else {
        return rifiutato();
    };
    use crate::riproduzione as r;
    let dal_ponte =
        |esito: Result<(), AppError>| esito.map_err(|err| err.code().kind().code().to_owned());
    let dalla_finestra = |esito: crate::errore::Esito<()>| esito.map_err(|err| err.code);
    let esito: Result<(), String> = match gesti::piano(azione, &adesso) {
        Passo::Niente => Ok(()),
        Passo::Superato => Err(ErrorCodeKind::InsiemeSuperato.code().to_owned()),
        Passo::Sconosciuto => Err(ErrorCodeKind::InsiemeGestoNonDisponibile.code().to_owned()),
        Passo::Ripeti(modo) => dal_ponte(r::imposta_ripeti(app, modo)),
        // La posizione la trova il lettore sotto lo stesso lucchetto che la
        // usa: una voce che nel frattempo non c'è più non si suona.
        Passo::Vai(uid) => dal_ponte(ponte::suona_voce(app, uid).map(|_| ())),
        Passo::Togli(uid) => dal_ponte(ponte::togli_voci(app, &[uid])),
        Passo::Pausa => dalla_finestra(r::pausa(app.clone(), stato)),
        Passo::Riprendi => dalla_finestra(r::riprendi_ora(app.clone(), stato)),
        Passo::Prossimo => dalla_finestra(r::prossimo_ora(app.clone(), stato)),
        Passo::Precedente => dalla_finestra(r::precedente_ora(app.clone(), stato)),
        Passo::VaiA(ms) => dalla_finestra(r::vai_a(app.clone(), stato, ms)),
        Passo::Riordina { da, a } => dalla_finestra(r::coda_riordina(app.clone(), stato, da, a)),
    };
    esito.err().map(|codice| codice_sul_filo(&codice))
}

/// Un codice per l'`Esito` sul filo: solo i codici della sessione viaggiano
/// (vedi `messaggi`); ogni altro guasto dell'host è, per chi l'ha chiesto, un
/// gesto rifiutato.
pub(super) fn codice_sul_filo(codice: &str) -> String {
    let della_sessione = ErrorCodeKind::ALL.iter().any(|kind| {
        kind.domain() == rymembra_domain::errors::Domain::Insieme && kind.code() == codice
    });
    if della_sessione {
        codice.to_owned()
    } else {
        ErrorCodeKind::InsiemeRifiutato.code().to_owned()
    }
}

/// Il lettore di un file del cloud da servire agli altri PC della sessione,
/// se il file è del cloud.
///
/// I byte vengono dalla sorgente del servizio dentro la cache — la stessa
/// della riproduzione — aperta alla prima richiesta e tenuta per le
/// successive. A ogni lettura si ricontrolla nello specchio che il file sia
/// ancora quello del gettone (dimensione e data), come `servente::leggi_file`
/// fa sul disco: un brano cambiato sul servizio a metà sessione non si legge a
/// pezzi di due versioni.
fn lettore_del_cloud(
    app: &AppHandle,
    file: &rymembra_insieme::FileServito,
) -> Option<rymembra_insieme::Lettore> {
    let percorso = file.percorso.to_string_lossy().into_owned();
    if !rymembra_domain::nuvola::e_nuvola(&percorso) {
        return None;
    }
    let archivio = crate::nuvola_musica::archivio(app)?;
    let atteso = (file.dimensione, file.modificato_ms);
    let aperta: std::sync::Mutex<Option<Box<dyn rymembra_net::flusso::Sorgente>>> =
        std::sync::Mutex::new(None);
    Some(Arc::new(move |da: u64, quanti: u64| {
        use rymembra_domain::errors::ErrorCodeKind;
        match archivio.voce_di(&percorso) {
            Some((_, voce)) if (voce.dimensione, voce.modificato_ms) == atteso => {}
            Some(_) | None => return Err(ErrorCodeKind::InsiemeFileCambiato),
        }
        let mut aperta = aperta
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if aperta.is_none() {
            let (sorgente, _) = archivio
                .apri_brano(&percorso)
                .map_err(|_| ErrorCodeKind::InsiemeIrraggiungibile)?;
            *aperta = Some(sorgente);
        }
        let sorgente = aperta
            .as_ref()
            .ok_or(ErrorCodeKind::InsiemeIrraggiungibile)?;
        let pezzo = sorgente
            .pezzo(da, quanti)
            .map_err(|_| ErrorCodeKind::InsiemeIrraggiungibile)?;
        Ok((pezzo.byte, pezzo.totale.unwrap_or(atteso.0)))
    }))
}
