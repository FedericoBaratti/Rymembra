//! Le app che l'utente registra presso Google, Microsoft e Dropbox perché
//! Rymembra entri nei suoi account.
//!
//! # Perché ognuno la sua
//!
//! Un programma desktop non sa tenere un segreto: quel che sta nel binario,
//! `strings` lo trova. Un'app comune a tutti metterebbe ogni utente sulla
//! stessa quota — che chiunque può consumare copiandone l'identificativo — e
//! sullo stesso editore, cioè sulla stessa verifica: Google chiede una revisione
//! di sicurezza per `drive.readonly`, Dropbox l'approvazione oltre i cinquanta
//! utenti, Microsoft un editore verificato perché un tenant aziendale accetti il
//! consenso. Con un'app propria la quota è di chi la usa, il consenso lo dà a sé
//! stesso, e nessuno può spegnerla agli altri.
//!
//! # Cosa decide questo modulo
//!
//! Solo le forme: chi rilascia un'app ([`Piattaforma`]) e come si riconosce un
//! identificativo scritto bene ([`difetto_id`]), prima che vada nel database e
//! in ogni invito di consenso. Dove le app si salvano lo sa `rymembra-app`; con
//! quali indirizzi e quali permessi si registrano lo sa `rymembra-cloud`, che li
//! usa.

/// Chi rilascia l'app: il posto in cui l'utente la registra.
///
/// Non è il servizio: un'app Google serve sia il backup sia le cartelle di
/// Drive, e con lei due consensi diversi. Un `enum`, come
/// [`crate::nuvola::Servizio`], per nominarle — nel database, negli errori,
/// nelle schermate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Piattaforma {
    /// Google Cloud Console: il backup, la sincronia e le cartelle di Drive.
    Google,
    /// Microsoft Entra: le cartelle di OneDrive.
    Microsoft,
    /// La App Console di Dropbox.
    Dropbox,
}

impl Piattaforma {
    /// Il nome stabile: sta nel database (`nuvola_app.piattaforma`, col suo
    /// `CHECK`) e nell'identificativo di ogni app. Non cambia mai.
    #[must_use]
    pub const fn chiave(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
            Self::Dropbox => "dropbox",
        }
    }

    /// Il nome per le schermate.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
            Self::Dropbox => "Dropbox",
        }
    }

    /// Dal nome stabile.
    #[must_use]
    pub fn da_chiave(chiave: &str) -> Option<Self> {
        Self::tutte().into_iter().find(|p| p.chiave() == chiave)
    }

    /// Tutte, nell'ordine in cui le schermate le mostrano.
    #[must_use]
    pub const fn tutte() -> [Self; 3] {
        [Self::Microsoft, Self::Google, Self::Dropbox]
    }

    /// L'app ha anche un segreto?
    ///
    /// Solo Google: il suo client «App desktop» lo vuole nello scambio del
    /// codice anche con PKCE, pur dichiarando che lì non è un segreto.
    /// Microsoft e Dropbox trattano un'app desktop da client pubblico, e un
    /// segreto mandato a loro sarebbe un errore — o peggio, un segreto vero
    /// finito dove non doveva.
    #[must_use]
    pub const fn serve_un_segreto(self) -> bool {
        matches!(self, Self::Google)
    }
}

/// Cosa non va in un identificativo o in un segreto scritto a mano.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifettoId {
    /// Non c'è.
    Vuoto,
    /// Ha uno spazio dentro: quasi sempre un pezzo di riga copiato di troppo.
    Spazi,
    /// Non ha la forma che quella piattaforma dà ai suoi identificativi.
    Forma,
    /// Più lungo di quanto una piattaforma ne dia mai.
    Lungo,
}

/// La lunghezza oltre la quale un identificativo o un segreto non è credibile.
///
/// Il più lungo dei tre, quello di Google, sta sotto gli ottanta caratteri.
/// Il tetto è largo apposta: serve a fermare un paragrafo incollato per sbaglio,
/// non a indovinare il formato di domani.
pub const LUNGHEZZA_MASSIMA: usize = 200;

/// Il suffisso di ogni identificativo di client Google.
const SUFFISSO_GOOGLE: &str = ".apps.googleusercontent.com";

/// Cosa non va nell'identificativo dell'app, se qualcosa non va.
///
/// Si controlla il testo già ripulito degli spazi ai bordi: chi incolla porta
/// spesso un a capo con sé, e rifiutarlo per quello sarebbe pedanteria.
///
/// - **Microsoft:** l'«ID applicazione (client)» è un GUID. Accanto, nella
///   stessa pagina, ci sono l'ID della directory e quello dell'oggetto, anche
///   loro GUID: da qui non si distinguono, e lo dicono i passi.
/// - **Dropbox:** la «App key» è fatta di lettere minuscole e cifre.
/// - **Google:** l'ID client finisce con `.apps.googleusercontent.com`.
#[must_use]
pub fn difetto_id(piattaforma: Piattaforma, id: &str) -> Option<DifettoId> {
    let id = id.trim();
    if let Some(difetto) = difetto_comune(id) {
        return Some(difetto);
    }
    let buono = match piattaforma {
        Piattaforma::Microsoft => e_un_guid(id),
        Piattaforma::Dropbox => {
            (8..=32).contains(&id.len())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        }
        Piattaforma::Google => id.strip_suffix(SUFFISSO_GOOGLE).is_some_and(|testa| {
            !testa.is_empty()
                && testa
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }),
    };
    (!buono).then_some(DifettoId::Forma)
}

/// L'identificativo come lo si salva e lo si confronta.
///
/// Ripulito ai bordi; per Microsoft anche in minuscolo, perché un GUID non
/// distingue le maiuscole — Entra accetta l'uno e l'altro — e lo stesso ID
/// scritto in due modi non deve diventare due app, né sembrare cambiato a chi
/// lo riscrive. Le altre piattaforme restano come sono: una App key di Dropbox
/// è già minuscola, e di un ID Google non si giura niente.
#[must_use]
pub fn forma_canonica(piattaforma: Piattaforma, id: &str) -> String {
    let id = id.trim();
    match piattaforma {
        Piattaforma::Microsoft => id.to_ascii_lowercase(),
        Piattaforma::Google | Piattaforma::Dropbox => id.to_owned(),
    }
}

/// Le virgolette che un segreto non ha mai: le dritte, le tipografiche, le
/// caporali e l'accento grave. Arrivano con un segreto copiato da un file
/// JSON, da un messaggio o da un documento.
pub const VIRGOLETTE: [char; 9] = [
    '"', '\'', '`', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '\u{ab}', '\u{bb}',
];

/// Cosa non va nel segreto di un'app Google, se qualcosa non va.
///
/// Del formato non si giura niente — oggi comincia con `GOCSPX-`, ieri no —:
/// solo che ci sia, senza spazi dentro e di una lunghezza credibile, e senza
/// [`VIRGOLETTE`]. Un segreto incollato fra virgolette si salvava, e lo
/// scambio del consenso falliva dopo con un `invalid_client` che non diceva
/// perché: le virgolette si rifiutano qui (`Forma`), come per un
/// identificativo, e non si tolgono di nascosto.
#[must_use]
pub fn difetto_segreto(segreto: &str) -> Option<DifettoId> {
    let segreto = segreto.trim();
    difetto_comune(segreto).or_else(|| segreto.contains(VIRGOLETTE).then_some(DifettoId::Forma))
}

/// I difetti che valgono per tutti.
fn difetto_comune(testo: &str) -> Option<DifettoId> {
    if testo.is_empty() {
        Some(DifettoId::Vuoto)
    } else if testo.len() > LUNGHEZZA_MASSIMA {
        Some(DifettoId::Lungo)
    } else if testo.chars().any(char::is_whitespace) {
        Some(DifettoId::Spazi)
    } else {
        None
    }
}

/// `8-4-4-4-12` cifre esadecimali.
fn e_un_guid(id: &str) -> bool {
    let gruppi: Vec<&str> = id.split('-').collect();
    gruppi.len() == 5
        && gruppi.iter().zip([8, 4, 4, 4, 12]).all(|(gruppo, quante)| {
            gruppo.len() == quante && gruppo.bytes().all(|b| b.is_ascii_hexdigit())
        })
}

/// Quanti caratteri può avere il nome di un'app.
pub const NOME_MASSIMO: usize = 60;

/// Il nome di un'app ripulito, o `None` se non va.
///
/// È quello che l'utente legge nel menu «Con l'app»: niente a capo, niente
/// caratteri di controllo, non vuoto e non più lungo di [`NOME_MASSIMO`]
/// caratteri (caratteri e non byte: «Più» è tre caratteri).
#[must_use]
pub fn nome_pulito(nome: &str) -> Option<String> {
    let nome = nome.trim();
    let caratteri = nome.chars().count();
    (caratteri > 0 && caratteri <= NOME_MASSIMO && !nome.chars().any(char::is_control))
        .then(|| nome.to_owned())
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::nuvola::Servizio;

    const GUID: &str = "0f2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6f";
    const GOOGLE: &str = "123456789012-abcdefghijklmnopqrstuvwxyz012345.apps.googleusercontent.com";

    #[test]
    fn i_nomi_stabili_non_cambiano() {
        // Stanno nel database e nel `CHECK` della 037: cambiarne uno vorrebbe
        // dire perdere le app registrate.
        let chiavi: Vec<&str> = Piattaforma::tutte().iter().map(|p| p.chiave()).collect();
        assert_eq!(chiavi, ["microsoft", "google", "dropbox"]);
        for piattaforma in Piattaforma::tutte() {
            assert_eq!(
                Piattaforma::da_chiave(piattaforma.chiave()),
                Some(piattaforma)
            );
            assert!(
                !piattaforma.chiave().contains('-'),
                "il trattino separa l'id"
            );
        }
        assert_eq!(Piattaforma::da_chiave("mega"), None);
    }

    #[test]
    fn solo_google_vuole_un_segreto() {
        assert!(Piattaforma::Google.serve_un_segreto());
        assert!(!Piattaforma::Microsoft.serve_un_segreto());
        assert!(!Piattaforma::Dropbox.serve_un_segreto());
    }

    #[test]
    fn ogni_servizio_col_consenso_nel_browser_ha_la_sua_piattaforma() {
        assert_eq!(
            Servizio::OneDrive.piattaforma(),
            Some(Piattaforma::Microsoft)
        );
        assert_eq!(
            Servizio::GoogleDrive.piattaforma(),
            Some(Piattaforma::Google)
        );
        assert_eq!(Servizio::Dropbox.piattaforma(), Some(Piattaforma::Dropbox));
        assert_eq!(Servizio::WebDav.piattaforma(), None);
        assert_eq!(Servizio::Mega.piattaforma(), None);
        assert_eq!(Servizio::MegaLink.piattaforma(), None);
    }

    #[test]
    fn gli_identificativi_buoni_passano() {
        for (piattaforma, id) in [
            (Piattaforma::Microsoft, GUID),
            (
                Piattaforma::Microsoft,
                "0F2C8A3E-5B7D-4C1A-9E8F-1A2B3C4D5E6F",
            ),
            (Piattaforma::Dropbox, "a1b2c3d4e5f6g7h"),
            (Piattaforma::Google, GOOGLE),
        ] {
            assert_eq!(difetto_id(piattaforma, id), None, "{id}");
            // Un a capo incollato con l'ID non è un difetto.
            assert_eq!(difetto_id(piattaforma, &format!(" {id}\n")), None, "{id}");
        }
    }

    #[test]
    fn gli_identificativi_sbagliati_dicono_perche() {
        for (piattaforma, id, atteso) in [
            (Piattaforma::Microsoft, "", DifettoId::Vuoto),
            (Piattaforma::Microsoft, "   ", DifettoId::Vuoto),
            (Piattaforma::Microsoft, "0f2c8a3e 5b7d", DifettoId::Spazi),
            // Il valore del segreto di un client Entra, incollato al posto dell'ID.
            (Piattaforma::Microsoft, "abc8Q~xyz", DifettoId::Forma),
            (
                Piattaforma::Microsoft,
                "0f2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6",
                DifettoId::Forma,
            ),
            (
                Piattaforma::Microsoft,
                "0f2c8a3e5b7d4c1a9e8f1a2b3c4d5e6f",
                DifettoId::Forma,
            ),
            (
                Piattaforma::Microsoft,
                "zf2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6f",
                DifettoId::Forma,
            ),
            (Piattaforma::Dropbox, "A1B2C3D4E5F6G7H", DifettoId::Forma),
            (Piattaforma::Dropbox, "corta", DifettoId::Forma),
            (Piattaforma::Dropbox, GUID, DifettoId::Forma),
            (Piattaforma::Google, "123456789012-abc", DifettoId::Forma),
            (
                Piattaforma::Google,
                ".apps.googleusercontent.com",
                DifettoId::Forma,
            ),
            (Piattaforma::Google, "GOCSPX-segreto", DifettoId::Forma),
            (Piattaforma::Google, GUID, DifettoId::Forma),
        ] {
            assert_eq!(
                difetto_id(piattaforma, id),
                Some(atteso),
                "{piattaforma:?} {id:?}"
            );
        }
        let lungo = format!("{}{SUFFISSO_GOOGLE}", "1".repeat(LUNGHEZZA_MASSIMA));
        assert_eq!(
            difetto_id(Piattaforma::Google, &lungo),
            Some(DifettoId::Lungo)
        );
    }

    #[test]
    fn un_guid_in_maiuscolo_e_lo_stesso_id() {
        assert_eq!(
            forma_canonica(
                Piattaforma::Microsoft,
                " 0F2C8A3E-5B7D-4C1A-9E8F-1A2B3C4D5E6F\n"
            ),
            GUID
        );
        assert_eq!(
            forma_canonica(Piattaforma::Google, &format!(" {GOOGLE} ")),
            GOOGLE
        );
        assert_eq!(
            forma_canonica(Piattaforma::Dropbox, "a1b2c3d4e5f6g7h"),
            "a1b2c3d4e5f6g7h"
        );
    }

    #[test]
    fn il_segreto_vuole_solo_esserci() {
        assert_eq!(difetto_segreto("GOCSPX-AbC_dEf-123"), None);
        assert_eq!(difetto_segreto(" GOCSPX-AbC\n"), None);
        assert_eq!(difetto_segreto(""), Some(DifettoId::Vuoto));
        assert_eq!(difetto_segreto("GOCSPX AbC"), Some(DifettoId::Spazi));
        // Copiato da un JSON, da un messaggio, da un documento.
        for incollato in [
            "\"GOCSPX-AbC\"",
            "'GOCSPX-AbC'",
            "`GOCSPX-AbC`",
            "\u{201c}GOCSPX-AbC\u{201d}",
            "\u{ab}GOCSPX-AbC\u{bb}",
        ] {
            assert_eq!(
                difetto_segreto(incollato),
                Some(DifettoId::Forma),
                "{incollato}"
            );
        }
        assert_eq!(
            difetto_segreto(&"x".repeat(LUNGHEZZA_MASSIMA + 1)),
            Some(DifettoId::Lungo)
        );
    }

    #[test]
    fn il_nome_si_ripulisce_e_ha_un_tetto() {
        assert_eq!(
            nome_pulito("  Google di casa \n").as_deref(),
            Some("Google di casa")
        );
        assert_eq!(nome_pulito(""), None);
        assert_eq!(nome_pulito("   "), None);
        assert_eq!(nome_pulito("due\nrighe"), None);
        let al_tetto = "è".repeat(NOME_MASSIMO);
        assert_eq!(nome_pulito(&al_tetto).as_deref(), Some(al_tetto.as_str()));
        assert_eq!(nome_pulito(&"è".repeat(NOME_MASSIMO + 1)), None);
    }
}
