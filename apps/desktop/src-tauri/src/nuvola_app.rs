//! Le app dell'utente presso Google, Microsoft e Dropbox, dal lato della
//! finestra: i tre comandi che le scrivono, e quel che succede a chi le usa.
//!
//! # Perché un modulo a parte
//!
//! Un'app Google serve due padroni — il backup (`nuvola.rs`) e le cartelle di
//! Drive (`nuvola_musica.rs`) — e cambiarla tocca tutti e due. Qui si tiene
//! l'ordine delle cose; i due moduli espongono solo quel che serve a farle.
//!
//! # L'ordine, quando un'app cambia
//!
//! 1. Si fermano i collegamenti in corso, e il backup se l'app è sua: chi
//!    aspetta un consenso tiene in mano le credenziali di prima, e le
//!    userebbe fra tre minuti. Se qualcuno è al lavoro si risponde
//!    `sync.busy`, e la finestra dice di riprovare fra poco.
//! 2. Il segreto, se c'è, va nel portachiavi **prima** del database: se il
//!    database poi rifiuta, si rimette quello di prima.
//! 3. Il database: l'app, e i legami.
//! 4. Chi la usa: se l'identificativo è cambiato, i conti legati si spengono
//!    e diventano «da ricollegare» (`cloud.appChanged`), il backup si
//!    scollega; se è cambiato solo il segreto, si riaprono con quello nuovo.
//!    Cartelle e brani non si toccano mai.
//! 5. Tutte le finestre lo sanno: `nuvola:app`, `nuvola:conti`, lo stato del
//!    backup.

use rymembra_app::nuvola_app::{self, AppSalvata};
use rymembra_cloud::musica::nuova_app;
use rymembra_cloud::oauth::{Credenziali, registrazione};
use rymembra_cloud::portachiavi::{self, DiSistema, Portachiavi};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola_app::{
    Piattaforma, difetto_id, difetto_segreto, forma_canonica, nome_pulito,
};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::nota;
use crate::spegnimento::Emette as _;
use crate::stato::{Stato, adesso_ms, con_libreria};

/// L'app che la 037 ha fatto col client che il backup aveva fino alla 3.1.
///
/// Il suo segreto può stare ancora nella voce di allora: vedi
/// [`segreto_dell_app`].
const EREDITATA: &str = "google-ereditata";

// ── quel che la finestra vede ───────────────────────────────────────────────

/// Le piattaforme e le app, per la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppNuvolaIpc {
    piattaforme: Vec<PiattaformaIpc>,
    app: Vec<AppIpc>,
}

/// Una piattaforma, con quel che va scritto nella sua console.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiattaformaIpc {
    chiave: &'static str,
    nome: &'static str,
    /// L'app ha anche un segreto (Google).
    segreto: bool,
    /// Gli indirizzi di ritorno da registrare, esatti.
    ritorni: Vec<String>,
    /// I permessi da concedere.
    permessi: Vec<&'static str>,
}

/// Un'app registrata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppIpc {
    id: String,
    piattaforma: &'static str,
    nome: String,
    client_id: String,
    /// Per Google: il segreto c'è. Per le altre sempre `true`.
    segreto_salvato: bool,
    /// È quella che si usa quando nessuno sceglie.
    predefinita: bool,
    /// Chi la usa.
    usata_da: Vec<UsoIpc>,
}

/// Chi usa un'app: un conto, o il backup (`conto: None`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsoIpc {
    conto: Option<String>,
    servizio: Option<&'static str>,
    nome: String,
}

// ── le credenziali ──────────────────────────────────────────────────────────

/// Il segreto di un'app, dal portachiavi.
///
/// Per [`EREDITATA`] torna anche alla voce di prima della 3.2
/// (`google.client_secret`): la 037 non la sposta, così niente si muove
/// all'avvio e una 3.1 rimessa al suo posto la ritrova.
///
/// Il portachiavi è un argomento perché questo ripiego è quel che tiene in vita
/// il backup di chi arriva dalla 3.1: le prove lo esercitano con un
/// [`portachiavi::InMemoria`], in produzione è sempre [`DiSistema`].
///
/// # Errori
///
/// `settings.secretUnavailable` se il portachiavi non risponde.
fn segreto_dell_app(chiavi: &dyn Portachiavi, app: &str) -> Result<Option<String>, AppError> {
    let proprio = chiavi
        .leggi(&portachiavi::nuvola_app_segreto(app))?
        .filter(|segreto| !segreto.trim().is_empty());
    if proprio.is_some() || app != EREDITATA {
        return Ok(proprio);
    }
    Ok(chiavi
        .leggi(portachiavi::GOOGLE_CLIENT_SECRET)?
        .filter(|segreto| !segreto.trim().is_empty()))
}

/// Le credenziali OAuth di un'app.
///
/// Da chiamare **fuori** dal lucchetto della libreria: legge il portachiavi.
///
/// # Errori
///
/// `cloud.appSecretMissing` per un'app Google senza segreto;
/// `settings.secretUnavailable` se il portachiavi non risponde.
pub(crate) fn credenziali_dell_app(app: &AppSalvata) -> Result<Credenziali, AppError> {
    credenziali_con(&DiSistema, app)
}

/// [`credenziali_dell_app`] con un portachiavi scelto da chi chiama.
fn credenziali_con(chiavi: &dyn Portachiavi, app: &AppSalvata) -> Result<Credenziali, AppError> {
    let client_secret = if app.piattaforma.serve_un_segreto() {
        Some(
            segreto_dell_app(chiavi, &app.app)?
                .ok_or_else(|| AppError::new(ErrorCode::CloudAppSecretMissing))?,
        )
    } else {
        None
    };
    Ok(Credenziali {
        client_id: app.client_id.clone(),
        client_secret,
    })
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Le piattaforme e le app registrate.
#[tauri::command]
pub async fn nuvola_app(app: AppHandle) -> Esito<AppNuvolaIpc> {
    in_disparte("app del cloud", move || stato_ipc(&app))
        .await
        .and_then(|esito| esito)
        .map_err(errore)
}

/// Registra un'app nuova (`id: None`) o cambia una che c'è.
///
/// `segreto: None` (o vuoto) tiene quello che c'è; un'app Google nuova lo
/// vuole. Vedi il `//!` per l'ordine delle cose.
#[tauri::command]
pub async fn nuvola_app_salva(
    app: AppHandle,
    id: Option<String>,
    piattaforma: String,
    nome: String,
    client_id: String,
    segreto: Option<String>,
) -> Esito<AppNuvolaIpc> {
    in_disparte("salvataggio di un'app del cloud", move || {
        salva(
            &app,
            id.as_deref(),
            &piattaforma,
            &nome,
            &client_id,
            segreto,
        )?;
        stato_ipc(&app)
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

/// Toglie un'app. I conti che la usavano restano, «da ricollegare».
#[tauri::command]
pub async fn nuvola_app_togli(app: AppHandle, id: String) -> Esito<AppNuvolaIpc> {
    in_disparte("rimozione di un'app del cloud", move || {
        togli(&app, &id)?;
        stato_ipc(&app)
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

// ── i corpi ─────────────────────────────────────────────────────────────────

/// Cosa è cambiato in un'app, per chi la usa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cambio {
    /// L'identificativo: i consensi dati all'app di prima non valgono più.
    Identificativo,
    /// Solo il segreto: i consensi valgono, le credenziali vanno riprese.
    Segreto,
}

/// Cosa tocca al backup quando un'app Google cambia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SulBackup {
    /// L'app non è sua, o non è cambiato niente che lo riguardi.
    Niente,
    /// Solo il segreto della sua app: il consenso vale, l'access token in
    /// cache no (è del segreto di prima).
    DimenticaToken,
    /// Un identificativo nuovo sulla sua app, o un'app adottata: il refresh
    /// token, se c'era, è di un'altra app, e il backup si scollega.
    DimenticaAccount,
}

/// Quel che il salvataggio di un'app comporta per chi la usa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Effetti {
    cambio: Option<Cambio>,
    /// Il backup senza app prende questa, che è nuova.
    adotta: bool,
    backup: SulBackup,
    /// I conti legati all'app si riaprono (`app_cambiata`).
    riapri_conti: bool,
}

/// La tabella di [`salva`]: da com'era l'app, com'è adesso, se è arrivato un
/// segreto e di chi è il backup, cosa succede a backup e conti.
///
/// Pura, e a parte, perché l'ordine dei casi conta e una prova lo fissa. Se si
/// guardasse il segreto prima dell'identificativo, chi corregge tutti e due
/// insieme otterrebbe solo [`SulBackup::DimenticaToken`]: il refresh token
/// della vecchia app resterebbe, e ogni passata del backup fallirebbe con
/// `invalid_grant`. E senza `sua` cambiare un'altra app Google scollegherebbe
/// il backup senza motivo.
///
/// `del_backup` è l'app del backup, se ne ha una; `con_backup` dice che il
/// backup è fermo nelle mani di chi salva, cioè che l'app è Google: senza, il
/// backup non si tocca.
fn effetti(
    prima: Option<&AppSalvata>,
    nuova: &AppSalvata,
    segreto_dato: bool,
    del_backup: Option<&str>,
    con_backup: bool,
) -> Effetti {
    let cambio = match prima {
        Some(prima) if prima.client_id != nuova.client_id => Some(Cambio::Identificativo),
        Some(_) if segreto_dato => Some(Cambio::Segreto),
        _ => None,
    };
    // Un'app Google nuova, col backup senza app: il backup la prende. È la
    // regola «una sola app, condivisa»: senza, il primo collegamento del
    // backup chiederebbe di scegliere fra un'app sola.
    let adotta = prima.is_none() && con_backup && del_backup.is_none();
    let sua = del_backup == Some(nuova.app.as_str());
    let backup = if con_backup {
        match cambio {
            Some(Cambio::Segreto) if sua => SulBackup::DimenticaToken,
            Some(Cambio::Identificativo) if sua => SulBackup::DimenticaAccount,
            None if adotta => SulBackup::DimenticaAccount,
            _ => SulBackup::Niente,
        }
    } else {
        SulBackup::Niente
    };
    Effetti {
        cambio,
        adotta,
        backup,
        riapri_conti: cambio.is_some(),
    }
}

fn salva(
    handle: &AppHandle,
    id: Option<&str>,
    piattaforma: &str,
    nome: &str,
    client_id: &str,
    segreto: Option<String>,
) -> Result<(), AppError> {
    let piattaforma = Piattaforma::da_chiave(piattaforma).ok_or_else(|| {
        AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "nuvola_app_salva".to_owned(),
            detail: Some(format!("«{piattaforma}» non è una piattaforma")),
        })
    })?;
    let segreto = segreto
        .map(|segreto| segreto.trim().to_owned())
        .filter(|segreto| !segreto.is_empty());
    if let Some(segreto) = &segreto {
        if !piattaforma.serve_un_segreto() {
            // Un segreto vero mandato a chi tratta l'app da client pubblico
            // sarebbe un errore, o un segreto finito dove non doveva.
            return Err(AppError::new(ErrorCode::CloudAppIdInvalid)
                .with_cause("questa piattaforma non vuole un segreto"));
        }
        if let Some(difetto) = difetto_segreto(segreto) {
            return Err(AppError::new(ErrorCode::CloudAppIdInvalid)
                .with_cause(format!("segreto dell'app: {difetto:?}")));
        }
    }
    // La forma prima di tutto: un identificativo storto non deve far scrivere
    // niente nel portachiavi. Il registro la ricontrolla, ed è lui a decidere.
    if nome_pulito(nome).is_none() || difetto_id(piattaforma, client_id).is_some() {
        return Err(AppError::new(ErrorCode::CloudAppIdInvalid)
            .with_cause("nome o identificativo dell'app"));
    }
    let stato = handle.try_state::<Stato>().ok_or_else(fuori_tempo)?;

    // Il lavoro, a gesti fermi: com'era l'app, chi la usa e cosa cambia si
    // leggono qui dentro e non prima, quando un altro gesto potrebbe ancora
    // cambiarli. Per un'app Google anche il backup è fermo, anche se non è
    // sua: chi usa cosa lo si sa solo qui.
    let lavoro = |backup: Option<(&Stato, &crate::nuvola::StatoNuvola)>| -> Result<(), AppError> {
        let prima = match id {
            Some(id) => Some(
                con_libreria(&stato, |l| nuvola_app::una(&l.connection, id))?
                    .ok_or_else(|| AppError::new(ErrorCode::CloudNotFound))?,
            ),
            None => None,
        };
        if let Some(prima) = &prima
            && prima.piattaforma != piattaforma
        {
            return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
                channel: "nuvola_app_salva".to_owned(),
                detail: Some("la piattaforma di un'app non cambia".to_owned()),
            }));
        }
        if prima.is_none() && piattaforma.serve_un_segreto() && segreto.is_none() {
            return Err(AppError::new(ErrorCode::CloudAppSecretMissing));
        }
        let app_id = match &prima {
            Some(prima) => prima.app.clone(),
            None => nuova_app(piattaforma)?,
        };
        let nuova = AppSalvata {
            app: app_id.clone(),
            piattaforma,
            nome: nome.to_owned(),
            client_id: forma_canonica(piattaforma, client_id),
            creata_ms: prima.as_ref().map_or_else(adesso_ms, |p| p.creata_ms),
        };
        let del_backup = con_libreria(&stato, |l| nuvola_app::app_del_backup(&l.connection))?;
        let da_fare = effetti(
            prima.as_ref(),
            &nuova,
            segreto.is_some(),
            del_backup.as_ref().map(|app| app.app.as_str()),
            backup.is_some(),
        );
        scrivi_app(&stato, &nuova, segreto.as_deref())?;
        if da_fare.adotta {
            con_libreria(&stato, |l| {
                nuvola_app::lega_il_backup(&l.connection, Some(&app_id))
            })?;
        }
        if let Some((stato, backup)) = backup {
            match da_fare.backup {
                SulBackup::Niente => {}
                SulBackup::DimenticaToken => crate::nuvola::dimentica_token(backup),
                SulBackup::DimenticaAccount => crate::nuvola::dimentica_l_account(stato, backup)?,
            }
        }
        if da_fare.riapri_conti {
            let conti = con_libreria(&stato, |l| nuvola_app::conti_legati(&l.connection, &app_id))?;
            crate::nuvola_musica::app_cambiata(handle, &conti);
        }
        Ok(())
    };
    crate::nuvola_musica::con_i_collegamenti_fermi(handle, || {
        if piattaforma == Piattaforma::Google {
            crate::nuvola::con_il_backup_fermo(handle, |stato, backup| {
                lavoro(Some((stato, backup)))
            })
        } else {
            lavoro(None)
        }
    })?;
    emetti(handle);
    Ok(())
}

/// Il segreto nel portachiavi, poi l'app nel database; se il database
/// rifiuta, il segreto torna quello di prima.
fn scrivi_app(stato: &Stato, app: &AppSalvata, segreto: Option<&str>) -> Result<(), AppError> {
    scrivi_app_con(&DiSistema, app, segreto, || {
        con_libreria(stato, |l| nuvola_app::salva(&l.connection, app))
    })
}

/// [`scrivi_app`] con il portachiavi e la scrittura nel database scelti da chi
/// chiama: è il punto in cui le prove fanno rifiutare il database, perché il
/// ritorno al segreto di prima è quel che impedisce un portachiavi col segreto
/// nuovo accanto a un database con l'app vecchia (ogni rinnovo del token
/// risponderebbe `invalid_client`).
fn scrivi_app_con(
    chiavi: &dyn Portachiavi,
    app: &AppSalvata,
    segreto: Option<&str>,
    salva_db: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    let voce = portachiavi::nuvola_app_segreto(&app.app);
    let di_prima = match segreto {
        Some(segreto) => {
            let di_prima = chiavi.leggi(&voce)?;
            chiavi.scrivi(&voce, segreto)?;
            Some(di_prima)
        }
        None => None,
    };
    let scritta = salva_db();
    if let Err(guasto) = scritta {
        if let Some(di_prima) = di_prima {
            let rimesso = match di_prima {
                Some(segreto) => chiavi.scrivi(&voce, &segreto),
                None => chiavi.cancella(&voce),
            };
            if let Err(altro) = rimesso {
                nota!(
                    "[nuvola] il segreto di {} non torna com'era: {altro}",
                    app.app
                );
            }
        }
        return Err(guasto);
    }
    if segreto.is_some() && app.app == EREDITATA {
        // Il segreto adesso sta nella voce dell'app: quella di prima della
        // 3.2 non serve più a nessuno.
        if let Err(guasto) = chiavi.cancella(portachiavi::GOOGLE_CLIENT_SECRET) {
            nota!("[nuvola] la voce vecchia del segreto Google resta: {guasto}");
        }
    }
    Ok(())
}

fn togli(handle: &AppHandle, id: &str) -> Result<(), AppError> {
    let stato = handle.try_state::<Stato>().ok_or_else(fuori_tempo)?;
    let di_google = con_libreria(&stato, |l| nuvola_app::una(&l.connection, id))?
        .ok_or_else(|| AppError::new(ErrorCode::CloudNotFound))?
        .piattaforma
        == Piattaforma::Google;
    // Chi la usava si legge dentro, a gesti fermi: vedi `salva`.
    let lavoro = |backup: Option<(&Stato, &crate::nuvola::StatoNuvola)>| -> Result<(), AppError> {
        let tolta = con_libreria(&stato, |l| nuvola_app::togli(&l.connection, id))?;
        if tolta.backup
            && let Some((stato, backup)) = backup
        {
            crate::nuvola::dimentica_l_account(stato, backup)?;
        }
        // I conti hanno già perso il legame nel database: riaperti, si
        // spengono e restano «da ricollegare».
        crate::nuvola_musica::app_cambiata(handle, &tolta.conti);
        Ok(())
    };
    crate::nuvola_musica::con_i_collegamenti_fermi(handle, || {
        if di_google {
            crate::nuvola::con_il_backup_fermo(handle, |stato, backup| {
                lavoro(Some((stato, backup)))
            })
        } else {
            lavoro(None)
        }
    })?;
    for voce in [portachiavi::nuvola_app_segreto(id)]
        .into_iter()
        .chain((id == EREDITATA).then(|| portachiavi::GOOGLE_CLIENT_SECRET.to_owned()))
    {
        if let Err(guasto) = DiSistema.cancella(&voce) {
            nota!("[nuvola] il segreto di un'app tolta resta nel portachiavi: {guasto}");
        }
    }
    emetti(handle);
    Ok(())
}

/// Lo stato per la finestra: il database sotto il lucchetto, il portachiavi
/// fuori.
fn stato_ipc(handle: &AppHandle) -> Result<AppNuvolaIpc, AppError> {
    let stato = handle.try_state::<Stato>().ok_or_else(fuori_tempo)?;
    let (app, del_backup, conti, predefinite) = con_libreria(&stato, |l| {
        let app = nuvola_app::elenco(&l.connection)?;
        let del_backup = nuvola_app::app_del_backup(&l.connection)?.map(|app| app.app);
        let conti = rymembra_app::nuvola::conti(&l.connection)?;
        let mut predefinite = Vec::new();
        for piattaforma in Piattaforma::tutte() {
            if let Some(app) = nuvola_app::predefinita(&l.connection, piattaforma)? {
                predefinite.push(app.app);
            }
        }
        Ok((app, del_backup, conti, predefinite))
    })?;
    let app = app
        .into_iter()
        .map(|salvata| {
            let mut usata_da: Vec<UsoIpc> = Vec::new();
            if del_backup.as_deref() == Some(salvata.app.as_str()) {
                usata_da.push(UsoIpc {
                    conto: None,
                    servizio: None,
                    nome: String::new(),
                });
            }
            usata_da.extend(
                conti
                    .iter()
                    .filter(|conto| conto.app.as_deref() == Some(salvata.app.as_str()))
                    .map(|conto| UsoIpc {
                        conto: Some(conto.conto.clone()),
                        servizio: Some(conto.servizio.chiave()),
                        nome: conto.nome.clone(),
                    }),
            );
            // Un portachiavi che non risponde vale «manca»: è la direzione
            // che non promette, e il guasto vero si vede collegando.
            let segreto_salvato = !salvata.piattaforma.serve_un_segreto()
                || segreto_dell_app(&DiSistema, &salvata.app)
                    .ok()
                    .flatten()
                    .is_some();
            AppIpc {
                predefinita: predefinite.contains(&salvata.app),
                piattaforma: salvata.piattaforma.chiave(),
                id: salvata.app,
                nome: salvata.nome,
                client_id: salvata.client_id,
                segreto_salvato,
                usata_da,
            }
        })
        .collect();
    let piattaforme = Piattaforma::tutte()
        .into_iter()
        .map(|piattaforma| {
            let registrazione = registrazione(piattaforma);
            PiattaformaIpc {
                chiave: piattaforma.chiave(),
                nome: piattaforma.nome(),
                segreto: piattaforma.serve_un_segreto(),
                ritorni: registrazione.ritorni,
                permessi: registrazione.permessi,
            }
        })
        .collect();
    Ok(AppNuvolaIpc { piattaforme, app })
}

/// Lo dice a tutte le finestre.
///
/// Anche da `nuvola_backup_app`: per Google l'app di serie è quella del
/// backup, e la finestra che non lo sapeva proponeva ancora quella di prima.
pub(crate) fn emetti(handle: &AppHandle) {
    match stato_ipc(handle) {
        Ok(stato) => handle.emetti("nuvola:app", stato),
        Err(guasto) => nota!("[nuvola] le app non si leggono: {guasto}"),
    }
}

/// L'errore di chi arriva mentre l'applicazione si chiude.
fn fuori_tempo() -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some("app del cloud".to_owned()),
    })
    .with_cause("lo stato dell'applicazione non c'è più")
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_cloud::portachiavi::InMemoria;

    fn app_google(app: &str, client_id: &str) -> AppSalvata {
        AppSalvata {
            app: app.to_owned(),
            piattaforma: Piattaforma::Google,
            nome: "Mia".to_owned(),
            client_id: client_id.to_owned(),
            creata_ms: 1,
        }
    }

    fn rifiuto() -> Result<(), AppError> {
        Err(AppError::new(ErrorCode::DbLocked))
    }

    #[test]
    fn l_app_ereditata_trova_il_segreto_della_3_1() {
        let chiavi = InMemoria::nuovo();
        chiavi
            .scrivi(portachiavi::GOOGLE_CLIENT_SECRET, "GOCSPX-vecchio")
            .unwrap();
        // Una voce propria fatta di soli spazi vale «non c'è».
        chiavi
            .scrivi(&portachiavi::nuvola_app_segreto(EREDITATA), "   ")
            .unwrap();
        assert_eq!(
            segreto_dell_app(&chiavi, "google-ereditata").unwrap(),
            Some("GOCSPX-vecchio".to_owned()),
            "il nome dell'app della 037 e il ripiego alla voce della 3.1 vanno insieme"
        );
        // Il ripiego è solo per lei: un'altra app Google senza segreto non
        // prende quello della 3.1.
        assert_eq!(segreto_dell_app(&chiavi, "google-k3x7q2ab").unwrap(), None);
        // E le credenziali del backup di chi aggiorna dalla 3.1 ci sono.
        let credenziali = credenziali_con(&chiavi, &app_google(EREDITATA, "id.apps")).unwrap();
        assert_eq!(credenziali.client_secret.as_deref(), Some("GOCSPX-vecchio"));
        let senza = credenziali_con(&chiavi, &app_google("google-k3x7q2ab", "id.apps"));
        assert!(matches!(
            senza.as_ref().map_err(AppError::code),
            Err(ErrorCode::CloudAppSecretMissing)
        ));
    }

    #[test]
    fn il_segreto_proprio_vince_su_quello_della_3_1() {
        let chiavi = InMemoria::nuovo();
        chiavi
            .scrivi(portachiavi::GOOGLE_CLIENT_SECRET, "GOCSPX-vecchio")
            .unwrap();
        chiavi
            .scrivi(&portachiavi::nuvola_app_segreto(EREDITATA), "GOCSPX-nuovo")
            .unwrap();
        assert_eq!(
            segreto_dell_app(&chiavi, EREDITATA).unwrap(),
            Some("GOCSPX-nuovo".to_owned())
        );
    }

    #[test]
    fn un_rifiuto_del_database_rimette_il_segreto_di_prima() {
        let app = app_google("google-k3x7q2ab", "id.apps");
        let voce = portachiavi::nuvola_app_segreto(&app.app);

        // C'era un segreto: torna quello.
        let chiavi = InMemoria::nuovo();
        chiavi.scrivi(&voce, "vecchio").unwrap();
        let esito = scrivi_app_con(&chiavi, &app, Some("nuovo"), rifiuto);
        assert!(
            esito.is_err(),
            "il rifiuto del database arriva a chi chiama"
        );
        assert_eq!(chiavi.leggi(&voce).unwrap().as_deref(), Some("vecchio"));

        // Non c'era: la voce si cancella.
        let chiavi = InMemoria::nuovo();
        let esito = scrivi_app_con(&chiavi, &app, Some("nuovo"), rifiuto);
        assert!(esito.is_err());
        assert_eq!(chiavi.leggi(&voce).unwrap(), None);

        // Col database che accetta, il segreto nuovo resta.
        let chiavi = InMemoria::nuovo();
        chiavi.scrivi(&voce, "vecchio").unwrap();
        scrivi_app_con(&chiavi, &app, Some("nuovo"), || Ok(())).unwrap();
        assert_eq!(chiavi.leggi(&voce).unwrap().as_deref(), Some("nuovo"));
    }

    #[test]
    fn salvare_il_segreto_dell_ereditata_cancella_la_voce_della_3_1() {
        let chiavi = InMemoria::nuovo();
        chiavi
            .scrivi(portachiavi::GOOGLE_CLIENT_SECRET, "GOCSPX-vecchio")
            .unwrap();
        let ereditata = app_google(EREDITATA, "id.apps");
        // Senza segreto nuovo la voce di prima serve ancora, e resta.
        scrivi_app_con(&chiavi, &ereditata, None, || Ok(())).unwrap();
        assert!(
            chiavi
                .leggi(portachiavi::GOOGLE_CLIENT_SECRET)
                .unwrap()
                .is_some()
        );
        // Un rifiuto del database non la tocca.
        let _ = scrivi_app_con(&chiavi, &ereditata, Some("GOCSPX-nuovo"), rifiuto);
        assert!(
            chiavi
                .leggi(portachiavi::GOOGLE_CLIENT_SECRET)
                .unwrap()
                .is_some()
        );
        // Il segreto scritto nella voce dell'app la rende inutile.
        scrivi_app_con(&chiavi, &ereditata, Some("GOCSPX-nuovo"), || Ok(())).unwrap();
        assert_eq!(
            chiavi.leggi(portachiavi::GOOGLE_CLIENT_SECRET).unwrap(),
            None
        );
        assert_eq!(
            segreto_dell_app(&chiavi, EREDITATA).unwrap(),
            Some("GOCSPX-nuovo".to_owned())
        );
    }

    // ── la tabella degli effetti ────────────────────────────────────────────

    const DEL_BACKUP: &str = "google-backup01";

    #[test]
    fn cambiare_identificativo_e_segreto_dell_app_del_backup_scollega_il_backup() {
        let prima = app_google(DEL_BACKUP, "vecchio.apps");
        let nuova = app_google(DEL_BACKUP, "nuovo.apps");
        let da_fare = effetti(Some(&prima), &nuova, true, Some(DEL_BACKUP), true);
        assert_eq!(da_fare.cambio, Some(Cambio::Identificativo));
        assert_eq!(da_fare.backup, SulBackup::DimenticaAccount);
        assert!(da_fare.riapri_conti);
        assert!(!da_fare.adotta);
        // Lo stesso senza segreto nuovo.
        let da_fare = effetti(Some(&prima), &nuova, false, Some(DEL_BACKUP), true);
        assert_eq!(da_fare.backup, SulBackup::DimenticaAccount);
    }

    #[test]
    fn cambiare_solo_il_segreto_dell_app_del_backup_riprende_il_token() {
        let prima = app_google(DEL_BACKUP, "stesso.apps");
        let da_fare = effetti(Some(&prima), &prima, true, Some(DEL_BACKUP), true);
        assert_eq!(da_fare.cambio, Some(Cambio::Segreto));
        assert_eq!(da_fare.backup, SulBackup::DimenticaToken);
        assert!(da_fare.riapri_conti);
    }

    #[test]
    fn un_app_google_nuova_col_backup_senza_app_la_adotta() {
        let nuova = app_google("google-nuova001", "nuovo.apps");
        let da_fare = effetti(None, &nuova, true, None, true);
        assert!(da_fare.adotta);
        assert_eq!(da_fare.cambio, None);
        assert_eq!(da_fare.backup, SulBackup::DimenticaAccount);
        assert!(!da_fare.riapri_conti);
        // Col backup che ha già un'app, la nuova non si adotta.
        let da_fare = effetti(None, &nuova, true, Some(DEL_BACKUP), true);
        assert!(!da_fare.adotta);
        assert_eq!(da_fare.backup, SulBackup::Niente);
        // Senza il backup in mano (un'app non Google) non si adotta niente.
        let da_fare = effetti(None, &nuova, true, None, false);
        assert!(!da_fare.adotta);
        assert_eq!(da_fare.backup, SulBackup::Niente);
    }

    #[test]
    fn cambiare_un_app_che_il_backup_non_usa_non_tocca_il_backup() {
        let prima = app_google("google-altra001", "vecchio.apps");
        let nuova = app_google("google-altra001", "nuovo.apps");
        let da_fare = effetti(Some(&prima), &nuova, true, Some(DEL_BACKUP), true);
        assert_eq!(da_fare.cambio, Some(Cambio::Identificativo));
        assert_eq!(da_fare.backup, SulBackup::Niente);
        assert!(
            da_fare.riapri_conti,
            "i conti legati a lei si riaprono lo stesso"
        );
        let solo_segreto = effetti(Some(&prima), &prima, true, Some(DEL_BACKUP), true);
        assert_eq!(solo_segreto.backup, SulBackup::Niente);
        assert!(solo_segreto.riapri_conti);
    }

    #[test]
    fn salvare_senza_cambiare_niente_non_riapre_i_conti() {
        let prima = app_google(DEL_BACKUP, "stesso.apps");
        let da_fare = effetti(Some(&prima), &prima, false, Some(DEL_BACKUP), true);
        assert_eq!(da_fare.cambio, None);
        assert_eq!(da_fare.backup, SulBackup::Niente);
        assert!(!da_fare.riapri_conti);
        assert!(!da_fare.adotta);
    }
}
