//! La chiave di questo dispositivo per il collegamento via internet.
//!
//! Sta nel portachiavi del sistema (Windows, o il Keystore di Android), voce
//! `lontano.chiave.<id del dispositivo>`, come quella di «Ascolta insieme»
//! (`insieme::fiducia`) e per le stesse ragioni: due cartelle dati hanno due
//! identità, e il database — che un backup copia — non vede mai il segreto.
//!
//! **Nessun ripiego.** Una voce che manca si crea, una volta; una che non si
//! legge è `lontano.identitaPersa`, e non si sovrascrive mai: una chiave nuova
//! al suo posto scollegherebbe in silenzio ogni dispositivo, che non
//! riconoscerebbe più questo.

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_lontano::Identita;
use rymembra_oauth::portachiavi::Portachiavi;
use zeroize::Zeroizing;

/// La voce del portachiavi.
#[must_use]
pub fn voce(dispositivo: &str) -> String {
    format!("lontano.chiave.{dispositivo}")
}

/// La chiave di questo dispositivo, dal portachiavi; creata e salvata se non
/// c'è.
///
/// # Errori
///
/// `lontano.identitaPersa`: vedi il `//!`. La causa dice quale strada è
/// mancata, mai la chiave.
pub fn identita(portachiavi: &dyn Portachiavi, dispositivo: &str) -> Result<Identita, AppError> {
    let voce = voce(dispositivo);
    let persa =
        |perche: &str| AppError::new(ErrorCode::LontanoIdentitaPersa).with_cause(perche.to_owned());
    match portachiavi.leggi(&voce) {
        Ok(Some(testo)) => {
            let testo = Zeroizing::new(testo);
            let byte = da_esadecimale(&testo)
                .ok_or_else(|| persa("la voce del portachiavi non è una chiave"))?;
            Ok(Identita::da_byte(&byte))
        }
        Ok(None) => {
            let nuova = Identita::nuova();
            let testo = in_esadecimale(&Zeroizing::new(nuova.byte()));
            portachiavi
                .scrivi(&voce, &testo)
                .map_err(|_| persa("il portachiavi non salva la chiave nuova"))?;
            Ok(nuova)
        }
        Err(_) => Err(persa("il portachiavi non risponde")),
    }
}

/// Trentadue byte in sessantaquattro cifre, che si azzerano quando cadono.
fn in_esadecimale(byte: &[u8; 32]) -> Zeroizing<String> {
    const CIFRE: &[u8; 16] = b"0123456789abcdef";
    let mut testo = Zeroizing::new(String::with_capacity(64));
    for b in byte {
        for mezzo in [b >> 4, b & 0x0f] {
            if let Some(&cifra) = CIFRE.get(usize::from(mezzo)) {
                testo.push(char::from(cifra));
            }
        }
    }
    testo
}

/// Il contrario: `None` per un testo che non è una chiave.
fn da_esadecimale(testo: &str) -> Option<Zeroizing<[u8; 32]>> {
    let testo = testo.trim().as_bytes();
    if testo.len() != 64 {
        return None;
    }
    let mut byte = Zeroizing::new([0_u8; 32]);
    // Sessantaquattro byte, quindi trentadue coppie e niente resto.
    let (coppie, _) = testo.as_chunks::<2>();
    for (posto, [alto, basso]) in byte.iter_mut().zip(coppie) {
        let alto = char::from(*alto).to_digit(16)?;
        let basso = char::from(*basso).to_digit(16)?;
        *posto = u8::try_from(alto * 16 + basso).ok()?;
    }
    Some(byte)
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Un portachiavi in memoria, che può far finta di non rispondere.
    #[derive(Default)]
    struct InMemoria {
        voci: Mutex<HashMap<String, String>>,
        muto: bool,
    }

    impl Portachiavi for InMemoria {
        fn leggi(&self, chiave: &str) -> Result<Option<String>, AppError> {
            if self.muto {
                return Err(AppError::new(ErrorCode::InternalUnexpected {
                    detail: None,
                }));
            }
            Ok(self.voci.lock().unwrap().get(chiave).cloned())
        }

        fn scrivi(&self, chiave: &str, valore: &str) -> Result<(), AppError> {
            if self.muto {
                return Err(AppError::new(ErrorCode::InternalUnexpected {
                    detail: None,
                }));
            }
            self.voci
                .lock()
                .unwrap()
                .insert(chiave.to_owned(), valore.to_owned());
            Ok(())
        }

        fn cancella(&self, chiave: &str) -> Result<(), AppError> {
            self.voci.lock().unwrap().remove(chiave);
            Ok(())
        }
    }

    #[test]
    fn la_chiave_si_crea_una_volta_e_poi_si_ritrova() {
        let portachiavi = InMemoria::default();
        let prima = identita(&portachiavi, "d-1").unwrap();
        let seconda = identita(&portachiavi, "d-1").unwrap();
        assert_eq!(prima.chiave(), seconda.chiave());
        let altra = identita(&portachiavi, "d-2").unwrap();
        assert_ne!(prima.chiave(), altra.chiave(), "una per cartella dati");
        let voce = portachiavi.voci.lock().unwrap()["lontano.chiave.d-1"].clone();
        assert_eq!(voce.len(), 64);
        assert!(
            !voce.contains(&prima.chiave()),
            "nella voce c'è il segreto, non la pubblica"
        );
    }

    #[test]
    fn una_voce_illeggibile_non_si_sovrascrive() {
        let portachiavi = InMemoria::default();
        portachiavi
            .voci
            .lock()
            .unwrap()
            .insert(voce("d-1"), "non una chiave".to_owned());
        let err = identita(&portachiavi, "d-1").unwrap_err();
        assert_eq!(err.code(), &ErrorCode::LontanoIdentitaPersa);
        assert_eq!(
            portachiavi.voci.lock().unwrap()[&voce("d-1")],
            "non una chiave",
            "nessun ripiego"
        );
        let muto = InMemoria {
            muto: true,
            ..InMemoria::default()
        };
        assert_eq!(
            identita(&muto, "d-1").unwrap_err().code(),
            &ErrorCode::LontanoIdentitaPersa
        );
    }

    #[test]
    fn esadecimale_avanti_e_indietro() {
        let byte: [u8; 32] = std::array::from_fn(|i| u8::try_from(i * 7).unwrap());
        let testo = in_esadecimale(&byte);
        assert_eq!(*da_esadecimale(&testo).unwrap(), byte);
        assert!(da_esadecimale(&testo[1..]).is_none());
        assert!(da_esadecimale(&"zz".repeat(32)).is_none());
    }
}
