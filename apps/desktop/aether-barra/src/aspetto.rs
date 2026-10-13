//! Da com'è il lettore a come sono i tre pulsanti.
//!
//! Tutta la parte che si può decidere senza Windows sta qui, e si prova qui:
//! quale simbolo, quale suggerimento, acceso o spento, e come si taglia un
//! suggerimento troppo lungo senza spezzare una lettera.

use crate::Tasto;
use crate::glifi::Glifo;

/// I suggerimenti dei pulsanti, nella lingua della finestra.
///
/// Sono le stesse quattro etichette delle voci della musica nel menù del
/// vassoio: chi aggancia le riceve dalla finestra, dove stanno tutti i testi
/// tradotti.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Etichette {
    /// «Precedente».
    pub precedente: String,
    /// «Riproduci», sul pulsante quando la musica è ferma.
    pub riproduci: String,
    /// «Pausa», sul pulsante quando la musica suona.
    pub pausa: String,
    /// «Successivo».
    pub successivo: String,
}

/// Come devono apparire i pulsanti.
///
/// Il valore di partenza ha i pulsanti spenti e senza suggerimento: è quel che
/// si vede nell'istante fra la comparsa della finestra e il primo stato del
/// lettore.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Aspetto {
    /// La musica sta andando: il pulsante di mezzo mostra la pausa.
    pub suona: bool,
    /// C'è qualcosa da comandare. Spenti, i tre pulsanti restano al loro posto
    /// ma grigi: Windows non permette di toglierli una volta messi, e un
    /// pulsante che sparisce e ricompare sposta gli altri sotto il dito.
    pub attivi: bool,
    /// I suggerimenti.
    pub etichette: Etichette,
}

/// L'identificativo del pulsante «precedente» nei messaggi di Windows.
///
/// Arriva nella parola bassa di `WM_COMMAND`, accanto a `THBN_CLICKED` nella
/// parola alta: è quella seconda parola a distinguere un pulsante
/// dell'anteprima da una voce di menù, quindi questi numeri non devono evitare
/// nient'altro. Tre numeri qualunque, purché diversi.
pub(crate) const ID_PRECEDENTE: u32 = 0xAE01;

/// L'identificativo del pulsante di mezzo.
pub(crate) const ID_ALTERNA: u32 = 0xAE02;

/// L'identificativo del pulsante «successivo».
pub(crate) const ID_SUCCESSIVO: u32 = 0xAE03;

/// Quante unità UTF-16 ha il suggerimento di un pulsante, terminatore escluso.
///
/// Il campo di Windows ne tiene 260, l'ultima per lo zero finale.
const SUGGERIMENTO_MASSIMO: usize = 259;

/// Un pulsante, pronto da consegnare a Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pulsante {
    /// L'identificativo che tornerà nel clic.
    pub id: u32,
    /// Il simbolo da disegnare.
    pub glifo: Glifo,
    /// Si può premere.
    pub attivo: bool,
    /// Il suggerimento, in UTF-16, già tagliato e chiuso dallo zero.
    pub suggerimento: [u16; 260],
}

/// I tre pulsanti, da sinistra a destra.
///
/// L'ordine è quello di qualunque lettore — indietro, play, avanti — e una
/// volta consegnato a Windows non si può più cambiare.
pub(crate) fn pulsanti(aspetto: &Aspetto) -> [Pulsante; 3] {
    let etichette = &aspetto.etichette;
    let (glifo_di_mezzo, testo_di_mezzo) = if aspetto.suona {
        (Glifo::Pausa, &etichette.pausa)
    } else {
        (Glifo::Riproduci, &etichette.riproduci)
    };
    [
        Pulsante {
            id: ID_PRECEDENTE,
            glifo: Glifo::Precedente,
            attivo: aspetto.attivi,
            suggerimento: suggerimento(&etichette.precedente),
        },
        Pulsante {
            id: ID_ALTERNA,
            glifo: glifo_di_mezzo,
            attivo: aspetto.attivi,
            suggerimento: suggerimento(testo_di_mezzo),
        },
        Pulsante {
            id: ID_SUCCESSIVO,
            glifo: Glifo::Successivo,
            attivo: aspetto.attivi,
            suggerimento: suggerimento(&etichette.successivo),
        },
    ]
}

/// Il tasto che corrisponde a un identificativo, se è uno dei nostri.
pub(crate) const fn tasto_di(id: u32) -> Option<Tasto> {
    match id {
        ID_PRECEDENTE => Some(Tasto::Precedente),
        ID_ALTERNA => Some(Tasto::Alterna),
        ID_SUCCESSIVO => Some(Tasto::Successivo),
        _ => None,
    }
}

/// Il testo in UTF-16, tagliato perché ci stia e chiuso dallo zero.
///
/// Il taglio cade sempre fra due lettere: una lettera che in UTF-16 occupa due
/// unità — un'emoji, un ideogramma raro — o entra intera o resta fuori. Tagliata
/// a metà diventerebbe un carattere che non esiste, e Windows lo disegnerebbe
/// come un quadratino.
fn suggerimento(testo: &str) -> [u16; 260] {
    let mut campo = [0u16; 260];
    let mut scritte = 0;
    let mut unita = [0u16; 2];
    for lettera in testo.chars() {
        let codificata = lettera.encode_utf16(&mut unita);
        if scritte + codificata.len() > SUGGERIMENTO_MASSIMO {
            break;
        }
        for &u in codificata.iter() {
            if let Some(posto) = campo.get_mut(scritte) {
                *posto = u;
            }
            scritte += 1;
        }
    }
    campo
}

#[cfg(test)]
mod prove {
    use super::*;

    fn etichette() -> Etichette {
        Etichette {
            precedente: "Precedente".to_owned(),
            riproduci: "Riproduci".to_owned(),
            pausa: "Pausa".to_owned(),
            successivo: "Successivo".to_owned(),
        }
    }

    /// Il suggerimento come stringa, fino allo zero.
    fn testo(campo: &[u16; 260]) -> String {
        let fine = campo.iter().position(|&u| u == 0).unwrap_or(campo.len());
        String::from_utf16_lossy(campo.get(..fine).unwrap_or_default())
    }

    #[test]
    fn a_musica_ferma_il_pulsante_di_mezzo_dice_riproduci() {
        let [_, mezzo, _] = pulsanti(&Aspetto {
            suona: false,
            attivi: true,
            etichette: etichette(),
        });
        assert_eq!(mezzo.glifo, Glifo::Riproduci);
        assert_eq!(testo(&mezzo.suggerimento), "Riproduci");
    }

    #[test]
    fn mentre_suona_il_pulsante_di_mezzo_dice_pausa() {
        let [_, mezzo, _] = pulsanti(&Aspetto {
            suona: true,
            attivi: true,
            etichette: etichette(),
        });
        assert_eq!(mezzo.glifo, Glifo::Pausa);
        assert_eq!(testo(&mezzo.suggerimento), "Pausa");
    }

    #[test]
    fn l_ordine_e_indietro_play_avanti() {
        let [primo, secondo, terzo] = pulsanti(&Aspetto {
            suona: false,
            attivi: true,
            etichette: etichette(),
        });
        assert_eq!(
            [primo.id, secondo.id, terzo.id],
            [ID_PRECEDENTE, ID_ALTERNA, ID_SUCCESSIVO]
        );
        assert_eq!(primo.glifo, Glifo::Precedente);
        assert_eq!(terzo.glifo, Glifo::Successivo);
        assert_eq!(testo(&primo.suggerimento), "Precedente");
        assert_eq!(testo(&terzo.suggerimento), "Successivo");
    }

    #[test]
    fn senza_niente_da_comandare_i_tre_pulsanti_sono_spenti() {
        let tutti = pulsanti(&Aspetto {
            suona: false,
            attivi: false,
            etichette: etichette(),
        });
        assert!(tutti.iter().all(|p| !p.attivo));
        // L'aspetto di partenza è proprio questo, e senza testi.
        let partenza = pulsanti(&Aspetto::default());
        assert!(
            partenza
                .iter()
                .all(|p| !p.attivo && testo(&p.suggerimento).is_empty())
        );
    }

    #[test]
    fn ogni_identificativo_torna_al_suo_tasto() {
        assert_eq!(tasto_di(ID_PRECEDENTE), Some(Tasto::Precedente));
        assert_eq!(tasto_di(ID_ALTERNA), Some(Tasto::Alterna));
        assert_eq!(tasto_di(ID_SUCCESSIVO), Some(Tasto::Successivo));
        assert_eq!(tasto_di(0), None);
        assert_eq!(tasto_di(ID_SUCCESSIVO + 1), None);
    }

    #[test]
    fn un_suggerimento_lungo_si_ferma_prima_dello_zero_finale() {
        let campo = suggerimento(&"a".repeat(400));
        assert_eq!(testo(&campo).len(), SUGGERIMENTO_MASSIMO);
        assert_eq!(campo.last(), Some(&0));
    }

    #[test]
    fn il_taglio_non_spezza_una_lettera_di_due_unita() {
        // 258 unità di «a», poi un'emoji da due: entrerebbe solo a metà.
        let testo_lungo = format!("{}🎵", "a".repeat(SUGGERIMENTO_MASSIMO - 1));
        let campo = suggerimento(&testo_lungo);
        let scritto = testo(&campo);
        assert_eq!(scritto, "a".repeat(SUGGERIMENTO_MASSIMO - 1));
        assert!(!scritto.contains('\u{FFFD}'), "mezza lettera");
        // Con un posto in più l'emoji entra intera.
        let campo = suggerimento(&format!("{}🎵", "a".repeat(SUGGERIMENTO_MASSIMO - 2)));
        assert!(testo(&campo).ends_with('🎵'));
    }

    #[test]
    fn le_lettere_accentate_passano_intere() {
        let campo = suggerimento("Più avanti — è così");
        assert_eq!(testo(&campo), "Più avanti — è così");
    }
}
