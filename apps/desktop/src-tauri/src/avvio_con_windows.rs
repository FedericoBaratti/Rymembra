//! «Avvia con Windows»: i due comandi dell'interruttore.
//!
//! La regola — cosa si scrive nel registro, quando un avvio vale acceso, cosa fa
//! la copia del Store — sta in `rymembra-barra` (`avvio`, `registro`), dove i
//! registri si fingono e le prove non toccano mai quello vero. Qui c'è solo ciò
//! che la lega all'applicazione: quale delle due copie siamo, chi non può
//! accendere l'avvio, e la forma che arriva alla finestra.
//!
//! # L'interruttore è pessimista
//!
//! Come `secondo_piano_attiva`: il comando risponde con lo stato **riletto** dal
//! sistema, non con quello chiesto. Qui conta più che altrove, perché
//! l'avvio può essere spento da fuori — Gestione attività, un criterio — e un
//! interruttore ottimista direbbe «acceso» per un avvio che non parte.
//!
//! # Chi non può
//!
//! - **Una copia di prova** (la cartella dati scelta da una variabile,
//!   [`DatiAMano`]): la voce `Run` si chiama `Rymembra`, come quella della copia
//!   vera di chi sta provando. Accenderla da una copia di prova rimanderebbe a
//!   un'altra cartella dati — o a un eseguibile che fra un'ora non c'è più — il
//!   login vero della sua persona. Qui si risponde `prova` e non si tocca niente.
//! - **Fuori da Windows** e **sul telefono**: `nonDisponibile`. Il registro dei
//!   comandi è uno solo, e la pagina non deve ricevere un «command not found».

use rymembra_app::preferenze::{self, RispostaProposta};
use rymembra_barra::{Impedimento, StatoAvvio};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager as _;

use crate::DatiAMano;
use crate::disparte::{comanda, stato_di};
use crate::errore::Esito;
use crate::errore::errore;
#[cfg(windows)]
use crate::nota;
use crate::stato::Stato;
use crate::stato::con_libreria;
#[cfg(any(windows, mobile))]
use rymembra_domain::errors::AppError;
#[cfg(windows)]
use rymembra_domain::errors::ErrorCode;

/// Perché l'interruttore non si tocca, come lo legge la pagina.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Impedito {
    /// L'ha spento l'utente da Windows: si rimette lì.
    Sistema,
    /// Lo decide un criterio del computer.
    Criterio,
    /// È una copia di prova: non tocca il login della copia vera.
    Prova,
    /// Qui l'avvio con Windows non esiste.
    #[cfg_attr(
        windows,
        allow(dead_code, reason = "lo costruisce solo chi non è Windows")
    )]
    NonDisponibile,
}

/// Lo stato dell'avvio con Windows, per la pagina.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvvioConWindowsIpc {
    /// Parte con Windows.
    pub acceso: bool,
    /// Se l'interruttore non si tocca, e perché.
    pub impedito: Option<Impedito>,
}

impl AvvioConWindowsIpc {
    /// Una copia di prova: spento, e non si accende da qui.
    const PROVA: Self = Self {
        acceso: false,
        impedito: Some(Impedito::Prova),
    };

    /// Un posto dove l'avvio con Windows non esiste.
    #[cfg(not(windows))]
    const NON_DISPONIBILE: Self = Self {
        acceso: false,
        impedito: Some(Impedito::NonDisponibile),
    };
}

impl From<StatoAvvio> for AvvioConWindowsIpc {
    fn from(stato: StatoAvvio) -> Self {
        Self {
            acceso: stato.acceso,
            impedito: stato.impedito.map(|impedimento| match impedimento {
                Impedimento::Sistema => Impedito::Sistema,
                Impedimento::Criterio => Impedito::Criterio,
            }),
        }
    }
}

/// Un guasto del sistema, nella forma che l'interfaccia sa mostrare: la causa
/// resta nel diario e nell'errore, non nella frase.
#[cfg(windows)]
fn guasto(cosa: &str, causa: String) -> AppError {
    nota!("[avvio] {cosa}: {causa}");
    AppError::new(ErrorCode::InternalUnexpected {
        detail: Some(format!("avvio con Windows: {cosa}")),
    })
    .with_cause(causa)
}

/// La copia è di prova? Lo dice la cartella dati scelta a mano.
fn e_di_prova(app: &tauri::AppHandle) -> bool {
    app.try_state::<DatiAMano>().is_some_and(|dati| dati.0)
}

/// Com'è l'avvio con Windows adesso, o come lo si cambia: la parte che parla col
/// sistema, per la copia vera.
#[cfg(windows)]
fn dal_sistema(acceso: Option<bool>) -> Result<AvvioConWindowsIpc, AppError> {
    use rymembra_barra::{
        RegistroDiWindows, avvio_imposta, avvio_imposta_dal_store, avvio_leggi,
        avvio_leggi_dal_store,
    };

    let stato = if crate::aggiornamenti::gestita_dallo_store() {
        match acceso {
            None => avvio_leggi_dal_store(),
            Some(acceso) => avvio_imposta_dal_store(acceso),
        }
        .map_err(|causa| guasto("l'attività di avvio del pacchetto", causa))?
    } else {
        let esegui = std::env::current_exe()
            .map_err(|err| guasto("il percorso dell'eseguibile", err.to_string()))?;
        match acceso {
            None => avvio_leggi(&RegistroDiWindows, &esegui),
            Some(acceso) => avvio_imposta(&RegistroDiWindows, &esegui, acceso),
        }
        .map_err(|causa| guasto("la voce di avvio", causa))?
    };
    Ok(stato.into())
}

/// Fuori da Windows non c'è niente da chiedere.
#[cfg(not(windows))]
fn dal_sistema(acceso: Option<bool>) -> Result<AvvioConWindowsIpc, AppError> {
    let _ = acceso;
    Ok(AvvioConWindowsIpc::NON_DISPONIBILE)
}

/// Parte con Windows? Lo dice il sistema, non una preferenza nostra.
#[tauri::command]
pub async fn avvio_con_windows_stato(app: tauri::AppHandle) -> Esito<AvvioConWindowsIpc> {
    // Legge il registro di Windows: sul pool bloccante.
    comanda("avvio con Windows", move || {
        if e_di_prova(&app) {
            return Ok(AvvioConWindowsIpc::PROVA);
        }
        dal_sistema(None).map_err(errore)
    })
    .await
}

/// Accende o spegne l'avvio con Windows. Riporta com'è rimasto, **riletto**.
///
/// Con la scelta dell'utente fatta altrove — Gestione attività, un criterio —
/// non scrive niente e dice perché: vedi `rymembra_barra::avvio`.
#[tauri::command]
pub async fn avvio_con_windows_imposta(
    app: tauri::AppHandle,
    acceso: bool,
) -> Esito<AvvioConWindowsIpc> {
    comanda("avvio con Windows", move || {
        if e_di_prova(&app) {
            return Ok(AvvioConWindowsIpc::PROVA);
        }
        dal_sistema(Some(acceso)).map_err(errore)
    })
    .await
}

/// Cosa fare della proposta: «parti con Windows e resta nel vassoio» sul
/// computer, «resta attiva in sottofondo» sul telefono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Proposta {
    /// Mostrarla.
    DaFare,
    /// Non mostrarla, e non ricordarlo: ci riproverà il prossimo avvio.
    No,
    /// Non c'è niente da proporre perché è già tutto acceso: si ricorda, e non
    /// si chiede.
    GiaAcceso,
}

/// La regola della proposta, senza sistema né libreria.
///
/// - **Una volta sola**: con una risposta già data non si ripropone.
/// - **Solo se si può accettare**: con l'avvio impedito — l'utente l'ha spento
///   da Windows, un criterio lo decide, è una copia di prova — il «sì» non
///   potrebbe fare quel che dice, e chiederlo sarebbe una domanda a cui non si
///   può rispondere.
/// - **Non a chi ha già tutto acceso**: sarebbe una domanda inutile.
#[cfg(desktop)]
fn da_proporre(
    risposta: Option<RispostaProposta>,
    avvio: &AvvioConWindowsIpc,
    secondo_piano: bool,
) -> Proposta {
    if risposta.is_some() || avvio.impedito.is_some() {
        return Proposta::No;
    }
    if avvio.acceso && secondo_piano {
        return Proposta::GiaAcceso;
    }
    Proposta::DaFare
}

/// La regola della proposta **del telefono**, senza libreria: «resta attiva in
/// sottofondo».
///
/// - **Una volta sola**, con la stessa chiave del computer
///   (`ui.proposta.sottofondo`): con una risposta già data non si ripropone.
/// - **Non a chi ha già il sottofondo acceso**: sarebbe una domanda inutile, e
///   si ricorda come risposta «già», com'è sul computer.
///
/// Non guarda «Raggiungibile da internet»: il sottofondo da solo non lo
/// accende, e se manca lo dice la finestra dopo il «sì» — la proposta resta
/// quella della guida.
#[cfg_attr(
    not(mobile),
    allow(
        dead_code,
        reason = "la regola del telefono, che le prove vedono ovunque"
    )
)]
fn da_proporre_sul_telefono(risposta: Option<RispostaProposta>, sottofondo: bool) -> Proposta {
    if risposta.is_some() {
        Proposta::No
    } else if sottofondo {
        Proposta::GiaAcceso
    } else {
        Proposta::DaFare
    }
}

/// «Più tardi» alla proposta, per il resto della sessione.
///
/// # Perché sta qui e non nella pagina
///
/// Perché la sessione è quella del **programma**. Con la X sul vassoio la pagina
/// muore e «Mostra» ne fa nascere una nuova (`comparsa`): un rimando tenuto
/// nella pagina — come il «Non ora» dell'aggiornamento, che sopravvive solo ai
/// cambi di faccia dello Studio — tornava a ogni «Mostra», cioè proprio nel
/// caso che il collaudo in Windows Sandbox ha provato. Sul telefono l'attività
/// rinasce spesso e il processo no.
///
/// **Non è una risposta**: non si scrive niente nella libreria
/// (`ui.proposta.sottofondo` resta com'è), e il prossimo avvio la rifà. Una
/// risposta data è per sempre; questo no.
#[derive(Debug, Default)]
pub struct Rimando(AtomicBool);

impl Rimando {
    /// Nessun rimando: la proposta si fa, se c'è da farla.
    #[must_use]
    pub const fn nuovo() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Da qui in poi la proposta non si rifà, finché il processo vive.
    pub fn imposta(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// La proposta è stata rimandata in questa sessione?
    #[must_use]
    pub fn attivo(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Il rimando di questa sessione.
static RIMANDO: Rimando = Rimando::nuovo();

/// La risposta alla proposta, e com'è rimasto tutto: la pagina disegna da qui.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RispostaPropostaIpc {
    /// L'avvio con Windows, com'è rimasto.
    pub avvio: AvvioConWindowsIpc,
    /// La X manda in secondo piano, com'è rimasta.
    pub secondo_piano: bool,
}

/// C'è da fare la proposta? Si chiede una volta, quando la pagina è pronta a
/// riceverla (`parti/PropostaSottofondo.tsx`): né col primo avvio né col giro.
///
/// Sul computer è «parti con Windows e resta nel vassoio»; sul telefono, «resta
/// attiva in sottofondo» ([`da_proporre_sul_telefono`]). Una chiave sola, ma
/// ogni installazione ha la sua risposta.
#[tauri::command]
pub async fn proposta_sottofondo_da_fare(app: tauri::AppHandle) -> Esito<bool> {
    comanda("proposta del sottofondo", move || {
        let stato = stato_di::<Stato>(&app)?;
        // «Più tardi» in questa sessione: prima di ogni lettura, che non costa né la
        // libreria né il sistema. Non si ricorda niente: vedi [`Rimando`].
        if RIMANDO.attivo() {
            return Ok(false);
        }
        #[cfg(desktop)]
        {
            if e_di_prova(&app) {
                return Ok(false);
            }
            let risposta = con_libreria(&stato, |libreria| {
                preferenze::proposta_sottofondo(&libreria.connection)
            })
            .map_err(errore)?;
            // Già risposto: non si chiede nemmeno al sistema.
            if risposta.is_some() {
                return Ok(false);
            }
            let avvio = dal_sistema(None).map_err(errore)?;
            let secondo_piano = con_libreria(&stato, |libreria| {
                preferenze::secondo_piano(&libreria.connection)
            })
            .map_err(errore)?;
            match da_proporre(risposta, &avvio, secondo_piano) {
                Proposta::DaFare => Ok(true),
                Proposta::No => Ok(false),
                Proposta::GiaAcceso => {
                    con_libreria(&stato, |libreria| {
                        preferenze::imposta_proposta_sottofondo(
                            &libreria.connection,
                            RispostaProposta::Gia,
                        )
                    })
                    .map_err(errore)?;
                    Ok(false)
                }
            }
        }
        #[cfg(mobile)]
        {
            // Una copia di prova è un fatto del computer: il telefono non ne ha.
            let _ = &app;
            let risposta = con_libreria(&stato, |libreria| {
                preferenze::proposta_sottofondo(&libreria.connection)
            })
            .map_err(errore)?;
            // Già risposto: non si chiede nemmeno alle impostazioni.
            if risposta.is_some() {
                return Ok(false);
            }
            let sottofondo = con_libreria(&stato, |libreria| {
                rymembra_app::dispositivi::impostazioni(&libreria.connection)
            })
            .map_err(errore)?
            .sottofondo;
            match da_proporre_sul_telefono(risposta, sottofondo) {
                Proposta::DaFare => Ok(true),
                Proposta::No => Ok(false),
                Proposta::GiaAcceso => {
                    con_libreria(&stato, |libreria| {
                        preferenze::imposta_proposta_sottofondo(
                            &libreria.connection,
                            RispostaProposta::Gia,
                        )
                    })
                    .map_err(errore)?;
                    Ok(false)
                }
            }
        }
    })
    .await
}

/// «Più tardi»: la proposta non si rifà per il resto della sessione del
/// programma, e non si ricorda nessuna risposta. Vedi [`Rimando`].
#[tauri::command]
pub fn proposta_sottofondo_rimanda() {
    RIMANDO.imposta();
}

/// Risponde alla proposta. «Sì» accende l'avvio con Windows e la X sul vassoio;
/// «No, grazie» non accende niente. Torna com'è rimasto, **riletto**.
///
/// La risposta si ricorda **prima** di toccare il sistema: una proposta si fa
/// una volta sola, e se l'accensione fallisce — un criterio, un registro che non
/// risponde — l'errore si dice e la domanda non si ripete a ogni avvio. L'avvio
/// con Windows resta spento, e c'è l'interruttore nelle Impostazioni.
#[tauri::command]
pub async fn proposta_sottofondo_rispondi(
    app: tauri::AppHandle,
    accetta: bool,
) -> Esito<RispostaPropostaIpc> {
    comanda("risposta alla proposta del sottofondo", move || {
        let stato = stato_di::<Stato>(&app)?;
        #[cfg(desktop)]
        {
            let risposta = if accetta {
                RispostaProposta::Si
            } else {
                RispostaProposta::No
            };
            con_libreria(&stato, |libreria| {
                preferenze::imposta_proposta_sottofondo(&libreria.connection, risposta)
            })
            .map_err(errore)?;
            // Una copia di prova non tocca il login vero né accende il vassoio per
            // una risposta che la proposta, lì, non avrebbe nemmeno chiesto.
            let di_prova = e_di_prova(&app);
            if accetta && !di_prova {
                crate::vassoio::imposta_il_secondo_piano(&app, &stato, true).map_err(errore)?;
                dal_sistema(Some(true)).map_err(errore)?;
            }
            let avvio = if di_prova {
                AvvioConWindowsIpc::PROVA
            } else {
                dal_sistema(None).map_err(errore)?
            };
            let secondo_piano = con_libreria(&stato, |libreria| {
                preferenze::secondo_piano(&libreria.connection)
            })
            .map_err(errore)?;
            Ok(RispostaPropostaIpc {
                avvio,
                secondo_piano,
            })
        }
        #[cfg(mobile)]
        {
            let _ = &app;
            let risposta = if accetta {
                RispostaProposta::Si
            } else {
                RispostaProposta::No
            };
            con_libreria(&stato, |libreria| {
                preferenze::imposta_proposta_sottofondo(&libreria.connection, risposta)
            })
            .map_err(errore)?;
            // Qui si ricorda e basta. Il servizio in primo piano lo accende la
            // finestra, con lo stesso comando dell'interruttore delle Impostazioni
            // (`lontano_sottofondo`), che chiede anche le notifiche; l'avvio con
            // Windows e il vassoio sul telefono non esistono.
            Ok(RispostaPropostaIpc {
                avvio: AvvioConWindowsIpc::NON_DISPONIBILE,
                secondo_piano: false,
            })
        }
    })
    .await
}

#[cfg(test)]
mod prove {
    use super::*;

    fn avvio(acceso: bool, impedito: Option<Impedito>) -> AvvioConWindowsIpc {
        AvvioConWindowsIpc { acceso, impedito }
    }

    #[test]
    fn la_proposta_si_fa_a_chi_non_ha_acceso_niente_e_si_puo_accettare() {
        let spento = avvio(false, None);
        assert_eq!(da_proporre(None, &spento, false), Proposta::DaFare);
        // Anche a chi ha una cosa sola: la proposta le accende tutte e due.
        assert_eq!(da_proporre(None, &spento, true), Proposta::DaFare);
        assert_eq!(
            da_proporre(None, &avvio(true, None), false),
            Proposta::DaFare
        );
    }

    #[test]
    fn la_proposta_del_telefono_si_fa_una_volta_a_chi_non_ha_il_sottofondo() {
        // Sul telefono la proposta è «resta attiva in sottofondo»: a chi non
        // l'ha acceso e non ha mai risposto.
        assert_eq!(da_proporre_sul_telefono(None, false), Proposta::DaFare);
        // Chi l'ha già acceso non ha niente da accettare: si ricorda, e non si
        // chiede.
        assert_eq!(da_proporre_sul_telefono(None, true), Proposta::GiaAcceso);
        // Una volta sola, qualunque cosa sia rimasta accesa.
        for risposta in [
            RispostaProposta::Si,
            RispostaProposta::No,
            RispostaProposta::Gia,
        ] {
            for sottofondo in [false, true] {
                assert_eq!(
                    da_proporre_sul_telefono(Some(risposta), sottofondo),
                    Proposta::No,
                    "{risposta:?}, sottofondo {sottofondo}"
                );
            }
        }
    }

    /// Sul telefono il comando ricorda la risposta con la stessa chiave del
    /// computer e basta: il servizio lo accende la finestra, con il comando
    /// dell'interruttore (`lontano_sottofondo`), che chiede anche le notifiche.
    #[test]
    fn sul_telefono_il_comando_ricorda_la_risposta_e_non_accende_niente() {
        let sorgente = include_str!("avvio_con_windows.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = sorgente
            .split("pub async fn proposta_sottofondo_rispondi(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let telefono = corpo.split("#[cfg(mobile)]").nth(1).unwrap_or_default();
        assert!(
            telefono.contains("imposta_proposta_sottofondo("),
            "sul telefono la risposta non si ricorda"
        );
        assert!(
            !telefono.contains("dal_sistema(") && !telefono.contains("imposta_il_secondo_piano("),
            "sul telefono il comando tocca l'avvio con Windows o il vassoio"
        );
        // La domanda stessa: una chiave sola, quella di sempre.
        let da_fare = sorgente
            .split("pub async fn proposta_sottofondo_da_fare(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .and_then(|resto| resto.split("#[cfg(mobile)]").nth(1))
            .unwrap_or_default();
        assert!(da_fare.contains("preferenze::proposta_sottofondo("));
        assert!(da_fare.contains("da_proporre_sul_telefono("));
        assert!(da_fare.contains("RispostaProposta::Gia"));
    }

    #[test]
    fn la_proposta_non_si_ripete() {
        let spento = avvio(false, None);
        for risposta in [
            RispostaProposta::Si,
            RispostaProposta::No,
            RispostaProposta::Gia,
        ] {
            assert_eq!(da_proporre(Some(risposta), &spento, false), Proposta::No);
        }
    }

    /// «Più tardi» dura quanto il processo: la pagina può morire e rinascere (la X
    /// col vassoio la distrugge, «Mostra» la ricrea) e la scheda non deve
    /// tornare, perché «questa sessione» è quella del programma e non della
    /// pagina. Una volta alzato non si abbassa.
    #[test]
    fn il_piu_tardi_dura_quanto_il_processo() {
        let rimando = Rimando::nuovo();
        assert!(!rimando.attivo(), "nasce spento: la proposta si fa");
        rimando.imposta();
        assert!(rimando.attivo(), "dopo «Più tardi» la proposta non si rifà");
        rimando.imposta();
        assert!(rimando.attivo(), "alzarlo due volte non lo abbassa");
    }

    /// La domanda guarda il rimando **prima** di ogni lettura (non costa né la
    /// libreria né il sistema), e «Più tardi» non ricorda niente nel database: è
    /// un rimando, non una risposta, e il prossimo avvio la rifà.
    #[test]
    fn il_piu_tardi_non_e_una_risposta_e_si_guarda_per_primo() {
        let sorgente = include_str!("avvio_con_windows.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let da_fare = sorgente
            .split("pub async fn proposta_sottofondo_da_fare(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let guardia = da_fare
            .find("RIMANDO.attivo()")
            .expect("la domanda guarda il rimando");
        assert!(
            guardia < da_fare.find("#[cfg(desktop)]").unwrap_or(usize::MAX)
                && guardia < da_fare.find("#[cfg(mobile)]").unwrap_or(usize::MAX)
                && guardia < da_fare.find("con_libreria(").unwrap_or(usize::MAX),
            "il rimando si guarda prima di ogni lettura, sul computer e sul telefono"
        );
        let rimanda = sorgente
            .split("pub fn proposta_sottofondo_rimanda(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .expect("il comando c'è");
        assert!(rimanda.contains("RIMANDO.imposta()"));
        assert!(
            !rimanda.contains("imposta_proposta_sottofondo(") && !rimanda.contains("con_libreria("),
            "«Più tardi» ricorda una risposta: la proposta non tornerebbe più"
        );
    }

    #[test]
    fn la_proposta_non_chiede_quel_che_non_si_puo_fare() {
        // Spento da Windows, deciso da un criterio, una copia di prova, un posto
        // senza avvio: il «sì» non potrebbe fare quel che dice. E non si
        // ricorda niente: se la cosa cambia, ci si può riprovare.
        for impedito in [
            Impedito::Sistema,
            Impedito::Criterio,
            Impedito::Prova,
            Impedito::NonDisponibile,
        ] {
            assert_eq!(
                da_proporre(None, &avvio(false, Some(impedito)), false),
                Proposta::No,
                "{impedito:?}"
            );
        }
    }

    #[test]
    fn a_chi_ha_gia_tutto_acceso_non_si_chiede_niente() {
        assert_eq!(
            da_proporre(None, &avvio(true, None), true),
            Proposta::GiaAcceso
        );
    }

    /// Ricordare la risposta **prima** di toccare il sistema: se l'accensione
    /// fallisce, la domanda non si ripete a ogni avvio.
    #[test]
    fn la_risposta_si_ricorda_prima_di_accendere_qualcosa() {
        let sorgente = include_str!("avvio_con_windows.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        let corpo = sorgente
            .split("pub async fn proposta_sottofondo_rispondi(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let ricorda = corpo.find("imposta_proposta_sottofondo(");
        let accende = corpo.find("dal_sistema(Some(true))");
        let secondo_piano = corpo.find("imposta_il_secondo_piano(");
        assert!(ricorda.is_some() && ricorda < accende && ricorda < secondo_piano);
        // E una copia di prova non accende niente.
        assert!(corpo.contains("accetta && !di_prova"));
    }

    #[test]
    fn la_risposta_arriva_alla_pagina_con_tutto_com_e_rimasto() {
        let json = serde_json::to_string(&RispostaPropostaIpc {
            avvio: avvio(true, None),
            secondo_piano: true,
        })
        .expect("serializza");
        assert_eq!(
            json,
            r#"{"avvio":{"acceso":true,"impedito":null},"secondoPiano":true}"#
        );
    }

    #[test]
    fn il_nome_del_valore_e_il_nome_del_prodotto() {
        // Il modello NSIS di Tauri toglie alla disinstallazione
        // `Run\${PRODUCTNAME}` e nient'altro (`installer.nsi`, «Removes the
        // Autostart entry»): con un nome diverso l'avvio resterebbe dopo la
        // disinstallazione.
        let configurazione: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("un JSON");
        assert_eq!(
            configurazione["productName"].as_str(),
            Some(rymembra_barra::NOME_VALORE)
        );
    }

    #[test]
    fn l_attivita_del_manifesto_e_quella_che_il_codice_cerca() {
        let manifesto = include_str!("../msix/AppxManifest.xml");
        assert!(
            manifesto.contains(&format!("TaskId=\"{}\"", rymembra_barra::ID_ATTIVITA_STORE)),
            "il manifesto MSIX non dichiara l'attività {}",
            rymembra_barra::ID_ATTIVITA_STORE
        );
    }

    #[test]
    fn una_scelta_del_sistema_arriva_alla_pagina_con_la_sua_ragione() {
        let spento_da_windows: AvvioConWindowsIpc = StatoAvvio::SPENTO_DAL_SISTEMA.into();
        assert_eq!(
            spento_da_windows,
            AvvioConWindowsIpc {
                acceso: false,
                impedito: Some(Impedito::Sistema)
            }
        );
        let acceso: AvvioConWindowsIpc = StatoAvvio::ACCESO.into();
        assert_eq!(
            acceso,
            AvvioConWindowsIpc {
                acceso: true,
                impedito: None
            }
        );
    }

    #[test]
    fn la_forma_sul_filo_e_quella_che_la_pagina_legge() {
        let json = serde_json::to_string(&AvvioConWindowsIpc {
            acceso: false,
            impedito: Some(Impedito::NonDisponibile),
        })
        .expect("serializza");
        assert_eq!(json, r#"{"acceso":false,"impedito":"nonDisponibile"}"#);
        let json = serde_json::to_string(&AvvioConWindowsIpc::PROVA).expect("serializza");
        assert_eq!(json, r#"{"acceso":false,"impedito":"prova"}"#);
    }

    /// Una copia di prova non tocca il login della copia vera: i comandi
    /// guardano `DatiAMano` **prima** di chiedere qualcosa al sistema.
    #[test]
    fn una_copia_di_prova_non_tocca_il_login_vero() {
        let sorgente = include_str!("avvio_con_windows.rs");
        let sorgente = sorgente.split("#[cfg(test)]").next().unwrap_or(sorgente);
        for comando in [
            "pub async fn avvio_con_windows_stato(",
            "pub async fn avvio_con_windows_imposta(",
        ] {
            let corpo = sorgente
                .split(comando)
                .nth(1)
                .and_then(|resto| resto.split("\n}\n").next())
                .unwrap_or_default();
            let prova = corpo.find("e_di_prova(");
            let sistema = corpo.find("dal_sistema(");
            assert!(
                prova.is_some() && prova < sistema,
                "{comando} chiede al sistema prima di sapere se è una copia di prova"
            );
        }
    }
}
