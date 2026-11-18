//! La riga di comando di Rymembra: due cose sole, e tutto il resto si ignora.
//!
//! # Cosa legge
//!
//! - **`--avvio-automatico`**: lo passa Windows al login (la voce `Run` della
//!   copia NSIS, vedi `rymembra_barra::comando_di_avvio`). Con lui la finestra si
//!   mostra **ridotta a icona nella barra**, e un secondo avvio con lui non alza
//!   la finestra di quello che c'è già.
//! - **`rymembra://collega/…`**: il link di collegamento di un altro dispositivo,
//!   cliccato in una chat o in una pagina. Windows lo manda a Rymembra perché
//!   l'installer (e il manifesto MSIX) registrano il protocollo.
//!
//! # Il link è un input che non è nostro
//!
//! Chiunque può scrivere `rymembra://collega/…` in una pagina web, e Windows lo
//! passa al programma come argomento. Per questo:
//!
//! - **un solo argomento vale come link**, il primo, e solo se comincia col
//!   prefisso giusto (le maiuscole del protocollo e dell'host non contano: i
//!   browser le normalizzano, e il resto — che è base64url, dove le maiuscole
//!   contano — si lascia com'è) ed è abbastanza corto per essere un link vero;
//! - quel che non somiglia a un link non arriva a nessuno: è ignorato;
//! - un link che somiglia, invece, **non si fida lo stesso**: passa da
//!   [`crate::lontano::link_arrivato`], che lo legge davvero
//!   (`Invito::da_link`: versione, lunghezze, byte in più), e poi dalla stessa
//!   conferma di sempre — il nome che dichiara e l'impronta della chiave, davanti
//!   a chi decide — prima di collegare qualcosa. Nessun link si apre da solo;
//! - gli altri argomenti non fanno niente, nemmeno quelli che un indirizzo
//!   costruito ad arte può infilare dopo le virgolette di `"%1"`: l'unico che
//!   conta è la parola esatta `--avvio-automatico`, e tutto quel che fa è non
//!   alzare una finestra.
//!
//! La regola è pura, e per questo si prova: niente processo, niente finestra.

use rymembra_barra::ARGOMENTO_AVVIO_AUTOMATICO;
use rymembra_lontano::collegamento::PREFISSO;

/// Quanto può essere lungo un link perché sia un link vero.
///
/// Il più lungo che `Invito::link` scrive — quattro indirizzi IPv6, il relay più
/// lungo, il nome più lungo — sta in circa 540 caratteri: il doppio è largo, e
/// molto sotto i 32 767 che Windows concede a una riga di comando. Quel che
/// supera è un argomento costruito per altro.
const MASSIMO_LINK: usize = 1024;

/// Quel che la riga di comando chiede.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Argomenti {
    /// Windows l'ha lanciato al login (`--avvio-automatico`).
    pub automatico: bool,
    /// Il link di collegamento aperto da fuori, nella forma di
    /// `rymembra://collega/…` e non ancora letto.
    pub link: Option<String>,
}

impl Argomenti {
    /// Legge gli argomenti **senza il primo**, che è il nome dell'eseguibile.
    pub fn da<I, S>(argomenti: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut letti = Self::default();
        for argomento in argomenti {
            let argomento = argomento.as_ref();
            if argomento == ARGOMENTO_AVVIO_AUTOMATICO {
                letti.automatico = true;
            } else if letti.link.is_none() {
                letti.link = link_di(argomento);
            }
        }
        letti
    }

    /// Quelli del processo, adesso. Un argomento che non è UTF-8 non può essere
    /// né l'uno né l'altro, e si legge storpiato.
    #[must_use]
    pub fn del_processo() -> Self {
        Self::da(
            std::env::args_os()
                .skip(1)
                .map(|argomento| argomento.to_string_lossy().into_owned()),
        )
    }
}

/// L'argomento, se è un link di collegamento, nella forma canonica.
///
/// Il prefisso si confronta senza badare alle maiuscole e si rimpiazza con
/// quello vero: a `da_link` il corpo arriva intatto.
fn link_di(argomento: &str) -> Option<String> {
    if argomento.len() > MASSIMO_LINK {
        return None;
    }
    let argomento = argomento.trim();
    let testa = argomento.get(..PREFISSO.len())?;
    if !testa.eq_ignore_ascii_case(PREFISSO) {
        return None;
    }
    let corpo = argomento.get(PREFISSO.len()..)?;
    // Il corpo è base64url, con al più la barra che certi programmi aggiungono
    // in coda a un indirizzo: tutto il resto — spazi, virgolette, un'altra
    // opzione — è un argomento che non è un link.
    let valido = !corpo.is_empty()
        && corpo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '='));
    valido.then(|| format!("{PREFISSO}{corpo}"))
}

/// Che cosa fa il programma già aperto quando ne parte un secondo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondoAvvio {
    /// Niente: è l'avvio automatico di Windows su un programma che c'è già, e
    /// non deve alzare una finestra che qualcuno ha ridotto o chiuso nel
    /// vassoio.
    Ignora,
    /// Porta davanti la finestra, ricreandola se il vassoio l'aveva chiusa: è
    /// quello che chi riapre Rymembra vuole.
    Mostra,
    /// Consegna il link, e porta davanti la finestra: la conferma sta lì.
    ConLink(String),
}

/// La decisione sul secondo avvio. Un link vale più di tutto, perché chi l'ha
/// cliccato aspetta una risposta; l'avvio automatico vale più di un avvio
/// qualunque, perché non l'ha chiesto nessuno.
#[must_use]
pub fn secondo_avvio(argomenti: &Argomenti) -> SecondoAvvio {
    match (&argomenti.link, argomenti.automatico) {
        (Some(link), _) => SecondoAvvio::ConLink(link.clone()),
        (None, true) => SecondoAvvio::Ignora,
        (None, false) => SecondoAvvio::Mostra,
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un link vero, com'è fatto da `Invito::link`.
    fn un_link() -> String {
        let invito = rymembra_lontano::Invito {
            chiave: [7; 32],
            gettone: [9; 16],
            indirizzi: vec!["192.168.1.20:4433".parse().expect("un indirizzo")],
            relay: Some("https://relay.esempio.it".to_owned()),
            nome: Some("Il mio PC".to_owned()),
        };
        invito.link()
    }

    #[test]
    fn senza_argomenti_non_chiede_niente() {
        assert_eq!(Argomenti::da(Vec::<String>::new()), Argomenti::default());
    }

    #[test]
    fn l_avvio_automatico_e_la_parola_esatta() {
        assert!(Argomenti::da(["--avvio-automatico"]).automatico);
        for non_e in [
            "--avvio-automatico=1",
            "--Avvio-Automatico",
            "-avvio-automatico",
            "avvio-automatico",
            "--avvio-automatico --altro",
            "",
        ] {
            assert!(!Argomenti::da([non_e]).automatico, "{non_e:?}");
        }
    }

    #[test]
    fn un_link_di_collegamento_si_riconosce() {
        let link = un_link();
        let letti = Argomenti::da([link.as_str()]);
        assert_eq!(letti.link.as_deref(), Some(link.as_str()));
        assert!(!letti.automatico);
        // E quel che si legge è un link che il nucleo sa leggere.
        assert!(rymembra_lontano::Invito::da_link(letti.link.as_deref().unwrap_or("")).is_ok());
    }

    #[test]
    fn le_maiuscole_del_protocollo_non_cambiano_il_corpo() {
        let link = un_link();
        let corpo = link.strip_prefix(PREFISSO).expect("il prefisso").to_owned();
        let scritto = format!("RYMEMBRA://Collega/{corpo}");
        let letti = Argomenti::da([scritto.as_str()]);
        // Il prefisso torna quello vero, il corpo — dove le maiuscole contano —
        // resta com'era.
        assert_eq!(letti.link, Some(link));
    }

    #[test]
    fn la_barra_in_coda_che_aggiunge_un_programma_si_perdona() {
        let link = format!("{}/", un_link());
        let letti = Argomenti::da([link.as_str()]);
        assert_eq!(letti.link.as_deref(), Some(link.as_str()));
        assert!(
            rymembra_lontano::Invito::da_link(letti.link.as_deref().unwrap_or("")).is_ok(),
            "il nucleo toglie la barra da sé"
        );
    }

    #[test]
    fn quel_che_non_somiglia_a_un_link_non_arriva_a_nessuno() {
        let corpo = un_link()
            .strip_prefix(PREFISSO)
            .expect("il prefisso")
            .to_owned();
        for no in [
            // Un altro protocollo, un'altra azione, nessuna azione.
            format!("https://collega/{corpo}"),
            format!("rymembra://altro/{corpo}"),
            format!("rymembra://collega{corpo}"),
            PREFISSO.to_owned(),
            // Una virgoletta, uno spazio, un'opzione dentro il corpo: è quel
            // che un indirizzo costruito ad arte infila dopo `"%1"`.
            format!("{PREFISSO}{corpo}\" --gpu-launcher=calc.exe"),
            format!("{PREFISSO}{corpo} --altro"),
            format!("{PREFISSO}{corpo}\u{0}"),
            // Troppo lungo per essere un link.
            format!("{PREFISSO}{}", "A".repeat(MASSIMO_LINK)),
            // Un percorso.
            r"C:\Musica\brano.flac".to_owned(),
        ] {
            assert_eq!(Argomenti::da([no.as_str()]).link, None, "{no:?}");
        }
    }

    #[test]
    fn il_primo_link_vale_e_gli_altri_no() {
        let primo = un_link();
        let secondo = format!("{PREFISSO}QUFB");
        let letti = Argomenti::da([primo.as_str(), secondo.as_str()]);
        assert_eq!(letti.link.as_deref(), Some(primo.as_str()));
    }

    #[test]
    fn l_avvio_automatico_e_un_link_si_leggono_insieme() {
        let link = un_link();
        let letti = Argomenti::da(["--avvio-automatico", link.as_str()]);
        assert!(letti.automatico);
        assert_eq!(letti.link.as_deref(), Some(link.as_str()));
    }

    #[test]
    fn il_secondo_avvio_automatico_non_alza_la_finestra() {
        let automatico = Argomenti::da(["--avvio-automatico"]);
        assert_eq!(secondo_avvio(&automatico), SecondoAvvio::Ignora);
    }

    #[test]
    fn un_secondo_avvio_qualunque_alza_la_finestra() {
        assert_eq!(secondo_avvio(&Argomenti::default()), SecondoAvvio::Mostra);
        // Anche con argomenti che non sono i nostri.
        let altri = Argomenti::da(["--boh", r"C:\Musica"]);
        assert_eq!(secondo_avvio(&altri), SecondoAvvio::Mostra);
    }

    #[test]
    fn un_link_vale_piu_dell_avvio_automatico() {
        let link = un_link();
        let tutti_e_due = Argomenti::da(["--avvio-automatico", link.as_str()]);
        assert_eq!(secondo_avvio(&tutti_e_due), SecondoAvvio::ConLink(link));
    }

    #[test]
    fn l_argomento_e_quello_che_scrive_il_registro() {
        // Il comando della voce `Run` e questo file devono dire la stessa parola.
        let comando = rymembra_barra::comando_di_avvio(std::path::Path::new(r"C:\r\rymembra.exe"))
            .expect("un comando");
        let argomenti: Vec<&str> = comando.split_whitespace().skip(1).collect();
        assert!(Argomenti::da(argomenti).automatico, "{comando}");
    }
}
