//! La sessione multimediale: la notifica del lettore, la schermata di blocco,
//! i tasti delle cuffie e Android Auto.
//!
//! Il lato Kotlin è un `MediaLibraryService` di Media3 con un giocatore che non
//! suona niente: **riflette** lo stato che arriva da qui ([`aggiorna`],
//! [`coda`]) e trasforma i tasti in gesti ([`aspetta_gesto`]). La musica la fa
//! sempre `rymembra-play`, in Rust; il servizio esiste perché Android la lasci
//! suonare a schermo spento, e perché il resto del sistema — l'auto compresa —
//! sappia che cosa suona e possa chiedere di suonare altro.
//!
//! # Perché i gesti si aspettano invece di arrivare
//!
//! Perché un metodo Kotlin che chiama Rust vorrebbe una funzione esportata con
//! `#[no_mangle]`, cioè un'altra eccezione a `unsafe`, e una firma da tenere
//! allineata a mano. Un filo Rust che chiede «c'è un gesto?» e aspetta fino a
//! una scadenza fa la stessa cosa con la porta che c'è già: Kotlin mette i
//! tasti in una coda, e [`aspetta_gesto`] li prende in ordine.
//!
//! Anche «suona questo album», scelto sullo schermo dell'auto, è un gesto e
//! passa da qui: sta nella stessa coda di «pausa» e «riprendi», che l'auto
//! manda subito dopo, e solo così arrivano nell'ordine in cui sono partiti.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Errore, chiama};

/// Quel che la notifica deve mostrare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scheda {
    /// La riga del brano, per «mi piace»; `0` se non c'è un brano.
    pub id: i64,
    /// Il titolo del brano; vuoto se non c'è un brano.
    pub titolo: String,
    /// L'artista.
    pub artista: String,
    /// L'album.
    pub album: String,
    /// La copertina, come percorso **dentro** la cartella delle copertine
    /// (`ab/abcdef….jpg`): Kotlin la serve all'auto e alla schermata di blocco
    /// dal suo `ContentProvider`, perché un file privato dell'app un altro
    /// processo non lo può aprire. Vedi [`pronta`].
    pub copertina: Option<String>,
    /// Quanto dura il brano, in millisecondi. `0` se non si sa.
    pub durata_ms: u64,
    /// Dove sta il brano adesso, in millisecondi.
    pub posizione_ms: u64,
    /// La musica sta suonando (e non è in pausa).
    pub suona: bool,
    /// C'è un brano aperto.
    pub brano: bool,
    /// C'è qualcosa in coda dopo il brano.
    pub dopo: bool,
    /// Il brano corrente è fra i preferiti.
    pub piace: bool,
    /// Lo shuffle è acceso.
    pub mescola: bool,
    /// Come si ripete: `off`, `one`, `all`.
    pub ripeti: String,
    /// Dove sta il brano nella coda, nell'ordine in cui suona.
    pub indice_coda: Option<usize>,
}

/// Una voce della coda, per la coda che l'auto mostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceCoda {
    /// Il posto nella coda, nell'ordine in cui suona: è quel che torna
    /// indietro con [`Gesto::Coda`] quando ci si salta.
    pub posto: usize,
    /// L'identificativo che la coda usa.
    pub id: i64,
    /// Il titolo.
    pub titolo: String,
    /// L'artista.
    pub artista: String,
    /// L'album.
    pub album: String,
    /// La copertina, come in [`Scheda::copertina`].
    pub copertina: Option<String>,
    /// Quanto dura, in millisecondi; `0` se non si sa.
    pub durata_ms: u64,
}

/// Un tratto della coda attorno al brano corrente.
///
/// Un tratto e non la coda intera: una coda di millequattrocento brani
/// spedita a ogni cambio non la scorre nessuno, in macchina.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coda {
    /// Le voci, nell'ordine in cui suoneranno. Un brano che nel frattempo è
    /// sparito dalla libreria non c'è, e il suo posto resta vuoto: per questo
    /// ogni voce dice il suo [`VoceCoda::posto`].
    pub voci: Vec<VoceCoda>,
}

/// Rispecchia lo stato del lettore nella sessione.
///
/// Non aspetta niente: Kotlin passa lo stato al filo principale e torna.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn aggiorna(scheda: &Scheda) -> Result<(), Errore> {
    chiama("sessione.aggiorna", scheda)
}

/// Rispecchia un tratto della coda nella sessione: è quel che l'auto mostra
/// sotto «In coda».
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn coda(coda: &Coda) -> Result<(), Errore> {
    chiama("sessione.coda", coda)
}

#[derive(Serialize)]
struct Pronta<'a> {
    copertine: &'a str,
}

/// Dice a Kotlin che il nucleo è partito, e dove stanno le copertine.
///
/// Fino a qui la sessione esiste ma non sa niente: se l'ha svegliata l'auto,
/// con l'app chiusa, le domande sulla libreria aspettano questo segnale.
///
/// # Errori
///
/// Se il ponte non risponde, o se il percorso non è testo.
pub fn pronta(copertine: &Path) -> Result<(), Errore> {
    let copertine = copertine
        .to_str()
        .ok_or_else(|| Errore::nuovo("la cartella delle copertine non è testo"))?;
    chiama("sessione.pronta", &Pronta { copertine })
}

/// Dice a chi sfoglia la libreria — l'auto — che è cambiata: una scansione
/// finita, brani nuovi. Kotlin le fa rileggere le schede che ha aperte.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn libreria_cambiata() -> Result<(), Errore> {
    chiama("sessione.libreria", &())
}

#[derive(Serialize)]
struct Avviso<'a> {
    codice: &'a str,
}

/// Dice a chi guarda la sessione — l'auto, soprattutto — che l'ultimo gesto
/// non ha portato a niente: una ricerca a voce senza risultati, un album che
/// non c'è più. `codice` è quello del catalogo degli errori; la frase la
/// sceglie Kotlin, nella lingua del telefono.
///
/// L'avviso resta finché la musica non riparte o non cambia brano.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn errore(codice: &str) -> Result<(), Errore> {
    chiama("sessione.errore", &Avviso { codice })
}

/// Ferma la sessione e il servizio, e toglie la notifica.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn stacca() -> Result<(), Errore> {
    chiama("sessione.stacca", &())
}

/// Che cosa chiede una ricerca a voce, quando l'assistente l'ha capito.
///
/// «Metti i Queen su Rymembra» arriva con l'artista già separato dal resto;
/// una frase che l'assistente non ha scomposto arriva solo in
/// [`Ricerca::testo`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ricerca {
    /// La frase intera. Vuota per «metti un po' di musica».
    #[serde(default)]
    pub testo: String,
    /// L'artista, se l'assistente l'ha riconosciuto.
    #[serde(default)]
    pub artista: Option<String>,
    /// L'album.
    #[serde(default)]
    pub album: Option<String>,
    /// Il titolo del brano.
    #[serde(default)]
    pub titolo: Option<String>,
}

/// Un tasto premuto fuori dalla finestra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gesto {
    /// Riprendi.
    Riprendi,
    /// Pausa.
    Pausa,
    /// Il brano dopo.
    Prossimo,
    /// Il brano prima, o da capo.
    Precedente,
    /// A questo millisecondo del brano.
    VaiA(u64),
    /// Avanti di tanti millisecondi.
    Avanti(u64),
    /// Indietro di tanti millisecondi.
    Indietro(u64),
    /// Suona una voce dell'albero dell'auto: un brano, un album, una playlist.
    Suona(String),
    /// Suona quel che una ricerca a voce ha chiesto.
    Cerca(Ricerca),
    /// Salta a questa posizione della coda.
    Coda(usize),
    /// Accende o spegne lo shuffle.
    Mescola,
    /// Passa al modo di ripetizione dopo.
    Ripeti,
    /// Mette o toglie il brano corrente dai preferiti.
    Piace,
    /// Un link `rymembra://collega/…` aperto da fuori: il QR letto con la
    /// fotocamera del sistema. Non è un tasto, ma arriva dalla stessa strada:
    /// è l'unica che va da Kotlin a Rust senza che Rust chieda.
    Collega(String),
    /// La rete di sistema è cambiata: per il collegamento via internet.
    CambioDiRete,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Attesa {
    entro_ms: u64,
}

#[derive(Deserialize)]
struct GestoKotlin {
    gesto: String,
    #[serde(default)]
    ms: u64,
    #[serde(default)]
    voce: String,
    #[serde(default)]
    ricerca: Option<Ricerca>,
}

/// Aspetta un tasto, al massimo per `entro`. `Ok(None)` se non ne arriva
/// nessuno.
///
/// Va chiamata da un filo che non ha altro da fare: blocca.
///
/// # Errori
///
/// Se il ponte non risponde, o se arriva un gesto che questa versione non
/// conosce.
pub fn aspetta_gesto(entro: Duration) -> Result<Option<Gesto>, Errore> {
    let entro_ms = u64::try_from(entro.as_millis()).unwrap_or(u64::MAX);
    let arrivato: Option<GestoKotlin> = chiama("sessione.gesto", &Attesa { entro_ms })?;
    arrivato.map(traduci).transpose()
}

/// Il gesto come lo scrive Kotlin, nel gesto di qui.
fn traduci(arrivato: GestoKotlin) -> Result<Gesto, Errore> {
    Ok(match arrivato.gesto.as_str() {
        "riprendi" => Gesto::Riprendi,
        "pausa" => Gesto::Pausa,
        "prossimo" => Gesto::Prossimo,
        "precedente" => Gesto::Precedente,
        "vai" => Gesto::VaiA(arrivato.ms),
        "avanti" => Gesto::Avanti(arrivato.ms),
        "indietro" => Gesto::Indietro(arrivato.ms),
        "suona" => Gesto::Suona(arrivato.voce),
        "cerca" => Gesto::Cerca(arrivato.ricerca.unwrap_or_default()),
        "coda" => Gesto::Coda(usize::try_from(arrivato.ms).unwrap_or(usize::MAX)),
        "mescola" => Gesto::Mescola,
        "ripeti" => Gesto::Ripeti,
        "piace" => Gesto::Piace,
        "collega" => Gesto::Collega(arrivato.voce),
        "rete" => Gesto::CambioDiRete,
        altro => return Err(Errore::nuovo(format!("gesto sconosciuto: {altro}"))),
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    fn da(json: &str) -> Result<Gesto, Errore> {
        traduci(serde_json::from_str(json).map_err(|err| Errore::nuovo(err.to_string()))?)
    }

    #[test]
    fn i_gesti_dell_auto_arrivano_con_quel_che_serve() {
        assert_eq!(
            da(r#"{"gesto":"suona","voce":"album:Queen|II"}"#),
            Ok(Gesto::Suona("album:Queen|II".to_owned()))
        );
        assert_eq!(da(r#"{"gesto":"coda","ms":7}"#), Ok(Gesto::Coda(7)));
        assert_eq!(
            da(r#"{"gesto":"cerca","ricerca":{"testo":"queen","artista":"Queen"}}"#),
            Ok(Gesto::Cerca(Ricerca {
                testo: "queen".to_owned(),
                artista: Some("Queen".to_owned()),
                album: None,
                titolo: None,
            }))
        );
        // «Metti un po' di musica»: una ricerca vuota, non un errore.
        assert_eq!(
            da(r#"{"gesto":"cerca"}"#),
            Ok(Gesto::Cerca(Ricerca::default()))
        );
    }

    #[test]
    fn un_gesto_sconosciuto_e_un_errore_e_non_un_silenzio() {
        assert!(da(r#"{"gesto":"vola"}"#).is_err());
    }
}
