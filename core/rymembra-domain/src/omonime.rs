//! Due playlist con lo stesso nome che non sono la stessa playlist (D-11).
//!
//! # La regola, una per tutte le importazioni
//!
//! Il nome normalizzato è l'identità di una playlist ([`crate::keys::PlaylistKey`]):
//! due dispositivi che si sincronizzano, un profilo, un backup la ritrovano
//! per nome. Ma due playlist possono chiamarsi uguali ed essere due cose
//! diverse — «Chill» messa insieme a mano qui, e un'altra «Chill» fatta
//! altrove, o arrivata da Spotify. Sostituire una con l'altra, o fonderle,
//! perde il lavoro di chi l'ha fatta qui. Allora l'importazione non la tocca:
//! la sua le sta **accanto**, «Chill (Spotify)», «Chill (Drive)», e il piano lo
//! dice.
//!
//! Questo modulo è la parte che non guarda un database: il nome accanto e la
//! scelta del primo posto libero. La usano `rymembra_app::playlists::destinazione`
//! (che chiede al database chi porta un nome) e il piano del ripristino
//! ([`crate::restore::plan_restore`], che lo chiede alle playlist di qui che ha
//! già in mano): due domande diverse, una risposta sola.

/// Quanto può essere lungo il nome di una playlist, in caratteri.
///
/// Lo stesso tetto di `rymembra_app::playlists::NOME_MASSIMO`, che lo prende da
/// qui: un nome accanto non deve superarlo.
pub const NOME_MASSIMO: usize = 200;

/// Quanti nomi «(Fonte n)» si provano prima di arrendersi.
///
/// Arrivarci vuol dire cento playlist con lo stesso nome e la stessa fonte: un
/// rifiuto è meglio di un ciclo che non finisce.
pub const ACCANTO_MASSIMI: usize = 100;

/// Due playlist con lo stesso nome sono **la stessa**?
///
/// Sì se sono nate nello stesso millisecondo: la data di creazione viaggia
/// con la playlist nella sincronia, nel profilo e nel backup, e ogni fusione
/// tiene la più antica, quindi due copie della stessa playlist ce l'hanno
/// uguale. Una playlist fatta a mano qui e una fatta altrove con lo stesso nome
/// no. Uno zero vuol dire «non si sa»: allora valgono le regole di sempre fra
/// due copie della stessa playlist, che è la direzione in cui niente si
/// raddoppia.
#[must_use]
pub const fn stessa_nascita(una: i64, altra: i64) -> bool {
    una == 0 || altra == 0 || una == altra
}

/// Il nome di una playlist messa accanto a un'altra: «Chill (Spotify)»,
/// «Chill (Spotify 2)»…, troncato davanti perché la fonte resti leggibile e il
/// tutto stia in [`NOME_MASSIMO`].
#[must_use]
pub fn nome_accanto(base: &str, etichetta: &str, numero: usize) -> String {
    let coda = if numero <= 1 {
        format!(" ({etichetta})")
    } else {
        format!(" ({etichetta} {numero})")
    };
    let spazio = NOME_MASSIMO.saturating_sub(coda.chars().count());
    let testa = if base.chars().count() <= spazio {
        base.to_owned()
    } else {
        let corta: String = base.chars().take(spazio.saturating_sub(1)).collect();
        format!("{}…", corta.trim_end())
    };
    format!("{testa}{coda}")
}

/// Chi porta già un nome candidato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Occupato<I> {
    /// Nessuno: il nome è libero.
    Libero,
    /// La playlist che questa stessa importazione aveva già scritto: è la sua.
    Suo(I),
    /// Un'altra playlist, che non si tocca. Porta il suo nome com'è scritto.
    Altrui(String),
}

/// Dove scrive un'importazione, deciso da [`trova_posto`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Posto<I> {
    /// Una playlist sua: quella da riscrivere, o da allungare.
    Suo(I),
    /// Nessuna: va creata con questo nome.
    Nuovo {
        /// Il nome libero: quello di là, o quello con la fonte fra parentesi.
        nome: String,
        /// Il nome della playlist che portava il nome di là, lasciata com'era,
        /// quando la nuova le sta accanto. È quel che il piano deve dire.
        accanto_a: Option<String>,
    },
}

/// Il primo posto per `titolo`, fra `titolo`, «titolo (etichetta)»,
/// «titolo (etichetta 2)»…
///
/// `occupato` risponde per un nome candidato. Il primo libero è quello della
/// playlist nuova; uno occupato dalla **sua** playlist è quello da riscrivere —
/// è così che si ritrova la copia messa accanto la volta prima; uno occupato da
/// qualunque altra si lascia stare e si passa al nome dopo. Mai sostituire una
/// playlist fatta qui.
///
/// `None` oltre [`ACCANTO_MASSIMI`].
///
/// # Errori
///
/// Quelli di `occupato`, così come sono.
pub fn trova_posto<I, E>(
    titolo: &str,
    etichetta: &str,
    mut occupato: impl FnMut(&str) -> Result<Occupato<I>, E>,
) -> Result<Option<Posto<I>>, E> {
    let titolo = titolo.trim();
    let mut accanto_a: Option<String> = None;
    for numero in 0..=ACCANTO_MASSIMI {
        let nome = if numero == 0 {
            titolo.to_owned()
        } else {
            nome_accanto(titolo, etichetta, numero)
        };
        match occupato(&nome)? {
            Occupato::Libero => return Ok(Some(Posto::Nuovo { nome, accanto_a })),
            Occupato::Suo(id) => return Ok(Some(Posto::Suo(id))),
            Occupato::Altrui(chi) => {
                accanto_a.get_or_insert(chi);
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_nome_accanto_tiene_la_fonte_anche_oltre_il_tetto() {
        let lungo = "x".repeat(NOME_MASSIMO);
        let nome = nome_accanto(&lungo, "Spotify", 1);
        assert!(
            nome.chars().count() <= NOME_MASSIMO,
            "{}",
            nome.chars().count()
        );
        assert!(nome.ends_with("… (Spotify)"), "{nome}");
        assert_eq!(nome_accanto("Chill", "Spotify", 3), "Chill (Spotify 3)");
    }

    #[test]
    fn il_primo_posto_libero_dopo_quelli_degli_altri() {
        let occupati = ["Chill", "Chill (Drive)"];
        let posto = trova_posto::<(), ()>("Chill", "Drive", |nome| {
            Ok(if occupati.contains(&nome) {
                Occupato::Altrui(nome.to_owned())
            } else {
                Occupato::Libero
            })
        });
        assert_eq!(
            posto,
            Ok(Some(Posto::Nuovo {
                nome: "Chill (Drive 2)".to_owned(),
                accanto_a: Some("Chill".to_owned()),
            }))
        );
        // La sua copia, accanto: si ritrova.
        let posto = trova_posto::<u8, ()>("Chill", "Drive", |nome| {
            Ok(match nome {
                "Chill" => Occupato::Altrui(nome.to_owned()),
                "Chill (Drive)" => Occupato::Suo(7),
                _ => Occupato::Libero,
            })
        });
        assert_eq!(posto, Ok(Some(Posto::Suo(7))));
    }

    #[test]
    fn la_stessa_nascita_e_zero_vuol_dire_non_si_sa() {
        assert!(stessa_nascita(10, 10));
        assert!(stessa_nascita(0, 10));
        assert!(stessa_nascita(10, 0));
        assert!(!stessa_nascita(10, 11));
    }
}
