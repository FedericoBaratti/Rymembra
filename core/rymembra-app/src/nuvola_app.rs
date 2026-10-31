//! Le app dell'utente (migrazione 037) e chi le usa: il backup, i conti del
//! cloud.
//!
//! # Cosa sta qui e cosa no
//!
//! Qui il registro — le righe di `nuvola_app` — e i legami: quale app usa il
//! backup (`nuvola.app` in `settings`), con quale app è collegato ogni conto
//! (`nuvola_conti.app` e `.client`). Le forme valide stanno nel dominio
//! ([`rymembra_domain::nuvola_app`]); il segreto delle app Google nel
//! portachiavi, dove questo crate non arriva; cosa succede ai conti quando
//! un'app cambia — spegnere i loro permessi, riaprirli — lo decide il desktop,
//! che tiene i fili.
//!
//! # Quale app, se nessuno la sceglie
//!
//! La [`predefinita`]. Per Google è quella del backup, se il backup ne ha una:
//! la decisione dell'utente è stata «una sola app, condivisa», e chi aggiunge
//! una cartella di Drive senza scegliere vuole la stessa. Per le altre, e per
//! Google senza backup, la prima registrata.

use rusqlite::Connection;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola_app::{Piattaforma, difetto_id, forma_canonica, nome_pulito};

use crate::library::db_error;
use crate::nuvola::{ContoSalvato, con_un_savepoint};
use crate::settings;

/// La chiave di `settings` con l'app del backup (e della sincronia, che passa
/// dallo stesso consenso).
pub const CHIAVE_DEL_BACKUP: &str = "nuvola.app";

/// Un'app, come sta in `nuvola_app`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSalvata {
    /// `google-k3x7q2ab`: non si riusa mai (vedi la 037).
    pub app: String,
    /// Presso chi è registrata.
    pub piattaforma: Piattaforma,
    /// Il nome scelto dall'utente, per i menu.
    pub nome: String,
    /// L'identificativo che la piattaforma le ha dato.
    pub client_id: String,
    /// Quando è stata registrata in Rymembra: decide la predefinita.
    pub creata_ms: i64,
}

/// Tutte le app, nell'ordine in cui sono state registrate.
///
/// Un'app di una piattaforma che questa versione non conosce — scritta da una
/// versione più nuova — si salta, come i conti in `nuvola::conti`.
///
/// # Errori
///
/// Quelli del database.
pub fn elenco(conn: &Connection) -> Result<Vec<AppSalvata>, AppError> {
    leggi(conn, "", [])
}

/// Un'app sola, se c'è.
///
/// # Errori
///
/// Quelli del database.
pub fn una(conn: &Connection, app: &str) -> Result<Option<AppSalvata>, AppError> {
    Ok(leggi(conn, "WHERE app = ?1", [app])?.into_iter().next())
}

fn leggi<P: rusqlite::Params>(
    conn: &Connection,
    dove: &str,
    parametri: P,
) -> Result<Vec<AppSalvata>, AppError> {
    let sql = format!(
        "SELECT app, piattaforma, nome, client_id, creata_ms
           FROM nuvola_app {dove} ORDER BY creata_ms, app"
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|err| db_error("lettura delle app del cloud", &err))?;
    let righe = statement
        .query_map(parametri, |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                riga.get::<_, String>(1)?,
                riga.get::<_, String>(2)?,
                riga.get::<_, String>(3)?,
                riga.get::<_, i64>(4)?,
            ))
        })
        .map_err(|err| db_error("lettura delle app del cloud", &err))?;
    let mut fuori = Vec::new();
    for riga in righe {
        let (app, piattaforma, nome, client_id, creata_ms) =
            riga.map_err(|err| db_error("lettura delle app del cloud", &err))?;
        let Some(piattaforma) = Piattaforma::da_chiave(&piattaforma) else {
            continue;
        };
        fuori.push(AppSalvata {
            app,
            piattaforma,
            nome,
            client_id,
            creata_ms,
        });
    }
    Ok(fuori)
}

/// Scrive un'app nuova, o il nome e l'identificativo di una che c'è.
///
/// La piattaforma di un'app che c'è non cambia: un'app Google che diventasse
/// Microsoft porterebbe con sé conti di Drive che Microsoft non conosce.
/// Il nome si salva ripulito degli spazi ai bordi, l'identificativo nella sua
/// [`forma_canonica`].
///
/// # Errori
///
/// - `cloud.appIdInvalid` se l'identificativo non ha la forma della
///   piattaforma, o il nome è vuoto o troppo lungo;
/// - `cloud.appDuplicate` se un'altra app ha già lo stesso identificativo;
/// - quelli del database.
pub fn salva(conn: &Connection, app: &AppSalvata) -> Result<(), AppError> {
    let Some(nome) = nome_pulito(&app.nome) else {
        return Err(AppError::new(ErrorCode::CloudAppIdInvalid).with_cause("nome dell'app"));
    };
    if let Some(difetto) = difetto_id(app.piattaforma, &app.client_id) {
        return Err(AppError::new(ErrorCode::CloudAppIdInvalid)
            .with_cause(format!("identificativo dell'app: {difetto:?}")));
    }
    let esito = conn.execute(
        "INSERT INTO nuvola_app (app, piattaforma, nome, client_id, creata_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(app) DO UPDATE SET nome = excluded.nome, client_id = excluded.client_id",
        rusqlite::params![
            app.app,
            app.piattaforma.chiave(),
            nome,
            forma_canonica(app.piattaforma, &app.client_id),
            app.creata_ms,
        ],
    );
    match esito {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(errore, _))
            if errore.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            Err(AppError::new(ErrorCode::CloudAppDuplicate))
        }
        Err(err) => Err(db_error("scrittura di un'app del cloud", &err)),
    }
}

/// Chi usava un'app che si è appena tolta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tolta {
    /// I conti che erano legati a lei, e ora non lo sono più.
    pub conti: Vec<String>,
    /// Il backup la usava.
    pub backup: bool,
}

/// Toglie un'app, e la toglie a chi la usava.
///
/// I conti restano — con le loro voci, le loro radici e quindi i loro brani —
/// e diventano «da ricollegare»: si slegano mettendo `app` a NULL, mai
/// togliendo il conto. Il backup perde la sua app e torna «non configurato».
/// Il segreto dell'app sta nel portachiavi, e lo cancella chi chiama.
///
/// # Errori
///
/// Quelli del database; in quel caso non si è tolto niente.
pub fn togli(conn: &Connection, app: &str) -> Result<Tolta, AppError> {
    let mut tolta = Tolta::default();
    con_un_savepoint(conn, "nuvola_togli_app", || {
        tolta.conti = conti_legati(conn, app)?;
        conn.execute("UPDATE nuvola_conti SET app = NULL WHERE app = ?1", [app])
            .map_err(|err| db_error("scollegamento dei conti di un'app", &err))?;
        if settings::read(conn, CHIAVE_DEL_BACKUP)?.as_deref() == Some(app) {
            settings::forget(conn, CHIAVE_DEL_BACKUP)?;
            tolta.backup = true;
        }
        conn.execute("DELETE FROM nuvola_app WHERE app = ?1", [app])
            .map_err(|err| db_error("cancellazione di un'app del cloud", &err))?;
        Ok(())
    })?;
    Ok(tolta)
}

/// I conti collegati con un'app.
///
/// # Errori
///
/// Quelli del database.
pub fn conti_legati(conn: &Connection, app: &str) -> Result<Vec<String>, AppError> {
    let mut statement = conn
        .prepare("SELECT conto FROM nuvola_conti WHERE app = ?1 ORDER BY conto")
        .map_err(|err| db_error("lettura dei conti di un'app", &err))?;
    let righe = statement
        .query_map([app], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error("lettura dei conti di un'app", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura dei conti di un'app", &err))
}

/// L'app del backup, se ne ha una e c'è ancora.
///
/// # Errori
///
/// Quelli del database.
pub fn app_del_backup(conn: &Connection) -> Result<Option<AppSalvata>, AppError> {
    let Some(app) = settings::read(conn, CHIAVE_DEL_BACKUP)? else {
        return Ok(None);
    };
    Ok(una(conn, &app)?.filter(|app| app.piattaforma == Piattaforma::Google))
}

/// Lega il backup a un'app Google, o lo slega.
///
/// # Errori
///
/// `cloud.appIdInvalid` se l'app non c'è o non è di Google; quelli del
/// database.
pub fn lega_il_backup(conn: &Connection, app: Option<&str>) -> Result<(), AppError> {
    let Some(app) = app else {
        return settings::forget(conn, CHIAVE_DEL_BACKUP).map(|_| ());
    };
    match una(conn, app)? {
        Some(trovata) if trovata.piattaforma == Piattaforma::Google => {
            settings::write(conn, CHIAVE_DEL_BACKUP, app)
        }
        _ => Err(AppError::new(ErrorCode::CloudAppIdInvalid)
            .with_cause("il backup vuole un'app Google che ci sia")),
    }
}

/// L'app da usare quando nessuno la sceglie: vedi il `//!`.
///
/// # Errori
///
/// Quelli del database.
pub fn predefinita(
    conn: &Connection,
    piattaforma: Piattaforma,
) -> Result<Option<AppSalvata>, AppError> {
    if piattaforma == Piattaforma::Google
        && let Some(del_backup) = app_del_backup(conn)?
    {
        return Ok(Some(del_backup));
    }
    Ok(elenco(conn)?
        .into_iter()
        .find(|app| app.piattaforma == piattaforma))
}

/// Se il consenso di un conto vale per l'app che ha oggi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatoDelConto {
    /// Il conto ha la sua app, e l'app ha l'identificativo di quando il conto
    /// ha dato il consenso (o non si sa quale avesse: allora si prova).
    Pronta(AppSalvata),
    /// L'app c'è, ma ha cambiato identificativo: il refresh token del conto è
    /// dell'app di prima. Si ricollega.
    Cambiata(AppSalvata),
    /// L'app non c'è: tolta, o mai avuta. Si ricollega con un'app.
    Mancante,
}

/// Lo stato di un conto rispetto alla sua app.
///
/// Lo decide il database e nient'altro: cambiare l'identificativo di un'app
/// non tocca il portachiavi, e rimetterlo com'era riporta i conti com'erano.
///
/// # Errori
///
/// Quelli del database.
pub fn stato_del_conto(conn: &Connection, conto: &ContoSalvato) -> Result<StatoDelConto, AppError> {
    let Some(app) = conto.app.as_deref() else {
        return Ok(StatoDelConto::Mancante);
    };
    let Some(app) = una(conn, app)? else {
        return Ok(StatoDelConto::Mancante);
    };
    if conto.servizio.piattaforma() != Some(app.piattaforma) {
        // Un legame che non può essere stato scritto da qui: meglio
        // ricollegare che mandare a Microsoft un refresh token di Google.
        return Ok(StatoDelConto::Mancante);
    }
    let cambiata = conto
        .client
        .as_deref()
        .is_some_and(|client| client != app.client_id);
    Ok(if cambiata {
        StatoDelConto::Cambiata(app)
    } else {
        StatoDelConto::Pronta(app)
    })
}

#[cfg(test)]
mod prove {
    use rymembra_domain::nuvola::Servizio;

    use super::*;
    use crate::nuvola::{conti, lega_conto, salva_conto};

    const GUID_A: &str = "0f2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6f";
    const GUID_B: &str = "1f2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6f";
    const GOOGLE_A: &str = "111-aaa.apps.googleusercontent.com";
    const GOOGLE_B: &str = "222-bbb.apps.googleusercontent.com";

    fn db() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    fn app(app: &str, piattaforma: Piattaforma, client_id: &str, creata_ms: i64) -> AppSalvata {
        AppSalvata {
            app: app.to_owned(),
            piattaforma,
            nome: piattaforma.nome().to_owned(),
            client_id: client_id.to_owned(),
            creata_ms,
        }
    }

    fn conto(
        conto: &str,
        servizio: Servizio,
        app: Option<&str>,
        client: Option<&str>,
    ) -> ContoSalvato {
        ContoSalvato {
            conto: conto.to_owned(),
            servizio,
            nome: "chi@esempio.it".to_owned(),
            dettagli: None,
            cursore: None,
            ultimo_ms: None,
            app: app.map(ToOwned::to_owned),
            client: client.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn un_app_si_salva_ripulita_e_si_rilegge() {
        let c = db();
        let mut nuova = app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_A, 5);
        nuova.nome = "  La mia app \n".to_owned();
        nuova.client_id = format!(" {GUID_A}\n");
        salva(&c, &nuova).expect("salvata");
        let letta = una(&c, "microsoft-aaaaaaaa")
            .expect("lettura")
            .expect("c'è");
        assert_eq!(letta.nome, "La mia app");
        assert_eq!(letta.client_id, GUID_A);
        assert_eq!(elenco(&c).expect("elenco"), [letta]);
        assert_eq!(una(&c, "microsoft-zzzzzzzz").expect("lettura"), None);
    }

    #[test]
    fn un_identificativo_storto_o_doppio_non_entra() {
        let c = db();
        let storta = app(
            "microsoft-aaaaaaaa",
            Piattaforma::Microsoft,
            "non-un-guid",
            1,
        );
        assert_eq!(
            salva(&c, &storta).expect_err("storta").code().kind().code(),
            "cloud.appIdInvalid"
        );
        let mut senza_nome = app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_A, 1);
        senza_nome.nome = "  ".to_owned();
        assert_eq!(
            salva(&c, &senza_nome)
                .expect_err("senza nome")
                .code()
                .kind()
                .code(),
            "cloud.appIdInvalid"
        );
        salva(
            &c,
            &app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_A, 1),
        )
        .expect("prima");
        let doppia = app("microsoft-bbbbbbbb", Piattaforma::Microsoft, GUID_A, 2);
        assert_eq!(
            salva(&c, &doppia).expect_err("doppia").code().kind().code(),
            "cloud.appDuplicate"
        );
        // Lo stesso GUID in maiuscolo è la stessa app.
        let maiuscola = app(
            "microsoft-cccccccc",
            Piattaforma::Microsoft,
            &GUID_A.to_ascii_uppercase(),
            3,
        );
        assert_eq!(
            salva(&c, &maiuscola)
                .expect_err("doppia")
                .code()
                .kind()
                .code(),
            "cloud.appDuplicate"
        );
        assert!(elenco(&c).expect("elenco").len() == 1);
    }

    #[test]
    fn riscrivere_un_app_non_ne_cambia_la_piattaforma() {
        let c = db();
        salva(
            &c,
            &app("google-aaaaaaaa", Piattaforma::Google, GOOGLE_A, 1),
        )
        .expect("nata");
        let mut altra = app("google-aaaaaaaa", Piattaforma::Google, GOOGLE_B, 99);
        altra.nome = "Rinominata".to_owned();
        salva(&c, &altra).expect("riscritta");
        let letta = una(&c, "google-aaaaaaaa").expect("lettura").expect("c'è");
        assert_eq!(letta.client_id, GOOGLE_B);
        assert_eq!(letta.nome, "Rinominata");
        assert_eq!(letta.creata_ms, 1, "la data di nascita resta");
    }

    #[test]
    fn la_predefinita_di_google_e_quella_del_backup() {
        let c = db();
        assert_eq!(predefinita(&c, Piattaforma::Google).expect("nessuna"), None);
        salva(
            &c,
            &app("google-aaaaaaaa", Piattaforma::Google, GOOGLE_A, 1),
        )
        .expect("a");
        salva(
            &c,
            &app("google-bbbbbbbb", Piattaforma::Google, GOOGLE_B, 2),
        )
        .expect("b");
        salva(
            &c,
            &app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_A, 0),
        )
        .expect("m");
        let prima = |c: &Connection, p| predefinita(c, p).expect("lettura").map(|a| a.app);
        assert_eq!(
            prima(&c, Piattaforma::Google).as_deref(),
            Some("google-aaaaaaaa")
        );
        lega_il_backup(&c, Some("google-bbbbbbbb")).expect("legato");
        assert_eq!(
            prima(&c, Piattaforma::Google).as_deref(),
            Some("google-bbbbbbbb")
        );
        assert_eq!(
            prima(&c, Piattaforma::Microsoft).as_deref(),
            Some("microsoft-aaaaaaaa")
        );
        assert_eq!(prima(&c, Piattaforma::Dropbox), None);
        // Il backup vuole un'app Google che ci sia.
        assert!(lega_il_backup(&c, Some("microsoft-aaaaaaaa")).is_err());
        assert!(lega_il_backup(&c, Some("google-zzzzzzzz")).is_err());
        lega_il_backup(&c, None).expect("slegato");
        assert_eq!(app_del_backup(&c).expect("lettura"), None);
    }

    #[test]
    fn togliere_un_app_slega_i_conti_e_lascia_le_loro_cartelle() {
        let c = db();
        salva(
            &c,
            &app("google-aaaaaaaa", Piattaforma::Google, GOOGLE_A, 1),
        )
        .expect("a");
        lega_il_backup(&c, Some("google-aaaaaaaa")).expect("backup");
        salva_conto(
            &c,
            &conto(
                "gdrive-aaaaaaaa",
                Servizio::GoogleDrive,
                Some("google-aaaaaaaa"),
                Some(GOOGLE_A),
            ),
        )
        .expect("conto");
        c.execute_batch(
            "INSERT INTO nuvola_voci (conto, id, nome, segmento, cartella, dimensione, modificato_ms)
             VALUES ('gdrive-aaaaaaaa', '1', 'a.flac', 'a.flac', 0, 1, 1);
             INSERT INTO nuvola_radici (percorso, conto, cima, completa)
             VALUES ('nuvola://gdrive-aaaaaaaa/Musica', 'gdrive-aaaaaaaa', 'X', 1);",
        )
        .expect("specchio");

        let tolta = togli(&c, "google-aaaaaaaa").expect("tolta");
        assert_eq!(
            tolta,
            Tolta {
                conti: vec!["gdrive-aaaaaaaa".to_owned()],
                backup: true
            }
        );
        assert!(elenco(&c).expect("elenco").is_empty());
        let rimasto = conti(&c).expect("conti").remove(0);
        assert_eq!(rimasto.app, None, "il conto resta, slegato");
        assert_eq!(
            stato_del_conto(&c, &rimasto).expect("stato"),
            StatoDelConto::Mancante
        );
        for tabella in ["nuvola_voci", "nuvola_radici"] {
            let righe: i64 = c
                .query_row(&format!("SELECT COUNT(*) FROM {tabella}"), [], |r| r.get(0))
                .expect("conto");
            assert_eq!(righe, 1, "{tabella}: le cartelle restano");
        }
        assert_eq!(app_del_backup(&c).expect("lettura"), None);
    }

    #[test]
    fn cambiare_l_identificativo_e_rimetterlo_riporta_i_conti() {
        let c = db();
        salva(
            &c,
            &app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_A, 1),
        )
        .expect("a");
        let collegato = conto(
            "onedrive-aaaaaaaa",
            Servizio::OneDrive,
            Some("microsoft-aaaaaaaa"),
            Some(GUID_A),
        );
        salva_conto(&c, &collegato).expect("conto");
        let stato = |c: &Connection| {
            let letto = conti(c).expect("conti").remove(0);
            stato_del_conto(c, &letto).expect("stato")
        };
        assert!(matches!(stato(&c), StatoDelConto::Pronta(_)));

        salva(
            &c,
            &app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_B, 1),
        )
        .expect("B");
        assert!(matches!(stato(&c), StatoDelConto::Cambiata(_)));

        salva(
            &c,
            &app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_A, 1),
        )
        .expect("A");
        assert!(matches!(stato(&c), StatoDelConto::Pronta(_)), "A → B → A");

        // Ricollegato con l'app nuova: il client del consenso è quello di oggi.
        salva(
            &c,
            &app("microsoft-aaaaaaaa", Piattaforma::Microsoft, GUID_B, 1),
        )
        .expect("B");
        lega_conto(
            &c,
            "onedrive-aaaaaaaa",
            Some("microsoft-aaaaaaaa"),
            Some(GUID_B),
        )
        .expect("ricollegato");
        assert!(matches!(stato(&c), StatoDelConto::Pronta(_)));
    }

    #[test]
    fn un_conto_senza_app_o_con_l_app_di_un_altra_piattaforma_va_ricollegato() {
        let c = db();
        salva(
            &c,
            &app("google-aaaaaaaa", Piattaforma::Google, GOOGLE_A, 1),
        )
        .expect("a");
        let webdav = conto("webdav-aaaaaaaa", Servizio::WebDav, None, None);
        assert_eq!(
            stato_del_conto(&c, &webdav).expect("stato"),
            StatoDelConto::Mancante
        );
        let storto = conto(
            "onedrive-aaaaaaaa",
            Servizio::OneDrive,
            Some("google-aaaaaaaa"),
            Some(GOOGLE_A),
        );
        assert_eq!(
            stato_del_conto(&c, &storto).expect("stato"),
            StatoDelConto::Mancante
        );
        // Un client che non si sa: si prova.
        let senza_client = conto(
            "gdrive-aaaaaaaa",
            Servizio::GoogleDrive,
            Some("google-aaaaaaaa"),
            None,
        );
        assert!(matches!(
            stato_del_conto(&c, &senza_client).expect("stato"),
            StatoDelConto::Pronta(_)
        ));
    }
}
