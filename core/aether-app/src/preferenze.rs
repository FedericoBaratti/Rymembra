//! Le due preferenze della finestra che stavano fuori dal nucleo.
//!
//! # Il tema, e perché torna dentro
//!
//! `aether.tema` viveva in `localStorage` (`apps/desktop/src/tema.ts`), e la
//! ragione era buona quando è stato scritto: la scelta fra chiaro e scuro non è
//! un dato della libreria, è una preferenza della finestra, e il nucleo non
//! aveva niente da decidere.
//!
//! Quel che è cambiato è che adesso esistono due cose che leggono *tutte* le
//! preferenze: il **backup su Drive** e il **profilo**. Una preferenza in
//! `localStorage` non finisce in nessuno dei due — quindi chi ripristina un
//! backup si ritrova la skin giusta e il tema sbagliato, e chi porta il proprio
//! profilo su un altro computer si porta tutto tranne quello. Non è un difetto
//! di `localStorage`: è che il confine era stato tracciato prima che esistesse
//! qualcosa che lo attraversa.
//!
//! Il ripiego resta: la prima apertura dopo l'aggiornamento non trova la chiave
//! e legge quella vecchia. È il motivo per cui [`tema`] restituisce un
//! `Option` — «mai scelto» e «scelto sistema» sono due stati diversi, e
//! confonderli vorrebbe dire buttare via la scelta di chi aveva fissato il tema
//! chiaro.
//!
//! # Le scorciatoie, e perché il nucleo non le capisce
//!
//! Si conservano come JSON **opaco**: qui si controlla che sia JSON valido e
//! nient'altro. Non è pigrizia — è che i nomi dei comandi (`alterna`, `cerca`,
//! `inRiproduzione`) appartengono alla finestra, e un nucleo che li validasse
//! andrebbe ricompilato per aggiungere una scorciatoia. La stessa disciplina di
//! `player.eq.presets`, che è un elenco di curve di cui il database non ha
//! nessuna opinione.
//!
//! Quel che invece si controlla è che il testo sia JSON: una stringa storta
//! scritta qui dentro non darebbe nessun sintomo finché la finestra non prova a
//! leggerla, cioè al riavvio successivo — e a quel punto tutte le scorciatoie
//! sparirebbero insieme, senza che niente colleghi le due cose.

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::Connection;

use crate::settings;

/// Chiaro, scuro, o quel che dice il sistema.
pub const CHIAVE_TEMA: &str = "ui.theme";

/// Le associazioni fra tasti e comandi, come JSON.
pub const CHIAVE_SCORCIATOIE: &str = "ui.shortcuts";

/// Le tre scelte del tema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tema {
    /// Sempre scuro.
    Scuro,
    /// Sempre chiaro.
    Chiaro,
    /// Quel che dice il sistema operativo.
    Sistema,
}

impl Tema {
    /// Il nome stabile, quello che finisce nel database.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Scuro => "scuro",
            Self::Chiaro => "chiaro",
            Self::Sistema => "sistema",
        }
    }

    /// Dal nome stabile.
    ///
    /// `None` per qualunque altra cosa: una riga scritta a mano con `sqlite3` o
    /// arrivata da un profilo di una versione futura non deve poter far
    /// comparire un tema che non esiste.
    #[must_use]
    pub fn dal_nome(nome: &str) -> Option<Self> {
        match nome {
            "scuro" => Some(Self::Scuro),
            "chiaro" => Some(Self::Chiaro),
            "sistema" => Some(Self::Sistema),
            _ => None,
        }
    }
}

/// Il tema scelto, o `None` se non è mai stato scelto.
///
/// La distinzione conta: chi non ha mai scelto va servito con la preferenza che
/// era rimasta in `localStorage`, chi ha scelto «sistema» no.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn tema(connection: &Connection) -> Result<Option<Tema>, AppError> {
    Ok(settings::read(connection, CHIAVE_TEMA)?.and_then(|nome| Tema::dal_nome(&nome)))
}

/// Scrive il tema.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn imposta_tema(connection: &Connection, tema: Tema) -> Result<(), AppError> {
    settings::write(connection, CHIAVE_TEMA, tema.nome())
}

/// Le scorciatoie come sono state scritte, o `None` se sono quelle di serie.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn scorciatoie(connection: &Connection) -> Result<Option<String>, AppError> {
    settings::read(connection, CHIAVE_SCORCIATOIE)
}

/// Scrive le scorciatoie. Una stringa vuota rimette quelle di serie.
///
/// # Errori
///
/// `settings.corrupt` se il testo non è JSON — vedi il preambolo sul perché il
/// controllo è questo e non di più. `db.queryFailed` per il resto.
pub fn imposta_scorciatoie(connection: &Connection, json: &str) -> Result<(), AppError> {
    let json = json.trim();
    if json.is_empty() {
        // Cancellare invece di scrivere `""`: assente vuol dire «quelle di
        // serie», e una stringa vuota vorrebbe dire «nessuna scorciatoia».
        settings::forget(connection, CHIAVE_SCORCIATOIE)?;
        return Ok(());
    }
    serde_json::from_str::<serde_json::Value>(json).map_err(|err| {
        AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!("le scorciatoie non sono JSON: {err}"))
    })?;
    settings::write(connection, CHIAVE_SCORCIATOIE, json)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn libreria() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    #[test]
    fn mai_scelto_non_e_sistema() {
        let c = libreria();
        assert_eq!(
            tema(&c),
            Ok(None),
            "senza questa distinzione la scelta rimasta in localStorage andrebbe persa"
        );
        imposta_tema(&c, Tema::Sistema).expect("scrittura");
        assert_eq!(tema(&c), Ok(Some(Tema::Sistema)));
    }

    #[test]
    fn i_tre_temi_vanno_e_tornano() {
        let c = libreria();
        for atteso in [Tema::Scuro, Tema::Chiaro, Tema::Sistema] {
            imposta_tema(&c, atteso).expect("scrittura");
            assert_eq!(tema(&c), Ok(Some(atteso)));
        }
    }

    #[test]
    fn un_tema_che_non_esiste_vale_come_mai_scelto() {
        let c = libreria();
        settings::write(&c, CHIAVE_TEMA, "seppia").expect("scrittura");
        assert_eq!(
            tema(&c),
            Ok(None),
            "una riga arrivata da una versione futura non deve inventare un tema"
        );
    }

    #[test]
    fn le_scorciatoie_devono_essere_json() {
        let c = libreria();
        let err = imposta_scorciatoie(&c, "alterna=Space").expect_err("non è JSON");
        assert_eq!(err.code().kind().code(), "settings.corrupt");
        assert_eq!(
            scorciatoie(&c),
            Ok(None),
            "e quel che non è JSON non viene scritto"
        );

        imposta_scorciatoie(&c, r#"{"alterna":[" "]}"#).expect("JSON valido");
        assert_eq!(scorciatoie(&c), Ok(Some(r#"{"alterna":[" "]}"#.to_owned())));
    }

    #[test]
    fn svuotare_rimette_quelle_di_serie() {
        let c = libreria();
        imposta_scorciatoie(&c, r#"{"cerca":["k"]}"#).expect("scrittura");
        imposta_scorciatoie(&c, "  ").expect("svuotamento");
        assert_eq!(
            scorciatoie(&c),
            Ok(None),
            "assente vuol dire «quelle di serie»; una stringa vuota vorrebbe dire «nessuna»"
        );
    }
}
