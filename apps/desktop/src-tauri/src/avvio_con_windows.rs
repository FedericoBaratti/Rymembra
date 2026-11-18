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

#[cfg(desktop)]
use rymembra_app::preferenze::{self, RispostaProposta};
use rymembra_barra::{Impedimento, StatoAvvio};
use serde::Serialize;
use tauri::{Manager as _, State};

use crate::DatiAMano;
use crate::errore::Esito;
use crate::errore::errore;
#[cfg(windows)]
use crate::nota;
use crate::stato::Stato;
#[cfg(desktop)]
use crate::stato::con_libreria;
#[cfg(any(windows, mobile))]
use rymembra_domain::errors::{AppError, ErrorCode};

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
#[tauri::command(async)]
pub fn avvio_con_windows_stato(app: tauri::AppHandle) -> Esito<AvvioConWindowsIpc> {
    if e_di_prova(&app) {
        return Ok(AvvioConWindowsIpc::PROVA);
    }
    dal_sistema(None).map_err(errore)
}

/// Accende o spegne l'avvio con Windows. Riporta com'è rimasto, **riletto**.
///
/// Con la scelta dell'utente fatta altrove — Gestione attività, un criterio —
/// non scrive niente e dice perché: vedi `rymembra_barra::avvio`.
#[tauri::command(async)]
pub fn avvio_con_windows_imposta(app: tauri::AppHandle, acceso: bool) -> Esito<AvvioConWindowsIpc> {
    if e_di_prova(&app) {
        return Ok(AvvioConWindowsIpc::PROVA);
    }
    dal_sistema(Some(acceso)).map_err(errore)
}

/// Cosa fare della proposta «parti con Windows e resta nel vassoio».
#[cfg(desktop)]
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
/// Sul telefono no: la sua parte — il servizio persistente — ha un'altra
/// strada, e nessuna domanda da fare da qui.
#[tauri::command(async)]
pub fn proposta_sottofondo_da_fare(app: tauri::AppHandle, stato: State<'_, Stato>) -> Esito<bool> {
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
        let _ = (app, stato);
        Ok(false)
    }
}

/// Risponde alla proposta. «Sì» accende l'avvio con Windows e la X sul vassoio;
/// «No, grazie» non accende niente. Torna com'è rimasto, **riletto**.
///
/// La risposta si ricorda **prima** di toccare il sistema: una proposta si fa
/// una volta sola, e se l'accensione fallisce — un criterio, un registro che non
/// risponde — l'errore si dice e la domanda non si ripete a ogni avvio. L'avvio
/// con Windows resta spento, e c'è l'interruttore nelle Impostazioni.
#[tauri::command(async)]
pub fn proposta_sottofondo_rispondi(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    accetta: bool,
) -> Esito<RispostaPropostaIpc> {
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
        let _ = (app, stato, accetta);
        Err(errore(AppError::new(ErrorCode::InternalAborted {
            what: Some("proposta del sottofondo sul telefono".to_owned()),
        })))
    }
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
            .split("pub fn proposta_sottofondo_rispondi(")
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
            "pub fn avvio_con_windows_stato(",
            "pub fn avvio_con_windows_imposta(",
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
