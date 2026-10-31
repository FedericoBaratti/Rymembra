//! Chi segue: l'ingresso nella sessione dell'altro, lo specchio della sua coda
//! e l'aggancio, i gesti e i brani propri mandati all'host.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::channel;
use std::sync::{Arc, PoisonError};
use std::time::Instant;

use rymembra_domain::errors::ErrorCodeKind;
use rymembra_insieme::messaggi::{
    AncoraSulFilo, Azione, Diritti, Dove, Messaggio, SorgenteSulFilo, StatoSessione, VoceSulFilo,
};
use rymembra_insieme::{Gettone, IdConnessione};
use rymembra_play::sincronia::regolatore::DallOspite;

use super::{
    ATTESA_DESCRITTORI, ATTESA_DESCRITTORI_SUONA, ATTESA_ECO_MS, Aggiunta, DalFilo, Dettagli, Filo,
    GESTI_IN_VOLO, Mandata, OGNI_AGGANCIO_MS, Seguendo, chiave_per_la_finestra,
    nel_lettore_di_chi_segue, ora, proprietari,
};
use crate::insieme::fasi::{Evento, Fase};
use crate::insieme::gesti;
use crate::insieme::segui::{self, Condivisa};
use crate::insieme::strada::Ruolo;
use crate::insieme::voci::{self, Servite};
use crate::nota;
use crate::riproduzione::insieme as ponte;

impl Filo {
    // ── chi segue ───────────────────────────────────────────────────────────

    pub(super) fn entra_come_membro(&mut self, host: IdConnessione) {
        let Some((indirizzo, chiave)) = self.capi.get(&host).copied() else {
            self.nutri(Evento::LettoreRifiuta);
            return;
        };
        // Il ruolo prima: vedi il `//!` di `filo`. I diritti sono quelli dell'`Ammesso`;
        // senza una sessione da seguire, il più stretto.
        let diritti = match &self.s.fase {
            Fase::Membro(m) => m.diritti,
            _ => Diritti::Ascolta,
        };
        self.ruolo(Ruolo::Membro(diritti));
        if let Err(err) = ponte::entra_come_membro(&self.app) {
            nota!(
                "[insieme] il lettore non entra come membro codice={}",
                err.code().kind().code()
            );
            // Per chi aspettava di entrare il lettore che non entra è un
            // ingresso andato male: `rifiutato` («nessuna risposta, o la
            // risposta è stata no») è la frase più vicina che la finestra ha.
            if let Some(chiave) =
                chiave_per_la_finestra(&err, Some(ErrorCodeKind::InsiemeRifiutato))
            {
                self.avvisa(chiave, None);
            }
            self.nutri(Evento::LettoreRifiuta);
            return;
        }
        let Some(lettore) = self
            .rete
            .as_ref()
            .map(|a| a.rete.lettore(indirizzo, chiave))
        else {
            self.nutri(Evento::LettoreRifiuta);
            return;
        };
        let condivisa = Arc::new(Condivisa::nuova());
        let (sveglia, orecchio) = channel();
        let filo = match segui::avvia(self.app.clone(), Arc::clone(&condivisa), orecchio) {
            Ok(filo) => filo,
            Err(err) => {
                nota!("[insieme] il filo della sincronia non parte: {err}");
                self.nutri(Evento::LettoreRifiuta);
                return;
            }
        };
        self.dal_filo = Some(DalFilo::nuovo(lettore, self.posta.clone()));
        self.seguendo = Some(Seguendo {
            host,
            indirizzo,
            chiave,
            condivisa,
            sveglia,
            filo: Some(filo),
            corrente: None,
            proprie: Servite::default(),
            uid_proprio: 0,
            offerta: false,
            prossimo_comando: 0,
            in_volo: Vec::new(),
            in_attesa: Vec::new(),
            mandate: HashMap::new(),
        });
        nota!("[insieme] si segue la connessione #{}", host.0);
        self.fotografa_membro();
    }

    pub(super) fn specchia(&mut self, stato: StatoSessione) {
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let io = m.io;
        let (Some(_), Some(dal)) = (&self.seguendo, &self.dal_filo) else {
            return;
        };
        let uids: Vec<u64> = stato.coda.iter().map(|v| v.uid).collect();
        let tenere: HashSet<u64> = uids.iter().copied().collect();
        let nuove: Vec<VoceSulFilo> = stato
            .coda
            .iter()
            .filter(|v| dal.registrate.get(&v.uid).map(|(r, _)| r) != Some(*v))
            .cloned()
            .collect();
        self.registra_dal_filo(nuove);
        let liberati = self
            .comune
            .copertine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tieni(&tenere);
        if let Some(dal) = &mut self.dal_filo {
            dal.registrate.retain(|uid, _| tenere.contains(uid));
            // Una voce che rientra nello specchio col suo gettone deve
            // poter richiedere la copertina liberata qui.
            for gettone in liberati {
                dal.chieste.remove(&gettone);
            }
        }
        self.tieni_le_proprie(&stato);
        if let Some(seguendo) = &mut self.seguendo {
            seguendo.corrente = stato.corrente;
        }
        ponte::specchia(
            &self.app,
            &uids,
            stato.corrente,
            stato.in_pausa,
            voci::ripeti_da_filo(stato.ripeti),
        );
        // «di Marco»: le voci che un altro **membro** ha aggiunto. Non quelle
        // di chi ospita, che sono la coda stessa: mille righe tutte «di A»
        // non direbbero niente che la fascia della sessione non dica già, e
        // viaggerebbero verso la finestra a ogni stato. Non le proprie, che
        // sono di chi guarda.
        self.proprietari = match &self.dal_filo {
            Some(dal) => proprietari(&uids, &stato.persone, |uid| {
                let (v, maniglia) = dal.registrate.get(&uid)?;
                if v.di == io || v.di == 0 {
                    return None;
                }
                Some(((*maniglia)?, v.di))
            }),
            None => Vec::new(),
        };
        self.locale = ponte::voce_corrente_locale(&self.app);
        self.fotografa_membro();
    }

    pub(super) fn segui(&self, ancora: AncoraSulFilo) {
        let Some(seguendo) = &self.seguendo else {
            return;
        };
        let cambia = seguendo.condivisa.ancora(DallOspite {
            etichetta: ancora.etichetta,
            posizione_us: ancora.posizione_us,
            udito_ospite_ns: ancora.udito_ns,
            in_corso: ancora.in_corso,
            programmata: ancora.programmata,
        });
        // Una pausa o un brano nuovo non aspettano il colpo dopo.
        if cambia {
            let _ = seguendo.sveglia.send(());
        }
    }

    pub(super) fn fotografa_membro(&self) {
        let (Fase::Membro(m), Some(seguendo), Some(dal)) =
            (&self.s.fase, &self.seguendo, &self.dal_filo)
        else {
            return;
        };
        let foto = nel_lettore_di_chi_segue(
            &self.nome_della_connessione(m.host),
            &m.persone,
            m.io,
            m.diritti,
            seguendo
                .corrente
                .and_then(|c| dal.registrate.get(&c))
                .map(|(v, _)| v),
            Dettagli {
                agganciato: seguendo.condivisa.aggancio().agganciato,
                locale: self.locale,
                calibrazione: m.calibrazione,
                revisione: self.revisione_voci,
            },
        );
        self.metti_nel_lettore(Some(foto));
    }

    /// L'aggancio alla finestra: subito quando cambia «agganciato», al più
    /// una volta al secondo per il resto. Allo stesso passo si riguarda se la
    /// voce che suona si legge da un file di qui (fase 3): la copia la trova
    /// il lettore mentre apre, e nessuno lo dice alla sessione.
    pub(super) fn guarda_l_aggancio(&mut self, adesso: u64) {
        let Some(seguendo) = &self.seguendo else {
            return;
        };
        let aggancio = seguendo.condivisa.aggancio();
        let cambiato = self
            .aggancio_mostrato
            .is_none_or(|prima| prima.agganciato != aggancio.agganciato);
        if cambiato || adesso >= self.aggancio_dal.saturating_add(OGNI_AGGANCIO_MS) {
            self.aggancio_mostrato = Some(aggancio);
            self.aggancio_dal = adesso;
            let locale = ponte::voce_corrente_locale(&self.app);
            if cambiato || locale != self.locale {
                self.locale = locale;
                self.fotografa_membro();
            }
        }
    }

    // ── fase 2: chi segue manda i suoi gesti ────────────────────────────────

    /// Azioni già tradotte, all'host, ognuna col suo numero. Non si aspetta
    /// l'`Esito`: arriva da sé, e se dice di no diventa un avviso.
    pub(super) fn manda_gesti(&mut self, azioni: Vec<Azione>) {
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let host = m.host;
        for azione in azioni {
            let Some(seguendo) = &mut self.seguendo else {
                return;
            };
            seguendo.prossimo_comando = seguendo.prossimo_comando.wrapping_add(1);
            let id = seguendo.prossimo_comando;
            if seguendo.in_volo.len() >= GESTI_IN_VOLO {
                seguendo.in_volo.remove(0);
            }
            seguendo
                .in_volo
                .push((id, Instant::now(), gesti::nome_dell_azione(&azione)));
            self.manda(host, &Messaggio::Comando { id, azione });
        }
    }

    /// Brani della libreria di questo PC da aggiungere alla sessione: gettoni
    /// e copertine si servono subito, il descrittore (fase 3) si aspetta un
    /// poco, poi `Aggiungi` parte. Vedi [`ATTESA_DESCRITTORI`].
    pub(super) fn aggiungi_proprie(
        &mut self,
        brani: Vec<ponte::BranoDiQuesto>,
        dove: Dove,
        suona: bool,
    ) {
        let Fase::Membro(_) = &self.s.fase else {
            return;
        };
        let Some(seguendo) = &mut self.seguendo else {
            return;
        };
        let mut uids = Vec::new();
        let mut servizi = Vec::new();
        let mut nuovo_gettone = Gettone::nuovo;
        for brano in &brani {
            seguendo.uid_proprio = seguendo.uid_proprio.saturating_add(1);
            let uid = seguendo.uid_proprio;
            match seguendo.proprie.aggiungi(uid, brano, &mut nuovo_gettone) {
                Ok(mut nuovi) => {
                    servizi.append(&mut nuovi);
                    if seguendo.proprie.ha(uid) {
                        uids.push(uid);
                    }
                }
                Err(err) => {
                    nota!(
                        "[insieme] gettone non creato codice={}",
                        err.code().kind().code()
                    );
                }
            }
        }
        if uids.is_empty() {
            return;
        }
        let attesa = if suona {
            ATTESA_DESCRITTORI_SUONA
        } else {
            ATTESA_DESCRITTORI
        };
        seguendo.in_attesa.push(Aggiunta {
            uids,
            dove,
            suona,
            scade: ora().saturating_add(attesa),
        });
        // La connessione di servizio, la prima volta: è da lì che l'host
        // chiederà i byte di questi gettoni per chi li ascolta.
        let offri = !seguendo.offerta;
        seguendo.offerta = true;
        let (indirizzo, chiave) = (seguendo.indirizzo, seguendo.chiave);
        self.esegui_servizi(servizi, true);
        if offri && let Some(accesa) = &self.rete {
            accesa.rete.offri(indirizzo, chiave);
        }
        self.manda_le_aggiunte();
    }

    /// Le aggiunte pronte partono, nell'ordine: pronta vuol dire che ogni sua
    /// voce ha il descrittore (o non lo avrà), o che l'attesa è finita.
    pub(super) fn manda_le_aggiunte(&mut self) {
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let (host, io) = (m.host, m.io);
        let adesso = ora();
        let mut pronte = Vec::new();
        if let Some(seguendo) = &mut self.seguendo {
            while let Some(prima) = seguendo.in_attesa.first() {
                let descritte = prima.uids.iter().all(|u| seguendo.proprie.descritta(*u));
                if !descritte && adesso < prima.scade {
                    break;
                }
                let aggiunta = seguendo.in_attesa.remove(0);
                let voci: Vec<VoceSulFilo> = aggiunta
                    .uids
                    .iter()
                    .filter_map(|uid| seguendo.proprie.voce(*uid).cloned())
                    .map(|mut v| {
                        v.di = io;
                        v
                    })
                    .collect();
                for uid in &aggiunta.uids {
                    seguendo.mandate.insert(
                        *uid,
                        Mandata {
                            vista: false,
                            alle: adesso,
                        },
                    );
                }
                seguendo.prossimo_comando = seguendo.prossimo_comando.wrapping_add(1);
                pronte.push(Messaggio::Comando {
                    id: seguendo.prossimo_comando,
                    azione: Azione::Aggiungi {
                        voci,
                        dove: aggiunta.dove,
                        suona: aggiunta.suona,
                    },
                });
            }
        }
        for messaggio in pronte {
            self.manda(host, &messaggio);
        }
    }

    /// Chi segue: le voci proprie che l'host ha tolto dalla coda (o che non vi
    /// sono mai arrivate) smettono di essere servite.
    fn tieni_le_proprie(&mut self, stato: &StatoSessione) {
        let adesso = ora();
        let Fase::Membro(m) = &self.s.fase else {
            return;
        };
        let io = m.io;
        let Some(seguendo) = &mut self.seguendo else {
            return;
        };
        let mut via = Vec::new();
        for uid in seguendo.proprie.uids().collect::<Vec<_>>() {
            let Some(mia) = seguendo.proprie.voce(uid).cloned() else {
                continue;
            };
            let presente = stato
                .coda
                .iter()
                .any(|v| v.di == io && stessa_sorgente(&v.sorgente, &mia.sorgente));
            let Some(mandata) = seguendo.mandate.get_mut(&uid) else {
                // Non ancora partita: aspetta il suo descrittore.
                continue;
            };
            if presente {
                mandata.vista = true;
            } else if mandata.vista || adesso.saturating_sub(mandata.alle) > ATTESA_ECO_MS {
                via.push(uid);
            }
        }
        if via.is_empty() {
            return;
        }
        for uid in &via {
            seguendo.mandate.remove(uid);
        }
        let servizi = seguendo.proprie.dimentica(&via);
        self.esegui_servizi(servizi, true);
    }
}

/// Due voci leggono gli stessi byte: lo stesso gettone, o lo stesso
/// riferimento di catalogo.
fn stessa_sorgente(a: &SorgenteSulFilo, b: &SorgenteSulFilo) -> bool {
    match (a, b) {
        (SorgenteSulFilo::File { gettone: x, .. }, SorgenteSulFilo::File { gettone: y, .. }) => {
            x == y
        }
        (
            SorgenteSulFilo::Catalogo { riferimento: x },
            SorgenteSulFilo::Catalogo { riferimento: y },
        ) => x == y,
        _ => false,
    }
}
