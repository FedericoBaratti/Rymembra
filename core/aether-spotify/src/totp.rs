//! Il codice a sei cifre con cui si chiede a Spotify un gettone anonimo.
//!
//! Il lettore web di Spotify firma la richiesta del proprio gettone con un TOTP
//! (RFC 6238: HMAC-SHA1, periodo di trenta secondi, sei cifre) la cui chiave
//! deriva da una sequenza di interi — il «cifrario» — incastonata nel codice
//! JavaScript del lettore e numerata per versione. Senza quel codice il punto di
//! scambio risponde «Invalid TOTP» e non dà niente.
//!
//! # Perché HMAC-SHA1 e non SHA-256
//!
//! Perché è quel che fa Spotify. SHA-1 non è più adatto a firmare niente di
//! serio, ma qui non si sta firmando: si sta riproducendo un calcolo altrui per
//! ottenere la stessa risposta, e cambiarlo vorrebbe dire ottenerne un'altra.
//! È anche la ragione per cui questo crate tira dentro `sha1` mentre tutto il
//! resto del workspace usa `sha2`.
//!
//! # La trasformazione, e il tranello che contiene
//!
//! La chiave HMAC **non** sono i byte trasformati. Sono i byte UTF-8 della
//! *stringa decimale* ottenuta concatenando i numeri trasformati: il cifrario
//! `[44, 55, …]` diventa `[37, 62, …]` che diventa la stringa `"3762…"` che
//! diventa i byte ASCII `[0x33, 0x37, 0x36, 0x32, …]`. Scriverlo con i byte
//! grezzi produce un codice a sei cifre perfettamente formato e sempre
//! sbagliato, che è il modo peggiore in cui un errore può presentarsi.
//!
//! Il giro per esadecimale e base32 che si vede negli strumenti costruiti su
//! `pyotp` serve solo a dare a quella libreria un segreto in base32, e approda
//! esattamente alla stessa chiave.
//!
//! # Volatile per costruzione
//!
//! Spotify ruota il cifrario. Quando succede, questo modulo non si accorge di
//! niente — produce un codice che il server rifiuta — e chi chiama scende al
//! livello successivo. I cifrari non stanno scritti qui ma in [`crate::config`],
//! dove si possono aggiornare senza ricompilare.

use hmac::{Hmac, Mac};
use sha1::Sha1;

/// Un cifrario: la sequenza di interi da cui deriva la chiave, e il numero di
/// versione che va dichiarato al server insieme al codice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cifrario {
    /// Va nel parametro `totpVer`. Deve combaciare con la versione che il
    /// server si aspetta, altrimenti il codice è giusto per la chiave sbagliata.
    pub versione: u32,
    /// Gli interi da trasformare.
    pub cifre: Vec<u8>,
}

/// Il periodo del TOTP, in secondi.
const PERIODO_S: u64 = 30;

/// La chiave HMAC: i byte UTF-8 della stringa decimale dei numeri trasformati.
///
/// Vedi la nota in testa al modulo sul perché non sono i byte grezzi.
#[must_use]
pub fn deriva_chiave(cifre: &[u8]) -> Vec<u8> {
    let mut decimale = String::with_capacity(cifre.len().saturating_mul(2));
    for (posizione, numero) in cifre.iter().enumerate() {
        // `(i % 33) + 9` sta sempre fra 9 e 41, quindi ci sta in un u8 e la
        // conversione non può troncare.
        let maschera = u8::try_from(posizione % 33).unwrap_or(0).saturating_add(9);
        let trasformato = numero ^ maschera;
        decimale.push_str(&trasformato.to_string());
    }
    decimale.into_bytes()
}

/// HOTP (RFC 4226) su un contatore a otto byte big-endian.
///
/// Restituisce `None` solo se la costruzione dell'HMAC fallisce, cosa che per
/// una chiave di lunghezza qualsiasi non succede. È un `Option` e non un panico
/// perché qui l'unico modo giusto di reagire a un imprevisto è provare il
/// cifrario successivo, che è esattamente quel che fa chi chiama.
fn hotp(chiave: &[u8], contatore: u64) -> Option<String> {
    let mut mac = Hmac::<Sha1>::new_from_slice(chiave).ok()?;
    mac.update(&contatore.to_be_bytes());
    let digesto = mac.finalize().into_bytes();

    // Il troncamento dinamico: gli ultimi quattro bit dell'ultimo byte dicono da
    // dove leggere i quattro byte che diventano il numero.
    let scostamento = usize::from(digesto.last().copied().unwrap_or(0) & 0x0f);
    let byte = |quale: usize| -> u32 {
        u32::from(
            digesto
                .get(scostamento.saturating_add(quale))
                .copied()
                .unwrap_or(0),
        )
    };
    let numero = ((byte(0) & 0x7f) << 24) | (byte(1) << 16) | (byte(2) << 8) | byte(3);
    Some(format!("{:06}", numero % 1_000_000))
}

/// Il codice a sei cifre valido nell'istante indicato.
///
/// `tempo_ms` è l'orologio **del server**, non il nostro: vedi
/// [`crate::sessione`] sul perché la differenza conta.
#[must_use]
#[expect(
    clippy::integer_division,
    reason = "il contatore TOTP è per definizione il numero di periodi interi \
              trascorsi: il resto è la posizione dentro il periodo corrente, che \
              la RFC 6238 scarta"
)]
pub fn genera(cifrario: &Cifrario, tempo_ms: u64) -> Option<String> {
    let contatore = tempo_ms / 1000 / PERIODO_S;
    hotp(&deriva_chiave(&cifrario.cifre), contatore)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// I vettori canonici della RFC 4226, appendice D: chiave `"12345678901234567890"`,
    /// contatori da 0 a 9. Provano il troncamento dinamico, che è l'unico pezzo
    /// di questo modulo dove si può sbagliare in silenzio.
    #[test]
    fn i_vettori_della_rfc_4226_combaciano() {
        let chiave = b"12345678901234567890";
        let attesi = [
            "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583",
            "399871", "520489",
        ];
        for (contatore, atteso) in attesi.iter().enumerate() {
            let contatore = u64::try_from(contatore).unwrap_or(0);
            assert_eq!(hotp(chiave, contatore).as_deref(), Some(*atteso));
        }
    }

    #[test]
    fn la_chiave_e_la_stringa_decimale_non_i_byte() {
        // Il tranello descritto in testa al modulo, fissato in un test perché
        // non possa tornare: con `[12, 34]` e le maschere 9 e 10 i numeri
        // trasformati sono 5 e 40, e la chiave è la stringa "540".
        assert_eq!(deriva_chiave(&[12, 34]), b"540".to_vec());
        assert_ne!(
            deriva_chiave(&[12, 34]),
            vec![5_u8, 40],
            "i byte grezzi darebbero un codice ben formato e sempre rifiutato"
        );
    }

    #[test]
    fn la_maschera_riparte_dopo_trentatre_posizioni() {
        // La prima posizione e la trentaquattresima hanno la stessa maschera.
        let mut cifre = vec![0_u8; 34];
        if let Some(primo) = cifre.first_mut() {
            *primo = 100;
        }
        if let Some(trentaquattresimo) = cifre.get_mut(33) {
            *trentaquattresimo = 100;
        }
        let chiave = String::from_utf8(deriva_chiave(&cifre)).unwrap_or_default();
        let trasformato = (100_u8 ^ 9).to_string();
        assert!(chiave.starts_with(&trasformato));
        assert!(chiave.ends_with(&trasformato));
    }

    #[test]
    fn il_codice_cambia_ogni_trenta_secondi_e_non_prima() {
        let cifrario = Cifrario {
            versione: 61,
            cifre: vec![44, 55, 47, 42, 70, 40],
        };
        let base = 1_800_000_000_000_u64;
        let dentro_la_finestra = genera(&cifrario, base.saturating_add(29_000));
        assert_eq!(genera(&cifrario, base), dentro_la_finestra);
        assert_ne!(genera(&cifrario, base), genera(&cifrario, base + 30_000));
    }

    #[test]
    fn il_codice_ha_sempre_sei_cifre() {
        // Gli zeri iniziali si perdono facilmente passando da un numero: un
        // codice di cinque cifre viene rifiutato senza spiegazioni.
        let cifrario = Cifrario {
            versione: 1,
            cifre: vec![1, 2, 3],
        };
        for passo in 0..500_u64 {
            let Some(codice) = genera(&cifrario, passo.saturating_mul(30_000)) else {
                panic!("il codice si genera sempre");
            };
            assert_eq!(codice.len(), 6, "codice storto: {codice}");
            assert!(codice.bytes().all(|b| b.is_ascii_digit()));
        }
    }
}
